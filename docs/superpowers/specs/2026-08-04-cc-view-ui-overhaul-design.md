# cc-view UI 大改造：合并 HUD 到命令面板

> 日期：2026-08-04
> 状态：已与用户对齐设计，待实现

## 背景与现状

cc-view 当前有**两个窗口**，职责分裂：

| 窗口 | label | 角色 | 关键行为 |
|------|-------|------|----------|
| HUD | `main` | 常驻会话面板 | 开机默认可见；标题栏 + 会话列表 +「刷新 / 显示已隐藏 / 置顶图钉」；可拖动记忆位置；普通 NSWindow |
| 命令面板 | `overlay` | 按需呼出的启动器 | ⌥Space 呼出；搜索框 + 分组列表 +「搁置 / 复制 ID」；**失焦自动收起**；NSPanel（跨全屏 Space、不激活 app） |

tray 菜单栏图标：仪表盘造型（圆环+指针+中心点），template image，当前菜单只有 `Quit`，**左键点击 = 显示 HUD**。

两者功能高度重叠（分组、搁置、排序逻辑各自重复一份），且 HUD 缺搜索、overlay 缺隐藏/显示已隐藏/置顶。用户希望合并成**一个面板**。

## 目标

废弃 HUD，让命令面板（overlay）成为 cc-view 的**唯一 UI**，承载两者全部功能；同时整理 tray 图标与菜单，使其符合标准 menu bar app 形态。

具体诉求（用户原话归纳）：
1. HUD 不要了，功能全部合并进命令面板
2. 命令面板可「定住」（pin），定住后不会因失焦消失
3. 「显示已隐藏」toggle 移到命令面板
4. 命令面板具备 HUD 所有功能与显示
5. 分组更清晰（待介入 / 项目名）
6. 每个会话行内的项目名去掉（与上方分组标题重复）
7. 隐藏按钮放进命令面板，显示为「隐藏」文字
8. 命令面板可拖动
9. tray 图标外框加粗、白色；左键点击弹原生菜单（版本号 / 偏好设置 / 检查更新 / 显示面板 / 退出），用官方样式

## 非目标（本次不做）

- **偏好设置**：菜单项占位（disabled），不实现设置窗口与设置项
- **检查更新**：菜单项占位（disabled），不集成 tauri-updater
- About 弹框：版本号直接以 disabled 菜单项静态展示，不做关于窗

---

## 设计

### Part A：架构与窗口 / tray 行为

#### A1. 窗口合并：删 main，overlay 升级为唯一面板

- `tauri.conf.json` 删除 `main` 窗口配置，只保留 `overlay`（label 不改名，减少改动面）；`visible: false`（开机默认隐藏，见 A2）
- overlay 保留现有的 NSPanel swizzle（`make_panel`）+ `join_all_spaces`——跨全屏 Space、不激活 app 的 Alfred 体验不丢
- `src/App.vue` 删除 `isOverlay` 分流逻辑，直接渲染面板组件（Overlay.vue 升级版）
- `src/components/SessionList.vue`（HUD 专用列表）**删除**——其分组/排序/行内逻辑已被 Overlay.vue 内联覆盖；共享工具仍在 `src/utils/session.ts`

#### A2. 可见性模型

- 开机默认**隐藏**（`visible: false`）
- 呼出方式二选一：⌥Space 全局快捷键 / tray 菜单「显示面板」
- 呼出后默认**未钉**（失焦自动收起）；点图钉「定住」后常驻

#### A3. pin（图钉 / 定住）语义

- **图钉只控制一件事**：「失焦是否自动收起」
  - 钉住（pinned=true）= 失焦不收起（常驻）
  - 未钉（pinned=false）= 现有 Alfred 行为（点别处就收起）
- `always_on_top` **保持 true 不动**：overlay 本就是 level 101 系统浮层，置顶无意义；pin 不再像旧 HUD 那样切 always_on_top，避免概念混淆
- pin 状态持久化（写进位置 json），开机/重新呼出按记忆恢复

