# 常驻模式（Resident Mode）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 cc-view 的 overlay 窗口加一个"常驻模式"——精简会话列表贴桌面常驻、失焦不收起、透明度可调，与现有"面板模式"一键互切。

**Architecture:** 同一 overlay 窗口的两种模式（`panel`/`resident`），共享 sessions 数据源。模式存 `prefs.json`；`set_mode` 在后端 resize 窗口 + 重新定位，并 emit `mode_changed` 让前端切换视图。常驻模式 = always-pinned（失焦不 hide）。配置全在偏好设置，常驻面板零控件。

**Tech Stack:** Tauri 2（Rust）+ Vue 3 + TypeScript；objc2（macOS NSWindow 操作）；无前端测试框架（vue-tsc 类型检查 + 手动验证），Rust `cargo test` 单测。

## Global Constraints

- 平台：macOS 13+，Apple Silicon；accessory app（`set_activation_policy(Accessory)`，平时无 dock）。
- 交互/注释用**中文**，代码标识符用英文（遵循 `~/.claude/CLAUDE.md`）。
- 错误处理 fail fast：lock 失败 `eprintln!` + 静默跳过不崩溃；非法输入返回 `Err`。
- 偏好持久化路径：`~/.claude/cc-view/prefs.json`（`prefs.rs` 已封装 load/save）。
- 旧 `prefs.json` 必须向后兼容：缺字段走 `#[serde(default)]`。
- overlay 窗口已做 `join_all_spaces` + `make_panel`（NSPanel swizzle）+ vibrancy `Effect::Menu`，两模式共用，不重复设置。
- 提交用 Conventional Commits（项目惯例：`feat:` / `refactor:` / `test:` / `chore:`）。
- 设计常量（常驻窗口宽度）定义为本文件 `const`，不算硬编码违规：A 布局 = 150 logical px，B 布局 = 212 logical px。

## File Structure

**后端（Rust）**
- `src-tauri/src/prefs.rs` — Modify：`OverlayMode` / `ResidentLayout` 枚举 + `Prefs` 5 个新字段 + default + `is_valid_opacity` + 测试扩展。
- `src-tauri/src/lib.rs` — Modify：6 个新 command（`set_mode` / `set_resident_layout` / `set_resident_show_snoozed` / `set_resident_show_idle` / `set_resident_opacity` / `set_resident_height`）+ 注册 + resize/定位 helper + 失焦行为读 mode。

**前端（Vue/TS）**
- `src/types.ts` — Modify：`OverlayMode` / `ResidentLayout` TS 类型 + `Prefs` 接口扩展。
- `src/App.vue` — Modify：overlay 窗口按 `mode` 分发 `PanelView`/`ResidentView`。
- `src/components/PanelView.vue` — Rename from `Overlay.vue` + 加"收起成常驻"按钮。
- `src/components/ResidentView.vue` — Create：常驻视图（B/A 布局 + 过滤 + 点行 focus + 展开入口 + 透明度 + 高度自适应）。
- `src/components/Preferences.vue` — Modify：新增「常驻面板」section。

---

## Task 1: prefs 数据模型 + opacity 校验（TDD）

**Files:**
- Modify: `src-tauri/src/prefs.rs`
- Test: `src-tauri/src/prefs.rs`（`#[cfg(test)]` 块）

**Interfaces:**
- Produces: `prefs::OverlayMode`（`Resident`/`Panel`，serde lowercase）、`prefs::ResidentLayout`（`B`/`A`）、`Prefs` 新字段 `mode` / `resident_layout` / `resident_show_snoozed` / `resident_show_idle` / `resident_opacity: u8`、`Prefs::is_valid_opacity(u8) -> bool`。

- [ ] **Step 1: 写失败测试**

在 `prefs.rs` 现有 `#[cfg(test)] mod tests` 块末尾追加：

```rust
    #[test]
    fn empty_json_uses_resident_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p.mode, OverlayMode::Resident);
        assert_eq!(p.resident_layout, ResidentLayout::B);
        assert!(p.resident_show_snoozed);
        assert!(p.resident_show_idle);
        assert_eq!(p.resident_opacity, 55);
    }

    #[test]
    fn partial_json_keeps_new_defaults_for_missing() {
        // 现有字段设了非默认值，新字段缺失 → 新字段填默认
        let p: Prefs =
            serde_json::from_str(r#"{"notify":false,"shortcut":"ctrl+space"}"#).unwrap();
        assert!(!p.notify);
        assert_eq!(p.mode, OverlayMode::Resident);
        assert_eq!(p.resident_layout, ResidentLayout::B);
        assert_eq!(p.resident_opacity, 55);
    }

    #[test]
    fn full_json_with_new_fields_roundtrip() {
        let p = Prefs {
            notify: false,
            shortcut: "ctrl+space".into(),
            poll_interval: 10,
            mode: OverlayMode::Panel,
            resident_layout: ResidentLayout::A,
            resident_show_snoozed: false,
            resident_show_idle: false,
            resident_opacity: 80,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn mode_and_layout_serde_lowercase() {
        let m: OverlayMode = serde_json::from_str("\"resident\"").unwrap();
        assert_eq!(m, OverlayMode::Resident);
        assert_eq!(serde_json::to_string(&OverlayMode::Panel).unwrap(), "\"panel\"");
        let l: ResidentLayout = serde_json::from_str("\"a\"").unwrap();
        assert_eq!(l, ResidentLayout::A);
        assert_eq!(serde_json::to_string(&ResidentLayout::B).unwrap(), "\"b\"");
    }

    #[test]
    fn is_valid_opacity_bounds() {
        assert!(!Prefs::is_valid_opacity(0));
        assert!(!Prefs::is_valid_opacity(19));
        assert!(Prefs::is_valid_opacity(20));
        assert!(Prefs::is_valid_opacity(55));
        assert!(Prefs::is_valid_opacity(100));
        assert!(!Prefs::is_valid_opacity(101));
        assert!(!Prefs::is_valid_opacity(255));
    }
```

- [ ] **Step 2: 运行测试，确认编译失败**

Run: `cd src-tauri && cargo test --lib prefs`
Expected: 编译失败——`OverlayMode` / `ResidentLayout` 未定义、`Prefs` 无新字段、`is_valid_opacity` 不存在。同时旧测试 `empty_json_uses_defaults` / `partial_json_keeps_defaults_for_missing` / `full_json_roundtrip` 也会因 `Prefs` 字段变化编译失败（Task 1 末尾会修）。

- [ ] **Step 3: 实现枚举 + 字段 + default + 校验**

在 `prefs.rs` 顶部（`ALLOWED_SHORTCUTS` 之后、`Prefs` 结构体之前）加枚举与 default 函数：

```rust
/// overlay 窗口模式：常驻精简 / 面板全功能。serde lowercase（json 里 "resident"/"panel"）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum OverlayMode {
    Resident,
    Panel,
}

/// 常驻模式布局：B 精简（分组+状态文字）/ A 极简（仅图标+名称）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ResidentLayout {
    B,
    A,
}

fn default_mode() -> OverlayMode {
    OverlayMode::Resident
}
fn default_layout() -> ResidentLayout {
    ResidentLayout::B
}
fn default_show() -> bool {
    true
}
fn default_opacity() -> u8 {
    55
}
```

