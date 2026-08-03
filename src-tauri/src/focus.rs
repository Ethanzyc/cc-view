use crate::models::Host;
use std::process::Command;

/// MVP focus：osascript activate 终端 app（不精确到 tab/pane）。
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
    // 激活终端 app：open -a + set frontmost，让目标终端可靠变 active（菜单栏变它）。
    // ⚠️ 已知限制：全屏 app 的 Space 切换受 macOS 系统保护，activate/open/set frontmost/AXRaise
    // 都切不到全屏 Space（辅助功能也翻不过）。若终端是全屏，点会话只 active、需用户手动 ⌘Tab 切 Space。
    // 可靠方案：终端不全屏（最大化窗口），activate 即可切 Space。
    let _ = Command::new("/usr/bin/open").args(["-a", app]).spawn();
    let script = format!(
        r#"tell application "System Events" to set frontmost of (first process whose name is "{}") to true"#,
        app
    );
    let _ = Command::new("/usr/bin/osascript").arg("-e").arg(script).spawn();
}
