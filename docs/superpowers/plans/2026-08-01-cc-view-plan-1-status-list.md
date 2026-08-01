# cc-view Plan 1: 实时会话状态列表（垂直切片） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一个 macOS menubar 常驻应用，点开 popover 能实时看到本机所有正在运行的 Claude Code 会话状态（Working/WaitingForInput/NeedsPermission/Shell）。

**Architecture:** Tauri 2 + Vue 3 前端 + Rust 后端。Rust 每 3s 轮询 `~/.claude/sessions/<pid>.json` + sysinfo 进程扫描，去重合并成统一 Session，纯函数算状态，内容 hash 去重后通过 Tauri event 推给 Vue popover。

**Tech Stack:** Tauri 2, Rust（sysinfo / serde / libc / dirs）, Vue 3 + Vite, cargo test / vitest。

## Global Constraints

- 平台：macOS（Darwin 25.x）。**禁用 App Sandbox**（proc_pidinfo / sysctl / osascript 需要）。
- 零侵入：只读 `~/.claude/`，**不装 hook、不写官方文件、不做 kill/删除**。
- 路径不硬编码：`~/.claude` 用 `dirs::home_dir()` 拼接，不写死 `/Users/zhuyuchen`。
- fail fast：解析失败隔离（skip + log），不吞异常、不崩整体。
- 纯逻辑模块（statemachine / reducer / parse）用 TDD。
- 交互/注释用中文，代码与标识符用英文。
- 依赖版本以 `cargo` / `npm` 当前兼容为准，下方为参考下限。

## Out of Scope（留给 Plan 2）

通知(A) · focus 跳转(C) · 隐藏归档(E) · `claude agents --json` / roster.json / JSONL 末尾数据源 · PermissionChecker · Compacting 状态 · Otty/cmux/iTerm2/Ghostty/tmux 精细 focus · NSPanel 不抢焦点精修 · 桌面 Widget。

---

## File Structure

后端 `src-tauri/src/`：
- `main.rs` — Tauri 入口、tray、轮询循环、emit
- `models.rs` — Session / Status / Source / Host / FocusHint 类型
- `statemachine.rs` — 纯函数 `decide()`（TDD）
- `collector.rs` — 解析 sessions 文件、扫 `~/.claude/sessions/`
- `liveness.rs` — pid 存活校验
- `reducer.rs` — 去重合并（TDD）

前端 `src/`：
- `App.vue` — 根组件，监听 `sessions` event
- `components/SessionList.vue` — 列表渲染
- `types.ts` — 前端 Session 类型（镜像 Rust models）

---

### Task 1: Tauri 2 + Vue 3 脚手架与 menubar 配置

**Files:**
- Create: 整个项目骨架（`package.json`, `src-tauri/`, `src/`, `index.html`）
- Modify: `src-tauri/tauri.conf.json`（tray + LSUIElement）
- Modify: `src-tauri/Cargo.toml`（依赖）

**Interfaces:** 无（首个任务）

- [ ] **Step 1: 用官方脚手架创建项目**

```bash
cd /Users/zhuyuchen/ai/cc-view
npm create tauri-app@latest . -- --template vue-ts --manager npm
```
选择：包管理 npm，前端 vue-ts，Rust 后端。若目录非空（已有 docs/），脚手架会询问，选继续。

- [ ] **Step 2: 确认能起 dev**

Run: `npm install && npm run tauri dev`
Expected: 弹出一个窗口（默认 Tauri 窗口，此刻还不是 menubar）。Ctrl+C 退出。

- [ ] **Step 3: 配置 menubar tray + 无 dock 图标**

`src-tauri/tauri.conf.json` 的 `app.windows` 设为 `[]`（无主窗口），`app.trayIcon` 添加一个 tray；`bundle.macOS` 加 `"exceptionDomain": ""`，并在 `Info.plist` 层面设 `LSUIElement = true`（无 dock 图标）。

在 `src-tauri/src/main.rs` 用 `tauri::tray::TrayIconBuilder` 创建托盘图标，点击时 `show`/`hide` 一个隐藏窗口作为 popover 容器：