把 `Prefs` 结构体改为（保留现有三字段，追加五字段）：

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prefs {
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    #[serde(default = "default_interval")]
    pub poll_interval: u64,
    #[serde(default = "default_mode")]
    pub mode: OverlayMode,
    #[serde(default = "default_layout")]
    pub resident_layout: ResidentLayout,
    #[serde(default = "default_show")]
    pub resident_show_snoozed: bool,
    #[serde(default = "default_show")]
    pub resident_show_idle: bool,
    #[serde(default = "default_opacity")]
    pub resident_opacity: u8,
}
```

更新 `impl Default for Prefs`：

```rust
impl Default for Prefs {
    fn default() -> Self {
        Self {
            notify: true,
            shortcut: default_shortcut(),
            poll_interval: default_interval(),
            mode: default_mode(),
            resident_layout: default_layout(),
            resident_show_snoozed: default_show(),
            resident_show_idle: default_show(),
            resident_opacity: default_opacity(),
        }
    }
}
```

在 `impl Prefs` 块里（`is_valid_shortcut` 旁）加：

```rust
    /// 常驻背景透明度合法范围 20–100（百分比）。
    pub fn is_valid_opacity(n: u8) -> bool {
        (20..=100).contains(&n)
    }
```

修旧测试 `full_json_roundtrip`（它构造了完整 `Prefs`，需补新字段）。把其 `let p = Prefs { ... }` 改为：

```rust
        let p = Prefs {
            notify: false,
            shortcut: "ctrl+space".into(),
            poll_interval: 10,
            mode: OverlayMode::Panel,
            resident_layout: ResidentLayout::A,
            resident_show_snoozed: false,
            resident_show_idle: false,
            resident_opacity: 80,
        };
```

（旧 `empty_json_uses_defaults` / `partial_json_keeps_defaults_for_missing` / `invalid_json_falls_back_to_default` / `is_valid_shortcut_checks_allowed` 不需改——它们不构造完整 `Prefs`。）

- [ ] **Step 4: 运行测试，确认通过**

Run: `cd src-tauri && cargo test --lib prefs`
Expected: PASS（所有 prefs 测试，含新增 5 个 + 旧 5 个）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/prefs.rs
git commit -m "feat(prefs): 新增常驻模式配置字段（mode/layout/show_snoozed/show_idle/opacity）"
```

---

## Task 2: 配置类 commands（非 set_mode）+ 注册

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `prefs::ResidentLayout`、`prefs::Prefs::is_valid_opacity`（来自 Task 1）。
- Produces: commands `set_resident_layout(layout: ResidentLayout, state)`、`set_resident_show_snoozed(bool, state)`、`set_resident_show_idle(bool, state)`、`set_resident_opacity(u8, state) -> Result<(), String>`。后两者/布局的副作用（窗口 resize）在 Task 3/4 接入；本 task 只持久化。

- [ ] **Step 1: 加四个 command**

在 `lib.rs` 的 `set_interval` command 之后（偏好 commands 区）加：

```rust
/// 设置常驻布局（B 精简 / A 极简）：存 prefs。窗口宽度调整由前端切布局后量高时连带处理，
/// 或常驻显示时由 set_resident_height 路径统一；这里只持久化，保持 command 单一职责。
#[tauri::command]
fn set_resident_layout(
    layout: prefs::ResidentLayout,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_layout = layout;
        p.save();
    }
}

/// 切换常驻模式是否显示搁置的会话：存 prefs（前端响应式过滤）。
#[tauri::command]
fn set_resident_show_snoozed(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_show_snoozed = show;
        p.save();
    }
}

/// 切换常驻模式是否显示闲置（等输入超时）的会话：存 prefs（前端响应式过滤）。
#[tauri::command]
fn set_resident_show_idle(
    show: bool,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_show_idle = show;
        p.save();
    }
}

/// 设置常驻背景透明度（20–100）：校验失败返回 Err（fail fast），合法则存 prefs。
#[tauri::command]
fn set_resident_opacity(
    opacity: u8,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
) -> Result<(), String> {
    if !prefs::Prefs::is_valid_opacity(opacity) {
        return Err(format!("opacity must be 20-100, got {}", opacity));
    }
    if let Ok(mut p) = state.lock() {
        p.resident_opacity = opacity;
        p.save();
    }
    Ok(())
}
```

- [ ] **Step 2: 注册到 invoke_handler**

在 `run()` 的 `generate_handler!` 宏列表里（`set_interval` 之后）加四项：

```rust
            set_interval,
            set_resident_layout,
            set_resident_show_snoozed,
            set_resident_show_idle,
            set_resident_opacity
```

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo build`
Expected: 编译通过（这些 command 依赖的 `prefs::ResidentLayout` 等已在 Task 1 就绪）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(commands): 常驻布局/显隐/透明度配置 commands"
```

---

## Task 3: set_mode + resize/定位 helper + 注册

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `prefs::OverlayMode`、`prefs::ResidentLayout`、overlay 窗口（`app.get_webview_window("overlay")`）、`OverlayPosition::load`。
- Produces: command `set_mode(mode: OverlayMode, state, app) -> Result<(), String>`，副作用：存 prefs + emit `mode_changed` + resize 窗口 + 重新定位。helper `resident_layout_width(ResidentLayout) -> f64`、`apply_mode_window(app, mode, layout)`。

- [ ] **Step 1: 加宽度常量 + helper**

在 `lib.rs` 顶部（`start_poll_loop` 之前，靠近其他自由函数处）加：

```rust
/// 常驻窗口宽度（logical px）。A 极简最窄，B 精简需容纳"名称 + 状态中文"。
const RESIDENT_WIDTH_A: f64 = 150.0;
const RESIDENT_WIDTH_B: f64 = 212.0;

fn resident_layout_width(layout: prefs::ResidentLayout) -> f64 {
    match layout {
        prefs::ResidentLayout::A => RESIDENT_WIDTH_A,
        prefs::ResidentLayout::B => RESIDENT_WIDTH_B,
    }
}

/// 面板模式窗口尺寸（logical px，与 tauri.conf.json overlay width/height 一致）。
const PANEL_W: f64 = 560.0;
const PANEL_H: f64 = 420.0;

/// 把 overlay 窗口尺寸 + 位置切到目标模式。
/// - panel：560×420；位置用 overlay-position.json 记忆，无则 center。
/// - resident：宽度按 layout（高度先沿用当前值，随后前端量内容 invoke set_resident_height 校正）；
///   位置用记忆，无记忆则屏幕右上角（menubar 下方 8px 边距）。
/// menubar 高度按 28 logical 估算（retina 实测 ~24–37）；如不准可改用 NSScreen visibleFrame。
fn apply_mode_window(app: &tauri::AppHandle, mode: prefs::OverlayMode, layout: prefs::ResidentLayout) {
    let Some(w) = app.get_webview_window("overlay") else { return };

    match mode {
        prefs::OverlayMode::Panel => {
            let _ = w.set_size(tauri::LogicalSize::new(PANEL_W, PANEL_H));
            if let Some(pos) = overlay_position::OverlayPosition::load() {
                let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
            } else {
                let _ = w.center();
            }
        }
        prefs::OverlayMode::Resident => {
            let width = resident_layout_width(layout);
            // 高度先沿用当前（面板 420 或上次常驻值），前端量内容后 set_resident_height 校正。
            let cur_h = w
                .outer_size()
                .map(|s| s.to_logical::<f64>(w.scale_factor()).height)
                .unwrap_or(PANEL_H);
            let _ = w.set_size(tauri::LogicalSize::new(width, cur_h));

            if let Some(pos) = overlay_position::OverlayPosition::load() {
                let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
            } else {
                // 右上角：screen 右 - 窗口宽 - 8；top + menubar(28) + 4。
                if let Ok(Some(mon)) = w.current_monitor() {
                    let sf = mon.scale_factor();
                    let screen_w = mon.size().width as f64 / sf;
                    let x = mon.position().x as f64 / sf + screen_w - width - 8.0;
                    let y = mon.position().y as f64 / sf + 28.0 + 4.0;
                    let _ = w.set_position(tauri::LogicalPosition::new(x, y));
                }
            }
        }
    }
}
```

