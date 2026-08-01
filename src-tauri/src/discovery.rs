// 进程树 host 探测：从 Claude session pid 爬父进程链，按进程名/exe 匹配终端 app。
// sysinfo 0.32 API：refresh_processes_specifics(ProcessesToUpdate, bool, ProcessRefreshKind)。
use crate::models::Host;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// 从 pid 爬父进程链（最多 8 层），按进程名/exe 匹配终端 app。
/// 每次调用都会 refresh 全量进程（with_exe），调用方按需批处理以减少开销。
pub fn detect_host(pid: u32) -> Host {
    let mut sys = System::new();
    // 0.32 API：三参数（ProcessesToUpdate, remove_dead, ProcessRefreshKind）。
    // with_exe(Always) 确保 p.exe() 可用（默认 Never 时 exe() 返回 None）。
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_exe(UpdateKind::Always),
    );
    let mut current = Pid::from_u32(pid);
    for _ in 0..8 {
        let Some(p) = sys.process(current) else {
            return Host::Unknown;
        };
        let name = p.name().to_string_lossy().to_lowercase();
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if let Some(host) = match_host(&name, &exe) {
            return host;
        }
        match p.parent() {
            Some(parent) => current = parent,
            None => return Host::Unknown,
        }
    }
    Host::Unknown
}

/// 纯函数：按进程名 + exe 路径字串匹配 Host。顺序敏感（先匹配多义关键字）。
/// 内部统一转小写，调用方无需预规范化——避免忘记 lowercasing 导致静默失配。
fn match_host(name: &str, exe: &str) -> Option<Host> {
    // 进程名通常是小写的 app 名（iTerm2 的进程名是 "iTerm2" 或 "iTerm Server"）
    let hay = format!("{name} {exe}").to_lowercase();
    let m = |k: &str| hay.contains(k);
    if m("iterm") {
        Some(Host::ITerm2)
    } else if m("ghostty") {
        Some(Host::Ghostty)
    } else if m("code") {
        Some(Host::Vscode)
    } else if m("intellij") || m("idea") {
        Some(Host::Idea)
    } else if m("otty") {
        Some(Host::Otty)
    } else if m("cmux") {
        Some(Host::Cmux)
    } else if m("tmux") {
        Some(Host::Tmux)
    } else if m("warp") {
        Some(Host::Warp)
    } else if m("terminal") {
        Some(Host::Terminal)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_iterm() {
        assert_eq!(match_host("iTerm2", "/Applications/iTerm.app/Contents/MacOS/iTerm2"), Some(Host::ITerm2));
    }

    #[test]
    fn match_ghostty_over_otty() {
        // "ghostty" 包含 "otty"——顺序敏感：ghostty 分支必须在 otty 前
        assert_eq!(match_host("ghostty", "/Applications/Ghostty.app"), Some(Host::Ghostty));
    }

    #[test]
    fn match_vscode() {
        assert_eq!(match_host("Code", "/Applications/Visual Studio Code.app/Contents/MacOS/Electron"), Some(Host::Vscode));
    }

    #[test]
    fn match_terminal() {
        assert_eq!(match_host("Terminal", "/System/Applications/Utilities/Terminal.app"), Some(Host::Terminal));
    }

    #[test]
    fn match_none_for_unrelated() {
        assert_eq!(match_host("claude", "/usr/local/bin/claude"), None);
    }

    #[test]
    fn detect_host_unknown_for_invalid_pid() {
        // PID 0 和超大 PID 都不会匹配真实进程 → Unknown
        assert_eq!(detect_host(0), Host::Unknown);
        assert_eq!(detect_host(u32::MAX), Host::Unknown);
    }
}
