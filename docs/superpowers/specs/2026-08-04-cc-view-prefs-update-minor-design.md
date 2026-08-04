# cc-view 偏好设置 + 检查更新 + Minor 收尾 Design

> 2026-08-04。plan-6（overlay 命令面板）落地、HUD 窗口删除后的三项收尾。

## 概述

tray 菜单「偏好设置…」「检查更新…」目前是 `enabled=false` 占位（`lib.rs:632-645`），菜单事件只处理 `show`/`quit`（`lib.rs:665-669`）。本设计把占位填成可用功能，并清一批 minor：

1. **偏好设置** —— 新窗口 + `accessory ⇄ regular` 切换 + 4 个设置项
2. **检查更新** —— `tauri-plugin-updater` + GitHub Releases + ed25519 验签
3. **Minor** —— `overlay_position` 单测补全、前端 `hidden` 改 `Set`、过时注释清理

## Global Constraints

- macOS；零侵入；`dirs::home_dir()`；代码英文 / 注释中文；fail fast。
- 复用既有模式：`~/.claude/cc-view/*.json` 持久化、`objc2` 调 `NSApp`、`App.vue` 按 `getCurrentWebviewWindow().label` 分发（plan-6 引入，HUD 删后 `App.vue` 退化为单分支，本次恢复多分支）。
- 偏好设置 / 检查更新交互用中文，代码用英文。

---

## 1. 偏好设置

### 1.1 窗口架构

- `tauri.conf.json` `app.windows` 加 `prefs` 窗口：
  ```json
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
  普通窗口风格（有标题栏），区别于 overlay 的无装饰浮层。
- `App.vue` 恢复 label 分发：`getCurrentWebviewWindow().label === 'prefs'` → `<Preferences/>`；否则 → `<Overlay/>`。共享 `main.ts` 入口与 design token。
- 新组件 `src/components/Preferences.vue`：4 个设置项 + 「检查更新」区（见 §2.3）。

### 1.2 dock 行为（accessory ⇄ regular）

cc-view 是 `LSUIElement` accessory app，平时无 dock。偏好设置需要 app 入口 → 点开时转 regular（dock 出现），关闭转回 accessory。

- **打开 prefs**：tray「偏好设置」菜单事件 → `setActivationPolicy:regular(0)` → `show` prefs 窗口 → `makeKeyAndOrderFront`。
- **关闭 prefs**：prefs 窗口 `on_window_event(CloseRequested)` → `setActivationPolicy:accessory(1)` → `hide`（不销毁，复用）。
- 实现：`objc2::msg_send![NSApp, setActivationPolicy: policy]`（`0` regular / `1` accessory），封装成 `set_activation_policy(policy: i64)` 辅助函数。
- **验证注意**：dev 模式裸二进制无 `Info.plist` 本就是 regular（有 dock），build 后 `.app` 才是 accessory —— dock 切换必须在 build 后的 `.app` 验证。

### 1.3 存储 `prefs.rs`

新模块 `src-tauri/src/prefs.rs`，沿用 `overlay_position.rs` 的 load/save 模式：

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Prefs {
    #[serde(default = "default_true")]
    pub notify: bool,            // 通知开关，默认 true
    #[serde(default = "default_shortcut")]
    pub shortcut: String,        // 快捷键，默认 "alt+space"
    #[serde(default = "default_interval")]
    pub poll_interval: u64,      // 轮询间隔（秒），默认 3
}
```

- `load()` / `save()` 读写 `~/.claude/cc-view/prefs.json`，文件不存在 / 无 home / 解析失败 → 默认值（不崩溃）。
- 开机自启动**不进 prefs.json**（`tauri-plugin-autostart` 自管状态）。
- app 启动时 `.manage(Mutex::new(Prefs::load()))` 注入 state。

### 1.4 四个设置项

