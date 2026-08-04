# cc-view UI 大改造（合并 HUD 到命令面板）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 废弃 HUD（main 窗口），让命令面板（overlay）成为 cc-view 唯一 UI，承载两者全部功能（搜索 + 分组 + 隐藏/搁置/复制ID + 显示已隐藏 + 可置顶定住 + 可拖动）；重画 tray 图标、左键弹原生菜单。

**Architecture:** 删 main 窗口，overlay 升级为唯一窗口（保留 NSPanel swizzle + 跨全屏 Space）。pin（图钉）只控制「失焦是否收起」，位置 + pin 状态持久化到 `~/.claude/cc-view/overlay-position.json`。tray 左键弹原生菜单，⌥Space 仍呼出。破坏性删除（main 窗口、SessionList、hud.rs）放到最后一个编码 task，前面所有升级都在 overlay 窗口内进行、每步编译/类型检查通过。

**Tech Stack:** Tauri 2（Rust 后端 + objc2 原生 NSPanel 调用）、Vue 3 + TypeScript（前端，无自动化测试框架）、macOS only。

## Global Constraints

- 平台：仅 macOS（objc2、NSPanel、menu bar template image）。
- 命名：窗口 label 保持 `overlay` 不改；新增 command 用 `get_overlay_pinned` / `set_overlay_pinned`。
- 位置文件：`~/.claude/cc-view/overlay-position.json`，结构 `{ x, y, pinned }`，`pinned` serde 默认 `false`（开机隐藏 + 呼出默认未钉）。
- 风格：注释中文、代码英文；fail fast 不吞异常，但前端 invoke 失败沿用现有 `console.error` 模式（UI 不崩）。
- 每个 task 结尾必须 `cargo build`（后端）或 `npx vue-tsc --noEmit`（前端）通过，再 commit。
- 后端测试：`#[cfg(test)]` 单元测试（TDD：先写失败测试再实现）。前端无测试框架：以 `npx vue-tsc --noEmit` 类型检查 + 手动验证清单作为 test cycle。
- frequent commits：每个 task 单独 commit，Conventional Commits 中文 message。

## File Structure

**后端（Rust）**
- `src-tauri/src/overlay_position.rs` — **新增**：overlay 位置 + pin 持久化（`OverlayPosition { x, y, pinned }` + load/save/save_all + 单元测试）。
- `src-tauri/src/lib.rs` — 注册 pin state/command、overlay 位置恢复与 Moved 保存、失焦双机制读 pin、tray 菜单构建与事件、删 main setup 与 hud pin command。
- `src-tauri/src/hud.rs` — **删除**（Task 8）。
- `src-tauri/tauri.conf.json` — 删 main window；`trayIcon.showMenuOnLeftClick: true`。
- `src-tauri/capabilities/default.json` — `windows: ["overlay"]`。
- `src-tauri/icons/source/tray.svg` — 圆环描边加粗；`src-tauri/icons/tray.png` 重新导出。

**前端（Vue/TS）**
- `src/components/Overlay.vue` — 升级：顶栏（显示已隐藏 toggle / 图钉 / drag-region / 计数）、行内 hide 按钮、hidden 过滤、项目名去重、分组视觉、复制ID 仅 hover、pin/hidden 状态自管。
- `src/App.vue` — 删 `isOverlay` 分流与 HUD 分支，直接渲染 `<Overlay/>`。
- `src/components/SessionList.vue` — **删除**（Task 8）。
- `src/utils/session.ts` — 不变。

---

## Task 1: overlay 位置 + pin 持久化模块

**Files:**
- Create: `src-tauri/src/overlay_position.rs`
- Modify: `src-tauri/src/lib.rs:7`（mod 声明区加 `mod overlay_position;`）

**Interfaces:**
- Produces: `overlay_position::OverlayPosition { x: i32, y: i32, pinned: bool }`，方法 `load() -> Option<Self>`、`save(x, y)`（保留磁盘 pinned）、`save_all(x, y, pinned)`。Task 2/3 消费。

- [ ] **Step 1: 写失败测试（新建文件含测试）**

创建 `src-tauri/src/overlay_position.rs`，先只写测试与空实现：

```rust
// overlay 窗口位置 + pin 持久化：load/save 读写 ~/.claude/cc-view/overlay-position.json。
// 用户拖动 overlay 后存位，下次呼出恢复——不再每次 center。pin（失焦是否收起）一并持久化。
// 模块在 Task 2 被命令引用前暂未被非测试代码使用，允许 dead_code。
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

/// pinned 的 serde 默认值：false（开机隐藏 + 呼出默认未钉 = 失焦收起）。
fn default_pinned() -> bool {
    false
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OverlayPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_pinned")]
    pub pinned: bool,
}

impl OverlayPosition {
    pub fn load() -> Option<Self> {
        unimplemented!("TODO")
    }
    pub fn save(_x: i32, _y: i32) {
        unimplemented!("TODO")
    }
    pub fn save_all(_x: i32, _y: i32, _pinned: bool) {
        unimplemented!("TODO")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let pos = OverlayPosition { x: 100, y: 200, pinned: true };
        let json = serde_json::to_string(&pos).unwrap();
        let back: OverlayPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, 100);
        assert_eq!(back.y, 200);
        assert!(back.pinned);
    }

    #[test]
    fn old_json_without_pinned_defaults_false() {
        // 向后兼容：无 pinned 字段时默认 false（区别于旧 hud-position.json 的 true）。
        let old = r#"{"x":42,"y":99}"#;
        let pos: OverlayPosition = serde_json::from_str(old).unwrap();
        assert_eq!(pos.x, 42);
        assert_eq!(pos.y, 99);
        assert!(!pos.pinned);
    }

    #[test]
    fn load_invalid_json_returns_none() {
        let pos: Option<OverlayPosition> = serde_json::from_str("not json").ok();
        assert!(pos.is_none());
    }
}
```

- [ ] **Step 2: 注册模块，跑测试确认失败**

