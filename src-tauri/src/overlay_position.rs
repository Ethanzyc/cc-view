// overlay 窗口位置 + pin 持久化：load/save 读写 ~/.claude/cc-view/overlay-position.json。
// 用户拖动 overlay 后存位，下次呼出恢复——不再每次 center。pin（失焦是否收起）一并持久化。
// 模块在 Task 2 被命令引用前暂未被非测试代码使用，允许 dead_code。
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

/// pinned 的 serde 默认值：false（开机隐藏 + 呼出默认未钉 = 失焦收起）。
fn default_pinned() -> bool {
    false
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OverlayPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_pinned")]
    pub pinned: bool,
}

impl OverlayPosition {
    /// 从磁盘加载上次保存的位置；文件不存在 / 无 home / 解析失败都返回 None。
    pub fn load() -> Option<Self> {
        let path = dirs::home_dir()?.join(".claude/cc-view/overlay-position.json");
        let txt = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// 拖动保存：保留磁盘上已有的 pinned（无值时默认 false）。
    pub fn save(x: i32, y: i32) {
        let pinned = Self::load().map(|p| p.pinned).unwrap_or(false);
        Self::save_all(x, y, pinned);
    }

    /// 显式保存完整位置（含 pinned），供 set_overlay_pinned command 调用。
    pub fn save_all(x: i32, y: i32, pinned: bool) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("overlay-position.json");
        if let Ok(json) = serde_json::to_string(&OverlayPosition { x, y, pinned }) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let pos = OverlayPosition { x: 100, y: 200, pinned: true };
        let json = serde_json::to_string(&pos).unwrap();
        let back: OverlayPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, 100);
        assert_eq!(back.y, 200);
        assert!(back.pinned);
    }

    #[test]
    fn old_json_without_pinned_defaults_false() {
        // 向后兼容：无 pinned 字段时默认 false（区别于旧 hud-position.json 的 true）。
        let old = r#"{"x":42,"y":99}"#;
        let pos: OverlayPosition = serde_json::from_str(old).unwrap();
        assert_eq!(pos.x, 42);
        assert_eq!(pos.y, 99);
        assert!(!pos.pinned);
    }

    #[test]
    fn load_invalid_json_returns_none() {
        let pos: Option<OverlayPosition> = serde_json::from_str("not json").ok();
        assert!(pos.is_none());
    }
}
