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
    let script = format!("tell application \"{}\" to activate", app);
    // spawn 不 wait：激活是异步副作用，失败静默忽略（let _ =）
    let _ = Command::new("osascript").arg("-e").arg(script).spawn();
}
