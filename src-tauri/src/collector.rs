// 解析 ~/.claude/sessions/<pid>.json，将原始 JSON 转换为 Session 模型。
// 依赖 models + statemachine::decide 决定最终 Status。
use crate::liveness::is_claude_alive;
use crate::models::{FocusHint, Session, Source, Status};
use crate::statemachine::{decide, DecideInput};
use std::path::Path;

#[derive(Debug)]
pub enum ParseError {
    // 字段经 Debug 打印使用；dead_code 分析有意忽略 Debug derive，故标注。
    #[allow(dead_code)]
    BadJson(serde_json::Error),
}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        ParseError::BadJson(e)
    }
}

#[derive(serde::Deserialize)]
struct RawSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    name: Option<String>,
    status: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
    #[serde(rename = "statusUpdatedAt")]
    status_updated_at: Option<i64>,
}

pub fn parse_session_file(pid: u32, json: &str) -> Result<Session, ParseError> {
    let raw: RawSession = serde_json::from_str(json)?;
    let status_str = raw.status.as_deref().unwrap_or("");
    let status = decide(&DecideInput {
        raw_status: status_str,
        pending_permission: false,
    });
    let project = Path::new(&raw.cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok(Session {
        id: raw.session_id,
        source: Source::Interactive,
        pid,
        project,
        cwd: raw.cwd,
        name: raw.name.unwrap_or_default(),
        status,
        started_at: raw.started_at.unwrap_or(0),
        status_updated_at: raw.status_updated_at.unwrap_or(0),
        alive: true,
        focus_hint: FocusHint::default(),
        snoozed: false,
    })
}

/// 扫 ~/.claude/sessions/*.json，每个文件名是 pid；解析 + 校验存活。
/// 单文件解析失败隔离（skip），不拖垮整体。
/// 每 3s 调用一次：PermissionChecker::from_settings 在循环外创建，复用一次磁盘读 settings.json。
pub fn collect_sessions() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let dir = home.join(".claude/sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    // 循环外读一次 settings.json：单次 collect_sessions 内所有 session 复用同一份 permission 配置
    let pc = crate::permission::PermissionChecker::from_settings();
    // 循环外刷新一次 System：避免每个活 session 各触发一次全量 refresh（N=5 时 8-17% CPU 纯冗余）
    // with_exe(Always) 确保父进程链爬到的 p.exe() 可用
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_exe(UpdateKind::Always),
    );
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(pid_str) = name.strip_suffix(".json") else { continue };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        let Ok(json) = std::fs::read_to_string(&path) else { continue }; // fail fast: 跳过坏文件
        match parse_session_file(pid, &json) {
            Ok(mut s) => {
                s.alive = is_claude_alive(pid);
                if s.alive {
                    // name 优先用 JSONL 的 ai-title（Claude 生成的会话短标题 = /status 显示的命令名），
                    // fallback sessions.json 的 name（derived/任务名/sessionId 短码）。
                    if let Some(title) = read_ai_title(&s.id, &s.cwd) {
                        s.name = title;
                    }
                    // 末尾文本一次读出，供 pending tool_use + compact 检测共用（避免两次 seek）
                    // 真实权限判定：读 JSONL 末尾 pending tool_use + PermissionChecker 预测。
                    // 任一环节失败（无 settings / 无 JSONL / 无 pending）静默跳过，保留原 status。
                    // 死进程（mid-tool-call 退出）的 JSONL 末尾 tool_use 永远无 tool_result，
                    // 不应判定为 pending permission——仅活进程做此检查。
                    let tail_text = read_jsonl_tail_text(&s.id, &s.cwd);
                    let pending = tail_text.as_deref().and_then(parse_pending_from_str);
                    let is_compacting = tail_text.as_deref().map(detect_compacting).unwrap_or(false);
                    let needs_perm = matches!(&pc, Some(pc) if matches!(&pending, Some(p) if pc.needs_permission(&p.name, p.bash_command.as_deref())));
                    // 优先级：permission > compact > 原 status（parse_session_file 已得的 Working/Shell/Waiting）
                    // 注：compact 与 permission 实际互斥（compact 是阻塞操作），此处冗余 guard 保 safety net
                    if needs_perm {
                        s.status = Status::NeedsPermission;
                    } else if is_compacting {
                        s.status = Status::Compacting;
                    }
                    // host 探测仅对活进程有意义（死进程的父进程链可能已失效）
                    s.focus_hint.host = crate::discovery::detect_host_with_sys(&sys, pid);
                }
                out.push(s);
            }
            Err(_) => continue, // 隔离坏解析
        }
    }
    // 合并后台 fleet agent（roster.json），pid 存活校验
    for mut w in read_roster() {
        w.alive = is_claude_alive(w.pid);
        if w.alive {
            w.focus_hint.host = crate::discovery::detect_host_with_sys(&sys, w.pid);
        }
        out.push(w);
    }
    // agents --json 最后 push：reducer last-wins。
    // 只合并 Source::Fleet（background）条目——它们覆盖/补充 roster 默认 Working。
    // interactive 条目不合并：它们的 busy/idle 会覆盖 JSONL 精确状态
    // （NeedsPermission / Compacting），造成静默降级为 Working。
    out.extend(
        read_agents()
            .into_iter()
            .filter(|s| s.source == crate::models::Source::Fleet),
    );
    out
}

