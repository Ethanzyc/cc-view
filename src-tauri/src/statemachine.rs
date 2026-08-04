use crate::models::Status;

#[derive(Debug)]
pub struct DecideInput<'a> {
    pub raw_status: &'a str,        // sessions.json 的 status 字段（busy/shell/idle/...）
    pub pending_permission: bool,   // PermissionChecker 结果（Plan 2 接入，此处由调用方给）
}

pub fn decide(input: &DecideInput) -> Status {
    // fail fast：权限请求优先级最高，先于任何 busy 判断
    if input.pending_permission {
        return Status::NeedsPermission;
    }
    match input.raw_status {
        "busy" => Status::Working,
        "shell" => Status::Shell,
        // sessions.json status="waiting"：过程中提问（Claude 问了问题，必须回答才能继续）。
        // 区别于 "idle"（任务完成、等下一条指令）→ WaitingForInput。
        "waiting" => Status::WaitingForReply,
        _ => Status::WaitingForInput,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_is_working() {
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: false }), Status::Working);
    }
    #[test]
    fn shell_is_shell() {
        assert_eq!(decide(&DecideInput { raw_status: "shell", pending_permission: false }), Status::Shell);
    }
    #[test]
    fn permission_outranks_busy() {
        assert_eq!(decide(&DecideInput { raw_status: "busy", pending_permission: true }), Status::NeedsPermission);
    }
    #[test]
    fn unknown_status_is_waiting() {
        assert_eq!(decide(&DecideInput { raw_status: "idle", pending_permission: false }), Status::WaitingForInput);
    }
    #[test]
    fn waiting_is_waiting_for_reply() {
        // sessions.json status="waiting" = 过程中提问，区别于 "idle"（任务完成）
        assert_eq!(decide(&DecideInput { raw_status: "waiting", pending_permission: false }), Status::WaitingForReply);
    }
}
