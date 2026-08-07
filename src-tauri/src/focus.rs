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
    // 激活终端 app：open -a（兜底 active）+ 点 Dock 图标。
    // 点 Dock 图标 = 切 app + 切到它的全屏 Space（macOS 用户切全屏 app 的标准方式，
    // 也是唯一能程序精确切特定全屏 app Space 的手段——⌘Tab 切最近 app 不精确，
    // activate/open/set frontmost/AXRaise 都只 active 不切全屏 Space）。
    // 需辅助功能权限（系统设置 → 隐私与安全 → 辅助功能 → cc-view）。
    // System Events 的 whose 查询对 Dock 无效，用循环遍历找图标。
    // Dock 显示名需与 app 一致（Otty/iTerm2/Ghostty/Visual Studio Code/...）。
    // 诊断（全屏 Space 跳转不对称：A→B 通、B→A 不通）——open -a 不切全屏 Space、click Dock 才切。
    // 打两步各自的 exit/stderr 定位哪步失败；定位后降为 trace 或移除。
    let open_out = Command::new("/usr/bin/open").args(["-a", app]).output();
    match &open_out {
        Ok(o) => eprintln!(
            "[activate_host] open -a {app} exit={} stderr={}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => eprintln!("[activate_host] open -a {app} spawn 失败: {e}"),
    }
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
    let osa_out = Command::new("/usr/bin/osascript").arg("-e").arg(script).output();
    match &osa_out {
        Ok(o) => eprintln!(
            "[activate_host] osascript click {app} exit={} stdout={} stderr={}",
            o.status,
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => eprintln!("[activate_host] osascript spawn 失败: {e}"),
    }
}
