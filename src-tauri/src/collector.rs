// 解析 ~/.claude/sessions/<pid>.json，将原始 JSON 转换为 Session 模型。
// 依赖 models + statemachine::decide 决定最终 Status。
use crate::models::{FocusHint, Session, Source, Status};
use crate::statemachine::{decide, DecideInput};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

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
        tokens_in: 0,
        tokens_out: 0,
    })
}

/// 扫 ~/.claude/sessions/*.json，每个文件名是 pid；解析 + 校验存活。
/// 单文件解析失败隔离（skip），不拖垮整体。
/// 每 3s 调用一次：权限配置按 session cwd 在循环内读三层 settings（user+project+local）。
pub fn collect_sessions() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let dir = home.join(".claude/sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    // 循环外刷新一次 System：避免每个活 session 各触发一次全量 refresh（N=5 时 8-17% CPU 纯冗余）
    // with_exe(Always) 确保父进程链爬到的 p.exe() 可用
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_exe(UpdateKind::Always),
    );
    // host 探测用 ps 全表（一次 ps -ax），避免每会话每层 spawn ps（review P1-1）。
    let ps_table = crate::discovery::read_ps_table();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(pid_str) = name.strip_suffix(".json") else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let Ok(json) = std::fs::read_to_string(&path) else {
            continue;
        }; // fail fast: 跳过坏文件
        match parse_session_file(pid, &json) {
            Ok(mut s) => {
                s.alive = crate::liveness::is_claude_alive_sys(&sys, pid);
                if s.alive {
                    // /status Session name 逻辑（参考 claude-hud transcript.ts）：JSONL
                    // custom-title（/rename 用户改名）优先，否则 ai-title（Claude 生成）。
                    // 无两者则保留 sessions.json 的 name（derived/sessionId 短码）。
                    // 实测：cc-view-45(无custom,ai=了解...)→了解...；述职(custom=述职)→述职。
                    if let Some(scan) = scan_session_jsonl_cached(&s.id, &s.cwd) {
                        if let Some(t) = scan.title {
                            s.name = t;
                        }
                        s.tokens_in = scan.tokens_in;
                        s.tokens_out = scan.tokens_out;
                    }
                    // 末尾文本一次读出，供 pending tool_use + compact 检测共用（避免两次 seek）
                    // 真实权限判定：读 JSONL 末尾 pending tool_use + PermissionChecker 预测。
                    // 任一环节失败（无 settings / 无 JSONL / 无 pending）静默跳过，保留原 status。
                    // 死进程（mid-tool-call 退出）的 JSONL 末尾 tool_use 永远无 tool_result，
                    // 不应判定为 pending permission——仅活进程做此检查。
                    let tail_text = read_jsonl_tail_text(&s.id, &s.cwd);
                    let pending = tail_text.as_deref().and_then(parse_pending_from_str);
                    let is_compacting =
                        tail_text.as_deref().map(detect_compacting).unwrap_or(false);
                    // 权限配置按 session cwd 读三层（user + project + local）：
                    // 不同项目 local allow 不同（如 Skill(gstack) 仅 life-planner 有）
                    let pc = crate::permission::PermissionChecker::from_settings_for_cwd(Some(
                        Path::new(&s.cwd),
                    ));
                    let needs_perm = matches!(&pc, Some(pc) if matches!(&pending, Some(p) if pc.needs_permission(&p.name, p.bash_command.as_deref())));
                    // 优先级：permission > compact > 原 status（parse_session_file 已得的 Working/Shell/Waiting）
                    // 注：compact 与 permission 实际互斥（compact 是阻塞操作），此处冗余 guard 保 safety net
                    if needs_perm {
                        s.status = Status::NeedsPermission;
                    } else if is_compacting {
                        s.status = Status::Compacting;
                    }
                    // host 探测仅对活进程有意义（死进程的父进程链可能已失效）
                    s.focus_hint.host = crate::discovery::detect_host_via_table(pid, &ps_table);
                }
                out.push(s);
            }
            Err(_) => continue, // 隔离坏解析
        }
    }
    // 合并后台 fleet agent（roster.json），pid 存活校验
    for mut w in read_roster() {
        w.alive = crate::liveness::is_claude_alive_sys(&sys, w.pid);
        if w.alive {
            fill_tokens(&mut w);
            w.focus_hint.host = crate::discovery::detect_host_via_table(w.pid, &ps_table);
        }
        out.push(w);
    }
    // agents --json 最后 push：reducer last-wins。
    // 只合并 Source::Fleet（background）条目——它们覆盖/补充 roster 默认 Working。
    // interactive 条目不合并：它们的 busy/idle 会覆盖 JSONL 精确状态
    // （NeedsPermission / Compacting），造成静默降级为 Working。
    let mut agents: Vec<Session> = read_agents()
        .into_iter()
        .filter(|s| s.source == crate::models::Source::Fleet)
        .collect();
    for s in agents.iter_mut().filter(|s| s.alive) {
        fill_tokens(s);
    }
    out.extend(agents);
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
        let Ok(row) = serde_json::from_str::<JsonlRow>(line) else {
            continue;
        };
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

