# cc-view 全套图标重新设计

- **日期**：2026-08-03
- **状态**：design（brainstorming 产出，待 review）
- **范围**：App icon 全套 + menubar tray icon + Mac 通知图标

## 背景与目标

cc-view（macOS menubar 常驻 Tauri 应用）当前有两个图标相关问题：

1. **App 图标为 Tauri 脚手架默认**（Tauri/Vue logo），`src-tauri/icons/` 下 18 个文件需全新品牌设计。
2. **Mac 通知图标错误**：通知经 `osascript display notification` 发送（`src-tauri/src/notify.rs:47`），macOS 会显示「脚本编辑器」图标而非 cc-view 的 App 图标——仅换图标文件无法解决，必须改发送方式。

**目标**：以「指挥台雷达」概念 + 系统蓝品牌重新设计全套图标；并将通知改走 `tauri-plugin-notification`，使新 App 图标同时出现在通知中心。

## 设计语言

| 维度 | 决策 | 依据 |
|---|---|---|
| 品牌色 | macOS 系统蓝 `#0A84FF` | design-system.json 主色；独立于 Claude 商标；不与 attention 橙 `#FF9F0A` 撞色 |
| 风格 | 克制 / minimal / flat | design-system `axis.colorMood: minimal, material: flat-glass` |
| 概念 | 指挥台雷达 | README「指挥台」定位；呼应 Working 呼吸签名动效 |
| 强调色 | 扫描线 `#64D2FF`（compacting 语义色） | 呼应 design-system 语义色体系 |
| attention 色 | `#FF9F0A`（system orange） | design-system `needsPermission`；现有 `tint_orange` 已用此色 |

## 资产清单

### A. App Icon（彩色 squircle）

**源图**：1024×1024 PNG，macOS squircle（连续曲率圆角，rx≈22%）。

**构图**（viewBox 0 0 300 300，按比例放大至 1024）：

- 背景：squircle 实色 `#0A84FF`
- 中心辉光：r≈78 径向渐变圆，`#FFFFFF` 从中心 opacity 0.20 → 0
- 外环：r≈104，stroke `#FFFFFF` width 5，opacity 0.16
- 中环：r≈70，stroke `#FFFFFF` width 6，opacity 0.42
- 扫描扇：从 −90°（正上）到 −25°（右上）的扇区，半径 104，填充径向渐变（`#FFFFFF` 从中心 opacity 0.42 → 边缘 0），模拟雷达辉光发散
- 扫描线（前缘）：中心(150,150)→(244,106)，青色 `#64D2FF`，width 6 圆头，opacity 0.95；其下叠 width 12、opacity 0.22 的同色辉光
- 中心核心点：r≈20 实心白；外加 r≈29、stroke `#FFFFFF` width 3.5、opacity 0.5 的光环

**生成**：`npm run tauri icon <1024源图>` 自动产出全套，覆盖 `src-tauri/icons/`：

- `icon.icns`（macOS App icon，通知 / Dock / 关于窗口取此）
- `icon.ico`（Windows）
- `icon.png`（512）、`128x128.png`、`128x128@2x.png`、`32x32.png`
- `Square{30,44,71,89,107,142,150,284,310}x*.png` + `StoreLogo.png`（Windows Store）

源 SVG 保留在 `src-tauri/icons/source/` 便于后续重生成。

### B. Menubar Tray Icon（单色剪影）

**变体 B 单环**（viewBox 0 0 44 44）：

- 环：r=14，stroke width 2.6，opacity 0.55
- 扫描线：中心(22,22)→(35,16)，stroke width 2.8 圆头，实色
- 中心点：r=3.8 实心
- 颜色：纯黑 `#000000` + 透明背景（template image 要求 black + clear；半透明环以 alpha 体现）

**template 动态切换方案（推荐）**：