修改 `src-tauri/src/lib.rs` 第 7 行，把
```rust
mod hud;
```
下面新增一行（保持字母序，插在 `mod hidden;` 与 `mod liveness;` 之间）：
```rust
mod overlay_position;
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml overlay_position`
Expected: FAIL（`unimplemented` panic on `serde_roundtrip` / `old_json_without_pinned_defaults_false`）

- [ ] **Step 3: 实现 load / save / save_all**

替换 `src-tauri/src/overlay_position.rs` 中 `impl OverlayPosition { ... }` 三方法的 `unimplemented!`：

```rust
impl OverlayPosition {
    /// 从磁盘加载上次保存的位置；文件不存在 / 无 home / 解析失败都返回 None。
    pub fn load() -> Option<Self> {
        let path = dirs::home_dir()?.join(".claude/cc-view/overlay-position.json");
        let txt = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// 拖动保存：保留磁盘上已有的 pinned（无值时默认 false）。
    pub fn save(x: i32, y: i32) {
        let pinned = Self::load().map(|p| p.pinned).unwrap_or(false);
        Self::save_all(x, y, pinned);
    }

    /// 显式保存完整位置（含 pinned），供 set_overlay_pinned command 调用。
    pub fn save_all(x: i32, y: i32, pinned: bool) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("overlay-position.json");
        if let Ok(json) = serde_json::to_string(&OverlayPosition { x, y, pinned }) {
            let _ = std::fs::write(path, json);
        }
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml overlay_position`
Expected: PASS（3 tests）

- [ ] **Step 5: cargo build 确认无 error**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过（`overlay_position` 暂 dead_code 被 `#![allow(dead_code)]` 抑制，仅 warn 级别消失）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/overlay_position.rs src-tauri/src/lib.rs
git commit -m "feat: 加 overlay 位置+pin 持久化模块（overlay_position.rs）"
```

---

## Task 2: overlay pin command + 失焦控制

**Files:**
- Modify: `src-tauri/src/lib.rs`
  - `run()` builder `.manage(...)` 区（约 459-461）加 pin state
  - 新增 `get_overlay_pinned` / `set_overlay_pinned` command（放在旧 `set_hud_pinned` 之后，约 343 行后）
  - `invoke_handler`（约 462-473）注册两个新 command
  - overlay setup 的 `Focused(false)` 闭包（约 544-549）读 pin 跳过
  - 快捷键 handler 的 frontmost 轮询 hide 处（约 653-657）读 pin 跳过

**Interfaces:**
- Consumes: `overlay_position::OverlayPosition`（Task 1）、`std::sync::Mutex`（已 import）、`tauri::State`。
- Produces: command `get_overlay_pinned() -> bool`、`set_overlay_pinned(pinned: bool)`；app state `Mutex<bool>`（pin 状态，初始从磁盘恢复）。

- [ ] **Step 1: 注册 pin state（初始从磁盘恢复）**

`src-tauri/src/lib.rs` 的 builder 链，在 `.manage(Mutex::new(Vec::<models::Session>::new()))`（约 461 行）**之后**加一行：

```rust
        .manage(Mutex::new(
            overlay_position::OverlayPosition::load()
                .map(|p| p.pinned)
                .unwrap_or(false),
        ))
```

> 说明：开机/启动时 pin 从 `overlay-position.json` 记忆恢复；无记录默认 false（未钉 = 失焦收起）。

- [ ] **Step 2: 新增两个 command**

在 `set_hud_pinned` 函数之后（约 343 行，`join_all_spaces` 注释之前）插入：

```rust
// --- overlay pin（图钉：失焦是否自动收起）command ---
// pin 状态由 app State<Mutex<bool>> 持有，失焦双机制读它判断要不要 hide。
// set 时同步持久化（保留磁盘 x,y），开机/重启按记忆恢复。

/// 读取 overlay 是否钉住（失焦不收起）。State 初始从磁盘恢复，无记录 false。
#[tauri::command]
fn get_overlay_pinned(state: tauri::State<'_, Mutex<bool>>) -> bool {
    *state.lock().unwrap()
}

/// 切换 overlay 钉住状态：更新 State + 持久化（保留现有 x,y）。
#[tauri::command]
fn set_overlay_pinned(pinned: bool, state: tauri::State<'_, Mutex<bool>>) {
    *state.lock().unwrap() = pinned;
    let (x, y) = overlay_position::OverlayPosition::load()
        .map(|p| (p.x, p.y))
        .unwrap_or((0, 0));
    overlay_position::OverlayPosition::save_all(x, y, pinned);
}
```

- [ ] **Step 3: 注册 command**

`invoke_handler` 的 `generate_handler!` 数组（约 462-473），在 `set_hud_pinned,` 之后加两行：

```rust
            get_overlay_pinned,
            set_overlay_pinned,
```

- [ ] **Step 4: overlay Focused(false) 读 pin 跳过**

`src-tauri/src/lib.rs` 约 544-549，把：
```rust
                let w = overlay.clone();
                overlay.on_window_event(move |e| {
                    if let tauri::WindowEvent::Focused(false) = e {
                        let _ = w.hide();
                    }
                });
```
改为（同时为 Task 3 预留 Moved 分支，但 Moved 实现在 Task 3；此处先只改 Focused）：

```rust
                let w = overlay.clone();
                let app_handle = app.handle().clone();
                overlay.on_window_event(move |e| {
                    if let tauri::WindowEvent::Focused(false) = e {
                        // 钉住时失焦不收起；未钉才 hide。
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        if !pinned {
                            let _ = w.hide();
                        }
                    }
                });
```

- [ ] **Step 5: frontmost 轮询读 pin 跳过**

快捷键 handler 内的轮询线程（约 640-658），把：
```rust
                                                    // 前台 app 变了（用户切到别的 app）→ hide
                                                    if frontmost_bundle_id() != stable_front {
                                                        let _ = w.hide();
                                                        break;
                                                    }
