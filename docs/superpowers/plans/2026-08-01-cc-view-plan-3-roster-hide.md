# cc-view Plan 3: roster 后台源 + 隐藏/归档 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** ① 把 `~/.claude/daemon/roster.json` 的后台 fleet agent 并入会话列表（覆盖非交互式后台会话）；② 用户可隐藏/归档某些会话（数据不动、可逆），前端有隐藏/恢复 UI。

**Architecture:** collector 加 `parse_roster` 读 roster.json 的 workers，每个 worker → Session(source=Fleet)，并入 collect_sessions（pid 存活校验，reducer 按 sessionId 去重）。新增 hidden 模块（`~/.claude/cc-view/hidden.json` 读写 + 过滤），通过 Tauri command 暴露给前端，popover 每行加隐藏按钮 + "显示已隐藏" toggle。

**Tech Stack:** Rust（serde_json / dirs）、Tauri 2 command、Vue 3、cargo test。

## Global Constraints

- macOS；零侵入只读 `~/.claude/`（hidden.json 写在 `~/.claude/cc-view/`，cc-view 自己的目录，不碰官方文件）。
- 路径用 `dirs::home_dir()`，禁止硬编码。
- 代码英文/注释中文；fail fast；纯逻辑（parse_roster / hidden）用 TDD。
- roster worker 无显式 status 字段（实测）→ 默认 `Working`（后台 agent 通常在执行），pid 死 → 保留但 alive=false。

## Out of Scope（留 Plan 4）

focus 跳回(C) · Compacting 状态 · `claude agents --json` 源 · 多终端 AppleScript。

---

## File Structure

后端 `src-tauri/src/`：
- `collector.rs`（**改**）— 加 `parse_roster` + roster worker → Session + collect_sessions 合并
- `hidden.rs`（**新**）— `HiddenList`（TDD）
- `lib.rs`（**改**）— Tauri command（hide/unhide/list）+ invoke_handler + 轮询 filter hidden
- `models.rs`（**改**，若需）— Source 已有 Fleet/Slash

前端 `src/`：
- `components/SessionList.vue`（**改**）— 隐藏按钮
- `App.vue`（**改**）— "显示已隐藏" toggle + 调 command

---

### Task 1: parse_roster（解析 roster.json，TDD）

**Files:**
- Modify: `src-tauri/src/collector.rs`（加 `parse_roster` + roster fixture）
- Create: `src-tauri/tests/fixtures/roster.json`

**Interfaces:**
- Consumes: `models::{Session, Source, Status, FocusHint}`、`dirs::home_dir`
- Produces: `pub fn parse_roster(json: &str) -> Vec<Session>`；`pub fn read_roster() -> Vec<Session>`

**roster.json 实测结构**：`{proto, supervisorPid, updatedAt, workers: { "<short>": { pid, sessionId, cwd, cliVersion, startedAt, dispatch: { source: "fleet"|"slash", seed: { intent }, ... }, ... } } }`

- [ ] **Step 1: fixture**

`src-tauri/tests/fixtures/roster.json`（取自真实结构，sessionId 已换为示例值）：
```json
{"proto":1,"supervisorPid":68572,"updatedAt":1785574698212,"workers":{"f0d42050":{"pid":1958,"sessionId":"f0d42050-7b39-46e3-996a-1c5829f55ffe","cwd":"/Users/zhuyuchen/ai","cliVersion":"2.1.201","startedAt":1785574478098,"dispatch":{"proto":1,"short":"f0d42050","source":"fleet","cwd":"/Users/zhuyuchen/ai","seed":{"intent":""}}}}}
```

- [ ] **Step 2: 写失败测试**

collector.rs 测试模块加：
```rust
#[test]
fn parses_roster_workers() {
    let json = std::fs::read_to_string("tests/fixtures/roster.json").unwrap();
    let v = super::parse_roster(&json);
    assert_eq!(v.len(), 1);
    let s = &v[0];
    assert_eq!(s.id, "f0d42050-7b39-46e3-996a-1c5829f55ffe");
    assert_eq!(s.source, crate::models::Source::Fleet);
    assert_eq!(s.pid, 1958);
    assert_eq!(s.project, "ai");
    assert_eq!(s.status, crate::models::Status::Working); // roster 无 status，默认 Working
}
```

- [ ] **Step 3: 跑确认失败**

Run: `cargo test collector`
Expected: FAIL（`parse_roster` 未定义）。

- [ ] **Step 4: 实现**

