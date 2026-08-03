// 进程树 host 探测：从 Claude session pid 用 ps 命令爬父进程链，按进程名匹配终端 app。
// 之前用 sysinfo，但实测 sysinfo 0.32 的 refresh_processes_specifics(All) 会遗漏进程
// （claude 的父 shell pid 在 sys.processes() 查不到），改用 ps（直接查系统，不依赖枚举完整性）。
use crate::models::Host;
use std::process::Command;

/// 从 pid 用 ps 爬父进程链（最多 8 层），按 comm 匹配终端 app。
pub fn detect_host(pid: u32) -> Host {
    detect_host_via_ps(pid)
}

/// 兼容旧签名（collect_sessions 传 sysinfo System）——内部走 ps，不依赖 sys。
/// 保留签名避免大改 collector；sys 参数未使用（后续可清理 sysinfo 依赖）。
pub fn detect_host_with_sys(_sys: &sysinfo::System, pid: u32) -> Host {
    detect_host_via_ps(pid)
}

fn detect_host_via_ps(pid: u32) -> Host {
    let mut current = pid;
    for _ in 0..8 {
        let Some((ppid, comm)) = ps_ppid_comm(current) else {
            return Host::Unknown;
        };
        if let Some(host) = match_host(&comm, &comm) {
            return host;
        }
        if ppid <= 1 {
            return Host::Unknown; // 到达 launchd，链上无终端
        }
        current = ppid;
    }
    Host::Unknown
}

/// `ps -o ppid=,comm= -p <pid>` → (ppid, comm)。失败/空输出返回 None。
fn ps_ppid_comm(pid: u32) -> Option<(u32, String)> {
    let out = Command::new("ps")
        .args(["-o", "ppid=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let ppid: u32 = parts.next()?.parse().ok()?;
    let comm = parts.collect::<Vec<_>>().join(" ");
    Some((ppid, comm))
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
}
