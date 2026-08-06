# cc-view 常驻模式（Resident Mode）设计

> 日期：2026-08-06
> 状态：已确认设计，待实现计划

## 背景

现有 overlay 是**面板模式**：⌥Space 呼出大命令面板（搜索框 + 分组列表 + 每行 focus/搁置/归档/复制操作），失焦自动收起（图钉可钉住）。它适合"主动介入"——搜索、跳转、整理会话。

但盯桌面这个场景它太重：尺寸大（560×420）、内容满（搜索栏 + 操作按钮），即使图钉钉住也占地方。用户要的是一种**常驻形态**：精简、贴桌面、失焦不收起、半透明融进桌面，随时瞥一眼就知道哪些会话在等什么。

两种形态是**同一个 overlay 窗口的两种模式**，同一时刻只显示一种，⌥Space 仍 toggle 显隐——显示的是当前选中模式的形态。

## 目标

- **常驻模式**：极简会话列表贴桌面常驻，失焦不收起，背景透明度可调。
- **默认 B 布局**：图标 + 名称 + 状态中文 + 分组（待介入/已搁置）+ 项目标题；可选 **A 极简布局**（图标 + 名称）。
- **可切换显示搁置 / 闲置**的会话。
- **面板 ⇄ 常驻一键互切**（偏好设置设默认 + overlay 内入口）。
- **所有配置在偏好设置**，常驻面板本身零控件（只留一个"展开成面板"入口）。

## 非目标（YAGNI）

- 不做常驻面板内的配置工具栏（搁置/闲置/透明度/布局切换）——配置全在偏好设置。
- 不做常驻模式右键操作菜单——要操作（搁置/归档/复制）就切面板模式。
- 不做两模式独立的位置记忆——MVP 共用 `overlay-position.json`。
- 透明度不作用于面板模式（面板是临时呼出交互，需保持清晰可读）。
- 不改变面板模式的现有内容与行为（仅加一个"收起成常驻"入口）。

## 设计

### 数据模型

`prefs.json`（`~/.claude/cc-view/prefs.json`）在现有 `notify` / `shortcut` / `poll_interval` 基础上新增字段。沿用 `prefs.rs` 的 serde `#[serde(default)]` 模式，旧文件缺字段时填默认值（向后兼容）。

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum OverlayMode { Resident, Panel }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ResidentLayout { B, A }

pub struct Prefs {
    // 现有
    pub notify: bool,            // default true
    pub shortcut: String,        // default "alt+space"
    pub poll_interval: u64,      // default 3
    // 新增
    pub mode: OverlayMode,                  // default Resident
    pub resident_layout: ResidentLayout,    // default B
    pub resident_show_snoozed: bool,        // default true
    pub resident_show_idle: bool,           // default true
    pub resident_opacity: u8,               // default 55，范围 20..=100
}
```

`resident_opacity` 存整数百分比（20–100）；`Opacity::default() = 55`。`set_resident_opacity` 做 `20..=100` 校验，越界返回 `Err`（fail fast）。

### 前端

**路由分发**：`App.vue` 现按 window label 分发（`prefs` → Preferences，其余 → Overlay）。改为 overlay 窗口内再按 `mode` 分发：

```
window label == "prefs"  → <Preferences />
window label == "overlay":
    mode == "panel"    → <PanelView />     （现有 Overlay.vue 改名）
    mode == "resident" → <ResidentView />  （新组件）