- [ ] **Step 2: 加 set_mode command**

在 Task 2 的四个 command 之后加：

```rust
/// 切换 overlay 模式（panel / resident）：存 prefs + resize 窗口 + 重新定位 + emit mode_changed
/// 让前端（overlay 窗口）切换视图。prefs 窗口的 set_mode 调用同样 emit，overlay 会响应。
#[tauri::command]
fn set_mode(
    mode: prefs::OverlayMode,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let layout = state.lock().map(|p| p.resident_layout).unwrap_or(prefs::ResidentLayout::B);
    if let Ok(mut p) = state.lock() {
        p.mode = mode;
        p.save();
    }
    apply_mode_window(&app, mode, layout);
    let _ = app.emit("mode_changed", mode);
    Ok(())
}
```

> `Emitter` trait 已在文件顶部 `use tauri::{Emitter, Manager};` 引入（现有代码 emit `sessions` 已用）。

- [ ] **Step 3: 注册 set_mode**

在 `generate_handler!` 列表加 `set_mode`（放 `set_resident_opacity` 之后）：

```rust
            set_resident_opacity,
            set_mode
```

- [ ] **Step 4: 编译确认**

Run: `cd src-tauri && cargo build`
Expected: 编译通过。`outer_size` / `current_monitor` / `to_logical` 都是 Tauri 2 `WebviewWindow` 方法。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(commands): set_mode 切换模式 + 窗口 resize/定位 + mode_changed 事件"
```

---

## Task 4: set_resident_height command + 注册

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `prefs.resident_layout`（决定宽度）、`prefs.mode`（仅 resident 生效）。
- Produces: command `set_resident_height(height: f64, state, app)`——前端量得常驻内容高度后校正窗口高度。

- [ ] **Step 1: 加 command**

在 `set_mode` 之后加：

```rust
/// 校正常驻窗口高度为内容实际高度（前端量得 offsetHeight 后调用）。
/// 仅当前 mode==resident 时生效——避免面板模式被误改。宽度按当前 resident_layout。
#[tauri::command]
fn set_resident_height(
    height: f64,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    let (mode, layout) = state
        .lock()
        .map(|p| (p.mode, p.resident_layout))
        .unwrap_or((prefs::OverlayMode::Resident, prefs::ResidentLayout::B));
    if mode != prefs::OverlayMode::Resident {
        return;
    }
    let width = resident_layout_width(layout);
    let Some(w) = app.get_webview_window("overlay") else { return };
    let _ = w.set_size(tauri::LogicalSize::new(width, height));
}
```

- [ ] **Step 2: 注册**

`generate_handler!` 列表加 `set_resident_height`：

```rust
            set_mode,
            set_resident_height
```

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo build`
Expected: 编译通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(commands): set_resident_height 按内容校正常驻窗口高度"
```

---

## Task 5: 失焦行为读 mode（常驻 = always-pinned）

**Files:**
- Modify: `src-tauri/src/lib.rs`（`setup` 里的 `on_window_event(Focused(false))` handler、`show_overlay` 的 frontmost 轮询线程）

**Interfaces:**
- Consumes: `prefs::OverlayMode`（从 `Mutex<Prefs>` state 读）。
- Produces: 失焦 hide 逻辑在 `mode == Resident` 时跳过。

- [ ] **Step 1: 改 on_window_event 的 Focused(false) handler**

定位 `run()` → `setup` 里 `overlay.on_window_event(move |e| match e { ... tauri::WindowEvent::Focused(false) => { ... } })`。把该分支改为读 mode：

```rust
                    tauri::WindowEvent::Focused(false) => {
                        // 常驻模式 = always-pinned（失焦不收起）；面板模式按 pin 决定。
                        let mode = app_handle
                            .state::<Mutex<prefs::Prefs>>()
                            .lock()
                            .map(|p| p.mode)
                            .unwrap_or(prefs::OverlayMode::Resident);
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        if mode != prefs::OverlayMode::Resident && !pinned {
                            let _ = w.hide();
                        }
                    }
```

> 该闭包已 `move` 捕获 `app_handle`（现有代码读 `Mutex<bool>` 已用 `app_handle.state`）。确认闭包捕获的 `app_handle` 在作用域内——现有代码 `let app_handle = app.handle().clone();` 已在 `overlay.on_window_event` 前。

- [ ] **Step 2: 改 frontmost 轮询线程**

定位 `show_overlay` 末尾的 `std::thread::spawn(move || { ... loop { ... if current_front != stable_front { ... if !pinned { hide } } } })`。在 hide 判断里加 mode 检查——把：

```rust
                    if current_front != stable_front {
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        if !pinned {
                            let _ = win.hide();
                            break;
                        }
                        // pinned: 静默 continue
                    }
```

改为：

```rust
                    if current_front != stable_front {
                        // 常驻模式不收起；面板模式按 pin 决定。
                        let mode = app_handle
                            .state::<Mutex<prefs::Prefs>>()
                            .lock()
                            .map(|p| p.mode)
                            .unwrap_or(prefs::OverlayMode::Resident);
                        let pinned = app_handle
                            .state::<Mutex<bool>>()
                            .lock()
                            .map(|g| *g)
                            .unwrap_or(false);
                        if mode != prefs::OverlayMode::Resident && !pinned {
                            let _ = win.hide();
                            break;
                        }
                        // resident 或 pinned：静默 continue（不收起）
                    }
```

> 注意：该线程闭包捕获的 `app_handle` 是 `show_overlay` 入参 `app: &tauri::AppHandle` 的 clone（`let app_handle = app.clone();`）。`.state::<Mutex<prefs::Prefs>>()` 在 `tauri::AppHandle` 上可用（`Manager` trait）。

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo build`
Expected: 编译通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(overlay): 常驻模式失焦不收起（onFocused + frontmost 轮询读 mode）"
```

---

## Task 6: 前端类型

**Files:**
- Modify: `src/types.ts`

**Interfaces:**
- Produces: `OverlayMode`（`'resident' | 'panel'`）、`ResidentLayout`（`'b' | 'a'`）类型；`Prefs` 接口扩展（供 Preferences/App 读 `get_prefs` 返回）。

- [ ] **Step 1: 加类型**

在 `src/types.ts` 末尾追加：

```ts
// overlay 窗口模式（与后端 prefs::OverlayMode serde lowercase 对齐）
export type OverlayMode = 'resident' | 'panel';
// 常驻布局（与后端 prefs::ResidentLayout serde lowercase 对齐）
export type ResidentLayout = 'b' | 'a';

// get_prefs 返回的完整偏好（与 Rust Prefs 字段一一对应）
export interface Prefs {
  notify: boolean;
  shortcut: string;
  poll_interval: number;
  mode: OverlayMode;
  resident_layout: ResidentLayout;
  resident_show_snoozed: boolean;
  resident_show_idle: boolean;
  resident_opacity: number;
}
```

- [ ] **Step 2: 类型检查**

Run: `npm run build`
Expected: vue-tsc 通过（`dist/` 产出）。现有代码未引用新类型，不影响。

- [ ] **Step 3: 提交**

```bash
git add src/types.ts
git commit -m "feat(types): OverlayMode / ResidentLayout / Prefs 类型"
```

---

## Task 7: Overlay.vue → PanelView.vue 改名 + 收起按钮

**Files:**
- Rename: `src/components/Overlay.vue` → `src/components/PanelView.vue`
- Modify: `src/App.vue`（import + 模板）、`src/components/PanelView.vue`（加收起按钮）

**Interfaces:**
- Consumes: `invoke('set_mode', { mode: 'resident' })`（Task 3）。
- Produces: `PanelView` 组件（原 Overlay 全功能 + 收起按钮）。

- [ ] **Step 1: 改名**

```bash
git mv src/components/Overlay.vue src/components/PanelView.vue
```

- [ ] **Step 2: 改 App.vue import + 注释**

把 `App.vue` 顶部 import 与注释里的 `Overlay` 改为 `PanelView`：

```ts
import PanelView from './components/PanelView.vue';
```

注释 `// 多视图单入口：overlay 承载命令面板，prefs 承载偏好设置。按 window label 分发。` 保留（mode 分发在 Task 8 接入；本步仅改名，行为不变）。模板仍是 `<PanelView v-else />`。

