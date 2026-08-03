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
    // open -a 激活 app：比 osascript activate 更可靠——后者对全屏 app 不切 Space
    // （从 accessory app 激活全屏 app 时光 activate 不够，用户点会话没反应）。
    // open -a 走 LaunchServices，会切到目标 app 的 Space。全路径防 GUI app 打包后 PATH 缺失。
    // spawn 不 wait（激活是异步副作用，失败静默）。
    let _ = Command::new("/usr/bin/open").args(["-a", app]).spawn();
}