> 取舍：旧 HUD 的图钉 = always_on_top 开关。合并后命令面板恒在最前，图钉改为「失焦收不收起」更贴合「定住」的心智模型。

#### A4. 拖动 + 位置记忆（顺带修一个现有 bug）

- overlay 顶栏当前用 `-webkit-app-region: drag`——**Electron 私有属性，WKWebView 不生效**（App.vue 注释已承认）。改用 Tauri 的 `data-tauri-drag-region`
- 新增位置持久化 `~/.claude/cc-view/overlay-position.json`，结构 `{ x, y, pinned }`（复用 `hud.rs` 思路）：
  - 拖动后 `WindowEvent::Moved` 存 (x, y, pinned)
  - 呼出时有记忆 → 恢复位置；无记忆 → `center()`
- 旧 `~/.claude/cc-view/hud-position.json` 不再使用（不主动迁移，旧文件留着无害）

#### A5. 失焦收起的双触发机制

现状失焦收起有两套（`WindowEvent::Focused(false)` + frontmost app 轮询）。合并后：
- **未钉**：两套都生效
- **钉住**：两套都跳过

实现：pin 状态存 `Mutex<bool>`（或 `State`），两个触发点读取判断；钉住时不启动 frontmost 轮询线程（省 CPU）。

#### A6. tray 图标

- `icons/source/tray.svg`：外圈圆环 `stroke-width` 加粗（2.6 → 约 3.5），保持黑色 + template（`iconAsTemplate: true` 不变）
- 深色菜单栏下系统自动反色为白色粗框 = 用户要的效果；浅色栏下黑色，符合 macOS menu bar 规范
- 重新导出 `icons/tray.png`
- badge 红点（等权限计数）继续画在右上角，造型不变 → 不用重新适配

#### A7. tray 左键 + 原生菜单

- `tauri.conf.json` 中 `trayIcon.showMenuOnLeftClick` 改 `true`，左键直接弹原生菜单
- 删除 `lib.rs` 中 `on_tray_icon_event` 的左键 toggle HUD 逻辑（左键现在交给菜单）
- ⌥Space 仍呼出面板；菜单「显示面板」也呼出
- 菜单项（原生 macOS 样式，`Menu::with_items`）：

  | id | 文本 | 状态 | 行为 |
 ----|------|------|------
  | `version` | `cc-view 0.1.0` | disabled | 只读版本号（版本取 `tauri.conf.json` 的 `version`） |
  | — | 分隔线 | | |
  | `show` | 显示面板 | enabled | show overlay + makeKey |
  | — | 分隔线 | | |
  | `prefs` | 偏好设置… | disabled（占位） | 本次 no-op |
  | `update` | 检查更新… | disabled（占位） | 本次 no-op |
  | — | 分隔线 | | |
  | `quit` | 退出 cc-view | enabled | `app.exit(0)` |

- 菜单事件用 `app.on_menu_event`（或 `Menu::on_event`）按 id 分发

---

### Part B：面板 UI

#### B1. 顶栏布局（同时是拖动区）

`search-bar` 升级为完整顶栏，仍是拖动区背景：

```
[🔍] [搜索框 ...............] [N 待介入] [☐ 显示已隐藏] [📌]
```

- 栏背景 `data-tauri-drag-region`（拖窗口）
- 搜索框 / 计数 / toggle / 图钉都 `no-drag`（可点可输入）
- 计数沿用现有逻辑：搜索态 →「N 个结果」；分组态 →「N 待介入」
- 图钉沿用旧 HUD 的 pin SVG（置顶时填充高亮、未钉时描边）

#### B2.「显示已隐藏」toggle

- checkbox +「显示已隐藏」文字（沿用旧 HUD 样式）
- **off（默认）**：分组态、搜索态都过滤掉 hidden 会话
- **on**：显示 hidden 会话，行更淡（opacity 降低）+ 行尾标「已隐藏」小字
- 恢复直接点行内「取消隐藏」，无需跳别处

#### B3. 行内操作按钮（三件套）

每行 `actions`：