- [ ] **Step 3: PanelView 加"收起成常驻"按钮**

在 `PanelView.vue` 模板的图钉按钮（`.pin-btn`）之后、`</div>`（关闭 `.search-bar`）之前加收起按钮：

```html
      <button
        class="collapse-btn"
        title="收起成常驻（精简面板）"
        aria-label="收起成常驻（精简面板）"
        data-tauri-drag-region="false"
        @click="collapseToResident"
      >
        <!-- 收起/最小化图标（Lucide minimize 之类）：四角向内 -->
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M8 3v3a2 2 0 0 1-2 2H3" />
          <path d="M21 8h-3a2 2 0 0 1-2-2V3" />
          <path d="M3 16h3a2 2 0 0 1 2 2v3" />
          <path d="M16 21v-3a2 2 0 0 1 2-2h3" />
        </svg>
      </button>
```

在 `<script setup>` 加函数（放在 `togglePin` 之后）：

```ts
// 收起成常驻模式：调后端 set_mode，后端 emit mode_changed → App.vue 切 ResidentView。
async function collapseToResident() {
  try {
    await invoke('set_mode', { mode: 'resident' });
  } catch (e) {
    console.error('set_mode(resident) failed', e);
  }
}
```

在 `<style scoped>` 加（仿 `.pin-btn`）：

```css
.collapse-btn {
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
.collapse-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.collapse-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}
```

- [ ] **Step 4: 类型检查 + 手动验证**

Run: `npm run build`
Expected: vue-tsc 通过。

手动（`npm run tauri dev`）：⌥Space 呼出面板 → 搜索栏右侧出现收起按钮 → 点它 → 窗口 resize 到常驻尺寸、定位右上角（此时还没有 ResidentView，Task 8 才显示；此步仅验证后端 set_mode 生效，窗口变小即成功）。验证后可 `Cmd+C` 停 dev。

- [ ] **Step 5: 提交**

```bash
git add src/App.vue src/components/PanelView.vue
git commit -m "refactor: Overlay.vue → PanelView.vue + 收起成常驻按钮"
```

---

## Task 8: App.vue mode 分发 + ResidentView 骨架（B 布局）

**Files:**
- Modify: `src/App.vue`
- Create: `src/components/ResidentView.vue`

**Interfaces:**
- Consumes: `invoke<Prefs>('get_prefs')`、`listen<OverlayMode>('mode_changed')`、`invoke('set_mode', { mode: 'panel' })`、`invoke<Session[]>('get_sessions')`、`listen<Session[]>('sessions')`、`utils/session.ts`（`STATUS_ZH`/`statusRank`/`projShort`/`isStaleInput`）。
- Produces: `ResidentView` 组件（B 布局基础 + 展开入口）；App.vue 按 `mode` 分发。

- [ ] **Step 1: 改 App.vue 为 mode 分发**

把 `App.vue` 的 `<script setup>` 改为：

```ts
// 多视图单入口：
//   prefs 窗口 → Preferences
//   overlay 窗口 → 按 mode 分发 PanelView（命令面板）/ ResidentView（常驻精简）
// mode 初值取 get_prefs；set_mode 成功后端 emit "mode_changed"，App 更新 ref 即时切视图。
import { ref, onMounted, computed } from 'vue';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import PanelView from './components/PanelView.vue';
import ResidentView from './components/ResidentView.vue';
import Preferences from './components/Preferences.vue';
import type { OverlayMode } from './types';

const isPrefs = computed(() => getCurrentWebviewWindow().label === 'prefs');
// overlay 模式 ref（prefs 窗口不用，但 ref 无害；listen 仅 overlay 窗口有意义）。
const mode = ref<OverlayMode>('resident');

onMounted(async () => {
  if (isPrefs.value) return;
  try {
    const p = await invoke<{ mode: OverlayMode }>('get_prefs');
    mode.value = p.mode;
  } catch (e) {
    console.error('get_prefs mode failed', e);
  }
  try {
    await listen<OverlayMode>('mode_changed', e => { mode.value = e.payload; });
  } catch (e) {
    console.error('listen mode_changed failed', e);
  }
});
```

把 `<template>` 改为：

```html
<template>
  <Preferences v-if="isPrefs" />
  <PanelView v-else-if="mode === 'panel'" />
  <ResidentView v-else />
</template>
```

`<style>`（:root token 等）保持不变。

- [ ] **Step 2: 建 ResidentView.vue 骨架（B 布局 + 展开入口）**

创建 `src/components/ResidentView.vue`：

