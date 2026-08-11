// 用户偏好：notify（通知开关）/ shortcut（全局快捷键预设）/ poll_interval（轮询间隔秒）
// + 常驻模式：mode（面板/常驻）/ resident_layout（A/B）/ resident_show_snoozed / resident_show_idle / resident_opacity。
// + 归档：show_archived（面板 toggle 控制，面板+常驻共享读；默认 false=隐藏归档）。
// 读写 ~/.claude/cc-view/prefs.json。自启动不进此文件（tauri-plugin-autostart 自管）。
// load 失败（无 home / 无文件 / 解析失败）→ 默认值，不崩溃。
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}
fn default_shortcut() -> String {
    "alt+space".into()
}
fn default_interval() -> u64 {
    3
}
fn default_mode() -> OverlayMode {
    OverlayMode::Panel
}
fn default_layout() -> ResidentLayout {
    ResidentLayout::B
}
fn default_show() -> bool {
    true
}
fn default_opacity() -> u8 {
    55
}
fn default_theme() -> Theme {
    Theme::Light
}
fn default_false() -> bool {
    false
}
fn default_token_unit() -> TokenUnit {
    TokenUnit::Km
}
fn default_update_source() -> UpdateSource {
    UpdateSource::Auto
}
fn default_locale() -> Locale {
    Locale::Auto
}
fn default_resident_width() -> Option<f64> {
    None
}

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

/// token 量单位：Km（k/M/B 国际）/ Wan（万/亿 中文）。默认 Km。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TokenUnit {
    Km,
    Wan,
}

/// 更新源：auto（GitHub 优先 → Gitee 兜底）/ gitee（Gitee 优先 → GitHub 兜底）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum UpdateSource {
    Auto,
    Gitee,
}

/// 界面语言：auto（跟随系统）/ zh / en。默认 auto。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    Auto,
    Zh,
    En,
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
    /// 是否显示已归档会话（面板 toggle 写；面板+常驻共享读；默认 false=隐藏归档）。
    #[serde(default = "default_false")]
    pub show_archived: bool,
    /// token 量单位（km 或 wan）。默认 km。
    #[serde(default = "default_token_unit")]
    pub token_unit: TokenUnit,
    /// 常驻面板宽度（logical px）。None = 用 resident_layout 默认（A=180/B=285）。
    #[serde(default = "default_resident_width")]
    pub resident_width: Option<f64>,
    /// 显示终端 app 名（如 Otty、iTerm）。默认 false。
    #[serde(default = "default_false")]
    pub show_host: bool,
    /// 显示 token 用量。默认 true。
    #[serde(default = "default_true")]
    pub show_tokens: bool,
    /// 显示操作按钮（仅面板模式）。默认 true。
    #[serde(default = "default_true")]
    pub show_actions: bool,
    /// 更新源偏好。auto = GitHub 优先 → Gitee 兜底；gitee = Gitee 优先 → GitHub 兜底。
    #[serde(default = "default_update_source")]
    pub update_source: UpdateSource,
    /// 界面语言。auto = 跟随系统。默认 auto。
    #[serde(default = "default_locale")]
    pub locale: Locale,
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
            show_archived: false,
            token_unit: default_token_unit(),
            resident_width: default_resident_width(),
            show_host: false,
            show_tokens: true,
            show_actions: true,
            update_source: default_update_source(),
            locale: default_locale(),
        }
    }
}

impl Prefs {
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else {
            return Self::default();
        };
        let path = home.join(".claude/cc-view/prefs.json");
        let Ok(txt) = std::fs::read_to_string(&path) else {
            log::warn!("prefs load: failed to read ~/.claude/cc-view/prefs.json");
            return Self::default();
        };
        serde_json::from_str(&txt).unwrap_or_else(|e| {
            log::warn!("prefs load: invalid json, using defaults: {}", e);
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
    pub fn is_valid_shortcut(s: &str) -> bool {
        ALLOWED_SHORTCUTS.contains(&s)
    }
    /// 常驻背景透明度合法范围 0–100（百分比；0=完全透明，vibrancy 仍托底窗口可见）。
    pub fn is_valid_opacity(n: u8) -> bool {
        (0..=100).contains(&n)
    }
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
        assert!(!p.show_archived);
        assert_eq!(p.token_unit, TokenUnit::Km);
        assert_eq!(p.resident_width, None);
        assert!(!p.show_host);
        assert!(p.show_tokens);
        assert!(p.show_actions);
        assert_eq!(p.update_source, UpdateSource::Auto);
        assert_eq!(p.locale, Locale::Auto);
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
        let p: Prefs = serde_json::from_str(r#"{"notify":false,"shortcut":"ctrl+space"}"#).unwrap();
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
            show_archived: true,
            token_unit: TokenUnit::Wan,
            resident_width: Some(300.0),
            show_host: true,
            show_tokens: false,
            show_actions: false,
            update_source: UpdateSource::Gitee,
            locale: Locale::En,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.resident_width, Some(300.0));
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
        assert_eq!(
            serde_json::to_string(&OverlayMode::Panel).unwrap(),
            "\"panel\""
        );
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

    #[test]
    fn locale_serde_lowercase() {
        let l: Locale = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(l, Locale::Auto);
        assert_eq!(serde_json::to_string(&Locale::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&Locale::En).unwrap(), "\"en\"");
    }
}