/// 从 JSONL 末尾解析出的未完成 tool_use（无对应 tool_result）。
pub struct PendingToolUse {
    pub name: String,
    pub bash_command: Option<String>,
}

#[derive(serde::Deserialize)]
struct JsonlRow {
    message: Option<JsonlMessage>,
}

#[derive(serde::Deserialize)]
struct JsonlMessage {
    content: Option<Vec<ContentItem>>,
}

#[derive(serde::Deserialize)]
struct ContentItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    tool_use_id: Option<String>,
}

/// 从 JSONL 文本解析最后一个未完成（无 tool_result）的 tool_use。纯函数，便于测试。
pub fn parse_pending_from_str(text: &str) -> Option<PendingToolUse> {
    let mut tool_uses: Vec<(String, String, Option<String>)> = Vec::new(); // (id, name, bash_cmd)
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<JsonlRow>(line) else { continue };
        let Some(msg) = row.message else { continue };
        let Some(items) = msg.content else { continue };
        for it in items {
            match it.item_type.as_deref() {
                Some("tool_use") => {
                    if let (Some(id), Some(name)) = (it.id.clone(), it.name.clone()) {
                        // 仅 Bash 工具抽取 command 字段
                        let bash_cmd = if name == "Bash" {
                            it.input
                                .as_ref()
                                .and_then(|v| v.get("command"))
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        };
                        tool_uses.push((id, name, bash_cmd));
                    }
                }
                Some("tool_result") => {
                    if let Some(tuid) = it.tool_use_id {
                        completed.insert(tuid);
                    }
                }
                _ => {}
            }
        }
    }
    // 从末尾找第一个未完成的 tool_use
    tool_uses
        .iter()
        .rev()
        .find(|(id, _, _)| !completed.contains(id))
        .map(|(_, name, cmd)| PendingToolUse {
            name: name.clone(),
            bash_command: cmd.clone(),
        })
}

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 末尾 ~8KB 文本。
/// pending tool_use 解析与 compact 检测共用同一份读取，避免双次 seek。
pub fn read_jsonl_tail_text(session_id: &str, cwd: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-"); // /Users/x -> -Users-x
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let mut f = std::fs::File::open(&path).ok()?;
    let size = f.metadata().ok()?.len();
    let start = size.saturating_sub(8192);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    // 若 seek 到非 0，首字节可能落在多字节 UTF-8 序列中间，跳到下一个 \n 边界
    let slice: &[u8] = if start > 0 {
        match bytes.iter().position(|&b| b == b'\n') {
            Some(idx) => &bytes[idx + 1..],
            None => return None,
        }
    } else {
        &bytes[..]
    };
    let text = std::str::from_utf8(slice).ok()?;
    Some(text.to_string())
}