```vue
<script setup lang="ts">
// 常驻模式视图：精简会话列表，贴桌面常驻、失焦不收起（后端控制）。
// 数据自管（listen sessions），排序/分组/ago 复用 utils/session.ts（MVP 与 PanelView 重复可接受）。
// 展开入口（右上角）调 set_mode(panel) → App.vue 切 PanelView。
// 本骨架先做 B 布局（分组 + 项目标题 + 图标+名称+状态）；A 布局/过滤/透明度/高度后续 task 接入。
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { Session } from '../types';
import StatusIcon from './StatusIcon.vue';
import { STATUS_ZH, projShort, isStaleInput } from '../utils/session';

const all = ref<Session[]>([]);
// now tick：isStaleInput 依赖时间，需前端定期刷新（后端 emit 有 hash 去重不随时间触发）。
const now = ref(Date.now());
let nowTimer: number | undefined;
const rootEl = ref<HTMLElement>();

// 非搜索态分组（与 PanelView 一致算法，MVP 重复）：待介入 / 已搁置；dead 不进常驻（盯桌面只看活的）。
type Section = { key: string; label: string; total: number; projs: [string, Session[]][] };
const DEAD_LIMIT = 5;
const groups = computed<Section[]>(() => {
  const list = all.value.filter(s => s.alive); // 常驻只看活会话
  const active = list.filter(s => !s.snoozed);
  const snoozedAlive = list.filter(s => s.snoozed);
  const byProj = (rows: Session[]): [string, Session[]][] => {
    const m = new Map<string, Session[]>();
    for (const s of rows) {
      const arr = m.get(s.project);
      if (arr) arr.push(s);
      else m.set(s.project, [s]);
    }
    return [...m.entries()];
  };
  const result: Section[] = [];
  if (active.length) {
    const n = now.value;
    // 全闲置 project 沉底（与 PanelView 一致）
    const activeProjs = byProj(active).sort((a, b) => {
      const aStale = a[1].every(s => isStaleInput(s, n));
      const bStale = b[1].every(s => isStaleInput(s, n));
      if (aStale !== bStale) return aStale ? 1 : -1;
      return 0;
    });
    result.push({ key: 'active', label: '待介入', total: active.length, projs: activeProjs });
  }
  if (snoozedAlive.length) {
    result.push({ key: 'snoozed', label: '已搁置', total: snoozedAlive.length, projs: byProj(snoozedAlive) });
  }
  return result;
});

async function focusSession(id: string) {
  try {
    await invoke('focus_session', { id });
  } catch (e) {
    console.error('focus_session failed', e);
  }
}

// 展开成面板模式：后端 set_mode + emit mode_changed → App 切 PanelView。
async function expandToPanel() {
  try {
    await invoke('set_mode', { mode: 'panel' });
  } catch (e) {
    console.error('set_mode(panel) failed', e);
  }
}

onMounted(async () => {
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
  }
  try {
    await listen<Session[]>('sessions', e => { all.value = e.payload; });
  } catch (e) {
    console.error('resident listen sessions failed', e);
  }
  nowTimer = window.setInterval(() => { now.value = Date.now(); }, 60_000);
});

onBeforeUnmount(() => {
  if (nowTimer) clearInterval(nowTimer);
});
</script>

<template>
  <div class="resident" ref="rootEl" data-tauri-drag-region="deep">
    <button
      class="expand-btn"
      title="展开成命令面板"
      aria-label="展开成命令面板"
      data-tauri-drag-region="false"
      @click="expandToPanel"
    >
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M5 11 L11 5" /><path d="M6 5 H11 V10" />
      </svg>
    </button>
    <template v-for="(section, si) in groups" :key="section.key">
      <div v-if="si > 0" class="group-sep" />
      <div class="group-head">
        {{ section.label }} <span class="cnt">{{ section.total }}</span>
      </div>
      <template v-for="[proj, rows] in section.projs" :key="section.key + '|' + proj">
        <div class="proj-head">{{ projShort(proj) }}</div>
        <div
          v-for="s in rows"
          :key="s.id"
          class="row"
          :class="{
            perm: s.status === 'needsPermission' && !s.snoozed,
            reply: s.status === 'waitingForReply' && !s.snoozed,
            dim: s.snoozed || isStaleInput(s, now),
          }"
          role="button"
          tabindex="0"
          :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
          data-tauri-drag-region="false"
          @click="focusSession(s.id)"
          @keydown.enter.prevent="focusSession(s.id)"
        >
          <StatusIcon :status="s.status" class="icon" />
          <span class="name">{{ s.name || s.project }}</span>
          <span class="st" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
        </div>
      </template>
    </template>
    <div v-if="!groups.length" class="empty">暂无会话</div>
  </div>
</template>

<style scoped>
.resident {
  position: relative;
  background: var(--color-bg-overlay);
  color: var(--color-fg);
  min-height: 100vh;
  max-height: 60vh;
  overflow-y: auto;
  border-radius: var(--radius-overlay);
  font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
  padding: 6px 0 8px;
}
.resident::-webkit-scrollbar { width: 6px; }
.resident::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }

.expand-btn {
  position: absolute; top: 5px; right: 6px;
  width: 18px; height: 18px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 5px; border: none; background: none;
  color: var(--color-tertiary); cursor: pointer; padding: 0;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.expand-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.expand-btn:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 1px; }

.group-head {
  display: flex; align-items: center; gap: 5px;
  padding: 9px 12px 3px;
  font: 600 9px/1 var(--font-utility);
  letter-spacing: 0.06em; text-transform: uppercase;
  color: var(--color-muted);
}
.group-head .cnt {
  color: var(--color-tertiary); background: var(--color-border);
  border-radius: 7px; padding: 0 5px; font-size: 9px; line-height: 13px;
}
.proj-head {
  padding: 4px 12px 1px;
  font: 600 10px/1 var(--font-utility);
  color: var(--color-muted);
}
.group-sep { height: 1px; background: var(--color-border); margin: 4px 10px 0; }

.row {
  display: flex; align-items: center; gap: 8px;
  height: 26px; padding: 0 12px;
  border-left: 2px solid transparent;
  cursor: pointer;
  transition: background var(--motion-duration) var(--motion-easing);
}
.row:hover { background: var(--color-hover); }
.row:focus-visible { outline: 2px solid var(--color-primary); outline-offset: -2px; }
.row.perm { border-left-color: var(--status-permission); background: color-mix(in srgb, var(--status-permission) 10%, transparent); }
.row.perm:hover { background: color-mix(in srgb, var(--status-permission) 16%, transparent); }
.row.reply { border-left-color: var(--status-reply); }
.row.dim { opacity: 0.45; }

.icon { flex-shrink: 0; }
.name {
  flex: 1; min-width: 0;
  font: 600 12px/1 var(--font-body);
  color: var(--color-fg);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.st {
  flex-shrink: 0; padding-left: 8px;
  font: 400 10px/1 var(--font-body);
  color: var(--color-muted);
}
.st.perm { color: var(--status-permission); }

.empty {
  padding: 32px 12px; text-align: center;
  font: 600 12px/1.3 var(--font-body);
  color: var(--color-tertiary);
}
</style>
```

- [ ] **Step 3: 类型检查 + 手动验证**

Run: `npm run build`
Expected: vue-tsc 通过。

手动（`npm run tauri dev`）：
- 启动后 overlay 默认显示 ResidentView（B 布局），窗口右上角、精简尺寸。
- 列表正确分组（待介入/已搁置）+ 项目标题 + 图标+名称+状态。
- 点右上角展开图标 → 切到 PanelView（560×420，搜索栏）。
- 在 PanelView 点收起按钮 → 切回 ResidentView。
- ⌥Space 能 toggle 显隐。
- ResidentView 显示时切到别的 app，**窗口不收起**（常驻语义）。

- [ ] **Step 4: 提交**

```bash
git add src/App.vue src/components/ResidentView.vue
git commit -m "feat(resident): App 按 mode 分发 + ResidentView B 布局骨架"
```

---

## Task 9: ResidentView A 布局 + layout 响应

**Files:**
- Modify: `src/components/ResidentView.vue`

**Interfaces:**
- Consumes: `invoke<{resident_layout}>('get_prefs')`、`invoke('set_resident_layout')`（本视图不直接调，Preferences 调；本视图只读 + 响应）。

- [ ] **Step 1: 读 layout + 响应**

在 `ResidentView.vue` `<script setup>` 的 `const rootEl` 之后加：

```ts
import type { ResidentLayout } from '../types';
const layout = ref<ResidentLayout>('b');

// layout 变化时重新量高（Task 13 接入高度自适应后由其 watcher 统一；此处先占位 ref）。
```

在 `onMounted` 开头（`get_sessions` 之前）加读 layout：

```ts
  try {
    const p = await invoke<{ resident_layout: ResidentLayout }>('get_prefs');
    layout.value = p.resident_layout;
  } catch (e) {
    console.error('get_prefs resident_layout failed', e);
  }
```

（Preferences 改 layout 时调 `set_resident_layout` 存盘；本视图需感知变化——简单做法：Preferences 与 ResidentView 同窗口时不会同时存在（prefs 是独立窗口），改 layout 后用户切回 overlay 窗口时 onMounted 已重读。MVP 不做跨窗口实时联动；若需实时，可 `listen('mode_changed')` 之外加一个 prefs 重读。本 task 先 onMounted 读一次。）

- [ ] **Step 2: 模板按 layout 切 A/B**

把 `<template>` 的分组渲染段替换为 layout 分支（A = 扁平列表，无分组/项目标题/状态文字）：

