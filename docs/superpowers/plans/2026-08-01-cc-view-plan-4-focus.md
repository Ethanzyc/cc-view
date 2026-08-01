# cc-view Plan 4: MVP focus + Compacting + agents --json Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** ① 点击 popover 某会话 → 跳到承载它的终端 app（MVP focus，sysinfo 进程树 + osascript activate）；② 补 Compacting 状态；③ 接入 `claude agents --json` 替换 roster 的默认 Working（更准的 busy/idle）。

**Architecture:** discovery 用 sysinfo 从 session.pid 爬父进程链（最多 8 层）匹配终端 app 名/exe → FocusHint.host；focus 模块按 host 调 osascript `activate`；前端点击 → invoke focus_session(id)。Compacting 由 JSONL 末尾 compact 标志判定。agents --json 输出解析后用其 busy/idle 覆盖 roster worker 的默认 Working。

**Tech Stack:** Rust（sysinfo / serde_json / std::process::Command）、osascript、cargo test。

## Global Constraints

- macOS；零侵入；路径 dirs::home_dir()；代码英文/注释中文；fail fast。
- MVP focus 只做到 **activate 终端 app**（不精确到 tab/pane）——精细版留 Plan 5。
- sysinfo 0.32 API 以官方文档为准（Process::parent/name/exe）；若签名不同，按实际调整。
- `claude agents --json` 输出格式需实测（spec §14 open question）——Task 5 先跑命令确认 schema 再解析。

## Out of Scope（留可选 Plan 5）

精细 focus：iTerm2 AppleScript 选 tab/session、Ghostty 按 cwd 选 terminal、tmux select-pane、Otty `otty://pane/<id>`、cmux AppleScript focus terminal。进程 env 读取（KERN_PROCARGS2）。

---

## File Structure

后端 `src-tauri/src/`：
- `discovery.rs`（**新**）— sysinfo 进程树 → Host
- `focus.rs`（**新**）— osascript activate + focus_session command
- `collector.rs`（**改**）— collect_sessions 设 focus_hint.host；加 agents --json 源
- `statemachine.rs`（**改**）— 加 Compacting 判定
- `lib.rs`（**改**）— mod discovery/focus；focus_session command 注册

前端 `src/`：
- `components/SessionList.vue`（**改**）— 点击行调 invoke focus_session

---

### Task 1: discovery（sysinfo 进程树 → Host）

**Files:**
- Create: `src-tauri/src/discovery.rs`
- Modify: `src-tauri/src/lib.rs`（`mod discovery;`）、`src-tauri/src/collector.rs`（collect_sessions 设 host）

**Interfaces:**
- Consumes: `sysinfo`、`models::Host`
- Produces: `pub fn detect_host(pid: u32) -> Host`

- [ ] **Step 1: 实现**

`discovery.rs`：
```rust
use crate::models::Host;
use sysinfo::{Pid, ProcessRefreshKind, System};

/// 从 pid 爬父进程链（最多 8 层），按进程名/exe 匹配终端 app。
pub fn detect_host(pid: u32) -> Host {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::new().with_exe(UpdateKind::Always), true);
    let mut current = Pid::from_u32(pid);
    for _ in 0..8 {
        let Some(p) = sys.process(current) else { return Host::Unknown; };
        let name = p.name().to_string_lossy().to_lowercase();
        let exe = p.exe().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if let Some(host) = match_host(&name, &exe) { return host; }
        match p.parent() {
            Some(parent) => current = parent,
            None => return Host::Unknown,
        }
    }
    Host::Unknown
}

fn match_host(name: &str, exe: &str) -> Option<Host> {
    // 进程名通常是小写的 app 名（iTerm2 的进程名是 "iTerm2" 或 "iTerm Server"）
    let hay = format!("{name} {exe}");
    let m = |k: &str| hay.contains(k);
    if m("iterm") { Some(Host::ITerm2) }
    else if m("ghostty") { Some(Host::Ghostty) }
    else if m("code") { Some(Host::Vscode) }
    else if m("intellij") || m("idea") { Some(Host::Idea) }
    else if m("otty") { Some(Host::Otty) }
    else if m("cmux") { Some(Host::Cmux) }
    else if m("tmux") { Some(Host::Tmux) }
    else if m("warp") { Some(Host::Warp) }
    else if m("terminal") { Some(Host::Terminal) }
    else { None }
}
```
> sysinfo 0.32：`refresh_processes_specifics` + `ProcessRefreshKind::new().with_exe(...)` + `UpdateKind`。若 API 不同（如 `refresh_processes` 已含 exe），按编译提示调整。`with_exe` 是为拿 `p.exe()`。

