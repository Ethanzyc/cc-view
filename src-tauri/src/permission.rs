// 读取三层 settings（user + project + local）的 permissions，预测工具调用是否需要用户确认。
//
// 三层合并（与 Claude Code /status "Setting sources" 同源）：
//   user   ~/.claude/settings.json
//   project <cwd>/.claude/settings.json        （团队共享，已 commit）
//   local   <cwd>/.claude/settings.local.json  （个人，gitignore）
// allow/ask/deny 各取并集。
//
// 判定优先级：deny → ask → 其余(allow/未知) = false。
// 只有显式命中 ask 才报警——避免 Agent/ToolSearch/Skill 等内置或 allow 工具误报"等权限"。
// 历史 bug：翻转前未知默认 true，导致 Claude Code 跑子智能体（Agent tool_use pending）
// 时 cc-view 误判 NeedsPermission，而 Claude Code 从不弹窗（内置放行 / local allow）。

use std::path::Path;

pub struct PermissionChecker {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl PermissionChecker {
    /// 按 session cwd 读三层 settings 合并 permissions。三层全无 permissions 段 → None。
    /// None 时调用方跳过权限判定（不猜测、不覆盖原 status）。
    pub fn from_settings_for_cwd(cwd: Option<&Path>) -> Option<Self> {
        let home = dirs::home_dir()?;
        let project = cwd.map(|c| c.join(".claude/settings.json"));
        let local = cwd.map(|c| c.join(".claude/settings.local.json"));
        merge_layers(
            &home.join(".claude/settings.json"),
            project.as_deref(),
            local.as_deref(),
        )
    }

    /// 判定优先级与 Claude Code 同源：deny → ask → allow → 未知。
    /// deny/allow → false（不需确认），ask → true（显式要求确认），未知 → false。
    /// 翻转前未知默认 true，导致 Agent/ToolSearch/Skill 等内置或 allow 工具误报"等权限"。
    pub fn needs_permission(&self, name: &str, bash_command: Option<&str>) -> bool {
        if matches_entry(&self.deny, name, bash_command) {
            return false;
        }
        if matches_entry(&self.ask, name, bash_command) {
            return true;
        }
        if matches_entry(&self.allow, name, bash_command) {
            return false;
        }
        false
    }
}

/// 合并三层 settings 路径的 permissions（allow/ask/deny 各取并集）。纯函数便于测试。
/// project/local 为 None 时跳过该层。三层全无 permissions 段 → None。
fn merge_layers(
    user: &Path,
    project: Option<&Path>,
    local: Option<&Path>,
) -> Option<PermissionChecker> {
    let user_p = read_layer(user);
    let project_p = project.and_then(read_layer);
    let local_p = local.and_then(read_layer);
    if user_p.is_none() && project_p.is_none() && local_p.is_none() {
        return None;
    }
    let mut allow = Vec::new();
    let mut ask = Vec::new();
    let mut deny = Vec::new();
    for (a, s, d) in [user_p, project_p, local_p].into_iter().flatten() {
        allow.extend(a);
        ask.extend(s);
        deny.extend(d);
    }
    Some(PermissionChecker { allow, ask, deny })
}

/// 读单层 settings.json 的 permissions。文件缺失/解析失败/无 permissions 段 → None。
fn read_layer(path: &Path) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
    let txt = std::fs::read_to_string(path).ok()?;
    #[derive(serde::Deserialize)]
    struct Root {
        permissions: Option<Perms>,
    }
    #[derive(serde::Deserialize)]
    struct Perms {
        allow: Option<Vec<String>>,
        ask: Option<Vec<String>>,
        deny: Option<Vec<String>>,
    }
    let p: Root = serde_json::from_str(&txt).ok()?;
    let perms = p.permissions?;
    Some((
        perms.allow.unwrap_or_default(),
        perms.ask.unwrap_or_default(),
        perms.deny.unwrap_or_default(),
    ))
}

fn matches_entry(list: &[String], name: &str, bash_command: Option<&str>) -> bool {
    list.iter().any(|e| entry_matches(e, name, bash_command))
}

