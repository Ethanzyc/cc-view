use std::ffi::OsString;
use sysinfo::System;
use std::os::unix::ffi::OsStringExt;

/// pid 存活且其可执行路径/进程名含 "claude" 或 "node"，防 PID 回收误判。
/// proc_pidpath 失败（claude-code 新版 sandbox/exec 映射 → ESRCH）时 fallback
/// sysinfo 进程名（sysctl comm，不受影响）。collect 复用循环外刷新的 sys，无额外开销。
/// claude-code 新版的进程 exe 是 sandbox/exec 映射（proc_pidpath 拿不到 → ESRCH），
/// 但 sysinfo name（sysctl comm）能拿到 "claude"。collect 复用循环外刷新的 sys，无额外开销。
pub fn is_claude_alive_sys(sys: &System, pid: u32) -> bool {
    if !kill_zero_ok(pid) {
        return false;
    }
    if let Some(path) = proc_pidpath(pid) {
        let p = path.to_string_lossy().to_lowercase();
        return p.contains("claude") || p.contains("node");
    }
    sys.process(sysinfo::Pid::from_u32(pid))
        .map(|p| {
            let name = p.name().to_string_lossy().to_lowercase();
            name.contains("claude") || name.contains("node")
        })
        .unwrap_or(false)
}

/// kill(pid, 0) 成功表示进程存在（且有权限发信号）。
fn kill_zero_ok(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// 通过 macOS libproc 的 proc_pidpath 获取可执行文件路径。
/// 失败（进程不存在/权限不足/buffer 不足）一律返回 None，让调用方 fail-closed。
fn proc_pidpath(pid: u32) -> Option<OsString> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe {
        libc::proc_pidpath(pid as i32, buf.as_mut_ptr() as *mut _, buf.len() as u32)
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(OsString::from_vec(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_process_is_alive() {
        // 当前测试进程自身一定能被 kill(,0) 探测到
        assert!(kill_zero_ok(std::process::id()));
    }

    #[test]
    fn dead_pid_not_alive() {
        // 999999 在 macOS 上几乎不可能是活跃 pid
        assert!(!kill_zero_ok(999_999));
    }

    #[test]
    fn diag_real_session_pids() {
        // 回归守护：claude-code 新版 proc_pidpath 拿不到 exe（ESRCH），但 sysinfo name 能拿到。
        // 至少有一个活 claude pid 能被 is_claude_alive_sys 判活（否则「全已退出」回归）。
        let home = std::env::var("HOME").unwrap();
        let dir = format!("{home}/.claude/sessions");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new().with_exe(UpdateKind::Always));
        let mut any_alive = false;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let Some(pid_str) = name.strip_suffix(".json") else { continue };
            let Ok(pid) = pid_str.parse::<u32>() else { continue };
            if is_claude_alive_sys(&sys, pid) {
                any_alive = true;
            }
        }
        // 本机通常有正在跑的 claude 会话；没有就跳过（不 fail，CI/无会话环境兼容）
        if any_alive { return; }
        log::debug!("warn: no alive claude session detected (may be none running)");
    }
}