```
改为：
```rust
                                                    // 前台 app 变了 → 仅未钉时 hide；钉住则继续轮询不收起。
                                                    if frontmost_bundle_id() != stable_front {
                                                        let pinned = app_handle
                                                            .state::<Mutex<bool>>()
                                                            .lock()
                                                            .map(|g| *g)
                                                            .unwrap_or(false);
                                                        if !pinned {
                                                            let _ = w.hide();
                                                            break;
                                                        }
                                                    }
```

> 取舍：钉住常驻时轮询每 200ms 空转一次（仅查 frontmost，开销极小），换取「pin 切换无需动态启停线程」的实现简单。

- [ ] **Step 6: build 确认通过**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过。`overlay_position` 现已被 `set_overlay_pinned` 引用，可移除其 `#![allow(dead_code)]`——打开 `overlay_position.rs` 删除第一行 `#![allow(dead_code)]`。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/overlay_position.rs
git commit -m "feat: 加 overlay pin command + 失焦按 pin 跳过收起"
```

---

## Task 3: overlay 位置恢复 + 拖动保存 + show 不再无条件 center

**Files:**
- Modify: `src-tauri/src/lib.rs`
  - overlay setup（约 525-550）：vibrancy 后恢复位置；`on_window_event` 加 `Moved` 分支
  - 快捷键 show 分支（约 611-613）：去掉无条件 `center()`，改为有记忆恢复、无则 center

**Interfaces:**
- Consumes: `overlay_position::OverlayPosition`（Task 1）。

- [ ] **Step 1: setup 恢复位置 + Moved 保存**

`src-tauri/src/lib.rs` overlay setup 块（约 525-550）。在 `make_panel(&overlay);`（约 542）**之后**、原 `on_window_event` 闭包**之前**，插入位置恢复：

```rust
                // 恢复上次保存的 overlay 位置（vibrancy / swizzle 之后）。
                // 无记录时跳过——由呼出时的 center() 兜底。
                if let Some(pos) = overlay_position::OverlayPosition::load() {
                    let _ = overlay.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                }
```

然后把 Task 2 改过的 `on_window_event` 闭包，从只 match `Focused(false)` 扩展为同时 match `Moved`。替换整个闭包为：

```rust
                let w = overlay.clone();
                let app_handle = app.handle().clone();
                overlay.on_window_event(move |e| match e {
                    tauri::WindowEvent::Moved(p) => {
                        // 拖动后存位置（保留磁盘 pinned）。
                        overlay_position::OverlayPosition::save(p.x, p.y);
                    }
                    tauri::WindowEvent::Focused(false) => {
                        // 钉住时失焦不收起；未钉才 hide。
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        if !pinned {
                            let _ = w.hide();
                        }
                    }
                    _ => {}
                });
```

- [ ] **Step 2: show 不再无条件 center**

快捷键 handler 的 show 分支（约 611-613），把：
```rust
                                        // 每次 show 前居中——即使上次拖动过，呼出总在屏幕中心
                                        let _ = w.center();
                                        let _ = w.show();
```
改为：
```rust
                                        // 有记忆位置则恢复，无则居中；不再每次强制 center。
                                        if let Some(pos) =
                                            overlay_position::OverlayPosition::load()
                                        {
                                            let _ = w.set_position(
                                                tauri::PhysicalPosition::new(pos.x, pos.y),
                                            );
                                        } else {
                                            let _ = w.center();
                                        }
                                        let _ = w.show();
```

- [ ] **Step 3: build 确认通过**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: overlay 拖动存位+恢复，呼出不再无条件 center"
```

---

## Task 4: 前端 overlay 顶栏升级（图钉 + 显示已隐藏 + drag-region + pin 状态）

**Files:**
- Modify: `src/components/Overlay.vue`
  - `<script setup>`：加 `pinned` / `showHidden` / `hidden` 状态，`togglePin`、`refreshHidden`、`onMount` 拉 pin+hidden
  - `<template>`：`search-bar` 加 `data-tauri-drag-region`、图钉按钮、显示已隐藏 checkbox；input/checkbox/按钮设 `data-tauri-drag-region="false"`
  - `<style>`：加 `.toggle` / `.pin-btn` 样式（沿用 App.vue HUD 的同名样式）

**Interfaces:**
- Consumes: 后端 command `get_overlay_pinned` / `set_overlay_pinned` / `list_hidden`（Task 2 + 现有）。
- Produces: Overlay 顶栏可拖动、可钉、可切显示已隐藏（Task 5 接管 hidden 过滤逻辑）。

> 说明：现有 Overlay 顶栏用 `-webkit-app-region: drag`（Electron 私有，WKWebView 不生效）——本 task 顺带修为 Tauri 的 `data-tauri-drag-region`。

- [ ] **Step 1: script 加状态与方法**

`src/components/Overlay.vue` `<script setup>` 顶部（`const all = ref...` 附近，约 16-20 行），在 `const copiedId` 之前插入三个状态：

```ts
// 隐藏列表 + 显示已隐藏 toggle（从 App.vue HUD 分支迁入）。visible 按 toggle 过滤。
const hidden = ref<string[]>([]);
const showHidden = ref(false);
// 图钉（pin = 失焦不收起）：后端 command + overlay_position.json 驱动。
const pinned = ref(false);
```

在 `onMounted` 内（约 145-166），`get_sessions` 拉取之后追加 pin + hidden 拉取。把 onMounted 改为：

```ts
onMounted(async () => {
  // 打开即拉当前会话，不等 3s 轮询/hash 变化——避免空列表。
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
  }
  try {
    hidden.value = await invoke<string[]>('list_hidden');
  } catch (e) {
    console.error('list_hidden on mount failed', e);
  }
  try {
    pinned.value = await invoke<boolean>('get_overlay_pinned');
  } catch (e) {
    console.error('get_overlay_pinned on mount failed', e);
  }
  try {
    await listen<Session[]>('sessions', e => { all.value = e.payload; });
  } catch (e) {
    console.error('overlay listen sessions failed', e);
  }

  // 窗口获焦时 focus + select 搜索框（overlay show/hide 复用，autofocus 仅首次生效）
  const win = getCurrentWebviewWindow();
  await win.onFocusChanged(({ payload: focused }) => {
    if (focused && searchRef.value) {
      searchRef.value.focus();
      searchRef.value.select();
    }
  });
});
```

