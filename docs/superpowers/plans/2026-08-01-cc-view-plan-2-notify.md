# cc-view Plan 2: 不漏"等我"通知闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 当某 Claude Code 会话进入 NeedsPermission / WaitingForInput 时，cc-view 弹出 macOS 通知（不漏"等我"）。核心是 PermissionChecker 预测 + JSONL 末尾 pending tool_use 检测 + 状态迁移通知。

**Architecture:** 后端每 3s 轮询时，对每个会话读 `~/.claude/projects/<proj>/<sid>.jsonl` 末尾，找最后一个无 `tool_result` 的 `tool_use`；用 PermissionChecker（读 `~/.claude/settings.json` 的 permissions 白名单）预测该 tool 是否需用户确认 → 真实 pending_permission 喂给状态机。Notifier 跟踪每会话上次状态，迁移到 NeedsPermission/WaitingInput 时发 osascript 通知（防抖：仅迁移触发）。

**Tech Stack:** Rust（serde_json / libc / dirs / std::io Seek），macOS `osascript` 通知，cargo test。

## Global Constraints

- 平台 macOS；禁用 App Sandbox（已在 Plan 1）。
- 零侵入：只读 `~/.claude/`，不装 hook。
- 路径用 `dirs::home_dir()`，禁止硬编码 `/Users/zhuyuchen`。
- 代码英文/注释中文；fail fast（解析失败隔离不崩）。
- 纯逻辑模块（permission / notify observe / read_pending）用 TDD。
- 权限判定**保守方向**：不确定就 NeedsPermission（宁可多通知，不漏"等我"）。

## Out of Scope（留 Plan 3）

隐藏/归档(E) · focus 跳转(C) · roster.json 后台源 · Compacting 状态 · `claude agents --json` 源 · per-session 静音 · Otty/cmux/iTerm2 focus · 桌面 Widget。

---

## File Structure

后端 `src-tauri/src/`：
- `collector.rs`（**改**）— 加 `read_pending_tool_use` + `PendingToolUse`
- `permission.rs`（**新**）— `PermissionChecker`（TDD）
- `notify.rs`（**新**）— `Notifier`（TDD）
- `lib.rs`（**改**）— `collect_sessions` 接入 pending；`start_poll_loop` 接 Notifier
- `tests/fixtures/`（**新**）— jsonl 片段

---

### Task 1: read_pending_tool_use（解析 JSONL 末尾 pending tool_use，TDD）

**Files:**
- Modify: `src-tauri/src/collector.rs`（加 `PendingToolUse` + `read_pending_tool_use`）
- Create: `src-tauri/tests/fixtures/pending.jsonl`、`completed.jsonl`
- Modify: `src-tauri/src/lib.rs`（无新 mod，collector 已声明）

**Interfaces:**
- Consumes: `dirs::home_dir`、`std::io::{Seek, SeekFrom, Read}`、`serde_json`
- Produces: `pub struct PendingToolUse { pub name: String, pub bash_command: Option<String> }`；`pub fn read_pending_tool_use(session_id: &str, cwd: &str) -> Option<PendingToolUse>`

**JSONL 格式（已实测确认）**：每行一条消息。assistant 行 `message.content[]` 含 `{type:"tool_use",id,name,input}`（Bash 的 `input.command` 是命令）；user 行 `message.content[]` 含 `{type:"tool_result",tool_use_id}`。pending = 最后一个 tool_use 的 id 未在后续 tool_result 中出现。

- [ ] **Step 1: 建 fixture**

`src-tauri/tests/fixtures/pending.jsonl`（最后一个 tool_use 无 tool_result）：
```
{"message":{"role":"assistant","content":[{"type":"tool_use","id":"call_a","name":"Read","input":{"file_path":"/x"}}]}}
{"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_a","content":"ok"}]}}
{"message":{"role":"assistant","content":[{"type":"tool_use","id":"call_b","name":"Bash","input":{"command":"kill 1"}}]}}
```
`src-tauri/tests/fixtures/completed.jsonl`（tool_use 有对应 tool_result）：
```
{"message":{"role":"assistant","content":[{"type":"tool_use","id":"call_a","name":"Read","input":{}}]}}
{"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_a","content":"ok"}]}}
```

- [ ] **Step 2: 写失败测试**

