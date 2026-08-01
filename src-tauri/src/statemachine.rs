use crate::models::Status;

#[derive(Debug)]
pub struct DecideInput<'a> {
    pub raw_status: &'a str,        // sessions.json 的 status 字段（busy/shell/idle/...）
    pub pending_permission: bool,   // PermissionChecker 结果（Plan 2 接入，此处由调用方给）
    pub is_compacting: bool,        // JSONL 末尾检出 compact_boundary（Plan 4 Task 4）
}

pub fn decide(input: &DecideInput) -> Status {
    // fail fast：权限请求优先级最高，先于任何 busy/compact 判断
    if input.pending_permission {
        return Status::NeedsPermission;
    }
    // compact 进行中：覆盖 Working/Waiting（autocompact 是阻塞操作，agent 暂不可交互）
    if input.is_compacting {
        return Status::Compacting;
    }
    match input.raw_status {
        "busy" => Status::Working,
        "shell" => Status::Shell,
        _ => Status::WaitingForInput,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_is_working() {
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: false, is_compacting: false }), Status::Working);
    }
    #[test]
    fn shell_is_shell() {
        assert_eq!(decide(&DecideInput { raw_status: "shell", pending_permission: false, is_compacting: false }), Status::Shell);
    }
    #[test]
    fn permission_outranks_busy() {
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: true, is_compacting: false }), Status::NeedsPermission);
    }
    #[test]
    fn unknown_status_is_waiting() {
        assert_eq!(decide(&DecideInput { raw_status: "idle", pending_permission: false, is_compacting: false }), Status::WaitingForInput);
    }
    #[test]
    fn compacting_overrides_busy() {
        // autocompact 进行中：即使 raw_status=busy 也显示 Compacting
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: false, is_compacting: true }), Status::Compacting);
    }
    #[test]
    fn compacting_overrides_waiting() {
        assert_eq!(decide(&DecideInput { raw_status: "idle", pending_permission: false, is_compacting: true }), Status::Compacting);
    }
    #[test]
    fn permission_outranks_compacting() {
        // 用户显式权限请求优先于后台 compact（实际两者互斥，但保 safety net）
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: true, is_compacting: true }), Status::NeedsPermission);
    }
}