在 `copyId` 函数之后（约 143 行后）加 pin/hidden 方法：

```ts
// 切换图钉：调后端 set_overlay_pinned（更新 State + 持久化），更新本地 ref。
async function togglePin() {
  const next = !pinned.value;
  try {
    await invoke('set_overlay_pinned', { pinned: next });
    pinned.value = next;
  } catch (e) {
    console.error('set_overlay_pinned failed', e);
  }
}

// 刷新隐藏列表（hide/unhide 成功后调，让 visible 立即反映）。
async function refreshHidden() {
  hidden.value = await invoke<string[]>('list_hidden');
}
```

- [ ] **Step 2: template 顶栏改造**

`src/components/Overlay.vue` `<template>` 的 `.search-bar`（约 170-186），整段替换为：

```html
    <div class="search-bar" data-tauri-drag-region>
      <svg class="search-icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="7" cy="7" r="4.5" />
        <path d="M10.5 10.5 L14 14" />
      </svg>
      <input
        ref="searchRef"
        class="search"
        v-model="q"
        placeholder="搜索会话（名称 / 项目）..."
        autofocus
        spellcheck="false"
        data-tauri-drag-region="false"
      />
      <!-- 计数：搜索态→结果数；非搜索态→待介入数 -->
      <span class="overlay-count" data-tauri-drag-region="false">{{ overlayCount }}</span>
      <label class="toggle" data-tauri-drag-region="false">
        <input type="checkbox" v-model="showHidden" />
        <span>显示已隐藏</span>
      </label>
      <button
        class="pin-btn"
        :class="{ pinned }"
        :title="pinned ? '取消定住' : '定住（失焦不收起）'"
        :aria-label="pinned ? '取消定住' : '定住（失焦不收起）'"
        :aria-pressed="pinned"
        data-tauri-drag-region="false"
        @click="togglePin"
      >
        <!-- 图钉（Lucide pin）：定住时填充高亮，未钉只描边 -->
        <svg width="13" height="13" viewBox="0 0 24 24" :fill="pinned ? 'currentColor' : 'none'" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="12" y1="17" x2="12" y2="22" />
          <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z" />
        </svg>
      </button>
    </div>
```

- [ ] **Step 3: style 加 toggle / pin-btn**

`src/components/Overlay.vue` `<style scoped>`，在 `.search:focus-visible { outline: none; }`（约 364-365）之后插入（沿用 App.vue HUD 的同名样式语义）：

```css
/* 显示已隐藏 toggle（从 App.vue HUD 迁入） */
.toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--gap-xs);
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
  cursor: pointer;
  flex-shrink: 0;
}
.toggle input {
  margin: 0;
  width: 12px;
  height: 12px;
  accent-color: var(--color-primary);
  cursor: pointer;
}
.toggle input:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 2px;
}

/* 图钉：未钉 tertiary，定住 primary 高亮，hover fg + hover bg（同 App.vue HUD） */
.pin-btn {
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  padding: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  flex-shrink: 0;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.pin-btn.pinned { color: var(--color-primary); }
.pin-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.pin-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}
```

并删除旧的 `-webkit-app-region` 规则：把 `.search-bar { -webkit-app-region: drag; ... }`（约 335）里的 `-webkit-app-region: drag;` 行删掉（保留其余 padding/transition）；把 `.search { -webkit-app-region: no-drag; ... }`（约 350）与 `.overlay-count { ... -webkit-app-region: no-drag; }`（约 373）里的 `-webkit-app-region` 行删掉。

- [ ] **Step 4: 类型检查 + 手动验证**

Run: `npx vue-tsc --noEmit`
Expected: 无错误。

手动验证（`npm run tauri dev` 后 ⌥Space 呼出）：
- 顶栏可拖动移动窗口（修复了旧 bug）
- 图钉点击切换高亮，钉住后点别处窗口不消失，再点取消
- 「显示已隐藏」checkbox 可勾选（过滤逻辑在 Task 5 接，此时勾选暂无效果属正常）

- [ ] **Step 5: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "feat(overlay): 顶栏加图钉/显示已隐藏，改 data-tauri-drag-region 修复拖动"
```

---

## Task 5: 前端行内 hide 按钮 + 显示已隐藏过滤

**Files:**
- Modify: `src/components/Overlay.vue`
  - `<script setup>`：加 `visible` computed（按 showHidden 过滤）、`hide` / `unhide` 函数；`sorted` / `flatResults` / `groups` 改基于 `visible`
  - `<template>`：分组态与搜索态行内 actions 加 hide 按钮；已隐藏行加「已隐藏」标记 + 更淡

**Interfaces:**
- Consumes: 后端 command `hide_session` / `unhide_session` / `list_hidden`（现有）；Task 4 的 `hidden` / `showHidden` / `refreshHidden`。
- Produces: overlay 行内可隐藏/取消隐藏；showHidden 控制过滤。

- [ ] **Step 1: 加 visible computed 与 hide/unhide 函数**

`src/components/Overlay.vue` `<script setup>`，在 `const kLower = ...`（约 36）之前插入 `visible`：

```ts
// visible：按 showHidden toggle 过滤 hidden。off→只未隐藏；on→全显示。
const visible = computed(() =>
  showHidden.value ? all.value : all.value.filter(s => !hidden.value.includes(s.id)),
);
```

把 `sorted`（约 23-31）的数据源从 `all.value` 改为 `visible.value`：
```ts
const sorted = computed(() =>
  [...visible.value].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    const pc = a.project.localeCompare(b.project);
    if (pc !== 0) return pc;
    return b.statusUpdatedAt - a.statusUpdatedAt;
  }),
);
```

在 `refreshHidden`（Task 4 加的）之后追加 hide/unhide：

```ts
// 隐藏/取消隐藏：成功后刷新 hidden 列表，visible 立即反映。
async function hide(id: string) {
  try {
    await invoke('hide_session', { id });
    await refreshHidden();
  } catch (e) {
    console.error('hide failed', e);
  }
}
async function unhide(id: string) {
  try {
    await invoke('unhide_session', { id });
    await refreshHidden();
  } catch (e) {
    console.error('unhide failed', e);
  }
}
```

- [ ] **Step 2: 搜索态行内加 hide 按钮 + 已隐藏标记**

搜索态 `<li>`（约 191-251）的 `.actions` 区，在「复制 ID」按钮**之前**插入 hide 按钮；并在 `.ago` 之后（actions 之前）加已隐藏标记。把该 `<li>` 内的：

```html
          <div class="actions">
            <button
              v-if="s.alive && s.snoozed"
              class="act-btn snooze"
              title="恢复（取消搁置）"
              @click.stop="unsnooze(s.id)"
            >恢复</button>
            <button
              v-else-if="s.alive && s.status === 'waitingForInput'"
              class="act-btn snooze"
              title="搁置（暂时不管）"
              @click.stop="snooze(s.id)"
            >搁置</button>
            <button
              class="act-btn copy"
              :class="{ done: copiedId === s.id }"
              :title="copiedId === s.id ? '已复制' : '复制 ID'"
              @click.stop="copyId(s.id)"
            >{{ copiedId === s.id ? '已复制' : '复制 ID' }}</button>
          </div>