```

`mode` 是响应式 `ref<OverlayMode>`，初始从 `get_prefs` 读，`set_mode` 成功后更新（悲观：invoke 成功再改 ref）。切换时 App.vue 自动换视图。

**PanelView**（现有 Overlay.vue 改名）：内容不变，仅在搜索栏图钉旁加一个"收起成常驻"按钮，点击 `invoke('set_mode', { mode: 'resident' })`。

**ResidentView**（新组件，`src/components/ResidentView.vue`）：

- 数据自管：`listen('sessions')` + onMount `get_sessions`（与 PanelView 同源同事件）。
- 排序/分组/ago 复用 `utils/session.ts`（`statusRank` / `isStaleInput` / `projShort` 等，不抽 composable，MVP 重复可接受——与 PanelView 现状一致）。
- `now` tick（60s）驱动 `isStaleInput` 重算（同 PanelView）。
- **布局 B**（默认）：
  - 分组标题：`待介入 N` / `已搁置 N`（uppercase，带 count 徽标），同 PanelView 非搜索态分组。
  - 项目标题：`projShort(project)`（mono）。
  - 行：`[StatusIcon 14px] [会话名] [状态中文]`（状态中文右对齐）。
  - 行高约 26px。
  - 等权限行：橙左边框 + 浅橙底（`.row.perm`）；等回答行：黄左边框（`.row.reply`）；闲置/搁置行：`opacity 0.45`（`.row.dim`）。
  - **无** 搜索栏、操作按钮、时间 ago。
- **布局 A**：
  - 无分组标题、无项目标题、无状态中文、无时间。
  - 行：`[StatusIcon] [会话名]`，按 `statusRank` 排序（等权限优先），闲置/搁置灰显沉底。
  - 行高约 26px。
- **过滤**：
  - `resident_show_snoozed == false` → 过滤掉 `s.snoozed` 的会话（已搁置组整组隐藏）。
  - `resident_show_idle == false` → 过滤掉 `isStaleInput(s, now)` 的会话。
  - 过滤后若某分组空则不显示该分组标题。
- **点行 = focus**：`@click="focusSession(s.id)"`，沿用 PanelView 的 `focus_session` command（无操作按钮）。与面板模式不同，focus 后窗口**保持显示**——常驻语义不收起，不照搬 PanelView focus 后 `hide()` 的行为。
- **展开入口**：右上角绝对定位一个小图标按钮（"展开成面板"语义，右上箭头），点击 `invoke('set_mode', { mode: 'panel' })`。
- **透明度**：onMount + `resident_opacity` 变化时，`document.documentElement.style.setProperty('--resident-bg', rgba(28,28,30, opacity/100))`。ResidentView 根元素背景用 `var(--resident-bg)`。面板模式不受影响（用现有 `--color-bg-overlay`）。
- **拖动**：根元素 `data-tauri-drag-region="deep"`，展开按钮 `data-tauri-drag-region="false"`（同 PanelView 搜索栏模式）。
- **高度自适应**：渲染后用 `ResizeObserver` 或 sessions/布局/过滤变化时 `nextTick` 量 `rootEl.offsetHeight`，`invoke('set_resident_height', { height })` 调整窗口高度。设 `max-height: 60vh`，超出则内部滚动（常驻模式不应占满屏）。

### 后端

**新 commands**（`lib.rs`，镜像现有 `set_notify` / `set_interval` 模式——更新 `Mutex<Prefs>` + `save()`，校验 fail fast）：

| command | 作用 | 额外副作用 |
|---|---|---|
| `set_mode(mode)` | 存 `prefs.mode` | resize 窗口 + 重新定位（见下） |
| `set_resident_layout(layout)` | 存 `prefs.resident_layout` | 若当前为常驻模式，resize 宽度（A/B） |
| `set_resident_show_snoozed(bool)` | 存 `prefs.resident_show_snoozed` | 无（前端响应式过滤） |
| `set_resident_show_idle(bool)` | 存 `prefs.resident_show_idle` | 无 |
| `set_resident_opacity(u8)` | 存 `prefs.resident_opacity`（校验 20–100） | 无（前端响应式设 CSS 变量） |
| `set_resident_height(u32)` | 仅常驻模式：把窗口高度校正为内容高度 | `set_size(layout_width, height)` |

读：复用现有 `get_prefs`（返回完整 `Prefs`，前端按需取字段）。

**`set_mode` 的窗口调整**：

```
mode → panel:
    set_size(560 × 420)
    定位：overlay-position.json 有记忆 → 记忆位置；否则 center()
mode → resident:
    宽度 = resident_layout 宽度（A ≈ 150 / B ≈ 212）
    高度 = 先按当前窗口高度，随后前端量内容 invoke set_resident_height 校正
    定位：overlay-position.json 有记忆 → 记忆位置；否则右上角
         （右上角 = screen_width − window_width − margin(8), menubar 下方 margin(8)）
