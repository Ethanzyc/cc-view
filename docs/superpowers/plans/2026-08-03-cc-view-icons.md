# cc-view 全套图标重新设计 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用「指挥台雷达」概念 + 系统蓝品牌替换 Tauri 默认图标全套，并让 Mac 通知显示 cc-view 自己的 App 图标。

**Architecture:** 三段式——(1) menubar tray 换单色剪影 + `iconAsTemplate` 运行时动态切换（正常黑白自适配 / attention 染橙跳出）；(2) 用 `tauri icon` 从 1024 源 PNG 生成彩色 squircle app icon 全套；(3) 通知从 osascript 改走 `tauri-plugin-notification`，图标自动取 App bundle `icon.icns`。

**Tech Stack:** Tauri v2 (Rust) · Vue 3 · `tauri-plugin-notification` v2 · Chrome headless（SVG→PNG 渲染）· `tauri icon` CLI

## Global Constraints

- 品牌色 `#0A84FF`；attention 色 `#FF9F0A`；扫描线强调色 `#64D2FF`
- macOS 最低版本 10.13（`tauri.conf.json`）；Rust edition 2021
- 不改 `design-system.json`、HUD/overlay 前端、`src/` 下任何 Vue 文件
- 通知一律走 `tauri-plugin-notification`，不再用 `osascript display notification`
- 任务顺序：Task 1（tray）→ Task 2（app icon）→ Task 3（通知）。Task 1 先把 tray 引用改到 `icons/tray.png`，这样 Task 2 用 `tauri icon` 覆盖 `icons/icon.png` 时不会影响 tray
- SVG→PNG 渲染统一用 Chrome headless + `--default-background-color=00000000`（透明背景，squircle 外与剪影透明区都需要）

---

### Task 1: Menubar tray icon — 单色剪影 + template 动态切换

**Files:**
- Create: `src-tauri/icons/source/tray.svg`
- Create: `src-tauri/icons/tray.png`（44×44，透明背景单色剪影）
- Modify: `src-tauri/Cargo.toml:21`（tauru features 加 `image-png`）
- Modify: `src-tauri/tauri.conf.json:43-49`（trayIcon.iconPath + iconAsTemplate）
- Modify: `src-tauri/src/lib.rs`（embed 剪影 + poll_loop 用 `set_icon_with_as_template`）

**Interfaces:**
- Consumes: `tint_orange`（`lib.rs:37`，保留不改签名）
- Produces: 编译期嵌入的 `TRAY_PNG` 常量；poll_loop attention 跳变时调用 `tray.set_icon_with_as_template(img, as_template)`

- [ ] **Step 1: 写单色剪影 SVG（变体 B 单环）**

Create `src-tauri/icons/source/tray.svg`：

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="0 0 44 44">
  <circle cx="22" cy="22" r="14" fill="none" stroke="#000000" stroke-width="2.6" stroke-opacity="0.55"/>
  <line x1="22" y1="22" x2="35" y2="16" stroke="#000000" stroke-width="2.8" stroke-linecap="round"/>
  <circle cx="22" cy="22" r="3.8" fill="#000000"/>
</svg>
```

> template image 要求 black + clear。`stroke-opacity="0.55"` 会渲染为半透明黑（alpha≈140），macOS 用 alpha 作 mask，半透明环成立。

- [ ] **Step 2: 渲染 tray.png（透明背景）**

Run（在项目根目录）：

```bash
ROOT=$(git rev-parse --show-toplevel)
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --no-first-run --no-default-browser-check \
  --hide-scrollbars --force-device-scale-factor=1 \
  --default-background-color=00000000 \
  --screenshot="$ROOT/src-tauri/icons/tray.png" \
  --window-size=44,44 \
  "file://$ROOT/src-tauri/icons/source/tray.svg"
