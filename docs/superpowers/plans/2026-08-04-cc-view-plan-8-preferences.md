# cc-view Plan 8: 偏好设置 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** tray「偏好设置」占位 → 可用窗口：`accessory ⇄ regular` dock 切换 + 4 个设置项（开机自启动 / 通知 / 全局快捷键预设 / 轮询间隔）。

**Architecture:** 新 `prefs` 窗口（`App.vue` 按 `getCurrentWebviewWindow().label` 分发到 `Preferences.vue`）；打开转 `regular`（dock 出现）/ 关闭转回 `accessory`；`prefs.rs` 读写 `~/.claude/cc-view/prefs.json`（notify/shortcut/poll_interval）；4 个 commands 驱动 4 设置项；快捷键启动按 prefs 动态注册，`poll_loop` 读 `AtomicU64` 间隔与 `Mutex<Prefs>.notify` 决定是否通知。

**Tech Stack:** `tauri-plugin-autostart`、`objc2 setActivationPolicy`、`tauri-plugin-global-shortcut` runtime register/unregister、Vue 3。

## Global Constraints

- macOS；代码英文 / 注释中文；fail fast；DRY/YAGNI；零侵入。
- `prefs.rs` I/O 沿用 `overlay_position`/`hidden` 模式（`~/.claude/cc-view/`）；load 失败 → 默认值不崩溃。
- autostart 状态不进 `prefs.json`（插件自管）。
- dev 模式裸二进制无 `Info.plist` 本就是 regular（有 dock），**dock 切换必须在 build 后 `.app` 验证**。
- 前端无单测框架，验证用 `npm run build` + 冒烟。

## File Structure

- `src-tauri/Cargo.toml` —— `+ tauri-plugin-autostart`
- `src-tauri/tauri.conf.json` —— `+ window "prefs"`
- `src-tauri/capabilities/default.json` —— `windows` 加 `"prefs"`
- `src-tauri/src/prefs.rs`（新）—— `Prefs` 结构 + load/save
- `src-tauri/src/lib.rs` —— autostart 插件、prefs state、`set_activation_policy`、`open_prefs`、6 commands、快捷键按 prefs 注册、poll_loop 读间隔/通知、菜单事件 `prefs`
- `src/App.vue` —— label 分发
- `src/components/Preferences.vue`（新）—— 4 设置项 UI

---

### Task 1: prefs.rs 模块（TDD）

**Files:**
- Create: `src-tauri/src/prefs.rs`
- Modify: `src-tauri/src/lib.rs`（`mod prefs;` 声明）
- Test: `src-tauri/src/prefs.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `prefs::Prefs { notify: bool, shortcut: String, poll_interval: u64 }`、`Prefs::load() -> Self`、`Prefs::save()`、`Prefs::is_valid_shortcut(&str) -> bool`、`prefs::ALLOWED_SHORTCUTS: &[&str]`、`Prefs: Default`。

> I/O（load/save）是薄包装且模式与已测的 `overlay_position` 一致，不为它单独参数化测文件（YAGNI）；本任务测 serde 行为与校验。

- [ ] **Step 1: 写测试（先红）**

Create `src-tauri/src/prefs.rs`（含测试，impl 此刻未写 → 编译失败）：
```rust
// 用户偏好：notify（通知开关）/ shortcut（全局快捷键预设）/ poll_interval（轮询间隔秒）。
// 读写 ~/.claude/cc-view/prefs.json。自启动不进此文件（tauri-plugin-autostart 自管）。
// load 失败（无 home / 无文件 / 解析失败）→ 默认值，不崩溃。
use serde::{Deserialize, Serialize};

fn default_true() -> bool { true }
fn default_shortcut() -> String { "alt+space".into() }
fn default_interval() -> u64 { 3 }

