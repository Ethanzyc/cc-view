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
    // 激活终端 app：open -a（LaunchServices）+ System Events set frontmost（强制前台）。
    // 单独 open -a / osascript activate 对全屏 app 只让其变 active（菜单栏变）但不切 Space；
    // set frontmost 强制前台，触发 macOS 切到该 app 所在的 Space（含全屏 Space）。
    // 注意：set frontmost 首次会触发"自动化权限"对话框（授权 cc-view 控制 System Events）。
    let _ = Command::new("/usr/bin/open").args(["-a", app]).spawn();
    let script = format!(
        r#"tell application "System Events" to set frontmost of (first process whose name is "{}") to true"#,
        app
    );
    let _ = Command::new("/usr/bin/osascript").arg("-e").arg(script).spawn();
}