/// 解析 JSONL 文本，取会话标题：最后一条 `custom-title`（/rename 用户改名）优先，
/// 否则最后一条 `ai-title`（Claude 生成）。与 Claude Code /status 同源
/// （参考 claude-hud transcript.ts: customTitle ?? aiTitle）。无则 None。
#[cfg(test)]
pub fn parse_session_title(text: &str) -> Option<String> {
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    for line in text.lines() {
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match d.get("type").and_then(|v| v.as_str()) {
            Some("custom-title") => {
                if let Some(t) = d.get("customTitle").and_then(|v| v.as_str()) {
                    custom = Some(t.to_string());
                }
            }
            Some("ai-title") => {
                if let Some(t) = d.get("aiTitle").and_then(|v| v.as_str()) {
                    ai = Some(t.to_string());
                }
            }
            _ => {}
        }
    }
    custom.or(ai)
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
        log::warn!("parse_roster: invalid roster json, skipping");
        return vec![];
    };
    f.workers
        .into_values()
        .map(|w| {
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
                tokens_in: 0,
                tokens_out: 0,
            }
        })
        .collect()
}

/// 读 ~/.claude/daemon/roster.json。
pub fn read_roster() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let path = home.join(".claude/daemon/roster.json");
    let Ok(json) = std::fs::read_to_string(&path) else {
        return vec![];
    };
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
        log::warn!("parse_agents: invalid agents json, skipping");
        return vec![];
    };
    entries
        .into_iter()
        .map(|a| {
            // interactive: status busy|idle；background: state blocked
            let status = if let Some(st) = a.status.as_deref() {
                match st {
                    "busy" => Status::Working,
                    "idle" => Status::WaitingForInput,
                    "waiting" => Status::WaitingForReply,
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
            let name = a
                .name
                .unwrap_or_else(|| a.session_id.chars().take(8).collect());
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
                tokens_in: 0,
                tokens_out: 0,
            }
        })
        .collect()
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
            log::warn!("read_agents: failed to spawn claude: {}", e);
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
                log::warn!("read_agents: claude agents --json timed out after 10s");
                return vec![];
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                log::warn!("read_agents: wait error: {}", e);
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

// ---- 全文 scan：标题 + 累计 token 一遍遍历（替代 read_session_title 单一职责）----
// 零额外 IO：取标题本来就要读全文，顺带累加 assistant usage。
#[derive(serde::Deserialize)]
struct ScanRow {
    #[serde(rename = "type")]
    row_type: Option<String>,
    message: Option<ScanMessage>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
}
#[derive(serde::Deserialize)]
struct ScanMessage {
    #[serde(default)]
    usage: Option<ScanUsage>,
}
#[derive(serde::Deserialize)]
struct ScanUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// 一遍遍历的产出：标题（custom ?? ai）+ 累计 input/output token。
pub struct SessionScan {
    pub title: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// 纯函数：从 JSONL 全文文本一遍遍历，取标题 + 累计 token。便于单测。
pub fn scan_session_jsonl_from_text(text: &str) -> SessionScan {
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    let mut tokens_in: u64 = 0;
    let mut tokens_out: u64 = 0;
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<ScanRow>(line) else {
            continue;
        };
        match row.row_type.as_deref() {
            Some("custom-title") => custom = row.custom_title,
            Some("ai-title") => ai = row.ai_title,
            Some("assistant") => {
                if let Some(u) = row.message.as_ref().and_then(|m| m.usage.as_ref()) {
                    tokens_in += u.input_tokens;
                    tokens_out += u.output_tokens;
                }
            }
            _ => {}
        }
    }
    SessionScan {
        title: custom.or(ai),
        tokens_in,
        tokens_out,
    }
}

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 全文，一遍出标题 + token。
pub fn scan_session_jsonl(session_id: &str, cwd: &str) -> Option<SessionScan> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let text = std::fs::read_to_string(&path).ok()?;
    Some(scan_session_jsonl_from_text(&text))
}