/// 允许的快捷键预设（"off" = 禁用）。
pub const ALLOWED_SHORTCUTS: &[&str] = &["alt+space", "cmd+alt+space", "ctrl+space", "off"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prefs {
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    #[serde(default = "default_interval")]
    pub poll_interval: u64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { notify: true, shortcut: default_shortcut(), poll_interval: default_interval() }
    }
}

impl Prefs {
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else { return Self::default() };
        let path = home.join(".claude/cc-view/prefs.json");
        let Ok(txt) = std::fs::read_to_string(&path) else {
            eprintln!("prefs load: failed to read ~/.claude/cc-view/prefs.json");
            return Self::default();
        };
        serde_json::from_str(&txt).unwrap_or_else(|e| {
            eprintln!("prefs load: invalid json, using defaults: {}", e);
            Self::default()
        })
    }
    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(dir.join("prefs.json"), json);
        }
    }
    pub fn is_valid_shortcut(s: &str) -> bool { ALLOWED_SHORTCUTS.contains(&s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_uses_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(p.notify);
        assert_eq!(p.shortcut, "alt+space");
        assert_eq!(p.poll_interval, 3);
    }

    #[test]
    fn partial_json_keeps_defaults_for_missing() {
        let p: Prefs = serde_json::from_str(r#"{"notify":false}"#).unwrap();
        assert!(!p.notify);
        assert_eq!(p.shortcut, "alt+space");
        assert_eq!(p.poll_interval, 3);
    }

    #[test]
    fn full_json_roundtrip() {
        let p = Prefs { notify: false, shortcut: "ctrl+space".into(), poll_interval: 10 };
        let json = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn invalid_json_falls_back_to_default() {
        let p: Prefs = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(p, Prefs::default());
    }

    #[test]
    fn is_valid_shortcut_checks_allowed() {
        assert!(Prefs::is_valid_shortcut("alt+space"));
        assert!(Prefs::is_valid_shortcut("off"));
        assert!(!Prefs::is_valid_shortcut("ctrl+shift+a"));
    }
}
```

在 `lib.rs:1-14` 的 `mod` 声明块加 `mod prefs;`（按字母序，插在 `mod overlay_position;` 之后、`mod liveness;` 之前）。

- [ ] **Step 2: 跑测试确认绿（impl 已含在 Step 1）**

Run: `cargo test --manifest-path src-tauri/Cargo.toml prefs`
Expected: 5 个测试全 PASS。

- [ ] **Step 3: 确认整体编译**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/prefs.rs src-tauri/src/lib.rs
git commit -m "feat(prefs): 加 Prefs 模块（notify/shortcut/poll_interval，serde + load/save）"
```

---

### Task 2: 配置（autostart 依赖 + prefs 窗口 + 权限）

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Cargo.toml 加 autostart**

`[dependencies]` 段加（紧跟 `tauri-plugin-global-shortcut = "2"` 之后）：
```toml
tauri-plugin-autostart = "2"
```

- [ ] **Step 2: tauri.conf.json 加 prefs 窗口**

`app.windows` 数组（当前只有 overlay）末尾追加：
```json
,
{
  "label": "prefs",
  "url": "index.html",
  "visible": false,
  "decorations": true,
  "resizable": false,
  "width": 480,
  "height": 460,
  "center": true,
  "skipTaskbar": false,
  "title": "cc-view 偏好设置"
}
```

- [ ] **Step 3: capabilities 加 prefs 窗口**

`capabilities/default.json` 的 `"windows": ["overlay"]` 改为：
```json
"windows": ["overlay", "prefs"],
```

- [ ] **Step 4: 编译确认**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过（autostart 依赖拉取 + prefs 窗口声明）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "chore(prefs): autostart 依赖 + prefs 窗口声明 + capabilities"
```

---

### Task 3: prefs 窗口生命周期（activation policy + open/close + 菜单）

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `set_activation_policy(policy: i64)`（macOS）、`open_prefs(&AppHandle)`；菜单事件 `"prefs"` → `open_prefs`。

- [ ] **Step 1: 加 set_activation_policy 辅助函数**

在 `join_all_spaces` 函数之后（`make_panel` 之前）插入：
```rust
/// 切换 app activation policy：0=regular（有 dock），1=accessory（无 dock）。
/// cc-view 平时 accessory（LSUIElement）；打开偏好设置需 regular 给用户 app 入口。
#[cfg(target_os = "macos")]
fn set_activation_policy(policy: i64) {
    use objc2::{class, msg_send, runtime::AnyObject};
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: policy as objc2::ffi::NSInteger];
    }
}
```

- [ ] **Step 2: 加 open_prefs 函数**

在 `show_overlay` 函数之后插入：
```rust
/// 打开偏好设置窗口：转 regular（dock 出现）→ show → focus。
/// accessory app 默认无 dock，点偏好设置时需切 regular 提供 app 入口。
fn open_prefs(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    set_activation_policy(0); // NSApplicationActivationPolicyRegular
    if let Some(w) = app.get_webview_window("prefs") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

- [ ] **Step 3: prefs 窗口关闭转回 accessory**

在 `setup` 里 overlay 的 `on_window_event` 代码块之后（`if let Some(overlay) = ...` 闭合 `}` 之后），加 prefs 窗口事件：
```rust
            // prefs 窗口：关闭即转回 accessory（dock 消失）+ hide（不销毁，下次复用）。
            if let Some(prefs_win) = app.get_webview_window("prefs") {
                let prefs_handle = app.handle().clone();
                prefs_win.on_window_event(move |e| {
                    if matches!(e, tauri::WindowEvent::CloseRequested { .. }) {
                        #[cfg(target_os = "macos")]
                        set_activation_policy(1); // accessory
                        if let Some(w) = prefs_handle.get_webview_window("prefs") {
                            let _ = w.hide();
                        }
                    }
                });
            }
```

- [ ] **Step 4: prefs 菜单项启用 + 菜单事件**

`prefs_item` 构造的 `enabled` 参数由 `false` 改 `true`：
- old: `                "prefs",\n                "偏好设置…",\n                false,`
- new: `                "prefs",\n                "偏好设置…",\n                true,`

菜单事件 match 加 `"prefs"` 分支：
- old:
```rust
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "show" => show_overlay(app),
                "quit" => app.exit(0),
                _ => {}
            });
```
- new:
```rust
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "show" => show_overlay(app),
                "prefs" => open_prefs(app),
                "quit" => app.exit(0),
                _ => {}
            });
```

- [ ] **Step 5: 编译确认**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(prefs): open/close 窗口 + accessory⇄regular dock 切换 + 菜单事件"
```

---

### Task 4: 偏好 state + commands

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces commands：`get_prefs` / `set_notify` / `toggle_autostart(enable)` / `get_autostart` / `set_shortcut(shortcut)` / `set_interval(seconds)`。

- [ ] **Step 1: 加 state 注入 + autostart 插件**

`run()` 开头先 load prefs（避免 builder 链里重复 load）：
- old: `pub fn run() {\n    tauri::Builder::default()`
- new:
```rust
pub fn run() {
    let loaded_prefs = prefs::Prefs::load();
    let poll_secs = loaded_prefs.poll_interval;
    tauri::Builder::default()
```

builder 链：在 `.plugin(tauri_plugin_notification::init())` 之后加 autostart：
```rust
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
```

`.manage(...)` 链：在现有 `.manage(Mutex::new(overlay_position::...))` 之后加：
```rust
        .manage(std::sync::atomic::AtomicU64::new(poll_secs))
        .manage(Mutex::new(loaded_prefs))
```
（`AtomicU64` 用全路径，避免顶部 use 改动）

- [ ] **Step 2: 加 6 个 commands**

在 `set_overlay_pinned` command 之后（`join_all_spaces` 之前）插入：
```rust
// --- 偏好设置 commands ---
// notify/shortcut/interval 存 Mutex<Prefs>，改后立即 save。autostart 走插件自管。
// 校验遵循 fail fast：非法 shortcut/interval 返回 Err 给前端。

#[tauri::command]
fn get_prefs(state: tauri::State<'_, Mutex<prefs::Prefs>>) -> prefs::Prefs {
    state.lock().map(|p| p.clone()).unwrap_or_default()
}

#[tauri::command]
fn set_notify(notify: bool, state: tauri::State<'_, Mutex<prefs::Prefs>>) {
    if let Ok(mut p) = state.lock() {
        p.notify = notify;
        p.save();
    }
}

/// 开/关开机自启动。enable=true→enable()，false→disable()。插件错误转 String 返回前端。
#[tauri::command]
fn toggle_autostart(app: tauri::AppHandle, enable: bool) -> Result<(), String> {
    // 核对：v2 trait 名通常是 ManagerExt（提供 app.autolaunch()）。
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enable { mgr.enable().map_err(|e| e.to_string())? }
    else { mgr.disable().map_err(|e| e.to_string())? }
    Ok(())
}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// 切换全局快捷键：unregister_all → 按新值 register（off 则不注册）→ 存 prefs。
/// 失败（解析/注册）返回 Err，不落库。
#[tauri::command]
fn set_shortcut(
    shortcut: String,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !prefs::Prefs::is_valid_shortcut(&shortcut) {
        return Err(format!("invalid shortcut: {}", shortcut));
    }
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    if shortcut != "off" {
        // 核对 v2.x：register 接受 Shortcut；字符串经 parse() 转 Shortcut。
        let s = shortcut.parse().map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
        app.global_shortcut().register(s).map_err(|e| e.to_string())?;
    }
    if let Ok(mut p) = state.lock() {
        p.shortcut = shortcut;
        p.save();
    }
    Ok(())
}

/// 设置轮询间隔（1-30 秒）：更新 AtomicU64（poll_loop 读）+ 存 prefs。
#[tauri::command]
fn set_interval(
    seconds: u64,
    state: tauri::State<'_, Mutex<prefs::Prefs>>,
    interval: tauri::State<'_, std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    if !(1..=30).contains(&seconds) {
        return Err(format!("interval must be 1-30, got {}", seconds));
    }
    interval.store(seconds, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut p) = state.lock() {
        p.poll_interval = seconds;
        p.save();
    }
    Ok(())
}
```

- [ ] **Step 3: 注册 commands**

`invoke_handler` 的 `generate_handler!` 数组加 6 个（在 `list_snoozed` 之后）：
- old: `            list_snoozed\n        ])`
- new:
```rust
            list_snoozed,
            get_prefs,
            set_notify,
            toggle_autostart,
            get_autostart,
            set_shortcut,
            set_interval
        ])
