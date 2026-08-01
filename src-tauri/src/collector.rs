// 解析 ~/.claude/sessions/<pid>.json，将原始 JSON 转换为 Session 模型。
// 依赖 models + statemachine::decide 决定最终 Status。
use crate::liveness::is_claude_alive;
use crate::models::{FocusHint, Session, Source};
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
pub fn collect_sessions() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let dir = home.join(".claude/sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(pid_str) = name.strip_suffix(".json") else { continue };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        let Ok(json) = std::fs::read_to_string(&path) else { continue }; // fail fast: 跳过坏文件
        match parse_session_file(pid, &json) {
            Ok(mut s) => { s.alive = is_claude_alive(pid); out.push(s); }
            Err(_) => continue, // 隔离坏解析
        }
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

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 末尾 ~8KB，返回 pending tool_use。
pub fn read_pending_tool_use(session_id: &str, cwd: &str) -> Option<PendingToolUse> {
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
    parse_pending_from_str(text)
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
}
