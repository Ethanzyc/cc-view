# cc-view Plan 6: 全局快捷键呼出命令面板（E） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 按 `⌥Space`（Option+Space，uTools 风格）全局快捷键 → 居中呼出命令面板（overlay），带搜索框 + 会话列表 + 快速操作（focus/隐藏/复制 ID），失焦或再按一次收起。

**Architecture:** `tauri-plugin-global-shortcut` 注册 ⌥Space → toggle overlay 窗口。新窗口 label "overlay"（独立于 HUD "main"），居中、always-on-top、默认隐藏、失焦自动 hide。App.vue 按当前 window label 切换渲染：HUD（main）vs Overlay（overlay）。Overlay 内嵌搜索框（过滤会话）+ SessionList + 操作按钮（focus/隐藏/复制 ID）。

**Tech Stack:** tauri-plugin-global-shortcut、Rust（tauri window）、Vue 3、cargo test。

## Global Constraints

- macOS；零侵入；dirs::home_dir()；代码英文/注释中文；fail fast。
- 复用现有 StatusIcon/SessionList/状态逻辑/commands（focus_session/hide/unhide）。
- 快捷键默认 `⌥Space`（Option+Space）；双击 ⌘（Alfred 风格，需 CGEventTap + 辅助功能权限）留可选增强，不在本 plan。
- overlay 失焦即收起（不抢焦点常驻，呼出时才浮在最前）。

## Out of Scope（留可选后续）

双击 ⌘（CGEventTap + 辅助功能权限）；overlay 跨 Space（NSPanel canJoinAllSpaces）；自定义快捷键设置 UI。

---

## File Structure

后端 `src-tauri/src/`：
- `lib.rs`（**改**）— global-shortcut 插件注册 + ⌥Space toggle overlay；overlay 失焦 hide；复制 ID command
- `Cargo.toml`（**改**）— tauri-plugin-global-shortcut 依赖
- `tauri.conf.json`（**改**）— plugins.globalShortcut；新 window "overlay"

前端 `src/`：
- `App.vue`（**改**）— 按 window label 切换 HUD vs Overlay
- `components/Overlay.vue`（**新**）— 搜索框 + 列表 + 操作

---

### Task 1: global-shortcut 插件 + ⌥Space toggle overlay

**Files:**
- Modify: `src-tauri/Cargo.toml`（加 tauri-plugin-global-shortcut）
- Modify: `src-tauri/tauri.conf.json`（plugins.globalShortcut + window "overlay"）
- Modify: `src-tauri/src/lib.rs`（plugin 注册 + 快捷键 toggle）
- Modify: `src-tauri/capabilities/default.json`（global-shortcut 权限）

**Interfaces:**
- Produces: ⌥Space 全局快捷键 → toggle overlay window show/hide

- [ ] **Step 1: 加依赖 + 插件**

Cargo.toml `[dependencies]` 加 `tauri-plugin-global-shortcut = "2"`。
lib.rs run() builder 加 `.plugin(tauri_plugin_global_shortcut::init())`。
lib.rs setup 里注册快捷键（用 Builder 或 set_handler）：
```rust
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
// ⌥Space = Modifiers::ALT + Code::Space
let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::Space);
app.global_shortcut().register(shortcut).on_shortcut(|app, _s, e| {
    if e.state == ShortcutState::Pressed {
        if let Some(w) = app.get_webview_window("overlay") {
            if w.is_visible().unwrap_or(false) { let _ = w.hide(); }
            else {
                // 居中 + show + focus（overlay 呼出时抢焦点，输入搜索）
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    }
})?;
```
> 核对 tauri-plugin-global-shortcut 2.x API（register/on_shortcut 签名），report 说明。

- [ ] **Step 2: overlay 窗口配置**

tauri.conf.json app.windows 加：
```json
{
  "label": "overlay",
  "url": "index.html?window=overlay",
  "visible": false,
  "decorations": false,
  "resizable": false,
  "width": 560,
  "height": 420,
  "center": true,
  "skipTaskbar": true,
  "alwaysOnTop": true,
  "transparent": true,
  "shadow": true
}
```
（center:true 启动居中；visible:false 默认隐藏；url 带 ?window=overlay 让前端识别）

- [ ] **Step 3: capabilities 权限**

capabilities/default.json 加 global-shortcut 权限（`"global-shortcut:allow-register"` 等，或 `"global-shortcut:default"`）。windows 数组加 "overlay"（让 overlay 也能收 sessions event + 调 command）。

- [ ] **Step 4: 构建**