/// 解析 JSONL 文本：找最后一条 `type=ai-title` 的 `aiTitle`（Claude 生成的会话短标题，
/// 即 /status 显示的命令名）。ai-title 可能在会话早期生成且稳定，故取最后一条兜底最新。
/// 无则 None。
pub fn parse_ai_title(text: &str) -> Option<String> {
    let mut last: Option<String> = None;
    for line in text.lines() {
        if !line.contains("\"type\":\"ai-title\"") {
            continue;
        }
        if let Ok(d) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(t) = d.get("aiTitle").and_then(|v| v.as_str()) {
                last = Some(t.to_string());
            }
        }
    }
    last
}

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 全文，取最后一条 ai-title。
/// 读全文（非尾部）：ai-title 可能在会话早期生成，尾部采样会漏。
/// 失败（无文件/无 ai-title）返回 None，调用方 fallback sessions.json 的 name。
pub fn read_ai_title(session_id: &str, cwd: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let text = std::fs::read_to_string(&path).ok()?;
    parse_ai_title(&text)
}

/// 扫 JSONL 末尾文本：若任行含 compact_boundary 标志 → true。
/// 实测格式：`{"type":"system","subtype":"compact_boundary",...}` —— 用 `"subtype":"compact_boundary"`
/// 而非裸 `compact_boundary` 收紧匹配，避免对话中提到此词（如本仓库自身 JSONL）造成误报。
/// 语义注意：compact 是阻塞操作，boundary 行在 compact **结束时**写入——此判据实际捕捉
/// "post-compact 窗口"（刚 compact 完、agent 尚未 resume）；真正的 in-progress 期间 JSONL 无写入，
/// 无法从 JSONL 检测。
pub fn detect_compacting(text: &str) -> bool {
    text.contains("\"subtype\":\"compact_boundary\"")
}

// ---- roster.json 解析 ----
// Claude Code daemon 的 fleet/slash worker 注册表。每个 worker → 一个 Session。

#[derive(serde::Deserialize)]
struct RosterFile {
    #[serde(default)]
    workers: std::collections::HashMap<String, RosterWorker>,
}
#[derive(serde::Deserialize)]
struct RosterWorker {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
    dispatch: Option<RosterDispatch>,
}
#[derive(serde::Deserialize)]
struct RosterDispatch {
    source: Option<String>,
    #[serde(default)]
    seed: RosterSeed,
}
#[derive(Default, serde::Deserialize)]
struct RosterSeed {
    intent: String,
}

/// 解析 roster.json，每个 worker → Session（source 由 dispatch.source 决定，status 默认 Working）。
/// 解析失败返回空 vec，不崩溃（fail fast：坏数据不拖垮 collect）。
pub fn parse_roster(json: &str) -> Vec<Session> {
    let Ok(f) = serde_json::from_str::<RosterFile>(json) else {
        eprintln!("parse_roster: invalid roster json, skipping");
        return vec![]
    };
    f.workers.into_values().map(|w| {
        let source = match w.dispatch.as_ref().and_then(|d| d.source.as_deref()) {
            Some("slash") => Source::Slash,
            _ => Source::Fleet,
        };
        let name = w
            .dispatch
            .as_ref()
            .map(|d| d.seed.intent.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| w.session_id.chars().take(8).collect());
        let project = std::path::Path::new(&w.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        Session {
            id: w.session_id,
            source,
            pid: w.pid,
            project,
            cwd: w.cwd,
            name,
            status: Status::Working,
            started_at: w.started_at.unwrap_or(0),
            status_updated_at: w.started_at.unwrap_or(0),
            alive: true, // collect_sessions 会用 pid 校验覆盖
            focus_hint: FocusHint::default(),
            snoozed: false,
        }
    }).collect()
}

/// 读 ~/.claude/daemon/roster.json。
pub fn read_roster() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let path = home.join(".claude/daemon/roster.json");
    let Ok(json) = std::fs::read_to_string(&path) else { return vec![] };
    parse_roster(&json)
}