```

替换为（插入 hide 按钮 + 复制 ID 加 `hover-only` class 以便 Task 6 控制显隐）：

```html
          <div class="actions">
            <button
              v-if="s.alive && s.snoozed"
              class="act-btn snooze"
              title="恢复（取消搁置）"
              @click.stop="unsnooze(s.id)"
            >恢复</button>
            <button
              v-else-if="s.alive && s.status === 'waitingForInput'"
              class="act-btn snooze"
              title="搁置（暂时不管）"
              @click.stop="snooze(s.id)"
            >搁置</button>
            <button
              class="act-btn hide"
              :title="hidden.includes(s.id) ? '取消隐藏' : '隐藏'"
              @click.stop="hidden.includes(s.id) ? unhide(s.id) : hide(s.id)"
            >{{ hidden.includes(s.id) ? '取消隐藏' : '隐藏' }}</button>
            <button
              class="act-btn copy hover-only"
              :class="{ done: copiedId === s.id }"
              :title="copiedId === s.id ? '已复制' : '复制 ID'"
              @click.stop="copyId(s.id)"
            >{{ copiedId === s.id ? '已复制' : '复制 ID' }}</button>
          </div>
```

- [ ] **Step 3: 分组态行内加 hide 按钮 + 已隐藏标记**

分组态 `<li>`（约 264-312）的 `.actions` 做同样改造（与 Step 2 同样的替换，把 hide 按钮插在 snooze 与 copy 之间，copy 加 `hover-only`）。

另外，分组态与搜索态的 `.row` 都需要：当 `hidden.includes(s.id)` 时整行更淡 + 行尾标「已隐藏」。在两个 `<li>` 的 `:class` 绑定里加 `'is-hidden': hidden.includes(s.id)`，并把分组态 `<li>` 的 class 改为：

```html
              :class="{
                dead: !s.alive,
                snoozed: s.snoozed,
                perm: s.status === 'needsPermission' && !s.snoozed,
                'is-hidden': hidden.includes(s.id),
              }"
```
搜索态 `<li>` 的 `:class` 同样加 `'is-hidden': hidden.includes(s.id)`。

在 `.ago` 元素内追加已隐藏小字（两个 `<li>` 都加，紧接 `{{ agoF(s.statusUpdatedAt) }}` 之后）：
```html
              <span v-if="hidden.includes(s.id)" class="hidden-tag">已隐藏</span>
```

- [ ] **Step 4: style 加 hide/hover-only/is-hidden/hidden-tag**

`src/components/Overlay.vue` `<style scoped>`，在 `.act-btn.copy.done { ... }`（约 571-574）之后追加：

```css
/* 已隐藏行更淡（比 dead/snoozed 更淡，强化"被收起"语义） */
.row.is-hidden { opacity: 0.35; }
.hidden-tag {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-tertiary);
  margin-left: var(--gap-xs);
}

/* 复制 ID：低频，仅 hover 行时显示（搁置/隐藏常驻） */
.act-btn.hover-only {
  opacity: 0;
  pointer-events: none;
  transition: opacity var(--motion-duration) var(--motion-easing);
}
.row:hover .act-btn.hover-only {
  opacity: 1;
  pointer-events: auto;
}
```

- [ ] **Step 5: 类型检查 + 手动验证**

Run: `npx vue-tsc --noEmit`
Expected: 无错误。

手动验证（`npm run tauri dev`）：
- 行内「隐藏」按钮：点后该行消失（showHidden off）；开「显示已隐藏」后该行以更淡 + 「已隐藏」标记出现，按钮变「取消隐藏」，点后恢复
- 搜索态同样可隐藏/取消隐藏

- [ ] **Step 6: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "feat(overlay): 行内加隐藏按钮+显示已隐藏过滤"
```

---

## Task 6: 前端项目名去重 + 分组视觉 + 复制ID hover 收尾

**Files:**
- Modify: `src/components/Overlay.vue`
  - `<template>`：分组态 `<li>` 删除 `.line2`（项目名重复）
  - `<style>`：`.group-head` 间距/字重提；`.proj-head` 改 mono + 色重提

**Interfaces:** 无新接口。

- [ ] **Step 1: 分组态删 line2**

`src/components/Overlay.vue` 分组态 `<li>`（约 280-287）里的：
```html
              <div class="info">
                <div class="line1">
                  <span class="name">{{ s.name || s.project }}</span>
                  <span class="status-zh" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
                </div>
                <div class="line2">{{ projShort(s.project) }}</div>
              </div>
```
改为（删 line2；分组有 proj-head 已显示项目名）：
```html
              <div class="info">
                <div class="line1">
                  <span class="name">{{ s.name || s.project }}</span>
                  <span class="status-zh" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
                </div>
              </div>
```