- [ ] **Step 2: collect_sessions 设 host**

collector.rs：parse_session_file 后、push 前，对 interactive session（有 pid）设 host：
```rust
s.focus_hint.host = crate::discovery::detect_host(pid);
```
（roster worker 也可设：`w.focus_hint.host = crate::discovery::detect_host(w.pid);`）

- [ ] **Step 3: 编译 + 测试**

Run: `cargo test` + `cargo check`
Expected: 通过。sysinfo API 正确。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/discovery.rs src-tauri/src/collector.rs src-tauri/src/lib.rs
git commit -m "feat: detect host terminal via sysinfo process tree"
```

---

### Task 2: focus 模块（osascript activate）+ focus_session command

**Files:**
- Create: `src-tauri/src/focus.rs`
- Modify: `src-tauri/src/lib.rs`（`mod focus;` + focus_session command + invoke_handler）

**Interfaces:**
- Consumes: `models::Host`、`hidden::HiddenList`（无）、Tauri State（sessions 缓存）
- Produces: `pub fn activate_host(host: &Host)`；Tauri command `focus_session(id: String, state)`

- [ ] **Step 1: focus.rs**

```rust
use crate::models::Host;
use std::process::Command;

/// MVP focus：osascript activate 终端 app（不精确到 tab/pane）。
pub fn activate_host(host: &Host) {
    let app = match host {
        Host::ITerm2 => "iTerm2",
        Host::Ghostty => "Ghostty",
        Host::Vscode => "Visual Studio Code",
        Host::Idea => "IntelliJ IDEA",
        Host::Otty => "Otty",
        Host::Cmux => "cmux",
        Host::Tmux => "Terminal", // tmux 跑在某个终端里，MVP 激活 Terminal 作兜底
        Host::Warp => "Warp",
        Host::Terminal => "Terminal",
        Host::Unknown => return, // 未知 host 不动作
    };
    let script = format!("tell application \"{}\" to activate", app);
    let _ = Command::new("osascript").arg("-e").arg(script).spawn();
}
```

- [ ] **Step 2: focus_session command**

lib.rs：command 接收 session id，从最近 sessions 快照查 host，调 activate。需要缓存最近 emit 的 sessions（用 `Mutex<Vec<Session>>`）：
```rust
use std::sync::Mutex;

#[tauri::command]
fn focus_session(id: String, cache: tauri::State<'_, Mutex<Vec<models::Session>>>) {
    if let Ok(sessions) = cache.lock() {
        if let Some(s) = sessions.iter().find(|s| s.id == id) {
            focus::activate_host(&s.focus_hint.host);
        } else {
            eprintln!("focus_session: session {} not in cache", id);
        }
    }
}
```
- 注册：`.manage(Mutex::new(Vec::<models::Session>::new()))`（与 hidden 的 Mutex 并列）。
- start_poll_loop 里 emit 前更新 cache：`if let Ok(mut c) = cache.lock() { *c = merged.clone(); }`（需把 cache handle 传入线程，或用 app handle 取 state）。简单做法：emit 时同时 `app.state::<Mutex<Vec<Session>>>()` 更新。

- [ ] **Step 3: invoke_handler 注册 focus_session**

`.invoke_handler(tauri::generate_handler![hide_session, unhide_session, list_hidden, focus_session])`

- [ ] **Step 4: 构建**

Run: `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/focus.rs src-tauri/src/lib.rs
git commit -m "feat: MVP focus — activate host terminal app"
```

---

### Task 3: 前端点击行 → invoke focus_session

**Files:**
- Modify: `src/components/SessionList.vue`

- [ ] **Step 1: 行点击调 focus**

SessionList.vue 模板的 `<li>` 加 `@click="focus(s.id)"`（避开 hide/unhide 按钮的点击冒泡——按钮加 `@click.stop`）：
```vue
<li v-for="s in sessions" :key="s.id" :class="{ dead: !s.alive }" @click="focus(s.id)">
  ...
  <button class="hide-btn" @click.stop="hide(s.id)" title="隐藏">×</button>
  <button v-if="hidden.includes(s.id)" class="hide-btn" @click.stop="unhide(s.id)" title="恢复">+</button>