Run: `cargo build`
Expected: 通过（插件 + 快捷键注册）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat: global ⌥Space shortcut toggles overlay window"
```

---

### Task 2: overlay 失焦自动收起 + 居中

**Files:**
- Modify: `src-tauri/src/lib.rs`（overlay window on_window_event Focused(false) → hide；show 时居中）

- [ ] **Step 1: 失焦 hide**

setup 里 overlay window 加失焦隐藏：
```rust
if let Some(overlay) = app.get_webview_window("overlay") {
    overlay.on_window_event(|e| {
        if let tauri::WindowEvent::Focused(false) = e {
            // 失焦自动收起（Alfred/uTools 行为）
            // 注意：on_window_event 闭包拿不到 window 引用 hide——用 WeakRef 或在外层捕获
        }
    });
}
```
> 闭包需调 window.hide()——核对 Tauri 2 on_window_event 是否传 window 引用（双参数版本 `|w, e|`），或用 `tauri::Manager` 弱引用。report 说明实际写法。

- [ ] **Step 2: show 时居中（恢复到屏幕中心）**

Task 1 的快捷键 handler 里 show 前调 `overlay.center()`（确保每次呼出都在屏幕中心，即使上次拖动过）。

- [ ] **Step 3: 构建**

Run: `cargo build`
Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: overlay auto-hides on blur + recenters on show"
```

---

### Task 3: overlay 前端（搜索 + 列表 + 操作）

**Files:**
- Create: `src/components/Overlay.vue`
- Modify: `src/App.vue`（按 window label 切换）
- Modify: `src-tauri/src/lib.rs`（加 copy_session_id command）

- [ ] **Step 1: App.vue 按 label 切换**

App.vue 用 `@tauri-apps/api/webviewWindow` 的 getCurrentWebviewWindow().label 区分：
```ts
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
const label = getCurrentWebviewWindow().label;
// label === 'overlay' → 渲染 <Overlay>；else → 现有 HUD（SessionList）
```
保留现有 HUD 渲染（label main），加 overlay 分支渲染 Overlay 组件。两者共享 sessions event 监听 + hidden 逻辑（提取到 composable 或重复——MVP 重复可接受）。

- [ ] **Step 2: Overlay.vue**

搜索框 + 列表 + 操作：
```vue
<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Session } from '../types';
import StatusIcon from './StatusIcon.vue';
const all = ref<Session[]>([]);
const q = ref('');
const visible = computed(() => {
  const k = q.value.trim().toLowerCase();
  const sorted = [...all.value].sort(byPriority); // 复用优先级排序逻辑
  return k ? sorted.filter(s => (s.name+s.project).toLowerCase().includes(k)) : sorted;
});
async function focus(id: string) { await invoke('focus_session', { id }); await getCurrentWebviewWindow().hide(); }
async function hide(id: string) { await invoke('hide_session', { id }); }
async function copyId(id: string) { await navigator.clipboard.writeText(id); }
onMounted(async () => { await listen<Session[]>('sessions', e => { all.value = e.payload; }); });
</script>
<template>
  <div class="overlay">
    <input class="search" v-model="q" placeholder="搜索会话（名称/项目）..." autofocus />
    <ul class="list">
      <li v-for="s in visible" :key="s.id" @click="focus(s.id)">
        <StatusIcon :status="s.status" />
        <span class="name">{{ s.name || s.project }}</span>
        <span class="proj">{{ s.project }}</span>
        <button @click.stop="hide(s.id)">隐藏</button>
        <button @click.stop="copyId(s.id)">复制 ID</button>
      </li>
    </ul>
  </div>
</template>
```
（byPriority 排序函数复用 SessionList 的，或提取 util。毛玻璃样式同 HUD。）

- [ ] **Step 3: copy_session_id command（可选，或前端 clipboard）**

若 navigator.clipboard 在 Tauri webview 可用，前端直接复制（不需 command）。若不行，加 `#[tauri::command] fn copy_session_id(id: String)` 用 rust 剪贴板。MVP 先用 navigator.clipboard，report 说明。

- [ ] **Step 4: 构建**

Run: `npm run build` + `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src/components/Overlay.vue src/App.vue src-tauri/src/lib.rs 2>/dev/null
git commit -m "feat: overlay command palette (search + focus/hide/copy)"
```

---

### Task 4: 冒烟 + README（全部功能完成）

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 技术冒烟**

`npm run tauri dev`：⌥Space 呼出 overlay（居中）、搜索过滤、点会话 focus+收起、失焦收起、再按 ⌥Space 收起。确认无 panic。

- [ ] **Step 2: README**

更新为"全部功能完成"：cc-view 现在是 **常驻指挥台 + 快捷呼出**——桌面 HUD（A）+ menubar 聚合（D）+ ⌥Space 命令面板（E）+ 通知（A 通知）+ 隐藏/归档（E）+ focus（C）。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: all features complete (HUD + menubar + overlay command palette)"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 6 覆盖 E（全局快捷键 + overlay 命令面板）。双击 ⌘ / 跨 Space / 自定义快捷键设置明确列可选后续。✅
- **Placeholder scan**：global-shortcut API 标注核对（on_shortcut 签名）；on_window_event 失焦 hide 闭包标注核对（window 引用）；navigator.clipboard 标注——非占位，是 API 确认。✅
- **Type consistency**：overlay window label "overlay"；commands 复用 focus_session/hide_session；Session/Status 复用。✅
- **注意**：overlay window url index.html?window=overlay，前端 getCurrentWebviewWindow().label 区分。HUD 与 overlay 共享 sessions 数据（同 event）。