collector.rs 加：
```rust
#[derive(serde::Deserialize)]
struct RosterFile {
    #[serde(default)] workers: std::collections::HashMap<String, RosterWorker>,
}
#[derive(serde::Deserialize)]
struct RosterWorker {
    pid: u32,
    #[serde(rename = "sessionId")] session_id: String,
    cwd: String,
    #[serde(rename = "startedAt")] started_at: Option<i64>,
    dispatch: Option<RosterDispatch>,
}
#[derive(serde::Deserialize)]
struct RosterDispatch {
    source: Option<String>,
    #[serde(default)] seed: RosterSeed,
}
#[derive(serde::Default, serde::Deserialize)]
struct RosterSeed { #[default] intent: String }

/// 解析 roster.json，每个 worker → Session（source 由 dispatch.source 决定，status 默认 Working）。
pub fn parse_roster(json: &str) -> Vec<Session> {
    let Ok(f) = serde_json::from_str::<RosterFile>(json) else { return vec![] };
    f.workers.into_values().map(|w| {
        let source = match w.dispatch.as_ref().and_then(|d| d.source.as_deref()) {
            Some("slash") => Source::Slash,
            _ => Source::Fleet,
        };
        let name = w.dispatch.as_ref().map(|d| d.seed.intent.clone()).filter(|s| !s.is_empty())
            .unwrap_or_else(|| w.session_id.chars().take(8).collect());
        let project = std::path::Path::new(&w.cwd)
            .file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        Session {
            id: w.session_id, source, pid: w.pid, project, cwd: w.cwd, name,
            status: Status::Working,
            started_at: w.started_at.unwrap_or(0),
            status_updated_at: w.started_at.unwrap_or(0),
            alive: true, // collect_sessions 会用 pid 校验覆盖
            focus_hint: FocusHint::default(),
        }
    }).collect()
}

/// 读 ~/.claude/daemon/roster.json。
pub fn read_roster() -> Vec<Session> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let path = home.join(".claude/daemon/roster.json");
    let Ok(json) = std::fs::read_to_string(&path) else { return vec![] };
    parse_roster(&json)
}
```

- [ ] **Step 5: 跑确认通过**

Run: `cargo test collector`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/collector.rs src-tauri/tests/fixtures/roster.json
git commit -m "feat: parse roster.json fleet workers into sessions"
```

---

### Task 2: collect_sessions 合并 roster workers

**Files:**
- Modify: `src-tauri/src/collector.rs`（`collect_sessions` 末尾合并 read_roster）

**Interfaces:**
- Consumes: `read_roster`、`liveness::is_claude_alive`、`reducer`（去重在外层）

- [ ] **Step 1: 合并 roster**

`collect_sessions` 末尾（return 前）加：
```rust
// 合并后台 fleet agent（roster.json），pid 存活校验
for mut w in read_roster() {
    w.alive = is_claude_alive(w.pid);
    out.push(w);
}
out
```

- [ ] **Step 2: 测试 + 构建**

Run: `cargo test` + `cargo check`
Expected: 全 PASS。`collect_sessions_runs_against_real_dir` 仍通过（真实 roster 可能 0-N workers）。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/collector.rs
git commit -m "feat: merge roster fleet workers into collect_sessions"
```

---

### Task 3: hidden 模块（TDD）

**Files:**
- Create: `src-tauri/src/hidden.rs`
- Modify: `src-tauri/src/lib.rs`（`mod hidden;`）

**Interfaces:**
- Produces: `pub struct HiddenList`；`load() -> Self`；`save(&self)`；`is_hidden(&str)->bool`；`add(&mut self,&str)`/`remove(&mut self,&str)`；`filter(&[Session])->Vec<Session>`（纯逻辑）

- [ ] **Step 1: 写测试 + 实现**