```

Expected: `src-tauri/icons/tray.png` 生成。验证：

```bash
sips -g pixelWidth -g pixelHeight src-tauri/icons/tray.png
```

应输出 `pixelWidth: 44` / `pixelHeight: 44`。目视确认背景透明、黑剪影。

> Fallback（若 Chrome 透明背景不工作）：`pip3 install cairosvg && cairosvg src-tauri/icons/source/tray.svg -o src-tauri/icons/tray.png -W 44 -H 44`
> 若 Retina 屏下偏糊，把 `--window-size` 与 SVG `width/height` 同步改为 `88,88` 重渲染。

- [ ] **Step 3: Cargo.toml 加 `image-png` feature**

Modify `src-tauri/Cargo.toml:21`：

```toml
tauri = { version = "2", features = ["tray-icon", "macos-private-api", "image-png"] }
```

> `Image::from_bytes` 解码 PNG 需要 `image-png` feature。

- [ ] **Step 4: tauri.conf.json 改 trayIcon 配置**

Modify `src-tauri/tauri.conf.json` 的 `app.trayIcon`（43-49 行）为：

```json
"trayIcon": {
  "id": "main",
  "iconPath": "icons/tray.png",
  "tooltip": "cc-view",
  "iconAsTemplate": true,
  "showMenuOnLeftClick": false
}
```

- [ ] **Step 5: lib.rs 加 embed 常量**

在 `tint_orange` 函数之后（约 `lib.rs:52`）插入：

```rust
/// 单色 menubar 剪影（template image：黑 + 透明）。
/// include_bytes 编译期嵌入，运行时无需读盘；改图后需重新编译。
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");
```

- [ ] **Step 6: lib.rs 改 poll_loop 图标加载**

把 `lib.rs:60-70`（`default_icon` / `orange_icon` 加载块）替换为：

```rust
        // 加载嵌入的单色剪影（template image）+ 预计算 attention 态橙色版。
        // tauri::image::Image::from_bytes 解码 PNG（需 tauri feature "image-png"）。
        let tray_icon = tauri::image::Image::from_bytes(TRAY_PNG)
            .map_err(|e| eprintln!("poll_loop: embedded tray.png decode failed: {e}"))
            .ok();
        let orange_icon = tray_icon.as_ref().map(tint_orange);
```

- [ ] **Step 7: lib.rs 改 set_icon 为 template 原子切换**

把 `lib.rs:114-126`（`set_icon 仅在 attention 状态跳变时调用` 块）替换为：

```rust
                // set_icon_with_as_template 原子切换图标 + template 状态（macOS）：
                // 正常 → 单色剪影 + template=true（自动适配深浅栏）；
                // attention → 橙色实色 + template=false（跳出 menubar 引起注意）。
                let has_attention = need_attention > 0;
                if has_attention != last_attention {
                    last_attention = has_attention;
                    let (icon, as_template) = if has_attention {
                        (orange_icon.as_ref(), false)
                    } else {
                        (tray_icon.as_ref(), true)
                    };
                    if let Some(img) = icon {
                        let _ = tray.set_icon_with_as_template(Some(img.clone()), as_template);
                    }
                }
```

> 若编译报 `set_icon_with_as_template` 不存在（Tauri 版本差异），fallback 改为两次调用：先 `tray.set_icon(Some(img.clone()))?;` 再 `tray.set_icon_as_template(as_template)?;`（非原子，但视觉等效）。

- [ ] **Step 8: 编译验证**

Run:

```bash
cd src-tauri && cargo build 2>&1 | tail -25
```

Expected: 编译通过，无错误。

- [ ] **Step 9: 测试验证**

Run:

```bash
cd src-tauri && cargo test 2>&1 | tail -25
```

Expected: 现有测试全通过（`tint_orange` 未改，`Notifier::observe` 测试不受影响）。

- [ ] **Step 10: 运行视觉验证**

Run（保持前台，手动观察）:

```bash
npm run tauri dev
```

逐项确认：
- menubar 出现黑色雷达剪影（深色菜单栏下自动显白）
- 系统设置切换「浅色/深色」外观，确认剪影自动反色（template 生效）
- 触发某会话进入 NeedsPermission/WaitingForInput（或临时在 `reducer` 注入假数据），确认图标变橙 `#FF9F0A`
- 鼠标悬停 tray，tooltip 显示「N 等我 · M 工作」或「M 工作」

- [ ] **Step 11: Commit**