```html
    <!-- A 极简：扁平列表，仅图标+名称，按状态 rank 排序，闲置/搁置灰显沉底 -->
    <template v-if="layout === 'a'">
      <div
        v-for="s in flatRows"
        :key="s.id"
        class="row"
        :class="{
          perm: s.status === 'needsPermission' && !s.snoozed,
          reply: s.status === 'waitingForReply' && !s.snoozed,
          dim: s.snoozed || isStaleInput(s, now),
        }"
        role="button" tabindex="0"
        :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
        data-tauri-drag-region="false"
        @click="focusSession(s.id)"
        @keydown.enter.prevent="focusSession(s.id)"
      >
        <StatusIcon :status="s.status" class="icon" />
        <span class="name">{{ s.name || s.project }}</span>
      </div>
      <div v-if="!flatRows.length" class="empty">暂无会话</div>
    </template>
    <!-- B 精简：分组 + 项目标题 + 状态（Task 8 已有，保留） -->
    <template v-else>
      <template v-for="(section, si) in groups" :key="section.key">
        <div v-if="si > 0" class="group-sep" />
        <div class="group-head">{{ section.label }} <span class="cnt">{{ section.total }}</span></div>
        <template v-for="[proj, rows] in section.projs" :key="section.key + '|' + proj">
          <div class="proj-head">{{ projShort(proj) }}</div>
          <div
            v-for="s in rows" :key="s.id" class="row"
            :class="{ perm: s.status === 'needsPermission' && !s.snoozed, reply: s.status === 'waitingForReply' && !s.snoozed, dim: s.snoozed || isStaleInput(s, now) }"
            role="button" tabindex="0"
            :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
            data-tauri-drag-region="false"
            @click="focusSession(s.id)" @keydown.enter.prevent="focusSession(s.id)"
          >
            <StatusIcon :status="s.status" class="icon" />
            <span class="name">{{ s.name || s.project }}</span>
            <span class="st" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
          </div>
        </template>
      </template>
      <div v-if="!groups.length" class="empty">暂无会话</div>
    </template>
```

在 `<script setup>` 加 `flatRows` computed（用 `statusRank`，需补 import）：

```ts
import { STATUS_ZH, projShort, isStaleInput, statusRank } from '../utils/session';

// A 布局扁平列表：活会话按 rank 排（等权限优先），闲置/搁置由 .dim 表达。
const flatRows = computed(() => {
  const n = now.value;
  return [...all.value]
    .filter(s => s.alive)
    .sort((a, b) => {
      const sa = isStaleInput(a, n), sb = isStaleInput(b, n);
      if (sa !== sb) return sa ? 1 : -1;
      return statusRank(a) - statusRank(b);
    });
});
```

> A 布局行模板不渲染 `.st`，CSS 里 `.name` 在 A 下应占满（已 `flex:1`，无需改）。

- [ ] **Step 3: 类型检查 + 手动验证**

Run: `npm run build` → vue-tsc 通过。

手动：编辑 `~/.claude/cc-view/prefs.json` 把 `"resident_layout"` 改 `"a"`，重启 dev → ResidentView 显示 A 扁平列表（无分组/状态文字，更窄）；改回 `"b"` 重启 → B 布局。（正式 UI 在 Task 14 偏好设置接入。）

- [ ] **Step 4: 提交**

```bash
git add src/components/ResidentView.vue
git commit -m "feat(resident): A 极简布局 + layout 响应"
```

---

## Task 10: ResidentView 过滤（show_snoozed / show_idle）

**Files:**
- Modify: `src/components/ResidentView.vue`

**Interfaces:**
- Consumes: `get_prefs` 的 `resident_show_snoozed` / `resident_show_idle`。

- [ ] **Step 1: 读两个开关 ref**

在 `const layout = ref<ResidentLayout>('b');` 之后加：

```ts
const showSnoozed = ref(true);
const showIdle = ref(true);
```

在 `onMounted` 读 layout 的 try 块里一起读（改 `invoke` 返回类型 + 赋值）：

```ts
  try {
    const p = await invoke<{
      resident_layout: ResidentLayout;
      resident_show_snoozed: boolean;
      resident_show_idle: boolean;
    }>('get_prefs');
    layout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
  } catch (e) {
    console.error('get_prefs resident config failed', e);
  }
```

- [ ] **Step 2: groups / flatRows 应用过滤**

把 `groups` computed 里 `const list = all.value.filter(s => s.alive);` 之后、分组前加过滤：

```ts
  const list = all.value
    .filter(s => s.alive)
    .filter(s => showSnoozed.value || !s.snoozed) // 关闭搁置 → 排除 snoozed
    .filter(s => showIdle.value || !isStaleInput(s, now.value)); // 关闭闲置 → 排除闲置
```

把 `flatRows` computed 的 `.filter(s => s.alive)` 之后加同样两条：

```ts
    .filter(s => s.alive)
    .filter(s => showSnoozed.value || !s.snoozed)
    .filter(s => showIdle.value || !isStaleInput(s, n))
```

- [ ] **Step 3: 类型检查 + 手动验证**

Run: `npm run build` → 通过。

手动：编辑 `prefs.json` 设 `"resident_show_idle": false`，重启 → 闲置会话消失（待介入组若全闲置则整组下沉/消失）；`"resident_show_snoozed": false` → 已搁置组整组消失。

- [ ] **Step 4: 提交**

```bash
git add src/components/ResidentView.vue
git commit -m "feat(resident): 显示搁置/闲置开关过滤"
```

---

## Task 11: ResidentView 点行 focus（已在骨架）+ 验证

> 骨架（Task 8）已实现 `focusSession` + `@click` + `@keydown.enter`，且常驻模式 focus 后**不 hide**（`focusSession` 无 `getCurrentWebviewWindow().hide()`，区别于 PanelView）。本 task 仅做验证 + 文档化，确认行为正确。

**Files:**
- 无代码改动（验证型 task）。

- [ ] **Step 1: 确认 focusSession 无 hide**

检查 `ResidentView.vue` 的 `focusSession`：

```ts
async function focusSession(id: string) {
  try {
    await invoke('focus_session', { id });
  } catch (e) {
    console.error('focus_session failed', e);
  }
}
```

确认**没有** `await getCurrentWebviewWindow().hide();`（PanelView 有，常驻不应有——常驻语义保持显示）。

- [ ] **Step 2: 手动验证**

`npm run tauri dev`：ResidentView 点某行 → 对应终端 app 被 activate（focus 跳转），**常驻窗口保持显示**（不收起）。

- [ ] **Step 3: 提交（无代码改动则空提交跳过，或并入下一 task）**

若无改动，跳过提交；若补了注释则：

```bash
git add src/components/ResidentView.vue
git commit -m "docs(resident): 确认点行 focus 后窗口保持显示（常驻不收起）"
```

---

## Task 12: ResidentView 透明度 CSS 变量

**Files:**
- Modify: `src/components/ResidentView.vue`、`src/App.vue`（`:root` 加 `--resident-bg` 默认值）

**Interfaces:**
- Consumes: `get_prefs` 的 `resident_opacity`。

- [ ] **Step 1: App.vue :root 加 --resident-bg 默认**

在 `App.vue` `<style>` 的 `:root` 块里（`--color-bg-overlay` 之后）加：

```css
  --resident-bg: rgba(28, 28, 30, 0.55);
```

（默认 55%；运行时由 ResidentView 覆盖。light 模式 `@media (prefers-color-scheme: light)` 块里也加一个 `--resident-bg: rgba(255,255,255,0.55);`。）

- [ ] **Step 2: ResidentView 用 --resident-bg + 读 opacity 设值**

把 `ResidentView.vue` `.resident` 的 `background: var(--color-bg-overlay);` 改为：

```css
  background: var(--resident-bg);
```

在 `<script setup>` 加 opacity ref + 应用函数（`const showIdle` 之后）：

