// overlay 窗口位置 + pin 持久化：load/save 读写 ~/.claude/cc-view/overlay-position.json。
// 用户拖动 overlay 后存位，下次呼出恢复——不再每次 center。pin（失焦是否收起）一并持久化。
// 路径参数化（load_from/save_to/save_all_to）供单测用 tempdir 隔离；便捷版 load/save/save_all
// 走默认路径，签名不变（lib.rs 调用兼容）。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// pinned 的 serde 默认值：false（开机隐藏 + 呼出默认未钉 = 失焦收起）。
fn default_pinned() -> bool {
    false
}

const FILENAME: &str = "overlay-position.json";

/// 默认配置目录 ~/.claude/cc-view（无 home 时 None）。
fn default_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude/cc-view"))
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OverlayPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_pinned")]
    pub pinned: bool,
}

impl OverlayPosition {
    /// 从默认路径加载（~/.claude/cc-view/overlay-position.json）；无 home / 文件不存在 / 解析失败 → None。
    pub fn load() -> Option<Self> {
        Self::load_from(&default_dir()?.join(FILENAME))
    }

    /// 从指定路径加载（单测用 tempdir 隔离）。
    pub fn load_from(path: &Path) -> Option<Self> {
        let txt = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// 拖动保存（默认路径）：保留磁盘上已有的 pinned（无值时默认 false）。
    pub fn save(x: i32, y: i32) {
        let Some(dir) = default_dir() else { return };
        Self::save_to(&dir.join(FILENAME), x, y);
    }

    /// 拖动保存（指定路径）：保留该文件已有的 pinned（单测用）。
    pub fn save_to(path: &Path, x: i32, y: i32) {
        let pinned = Self::load_from(path).map(|p| p.pinned).unwrap_or(false);
        Self::save_all_to(path, x, y, pinned);
    }

    /// 显式保存完整位置（含 pinned），默认路径，供 set_overlay_pinned command 调用。
    pub fn save_all(x: i32, y: i32, pinned: bool) {
        let Some(dir) = default_dir() else { return };
        Self::save_all_to(&dir.join(FILENAME), x, y, pinned);
    }

    /// 显式保存完整位置（指定路径，含建父目录），单测用。
    pub fn save_all_to(path: &Path, x: i32, y: i32, pinned: bool) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&OverlayPosition { x, y, pinned }) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    #[test]
    fn save_all_to_roundtrip() {
        // save_all_to 写盘 → load_from 读回，字段全等。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_all_to(&path, 1, 2, true);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y, pos.pinned), (1, 2, true));
    }

    #[test]
    fn save_to_preserves_existing_pinned() {
        // 磁盘已有 pinned:true，save_to(新坐标) 必须保留 pinned（拖动不改 pin 的核心不变量）。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_all_to(&path, 5, 5, true);
        OverlayPosition::save_to(&path, 10, 20);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y), (10, 20));
        assert!(pos.pinned, "save_to must preserve existing pinned");
    }

    #[test]
    fn save_to_defaults_pinned_false_when_no_file() {
        // 无文件时 save_to，pinned 默认 false（新装用户首次拖动）。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_to(&path, 7, 8);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y), (7, 8));
        assert!(!pos.pinned);
    }
}
