// 用户偏好：notify（通知开关）/ shortcut（全局快捷键预设）/ poll_interval（轮询间隔秒）
// + 常驻模式：mode（面板/常驻）/ resident_layout（A/B）/ resident_show_snoozed / resident_show_idle / resident_opacity。
// 读写 ~/.claude/cc-view/prefs.json。自启动不进此文件（tauri-plugin-autostart 自管）。
// load 失败（无 home / 无文件 / 解析失败）→ 默认值，不崩溃。
use serde::{Deserialize, Serialize};

fn default_true() -> bool { true }
fn default_shortcut() -> String { "alt+space".into() }
fn default_interval() -> u64 { 3 }
fn default_mode() -> OverlayMode { OverlayMode::Panel }
fn default_layout() -> ResidentLayout { ResidentLayout::B }
fn default_show() -> bool { true }
fn default_opacity() -> u8 { 55 }
fn default_theme() -> Theme { Theme::Light }

/// 允许的快捷键预设（"off" = 禁用）。
pub const ALLOWED_SHORTCUTS: &[&str] = &["alt+space", "cmd+alt+space", "ctrl+space", "off"];

/// overlay 窗口模式：常驻精简 / 面板全功能。serde lowercase（json 里 "resident"/"panel"）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum OverlayMode {
    Resident,
    Panel,
}

/// 常驻模式布局：B 精简（分组+状态文字）/ A 极简（仅图标+名称）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ResidentLayout {
    B,
    A,
}

/// 外观主题：浅色 / 深色。serde lowercase（json 里 "light"/"dark"）。默认 Light，不跟随系统。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prefs {
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    #[serde(default = "default_interval")]
    pub poll_interval: u64,
    #[serde(default = "default_mode")]
    pub mode: OverlayMode,
    #[serde(default = "default_layout")]
    pub resident_layout: ResidentLayout,
    #[serde(default = "default_show")]
    pub resident_show_snoozed: bool,
    #[serde(default = "default_show")]
    pub resident_show_idle: bool,
    #[serde(default = "default_opacity")]
    pub resident_opacity: u8,
    #[serde(default = "default_theme")]
    pub theme: Theme,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            notify: true,
            shortcut: default_shortcut(),
            poll_interval: default_interval(),
            mode: default_mode(),
            resident_layout: default_layout(),
            resident_show_snoozed: default_show(),
            resident_show_idle: default_show(),
            resident_opacity: default_opacity(),
            theme: default_theme(),
        }
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
    /// 常驻背景透明度合法范围 0–100（百分比；0=完全透明，vibrancy 仍托底窗口可见）。
    pub fn is_valid_opacity(n: u8) -> bool { (0..=100).contains(&n) }
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
        assert_eq!(p.mode, OverlayMode::Panel);
        assert_eq!(p.resident_layout, ResidentLayout::B);
        assert!(p.resident_show_snoozed);
        assert!(p.resident_show_idle);
        assert_eq!(p.resident_opacity, 55);
        assert_eq!(p.theme, Theme::Light);
    }

    #[test]
    fn partial_json_keeps_defaults_for_missing() {
        let p: Prefs = serde_json::from_str(r#"{"notify":false}"#).unwrap();
        assert!(!p.notify);
        assert_eq!(p.shortcut, "alt+space");
        assert_eq!(p.poll_interval, 3);
    }

    #[test]
    fn partial_json_keeps_new_defaults_for_missing() {
        // 现有字段设了非默认值，新字段缺失 → 新字段填默认
        let p: Prefs =
            serde_json::from_str(r#"{"notify":false,"shortcut":"ctrl+space"}"#).unwrap();
        assert!(!p.notify);
        assert_eq!(p.mode, OverlayMode::Panel);
        assert_eq!(p.resident_layout, ResidentLayout::B);
        assert_eq!(p.resident_opacity, 55);
    }

    #[test]
    fn full_json_roundtrip() {
        let p = Prefs {
            notify: false,
            shortcut: "ctrl+space".into(),
            poll_interval: 10,
            mode: OverlayMode::Panel,
            resident_layout: ResidentLayout::A,
            resident_show_snoozed: false,
            resident_show_idle: false,
            resident_opacity: 80,
            theme: Theme::Dark,
        };
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

    #[test]
    fn mode_and_layout_serde_lowercase() {
        let m: OverlayMode = serde_json::from_str("\"resident\"").unwrap();
        assert_eq!(m, OverlayMode::Resident);
        assert_eq!(serde_json::to_string(&OverlayMode::Panel).unwrap(), "\"panel\"");
        let l: ResidentLayout = serde_json::from_str("\"a\"").unwrap();
        assert_eq!(l, ResidentLayout::A);
        assert_eq!(serde_json::to_string(&ResidentLayout::B).unwrap(), "\"b\"");
    }

    #[test]
    fn theme_serde_lowercase() {
        let t: Theme = serde_json::from_str("\"dark\"").unwrap();
        assert_eq!(t, Theme::Dark);
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), "\"light\"");
    }

    #[test]
    fn is_valid_opacity_bounds() {
        // 0 允许（完全透明，vibrancy 仍托底）；上限 100。
        assert!(Prefs::is_valid_opacity(0));
        assert!(Prefs::is_valid_opacity(20));
        assert!(Prefs::is_valid_opacity(55));
        assert!(Prefs::is_valid_opacity(100));
        assert!(!Prefs::is_valid_opacity(101));
        assert!(!Prefs::is_valid_opacity(255));
    }
}