fn entry_matches(entry: &str, name: &str, bash_command: Option<&str>) -> bool {
    // 裸工具名（"Read"/"Bash"）→ 匹配该工具所有调用
    if entry == name {
        return true;
    }
    // "Tool(pattern)" → 提取括号内 pattern，按工具类型匹配
    let Some(rest) = entry
        .strip_prefix(&format!("{}(", name))
        .and_then(|s| s.strip_suffix(')'))
    else {
        return false;
    };
    if name == "Bash" {
        // Bash(command-pattern)：按命令精确匹配（前缀 * 或子串包含）。
        // 历史 bug：旧实现 starts_with("Bash(") 让任意 Bash(…) 条目匹配所有 Bash 调用，
        // deny 的 Bash(rm -rf *) 会短路掩盖 ask 的 Bash(kill *)，造成 kill 等命令漏报。
        return bash_command
            .map(|cmd| match rest.strip_suffix('*') {
                Some(prefix) => cmd.starts_with(prefix.trim_end()),
                None => cmd.contains(rest),
            })
            .unwrap_or(false);
    }
    // 非 Bash 工具（Read/Write/Edit…）：cc-view 不解析 tool_use 的 path 参数，
    // 无法精确匹配 Read(.env*)，保守视为命中（deny→false / allow→false 均安全）
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(allow: &[&str], ask: &[&str], deny: &[&str]) -> PermissionChecker {
        PermissionChecker {
            allow: allow.iter().map(|s| s.to_string()).collect(),
            ask: ask.iter().map(|s| s.to_string()).collect(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
        }
    }

    // 每个测试用独立子目录（tag 唯一），避免并行测试互相 remove_dir_all 清掉对方文件。
    fn test_subdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("cc-view-perm-{}", std::process::id()))
            .join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    // 在 dir 下写一个 settings 文件，返回路径。
    fn write_in(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn allow_tool_not_needs() {
        assert!(!pc(&["Read"], &[], &[]).needs_permission("Read", None));
    }
    #[test]
    fn ask_bash_pattern_needs() {
        assert!(pc(&[], &["Bash(kill *)"], &[]).needs_permission("Bash", Some("kill 123")));
    }
    #[test]
    fn deny_tool_not_needs() {
        assert!(!pc(&[], &[], &["Read(.env*)"]).needs_permission("Read", None));
    }
    #[test]
    fn unknown_tool_not_needs() {
        // 翻转后：未知工具不再报警（修复 Agent/ToolSearch 误报）
        assert!(!pc(&["Read"], &[], &[]).needs_permission("Write", None));
    }
    #[test]
    fn allow_bash_all_not_needs() {
        assert!(!pc(&["Bash"], &[], &[]).needs_permission("Bash", Some("ls")));
    }

    // Bash pattern 按命令精确匹配（修复 starts_with("Bash(") 误匹配所有 Bash 的 bug）。
    // 用户的真实配置：deny Bash(rm -rf *) + ask Bash(kill *)，两者不能互相掩盖。
    #[test]
    fn bash_pattern_matches_command_not_all_bash() {
        let checker = pc(&[], &["Bash(kill *)"], &["Bash(rm -rf *)"]);
        // kill 命令命中 ask → 需权限（修复前 deny Bash(rm -rf *) 短路掩盖 → 漏报）
        assert!(checker.needs_permission("Bash", Some("kill 123")));
        // rm -rf 命中 deny → 不需权限
        assert!(!checker.needs_permission("Bash", Some("rm -rf /tmp")));
        // 普通命令（ls）既不 ask 也不 deny → 不需权限
        assert!(!checker.needs_permission("Bash", Some("ls")));
    }

    // 内置工具（Agent/ToolSearch/Skill/TodoWrite）即使不在 allow 也不报警
    #[test]
    fn builtin_tools_not_needs() {
        let checker = pc(&["Read"], &[], &[]); // 仅 Read 在 allow
        assert!(!checker.needs_permission("Agent", None));
        assert!(!checker.needs_permission("ToolSearch", None));
        assert!(!checker.needs_permission("Skill", None));
        assert!(!checker.needs_permission("TodoWrite", None));
    }

    // 三层合并：local 层 allow 覆盖未知——修复 Skill(gstack) 仅在 local 的误报
    #[test]
    fn merge_local_allow_suppresses_unknown() {
        let dir = test_subdir("local_allow");
        let user = write_in(&dir, "user.json", r#"{"permissions":{"allow":["Read"]}}"#);
        let local = write_in(&dir, "local.json", r#"{"permissions":{"allow":["Skill(gstack)"]}}"#);
        let checker = merge_layers(&user, None, Some(&local)).unwrap();
        // Skill 在 local allow → 不报警（翻转前：Skill 未知 → 误报 true）
        assert!(!checker.needs_permission("Skill", None));
        assert!(!checker.needs_permission("Read", None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 三层合并：local 层的 ask 生效（不同项目可能有自己的 ask 规则）
    #[test]
    fn merge_ask_from_local_layer() {
        let dir = test_subdir("ask_local");
        let user = write_in(&dir, "u.json", r#"{"permissions":{"allow":["Read"]}}"#);
        let local = write_in(&dir, "l.json", r#"{"permissions":{"ask":["Bash(kill *)"]}}"#);
        let checker = merge_layers(&user, None, Some(&local)).unwrap();
        assert!(checker.needs_permission("Bash", Some("kill 1"))); // local ask 生效
        assert!(!checker.needs_permission("Read", None)); // user allow 生效
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 三层全无 permissions 段 → None（无配置不猜测，调用方跳过判定）
    #[test]
    fn merge_none_when_no_permissions_anywhere() {
        let dir = test_subdir("no_perms");
        let user = write_in(&dir, "nop.json", r#"{"theme":"dark"}"#);
        assert!(merge_layers(&user, None, None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 三层合并：user allow + local ask 同时存在，各司其职
    #[test]
    fn merge_user_allow_and_local_ask_coexist() {
        let dir = test_subdir("allow_ask");
        let user = write_in(&dir, "u.json", r#"{"permissions":{"allow":["Bash"]}}"#);
        let local =
            write_in(&dir, "l.json", r#"{"permissions":{"ask":["Bash(git push --force *)"]}}"#);
        let checker = merge_layers(&user, None, Some(&local)).unwrap();
        // 普通 Bash：user allow → 不报警
        assert!(!checker.needs_permission("Bash", Some("ls")));
        // ask 命中的 Bash：仍报警（ask 优先于 allow）
        assert!(checker.needs_permission("Bash", Some("git push --force origin main")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