```ts
const opacity = ref(55);

function applyOpacity() {
  // 透明度作用于 --resident-bg 的 alpha（仅常驻；面板模式用 --color-bg-overlay 不受影响）。
  const a = (opacity.value / 100).toFixed(3);
  document.documentElement.style.setProperty('--resident-bg', `rgba(28, 28, 30, ${a})`);
}
```

在 `onMounted` 读 opacity 的 try 块扩展（把 invoke 返回类型再加 `resident_opacity: number`，赋值 + 调 applyOpacity）：

```ts
  try {
    const p = await invoke<{
      resident_layout: ResidentLayout;
      resident_show_snoozed: boolean;
      resident_show_idle: boolean;
      resident_opacity: number;
    }>('get_prefs');
    layout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
    opacity.value = p.resident_opacity;
    applyOpacity();
  } catch (e) {
    console.error('get_prefs resident config failed', e);
  }
```

> Preferences 改透明度后需让 overlay 窗口感知——MVP 用事件：Preferences 的 slider 在 `set_resident_opacity` 成功后 `emit` 不便（prefs 窗口 emit，overlay listen）。简单方案：Task 14 里 Preferences 调 `set_resident_opacity` 后，由后端在 `set_resident_opacity` 内 `app.emit("prefs_changed", ())`，ResidentView `listen("prefs_changed")` 重读 get_prefs + applyOpacity。本 task 先做 onMount 应用（打开即正确）；跨窗口实时联动放 Task 14 一起接入 `prefs_changed`。

- [ ] **Step 3: 类型检查 + 手动验证**

Run: `npm run build` → 通过。

手动：编辑 `prefs.json` 设 `"resident_opacity": 80`，重启 → ResidentView 背景更不透明；设 `25` → 更透（vibrancy/桌面透出明显）。

- [ ] **Step 4: 提交**

```bash
git add src/App.vue src/components/ResidentView.vue
git commit -m "feat(resident): 背景透明度可调（--resident-bg CSS 变量）"
```

---

## Task 13: ResidentView 高度自适应（set_resident_height）

**Files:**
- Modify: `src/components/ResidentView.vue`

**Interfaces:**
- Consumes: `invoke('set_resident_height', { height })`（Task 4）。

- [ ] **Step 1: 加 ResizeObserver 量高 + invoke**

在 `<script setup>` 加（`applyOpacity` 之后）：

```ts
// 高度自适应：内容变化（sessions/layout/过滤）后量 rootEl 实际高度，通知后端 set_resident_height
// 校正窗口高度。仅 mode==resident 时后端生效（Task 4 守卫）。节流避免高频 set_size。
let resizeRaf = 0;
function syncHeight() {
  cancelAnimationFrame(resizeRaf);
  resizeRaf = requestAnimationFrame(async () => {
    const el = rootEl.value;
    if (!el) return;
    const h = Math.round(el.scrollHeight);
    if (h > 0) {
      try {
        await invoke('set_resident_height', { height: h });
      } catch (e) {
        console.error('set_resident_height failed', e);
      }
    }
  });
}
let ro: ResizeObserver | undefined;
```

在 `onMounted` 末尾（`nowTimer = ...` 之后）加：

```ts
  // 监听内容尺寸变化 → 校正窗口高度。
  if (rootEl.value) {
    ro = new ResizeObserver(() => syncHeight());
    ro.observe(rootEl.value);
  }
  syncHeight(); // 首次量一次
```

在 `onBeforeUnmount` 加清理：

```ts
onBeforeUnmount(() => {
  if (nowTimer) clearInterval(nowTimer);
  if (ro) ro.disconnect();
  cancelAnimationFrame(resizeRaf);
});
```

> `scrollHeight` 含 padding，是常驻列表的实际内容高度（logical px）。窗口 `max-height: 60vh` 在 CSS 控制；超出则 `.resident` 自身滚动，`scrollHeight` 反映完整内容但窗口被 set_size 到 60vh 对应高度——需取 `Math.min(scrollHeight, maxViewport)`。简化：`set_resident_height` 传 `scrollHeight`，后端按值 set_size；CSS `max-height:60vh` 会让窗口内容区不超过 60vh，但窗口本身 set_size 高度可能 > 60vh（内容溢出隐藏）。为避免窗口过高，改取 `Math.min(el.scrollHeight, el.clientHeight)`？`clientHeight` 受 max-height 限制 = 实际可见高。用 `Math.round(el.getBoundingClientRect().height)`（受 max-height 裁剪后的实际高）更准。把 `syncHeight` 里 `const h = Math.round(el.scrollHeight);` 改为：

```ts
    const h = Math.round(el.getBoundingClientRect().height);
```

（`getBoundingClientRect().height` 是布局后实际渲染高度，受 `max-height:60vh` 限制，反映"窗口该多高"。）

- [ ] **Step 2: 类型检查 + 手动验证**

Run: `npm run build` → 通过。

手动：
- ResidentView 高度随会话数变化（增删会话、开关搁置/闲置）即时收缩/增长，无大片空白、不超高（超 60vh 内部滚动）。
- 切 A/B 布局高度随之变。
- 切到 PanelView 再切回，高度重新校正。

- [ ] **Step 3: 提交**

```bash
git add src/components/ResidentView.vue
git commit -m "feat(resident): 内容高度自适应（ResizeObserver → set_resident_height）"
```

---

## Task 14: Preferences 常驻面板 section + 跨窗口实时联动

**Files:**
- Modify: `src-tauri/src/lib.rs`（`set_resident_*` 内 emit `prefs_changed`）、`src/components/Preferences.vue`、`src/components/ResidentView.vue`（listen `prefs_changed` 重读）

**Interfaces:**
- Consumes: `Prefs`（Task 6 类型）、各 `set_resident_*` / `set_mode` commands。
- Produces: Preferences 「常驻面板」section；后端 `prefs_changed` 事件；ResidentView 实时响应配置变化。

- [ ] **Step 1: 后端 set_resident_* / set_mode 内 emit prefs_changed**

在 `lib.rs` 给 `set_resident_layout` / `set_resident_show_snoozed` / `set_resident_show_idle` / `set_resident_opacity` / `set_mode` 的签名加 `app: tauri::AppHandle` 参数，并在存盘后 `let _ = app.emit("prefs_changed", ());`。

例：`set_resident_layout` 改为：

```rust
#[tauri::command]
fn set_resident_layout(
    layout: prefs::ResidentLayout,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) {
    if let Ok(mut p) = state.lock() {
        p.resident_layout = layout;
        p.save();
    }
    let _ = app.emit("prefs_changed", ());
}
```

对其余三个 `set_resident_show_*` / `set_resident_opacity` 做同样改动（加 `app` 参 + emit）。`set_mode` 已有 `app` 参，在其 `let _ = app.emit("mode_changed", mode);` 之后加 `let _ = app.emit("prefs_changed", ());`（让 ResidentView 也重读 mode 之外的配置，虽然 mode 走 mode_changed，多 emit 一次 prefs_changed 无害）。

- [ ] **Step 2: Preferences.vue 加常驻面板 section**

在 `Preferences.vue` `<script setup>`：
- import `Prefs` 类型、扩展 `get_prefs` 返回类型、加 ref：

```ts
import type { Prefs, OverlayMode, ResidentLayout } from '../types';

const mode = ref<OverlayMode>('resident');
const residentLayout = ref<ResidentLayout>('b');
const showSnoozed = ref(true);
const showIdle = ref(true);
const opacity = ref(55);
```

- 把 `onMounted` 里 `const p = await invoke<{ notify: boolean; shortcut: string; poll_interval: number }>('get_prefs');` 改为读 `Prefs` 全字段并赋值：