```

- [ ] **Step 4: 编译确认**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过。（若 `ManagerExt` trait 名报错，核对 tauri-plugin-autostart v2 实际导出 trait 名并修正 import。）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(prefs): 6 commands（get_prefs/set_notify/toggle_autostart/get_autostart/set_shortcut/set_interval）"
```

---

### Task 5: 快捷键按 prefs 注册 + poll_loop 读间隔/通知

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 启动按 prefs 注册快捷键（替代硬编码 alt+space）**

当前 `setup` 内 `#[cfg(desktop)]` 快捷键注册块（`Builder::new().with_shortcuts(["alt+space"])...`）改为按 prefs 动态读：
- old:
```rust
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{
                    Builder, Code, Modifiers, ShortcutState,
                };
                app.handle().plugin(
                    Builder::new()
                        .with_shortcuts(["alt+space"])?
                        .with_handler(|app, shortcut, event| {
                            if event.state == ShortcutState::Pressed
                                && shortcut.matches(Modifiers::ALT, Code::Space)
                            {
                                if let Some(w) = app.get_webview_window("overlay") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        show_overlay(app);
                                    }
                                }
                            }
                        })
                        .build(),
                )?;
            }
```
- new:
```rust
            // 快捷键按 prefs.shortcut 注册（默认 alt+space，可改/禁用）。
            // handler 不写死组合——对当前注册的任意快捷键都 toggle overlay。
            // 核对 v2.x：with_shortcuts 接受 [&str]，"cmd+alt+space"/"ctrl+space" 能解析。
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Builder, ShortcutState};
                let shortcut_str = app
                    .state::<Mutex<prefs::Prefs>>()
                    .lock()
                    .map(|p| p.shortcut.clone())
                    .unwrap_or_else(|_| "alt+space".into());
                if shortcut_str != "off" {
                    app.handle().plugin(
                        Builder::new()
                            .with_shortcuts([shortcut_str.as_str()])?
                            .with_handler(|app, _shortcut, event| {
                                if event.state == ShortcutState::Pressed {
                                    if let Some(w) = app.get_webview_window("overlay") {
                                        if w.is_visible().unwrap_or(false) {
                                            let _ = w.hide();
                                        } else {
                                            show_overlay(app);
                                        }
                                    }
                                }
                            })
                            .build(),
                    )?;
                }
            }
```