```rust
use tauri::{Manager, tray::TrayIconBuilder, menu::MenuItem};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let item = MenuItem::with_id(app.handle(), "quit", "Quit", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app.handle(), &[&item])?;
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_tray_icon_event(|tray, event| {
                    // Task 9 接入 popover；此处先占位打印
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        println!("tray clicked");
                    }
                })
                .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    }
```

> LSUIElement：在 `tauri.conf.json` 的 `bundle.macOS` 下无法直接设，需在 `src-tauri/Info.plist`（或 `build.rs` 注入）加 `<key>LSUIElement</key><true/>`。执行时核对 [Tauri 2 macOS bundle 文档](https://v2.tauri.app/reference/config)。

- [ ] **Step 4: 验证 menubar 形态**

Run: `npm run tauri dev`
Expected: 顶部 menubar 出现图标，**dock 无图标**，无主窗口弹出。

- [ ] **Step 5: 添加 Rust 依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]`：
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sysinfo = "0.32"
libc = "0.2"
dirs = "5"
```

Run: `cd src-tauri && cargo check`
Expected: 编译通过。

- [ ] **Step 6: Commit**

```bash
git init && git add -A && git commit -m "feat: scaffold Tauri 2 + Vue menubar app"
```

---

### Task 2: 数据模型 models.rs

**Files:**
- Create: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/main.rs`（`mod models;`）

**Interfaces:**
- Produces: `Session`, `Status`, `Source`, `Host`, `FocusHint`（后续所有任务依赖）

- [ ] **Step 1: 写 models.rs**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Working,
    WaitingForInput,
    NeedsPermission,
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Interactive,
    Fleet,
    Slash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Host {
    #[default]
    Unknown,
    ITerm2,
    Ghostty,
    Vscode,
    Idea,
    Terminal,
    Otty,
    Cmux,
    Tmux,
    Warp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusHint {
    pub host: Host,
    pub iterm_session_id: Option<String>,
    pub tmux_pane: Option<String>,
    pub term_program: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub source: Source,
    pub pid: u32,
    pub project: String,
    pub cwd: String,
    pub name: String,
    pub status: Status,
    pub started_at: i64,
    pub status_updated_at: i64,
    pub alive: bool,
    pub focus_hint: FocusHint,
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 通过（在 main.rs 加 `mod models;`）。

- [ ] **Step 3: 写序列化测试**

`src-tauri/src/models.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_serializes_to_camel_case() {
        let s = Session {
            id: "x".into(), source: Source::Interactive, pid: 1,
            project: "p".into(), cwd: "/c".into(), name: "n".into(),
            status: Status::Working, started_at: 0, status_updated_at: 0,
            alive: true, focus_hint: FocusHint::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"statusUpdatedAt\""));
        assert!(json.contains("\"focusHint\""));
    }
}
```

Run: `cd src-tauri && cargo test models`
Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/main.rs
git commit -m "feat: add Session/Status models"
```

---

### Task 3: 状态机 statemachine.rs（TDD）

**Files:**
- Create: `src-tauri/src/statemachine.rs`
- Modify: `src-tauri/src/main.rs`（`mod statemachine;`）

**Interfaces:**
- Consumes: `models::Status`
- Produces: `fn decide(input: &DecideInput) -> Status`

- [ ] **Step 1: 写失败测试**

`src-tauri/src/statemachine.rs`：
```rust
use crate::models::Status;

#[derive(Debug)]
pub struct DecideInput<'a> {
    pub raw_status: &'a str,        // sessions.json 的 status 字段（busy/shell/idle/...）
    pub pending_permission: bool,   // PermissionChecker 结果（Plan 2 接入，此处由调用方给）
}

pub fn decide(input: &DecideInput) -> Status {
    Status::WaitingForInput // 占位，让测试失败
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
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test statemachine`
Expected: 3 个测试 FAIL（busy_is_working / shell_is_shell / permission_outranks_busy）。

- [ ] **Step 3: 写最小实现**