/// JSONL token 缓存：session_id -> (mtime, size, tokens_in, tokens_out, title)。
/// 每 3s 全文扫描的优化——文件 mtime+size 未变则复用上次结果，变了才重算。
/// 进程内缓存，重启清空（下一轮 3s 重算，无妨）。
static JSONL_CACHE: LazyLock<Mutex<HashMap<String, (SystemTime, u64, u64, u64, Option<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 带 mtime+size 缓存的 scan：文件未变则复用上次结果，避免每 3s 全文重扫。
/// 失效条件：modified time 或 size 任一变化（追加写必然改 size）。
pub fn scan_session_jsonl_cached(session_id: &str, cwd: &str) -> Option<SessionScan> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let meta = std::fs::metadata(&path).ok()?;
    let mtime = meta.modified().ok()?;
    let size = meta.len();
    // 命中：mtime + size 都未变 → 复用缓存
    if let Ok(cache) = JSONL_CACHE.lock() {
        if let Some((c_mtime, c_size, t_in, t_out, title)) = cache.get(session_id) {
            if *c_mtime == mtime && *c_size == size {
                return Some(SessionScan {
                    title: title.clone(),
                    tokens_in: *t_in,
                    tokens_out: *t_out,
                });
            }
        }
    }
    // 未命中：全文重算并写回缓存
    let scan = scan_session_jsonl(session_id, cwd)?;
    if let Ok(mut cache) = JSONL_CACHE.lock() {
        cache.insert(
            session_id.to_string(),
            (
                mtime,
                size,
                scan.tokens_in,
                scan.tokens_out,
                scan.title.clone(),
            ),
        );
    }
    Some(scan)
}

/// 对单个活会话填充累计 token（不碰 name）。roster/agents 共用。
fn fill_tokens(s: &mut Session) {
    if let Some(scan) = scan_session_jsonl_cached(&s.id, &s.cwd) {
        s.tokens_in = scan.tokens_in;
        s.tokens_out = scan.tokens_out;
    }
}

// ---- 详情扫描：回合分组 + 汇总（on-demand，无缓存）----
#[derive(serde::Deserialize)]
struct DetailRow {
    #[serde(rename = "type")]
    row_type: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    message: Option<DetailMessage>,
    timestamp: Option<String>,
}
#[derive(serde::Deserialize)]
struct DetailMessage {
    #[serde(default)]
    model: Option<String>,
    // content 可能是 string 或 array，用 Value 统一处理
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    usage: Option<DetailUsage>,
}
#[derive(serde::Deserialize)]
struct DetailUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    server_tool_use: Option<DetailServerToolUse>,
}
#[derive(serde::Deserialize)]
struct DetailServerToolUse {
    #[serde(default)]
    web_search_requests: u64,
    #[serde(default)]
    web_fetch_requests: u64,
}

