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
    pub fn trusted(_prompt: bool) -> bool {
        true
    }
}

/// 辅助功能权限查询。focus 切全屏 Space（点 Dock）需要；未授权时调 prompt=true 弹系统窗。
pub fn ax_trusted(prompt: bool) -> bool {
    ax::trusted(prompt)
}

/// 精确切终端 window/tab/pane。
///
/// 分三档策略：
/// 1. **TTY 匹配**（iTerm2 / Terminal / Otty）：从 claude pid 取控制 TTY → AppleScript
///    遍历 windows/tabs/sessions 找 tty 匹配 → select tab + raise window。
/// 2. **Remote control**（Kitty）：`kitten @ focus-window --match "pid:N"`（需 allow_remote_control）。
/// 3. **cwd 匹配**（Ghostty ≥ 1.3.0 / cmux）：基于 libghostty AppleScript 模型，
///    用 OSC 7 marker 或 cwd 子串定位 terminal。cmux 继承 Ghostty 的 `terminal` 对象和
///    `working directory` 属性（v0.63.0 修复），AppleScript 接口一致。
/// 4. **App 级 activate**（Warp / VSCode / IntelliJ / WezTerm / Alacritty / tmux / Unknown）：
///    open -a + click Dock（保持原行为）。
///
/// 全屏 Space：所有 host 统一在精确切换后 click Dock 切 Space。
/// tty=None 或 AppleScript/kitten 失败 → 降级到 app 级 activate（容错）。
pub fn activate_host(host: &Host, tty: &Option<String>, cwd: &str) {
    let app_name = match host {
        Host::ITerm2 => "iTerm", // app bundle 名是 iTerm（不是 iTerm2）
        Host::Ghostty => "Ghostty",
        Host::Kitty => "kitty",
        Host::Vscode => "Visual Studio Code",
        Host::Idea => "IntelliJ IDEA",
        Host::Otty => "Otty",
        Host::Cmux => "cmux",
        Host::Tmux => "Terminal",
        Host::Warp => "Warp",
        Host::WezTerm => "WezTerm",
        Host::Alacritty => "Alacritty",
        Host::Terminal => "Terminal",
        Host::ZcodeApp => "ZCode", // ZCode.app（GUI 内嵌会话，无终端宿主）
        Host::Unknown => return,   // 未知 host 不动作
    };

    // 先尝试精确切换（AppleScript / kitten），失败则走 app 级 activate。
    // 精确切换成功时 open -a + click Dock 仍执行（切 Space 需要），
    // AppleScript 已选定正确 tab，activate 不会改变选中状态。
    // ZCode 不做精确切换：官方唯一入口 `zcode://workspace/open` 每次都弹
    // 「是否打开此文件夹」确认且不记忆授权（实测二次触发仍弹），故退化为纯 App 级激活。
    let _precise_ok = match host {
        Host::ITerm2 => focus_via_tty("iTerm", tty),
        Host::Terminal => focus_via_tty("Terminal", tty),
        Host::Otty => focus_via_tty("Otty", tty),
        Host::Kitty => focus_kitty(),
        Host::Ghostty => focus_via_cwd("Ghostty", cwd, tty),
        Host::Cmux => focus_via_cwd("cmux", cwd, tty),
        _ => false, // app 级终端不做精确切换
    };

    // app 级 activate + Space 切换：同步顺序执行，避免竞态。
    // open -a 先把 app 激活（可能切到当前 Space 上该 app 的窗口），
    // click Dock 切到 app 的全屏/独立 Space——必须串行，否则 race。
    let _ = Command::new("/usr/bin/open")
        .args(["-a", app_name])
        .output();
    click_dock_icon(app_name);
}

/// AppleScript TTY 匹配：遍历 app 的 windows → tabs → sessions，
/// 找 tty 匹配的 session → select 其 tab + set window frontmost。
/// 适用于 iTerm2 / Terminal / Otty（三者 AppleScript 模型兼容）。
/// tty=None 或 AppleScript 失败返回 false（调用方降级 app 级）。
fn focus_via_tty(app_name: &str, tty: &Option<String>) -> bool {
    let Some(tty) = tty else {
        return false;
    };
    if tty.is_empty() {
        return false;
    }

    // iTerm2 的 session 有 `tty` 属性，Terminal/Otty 的 tab 有 `tty` 属性。
    // 统一用"遍历 windows → tabs →（iTerm: sessions of tab）"模型。
    // 对 iTerm2：session.tty；对 Terminal/Otty：tab.tty。
    // macOS High Sierra 后 Terminal/Otty 每个 tab 是独立 window（tab 1 of window N）。
    let script = if app_name == "iTerm" {
        format!(
            r#"tell application "iTerm2"
    set matched to false
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if tty of s is "{}" then
                    select t
                    set index of w to 1
                    set matched to true
                    exit repeat
                end if
            end repeat
            if matched then exit repeat
        end repeat
        if matched then exit repeat
    end repeat
end tell"#,
            tty
        )
    } else {
        // Terminal / Otty：tab 直接有 tty 属性（Terminal.app 模型）。
        format!(
            r#"tell application "{}"
    set matched to false
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is "{}" then
                set selected of t to true
                set index of w to 1
                set matched to true
                exit repeat
            end if
        end repeat
        if matched then exit repeat
    end repeat
end tell"#,
            app_name, tty
        )
    };

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .output();

    match output {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            log::debug!(
                "focus_via_tty({}) osascript failed: {}",
                app_name,
                String::from_utf8_lossy(&o.stderr)
            );
            false
        }
        Err(e) => {
            log::debug!("focus_via_tty({}) spawn failed: {}", app_name, e);
            false
        }
    }
}