```rust
pub fn decide(input: &DecideInput) -> Status {
    if input.pending_permission {
        return Status::NeedsPermission;
    }
    match input.raw_status {
        "busy" => Status::Working,
        "shell" => Status::Shell,
        _ => Status::WaitingForInput,
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test statemachine`
Expected: 4 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/statemachine.rs src-tauri/src/main.rs
git commit -m "feat: status machine (Working/Waiting/Permission/Shell)"
```

---

### Task 4: sessions/<pid>.json 解析（TDD + fixture）

**Files:**
- Create: `src-tauri/src/collector.rs`
- Create: `src-tauri/tests/fixtures/session-busy.json`
- Modify: `src-tauri/src/main.rs`（`mod collector;`）

**Interfaces:**
- Consumes: `models::{Session, Status, Source, FocusHint}`, `statemachine::decide`
- Produces: `fn parse_session_file(pid: u32, json: &str) -> Result<Session, ParseError>`

- [ ] **Step 1: 建 fixture（取自真实文件）**

`src-tauri/tests/fixtures/session-busy.json`：
```json
{"pid":27074,"sessionId":"736f6944-db6d-4327-b4f3-a87154de33ec","cwd":"/Users/zhuyuchen/ai/cc-view","name":"cc-view-94","nameSource":"derived","status":"busy","startedAt":1785574426151,"procStart":"Sat Aug  1 08:53:45 2026","version":"2.1.201","peerProtocol":1,"kind":"interactive","entrypoint":"cli","statusUpdatedAt":1785575606089}
```

- [ ] **Step 2: 写失败测试**

`src-tauri/src/collector.rs`：
```rust
use crate::models::{Session, Source, Status, FocusHint};
use crate::statemachine::{decide, DecideInput};
use std::path::Path;

#[derive(Debug)]
pub enum ParseError {
    BadJson(serde_json::Error),
    MissingField,
}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self { ParseError::BadJson(e) }
}

#[derive(serde::Deserialize)]
struct RawSession {
    #[serde(rename = "sessionId")] session_id: String,
    cwd: String,
    name: Option<String>,
    status: Option<String>,
    #[serde(rename = "startedAt")] started_at: Option<i64>,
    #[serde(rename = "statusUpdatedAt")] status_updated_at: Option<i64>,
}

