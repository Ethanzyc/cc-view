// 会话搁置表：{ session_id: snoozed_at_ms }。持久化到 ~/.claude/cc-view/snoozed.json。
// 与 hidden.rs 同构，但存时间戳（自动失效需要）。
use crate::models::Session;
use std::collections::HashMap;

pub struct SnoozeMap {
    map: HashMap<String, i64>,
}

impl SnoozeMap {
    pub fn empty() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else { return Self::empty(); };
        let path = home.join(".claude/cc-view/snoozed.json");
        let Ok(json) = std::fs::read_to_string(&path) else {
            log::warn!("snoozed load: failed to read ~/.claude/cc-view/snoozed.json");
            return Self::empty();
        };
        Self {
            map: serde_json::from_str(&json).unwrap_or_else(|e| {
                log::warn!("snoozed load: invalid json, ignoring: {e}");
                HashMap::new()
            }),
        }
    }

    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else { return; };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string(&self.map) {
            let _ = std::fs::write(dir.join("snoozed.json"), json);
        }
    }

    pub fn add(&mut self, id: &str, at: i64) {
        self.map.insert(id.to_string(), at);
    }

    pub fn remove(&mut self, id: &str) {
        self.map.remove(id);
    }

    pub fn to_map(&self) -> HashMap<String, i64> {
        self.map.clone()
    }

    /// 有效搁置：有 snoozedAt，且搁置后状态未再更新。
    /// 失效 = statusUpdatedAt > snoozedAt：搁置后又重新输入/触发新动作（无论新状态是
    ///        working 还是等输入）即视为重新关注 → 自动取消搁置、冒泡回待介入。
    /// 边界：statusUpdatedAt == snoozedAt 不算更新（同刻搁置不立即失效）。
    pub fn is_effectively_snoozed(&self, s: &Session) -> bool {
        let Some(at) = self.map.get(&s.id).copied() else { return false; };
        s.status_updated_at <= at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FocusHint, Source, Status};

    fn sess(id: &str, st: Status, updated_at: i64) -> Session {
        Session {
            id: id.into(), source: Source::Interactive, pid: 1, project: "p".into(),
            cwd: "/c".into(), name: id.into(), status: st, started_at: 0,
            status_updated_at: updated_at, alive: true, focus_hint: FocusHint::default(),
            snoozed: false,
            tokens_in: 0,
            tokens_out: 0,
        }
    }

    #[test]
    fn add_then_visible_in_map() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert_eq!(m.to_map().get("a").copied(), Some(1000));
        assert!(m.to_map().get("b").is_none());
    }

    #[test]
    fn remove_clears() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        m.remove("a");
        assert!(m.to_map().get("a").is_none());
    }

    #[test]
    fn not_snoozed_when_absent() {
        let m = SnoozeMap::empty();
        assert!(!m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 500)));
    }

    #[test]
    fn snoozed_when_stale_status_unchanged() {
        // 搁置时 statusUpdatedAt=1000，之后未变 → 仍有效搁置
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 1000)));
    }

    #[test]
    fn auto_unsnooze_when_new_waiting_input() {
        // 搁置(at=1000)后状态更新(updated_at=2000)且停在 WaitingForInput → 失效冒泡
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(!m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 2000)));
    }

    #[test]
    fn auto_unsnooze_when_new_needs_permission() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(!m.is_effectively_snoozed(&sess("a", Status::NeedsPermission, 2000)));
    }

    #[test]
    fn auto_unsnooze_when_new_working() {
        // 搁置后又输入/动作（statusUpdatedAt 更新）→ 即使停在 Working 也取消搁置（重新关注）
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(!m.is_effectively_snoozed(&sess("a", Status::Working, 2000)));
    }

    #[test]
    fn boundary_equal_not_stale() {
        // statusUpdatedAt == snoozedAt（同刻）→ 不视为"更新过"，仍搁置
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 1000)));
    }
}