/// Kitty remote control：`kitten @ focus-window --match "pid:<claude_pid>"`。
/// 需要 kitty 开启 `allow_remote_control`。失败（未安装/未开启/无匹配）返回 false。
fn focus_kitty() -> bool {
    // kitten @ 从 kitty 窗口外调用需要 remote control socket。
    // 若未配置 allow_remote_control，此命令会失败 → 返回 false → 降级 app 级。
    // 不做 pid 匹配（调用方没有 claude pid），改为尝试 activate kitty（app 级由调用方兜底）。
    // 注：kitty 的精确切换需要 `kitten @ --to <socket> focus-window --match "pid:N"`，
    // socket 地址在 KITTY_LISTEN_ON 或 kitty.conf listen_on。无法从外部可靠获取 →
    // 当前版本 kitty 精确切换暂不可靠，降级 app 级。后续可通过 env KITTY_LISTEN_ON 增强。
    false
}

/// cwd 匹配（Ghostty ≥ 1.3.0 / cmux）：基于 libghostty AppleScript 模型精确定位 terminal。
///
/// Ghostty / cmux AppleScript 不暴露 tty 属性，多个 session 同 cwd 时无法区分。
/// 解法（cctop 同款）：往目标 TTY 写唯一 OSC 7 cwd 标记 → 更新该 terminal 的
/// working directory → AppleScript 匹配标记精确找到 → focus → 写回原始 cwd 恢复。
///
/// cmux 基于 libghostty，继承了 Ghostty 的 `terminal` 对象和 `working directory` 属性
/// （cmux v0.63.0 修复了 AppleScript 返回值），接口一致，共用同一套逻辑。
///
/// 有 tty 时用 OSC 7 精确匹配；无 tty 时降级为 cwd 子串匹配（同 cwd 多 session 不精确）。
fn focus_via_cwd(app_name: &str, cwd: &str, tty: &Option<String>) -> bool {
    if cwd.is_empty() {
        return false;
    }

    // 有 TTY：OSC 7 marker 精确匹配
    if let Some(tty_dev) = tty {
        if !tty_dev.is_empty() {
            return focus_via_osc7(app_name, tty_dev, cwd);
        }
    }

    // 无 TTY：降级 cwd 子串匹配
    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "{}"
    set matches to every terminal whose working directory contains "{}"
    if (count of matches) > 0 then
        focus (item 1 of matches)
    end if
end tell"#,
        app_name, escaped_cwd
    );
    run_osascript(&script, &format!("focus_via_cwd_{}", app_name))
}

/// OSC 7 marker 精确匹配 terminal（Ghostty / cmux）。
/// 1. 往 TTY 写唯一标记 OSC 7 → 更新 working directory
/// 2. AppleScript 匹配标记 → focus
/// 3. 写回原始 cwd OSC 7 恢复
///
/// OSC 7 格式：`ESC]7;file://<hostname>/<path> BEL`
/// Ghostty / cmux 要求带 hostname（file:///path 空 hostname 不生效）。
fn focus_via_osc7(app_name: &str, tty_dev: &str, cwd: &str) -> bool {
    // 获取 hostname（OSC 7 必须带）
    let hostname = Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "localhost".to_string());

    let marker = format!(
        "ccview-focus-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    // 1. 写标记 OSC 7（file://<hostname>/<marker> → working directory 变为 /<marker>）
    let osc7_marker = format!("\x1b]7;file://{}/{}\x07", hostname, marker);
    if std::fs::write(tty_dev, osc7_marker.as_bytes()).is_err() {
        log::debug!(
            "focus_via_osc7_{}: failed to write marker to {}",
            app_name,
            tty_dev
        );
        return false;
    }

    // 2. 等 app 处理 OSC 7（异步解析 escape sequence）
    std::thread::sleep(std::time::Duration::from_millis(80));

    // 3. AppleScript 匹配标记 → focus
    let script = format!(
        r#"tell application "{}"
    set matches to every terminal whose working directory contains "{}"
    if (count of matches) > 0 then
        focus (item 1 of matches)
    end if
end tell"#,
        app_name, marker
    );
    let ok = run_osascript(&script, &format!("focus_via_osc7_{}", app_name));

    // 4. 写回原始 cwd OSC 7 恢复（无论 step 3 成功与否都恢复）
    // cwd 已是绝对路径（/Users/...），file://<hostname><cwd> 直接拼
    let osc7_restore = format!("\x1b]7;file://{}{}\x07", hostname, cwd);
    let _ = std::fs::write(tty_dev, osc7_restore.as_bytes());

    ok
}

/// 运行 AppleScript，成功返回 true，失败 log + 返回 false。
fn run_osascript(script: &str, tag: &str) -> bool {
    match Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
    {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            log::debug!(
                "{} osascript failed: {}",
                tag,
                String::from_utf8_lossy(&o.stderr)
            );
            false
        }
        Err(e) => {
            log::debug!("{} spawn failed: {}", tag, e);
            false
        }
    }
}

/// 点 Dock 图标 = 切 app + 切到它的全屏 Space（唯一可靠方式）。
/// 需辅助功能权限。System Events 的 whose 查询对 Dock 无效，用循环遍历找图标。
/// 同步执行（.output）——须在 open -a 之后串行调用，否则 race 导致跨 Space 切换失败。
fn click_dock_icon(app_name: &str) {
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
        app_name
    );
    let _ = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output();
}