```

**`set_resident_height(height)`**：常驻模式专用，前端量得内容高度后调用，`set_size(layout_width, height)`。仅当当前 mode == resident 时生效（避免面板模式被误改）。

**失焦行为改造**（常驻 = always-pinned 语义）：

- `on_window_event(Focused(false))` handler：现有 `if !pinned { hide() }` → 改为 `if mode == resident || pinned { skip } else { hide() }`。`mode` 从 `Mutex<Prefs>` 读。
- `show_overlay` 的 frontmost 轮询线程：每轮检查 `mode`（从 state 读），`mode == resident` 时 `continue`（不 hide）。这样切到常驻后正在跑的轮询自然停止收起。

> 注：常驻模式仍走 `show_overlay` 显示（⌥Space / 托盘菜单「显示面板」），只是显示后不因失焦收起。窗口的 `join_all_spaces` / `make_panel`（NSPanel swizzle）/ vibrancy 设置在 setup 时一次性完成，两种模式共用，不重复。

### 窗口行为对照

| | 面板模式（现有） | 常驻模式（新） |
|---|---|---|
| 尺寸 | 560×420 固定 | 宽固定（A≈150 / B≈212）× 高自适应（max 60vh） |
| 失焦 | 收起（图钉可钉） | 不收起 |
| 透明度 | 固定（`--color-bg-overlay` 0.45） | 可调（`--resident-bg`，20%–100%，默认 55%） |
| 默认位置 | 居中 / 记忆 | 右上角 / 记忆 |
| 内容 | 搜索 + 分组列表 + 操作按钮 | 精简列表（B：图标+名称+状态+分组+项目标题 / A：图标+名称） |
| 入口 | 搜索栏图钉旁「收起成常驻」 | 右上角「展开成面板」 |

### 配置入口（偏好设置）

`Preferences.vue` 新增一个 `<section>`「常驻面板」，含：

- **默认形态**：select（常驻 / 面板），默认常驻 → `set_mode`
- **常驻布局**：select（B 精简 / A 极简），默认 B → `set_resident_layout`
- **显示搁置的会话**：checkbox，默认开 → `set_resident_show_snoozed`
- **显示闲置的会话**：checkbox，默认开 → `set_resident_show_idle`
- **背景透明度**：range slider（20–100），默认 55 → `set_resident_opacity`

沿用现有 `wrap(key, fn)` 悲观更新 + saving/error 反馈模式。

## 边缘情况

- **旧 prefs.json 无新字段**：serde `#[serde(default)]` 填充 → `mode=resident`、`layout=b`、两个 show=true、`opacity=55`。现有 prefs 测试模式（empty/partial/full roundtrip）扩展覆盖新字段。
- **会话数为 0**：常驻模式显示空状态（小窗「暂无会话」），不隐藏窗口（用户能知道它在跑）。
- **会话很多**：`max-height: 60vh`，超出内部滚动。
- **切布局 A↔B**：宽度变化（`set_resident_layout` 内 resize 宽度），高度由前端重新量。
- **全屏 app 下的常驻**：`join_all_spaces` 已在 setup 设置，常驻模式继承，跨 Space 含全屏可见。
- **透明度极端值**：100% = 不透明白底；20% = 很透（vibrancy + 桌面强烈透出）。文字始终前景色，可读性由 vibrancy 托底（与现有 `--color-bg-overlay` 同理）。
- **模式切换时窗口可见**：用户在可见的常驻面板点"展开" → 同窗口切视图 + resize，不 re-show；反之亦然。

## 测试

**Rust（`prefs.rs`）**：
- `empty_json_uses_defaults` 扩展：新字段填默认（mode=resident, layout=b, show_*=true, opacity=55）。
- `partial_json_keeps_defaults_for_missing` 扩展：缺部分新字段时其余填默认。
- `full_json_roundtrip` 扩展：含所有新字段。
- `set_resident_opacity` 越界（19 / 101）返回 Err。

**Rust（`lib.rs` 集成，若易测）**：
- `set_mode` 在 resident 时 `Focused(false)` 不 hide（逻辑分支单测，若可抽离）。

**前端手动验证**：
- 常驻模式默认显示 B 布局、右上角、失焦不收起。
- 偏好设置切 A/B、开关搁置/闲置、拖透明度 slider，常驻面板即时反映。
- 点行 focus 跳终端；点展开图标切到面板模式（560×420，搜索栏出现）。
- 面板模式点"收起"切回常驻（精简、右上角）。
- ⌥Space 在两模式下都能 toggle 显隐。
- 旧 prefs.json（删掉新字段）启动后常驻模式正常，默认值生效。

## 实现顺序（供 writing-plans 参考）

1. `prefs.rs`：新增枚举 + 字段 + default + 测试。
2. 后端 commands：`set_mode` / `set_resident_layout` / `set_resident_show_snoozed` / `set_resident_show_idle` / `set_resident_opacity` / `set_resident_height` + 注册。
3. 失焦行为改造（`on_window_event` + frontmost 轮询读 mode）。
4. `App.vue`：overlay 窗口按 mode 分发；现有 `Overlay.vue` 改名 `PanelView.vue` + 加"收起"按钮。
5. `ResidentView.vue`：B/A 布局 + 过滤 + 点行 focus + 展开入口 + 透明度 + 高度自适应。
6. `Preferences.vue`：新增「常驻面板」section。
7. 手动验证全部场景。