- **正常态**：单色剪影 + `iconAsTemplate=true` → macOS 自动适配深浅模式（深色栏显白、浅色栏显黑），完美融入 menubar
- **attention 态**：同形状橙色剪影（`#FF9F0A`，保留 alpha 半透明环）+ `iconAsTemplate=false` → 显示彩色橙，跳出 menubar 引起注意
- **实现**：Tauri `TrayIcon::set_icon_with_as_template(icon, as_template)` 原子切换（macOS only，API 已查证存在）
- **橙色版**：运行时由 `tint_orange` 从单色剪影生成（把 alpha>0 像素 RGB 置 `#FF9F0A`，保留 alpha），不维护第二份资源文件

**fallback（Option 1）**：实色蓝 `#0A84FF` 剪影 + `iconAsTemplate=false`，染橙逻辑零改动。缺点：非标准 template，深浅模式不自动反色（蓝色在深浅栏均可见，但融入感弱）。若 `set_icon_with_as_template` 在目标 macOS 版本表现异常，回退此方案。

**文件**：新增 `icons/tray.png`（44×44 单色剪影）。`tauri.conf.json` 的 `trayIcon.iconPath` → `icons/tray.png`，`iconAsTemplate` → `true`。

### C. 通知图标

**无单独资产**。接入 `tauri-plugin-notification` 后，通知经 macOS UserNotifications API 发送，图标自动取 App bundle 的 `icon.icns`（即资产 A）。

## 代码改动

### 1. 通知发送方式（`Cargo.toml` + `lib.rs` + `notify.rs`）

- `Cargo.toml`：加 `tauri-plugin-notification = "2"`
- `lib.rs`：`Builder::default().plugin(tauri_plugin_notification::init())`；启动时 `permission_state` 为 Unknown 则 `request_permission()`
- `notify.rs`：`send_notification` 改为接收 `&AppHandle`，用 `NotificationExt`：
  ```rust
  use tauri_plugin_notification::NotificationExt;
  handle.notification().builder().title(title).body(msg).show()?;
  ```
- 移除 osascript 的 `std::process::Command::new("osascript")` 路径
- poll_loop 已持有 `handle: AppHandle`，调用处理论直接传引用（注意 `send_notification` 当前 `spawn` 不阻塞，改 plugin 后 `show()` 同样轻量，可在 poll 线程内调用）

> `notify.rs` 现有单测针对 `Notifier::observe`（纯逻辑），不涉及 `send_notification`，签名变更不影响测试。

### 2. tray icon 配置与染橙（`tauri.conf.json` + `lib.rs`）

- `tauri.conf.json`：`trayIcon.iconPath` → `icons/tray.png`，`iconAsTemplate` → `true`
- `lib.rs` poll_loop：
  - `include_bytes!("../icons/tray.png")` embed 单色剪影，启动时加载为 `Image`
  - 预计算橙色剪影（`tint_orange` 复用，输入改为剪影而非 `default_window_icon`）
  - attention 跳变（0↔>0）时 `tray.set_icon_with_as_template(img, as_template)` 原子切换：
    - 正常 → `(剪影, true)`
    - attention → `(橙剪影, false)`
  - 移除对 `default_window_icon()` 的 tray 用途依赖（app icon 仍作窗口图标）

## 验证

1. **源图生成**：Chrome headless（已验证可用）把设计 SVG 渲染成 1024 PNG（app icon 源）与 44 PNG（tray 剪影）
2. **资产生成**：`npm run tauri icon <1024源>` 产出全套，确认 `icons/` 文件更新
3. **运行验证** `npm run tauri dev`：
   - menubar 显示雷达剪影；切换系统深浅模式，确认 template 自动反色
   - 触发会话 NeedsPermission / WaitingForInput，确认：图标染橙 + 系统通知弹出 + **通知图标为 cc-view 雷达**（非脚本编辑器）
   - 首次运行确认通知权限请求弹窗
4. **测试**：`cargo test`（notify.rs 测试不受影响，确认仍通过）

## 不在范围

- design-system.json 不改（图标独立于 HUD 配色）
- HUD / overlay 前端 UI 不改
- Windows/Linux 的 tray template 行为差异（应用目标 macOS；icon 全套含 Windows 资源但不调试 Windows tray）
