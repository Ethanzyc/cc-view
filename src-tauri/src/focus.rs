use crate::models::Host;
use std::process::Command;

#[cfg(target_os = "macos")]
mod ax {
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        // 返回 macOS Boolean（unsigned char），用 u8 接再转 bool
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    }

    /// 辅助功能权限。prompt=true 触发系统授权弹窗（"cc-view 想要控制此电脑"，
    /// 主流 app 同款）。首次调用把 app 加入系统设置列表，重复调用安全。
    pub fn trusted(prompt: bool) -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let val = CFBoolean::from(prompt);
        let opts = CFDictionary::from_CFType_pairs(&[(key, val)]);
        unsafe { AXIsProcessTrustedWithOptions(opts.as_CFTypeRef()) != 0 }
    }
}

#[cfg(not(target_os = "macos"))]
mod ax {
    pub fn trusted(_prompt: bool) -> bool { true }
}

/// 辅助功能权限查询。focus 切全屏 Space（点 Dock）需要；未授权时调 prompt=true 弹系统窗。
pub fn ax_trusted(prompt: bool) -> bool {
    ax::trusted(prompt)
}

/// MVP focus：activate 终端 **app**（不精确到 window/tab/pane）。
///
/// 已知限制：同一 app 多窗口（如 Otty 的 B1 全屏终端 / B2 / 桌面窗口）时，activate 整个
/// app 后 macOS 带前哪个窗口不确定 → 可能跳到非终端所在窗口。日志已验证 open -a + click
/// Dock 都 exit=0、found=true（命令全成功），问题在 app 级 activate 的窗口不精确。
/// cc-view 是文件系统发现 session、无终端窗口句柄，macOS AX 也不暴露 shell pid→终端
/// window/tab 映射，故通用层面难精确。
///
/// TODO(终端集成)：各终端 app 的 AppleScript/CLI 支持 tab/window 级控制，cc-view 记住
/// session→tab 后可精确切。用户 2026-08-07 定：常见终端 app 都要支持。详见 memory
/// [[focus-terminal-window-integration]]。
///
/// Unknown host 直接返回不动作；spawn 失败忽略（fire-and-forget）。
pub fn activate_host(host: &Host) {
    // host → macOS 应用名映射；tmux 兜底激活 Terminal。
    let app = match host {
        Host::ITerm2 => "iTerm2",
        Host::Ghostty => "Ghostty",
        Host::Vscode => "Visual Studio Code",
        Host::Idea => "IntelliJ IDEA",
        Host::Otty => "Otty",
        Host::Cmux => "cmux",
        Host::Tmux => "Terminal",
        Host::Warp => "Warp",
        Host::Terminal => "Terminal",
        Host::Unknown => return, // 未知 host 不动作
    };
    // 激活终端 app：open -a（兜底 active）+ 点 Dock 图标。
    // 点 Dock 图标 = 切 app + 切到它的全屏 Space（macOS 用户切全屏 app 的标准方式，
    // 也是唯一能程序精确切特定全屏 app Space 的手段——⌘Tab 切最近 app 不精确，
    // activate/open/set frontmost/AXRaise 都只 active 不切全屏 Space）。
    // 需辅助功能权限（系统设置 → 隐私与安全 → 辅助功能 → cc-view）。
    // System Events 的 whose 查询对 Dock 无效，用循环遍历找图标。
    // Dock 显示名需与 app 一致（Otty/iTerm2/Ghostty/Visual Studio Code/...）。
    // 注：仍是 app 级 activate——同 app 多窗口不精确（见函数 doc 注释）。
    let _ = Command::new("/usr/bin/open").args(["-a", app]).spawn();
    let script = format!(
        r#"tell application "System Events"
    tell process "Dock"
        repeat with el in UI elements of list 1
            try
                if name of el is "{}" then
                    click el
                    exit repeat
                end if
            end try
        end repeat
    end tell
end tell"#,
        app
    );
    let _ = Command::new("/usr/bin/osascript").arg("-e").arg(script).spawn();
}