`hidden.rs`：
```rust
use crate::models::Session;

pub struct HiddenList { ids: Vec<String> }

impl HiddenList {
    pub fn empty() -> Self { Self { ids: vec![] } }
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else { return Self::empty() };
        let path = home.join(".claude/cc-view/hidden.json");
        let Ok(json) = std::fs::read_to_string(&path) else { return Self::empty() };
        Self { ids: serde_json::from_str(&json).unwrap_or_default() }
    }
    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hidden.json");
        if let Ok(json) = serde_json::to_string(&self.ids) {
            let _ = std::fs::write(path, json);
        }
    }
    pub fn is_hidden(&self, id: &str) -> bool { self.ids.iter().any(|x| x == id) }
    pub fn add(&mut self, id: &str) { if !self.is_hidden(id) { self.ids.push(id.into()); } }
    pub fn remove(&mut self, id: &str) { self.ids.retain(|x| x != id); }
    pub fn filter<'a>(&self, sessions: &'a [Session]) -> Vec<&'a Session> {
        sessions.iter().filter(|s| !self.is_hidden(&s.id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Source, Status, FocusHint};
    fn mk(id: &str) -> Session {
        Session { id: id.into(), source: Source::Interactive, pid: 1, project: "p".into(),
            cwd: "/c".into(), name: id.into(), status: Status::Working, started_at: 0,
            status_updated_at: 0, alive: true, focus_hint: FocusHint::default() }
    }
    #[test]
    fn add_and_is_hidden() {
        let mut h = HiddenList::empty();
        h.add("a");
        assert!(h.is_hidden("a"));
        assert!(!h.is_hidden("b"));
        h.add("a"); // 去重
        assert_eq!(h.ids.len(), 1);
    }
    #[test]
    fn remove_unhides() {
        let mut h = HiddenList::empty();
        h.add("a"); h.remove("a");
        assert!(!h.is_hidden("a"));
    }
    #[test]
    fn filter_excludes_hidden() {
        let h = { let mut x = HiddenList::empty(); x.add("a"); x };
        let ss = [mk("a"), mk("b")];
        let visible = h.filter(&ss);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "b");
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test hidden`
Expected: 3 PASS。`lib.rs` 加 `mod hidden;`。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/hidden.rs src-tauri/src/lib.rs
git commit -m "feat: hidden list (hide/archive sessions, reversible)"
```

---

### Task 4: Tauri command + 轮询 filter hidden

**Files:**
- Modify: `src-tauri/src/lib.rs`（加 hide_session/unhide_session/list_hidden commands + invoke_handler + 轮询 filter）

**Interfaces:**
- Produces: Tauri commands `hide_session(id: String)` / `unhide_session(id)` / `list_hidden() -> Vec<String>`

- [ ] **Step 1: 加 commands**

lib.rs 加：
```rust
use std::sync::Mutex;

#[tauri::command]
fn hide_session(state: tauri::State<'_, Mutex<hidden::HiddenList>>, id: String) {
    if let Ok(mut h) = state.lock() { h.add(&id); h.save(); }
}
#[tauri::command]
fn unhide_session(state: tauri::State<'_, Mutex<hidden::HiddenList>>, id: String) {
    if let Ok(mut h) = state.lock() { h.remove(&id); h.save(); }
}
#[tauri::command]
fn list_hidden(state: tauri::State<'_, Mutex<hidden::HiddenList>>) -> Vec<String> {
    state.lock().map(|h| h.ids.clone()).unwrap_or_default()
}
```

- [ ] **Step 2: 注册 state + invoke_handler**

`run()` 的 builder：
```rust
.manage(std::sync::Mutex::new(hidden::HiddenList::load()))
.invoke_handler(tauri::generate_handler![hide_session, unhide_session, list_hidden])
```

- [ ] **Step 3: emit 完整 merged（前端负责 filter）**

`start_poll_loop`：reduce 后，Notifier.observe 用完整 merged（隐藏会话状态仍跟踪，避免 unhide 后重复通知）；hash + emit **完整 merged**（不 filter）。隐藏/显示的过滤交给前端（按 `list_hidden` 结果，见 Task 5），这样隐藏/恢复即时反映、无需后端重新触发。保持 Plan 2 的 hash + emit 逻辑不变。

- [ ] **Step 4: 构建**

Run: `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: hide/unhide commands + filter hidden from emit"
```

---

### Task 5: 前端隐藏 UI

**Files:**
- Modify: `src/components/SessionList.vue`（每行隐藏按钮）
- Modify: `src/App.vue`（"显示已隐藏" toggle）
- Modify: `src-tauri/capabilities/default.json`（允许 popover 调用 commands，若需）

**Interfaces:**
- Consumes: Tauri invoke `hide_session` / `unhide_session`

- [ ] **Step 1: SessionList 加隐藏按钮**

`SessionList.vue` 加 props `showHidden` + 每行一个隐藏按钮（调 invoke）：
```vue
<script setup lang="ts">
import type { Session, Status } from '../types';
import { invoke } from '@tauri-apps/api/core';
defineProps<{ sessions: Session[]; showHidden?: boolean }>();
const icon: Record<Status, string> = { working:'⚡', waitingForInput:'💤', needsPermission:'⏳', shell:'🖥️' };
function ago(ts: number) { const s=Math.floor((Date.now()-ts)/1000); return s<60?`${s}s`:`${Math.floor(s/60)}m`; }
async function hide(id: string) { await invoke('hide_session', { id }); }
</script>
<template>
  <ul class="list">
    <li v-for="s in sessions" :key="s.id" :class="{ dead: !s.alive }">
      <span class="ico">{{ icon[s.status] }}</span>
      <span class="name">{{ s.name || s.project }}</span>
      <span class="proj">{{ s.project }}</span>
      <span class="ago">{{ ago(s.statusUpdatedAt) }}</span>
      <button class="hide-btn" @click="hide(s.id)" title="隐藏">×</button>
    </li>
  </ul>