在 `collector.rs` 的 `#[cfg(test)] mod tests` 加：
```rust
#[test]
fn pending_tool_use_detected() {
    // 用 fixture 内容直接喂解析函数（不读磁盘，测纯解析）
    let jsonl = std::fs::read_to_string("tests/fixtures/pending.jsonl").unwrap();
    let p = super::parse_pending_from_str(&jsonl).unwrap();
    assert_eq!(p.name, "Bash");
    assert_eq!(p.bash_command.as_deref(), Some("kill 1"));
}
#[test]
fn no_pending_when_completed() {
    let jsonl = std::fs::read_to_string("tests/fixtures/completed.jsonl").unwrap();
    assert!(super::parse_pending_from_str(&jsonl).is_none());
}
```

- [ ] **Step 3: 跑确认失败**

Run: `cargo test collector`
Expected: 2 FAIL（`parse_pending_from_str` 未定义）。

- [ ] **Step 4: 实现**

在 `collector.rs` 加：
```rust
pub struct PendingToolUse {
    pub name: String,
    pub bash_command: Option<String>,
}

#[derive(serde::Deserialize)]
struct JsonlRow { message: Option<JsonlMessage> }
#[derive(serde::Deserialize)]
struct JsonlMessage { content: Option<Vec<ContentItem>> }
#[derive(serde::Deserialize)]
struct ContentItem {
    #[serde(rename = "type")] item_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    tool_use_id: Option<String>,
}

/// 从 JSONL 文本解析最后一个未完成（无 tool_result）的 tool_use。纯函数，便于测试。
pub fn parse_pending_from_str(text: &str) -> Option<PendingToolUse> {
    let mut tool_uses: Vec<(String, String, Option<String>)> = Vec::new(); // (id, name, bash_cmd)
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<JsonlRow>(line) else { continue };
        let Some(msg) = row.message else { continue };
        let Some(items) = msg.content else { continue };
        for it in items {
            match it.item_type.as_deref() {
                Some("tool_use") => {
                    if let (Some(id), Some(name)) = (it.id.clone(), it.name.clone()) {
                        let bash_cmd = if name == "Bash" {
                            it.input.as_ref()
                                .and_then(|v| v.get("command"))
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        } else { None };
                        tool_uses.push((id, name, bash_cmd));
                    }
                }
                Some("tool_result") => {
                    if let Some(tuid) = it.tool_use_id { completed.insert(tuid); }
                }
                _ => {}
            }
        }
    }
    tool_uses.iter().rev()
        .find(|(id, _, _)| !completed.contains(id))
        .map(|(_, name, cmd)| PendingToolUse { name: name.clone(), bash_command: cmd.clone() })
}

/// 读 ~/.claude/projects/<encoded-cwd>/<session-id>.jsonl 末尾 ~8KB，返回 pending tool_use。
pub fn read_pending_tool_use(session_id: &str, cwd: &str) -> Option<PendingToolUse> {
    use std::io::{Read, Seek, SeekFrom};
    let home = dirs::home_dir()?;
    let encoded = cwd.replace('/', "-"); // /Users/x -> -Users-x
    let path = home.join(".claude/projects").join(&encoded).join(format!("{}.jsonl", session_id));
    let mut f = std::fs::File::open(&path).ok()?;
    let size = f.metadata().ok()?.len();
    let start = size.saturating_sub(8192);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).ok()?;
    // 若 seek 到非 0，首行可能截断，跳过
    let text = if start > 0 {
        buf.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else { buf };
    parse_pending_from_str(&text)
}
```

- [ ] **Step 5: 跑确认通过**