</li>
```
script 加：
```ts
async function focus(id: string) {
  try { await invoke('focus_session', { id }); }
  catch (e) { console.error('focus failed', e); }
}
```

- [ ] **Step 2: 构建**

Run: `npm run build` + `cargo build`
Expected: 通过。

- [ ] **Step 3: Commit**

```bash
git add src/components/SessionList.vue
git commit -m "feat: click session row to focus host terminal"
```

---

### Task 4: Compacting 状态（statemachine + JSONL 检测）

**Files:**
- Modify: `src-tauri/src/statemachine.rs`、`src-tauri/src/collector.rs`（read_pending 顺便检测 compact）、`src-tauri/src/models.rs`（Status 加 Compacting）

**注意**：Compacting JSONL 标志格式需实测确认（gmr 调研：`type:system, subtype:compact_boundary` 或 agentId 以 `acompact-` 开头）。Task 先实测一个真实 autocompact 的 JSONL 行确认，再实现判定。

- [ ] **Step 1: models.rs Status 加 Compacting**

```rust
pub enum Status {
    Working, WaitingForInput, NeedsPermission, Shell,
    Compacting, // autocompact 进行中
}
```
（前端 types.ts Status 联合加 `'compacting'`，SessionList icon 加 `compacting: '🧹'`）

- [ ] **Step 2: collector 检测 compact**

在 `parse_pending_from_str` 附近加 `detect_compacting(text: &str) -> bool`：扫 JSONL 末尾，若任行含 `compact_boundary` 或 agentId 以 `acompact-` 开头 → true。collect_sessions 里 read_pending 后顺便检测：
```rust
// 末尾文本复用 read_pending 的读取，或单独读末尾检测
let is_compacting = read_jsonl_tail(&s.id, &s.cwd).map(|t| detect_compacting(&t)).unwrap_or(false);
if is_compacting { s.status = Status::Compacting; }
```
> 注：read_pending_tool_use 目前返回 Option<PendingToolUse> 不返回 text。可加 `read_jsonl_tail_text(session_id, cwd) -> Option<String>` 返回末尾文本，供 pending + compact 共用。

- [ ] **Step 3: 测试**

statemachine 测试加 compact 优先级（compact 高于其他，除了... 实际 compact 时 session 在 autocompact，应显示 Compacting）。collector 测试加 detect_compacting（fixture 含 compact_boundary 行 → true）。

- [ ] **Step 4: 跑测试 + Commit**

```bash
cargo test
git add ...
git commit -m "feat: Compacting status from JSONL compact markers"
```

---

### Task 5: claude agents --json 源（实测 + 替换 roster 默认 Working）

**Files:**
- Modify: `src-tauri/src/collector.rs`

**注意**：先实测 `claude agents --json` 输出 schema（spec §14 open question）。

- [ ] **Step 1: 实测 schema**

Run: `claude agents --json` （或 `claude agents --json 2>&1 | head`），观察输出结构（sessionId/status busy|idle/cwd）。

- [ ] **Step 2: 实现 parse_agents**

collector.rs 加 `parse_agents(json: &str) -> Vec<Session>`（基于实测 schema）。status 映射：busy→Working，idle→WaitingForInput。read_agents() 跑 `claude agents --json` 命令拿 stdout。

- [ ] **Step 3: collect_sessions 合并 agents**

roster worker 若有对应 agents --json 条目，用 agents 的 status 覆盖 roster 默认 Working。简单做法：collect_sessions 合并 read_agents()，reducer 按 sessionId 去重时 agents 优先（push 顺序：sessions → roster → agents，agents 最后 = last-wins 覆盖）。

- [ ] **Step 4: 测试 + Commit**

fixture + parse_agents 测试。commit `feat: claude agents --json source for accurate fleet status`。

---

### Task 6: 冒烟 + README（Plan 4 收尾，全部功能完成）

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 技术冒烟**

`npm run tauri dev` 编译运行无 panic，pkill 干净退出。

- [ ] **Step 2: README**

更新"已知限制"——MVP focus 已实现（点击跳终端 app），精细 focus（tab/pane）标注为可选增强；移除 Compacting 未实现、agents --json 未接入。最终 README 反映"所有核心功能完成"。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: all core features complete (focus/compacting/agents source)"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 4 覆盖 §8 C focus（MVP activate）、§6 Compacting、§5.1 agents --json。精细 focus（每终端 AppleScript tab/pane）明确列 Plan 5。✅
- **Placeholder scan**：MVP focus 代码完整（sysinfo + osascript）；Compacting 标注实测 JSONL 格式；agents --json 标注实测 schema（Task 5 Step 1 先跑命令）——这两个是 spec §14 open question 的实测依赖，非占位。✅
- **Type consistency**：`detect_host(pid)->Host` / `activate_host(&Host)` / `focus_session(id, cache)` / `detect_compacting(&str)->bool` / `parse_agents(&str)->Vec<Session>` 跨任务一致；Status 加 Compacting。✅
- **注意**：Task 2 的 sessions cache（Mutex<Vec<Session>>）与 hidden 的 Mutex 并列；Task 4 compact 检测与 read_pending 共用末尾文本读取。
