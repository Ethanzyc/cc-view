// 读取 ~/.claude/settings.json 的 permissions 配置，预测工具调用是否需要用户确认。
// 判定优先级：deny → ask → allow → 未知（保守 true，需确认）。

pub struct PermissionChecker {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl PermissionChecker {
    // 从 ~/.claude/settings.json 读取 permissions 字段，缺失任一字段则视为空列表。
    // 解析失败、文件不存在、没有 permissions 段都返回 None。
    pub fn from_settings() -> Option<Self> {
        let path = dirs::home_dir()?.join(".claude/settings.json");
        let txt = std::fs::read_to_string(&path).ok()?;
        #[derive(serde::Deserialize)]
        struct Root { permissions: Option<Perms> }
        #[derive(serde::Deserialize)]
        struct Perms { allow: Option<Vec<String>>, ask: Option<Vec<String>>, deny: Option<Vec<String>> }
        let p: Root = serde_json::from_str(&txt).ok()?;
        let perms = p.permissions?;
        Some(Self {
            allow: perms.allow.unwrap_or_default(),
            ask: perms.ask.unwrap_or_default(),
            deny: perms.deny.unwrap_or_default(),
        })
    }

    // 按 deny → ask → allow → 未知(保守 true) 优先级判定。
    pub fn needs_permission(&self, name: &str, bash_command: Option<&str>) -> bool {
        if matches_entry(&self.deny, name, bash_command) { return false; }
        if matches_entry(&self.ask, name, bash_command) { return true; }
        if matches_entry(&self.allow, name, bash_command) { return false; }
        true // 未知工具：保守需确认
    }
}

fn matches_entry(list: &[String], name: &str, bash_command: Option<&str>) -> bool {
    list.iter().any(|e| entry_matches(e, name, bash_command))
}

fn entry_matches(entry: &str, name: &str, bash_command: Option<&str>) -> bool {
    // 工具名精确，或 "Tool(" 前缀（如 Read(.env*) 视为匹配 Read 工具）
    if entry == name || entry.starts_with(&format!("{}(", name)) { return true; }
    if name == "Bash" {
        if entry == "Bash" { return true; }
        if let Some(pat) = entry.strip_prefix("Bash(").and_then(|s| s.strip_suffix(")")) {
            if let Some(cmd) = bash_command {
                if let Some(prefix) = pat.strip_suffix("*") {
                    return cmd.starts_with(prefix.trim_end());
                }
                return cmd.contains(pat);
            }
        }
    }
    false
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
    #[test]
    fn allow_tool_not_needs() { assert!(!pc(&["Read"], &[], &[]).needs_permission("Read", None)); }
    #[test]
    fn ask_bash_pattern_needs() {
        assert!(pc(&[], &["Bash(kill *)"], &[]).needs_permission("Bash", Some("kill 123")));
    }
    #[test]
    fn deny_tool_not_needs() { assert!(!pc(&[], &[], &["Read(.env*)"]).needs_permission("Read", None)); }
    #[test]
    fn unknown_tool_needs() { assert!(pc(&["Read"], &[], &[]).needs_permission("Write", None)); }
    #[test]
    fn allow_bash_all_not_needs() { assert!(!pc(&["Bash"], &[], &[]).needs_permission("Bash", Some("ls"))); }
}