/// 从 user message content 提取真实用户输入文本。
/// string → 整段；array → 取首个 type=="text" 的 text；纯 tool_result → None（不开回合）。
fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

/// 统计 assistant content 中 tool_use block 数。
fn count_tool_use(content: &serde_json::Value) -> u32 {
    content
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                .count() as u32
        })
        .unwrap_or(0)
}

/// 纯函数：一遍遍历 JSONL 文本，按回合分组 + 累计汇总。session_id 留空，由调用方填。
/// 回合定义：真实用户输入（string 或含 text block）开新回合；tool_result 不开回合。
pub fn scan_detail_from_text(text: &str) -> crate::models::SessionDetail {
    use crate::models::{SessionDetail, TurnStat};
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut cache_read = 0u64;
    let mut cache_creation = 0u64;
    let mut model = String::new();
    let mut tool_calls = 0u32;
    let mut web_searches = 0u64;
    let mut web_fetches = 0u64;
    let mut context_peak = 0u64;
    let mut last_ctx = 0u64;
    let mut prev_valid_ctx = 0u64;
    let mut compact_count = 0u32;
    let mut turns: Vec<TurnStat> = Vec::new();
    let mut cur: Option<TurnStat> = None;
    let mut turn_idx = 0u32;

    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<DetailRow>(line) else {
            continue;
        };
        match row.row_type.as_deref() {
            Some("user") => {
                let Some(msg) = row.message.as_ref() else {
                    continue;
                };
                let Some(content) = msg.content.as_ref() else {
                    continue;
                };
                if let Some(prompt) = extract_user_text(content) {
                    // 真实输入 → 结束当前回合、开新回合
                    if let Some(t) = cur.take() {
                        turns.push(t);
                    }
                    turn_idx += 1;
                    cur = Some(TurnStat {
                        idx: turn_idx,
                        prompt: prompt.chars().take(40).collect(),
                        tokens_in: 0,
                        tokens_out: 0,
                        tool_calls: 0,
                        ctx: prev_valid_ctx,
                        ts: row.timestamp.clone().unwrap_or_default(),
                    });
                }
                // tool_result：不开回合，忽略
            }
            Some("assistant") => {
                let Some(msg) = row.message.as_ref() else {
                    continue;
                };
                if let Some(u) = msg.usage.as_ref() {
                    tokens_in += u.input_tokens;
                    tokens_out += u.output_tokens;
                    cache_read += u.cache_read_input_tokens;
                    cache_creation += u.cache_creation_input_tokens;
                    if let Some(stu) = u.server_tool_use.as_ref() {
                        web_searches += stu.web_search_requests;
                        web_fetches += stu.web_fetch_requests;
                    }
                    // 上下文占用 = 该请求送入模型的总 token（input + 缓存读 + 缓存写）
                    let ctx =
                        u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens;
                    // usage 全 0 的异常 assistant（如 stop_sequence 提前终止）不计入 ctx，
                    // 否则 sparkline 掉底、compact 误判。
                    if ctx > 0 {
                        // compact 启发式：相邻有效 ctx 大幅下降（降 30%+）视为一次压缩。
                        // cc 新版 compact 不写 compact_boundary 标记，只能从 ctx 跳降推断。
                        if prev_valid_ctx > 0 && ctx * 10 < prev_valid_ctx * 7 {
                            compact_count += 1;
                        }
                        if ctx > context_peak {
                            context_peak = ctx;
                        }
                        last_ctx = ctx;
                        prev_valid_ctx = ctx;
                        if let Some(t) = cur.as_mut() {
                            t.ctx = ctx;
                        }
                    }
                }
                if let Some(m) = msg.model.as_ref().filter(|m| !m.is_empty()) {
                    model = m.clone();
                }
                let tc = msg.content.as_ref().map(count_tool_use).unwrap_or(0);
                tool_calls += tc;
                if let Some(t) = cur.as_mut() {
                    if let Some(u) = msg.usage.as_ref() {
                        t.tokens_in += u.input_tokens;
                        t.tokens_out += u.output_tokens;
                    }
                    t.tool_calls += tc;
                }
            }
            Some("system") => {
                if row.subtype.as_deref() == Some("compact_boundary") {
                    compact_count += 1;
                }
            }
            _ => {}
        }
    }
    if let Some(t) = cur.take() {
        turns.push(t);
    }
    let turn_count = turns.len() as u32;
    SessionDetail {
        session_id: String::new(),
        tokens_in,
        tokens_out,
        cache_read,
        cache_creation,
        model,
        turn_count,
        tool_calls,
        web_searches: web_searches as u32,
        web_fetches: web_fetches as u32,
        context_current: last_ctx,
        context_peak,
        compact_count,
        turns,
    }
}

