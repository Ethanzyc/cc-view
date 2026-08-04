// 后续所有任务共享的数据模型（Session / Status / Source / Host / FocusHint）。
// 序列化统一 camelCase，供前端 TS 直接使用。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Working,
    WaitingForInput,
    /// 过程中提问（sessions.json status="waiting"）：Claude 问了问题/呈现选项，
    /// 必须用户回答才能继续当前任务。区别于 WaitingForInput（任务完成、等下一条指令）。
    WaitingForReply,
    NeedsPermission,
    Shell,
    Compacting, // post-compact 窗口（刚 compact 完、agent 未 resume）；进行中无法从 JSONL 检测
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Interactive,
    Fleet,
    Slash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Host {
    #[default]
    Unknown,
    ITerm2,
    Ghostty,
    Vscode,
    Idea,
    Terminal,
    Otty,
    Cmux,
    Tmux,
    Warp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusHint {
    pub host: Host,
    pub iterm_session_id: Option<String>,
    pub tmux_pane: Option<String>,
    pub term_program: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub source: Source,
    pub pid: u32,
    pub project: String,
    pub cwd: String,
    pub name: String,
    pub status: Status,
    pub started_at: i64,
    pub status_updated_at: i64,
    pub alive: bool,
    pub focus_hint: FocusHint,
    /// derived：由 poll_loop 每轮用 snoozed::is_effectively_snoozed 算，不持久化。
    /// serde default 兼容旧缓存/前端旧版。
    #[serde(default)]
    pub snoozed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_serializes_to_camel_case() {
        let s = Session {
            id: "x".into(), source: Source::Interactive, pid: 1,
            project: "p".into(), cwd: "/c".into(), name: "n".into(),
            status: Status::Working, started_at: 0, status_updated_at: 0,
            alive: true, focus_hint: FocusHint::default(),
            snoozed: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"statusUpdatedAt\""));
        assert!(json.contains("\"focusHint\""));
    }
}