- [ ] **Step 2: poll_loop 读 AtomicU64 间隔**

`start_poll_loop` 末尾的 `std::thread::sleep(Duration::from_secs(3));`（L182）改为读 atomic：
- old: `            std::thread::sleep(Duration::from_secs(3));`
- new:
```rust
            // 间隔由偏好 AtomicU64 控制（默认 3，可 1-30）；无 state 则兜底 3。
            let secs = handle
                .try_state::<std::sync::atomic::AtomicU64>()
                .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                .unwrap_or(3)
                .max(1);
            std::thread::sleep(Duration::from_secs(secs));
```

- [ ] **Step 3: poll_loop 通知按 prefs.notify 开关**

`start_poll_loop` 里发通知的循环（L89-98 `for (name, status) in to_notify { ... }`）包一层 notify_on 守卫：
- old:
```rust
            let to_notify = notifier.observe(&derived);
            for (name, status) in to_notify {
                let status_zh = match status {
                    models::Status::NeedsPermission => "等待权限确认",
                    models::Status::WaitingForReply => "等待你回答",
                    models::Status::WaitingForInput => "等待输入",
                    _ => "需要关注",
                };
                notify::send_notification(&handle, "cc-view", &format!("{}：{}", name, status_zh));
            }
```
- new:
```rust
            let to_notify = notifier.observe(&derived);
            // 通知开关：prefs.notify=false 时静默（emit/tray badge 不受影响，只压通知）。
            let notify_on = handle
                .try_state::<Mutex<prefs::Prefs>>()
                .and_then(|s| s.lock().ok().map(|p| p.notify))
                .unwrap_or(true);
            if notify_on {
                for (name, status) in to_notify {
                    let status_zh = match status {
                        models::Status::NeedsPermission => "等待权限确认",
                        models::Status::WaitingForReply => "等待你回答",
                        models::Status::WaitingForInput => "等待输入",
                        _ => "需要关注",
                    };
                    notify::send_notification(&handle, "cc-view", &format!("{}：{}", name, status_zh));
                }
            }
```

