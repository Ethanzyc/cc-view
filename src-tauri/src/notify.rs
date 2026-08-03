use crate::models::{Session, Status};
use std::collections::HashMap;

pub struct Notifier {
    last: HashMap<String, Status>,
    bootstrapped: bool,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
            bootstrapped: false,
        }
    }

    /// 返回本次新迁移到 NeedsPermission/WaitingInput 的 (name, status)。纯逻辑。
    /// 首轮 observe 只初始化 `last` 不发通知——避免启动时多个 idle session 触发通知轰炸。
    pub fn observe(&mut self, sessions: &[Session]) -> Vec<(String, Status)> {
        let mut cur = HashMap::new();
        for s in sessions {
            // cur 记录所有 session（含死的），避免 session 复活时漏迁移
            cur.insert(s.id.clone(), s.status.clone());
        }
        if !self.bootstrapped {
            self.last = cur;
            self.bootstrapped = true;
            return Vec::new(); // 首轮只初始化，不通知
        }
        let mut to_notify = Vec::new();
        for s in sessions {
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

/// 发 macOS 通知（tauri-plugin-notification，走原生 UserNotifications / 旧版 NSUserNotificationCenter）。
/// 图标：build 后 .app 取 bundle icon.icns（雷达）；dev 模式（npm run tauri dev）下插件调
/// notify_rust::set_application("com.apple.Terminal")，通知图标显示为终端图标——验证雷达
/// 图标需 `npm run tauri build` 产物。builder 自动转义 title/msg，调用方无需预处理。
/// show() 轻量，可在 poll 线程内直接调用。
pub fn send_notification(handle: &tauri::AppHandle, title: &str, msg: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = handle
        .notification()
        .builder()
        .title(title)
        .body(msg)
        .show();
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
    fn first_round_silent() {
        // 首轮 observe 即使有 NeedsPermission/WaitingForInput 也不通知
        let mut n = Notifier::new();
        let r = n.observe(&[
            sess("a", Status::NeedsPermission),
            sess("b", Status::WaitingForInput),
        ]);
        assert!(r.is_empty(), "bootstrap round must not notify");
    }

    #[test]
    fn bootstrap_then_transition_triggers() {
        let mut n = Notifier::new();
        // 首轮 bootstrap：不通知
        assert!(n.observe(&[sess("a", Status::Working)]).is_empty());
        // 第二轮 a 从 Working 迁移到 NeedsPermission → 通知
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn same_status_no_renotify() {
        let mut n = Notifier::new();
        // 先 bootstrap（Working 不通知，仅初始化 last）
        n.observe(&[sess("a", Status::Working)]);
        // 迁移到 NeedsPermission → 通知一次
        n.observe(&[sess("a", Status::NeedsPermission)]);
        // 同状态再 observe → 不通知（防抖）
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert!(r.is_empty());
    }

    #[test]
    fn working_not_notified() {
        let mut n = Notifier::new();
        n.observe(&[sess("a", Status::Working)]); // bootstrap
        let r = n.observe(&[sess("a", Status::Working)]);
        assert!(r.is_empty());
    }

    /// 死 session（alive=false）即使 status 是 NeedsPermission 也不应触发通知。
    #[test]
    fn dead_session_not_notified() {
        let mut n = Notifier::new();
        n.observe(&[sess("a", Status::Working)]); // bootstrap
        let mut s = sess("a", Status::NeedsPermission);
        s.alive = false;
        let r = n.observe(&[s]);
        assert!(r.is_empty());
    }
}
