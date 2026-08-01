use crate::models::{Session, Status};
use std::collections::HashMap;

pub struct Notifier {
    last: HashMap<String, Status>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
        }
    }

    /// 返回本次新迁移到 NeedsPermission/WaitingInput 的 (name, status)。纯逻辑。
    pub fn observe(&mut self, sessions: &[Session]) -> Vec<(String, Status)> {
        let mut to_notify = Vec::new();
        let mut cur = HashMap::new();
        for s in sessions {
            // cur 记录所有 session（含死的），避免 session 复活时漏迁移
            cur.insert(s.id.clone(), s.status.clone());
            // 仅活 session 且状态为 NeedsPermission/WaitingInput 时判定通知
            if s.alive && matches!(s.status, Status::NeedsPermission | Status::WaitingForInput) {
                if self.last.get(&s.id) != Some(&s.status) {
                    to_notify.push((s.name.clone(), s.status.clone()));
                }
            }
        }
        self.last = cur;
        to_notify
    }
}

/// 发 macOS 通知（osascript）。msg/title 不含双引号（调用方确保）。
/// spawn 不 wait——避免阻塞轮询线程。
pub fn send_notification(title: &str, msg: &str) {
    let script = format!("display notification \"{}\" with title \"{}\"", msg, title);
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FocusHint, Source};

    fn sess(id: &str, st: Status) -> Session {
        Session {
            id: id.into(),
            source: Source::Interactive,
            pid: 1,
            project: "p".into(),
            cwd: "/c".into(),
            name: id.into(),
            status: st,
            started_at: 0,
            status_updated_at: 0,
            alive: true,
            focus_hint: FocusHint::default(),
        }
    }

    #[test]
    fn first_permission_triggers() {
        let mut n = Notifier::new();
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn same_status_no_renotify() {
        let mut n = Notifier::new();
        n.observe(&[sess("a", Status::NeedsPermission)]);
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert!(r.is_empty()); // 防抖
    }

    #[test]
    fn working_not_notified() {
        let mut n = Notifier::new();
        let r = n.observe(&[sess("a", Status::Working)]);
        assert!(r.is_empty());
    }

    /// 死 session（alive=false）即使 status 是 NeedsPermission 也不应触发通知。
    #[test]
    fn dead_session_not_notified() {
        let mut n = Notifier::new();
        let mut s = sess("a", Status::NeedsPermission);
        s.alive = false;
        let r = n.observe(&[s]);
        assert!(r.is_empty());
    }
}