- [ ] **Step 4: 编译确认**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(prefs): 快捷键按 prefs 注册 + poll_loop 读间隔/通知开关"
```

---

### Task 6: 前端 App.vue label 分发 + Preferences.vue

**Files:**
- Modify: `src/App.vue`
- Create: `src/components/Preferences.vue`

- [ ] **Step 1: App.vue label 分发**

整体替换 `src/App.vue` 的 `<script setup>` 与 `<template>`（`<style>` 保留不动）：
- old（script+template）:
```vue
<script setup lang="ts">
// 单窗口：overlay 承载全部 UI（原 HUD 已废弃合并）。App 仅挂载 Overlay。
import Overlay from './components/Overlay.vue';
</script>

<template>
  <Overlay />
</template>
```
- new:
```vue
<script setup lang="ts">
// 多视图单入口：overlay 承载命令面板，prefs 承载偏好设置。按 window label 分发。
import { computed } from 'vue';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import Overlay from './components/Overlay.vue';
import Preferences from './components/Preferences.vue';
const isPrefs = computed(() => getCurrentWebviewWindow().label === 'prefs');
</script>

<template>
  <Preferences v-if="isPrefs" />
  <Overlay v-else />
</template>
```

- [ ] **Step 2: 创建 Preferences.vue**

Create `src/components/Preferences.vue`：
```vue
<script setup lang="ts">
// 偏好设置：开机自启动 / 通知 / 全局快捷键 / 轮询间隔。调用后端 commands 持久化（悲观更新）。
import { ref, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';

const notify = ref(true);
const shortcut = ref('alt+space');
const interval = ref(3);
const autostart = ref(false);
const saving = ref<string | null>(null); // 正在保存的项 key（反馈）
const error = ref<string | null>(null);

const shortcuts = [
  { value: 'alt+space', label: '⌥Space（默认）' },
  { value: 'cmd+alt+space', label: '⌘⌥Space' },
  { value: 'ctrl+space', label: '⌃Space' },
  { value: 'off', label: '禁用' },
];

onMounted(async () => {
  try {
    const p = await invoke<{ notify: boolean; shortcut: string; poll_interval: number }>('get_prefs');
    notify.value = p.notify;
    shortcut.value = p.shortcut;
    interval.value = p.poll_interval;
  } catch (e) {
    console.error('get_prefs failed', e);
  }
  try {
    autostart.value = await invoke<boolean>('get_autostart');
  } catch (e) {
    console.error('get_autostart failed', e);
  }
});

// 悲观更新：invoke 成功后再改本地 ref，失败保留旧值 + 显示 error。
async function wrap(key: string, fn: () => Promise<unknown>) {
  error.value = null;
  saving.value = key;
  try {
    await fn();
  } catch (e: unknown) {
    error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '保存失败';
  } finally {
    saving.value = null;
  }
}

const onNotify = (v: boolean) => wrap('notify', async () => { await invoke('set_notify', { notify: v }); notify.value = v; });
const onAutostart = (v: boolean) => wrap('autostart', async () => { await invoke('toggle_autostart', { enable: v }); autostart.value = v; });
const onShortcut = (v: string) => wrap('shortcut', async () => { await invoke('set_shortcut', { shortcut: v }); shortcut.value = v; });
const onInterval = (v: number) => wrap('interval', async () => { await invoke('set_interval', { seconds: v }); interval.value = v; });
</script>

<template>
  <div class="prefs">
    <h1>cc-view 偏好设置</h1>
    <section>
      <label class="row">
        <span>开机自启动</span>
        <input type="checkbox" :checked="autostart"
               :disabled="saving === 'autostart'"
               @change="onAutostart(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>通知</span>
        <input type="checkbox" :checked="notify"
               :disabled="saving === 'notify'"
               @change="onNotify(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>全局快捷键</span>
        <select :value="shortcut" :disabled="saving === 'shortcut'"
                @change="onShortcut(($event.target as HTMLSelectElement).value)">
          <option v-for="s in shortcuts" :key="s.value" :value="s.value">{{ s.label }}</option>
        </select>
      </label>
      <label class="row">
        <span>轮询间隔（秒，1–30）</span>
        <input type="number" min="1" max="30" :value="interval"
               :disabled="saving === 'interval'"
               @change="onInterval(Number(($event.target as HTMLInputElement).value))" />
      </label>
    </section>
    <p v-if="error" class="error">⚠ {{ error }}</p>
  </div>
</template>

<style scoped>
.prefs { padding: 24px 28px; color: var(--color-fg); font-family: var(--font-body); }
h1 { font-size: 18px; font-weight: 700; margin: 0 0 20px; }
.row {
  display: flex; justify-content: space-between; align-items: center;
  padding: 12px 0; border-bottom: 1px solid var(--color-border);
  font-size: var(--fs-body);
}
.row input[type="checkbox"] { width: 18px; height: 18px; }
.row select, .row input[type="number"] {
  font-size: var(--fs-control); padding: 4px 8px;
  background: var(--color-bg); color: var(--color-fg);
  border: 1px solid var(--color-border); border-radius: 6px;
}
.error { color: var(--status-permission); margin-top: 16px; }
</style>
```

- [ ] **Step 3: 前端类型检查**

Run: `npm run build`
Expected: 通过（vue-tsc 校验 invoke 参数 / ref 类型）。

- [ ] **Step 4: Commit**

```bash
git add src/App.vue src/components/Preferences.vue
git commit -m "feat(prefs): App.vue label 分发 + Preferences.vue（4 设置项）"
```

---

### Task 7: 综合冒烟

- [ ] **Step 1: dev 冒烟（功能）**

Run: `npm run tauri dev`
验证：
1. tray 菜单「偏好设置…」可点（非灰）→ prefs 窗口打开，显示 4 设置项
2. 通知开关切换 → `~/.claude/cc-view/prefs.json` 的 `notify` 字段更新
3. 快捷键切到 `⌃Space` → ⌃Space 能呼出 overlay，⌥Space 不再生效
4. 快捷键切「禁用」→ 所有快捷键不响应
5. 切回 `⌥Space` → 恢复
6. 轮询间隔改 1 → sessions 刷新明显变快（~1s）
7. 开机自启动勾选 → 系统登录项出现 cc-view（系统设置 → 通用 → 登录项）
8. 关闭 prefs 窗口 → 窗口消失

Expected: 全部正常，无 console / panic。

- [ ] **Step 2: build 后 .app 验证 dock 切换**

Run: `npm run tauri build`，安装/打开产物 `.app`。
验证：点 tray「偏好设置」→ **dock 出现 cc-view 图标**；关闭 prefs 窗口 → **dock 图标消失**（转回 accessory）。
Expected: dock 显隐正确（dev 裸二进制本就 regular，测不了这条，必须 build 后验）。

- [ ] **Step 3: Commit（如有 README 更新）**

```bash
git add README.md 2>/dev/null
git commit -m "docs: 偏好设置功能说明" 2>/dev/null || echo "no readme change"
```

---

## Self-Review 结论

- **Spec coverage**：§1.1 窗口架构 → Task 2+3+6；§1.2 dock 切换 → Task 3；§1.3 prefs.rs → Task 1；§1.4 四设置项 → Task 4+5（后端）+ Task 6（前端）；§1.5 文件清单全覆盖。检查更新区（§2.3）属 Plan 9，本 plan 不含。✅
- **Placeholder scan**：无 TBD；autostart `ManagerExt` trait 名、global-shortcut `register` 签名、`with_shortcuts` 字符串解析三处标注「核对 v2.x」（沿用 plan-6 惯例，非占位）。✅
- **Type consistency**：`Prefs { notify: bool, shortcut: String, poll_interval: u64 }` 在 Task 1/4/5/6 一致；commands 名 `get_prefs/set_notify/toggle_autostart/get_autostart/set_shortcut/set_interval` 在 Task 4 注册与 Task 6 前端 invoke 一致；`AtomicU64` 间隔在 Task 4 store 与 Task 5 load 一致。✅
- **风险点**：Task 5 Step 2 的过渡版 sleep 是说明性占位，**执行时必须用紧跟其后的修正版**（已在 plan 内明确标注，非遗漏）。✅