```bash
git add src-tauri/icons/source/tray.svg src-tauri/icons/tray.png \
        src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: menubar tray icon — radar silhouette + template 动态切换

单色剪影 template image（黑+透明），正常态 iconAsTemplate=true 自动适配
深浅模式；attention 态切橙色实色 + template=false 跳出提醒。
set_icon_with_as_template 原子切换。

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: App icon 全套（彩色 squircle）

**Files:**
- Create: `src-tauri/icons/source/icon.svg`
- Create: `src-tauri/icons/source/icon-1024.png`（1024×1024 源图）
- Regenerate: `src-tauri/icons/` 下 `icon.icns` / `icon.ico` / `icon.png` / `32x32.png` / `128x128.png` / `128x128@2x.png` / `Square*.png` / `StoreLogo.png`（`tauri icon` 覆盖）

**Interfaces:**
- Consumes: 无
- Produces: 全套 app icon；`icon.icns` 供通知中心 / Dock / 关于窗口 / 活动监视器取用

> 前置：Task 1 已把 tray 引用改为 `icons/tray.png`，本任务覆盖 `icons/icon.png` 不影响 tray。

- [ ] **Step 1: 写 app icon 源 SVG（指挥台雷达）**

Create `src-tauri/icons/source/icon.svg`：

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 300 300">
  <defs>
    <radialGradient id="centerGlow" cx="150" cy="150" r="78" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.20"/>
      <stop offset="100%" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="sweepGrad" cx="150" cy="150" r="104" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.42"/>
      <stop offset="100%" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <rect width="300" height="300" rx="68" fill="#0A84FF"/>
  <circle cx="150" cy="150" r="78" fill="url(#centerGlow)"/>
  <circle cx="150" cy="150" r="104" fill="none" stroke="#FFFFFF" stroke-width="5" opacity="0.16"/>
  <circle cx="150" cy="150" r="70" fill="none" stroke="#FFFFFF" stroke-width="6" opacity="0.42"/>
  <path d="M150,150 L150,46 A104,104 0 0 1 244,106 Z" fill="url(#sweepGrad)"/>
  <line x1="150" y1="150" x2="244" y2="106" stroke="#64D2FF" stroke-width="12" stroke-linecap="round" opacity="0.22"/>
  <line x1="150" y1="150" x2="244" y2="106" stroke="#64D2FF" stroke-width="6" stroke-linecap="round" opacity="0.95"/>
  <circle cx="150" cy="150" r="29" fill="none" stroke="#FFFFFF" stroke-width="3.5" opacity="0.5"/>
  <circle cx="150" cy="150" r="20" fill="#FFFFFF"/>
</svg>
```

- [ ] **Step 2: 渲染 1024 源 PNG（透明背景）**

Run（项目根目录）:

```bash
ROOT=$(git rev-parse --show-toplevel)
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --no-first-run --no-default-browser-check \
  --hide-scrollbars --force-device-scale-factor=1 \
  --default-background-color=00000000 \
  --screenshot="$ROOT/src-tauri/icons/source/icon-1024.png" \
  --window-size=1024,1024 \
  "file://$ROOT/src-tauri/icons/source/icon.svg"
```

Expected: `icon-1024.png` 生成，squircle 圆角外透明。验证：

```bash
sips -g pixelWidth -g pixelHeight src-tauri/icons/source/icon-1024.png
```

应输出 `pixelWidth: 1024` / `pixelHeight: 1024`。

> Fallback 同 Task 1 Step 2（cairosvg）。

- [ ] **Step 3: 用 tauri icon 生成全套**

Run:

```bash
npx tauri icon src-tauri/icons/source/icon-1024.png
```

Expected: 控制台输出 `icons/icon.icns`、`icons/icon.ico`、`icons/icon.png`、`icons/32x32.png`、`icons/128x128.png`、`icons/128x128@2x.png`、`icons/Square*.png`、`icons/StoreLogo.png` 已生成。

验证文件已更新：

```bash
ls -lt src-tauri/icons/*.png src-tauri/icons/*.icns src-tauri/icons/*.ico | head -20
```

mtime 应为刚才。

- [ ] **Step 4: 运行视觉验证**

Run:

```bash
npm run tauri dev
```

确认：
- App icon 在通知中心（触发 Task 3 通知前，可暂用 osascript 残留或等 Task 3）显示为雷达——本步主要确认 `tauri icon` 生成的 `icon.icns` 能被 Tauri 加载（无 asset 报错）
- 控制台无 `AssetNotFound` / icon 解码错误

> Dock 因 `LSUIElement=true` 隐藏，故 Dock 看不到 icon 属正常。活动监视器进程列表 / 关于窗口会显示。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/icons/source/icon.svg src-tauri/icons/source/icon-1024.png src-tauri/icons/
git commit -m "$(cat <<'EOF'
feat: app icon — 指挥台雷达 squircle 全套

蓝底 #0A84FF + 双同心环 + 径向辉光扫描扇 + 青色扫描线 #64D2FF + 中心核心点。
tauri icon 从 1024 源 PNG 生成 icns/ico/png/Square 全套。

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 通知改走 tauri-plugin-notification

**Files:**
- Modify: `src-tauri/Cargo.toml`（加 `tauri-plugin-notification = "2"`）
- Modify: `src-tauri/src/lib.rs:355`（Builder 注册 plugin）+ `lib.rs:367`（setup 权限请求）+ `lib.rs:84`（poll_loop 调用）
- Modify: `src-tauri/src/notify.rs:46-52`（`send_notification` 用 `NotificationExt`）