</template>
<style scoped>
.list { list-style:none; margin:0; padding:0; min-width:380px; }
li { display:flex; gap:8px; padding:6px 10px; align-items:center; }
li.dead { opacity:.4; }
.proj,.ago { color:#888; font-size:12px; }
.name { flex:1; }
.hide-btn { background:none; border:none; color:#888; cursor:pointer; }
.hide-btn:hover { color:#333; }
</style>
```

- [ ] **Step 2: App.vue 加"显示已隐藏" + 刷新**

`App.vue` 加 `showHidden` ref + toggle；隐藏后需刷新列表（invoke 后重新触发——简单做法：emit 一个 `refresh` 或让 popover 重新拉。MVP：隐藏后用户切 toggle 时 list_hidden 决定显示）。最小实现：App 维护 showHidden toggle，传给 SessionList；隐藏的会话平时不在 emit 的 visible 里（后端已 filter），所以"显示已隐藏"需后端另发全量。**MVP 简化**：后端始终 emit 全量，前端按本地 hidden 集合过滤（前端调 list_hidden 拿集合）。

调整：后端 emit 全量 merged（不 filter），前端用 list_hidden 过滤显示。改 Task 4 Step 3：emit 完整 merged，不 filter（filter 移到前端）。
```vue
<!-- App.vue -->
<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import SessionList from './components/SessionList.vue';
import type { Session } from './types';
const all = ref<Session[]>([]);
const hidden = ref<string[]>([]);
const showHidden = ref(false);
const visible = computed(() => showHidden.value ? all.value : all.value.filter(s => !hidden.value.includes(s.id)));
async function refreshHidden() { hidden.value = await invoke<string[]>('list_hidden'); }
onMounted(async () => {
  await refreshHidden();
  await listen<Session[]>('sessions', e => { all.value = e.payload; });
});
</script>
<template>
  <div class="app">
    <h3>Claude Code 会话 <button @click="refreshHidden">↻</button>
      <label class="toggle"><input type="checkbox" v-model="showHidden"/> 显示已隐藏</label></h3>
    <SessionList :sessions="visible" />
  </div>
</template>
```
> **据此修正 Task 4 Step 3**：emit 完整 merged（不 filter），Notifier.observe 用完整 merged。前端负责 filter 显示。hidden 命令仍持久化到 hidden.json。

- [ ] **Step 3: capabilities（若前端 invoke 报权限）**

若 popover 调 invoke 报权限错，在 `src-tauri/capabilities/default.json` 的 permissions 加 `"core:event:default"` 已有；invoke 自定义 command 需确认 capabilities 允许 window "main" 调用（Tauri 2 默认允许同一 capability 内的 command）。若报错，查 Tauri 2 command 权限文档。

- [ ] **Step 4: 构建 + 验证**

Run: `npm run build` + `cargo build`
Expected: 通过。GUI 留 Task 6 冒烟。

- [ ] **Step 5: Commit**

```bash
git add src/ src-tauri/src/lib.rs src-tauri/capabilities/
git commit -m "feat: hide/archive UI with show-hidden toggle"
```

---

### Task 6: 冒烟 + README

**Files:**
- Modify: `README.md`（移除"无隐藏"限制）

- [ ] **Step 1: 技术冒烟**

后台起 `npm run tauri dev`，编译运行无 panic，pkill 干净退出。

- [ ] **Step 2: README**

移除"无隐藏/归档（E，Plan 3）"。保留 focus/Compacting（Plan 4）等。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: update known limits after Plan 3 hide/archive"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 3 覆盖 §5.2 roster 补充源、§8 E 隐藏/归档。Out of scope（focus C / Compacting / agents --json）明确列 Plan 4。✅
- **Placeholder scan**：roster 格式基于实测（Task 1 fixture 取自真实结构）；hidden/command/UI 代码完整；Task 5 capabilities 标注"若报错查文档"（条件性，非占位——MVP 默认允许）。✅
- **Type consistency**：`parse_roster(&str)->Vec<Session>` / `HiddenList::filter(&[Session])->Vec<&Session>` / commands 签名跨任务一致；Source::Fleet 复用 models。✅
- **注意**：Task 4→5 有一次设计调整（emit 全量、前端 filter）——以 Task 5 Step 2 的修正为准（Task 4 Step 3 emit 完整 merged 不 filter）。