| 项 | 后端 | 前端 |
|---|---|---|
| **开机自启动** | `tauri-plugin-autostart`（`MacosLauncher::LaunchAgent`）；`enable()`/`disable()`/`is_enabled()` | 开关，调 `toggle_autostart` command |
| **通知开关** | `Mutex<Prefs>.notify`；poll_loop 每轮读，`false` 跳过 `send_notification`（emit / tray badge 不受影响） | 开关，调 `set_notify` command |
| **全局快捷键** | 预设四选一：`alt+space` / `cmd+alt+space` / `ctrl+space` / `off`。改时 `global_shortcut().unregister_all()` + 按新值 `register`（off 则不注册）。启动按 prefs 注册 | 单选，调 `set_shortcut` command |
| **轮询间隔** | `AtomicU64`（秒，1–30）；poll_loop `sleep` 前读当前值；`set_interval` command 更新 atomic + 存 prefs.json | 数字输入，调 `set_interval` command |

**快捷键 handler**：保留单个通用 handler（toggle overlay），对当前注册的任意快捷键响应；改键 = 换注册的 shortcut，handler 不变。

### 1.5 文件清单（偏好设置）

- `src-tauri/Cargo.toml` —— `+ tauri-plugin-autostart = "2"`
- `src-tauri/tauri.conf.json` —— `+ window "prefs"`
- `src-tauri/capabilities/default.json` —— `windows` 加 `"prefs"`
- `src-tauri/src/prefs.rs`（新）
- `src-tauri/src/lib.rs` —— autostart 插件、prefs state、`set_activation_policy`、`open_prefs`、`set_notify`/`set_shortcut`/`set_interval`/`toggle_autostart` commands、快捷键按 prefs 注册、poll_loop 读间隔/通知开关、菜单事件 `prefs`
- `src/App.vue` —— label 分发
- `src/components/Preferences.vue`（新）

---

## 2. 检查更新

### 2.1 依赖与配置

- `Cargo.toml` `+ tauri-plugin-updater = "2"`。
- `tauri.conf.json`：
  ```json
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/Ethanzyc/cc-view/releases/latest/download/latest.json"
      ],
      "pubKey": "<ed25519 公钥，生成后填>"
    }
  },
  "bundle": {
    "createUpdaterArtifacts": "v1Compatible"
  }
  ```
  （`createUpdaterArtifacts` 取值按 tauri-plugin-updater 实际版本核对，实现时确认。）
- `capabilities/default.json` 加 `"updater:default"`，`windows` 已含 prefs（§1.5）。

### 2.2 签名密钥

- 实现阶段 `tauri signer generate` 生成 ed25519 对：
  - **私钥** `~/.tauri/cc-view.key`（加 `.gitignore`，**绝不入库**；构建时 `TAURI_SIGNING_PRIVATE_KEY` 环境变量引用）
  - **公钥** 写进 `tauri.conf.json` `plugins.updater.pubKey`
- 建议私钥无密码，简化后续 CI / 发布脚本（用户已认可）。

### 2.3 流程

- Rust `check_update` command：`app.updater()?.check()` → `Option<Update>`，返回 `{ available: bool, version: Option<String>, notes: Option<String> }`。
- Rust `install_update` command：`update.download_and_install()` → `tauri::process::restart()`。
- **UI 入口**：`Preferences.vue` 内「检查更新」按钮（+ 版本号展示）。tray「检查更新」菜单事件 = `open_prefs` + 自动触发一次 check（复用 prefs 窗口与 regular/dock 流程，避免再管一个窗口的 dock）。
- **无更新反馈**：若由 tray 触发且 prefs 原本关闭，发系统通知「已是最新版本」而不强制开窗；prefs 内触发则在窗口内显示「已是最新」。
- **有更新**：prefs 窗口显示新版本号 / notes / 「下载并安装」按钮 → `install_update` → 重启。

### 2.4 发布流程（实现阶段执行，均为对外操作，执行前确认）

1. 建 GitHub repo `Ethanzyc/cc-view`（公开），`git remote add origin` + push。
2. `tauri signer generate` 生成密钥对，公钥落配置、私钥落本地。
3. `TAURI_SIGNING_PRIVATE_KEY=~/.tauri/cc-view.key npm run tauri build` 生成签名 `.dmg` + `.app.tar.gz` + `.sig`。
4. 用 `tauri` 的 `updater` 产物脚本（或手写）生成 `latest.json`。
5. `gh release create v0.1.0` 上传 bundle + `latest.json`（首个 release 建立基线）。
6. 后续 bump 到 `v0.1.1` 验证 updater 端到端（旧版检查到新版 → 下载安装 → 重启）。

