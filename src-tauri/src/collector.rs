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
/// 单文件解析失败隔离（log + skip），不拖垮整体。
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