**Interfaces:**
- Consumes: `tauri-plugin-notification` v2 crate
- Produces: `pub fn send_notification(handle: &tauri::AppHandle, title: &str, msg: &str)`（新签名，多一个 `handle` 参数）

> Rust 侧调用 plugin API，无需改 `capabilities/default.json`（capability 只管前端 JS→Rust 的 invoke 权限）。

- [ ] **Step 1: Cargo.toml 加依赖**

Modify `src-tauri/Cargo.toml`，在 `tauri-plugin-opener = "2"`（行 22）后加一行：

```toml
tauri-plugin-notification = "2"
```

- [ ] **Step 2: lib.rs Builder 注册 plugin**

Modify `src-tauri/src/lib.rs:354-355`，在 `.plugin(tauri_plugin_opener::init())` 后加：

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
```

- [ ] **Step 3: lib.rs setup 开头请求通知权限**

Modify `src-tauri/src/lib.rs:367` 的 setup 闭包，在 `|app| {` 之后、原有代码之前插入：

```rust
        .setup(|app| {
            // 通知权限：首次 Unknown 时请求（macOS 弹授权弹窗一次）。Granted/Denied 后不再烦扰。
            use tauri::plugin::PermissionState;
            use tauri_plugin_notification::NotificationExt;
            let notif = app.notification();
            if matches!(notif.permission_state(), Ok(PermissionState::Unknown)) {
                let _ = notif.request_permission();
            }
```

- [ ] **Step 4: notify.rs 改 send_notification**

Modify `src-tauri/src/notify.rs`，把 46-52 行的 `send_notification` 替换为：

```rust
/// 发 macOS 通知（tauri-plugin-notification，走原生 UserNotifications）。
/// 图标自动取 App bundle icon（icon.icns）——这就是「通知图标 = cc-view 雷达」的来源。
/// msg/title 不含双引号（调用方确保）。show() 轻量，可在 poll 线程内直接调用。
pub fn send_notification(handle: &tauri::AppHandle, title: &str, msg: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = handle
        .notification()
        .builder()
        .title(title)
        .body(msg)
        .show();
}
```

> 删除原 `std::process::Command::new("osascript")` 整段。文件顶部 `use crate::models::{Session, Status};` 与 `use std::collections::HashMap;` 保留（`Notifier` 仍用）。

- [ ] **Step 5: lib.rs poll_loop 调用适配新签名**

Modify `src-tauri/src/lib.rs:84`，把：

```rust
                notify::send_notification("cc-view", &format!("{}：{}", name, status_zh));
```

改为：

```rust
                notify::send_notification(&handle, "cc-view", &format!("{}：{}", name, status_zh));
```

- [ ] **Step 6: 编译验证**

Run:

```bash
cd src-tauri && cargo build 2>&1 | tail -25
```

Expected: 编译通过，无 `unused import` / 类型错误。

- [ ] **Step 7: 测试验证**

Run:

```bash
cd src-tauri && cargo test 2>&1 | tail -25
```

Expected: `Notifier::observe` 的 4 个测试全通过（签名变更不影响纯逻辑测试）。

- [ ] **Step 8: 运行端到端验证**

Run（前台观察）:

```bash
npm run tauri dev
```

确认：
- 首次启动：macOS 弹「cc-view 想要发送通知」授权弹窗 → 点允许
- 触发会话进入 NeedsPermission/WaitingForInput（如对某 claude 会话输入触发权限请求，或临时在 `collector` 注入假 session）
- 系统通知弹出，**图标为 cc-view 雷达**（不再是脚本编辑器图标）
- 通知标题 `cc-view`，正文「会话名：等待权限确认」/「会话名：等待输入」

- [ ] **Step 9: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/notify.rs
git commit -m "$(cat <<'EOF'
feat: 通知改走 tauri-plugin-notification，图标显示 App icon

替换 osascript display notification（图标错显为脚本编辑器）为原生
UserNotifications API；通知图标自动取 bundle icon.icns。首次启动
请求通知权限。

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## 最终验收（三任务全完成后）

- [ ] `cd src-tauri && cargo test` 全绿
- [ ] `npm run tauri dev` 联合验证：menubar 雷达剪影（深浅反色 + 染橙）+ 通知图标为 cc-view 雷达 + 通知标题/正文正确
- [ ] 可选：`npm run tauri build` 产出 `.app`，右键「显示简介」确认 app icon 为雷达；通知中心历史记录图标正确
