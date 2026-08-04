// 用户偏好：notify（通知开关）/ shortcut（全局快捷键预设）/ poll_interval（轮询间隔秒）。
// 读写 ~/.claude/cc-view/prefs.json。自启动不进此文件（tauri-plugin-autostart 自管）。
// load 失败（无 home / 无文件 / 解析失败）→ 默认值，不崩溃。
use serde::{Deserialize, Serialize};

fn default_true() -> bool { true }
fn default_shortcut() -> String { "alt+space".into() }
fn default_interval() -> u64 { 3 }

/// 允许的快捷键预设（"off" = 禁用）。
pub const ALLOWED_SHORTCUTS: &[&str] = &["alt+space", "cmd+alt+space", "ctrl+space", "off"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prefs {
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    #[serde(default = "default_interval")]
    pub poll_interval: u64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { notify: true, shortcut: default_shortcut(), poll_interval: default_interval() }
    }
}

impl Prefs {
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else { return Self::default() };
        let path = home.join(".claude/cc-view/prefs.json");
        let Ok(txt) = std::fs::read_to_string(&path) else {
            eprintln!("prefs load: failed to read ~/.claude/cc-view/prefs.json");
            return Self::default();
        };
        serde_json::from_str(&txt).unwrap_or_else(|e| {
            eprintln!("prefs load: invalid json, using defaults: {}", e);
            Self::default()
        })
    }
    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(dir.join("prefs.json"), json);
        }
    }
    pub fn is_valid_shortcut(s: &str) -> bool { ALLOWED_SHORTCUTS.contains(&s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_uses_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(p.notify);
        assert_eq!(p.shortcut, "alt+space");
        assert_eq!(p.poll_interval, 3);
    }

    #[test]
    fn partial_json_keeps_defaults_for_missing() {
        let p: Prefs = serde_json::from_str(r#"{"notify":false}"#).unwrap();
        assert!(!p.notify);
        assert_eq!(p.shortcut, "alt+space");
        assert_eq!(p.poll_interval, 3);
    }

    #[test]
    fn full_json_roundtrip() {
        let p = Prefs { notify: false, shortcut: "ctrl+space".into(), poll_interval: 10 };
        let json = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn invalid_json_falls_back_to_default() {
        let p: Prefs = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(p, Prefs::default());
    }

    #[test]
    fn is_valid_shortcut_checks_allowed() {
        assert!(Prefs::is_valid_shortcut("alt+space"));
        assert!(Prefs::is_valid_shortcut("off"));
        assert!(!Prefs::is_valid_shortcut("ctrl+shift+a"));
    }
}
