// 解析 ~/.claude/sessions/<pid>.json，将原始 JSON 转换为 Session 模型。
// 依赖 models + statemachine::decide 决定最终 Status。
use crate::liveness::is_claude_alive;
use crate::models::{FocusHint, Session, Source, Status};
use crate::statemachine::{decide, DecideInput};
use std::path::Path;

#[derive(Debug)]
pub enum ParseError {
    BadJson(serde_json::Error),
    MissingField,
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
}