```ts
    const p = await invoke<Prefs>('get_prefs');
    notify.value = p.notify;
    shortcut.value = p.shortcut;
    interval.value = p.poll_interval;
    mode.value = p.mode;
    residentLayout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
    opacity.value = p.resident_opacity;
```

- 加 wrap handler（仿现有 `onNotify` 等）：

```ts
const onMode = (v: OverlayMode) => wrap('mode', async () => { await invoke('set_mode', { mode: v }); mode.value = v; });
const onLayout = (v: ResidentLayout) => wrap('layout', async () => { await invoke('set_resident_layout', { layout: v }); residentLayout.value = v; });
const onShowSnoozed = (v: boolean) => wrap('showSnoozed', async () => { await invoke('set_resident_show_snoozed', { show: v }); showSnoozed.value = v; });
const onShowIdle = (v: boolean) => wrap('showIdle', async () => { await invoke('set_resident_show_idle', { show: v }); showIdle.value = v; });
const onOpacity = (v: number) => wrap('opacity', async () => { await invoke('set_resident_opacity', { opacity: v }); opacity.value = v; });
```

- 在 `<template>` 现有 `<section>`（自启动/通知/快捷键/轮询）之后、`<section class="update-section">` 之前加：

```html
    <section>
      <h2 class="section-title">常驻面板</h2>
      <label class="row">
        <span>默认形态</span>
        <select :value="mode" :disabled="saving === 'mode'"
                @change="onMode(($event.target as HTMLSelectElement).value as OverlayMode)">
          <option value="resident">常驻（精简）</option>
          <option value="panel">面板（全功能）</option>
        </select>
      </label>
      <label class="row">
        <span>常驻布局</span>
        <select :value="residentLayout" :disabled="saving === 'layout'"
                @change="onLayout(($event.target as HTMLSelectElement).value as ResidentLayout)">
          <option value="b">B 精简（分组+状态）</option>
          <option value="a">A 极简（仅图标+名称）</option>
        </select>
      </label>
      <label class="row">
        <span>显示搁置的会话</span>
        <input type="checkbox" :checked="showSnoozed" :disabled="saving === 'showSnoozed'"
               @change="onShowSnoozed(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>显示闲置的会话</span>
        <input type="checkbox" :checked="showIdle" :disabled="saving === 'showIdle'"
               @change="onShowIdle(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>背景透明度（20–100）</span>
        <input type="range" min="20" max="100" :value="opacity" :disabled="saving === 'opacity'"
               @input="onOpacity(Number(($event.target as HTMLInputElement).value))" />
        <span class="muted">{{ opacity }}%</span>
      </label>
    </section>
```

- 加 section-title 样式（`<style scoped>`）：

```css
.section-title { font-size: 13px; font-weight: 700; margin: 20px 0 4px; color: var(--color-muted); letter-spacing: 0.03em; }
.row input[type="range"] { width: 120px; }
```

- [ ] **Step 3: ResidentView listen prefs_changed 重读**

在 `ResidentView.vue` `onMounted` 的 listen 块之后加：

```ts
  try {
    await listen('prefs_changed', async () => {
      // 配置变化（通常来自偏好设置窗口）→ 重读并应用。
      try {
        const p = await invoke<{
          resident_layout: ResidentLayout;
          resident_show_snoozed: boolean;
          resident_show_idle: boolean;
          resident_opacity: number;
        }>('get_prefs');
        layout.value = p.resident_layout;
        showSnoozed.value = p.resident_show_snoozed;
        showIdle.value = p.resident_show_idle;
        opacity.value = p.resident_opacity;
        applyOpacity();
      } catch (e) {
        console.error('prefs_changed reload failed', e);
      }
    });
  } catch (e) {
    console.error('listen prefs_changed failed', e);
  }
```

- [ ] **Step 4: 类型检查 + 手动验证**

Run: `npm run build` → 通过。`cd src-tauri && cargo build` → 通过。

手动（`npm run tauri dev`，同时打开偏好设置 ⌘, 或托盘菜单「偏好设置…」）：
- 改「默认形态」→ overlay 窗口即时切 panel/resident。
- 改「常驻布局」A/B → ResidentView 即时切布局 + 宽度变。
- 开关「显示搁置/闲置」→ 列表即时过滤。
- 拖「透明度」slider → ResidentView 背景即时变透明度。
- 偏好设置里每个控件 saving 反馈正常，非法值（透明度越界不可能，range 限死）不报错。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs src/components/Preferences.vue src/components/ResidentView.vue
git commit -m "feat(prefs): 偏好设置常驻面板 section + prefs_changed 跨窗口实时联动"
```

---

## Task 15: 端到端手动验证 + 收尾

**Files:**
- 无代码改动（验证 + 文档）。

- [ ] **Step 1: 完整功能验证清单**

`npm run tauri dev`，逐项验证：

1. **默认形态**：全新启动（删 `~/.claude/cc-view/prefs.json`）→ overlay 默认 ResidentView（B 布局）、右上角、精简尺寸、失焦不收起。
2. **A/B 布局**：偏好设置切 A → 扁平列表更窄；切 B → 分组+项目标题+状态。
3. **显示搁置**：关 → 已搁置会话/整组消失；开 → 恢复。
4. **显示闲置**：关 → 等输入超时的会话消失；开 → 恢复（灰显）。
5. **透明度**：拖 slider 20–100 → 背景透明度实时变；20 很透、100 不透。
6. **点行 focus**：点会话行 → 终端 activate，常驻窗口保持显示。
7. **模式切换**：ResidentView 点展开 → PanelView（560×420）；PanelView 点收起 → ResidentView。
8. **⌥Space**：两模式下都能 toggle 显隐。
9. **失焦行为**：ResidentView 显示时切别的 app → 不收起；PanelView（未钉）失焦 → 收起；PanelView 钉住 → 不收起。
10. **跨 Space**：进全屏 app，ResidentView 仍可见（join_all_spaces）。
11. **向后兼容**：把 `prefs.json` 删到只剩 `{"notify":true}`，重启 → 常驻模式正常，新字段填默认。
12. **位置记忆**：拖动 ResidentView 到别处 → 重启/重显恢复位置（overlay-position.json）。
13. **menubar** 通知、托盘 badge 等现有功能不受影响。

- [ ] **Step 2: Rust 测试全绿**

Run: `cd src-tauri && cargo test`
Expected: 全部 PASS（prefs 新测试 + 现有 overlay_position 等）。

- [ ] **Step 3: 前端类型检查**

Run: `npm run build`
Expected: vue-tsc 通过，`dist/` 产出。

- [ ] **Step 4: 更新 README（可选）**

在 `README.md`「功能」节加一条「常驻模式」描述（一句话 + 提到偏好设置可切）。若用户希望保持简洁可跳过。

- [ ] **Step 5: 最终提交**

```bash
git add README.md  # 若改了
git commit -m "docs: README 补充常驻模式说明"
```

---

## Self-Review 已完成

- **Spec 覆盖**：spec 的数据模型（Task 1）、commands（Task 2/3/4）、失焦行为（Task 5）、前端分发+PanelView+ResidentView+Preferences（Task 6-14）、边缘情况（向后兼容 Task 1 测试 + Task 15 验证、空会话 Task 8 empty、max-height Task 8/13、全屏 Task 15-10）均有对应 task。
- **Placeholder**：无 TBD/TODO；每步含完整代码或确切命令。
- **类型一致**：`OverlayMode`/`ResidentLayout` 跨 Rust（lowercase 枚举）↔ TS（字面量联合）↔ command 参数命名（`mode`/`layout`/`show`/`opacity`/`height`）一致；`set_resident_height` 高度用 `f64`（logical），前端 `getBoundingClientRect().height` 同为 logical px。
