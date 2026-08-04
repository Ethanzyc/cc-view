# cc-view Plan 5: 常驻指挥台（menubar 聚合 + 桌面 HUD） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 把现有"点击才弹的 popover"升级成常驻指挥台：① menubar 图标动态聚合状态（D，鼠标悬停看"几个等我"+ 有等我时图标变橙）；② 桌面常驻悬浮 HUD（A，always-on-top，一直可见，可拖动、位置记忆，menubar 点击 toggle 显示）。

**Architecture:** popover 窗口 "main" 改造成常驻 HUD（always_on_top + 默认 visible + 拖动 + 位置记忆）。轮询后算聚合状态 → tray.set_tooltip 文字 + tray.set_icon（有 NeedsPermission/WaitingInput → 橙图标）。menubar 点击从"贴 tray 临时弹"改成"toggle HUD show/hide"。HUD 拖动后位置存 `~/.claude/cc-view/hud-position.json`，启动恢复。

**Tech Stack:** Rust（tauri tray/window API、serde）、Vue 3、cargo test。

## Global Constraints

- macOS；零侵入；路径 dirs::home_dir()；代码英文/注释中文；fail fast。
- 复用现有 StatusIcon/SessionList/状态逻辑（Plan 1-4）。
- HUD 常驻但不抢焦点（always_on_top + 不激活 app，像 Spotlight 浮层）。

## Out of Scope（留 Plan 6）

E 全局快捷键呼出命令面板（Alfred/uTools 风格 + 搜索 + 操作）。

---

## File Structure

后端 `src-tauri/src/`：
- `lib.rs`（**改**）— 轮询后算聚合 + tray tooltip/icon；menubar 点击 toggle HUD；HUD 位置记忆
- `hud.rs`（**新**）— HudPosition 读写
- `tauri.conf.json`（**改**）— window "main" → always_on_top + visible + 拖动配置

前端 `src/`：
- `App.vue`（**改**）— 拖动 title 区 + HUD 样式（紧凑常驻形态）

---

### Task 1: menubar 图标动态聚合（D）

**Files:**
- Modify: `src-tauri/src/lib.rs`（轮询后算聚合 + set_tooltip + set_icon）
- Create: `src-tauri/icons/icon-orange.png`（橙版图标，有"等我"时用；或代码着色）

**Interfaces:**
- Consumes: `models::Status`、`tray_by_id("main")`

- [ ] **Step 1: 算聚合 + tooltip**

lib.rs start_poll_loop，reduce 后算聚合：
```rust
let need_attention = merged.iter().filter(|s| s.alive && matches!(
    s.status, models::Status::NeedsPermission | models::Status::WaitingForInput
)).count();
let working = merged.iter().filter(|s| s.alive && matches!(s.status, models::Status::Working)).count();
let tip = format!("{} 等我 · {} 工作", need_attention, working);
if let Some(tray) = handle.tray_by_id("main") {
    let _ = tray.set_tooltip(Some(tip));
    // 有"等我"→ 橙图标；否则默认
    let img = if need_attention > 0 { orange_icon() } else { default_icon() };
    let _ = tray.set_icon(Some(img));
}
```

- [ ] **Step 2: 橙图标**

两种方式任选（implementer 判断）：
- (a) 预制 `src-tauri/icons/icon-orange.png`（用 Image::from_path 加载）
- (b) 代码着色：把默认图标 RGBA 的非透明像素染橙（`tauri::image::Image` 操作 RGBA）
report 说明用了哪种。`default_icon()` 从 app.default_window_icon() 拿。

- [ ] **Step 3: 构建**

Run: `cargo build`
Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/icons/ 2>/dev/null
git commit -m "feat: dynamic menubar icon + tooltip aggregating session status"
```

---

### Task 2: popover → 常驻 HUD（always-on-top + 拖动 + visible）

**Files:**
- Modify: `src-tauri/tauri.conf.json`（window "main" 配置）
- Modify: `src/App.vue`（拖动 title 区）

- [ ] **Step 1: window 配置改常驻 HUD**

tauri.conf.json 的 app.windows[0]（label "main"）：
```json
{
  "label": "main",
  "visible": true,
  "decorations": false,
  "resizable": false,
  "width": 340,
  "height": 480,
  "skipTaskbar": true,
  "alwaysOnTop": true,
  "transparent": true,
  "shadow": true
}
```
（alwaysOnTop:true 常驻最前；visible:true 默认显示）

- [ ] **Step 2: 前端拖动 title 区**

App.vue 顶部加一个 title bar 区，`-webkit-app-region: drag`（让用户拖动 HUD）。按钮区 `no-drag`：
```vue
<div class="title-bar">
  <span class="title">Claude Code 会话</span>
  <span class="count">{{ activeCount }} 个活跃</span>