> 搜索态（扁平列表，无 proj-head）**保留** line2，不改。

- [ ] **Step 2: 分组视觉清晰化**

`src/components/Overlay.vue` `<style scoped>`，把 `.group-head`（约 401-410）改为（加大上下间距 + 字重提）：
```css
.group-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 12px var(--pad-x) 5px;
  font: 600 var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-muted);
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
```

把 `.proj-head`（约 421-425）改为（改 mono + 色重提，与一级标题层级拉开）：
```css
.proj-head {
  padding: 5px var(--pad-x) 2px;
  font: 600 var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-muted);
}
```

- [ ] **Step 3: 类型检查 + 手动验证**

Run: `npx vue-tsc --noEmit`
Expected: 无错误。

手动验证（`npm run tauri dev`）：
- 分组态行不再显示第二行项目名（与上方 proj-head 不重复）
- 搜索态行仍有项目名第二行
- 一级分组标题（待介入等）更扎实，二级项目名更清晰、mono 感
- 行 hover 时「复制 ID」按钮淡入出现

- [ ] **Step 4: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "style(overlay): 分组态去重项目名+分组视觉清晰化"
```

---

## Task 7: tray 图标加粗 + 原生菜单 + 左键弹菜单

**Files:**
- Modify: `src-tauri/icons/source/tray.svg`（描边加粗）
- Regenerate: `src-tauri/icons/tray.png`（44×44）
- Modify: `src-tauri/tauri.conf.json`（`showMenuOnLeftClick: true`）
- Modify: `src-tauri/src/lib.rs`
  - `use tauri::menu::{Menu, MenuItem};`（约 21）加 `PredefinedMenuItem`
  - 抽 `show_overlay(app: &AppHandle)` 函数（快捷键 + 菜单共用）
  - tray 菜单构建（约 552-564）扩展为 version/show/prefs/update/quit
  - `on_menu_event` 分发
  - 删 `on_tray_icon_event` 左键 toggle 逻辑（约 565-582）

**Interfaces:**
- Produces: tray 左键弹原生菜单；菜单「显示面板」与 ⌥Space 共用 `show_overlay`。

- [ ] **Step 1: 改 tray.svg 描边加粗**

`src-tauri/icons/source/tray.svg`，把圆环 `stroke-width="2.6"` 改为 `stroke-width="3.5"`（指针 2.8 保持）。整文件改为：

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="0 0 44 44">
  <circle cx="22" cy="22" r="14" fill="none" stroke="#000000" stroke-width="3.5" stroke-opacity="0.55"/>
  <line x1="22" y1="22" x2="35" y2="16" stroke="#000000" stroke-width="2.8" stroke-linecap="round"/>
  <circle cx="22" cy="22" r="3.8" fill="#000000"/>
</svg>
```

- [ ] **Step 2: 重新导出 tray.png（44×44）**

若未装 rsvg-convert：`brew install librsvg`。然后：

Run: `rsvg-convert -w 44 -h 44 src-tauri/icons/source/tray.svg -o src-tauri/icons/tray.png`
验证: `sips -g pixelWidth -g pixelHeight src-tauri/icons/tray.png` → 44×44。

> 备选（无 rsvg）：`npx --yes @resvg/resvg-js-cli --width 44 --height 44 src-tauri/icons/source/tray.svg src-tauri/icons/tray.png`（需网络）。

- [ ] **Step 3: tauri.conf.json 左键弹菜单**

`src-tauri/tauri.conf.json` 的 `app.trayIcon`，加 `"showMenuOnLeftClick": true`：
```json
    "trayIcon": {
      "id": "main",
      "iconPath": "icons/tray.png",
      "tooltip": "cc-view",
      "iconAsTemplate": true,
      "showMenuOnLeftClick": true
    },
```

- [ ] **Step 4: import 加 PredefinedMenuItem**

`src-tauri/src/lib.rs` 第 21 行：
```rust
use tauri::menu::{Menu, MenuItem};
```
改为：
```rust
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
```

- [ ] **Step 5: 抽 show_overlay 函数（快捷键 + 菜单共用）**

`src-tauri/src/lib.rs`，在 `frontmost_bundle_id` 函数之后（约 452，`pub fn run()` 之前）插入：

```rust
/// 呼出 overlay：恢复/居中位置 → show → makeKey → 启动失焦轮询。
/// 快捷键 ⌥Space 与 tray 菜单「显示面板」共用。
fn show_overlay(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("overlay") else { return };
    // show 前设 collectionBehavior + level，否则被钉在桌面 Space。
    #[cfg(target_os = "macos")]
    join_all_spaces(&w);
    if let Some(pos) = overlay_position::OverlayPosition::load() {
        let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
    } else {
        let _ = w.center();
    }
    let _ = w.show();
    #[cfg(target_os = "macos")]
    make_key(&w);
    #[cfg(not(target_os = "macos"))]
    let _ = w.set_focus();
    #[cfg(target_os = "macos")]
    join_all_spaces(&w);

    // 失焦轮询（钉住时按 pin 跳过 hide，见轮询内判断）
    #[cfg(target_os = "macos")]
    {
        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let stable_front = frontmost_bundle_id();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let Some(win) = app_handle.get_webview_window("overlay") else { break };
                if !win.is_visible().unwrap_or(false) { break; }
                if frontmost_bundle_id() != stable_front {
                    let pinned = app_handle
                        .state::<Mutex<bool>>()
                        .lock()
                        .map(|g| *g)
                        .unwrap_or(false);
                    if !pinned {
                        let _ = win.hide();
                        break;
                    }
                }
            }
        });
    }
}
```

- [ ] **Step 6: tray 菜单构建 + 删左键 toggle**

`src-tauri/src/lib.rs` 约 552-582，把整段（从 `// 构建 menubar 托盘菜单` 到 `on_tray_icon_event` 闭合的 `});`）替换为：