| 按钮 | 条件 | 显隐 |
|------|------|------|
| 「搁置」/「恢复」 | snooze（仅 alive；恢复 = 已搁置时） | 常驻 |
| 「隐藏」/「取消隐藏」 | hide（文字，**非 ×**） | 常驻 |
| 「复制 ID」/「已复制」 | copy | **仅 hover 出现** |

命名两两区分、无歧义：搁置↔恢复 vs 隐藏↔取消隐藏。「复制 ID」低频且文字长，仅 hover 出现以避免三按钮常驻挤行。

#### B4. 项目名去重

- **分组态**：删行内 `line2`（`proj-head` 二级标题已显示项目名，重复）
- **搜索态**：**保留 `line2`**（扁平列表没有 `proj-head`，否则看不到项目归属）

#### B5. 分组视觉「清晰一些」

- `group-head`（待介入 / 已搁置 / 已退出）：加大上下间距 + 字重略提，标签更扎实
- `proj-head`（项目名）：改 mono 字体（`--font-utility`，呼应路径感）+ 色重略提，与一级标题层级拉开
- 组间 `group-sep` 分隔线保留
- 具体数值在实现时微调

#### B6. 刷新按钮：省略

overlay 已实时 `listen('sessions')` + 打开即拉 `get_sessions`，数据本就实时。HUD 当年需要刷新按钮是因为它不监听事件——合并后冗余。**本次不加刷新按钮**。

---

## 涉及文件

### 前端
- `src/App.vue` — 删 `isOverlay` 分流，直接渲染 Overlay；删 HUD 相关状态/样式
- `src/components/Overlay.vue` — 升级：顶栏（toggle/图钉/拖动 region）、hide 按钮、pin 与位置管理、分组视觉、项目名去重
- `src/components/SessionList.vue` — **删除**
- `src/utils/session.ts` — 基本不变（hide/unhide 调用迁入 Overlay）

### 后端
- `src-tauri/tauri.conf.json` — 删 `main` window；`showMenuOnLeftClick: true`
- `src-tauri/src/lib.rs` — 删 main setup（vibrancy/位置/事件）；overlay 加 pin 控制 + 位置持久化 + 失焦双机制跳过；tray 菜单构建 + 菜单事件分发；command 注册调整
- `src-tauri/src/hud.rs` — **删除**（名字绑定已废弃的 HUD 概念）
- `src-tauri/src/overlay_position.rs` — **新增**：overlay 位置持久化（`overlay-position.json`，结构 `{ x, y, pinned }`），逻辑照搬旧 hud.rs
- `src-tauri/icons/source/tray.svg` — 圆环描边加粗
- `src-tauri/icons/tray.png` — 重新导出

### 后端 command 变化
- 废弃：`get_hud_pinned` / `set_hud_pinned`（面向已删除的 main 窗口）
- 新增：`get_overlay_pinned` / `set_overlay_pinned`（控制失焦 hide + 持久化）
- 保留：`hide_session` / `unhide_session` / `list_hidden` / `focus_session` / `get_sessions` / `snooze_session` / `unsnooze_session` / `list_snoozed`

---

## 关键决策记录

1. **pin 只管「失焦收不收起」，不再切 always_on_top** — 命令面板恒在 level 101 浮层，置顶无意义；图钉语义收敛为「定住不消失」，最干净。
2. **版本号用 disabled 菜单项静态显示**，不做 About 弹框 — 更轻。
3. **偏好设置 / 检查更新占位 disabled** — 这两个是独立大功能，本次只搭菜单骨架。
4. **拖动改 `data-tauri-drag-region`** — 现有 `-webkit-app-region: drag` 在 WKWebView 根本不生效，属于既有 bug，顺带修。
5. **删 SessionList.vue** — 与 Overlay 列表逻辑重复，合并后无消费者。
6. **省略刷新按钮** — 实时监听已覆盖，按钮冗余。
7. **tray 图标保 template + 加粗** — 符合 macOS menu bar 规范，深色栏下即用户要的白色；固定白色会在浅色栏看不见。