Run: `cargo test collector`
Expected: 全 PASS（含原 collector 测试 + 2 新测试）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/collector.rs src-tauri/tests/fixtures/
git commit -m "feat: detect pending tool_use from JSONL tail"
```

---

### Task 2: PermissionChecker（TDD）

**Files:**
- Create: `src-tauri/src/permission.rs`
- Modify: `src-tauri/src/lib.rs`（`mod permission;`）

**Interfaces:**
- Consumes: `dirs::home_dir`、`serde_json`
- Produces: `pub struct PermissionChecker { allow, ask, deny }`；`PermissionChecker::from_settings() -> Option<Self>`；`needs_permission(&self, name: &str, bash_command: Option<&str>) -> bool`

**判定规则（保守）**：deny 匹配 → false（自动拒，不等待）；ask 匹配 → true；allow 匹配 → false；都不匹配 → true（未知保守需确认）。匹配：工具名精确（`"Read"`）或 `Tool(` 前缀（`Read(.env*)` 视为匹配 Read 工具）；Bash 特殊：`"Bash"` 全匹配，`Bash(pattern)` 中 `pattern` 以 `*` 结尾取前缀 starts_with，否则 contains。

- [ ] **Step 1: 写失败测试**

`permission.rs`：
```rust
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
```

- [ ] **Step 2: 跑确认失败**

Run: `cargo test permission`
Expected: FAIL（结构/方法未定义）。

- [ ] **Step 3: 实现**

`permission.rs`（替换上面的占位 import）：
```rust
pub struct PermissionChecker {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl PermissionChecker {
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
    // 工具名精确，或 "Tool(..." 前缀（如 Read(.env*) 视为匹配 Read 工具）
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
```

`lib.rs` 加 `mod permission;`。

- [ ] **Step 4: 跑确认通过**

Run: `cargo test permission`
Expected: 5 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/permission.rs src-tauri/src/lib.rs
git commit -m "feat: PermissionChecker predicts if tool_use needs user approval"
```

---

### Task 3: collect_sessions 接入真实 pending_permission

**Files:**
- Modify: `src-tauri/src/collector.rs`（`collect_sessions` 内重算 status）

**Interfaces:**
- Consumes: `read_pending_tool_use`、`PermissionChecker::from_settings`、`statemachine::decide`
- Produces: `collect_sessions` 现在产出带真实 NeedsPermission 的 Session

- [ ] **Step 1: 改 collect_sessions**

在 `collect_sessions` 里，`parse_session_file` 成功后、`push` 前，加：
```rust
// 真实权限判定：读 JSONL 末尾 pending tool_use + PermissionChecker 预测
let pending = crate::permission::PermissionChecker::from_settings();
if let (Some(pc), Some(p)) = (&pending, crate::collector::read_pending_tool_use(&s.id, &s.cwd)) {
    if pc.needs_permission(&p.name, p.bash_command.as_deref()) {
        // 重算为 NeedsPermission
        let mut s = s;
        s.status = crate::statemachine::decide(&crate::statemachine::DecideInput {
            raw_status: "", // pending_permission 优先，raw_status 留空
            pending_permission: true,
        });
        out.push(s);
        continue;
    }
}
out.push(s);
```
> 注意：`parse_session_file` 返回的 `s` 此时需可变（`let mut s`）。调整原代码让 s 可变。`PermissionChecker::from_settings` 每 3s 读一次 settings.json，可接受。

- [ ] **Step 2: 跑测试**

Run: `cargo test` + `cargo check`
Expected: 全 PASS，无新 error。`collect_sessions_runs_against_real_dir` 仍通过（真实目录可能无 pending → 不影响）。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/collector.rs
git commit -m "feat: wire PermissionChecker into collect_sessions for real NeedsPermission"
```

---

### Task 4: Notifier（状态迁移检测，TDD）

**Files:**
- Create: `src-tauri/src/notify.rs`
- Modify: `src-tauri/src/lib.rs`（`mod notify;`）

**Interfaces:**
- Consumes: `models::{Session, Status}`
- Produces: `pub struct Notifier`；`Notifier::new()`；`observe(&mut self, sessions: &[Session]) -> Vec<(String, Status)>`（返回本次需通知的 (name, status)，纯逻辑可测）

- [ ] **Step 1: 写失败测试**

`notify.rs`：
```rust
use crate::models::{Session, Status, Source, FocusHint};
use std::collections::HashMap;

pub struct Notifier { last: HashMap<String, Status> }

impl Notifier {
    pub fn new() -> Self { Self { last: HashMap::new() } }

    /// 返回本次新迁移到 NeedsPermission/WaitingInput 的 (name, status)。纯逻辑。
    pub fn observe(&mut self, sessions: &[Session]) -> Vec<(String, Status)> {
        let mut to_notify = Vec::new();
        let mut cur = HashMap::new();
        for s in sessions {
            cur.insert(s.id.clone(), s.status.clone());
            if matches!(s.status, Status::NeedsPermission | Status::WaitingForInput) {
                if self.last.get(&s.id) != Some(&s.status) {
                    to_notify.push((s.name.clone(), s.status.clone()));
                }
            }
        }
        self.last = cur;
        to_notify
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sess(id: &str, st: Status) -> Session {
        Session { id: id.into(), source: Source::Interactive, pid: 1, project: "p".into(),
            cwd: "/c".into(), name: id.into(), status: st, started_at: 0, status_updated_at: 0,
            alive: true, focus_hint: FocusHint::default() }
    }
    #[test]
    fn first_permission_triggers() {
        let mut n = Notifier::new();
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert_eq!(r.len(), 1);
    }
    #[test]
    fn same_status_no_renotify() {
        let mut n = Notifier::new();
        n.observe(&[sess("a", Status::NeedsPermission)]);
        let r = n.observe(&[sess("a", Status::NeedsPermission)]);
        assert!(r.is_empty()); // 防抖
    }
    #[test]
    fn working_not_notified() {
        let mut n = Notifier::new();
        let r = n.observe(&[sess("a", Status::Working)]);
        assert!(r.is_empty());
    }
}
```

- [ ] **Step 2: 跑确认通过**（实现已含在 Step 1）

Run: `cargo test notify`
Expected: 3 PASS。

`lib.rs` 加 `mod notify;`。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/notify.rs src-tauri/src/lib.rs
git commit -m "feat: Notifier detects status transitions (debounced)"
```

---

### Task 5: 轮询集成（Notifier + osascript 通知）

**Files:**
- Modify: `src-tauri/src/notify.rs`（加 `send_notification`）
- Modify: `src-tauri/src/lib.rs`（`start_poll_loop` 接 Notifier）

**Interfaces:**
- Consumes: `Notifier`、`std::process::Command`（osascript）

- [ ] **Step 1: 加 send_notification**

`notify.rs` 加：
```rust
/// 发 macOS 通知（osascript）。msg/title 不含双引号（调用方确保）。
pub fn send_notification(title: &str, msg: &str) {
    let script = format!("display notification \"{}\" with title \"{}\"", msg, title);
    let _ = std::process::Command::new("osascript").arg("-e").arg(script).spawn();
}
```

- [ ] **Step 2: start_poll_loop 接入 Notifier**

`lib.rs` 的 `start_poll_loop`：线程内持有 `let mut notifier = notify::Notifier::new();`，每轮 reduce 后：
```rust
let to_notify = notifier.observe(&merged);
for (name, status) in to_notify {
    let status_zh = match status {
        models::Status::NeedsPermission => "等待权限确认",
        models::Status::WaitingForInput => "等待输入",
        _ => "需要关注",
    };
    notify::send_notification("cc-view", &format!("{}：{}", name, status_zh));
}
```
（放在 hash 比较之前——即使 hash 没变也观察？不：observe 依赖每轮调用以更新内部状态；放在 reduce 之后、hash 之前，每轮都 observe。）

> 注意：notifier 在 `move ||` 闭包内需可变，`let mut notifier` 在闭包内声明即可（线程私有，无需 Mutex）。

- [ ] **Step 3: 构建验证**

Run: `cargo build`
Expected: 通过。无 GUI 验证（通知弹出留 Task 6 冒烟）。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/notify.rs src-tauri/src/lib.rs
git commit -m "feat: poll loop fires macOS notification on status transition"
```

---

### Task 6: 冒烟 + README 更新

**Files:**
- Modify: `README.md`（移除"无通知"限制项）

- [ ] **Step 1: 技术冒烟**

后台起 `npm run tauri dev`，确认编译 + 进程启动无 panic，pkill 干净退出（通知真实弹出由用户在真实等权限场景验证）。

- [ ] **Step 2: 更新 README**

`README.md` 的"已知限制（Plan 1）"节：移除"无通知（A，Plan 2）"和"NeedsPermission 暂不区分"两条（Plan 2 已实现）。改节标题为"已知限制"。保留仍属 Plan 3 的项（无 focus、无隐藏、无 roster 源、Compacting、popover 精修）。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: update known limits after Plan 2 notifications"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 2 覆盖 spec §8 A（通知）、§7 permission/discovery（PermissionChecker）、§6 NeedsPermission 真实判定、§9（解析失败隔离）。Out of scope 项（C focus / E 隐藏 / roster / Compacting）明确列 Plan 3。✅
- **Placeholder scan**：无 TBD；JSONL 格式基于实测（Task 1 已确认 tool_use/tool_result 结构）；PermissionChecker 规则明确；osascript 通知代码完整。✅
- **Type consistency**：`PendingToolUse{name, bash_command}` / `PermissionChecker::needs_permission(name, bash_command)` / `Notifier::observe(&[Session])->Vec<(String,Status)>` 跨任务签名一致；`Status` 复用 models。✅