```rust
            // 构建 menubar 托盘菜单：版本号(只读) / 显示面板 / 偏好设置(占位) / 检查更新(占位) / 退出。
            let version = env!("CARGO_PKG_VERSION");
            let version_item = MenuItem::with_id(
                app.handle(),
                "version",
                &format!("cc-view {version}"),
                false,
                None::<&str>,
            )?;
            let sep1 = PredefinedMenuItem::separator(app.handle())?;
            let show_item =
                MenuItem::with_id(app.handle(), "show", "显示面板", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app.handle())?;
            let prefs_item = MenuItem::with_id(
                app.handle(),
                "prefs",
                "偏好设置…",
                false,
                None::<&str>,
            )?;
            let update_item = MenuItem::with_id(
                app.handle(),
                "update",
                "检查更新…",
                false,
                None::<&str>,
            )?;
            let sep3 = PredefinedMenuItem::separator(app.handle())?;
            let quit_item =
                MenuItem::with_id(app.handle(), "quit", "退出 cc-view", true, None::<&str>)?;
            let menu = Menu::with_items(
                app.handle(),
                &[
                    &version_item, &sep1, &show_item, &sep2, &prefs_item, &update_item, &sep3,
                    &quit_item,
                ],
            )?;

            // tray icon 已在 tauri.conf.json 声明（id="main"），取出附菜单。
            // 左键弹菜单（showMenuOnLeftClick: true）——不再 on_tray_icon_event toggle。
            let tray = app.tray_by_id("main").ok_or_else(|| {
                tauri::Error::AssetNotFound("tray icon 'main'".to_string())
            })?;
            tray.set_menu(Some(menu))?;
```

- [ ] **Step 7: 菜单事件分发**

`src-tauri/src/lib.rs` setup 闭包内，紧接 tray 菜单设置之后（上面替换段的 `tray.set_menu(Some(menu))?;` 之后）追加菜单事件处理：

```rust
            // 菜单事件：show → 呼出 overlay；quit → 退出。version/prefs/update 占位 no-op。
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "show" => show_overlay(app),
                "quit" => app.exit(0),
                _ => {}
            });
```

- [ ] **Step 8: 快捷键 handler 改用 show_overlay**

`src-tauri/src/lib.rs` 快捷键 handler（约 598-664），把 show 分支整段（从 `if w.is_visible()...` 的 else 分支起，到对应闭合）替换为调用 `show_overlay`。把：
```rust
                                if let Some(w) = app.get_webview_window("overlay") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        // ...（原有 show + 轮播整段，约 606-660）
                                    }
                                }
```
改为：
```rust
                                if let Some(w) = app.get_webview_window("overlay") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        show_overlay(app);
                                    }
                                }
```

> 这一步把原有内联的 show + join_all_spaces + center/set_position + make_key + 失焦轮播整段抽进了 `show_overlay`（Task 7 Step 5 已含 position 恢复 + pin 判断）。替换后原内联代码全部删除。

- [ ] **Step 9: build 确认通过**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过（原有快捷键内联代码已被 show_overlay 取代，无悬空引用）。

- [ ] **Step 10: Commit**

```bash
git add src-tauri/icons/source/tray.svg src-tauri/icons/tray.png src-tauri/tauri.conf.json src-tauri/src/lib.rs
git commit -m "feat: tray 图标加粗+原生菜单(版本/显示面板/偏好占位/更新占位/退出)+左键弹菜单"
```

---

## Task 8: 删 main 窗口 + 清理（破坏性收尾）

**Files:**
- Modify: `src-tauri/tauri.conf.json`（删 main window）
- Modify: `src-tauri/capabilities/default.json`（windows 改 `["overlay"]`）
- Delete: `src-tauri/src/hud.rs`
- Modify: `src-tauri/src/lib.rs`
  - mod 声明删 `mod hud;`（约第 7 行）
  - 删 `get_hud_pinned` / `set_hud_pinned`（约 319-342）
  - setup 删 main 窗口块（约 492-517：vibrancy + 位置恢复 + Moved）
  - `invoke_handler` 删 `get_hud_pinned,` / `set_hud_pinned,`
- Modify: `src/App.vue`（删 isOverlay 分流 + HUD 分支 + 相关状态/样式，直接渲染 Overlay）
- Delete: `src/components/SessionList.vue`

**Interfaces:** 无新接口；移除面向 main 的旧接口。

- [ ] **Step 1: tauri.conf.json 删 main window**

`src-tauri/tauri.conf.json` 的 `app.windows` 数组，删掉 `label: "main"` 的整个对象，只留 overlay 对象：

```json
    "windows": [
      {
        "label": "overlay",
        "url": "index.html",
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
    ],
```

- [ ] **Step 2: capabilities windows 改 overlay-only**

`src-tauri/capabilities/default.json`：
```json
  "windows": ["overlay"],
```
description 也改为 `"Capability for the overlay window"`。

- [ ] **Step 3: lib.rs 删 main setup + hud pin command + mod hud**

`src-tauri/src/lib.rs`：
1. 第 7 行删 `mod hud;`。
2. 删 `get_hud_pinned` / `set_hud_pinned` 两个函数及其上方注释（约 319-342 整段，从 `// --- HUD always-on-top` 到 `set_hud_pinned` 闭合 `}`）。
3. setup 内删 main 窗口块（约 488-517，从注释 `// 给 popover 窗口设原生 vibrancy` 起到 main 的 `on_window_event` 闭合 `});` + `}`）——**注意只删 `if let Some(w) = app.get_webview_window("main") {...}` 整块**，不要动后面 overlay 块。
4. `invoke_handler` 的 `generate_handler!` 删 `get_hud_pinned,` 和 `set_hud_pinned,` 两行。

- [ ] **Step 4: 删 hud.rs**

```bash
git rm src-tauri/src/hud.rs
```

- [ ] **Step 5: 后端 build 确认通过**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过（无 main 窗口、无 hud 引用）。

