use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

/// pid 存活且其可执行路径含 "claude" 或 "node"，防 PID 回收误判。
pub fn is_claude_alive(pid: u32) -> bool {
    if !kill_zero_ok(pid) {
        return false;
    }
    match proc_pidpath(pid) {
        Some(path) => {
            let p = path.to_string_lossy().to_lowercase();
            p.contains("claude") || p.contains("node")
        }
        None => false,
    }
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
}
