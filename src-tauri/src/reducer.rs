use crate::models::Session;
use std::collections::HashMap;

/// 按 id 去重：同 id 取 alive=true 的那条，否则 last-wins（后者覆盖前者）。
///
/// 现状（Plan 4 后）：collect_sessions 顺序 sessions → roster → agents(仅 Fleet)，
/// agents 最后 push 覆盖 roster 默认 Working。同 sessionId 两个活跃会话时仍 last-wins，
/// 但实际风险低（interactive 前台与 fleet 后台 worker 的 pid/sessionId 几乎不重叠）。
/// 若未来需要字段级合并（取最丰富），在此扩展。
pub fn reduce(sessions: Vec<Session>) -> Vec<Session> {
    let mut map: HashMap<String, Session> = HashMap::new();
    for s in sessions {
        match map.get(&s.id) {
            // 已有存活的那条，新来的非存活直接跳过
            Some(prev) if prev.alive && !s.alive => continue,
            _ => {
                map.insert(s.id.clone(), s);
            }
        }
    }
    let mut v: Vec<Session> = map.into_values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Source, Status, FocusHint};

    fn mk(id: &str, alive: bool) -> Session {
        Session {
            id: id.into(),
            source: Source::Interactive,
            pid: 1,
            project: "p".into(),
            cwd: "/c".into(),
            name: id.into(),
            status: Status::Working,
            started_at: 0,
            status_updated_at: 0,
            alive,
            focus_hint: FocusHint::default(),
            snoozed: false,
        }
    }

    #[test]
    fn dedups_preferring_alive() {
        let r = reduce(vec![mk("a", false), mk("a", true)]);
        assert_eq!(r.len(), 1);
        assert!(r[0].alive);
    }

    #[test]
    fn keeps_distinct_ids() {
        let r = reduce(vec![mk("a", true), mk("b", true)]);
        assert_eq!(r.len(), 2);
    }
}