</div>
```
```css
.title-bar { -webkit-app-region: drag; ... }  /* 可拖动 */
.title-bar button, .toggle { -webkit-app-region: no-drag; }
```

- [ ] **Step 3: 构建**

Run: `npm run build` + `cargo build`
Expected: 通过。HUD 常驻显示 + 可拖动。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src/App.vue
git commit -m "feat: popover -> always-on-top persistent HUD with drag"
```

---

### Task 3: menubar 点击 toggle HUD + 位置记忆

**Files:**
- Modify: `src-tauri/src/lib.rs`（tray 点击 toggle show/hide；位置记忆）
- Create: `src-tauri/src/hud.rs`（HudPosition 读写）

**Interfaces:**
- Produces: `hud::HudPosition::load()/save()`；tray 点击 toggle window

- [ ] **Step 1: hud.rs 位置记忆**

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct HudPosition { pub x: i32, pub y: i32 }

impl HudPosition {
    pub fn load() -> Option<Self> {
        let path = dirs::home_dir()?.join(".claude/cc-view/hud-position.json");
        let txt = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&txt).ok()
    }
    pub fn save(x: i32, y: i32) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hud-position.json");
        if let Ok(json) = serde_json::to_string(&HudPosition { x, y }) {
            let _ = std::fs::write(path, json);
        }
    }
}
```
lib.rs 加 `mod hud;`。

- [ ] **Step 2: 启动恢复位置**

run() setup 里，popover window 创建后 + vibrancy 后，恢复位置：
```rust
if let Some(w) = app.get_webview_window("main") {
    // ... vibrancy ...
    if let Some(pos) = hud::HudPosition::load() {
        let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
    }
}
```

- [ ] **Step 3: 拖动后存位置**

HUD 拖动后存位置。两种方式：
- (a) Rust 端 window 的 on_window_event（Moved）拿位置存
- (b) 前端定时/拖动结束调 command 存
推荐 (a)：setup 里 `w.on_window_event(|e| if let WindowEvent::Moved(p) = e { hud::HudPosition::save(p.x, p.y); })`（核对 Tauri 2 WindowEvent::Moved 字段）。

- [ ] **Step 4: menubar 点击 toggle HUD**

tray `on_tray_icon_event` 改：左键点击 → toggle window show/hide（不再贴 tray 临时弹 set_position）：
```rust
.on_tray_icon_event(|tray, _event| {
    // 简化：任何点击 toggle（或只左键）
    let app = tray.app_handle();
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) { let _ = w.hide(); }
        else { let _ = w.show(); }
    }
})
```
（去掉之前的 set_position 贴 tray 逻辑——HUD 位置由用户拖动记忆。）

- [ ] **Step 5: 构建 + 测试**

Run: `cargo build` + `cargo test`
Expected: 通过。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/hud.rs src-tauri/src/lib.rs
git commit -m "feat: toggle HUD from menubar + remember position"
```

---

### Task 4: 冒烟 + README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 技术冒烟**

`npm run tauri dev`：HUD 常驻显示、可拖动、menubar 点击 toggle、位置重启恢复、tooltip 聚合、有等我时图标变橙。

- [ ] **Step 2: README**

更新：cc-view 现在是常驻指挥台（HUD + menubar 聚合）。移除"popover 精修"限制。注明快捷键呼出（E）在 Plan 6。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: persistent HUD command center"
```

---

## Self-Review 结论

- **Spec coverage**：Plan 5 覆盖 A（常驻 HUD）+ D（menubar 聚合）。E（快捷呼出）明确 Plan 6。✅
- **Placeholder scan**：tray icon 橙版给了两种实现方式（预制/着色）任选；WindowEvent::Moved 标注核对 API——非占位，是 API 确认。✅
- **Type consistency**：`HudPosition{x,y}` load/save；tray tooltip/icon；toggle window 一致。✅
- **注意**：Task 2 改 window visible:true 后，首次启动 HUD 默认显示（常驻）。Task 3 位置记忆恢复上次位置。