> 首个 release `v0.1.0` 时 updater 查到的"最新"就是自身版本 → 报"已是最新"，无法验证"有更新"分支；真正的更新链路在 `v0.1.1` 才完整可测。实现时如需立即验证，可临时把本地版本降到 `0.1.0-rc` 再检查。

### 2.5 文件清单（检查更新）

- `src-tauri/Cargo.toml` —— `+ tauri-plugin-updater`
- `src-tauri/tauri.conf.json` —— `plugins.updater` + `bundle.createUpdaterArtifacts`
- `src-tauri/capabilities/default.json` —— `+ updater:default`
- `src-tauri/src/lib.rs` —— updater 插件、`check_update`/`install_update` commands、菜单事件 `update`
- `src/components/Preferences.vue` —— 检查更新区（与 §1.4 同一组件）
- `.gitignore` —— `~/.tauri/cc-view.key` 不入库（实际私钥在 home 不在 repo，但若有 CI 配置需排除）

---

## 3. Minor

### 3.1 `overlay_position` 单测补全

**问题**：`load()` / `save_all()` 路径硬编码 `~/.claude/cc-view/overlay-position.json`，无法隔离测试。现有 3 个测试只覆盖 serde（roundtrip / 旧 json 无 pinned / 非法 json），没覆盖 `save(x,y)` 保留磁盘 pinned 与 `save_all` 写盘。

**方案**：抽取路径参数：
```rust
impl OverlayPosition {
    pub fn load() -> Option<Self> { Self::load_from(default_path()) }
    pub fn load_from(path: &Path) -> Option<Self> { /* 现有逻辑，参数化 */ }
    pub fn save_all(x, y, pinned) { Self::save_all_to(default_path(), x, y, pinned) }
    pub fn save_all_to(path: &Path, x, y, pinned) { /* 参数化 */ }
}
```
`save(x,y)` 内部调 `load()`（便捷版）取 pinned 再 `save_all`，逻辑不变。
新增测试（`tempdir` 隔离）：
- `save_preserves_existing_pinned` —— 磁盘有 `{pinned:true}`，调 `save(10,20)` 后 `load` 仍 `pinned:true`、`x:10,y:20`。
- `save_all_roundtrip` —— `save_all_to(tmp, 1, 2, true)` → `load_from(tmp)` 字段全等。

### 3.2 `hiddenSet`（前端 `hidden` 改 `Set`）

**现状**：`Overlay.vue:19` `const hidden = ref<string[]>([])`，模板/脚本多处 `.includes(s.id)` 做 O(n) 查询（行 29/272/304/321-323/353/372/389-391 等）。

**方案**：改 `const hidden = ref<Set<string>>(new Set())`：
- 赋值整体替换触发响应式：`hidden.value = new Set(await invoke<string[]>('list_hidden'))`。
- `.includes(s.id)` → `.has(s.id)`。
- `hide`/`unhide` 后重建 Set（`refreshHidden` 重新拉取整体替换）。

### 3.3 过时注释清理

`lib.rs` 通读，清理提已删 HUD/main 窗口的残留，初步识别：
- L346「仅对 overlay 调；HUD（main）保持默认（不跨全屏，避免干扰沉浸）」—— main 已删，删掉该句。
- L430-431「make_key 已移除」整段过渡注释 —— 删除。
- L482「HUD 已删无牵连」—— 简化。
- L583「与 HUD 视觉一致」—— 改为「与 overlay 视觉一致」或删除。
- 实现时通读补全其余。

---

## Self-Review

- **范围**：三项独立但同属 plan-6 收尾，规模中+中+小，合一 spec 合理；实现拆 2 个 plan（preferences / updater）+ minor 并入其一或单独小 plan。
- **placeholder**：updater `pubKey` / `createUpdaterArtifacts` 标注「生成后填 / 按版本核对」—— 非占位，是受外部依赖约束的待填项。无 TBD。
- **一致性**：prefs 窗口复用 `App.vue` label 分发与 overlay 同源；updater UI 复用 prefs 窗口；`prefs.rs` 沿用 `overlay_position.rs` 模式。
- **模糊点**：无。
