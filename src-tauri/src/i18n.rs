// 后端用户可见文案 i18n：菜单项 / tray tooltip / 通知文本。
// 量小（~10 条），用 match 查表，不引入 i18n crate。
// 前端 locale 存 prefs → resolve 得 Lang →查 BackendStrings。
use crate::prefs::Locale;

/// 解析后的实际语言（auto 在入口处 resolve 为 Zh 或 En）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang {
    Zh,
    En,
}

/// auto → 按系统语言 resolve；zh/en 直接返回。
pub fn resolve(locale: Locale) -> Lang {
    match locale {
        Locale::Zh => Lang::Zh,
        Locale::En => Lang::En,
        Locale::Auto => {
            let sys = sys_locale::get_locale().unwrap_or_default();
            if sys.starts_with("zh") {
                Lang::Zh
            } else {
                Lang::En
            }
        }
    }
}

/// 后端所有用户可见文案（一个 struct 集中管理）。
pub struct BackendStrings {
    pub menu_show: &'static str,
    pub menu_prefs: &'static str,
    pub menu_update: &'static str,
    pub menu_quit: &'static str,
    // tooltip 片段模板（{} 为数字占位）
    pub tip_perm: &'static str,
    pub tip_reply: &'static str,
    pub tip_idle: &'static str,
    pub tip_working: &'static str,
    pub tip_sep: &'static str,
    // 通知文案
    pub notify_perm: &'static str,
    pub notify_reply: &'static str,
    pub notify_input: &'static str,
    pub notify_attention: &'static str,
    // 更新通知
    pub update_title: &'static str,
    pub update_body: &'static str,
}

impl Lang {
    pub fn strings(self) -> BackendStrings {
        match self {
            Lang::Zh => BackendStrings {
                menu_show: "显示面板",
                menu_prefs: "偏好设置…",
                menu_update: "检查更新…",
                menu_quit: "退出 cc-view",
                tip_perm: "{} 等权限",
                tip_reply: "{} 等回答",
                tip_idle: "{} 等我",
                tip_working: "{} 工作",
                tip_sep: " · ",
                notify_perm: "等待权限确认",
                notify_reply: "等待你回答",
                notify_input: "等待输入",
                notify_attention: "需要关注",
                update_title: "CC View 已更新到 {}",
                update_body: "点击查看更新内容",
            },
            Lang::En => BackendStrings {
                menu_show: "Show Panel",
                menu_prefs: "Preferences…",
                menu_update: "Check for Updates…",
                menu_quit: "Quit cc-view",
                tip_perm: "{} perm",
                tip_reply: "{} reply",
                tip_idle: "{} waiting",
                tip_working: "{} working",
                tip_sep: " · ",
                notify_perm: "Waiting for permission",
                notify_reply: "Waiting for your reply",
                notify_input: "Waiting for input",
                notify_attention: "Needs attention",
                update_title: "CC View updated to {}",
                update_body: "Click to view changes",
            },
        }
    }
}