// ---- claude agents --json 解析 ----
// CC≥2.1.145 官方命令，输出当前所有 agent（interactive + background）的实时状态。
// 用其准确 status 覆盖 roster 默认 Working。agents 最后 push → reducer last-wins。

/// 实测 schema（CC 2.1.201）：
/// ```json
/// [
///   {
///     "id": "04bb90a4",            // sessionId 的 8 字符短前缀
///     "pid": 63586,                // background blocked agent 可能缺
///     "cwd": "/Users/x/proj",
///     "kind": "interactive",       // "background" | "interactive"
///     "startedAt": 1784706530163,  // epoch 毫秒
///     "sessionId": "04bb90a4-...", // 完整 UUID
///     "name": "proj-ab",
///     "status": "busy",            // interactive: "busy" | "idle"
///     "state": "blocked"           // background: "blocked"
///   }
/// ]
/// ```
#[derive(serde::Deserialize)]
struct AgentsEntry {
    #[serde(default)]
    pid: Option<u32>,
    cwd: String,
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: String,
    name: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
    status: Option<String>,
    state: Option<String>,
}

/// 解析 `claude agents --json` 的 stdout。
/// status 映射：busy→Working，idle→WaitingForInput，state:blocked→NeedsPermission。
/// 解析失败返回空 vec（fail fast：坏数据不拖垮 collect）。
pub fn parse_agents(json: &str) -> Vec<Session> {
    let Ok(entries) = serde_json::from_str::<Vec<AgentsEntry>>(json) else {
        eprintln!("parse_agents: invalid agents json, skipping");
        return vec![];
    };
    entries.into_iter().map(|a| {
        // interactive: status busy|idle；background: state blocked
        let status = if let Some(st) = a.status.as_deref() {
            match st {
                "busy" => Status::Working,
                "idle" => Status::WaitingForInput,
                _ => Status::Working,
            }
        } else if let Some(state) = a.state.as_deref() {
            match state {
                "blocked" => Status::NeedsPermission,
                _ => Status::Working,
            }
        } else {
            Status::Working
        };
        let source = match a.kind.as_deref() {
            Some("background") => Source::Fleet,
            _ => Source::Interactive,
        };
        let project = Path::new(&a.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let name = a.name.unwrap_or_else(|| a.session_id.chars().take(8).collect());
        Session {
            id: a.session_id,
            source,
            pid: a.pid.unwrap_or(0),
            project,
            cwd: a.cwd,
            name,
            status,
            started_at: a.started_at.unwrap_or(0),
            status_updated_at: a.started_at.unwrap_or(0),
            alive: true, // agents --json 只列活进程
            focus_hint: FocusHint::default(),
            snoozed: false,
        }
    }).collect()
}

/// 跑 `claude agents --json`，10s 超时兜底。
/// 命令失败/超时/无输出返回空 vec（fail fast：不崩 collect）。
pub fn read_agents() -> Vec<Session> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = match Command::new("claude")
        .args(["agents", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("read_agents: failed to spawn claude: {}", e);
            return vec![];
        }
    };

    // 10s 超时兜底：polling try_wait，到期 kill + reap 防 zombie
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait(); // reap zombie
                eprintln!("read_agents: claude agents --json timed out after 10s");
                return vec![];
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                eprintln!("read_agents: wait error: {}", e);
                return vec![];
            }
        }
    }

    // 子进程已退出，从 pipe 读 stdout
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if stdout.trim().is_empty() {
        return vec![];
    }
    parse_agents(&stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Status;
    use std::fs;

    #[test]
    fn parses_busy_session() {
        let json = fs::read_to_string("tests/fixtures/session-busy.json").unwrap();
        let s = parse_session_file(27074, &json).unwrap();
        assert_eq!(s.id, "736f6944-db6d-4327-b4f3-a87154de33ec");
        assert_eq!(s.status, Status::Working);
        assert_eq!(s.project, "cc-view");
        assert_eq!(s.name, "cc-view-94");
    }

    #[test]
    fn parse_ai_title_finds_last() {
        let text = "{\"type\":\"ai-title\",\"aiTitle\":\"旧标题\",\"sessionId\":\"x\"}\n\
{\"type\":\"user\",\"message\":\"hi\"}\n\
{\"type\":\"ai-title\",\"aiTitle\":\"新标题\",\"sessionId\":\"x\"}";
        assert_eq!(parse_ai_title(text).as_deref(), Some("新标题"));
    }

    #[test]
    fn parse_ai_title_none_when_absent() {
        assert_eq!(parse_ai_title("{\"type\":\"user\",\"message\":\"hi\"}"), None);
    }

    #[test]
    fn missing_status_defaults_to_waiting() {
        let json = r#"{"sessionId":"s","cwd":"/x/y"}"#;
        let s = parse_session_file(1, json).unwrap();
        assert_eq!(s.status, Status::WaitingForInput);
        assert_eq!(s.project, "y");
    }

    #[test]
    fn collect_sessions_runs_against_real_dir() {
        // 集成测试：调用不 panic、返回 Vec（可能为空若本机无运行会话）
        let sessions = super::collect_sessions();
        // 当前 cc-view 自身会话通常存在；只校验结构不校验数量
        for s in &sessions {
            assert!(!s.id.is_empty());
        }
    }

    #[test]
    fn pending_tool_use_detected() {
        // 用 fixture 内容直接喂解析函数（不读磁盘，测纯解析）
        let jsonl = std::fs::read_to_string("tests/fixtures/pending.jsonl").unwrap();
        let p = super::parse_pending_from_str(&jsonl).unwrap();
        assert_eq!(p.name, "Bash");
        assert_eq!(p.bash_command.as_deref(), Some("kill 1"));
    }

    #[test]
    fn no_pending_when_completed() {
        let jsonl = std::fs::read_to_string("tests/fixtures/completed.jsonl").unwrap();
        assert!(super::parse_pending_from_str(&jsonl).is_none());
    }

    #[test]
    fn parses_roster_workers() {
        let json = std::fs::read_to_string("tests/fixtures/roster.json").unwrap();
        let v = super::parse_roster(&json);
        assert_eq!(v.len(), 1);
        let s = &v[0];
        assert_eq!(s.id, "f0d42050-7b39-46e3-996a-1c5829f55ffe");
        assert_eq!(s.source, crate::models::Source::Fleet);
        assert_eq!(s.pid, 1958);
        assert_eq!(s.project, "ai");
        assert_eq!(s.status, crate::models::Status::Working); // roster 无 status，默认 Working
    }

    #[test]
    fn detect_compacting_finds_boundary() {
        // fixture 末尾含 compact_boundary 行 → true
        let jsonl = std::fs::read_to_string("tests/fixtures/compacting.jsonl").unwrap();
        assert!(super::detect_compacting(&jsonl));
    }

    #[test]
    fn detect_compacting_misses_when_absent() {
        // 普通 pending.jsonl 无 compact_boundary → false
        let jsonl = std::fs::read_to_string("tests/fixtures/pending.jsonl").unwrap();
        assert!(!super::detect_compacting(&jsonl));
    }

    #[test]
    fn detect_compacting_boundary_inline() {
        // 真实 JSONL 行结构 → 命中
        assert!(super::detect_compacting(r#"{"type":"system","subtype":"compact_boundary"}"#));
        // 裸字符串 "compact_boundary"（对话中提到此词）不命中——收紧避免误报
        assert!(!super::detect_compacting(r#"{"type":"user","message":"compact_boundary is a marker"}"#));
        assert!(!super::detect_compacting(r#"{"type":"user","message":"hello"}"#));
    }

    // ---- parse_agents 测试（基于实测 claude agents --json schema） ----

    #[test]
    fn parse_agents_maps_busy_idle_blocked() {
        let json = std::fs::read_to_string("tests/fixtures/agents.json").unwrap();
        let v = super::parse_agents(&json);
        assert_eq!(v.len(), 3);

        // interactive busy → Working
        let busy = v.iter().find(|s| s.id == "04bb90a4-c80d-47ef-98ea-c040df5da3d7").unwrap();
        assert_eq!(busy.status, Status::Working);
        assert_eq!(busy.source, crate::models::Source::Interactive);
        assert_eq!(busy.pid, 63586);
        assert_eq!(busy.project, "fang");
        assert_eq!(busy.name, "fang-dd");
        assert!(busy.alive);

        // interactive idle → WaitingForInput
        let idle = v.iter().find(|s| s.id == "21eb6f0e-8687-479e-9e04-6599d98bce43").unwrap();
        assert_eq!(idle.status, Status::WaitingForInput);
        assert_eq!(idle.project, "cc-job");

        // background blocked → NeedsPermission，pid 缺省为 0
        let blocked = v.iter().find(|s| s.id == "13ce4523-bd41-49b7-8b90-8f8d739d62b6").unwrap();
        assert_eq!(blocked.status, Status::NeedsPermission);
        assert_eq!(blocked.source, crate::models::Source::Fleet);
        assert_eq!(blocked.pid, 0); // background blocked agent 无 pid
        assert_eq!(blocked.project, "agu");
    }

    #[test]
    fn parse_agents_bad_json_returns_empty() {
        assert!(super::parse_agents("not json").is_empty());
        assert!(super::parse_agents("{}").is_empty()); // 不是数组
    }

    #[test]
    fn parse_agents_empty_array() {
        assert!(super::parse_agents("[]").is_empty());
    }

    #[test]
    fn parse_agents_missing_status_defaults_working() {
        // kind=interactive 但无 status 也无 state → 默认 Working
        let json = r#"[{"sessionId":"s1","cwd":"/x/p","kind":"interactive"}]"#;
        let v = super::parse_agents(json);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].status, Status::Working);
    }

    #[test]
    fn agents_fleet_overrides_roster_in_reducer() {
        // 验证合并顺序：roster(Working) → agents(Fleet, blocked)，reducer last-wins → NeedsPermission。
        // 仅 Source::Fleet 条目会进入 collect_sessions 输出（interactive 被 filter 掉，
        // 避免覆盖 JSONL 精确状态）。background 真实 schema 用 state 而非 status。
        use crate::models::{FocusHint, Source};
        let roster_session = Session {
            id: "abc123".into(), source: Source::Fleet, pid: 100,
            project: "p".into(), cwd: "/c".into(), name: "test".into(),
            status: Status::Working, started_at: 0, status_updated_at: 0,
            alive: true, focus_hint: FocusHint::default(),
            snoozed: false,
        };
        let agents_json = r#"[{"sessionId":"abc123","cwd":"/c","kind":"background","state":"blocked"}]"#;
        let agents_sessions: Vec<Session> = super::parse_agents(agents_json)
            .into_iter()
            .filter(|s| s.source == Source::Fleet)
            .collect();
        // 模拟 collect_sessions 的 push 顺序
        let merged = crate::reducer::reduce({
            let mut v = vec![roster_session];
            v.extend(agents_sessions);
            v
        });
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].status, Status::NeedsPermission); // Fleet agents 覆盖了 roster 的 Working
    }
}