/// 读 JSONL 全文做详情扫描。
pub fn scan_session_detail(session_id: &str, cwd: &str) -> Option<crate::models::SessionDetail> {
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-");
    let path = home
        .join(".claude/projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));
    let text = std::fs::read_to_string(&path).ok()?;
    let mut d = scan_detail_from_text(&text);
    d.session_id = session_id.to_string();
    Some(d)
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
    fn parse_session_title_custom_over_ai() {
        // custom-title（用户 rename）优先于 ai-title（Claude 生成）
        let text = "{\"type\":\"ai-title\",\"aiTitle\":\"AI生成标题\",\"sessionId\":\"x\"}\n\
{\"type\":\"custom-title\",\"customTitle\":\"用户改名\",\"slug\":\"abc\"}";
        assert_eq!(parse_session_title(text).as_deref(), Some("用户改名"));
    }

    #[test]
    fn parse_session_title_ai_when_no_custom() {
        let text = "{\"type\":\"ai-title\",\"aiTitle\":\"AI生成标题\",\"sessionId\":\"x\"}";
        assert_eq!(parse_session_title(text).as_deref(), Some("AI生成标题"));
    }

    #[test]
    fn parse_session_title_none_when_absent() {
        assert_eq!(
            parse_session_title("{\"type\":\"user\",\"message\":\"hi\"}"),
            None
        );
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
        assert!(super::detect_compacting(
            r#"{"type":"system","subtype":"compact_boundary"}"#
        ));
        // 裸字符串 "compact_boundary"（对话中提到此词）不命中——收紧避免误报
        assert!(!super::detect_compacting(
            r#"{"type":"user","message":"compact_boundary is a marker"}"#
        ));
        assert!(!super::detect_compacting(
            r#"{"type":"user","message":"hello"}"#
        ));
    }

    // ---- parse_agents 测试（基于实测 claude agents --json schema） ----

    #[test]
    fn parse_agents_maps_busy_idle_blocked() {
        let json = std::fs::read_to_string("tests/fixtures/agents.json").unwrap();
        let v = super::parse_agents(&json);
        assert_eq!(v.len(), 3);

        // interactive busy → Working
        let busy = v
            .iter()
            .find(|s| s.id == "04bb90a4-c80d-47ef-98ea-c040df5da3d7")
            .unwrap();
        assert_eq!(busy.status, Status::Working);
        assert_eq!(busy.source, crate::models::Source::Interactive);
        assert_eq!(busy.pid, 63586);
        assert_eq!(busy.project, "fang");
        assert_eq!(busy.name, "fang-dd");
        assert!(busy.alive);

        // interactive idle → WaitingForInput
        let idle = v
            .iter()
            .find(|s| s.id == "21eb6f0e-8687-479e-9e04-6599d98bce43")
            .unwrap();
        assert_eq!(idle.status, Status::WaitingForInput);
        assert_eq!(idle.project, "cc-job");

        // background blocked → NeedsPermission，pid 缺省为 0
        let blocked = v
            .iter()
            .find(|s| s.id == "13ce4523-bd41-49b7-8b90-8f8d739d62b6")
            .unwrap();
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
            id: "abc123".into(),
            source: Source::Fleet,
            pid: 100,
            project: "p".into(),
            cwd: "/c".into(),
            name: "test".into(),
            status: Status::Working,
            started_at: 0,
            status_updated_at: 0,
            alive: true,
            focus_hint: FocusHint::default(),
            snoozed: false,
            tokens_in: 0,
            tokens_out: 0,
        };
        let agents_json =
            r#"[{"sessionId":"abc123","cwd":"/c","kind":"background","state":"blocked"}]"#;
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

    #[test]
    fn scan_totals_accumulate_assistant_usage() {
        let text = "\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":200,\"output_tokens\":80}}}";
        let scan = super::scan_session_jsonl_from_text(text);
        assert_eq!(scan.tokens_in, 300);
        assert_eq!(scan.tokens_out, 130);
        assert_eq!(scan.title, None);
    }

    #[test]
    fn scan_picks_custom_title_over_ai_with_tokens() {
        let text = "\
{\"type\":\"ai-title\",\"aiTitle\":\"AI标题\"}
{\"type\":\"custom-title\",\"customTitle\":\"用户改名\"}
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}";
        let scan = super::scan_session_jsonl_from_text(text);
        assert_eq!(scan.title.as_deref(), Some("用户改名"));
        assert_eq!(scan.tokens_in, 10);
        assert_eq!(scan.tokens_out, 5);
    }

    #[test]
    fn scan_detail_groups_turns_and_accumulates() {
        let text = "\
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"第一问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\"}],\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":500,\"output_tokens\":40}}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:06.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"x\",\"content\":\"ok\"}]}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"答1\"}],\"usage\":{\"input_tokens\":50,\"cache_read_input_tokens\":800,\"output_tokens\":20}}}
{\"type\":\"system\",\"timestamp\":\"2026-08-07T01:00:30.000Z\",\"subtype\":\"compact_boundary\"}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:01:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"第二问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:01:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"答2\"}],\"usage\":{\"input_tokens\":30,\"cache_read_input_tokens\":1200,\"output_tokens\":10}}}";
        let d = super::scan_detail_from_text(text);
        assert_eq!(d.turn_count, 2); // tool_result 不开回合
        assert_eq!(d.tokens_in, 180); // 100+50+30
        assert_eq!(d.tokens_out, 70); // 40+20+10
        assert_eq!(d.cache_read, 2500); // 500+800+1200
        assert_eq!(d.tool_calls, 1);
        assert_eq!(d.model, "glm-5.2");
        // 上下文：ctx = input + cache_read + cache_creation（cc=0）
        assert_eq!(d.context_current, 1230); // 最后一条 A3: 30+1200
        assert_eq!(d.context_peak, 1230); // A3 最大
        assert_eq!(d.compact_count, 1); // compact_boundary 行
                                        // 回合1：含 tool_use assistant + text assistant，tool_result 归入回合1
        assert_eq!(d.turns[0].idx, 1);
        assert_eq!(d.turns[0].prompt, "第一问");
        assert_eq!(d.turns[0].tokens_in, 150); // 100+50
        assert_eq!(d.turns[0].tokens_out, 60); // 40+20
        assert_eq!(d.turns[0].tool_calls, 1);
        assert_eq!(d.turns[0].ctx, 850); // 回合1 最后一条 A2: 50+800
        assert_eq!(d.turns[0].ts, "2026-08-07T01:00:00.000Z");
        assert_eq!(d.turns[1].idx, 2);
        assert_eq!(d.turns[1].prompt, "第二问");
        assert_eq!(d.turns[1].ctx, 1230); // A3: 30+1200
    }

    #[test]
    fn scan_detail_empty_text_returns_zero() {
        let d = super::scan_detail_from_text("");
        assert_eq!(d.turn_count, 0);
        assert_eq!(d.tokens_in, 0);
        assert!(d.turns.is_empty());
    }

    #[test]
    fn scan_detail_detects_compact_by_ctx_dip() {
        // cc 新版 compact 不写 compact_boundary，靠相邻有效 ctx 大幅下降（降 30%+）推断
        let text = "\
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"a\"}],\"usage\":{\"input_tokens\":100000,\"output_tokens\":10}}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:01:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"再问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:01:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"glm-5.2\",\"content\":[{\"type\":\"text\",\"text\":\"b\"}],\"usage\":{\"input_tokens\":40000,\"output_tokens\":10}}}";
        let d = super::scan_detail_from_text(text);
        assert_eq!(d.compact_count, 1); // 100k→40k 降 60%
        assert_eq!(d.context_peak, 100000);
        assert_eq!(d.context_current, 40000);
    }

    #[test]
    fn scan_detail_skips_zero_ctx_anomaly() {
        // usage 全 0 的异常 assistant（stop_sequence）不计入 ctx
        let text = "\
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"问\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"a\"}],\"usage\":{\"input_tokens\":50000,\"output_tokens\":10}}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:06.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":0,\"output_tokens\":0},\"stop_reason\":\"stop_sequence\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:07.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"c\"}],\"usage\":{\"input_tokens\":55000,\"output_tokens\":10}}}";
        let d = super::scan_detail_from_text(text);
        assert_eq!(d.context_current, 55000); // 最后有效值，非 0
        assert_eq!(d.context_peak, 55000);
        assert_eq!(d.compact_count, 0); // 0 被跳过，不算跳降
        assert_eq!(d.turns[0].ctx, 55000); // 回合 ctx = 最后有效值
    }

    #[test]
    fn scan_detail_interrupted_turn_inherits_prev_ctx() {
        // 中断回合（stop_sequence、usage 全 0 的 assistant 独占回合）继承前一回合 ctx，
        // 不归零——上下文没变，只是该响应被中断（用户按 Esc 等）。
        let text = "\
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"问1\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"a\"}],\"usage\":{\"input_tokens\":50000,\"output_tokens\":10}}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:01:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"继续\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:01:05.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"\"}],\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0},\"stop_reason\":\"stop_sequence\"}}
{\"type\":\"user\",\"timestamp\":\"2026-08-07T01:02:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"再继续\"}}
{\"type\":\"assistant\",\"timestamp\":\"2026-08-07T01:02:05.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"c\"}],\"usage\":{\"input_tokens\":60000,\"output_tokens\":10}}}";
        let d = super::scan_detail_from_text(text);
        assert_eq!(d.turn_count, 3);
        assert_eq!(d.turns[0].ctx, 50000);
        assert_eq!(d.turns[1].ctx, 50000); // 中断回合继承回合1 ctx，非 0
        assert_eq!(d.turns[2].ctx, 60000);
        assert_eq!(d.compact_count, 0); // 继承不算 compact 跳降
    }

    #[test]
    fn cached_scan_returns_same_as_plain_for_real_session() {
        // 集成：对真实 ~/.claude 会话，cached 版与原版返回相同 token（若有运行会话）
        let sessions = super::collect_sessions();
        for s in sessions.iter().take(2) {
            let plain = super::scan_session_jsonl(&s.id, &s.cwd);
            let cached = super::scan_session_jsonl_cached(&s.id, &s.cwd);
            assert_eq!(plain.is_some(), cached.is_some());
            if let (Some(p), Some(c)) = (plain, cached) {
                assert_eq!(p.tokens_in, c.tokens_in);
                assert_eq!(p.tokens_out, c.tokens_out);
                assert_eq!(p.title, c.title);
            }
        }
    }
}
