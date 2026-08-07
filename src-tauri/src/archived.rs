// 会话归档列表（可逆）：load/save 读写 ~/.claude/cc-view/archived.json。
// 纯逻辑：filter 只在内存里筛除 archived id，不修改原始数据。
use crate::models::Session;

pub struct ArchivedList {
    ids: Vec<String>,
}

impl ArchivedList {
    pub fn empty() -> Self {
        Self { ids: vec![] }
    }
    pub fn load() -> Self {
        // 失败静默返回空——文件不存在/无 home/解析失败都视为空列表，避免崩溃。
        let Some(home) = dirs::home_dir() else {
            return Self::empty();
        };
        let dir = home.join(".claude/cc-view");
        // 优先读 archived.json；不存在则 fallback 读旧 hidden.json（hide→archive 改名前数据迁移）。
        // save() 始终写 archived.json，下次保存后旧 hidden.json 即孤立（不主动删，留作备份）。
        let json = std::fs::read_to_string(dir.join("archived.json"))
            .or_else(|_| std::fs::read_to_string(dir.join("hidden.json")));
        let Ok(json) = json else {
            return Self::empty();
        };
        Self {
            ids: serde_json::from_str(&json).unwrap_or_else(|e| {
                eprintln!("archived load: invalid json, ignoring: {e}");
                vec![]
            }),
        }
    }
    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("archived.json");
        if let Ok(json) = serde_json::to_string(&self.ids) {
            let _ = std::fs::write(path, json);
        }
    }

    // --- 纯逻辑：基于 ids 集合做判断与变更 ---
    pub fn is_archived(&self, id: &str) -> bool {
        self.ids.iter().any(|x| x == id)
    }
    pub fn add(&mut self, id: &str) {
        if !self.is_archived(id) {
            self.ids.push(id.into());
        }
    }
    pub fn remove(&mut self, id: &str) {
        self.ids.retain(|x| x != id);
    }
    /// 暴露归档 id 列表副本（外部只读访问，保护 add() 的去重不变量）
    pub fn to_vec(&self) -> Vec<String> {
        self.ids.clone()
    }
    /// 过滤掉已归档的 session（保留备用：当前前端按 list_archived 过滤）
    #[allow(dead_code)]
    pub fn filter<'a>(&self, sessions: &'a [Session]) -> Vec<&'a Session> {
        sessions.iter().filter(|s| !self.is_archived(&s.id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FocusHint, Source, Status};

    fn mk(id: &str) -> Session {
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
            alive: true,
            focus_hint: FocusHint::default(),
            snoozed: false,
            tokens_in: 0,
            tokens_out: 0,
        }
    }

    #[test]
    fn add_and_is_archived() {
        let mut h = ArchivedList::empty();
        h.add("a");
        assert!(h.is_archived("a"));
        assert!(!h.is_archived("b"));
        h.add("a"); // 去重
        assert_eq!(h.ids.len(), 1);
    }

    #[test]
    fn remove_unarchives() {
        let mut h = ArchivedList::empty();
        h.add("a");
        h.remove("a");
        assert!(!h.is_archived("a"));
    }

    #[test]
    fn filter_excludes_archived() {
        let h = {
            let mut x = ArchivedList::empty();
            x.add("a");
            x
        };
        let ss = [mk("a"), mk("b")];
        let visible = h.filter(&ss);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "b");
    }
}