- [ ] **Step 6: App.vue 直接渲染 Overlay**

`src/App.vue` 整文件替换为（删 isOverlay 分流、HUD 状态/方法/样式，只留挂载 Overlay）：

```vue
<script setup lang="ts">
// 单窗口：overlay 承载全部 UI（原 HUD 已废弃合并）。App 仅挂载 Overlay。
import Overlay from './components/Overlay.vue';
</script>

<template>
  <Overlay />
</template>

<style>
/* 设计 token（dark 默认 + light 覆盖）——全局变量供 Overlay 使用 */
:root {
  --color-bg: transparent;
  --color-fg: #E5E5E7;
  --color-muted: #AEAEB2;
  --color-tertiary: #8E8E93;
  --color-primary: #0A84FF;
  --color-accent: #0A84FF;
  --color-border: rgba(255, 255, 255, 0.08);
  --color-hover: rgba(255, 255, 255, 0.08);
  --status-working: #30D158;
  --status-waiting: #0A84FF;
  --status-permission: #FF9F0A;
  --status-shell: #BF5AF2;
  --status-compacting: #64D2FF;
  --font-body: -apple-system, "PingFang SC", "SF Pro Text", sans-serif;
  --font-utility: "SF Mono", ui-monospace, "Menlo", monospace;
  --fs-display: 13px; --fw-display: 700; --lh-display: 1.3;
  --fs-body: 13px;    --fw-body: 600;    --lh-body: 1.25;
  --fs-caption: 11px; --fw-caption: 400; --lh-caption: 1.3;
  --fs-utility: 10px; --fw-utility: 400; --lh-utility: 1.3;
  --radius-hud: 10px; --radius-overlay: 12px;
  --row-hud: 36px; --row-overlay: 36px;
  --pad-x: 12px; --pad-y: 8px; --gap: 8px;
  --gap-sm: 6px; --gap-xs: 4px;
  --fs-control: 15px;
  --space-empty: 40px;
  --motion-duration: 160ms;
  --motion-easing: cubic-bezier(0.22, 1, 0.36, 1);
}
@media (prefers-color-scheme: light) {
  :root {
    --color-fg: #1D1D1F;
    --color-border: rgba(0, 0, 0, 0.08);
    --color-hover: rgba(0, 0, 0, 0.06);
    --color-muted: #6E6E73;
    --color-tertiary: #6E6E73;
  }
}
@keyframes breathe {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.5; }
}
@media (prefers-reduced-motion: reduce) {
  .status-icon--working { animation: none !important; }
}
* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--color-bg);
  font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  user-select: none;
  -webkit-user-select: none;
}
</style>
```

- [ ] **Step 7: 删 SessionList.vue**

```bash
git rm src/components/SessionList.vue
```

- [ ] **Step 8: 前端类型检查 + 全量 build**

Run: `npx vue-tsc --noEmit`
Expected: 无错误（App.vue 不再引用 SessionList / HUD 状态）。

Run: `npm run build`
Expected: vite build 成功。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor: 删 main(HUD) 窗口，overlay 成为唯一 UI"
```

---

## Task 9: 端到端验证

**Files:** 无（验证 only）。

- [ ] **Step 1: 启动 dev**

Run: `npm run tauri dev`（后台运行，等待编译 + 窗口出现）

- [ ] **Step 2: 手动验证清单**

逐一确认（⌥Space 呼出面板）：
1. 开机面板默认**不可见**（main 窗口已删，无残留窗口）
2. ⌥Space 呼出面板；再按收起
3. tray 左键 → 弹原生菜单：`cc-view 0.1.0`（灰）/ 显示面板 / 偏好设置…（灰）/ 检查更新…（灰）/ 退出 cc-view
4. 菜单「显示面板」呼出面板
5. 菜单「退出」退出 app
6. tray 图标为加粗仪表盘，深色菜单栏下白色；有等权限会话时右上角红点 badge 正常
7. 顶栏可拖动移动窗口；拖动后再呼出位置被记住（不再 center）
8. 图钉：未钉时点别处面板消失；钉住后点别处不消失；pin 状态重启后保留
9. 分组：待介入 / 已搁置 / 已退出 一级 + 项目名二级；行内**无**重复项目名；搜索态行有项目名
10. 行内：搁置/恢复、隐藏/取消隐藏（文字）、复制 ID（hover 出现）三按钮
11. 「显示已隐藏」toggle：off 隐藏 hidden 会话；on 显示（更淡 + 已隐藏标记）
12. 分组视觉：一级标题扎实、二级项目名 mono 清晰

- [ ] **Step 3: 截图存证（可选）**

用 gstack 截面板 + tray 菜单两张图，确认视觉符合预期。

- [ ] **Step 4: 收尾 commit（若有截图或最终微调）**

```bash
git add -A
git commit -m "chore: UI 大改造端到端验证通过" --allow-empty
```

---

## Self-Review 结论

- **Spec 覆盖**：spec 的 A1-A7、B1-B6 全部映射到 Task 1-9（A1 窗口合并→T8；A2 可见性→T8 conf+T3；A3 pin→T2；A4 拖动+位置→T3+T4；A5 失焦双机制→T2；A6 图标→T7；A7 菜单→T7；B1 顶栏→T4；B2 toggle→T4+T5；B3 三按钮→T5+T6；B4 去重→T6；B5 分组视觉→T6；B6 刷新省略→不做，无 task）。无遗漏。
- **占位扫描**：无 TBD/TODO（overlay_position 的 `unimplemented!` 是 TDD 红灯步骤，Step 3 即实现）。
- **类型一致**：`OverlayPosition { x, y, pinned }`、`get/set_overlay_pinned`、`show_overlay(app)`、前端 `pinned/hidden/showHidden` 跨 task 命名一致。
- **顺序安全**：破坏性删除集中在 T8，T1-T7 每步编译/类型检查通过、main 窗口与 hud.rs 在 T8 前仍可用（前端 get_hud_pinned 在 T8 前未被新代码调用，T8 同步清理）。