pub fn parse_session_file(pid: u32, json: &str) -> Result<Session, ParseError> {
    let raw: RawSession = serde_json::from_str(json)?;
    let status_str = raw.status.as_deref().unwrap_or("");
    let status = decide(&DecideInput { raw_status: status_str, pending_permission: false });
    let project = Path::new(&raw.cwd)
        .file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    Ok(Session {
        id: raw.session_id,
        source: Source::Interactive,
        pid,
        project,
        cwd: raw.cwd,
        name: raw.name.unwrap_or_default(),
        status,
        started_at: raw.started_at.unwrap_or(0),
        status_updated_at: raw.status_updated_at.unwrap_or(0),
        alive: true,
        focus_hint: FocusHint::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn parses_busy_session() {
        let json = fs::read_to_string("tests/fixtures/session-busy.json").unwrap();
        let s = parse_session_file(27074, &json).unwrap();
        assert_eq!(s.id, "736f6944-db6d-4327-b4f3-a87154de33ec");
        assert_eq!(s.status, Status::Working);
        assert_eq!(s.project, "cc-view");
        assert_eq!(s.name, "cc-view-94");
    }
    #[test]
    fn missing_status_defaults_to_waiting() {
        let json = r#"{"sessionId":"s","cwd":"/x/y"}"#;
        let s = parse_session_file(1, json).unwrap();
        assert_eq!(s.status, Status::WaitingForInput);
        assert_eq!(s.project, "y");
    }
}
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cd src-tauri && cargo test collector`
Expected: 2 PASS（实现已直接给出）。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/collector.rs src-tauri/tests/fixtures/session-busy.json src-tauri/src/main.rs
git commit -m "feat: parse ~/.claude/sessions/<pid>.json"
```

---

### Task 5: liveness（pid 存活校验）

**Files:**
- Create: `src-tauri/src/liveness.rs`
- Modify: `src-tauri/src/main.rs`（`mod liveness;`）

**Interfaces:**
- Produces: `fn is_claude_alive(pid: u32) -> bool`（kill(pid,0) + proc_pidpath 含 claude/node）

- [ ] **Step 1: 写实现（macOS libc FFI）**

`src-tauri/src/liveness.rs`：
```rust
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

/// pid 存活且其可执行路径含 "claude" 或 "node"，防 PID 回收误判。
pub fn is_claude_alive(pid: u32) -> bool {
    if !kill_zero_ok(pid) { return false; }
    match proc_pidpath(pid) {
        Some(path) => {
            let p = path.to_string_lossy().to_lowercase();
            p.contains("claude") || p.contains("node")
        }
        None => false,
    }
}

fn kill_zero_ok(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn proc_pidpath(pid: u32) -> Option<OsString> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe {
        libc::proc_pidpath(pid as i32, buf.as_mut_ptr() as *mut _, buf.len() as u32)
    };
    if n <= 0 { return None; }
    buf.truncate(n as usize);
    Some(OsString::from_vec(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn self_process_is_alive() {
        assert!(kill_zero_ok(std::process::id()));
    }
    #[test]
    fn dead_pid_not_alive() {
        assert!(!kill_zero_ok(999_999));
    }
}
```

> 注：`libc::PROC_PIDPATHINFO_MAXSIZE` 与 `proc_pidpath` 在 macOS libc 绑定可用；若绑定缺失，`extern "C"` 声明 `proc_pidpath` 并定义常量 `4096`。

- [ ] **Step 2: 运行测试**

Run: `cd src-tauri && cargo test liveness`
Expected: 2 PASS。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/liveness.rs src-tauri/src/main.rs
git commit -m "feat: pid liveness check with proc_pidpath"
```

---

### Task 6: 扫描 ~/.claude/sessions/ 发现会话

**Files:**
- Modify: `src-tauri/src/collector.rs`（加 `collect_sessions`）

**Interfaces:**
- Consumes: `parse_session_file`, `liveness::is_claude_alive`, `dirs::home_dir`
- Produces: `fn collect_sessions() -> Vec<Session>`

- [ ] **Step 1: 写实现**

在 `collector.rs` 加：
```rust
use crate::liveness::is_claude_alive;

/// 扫 ~/.claude/sessions/*.json，每个文件名是 pid；解析 + 校验存活。
/// 单文件解析失败隔离（log + skip），不拖垮整体。
pub fn collect_sessions() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let dir = home.join(".claude/sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(pid_str) = name.strip_suffix(".json") else { continue };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        let Ok(json) = std::fs::read_to_string(&path) else { continue }; // fail fast: 跳过坏文件
        match parse_session_file(pid, &json) {
            Ok(mut s) => { s.alive = is_claude_alive(pid); out.push(s); }
            Err(_) => continue, // 隔离坏解析
        }
    }
    out
}
```

- [ ] **Step 2: 集成测试（真实目录）**

`src-tauri/src/collector.rs` 测试模块加：
```rust
#[test]
fn collect_sessions_runs_against_real_dir() {
    // 集成测试：调用不 panic、返回 Vec（可能为空若本机无运行会话）
    let sessions = super::collect_sessions();
    // 当前 cc-view 自身会话通常存在；只校验结构不校验数量
    for s in &sessions {
        assert!(!s.id.is_empty());
    }
}
```

Run: `cd src-tauri && cargo test collector`
Expected: PASS（3 个测试）。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/collector.rs
git commit -m "feat: scan ~/.claude/sessions and collect live sessions"
```

---

### Task 7: reducer 去重合并（TDD）

**Files:**
- Create: `src-tauri/src/reducer.rs`
- Modify: `src-tauri/src/main.rs`（`mod reducer;`）

**Interfaces:**
- Consumes: `models::Session`
- Produces: `fn reduce(sessions: Vec<Session>) -> Vec<Session>`（按 id 去重）

- [ ] **Step 1: 写失败测试**

`src-tauri/src/reducer.rs`：
```rust
use crate::models::Session;
use std::collections::HashMap;

/// 按 id 去重：同 id 取 alive=true 的那条，否则保留最后一条。
pub fn reduce(sessions: Vec<Session>) -> Vec<Session> {
    let mut map: HashMap<String, Session> = HashMap::new();
    for s in sessions {
        match map.get(&s.id) {
            Some(prev) if prev.alive && !s.alive => continue, // 保留存活的那条
            _ => { map.insert(s.id.clone(), s); }
        }
    }
    let mut v: Vec<Session> = map.into_values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Source, Status, FocusHint};
    fn mk(id: &str, alive: bool) -> Session {
        Session {
            id: id.into(), source: Source::Interactive, pid: 1, project: "p".into(),
            cwd: "/c".into(), name: id.into(), status: Status::Working, started_at: 0,
            status_updated_at: 0, alive, focus_hint: FocusHint::default(),
        }
    }
    #[test]
    fn dedups_preferring_alive() {
        let r = reduce(vec![mk("a", false), mk("a", true)]);
        assert_eq!(r.len(), 1);
        assert!(r[0].alive);
    }
    #[test]
    fn keeps_distinct_ids() {
        let r = reduce(vec![mk("a", true), mk("b", true)]);
        assert_eq!(r.len(), 2);
    }
}
```

- [ ] **Step 2: 运行测试**

Run: `cd src-tauri && cargo test reducer`
Expected: 2 PASS。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/reducer.rs src-tauri/src/main.rs
git commit -m "feat: reduce/dedup sessions by id"
```

---

### Task 8: 轮询循环 + 内容 hash 去重 + emit

**Files:**
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `collector::collect_sessions`, `reducer::reduce`, Tauri `AppHandle`
- Produces: 每 3s `app.emit("sessions", Vec<Session>)`，仅 hash 变化时 emit

- [ ] **Step 1: 写轮询与 emit**

`main.rs` 的 setup 里，起一个后台线程：
```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tauri::Emitter;

fn hash_sessions(s: &[models::Session]) -> u64 {
    let mut h = DefaultHasher::new();
    // 简单按 (id, status, alive) hash，足以检测状态变化
    for x in s {
        x.id.hash(&mut h);
        format!("{:?}", x.status).hash(&mut h);
        x.alive.hash(&mut h);
    }
    h.finish()
}

fn start_poll_loop(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_hash = 0u64;
        loop {
            let sessions = collector::collect_sessions();
            let merged = reducer::reduce(sessions);
            let h = hash_sessions(&merged);
            if h != last_hash {
                last_hash = h;
                let _ = handle.emit("sessions", &merged);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}
```

在 `setup` 闭包里调用 `start_poll_loop(app.handle().clone());`。

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 通过。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat: 3s poll loop with hash dedup, emit sessions event"
```

---

### Task 9: 前端 popover 列表

**Files:**
- Create: `src/types.ts`
- Create: `src/components/SessionList.vue`
- Modify: `src/App.vue`

**Interfaces:**
- Consumes: Tauri event `sessions`（payload = `Session[]`）

- [ ] **Step 1: 前端类型镜像**

`src/types.ts`：
```ts
export type Status = 'working' | 'waitingForInput' | 'needsPermission' | 'shell';
export interface Session {
  id: string;
  source: string;
  pid: number;
  project: string;
  cwd: string;
  name: string;
  status: Status;
  startedAt: number;
  statusUpdatedAt: number;
  alive: boolean;
  focusHint: { host: string };
}
```

- [ ] **Step 2: SessionList 组件**

`src/components/SessionList.vue`：
```vue
<script setup lang="ts">
import type { Session, Status } from '../types';
defineProps<{ sessions: Session[] }>();
const icon: Record<Status, string> = {
  working: '⚡', waitingForInput: '💤', needsPermission: '⏳', shell: '🖥️',
};
function ago(ts: number) {
  const s = Math.floor((Date.now() - ts) / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s/60)}m`;
}
</script>
<template>
  <ul class="list">
    <li v-for="s in sessions" :key="s.id" :class="{ dead: !s.alive }">
      <span class="ico">{{ icon[s.status] }}</span>
      <span class="name">{{ s.name || s.project }}</span>
      <span class="proj">{{ s.project }}</span>
      <span class="ago">{{ ago(s.statusUpdatedAt) }}</span>
    </li>
  </ul>
</template>
<style scoped>
.list { list-style: none; margin: 0; padding: 0; min-width: 360px; }
li { display: flex; gap: 8px; padding: 6px 10px; align-items: center; }
li.dead { opacity: 0.4; }
.proj, .ago { color: #888; font-size: 12px; }
.name { flex: 1; }
</style>
```

- [ ] **Step 3: App.vue 监听事件**

`src/App.vue`：
```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import SessionList from './components/SessionList.vue';
import type { Session } from './types';
const sessions = ref<Session[]>([]);
onMounted(async () => {
  await listen<Session[]>('sessions', e => { sessions.value = e.payload; });
});
</script>
<template>
  <div class="app">
    <h3>Claude Code 会话</h3>
    <SessionList :sessions="sessions" />
  </div>
</template>
<style>
body { margin: 0; font-family: -apple-system, sans-serif; }
.app { padding: 8px; }
</style>
```

- [ ] **Step 4: 配置 popover 窗口**

在 `tauri.conf.json` 的 `app.windows` 加一个窗口：`"visible": false`, `"decorations": false`, `"resizable": false`, `"width": 380`, `"height": 480`, `"skipTaskbar": true`，label 如 `popover`。Task 1 的 tray 点击事件改为 toggle 该窗口的 `show`/`hide` + `set_focus`：

```rust
.on_tray_icon_event(|tray, event| {
    if let tauri::tray::TrayIconEvent::Click { button: tauri::MouseButton::Left, button_state: tauri::MouseButtonState::Up, .. } = event {
        let app = tray.app_handle();
        if let Some(w) = app.get_webview_window("popover") {
            if w.is_visible().unwrap_or(false) { let _ = w.hide(); }
            else { let _ = w.show(); let _ = w.set_focus(); }
        }
    }
})
```

- [ ] **Step 5: 验证**

Run: `npm run tauri dev`
Expected: 点 menubar 图标弹出 popover，显示当前 Claude 会话列表（至少 cc-view 自身），状态图标正确，3s 内随会话状态变化刷新。

- [ ] **Step 6: Commit**

```bash
git add src/ src-tauri/tauri.conf.json src-tauri/src/main.rs
git commit -m "feat: popover session list driven by sessions event"
```

---

### Task 10: 端到端冒烟测试

**Files:** 无（手动验证 + 记录）

- [ ] **Step 1: 多会话场景验证**

在 iTerm2 / Otty / cmux 各开一个目录跑 `claude`，另保留当前 cc-view 会话。点 popover 确认：
- 列表显示所有会话，项目名/会话名正确。
- 在某个 claude 里发 prompt 让它干活 → 该会话显示 ⚡；它停下等你 → 变 💤。
- 关掉某个 claude → 几秒内该行变灰（alive=false）。

- [ ] **Step 2: 记录已知限制到 README**

`README.md` 加一节"已知限制（Plan 1）"：仅前台 sessions 文件源、无通知、无 focus 跳转、无隐藏、NeedsPermission 暂不区分（待 Plan 2 PermissionChecker）。

- [ ] **Step 3: Commit**

```bash
git add README.md && git commit -m "docs: plan-1 known limitations"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 1 覆盖 spec §4 架构、§5.1 主源（sessions 文件 + 进程扫描）、§6 状态机（4 态，Compacting 留 Plan 2）、§7 collector/reducer/statemachine/liveness 模块、§8 B 总览、§9 liveness/PID 回收防护、§10 statemachine TDD。未覆盖项（A/C/E/其他源/focus/Compacting）明确列入 Out of Scope → Plan 2。✅
- **Placeholder scan**：无 TBD/TODO；Task 1 的 LSUIElement 与 libc 绑定两处标注"执行时核对"，属框架版本相关的外部核对，非内容占位。
- **Type consistency**：`decide(&DecideInput)` / `parse_session_file(pid, &str)` / `collect_sessions()` / `reduce(Vec<Session>)` / `is_claude_alive(pid)` 在各任务间签名一致。`Status` 四态在 models/statemachine/前端 types.ts 一致。✅
