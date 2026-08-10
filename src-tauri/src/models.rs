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
    /// 累计 token（collect 时 scan JSONL 填充，roster/agents 无 JSONL 时为 0）。
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
}

/// 单个会话的 token 消耗详情（on-demand：点详情才扫描）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session_id: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub model: String, // 最近一条 assistant 的 model
    pub turn_count: u32,
    pub tool_calls: u32,
    pub web_searches: u32,
    pub web_fetches: u32,
    /// 当前上下文占用（最后一条 assistant 的 input + 缓存读 + 缓存写）
    pub context_current: u64,
    /// 历史最高上下文占用
    pub context_peak: u64,
    /// compact_boundary 次数（上下文压缩过几次）
    pub compact_count: u32,
    pub turns: Vec<TurnStat>,
}

/// 按回合的消耗明细。ts 为原始 ISO 8601 字符串，前端 Date.parse 转 ms（省后端解析）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStat {
    pub idx: u32,
    pub prompt: String, // 用户输入前 40 字
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: u32,
    /// 该回合最后一条 assistant 的上下文占用（画 sparkline 用）
    pub ctx: u64,
    pub ts: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_serializes_to_camel_case() {
        let s = Session {
            id: "x".into(),
            source: Source::Interactive,
            pid: 1,
            project: "p".into(),
            cwd: "/c".into(),
            name: "n".into(),
            status: Status::Working,
            started_at: 0,
            status_updated_at: 0,
            alive: true,
            focus_hint: FocusHint::default(),
            snoozed: false,
            tokens_in: 0,
            tokens_out: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"statusUpdatedAt\""));
        assert!(json.contains("\"focusHint\""));
    }
}
