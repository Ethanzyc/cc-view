# 设计：搁置（Snooze）+ 等权限 Tray Badge 常驻

- 日期：2026-08-03
- 状态：待 review
- 关联原型：`docs/superpowers/prototypes/snooze-prototype.html`（已用 gstack 验证交互通过）

## 1. 背景与目标

cc-view 的核心价值是"一眼看出哪些 Claude 会话需要我介入"。本次增强两件事，**一起交付**：

1. **搁置（Snooze）**：`WaitingForInput`（等输入）当前一律高优先级排在前面，无法区分"马上继续"和"暂时不管"。新增一个用户手动标记 `snoozed`，把"暂时不管"的会话压下去；有新动静自动冒泡。
2. **等权限 Tray Badge 常驻**：`NeedsPermission`（等权限）是最该被看到的硬阻塞。在 menu bar tray icon 上常驻一个计数 badge，**不处理不消失**（直到该会话离开等权限状态）。

两个功能共用同一套"snoozed 推导"基础。

## 2. 现状（已确认）

- **App 模型**：`LSUIElement=true`（`src-tauri/Info.plist`），menu bar accessory app，**无 dock icon**。因此 dock badge 不可用，badge 只能画在 menu bar tray icon 上。
- **现有 tray**（`tauri.conf.json` app.trayIcon id="main"；`lib.rs:110-128` poll_loop 每轮更新）：
  - 聚合 `need_attention` = `alive && (NeedsPermission|WaitingForInput)`、`working` = `alive && Working`。
  - tooltip：`need_attention>0` → "N 等我 · M 工作"，否则 "M 工作"。
  - icon：`has_attention` 翻转时切换"橙色实色 template=false / 单色剪影 template=true"。
  - 左键点击 toggle HUD 窗口（`lib.rs:460`）。
- **现有手动标记先例**：`hidden.rs` `HiddenList`（`~/.claude/cc-view/hidden.json`，id 数组，`hide_session`/`unhide_session`/`list_hidden`）。snoozed 复用这套存储/命令模式，但存时间戳。
- **现有通知**：`notify.rs` `Notifier::observe` 在 alive 会话迁移到 `NeedsPermission|WaitingForInput` 时发系统通知（首轮静默、同状态防抖、dead 不通知）；`send_notification` 走 `tauri-plugin-notification`。

## 3. 功能一：搁置（Snooze）

### 3.1 概念

`Status` 描述 Claude 在干什么（客观）。新增正交维度 `snoozed`（用户主观），二者组合决定一行最终显示与排序。默认 `unsnoozed`，用户手动标记。

### 3.2 自动失效规则（核心）

搁置不是永久的。每条搁置记 `snoozedAt`（点搁置的时刻）。**有效搁置**判定（前端 + 后端共用同一逻辑）：

```
isSnoozed(session) =
    snoozedMap[session.id] 存在
  AND NOT ( session.statusUpdatedAt > snoozedAt
            AND session.status ∈ {WaitingForInput, NeedsPermission} )
```

语义：搁置后，只要该会话状态又更新过（`statusUpdatedAt` 变了）**且**停在"又需要你"的状态（等输入 / 等权限），就自动取消搁置、重新冒泡。搁置静止的会话保持压下。

> 边界正确性：搁置一个 `WaitingForInput` 会话时 `snoozedAt ≈ 当前`；若会话静止，`statusUpdatedAt` 不变 → 不失效。Claude 下一轮 `Working→WaitingForInput` 时 `statusUpdatedAt` 更新到 > `snoozedAt` → 失效冒泡。✓

### 3.3 展示

**排序**（在现有 `statusRank` 基础上插档 + 加项目维度）：

```
状态档：等权限(1) > 等输入(2) > 工作(3) > Shell(4) > 压缩(5)
        > 有效搁置(5.5) > 已退出(6) > 搁置且已退出(6.5)
同档内：按项目聚类（project 字典序）→ 再按时间近的靠前（ago 升序）
```

**分组**（一级 = 状态，二级 = 项目）：
- 一级小标题：`待介入 N` / `已搁置 N` / `已退出 N`，常驻展开（分隔线 + 小标题，不做可折叠）。
- 二级小标题：项目路径（`~/ai/fang` 形式，`/Users/<user>/` → `~/`）。同项目会话聚在一起。

**视觉**：
- 有效搁置行：`opacity: 0.5`（参考现有 dead 的 0.45，略高）。
- **状态文字保留 cc 真实状态**（如"等输入"），不显示"已搁置"——分组标题已表达搁置语义。颜色走 cc 状态色，靠行整体透明度淡化。
- 已搁置行不显示 `isFresh` 蓝点。

**dead 限制**：已退出会话最多显示 5 个（最近的），超出折叠为 `+X 个更早的已隐藏（共 Y）`。（数据源 `~/.claude/sessions/<pid>.json` 由 Claude Code 退出时自清理，dead 是过渡态不会无限累积；上限仅作极端兜底。）

### 3.4 交互

- **搁置按钮**：仅 `waitingForInput` 行显示文字药丸「搁置」；已搁置行显示「恢复」。`needsPermission` / `working` 不显示搁置入口。
- **HUD 与 Overlay 都做**。Overlay = HUD 的全集（待介入 + 已搁置 + 已退出，且不过滤 hidden）+ 搜索。
- **Overlay 补齐 HUD 的时间/颜色逻辑**：每行显示 `ago`；`needsPermission` 行橙边高亮（`perm` class）；`fresh`（<120s 等输入）蓝点。
- **Overlay 搜索**：输入即时过滤（name + 项目），**搜索态扁平列表**（不分组）、匹配文字高亮 `<mark>`、顶部显示结果计数；清空恢复分组。
- 行点击：HUD → `focus_session`；Overlay → `focus_session` + hide 窗口（现有行为）。

### 3.5 数据与存储

- 存储文件：`~/.claude/cc-view/snoozed.json`，结构 `{ "<sessionId>": <snoozedAt ms>, ... }`（需要时间戳，所以是 map 不是数组，区别于 hidden）。
- 后端模块：新增 `src-tauri/src/snoozed.rs`，`SnoozeMap { map: HashMap<String,i64> }`，镜像 `hidden.rs` 的 `load/save/add/remove` 模式（失败静默返回空、save 失败忽略），并暴露纯逻辑 `is_effectively_snoozed(session, &SnoozeMap) -> bool`。
- Tauri 命令：`snooze_session(id)` / `unsnooze_session(id)` / `list_snoozed() -> {id: i64}`，镜像 `hide_session`/`unhide_session`/`list_hidden`。
- **derived 字段**（关键决策）：在 `models.rs` `Session` 增加非持久化字段 `pub snoozed: bool`，由 poll_loop 每轮用 `is_effectively_snoozed` 算好后随 Session 一起 emit 给前端。这样 isSnoozed 逻辑只在后端一处（Rust），前端 + badge count 都消费同一来源，避免前后端推导不一致。（区别于 hidden 的"前端拉 list 自己 filter"，snoozed 因带时间推导逻辑，改用后端 derived。）
- **更新时效**：`snooze_session`/`unsnooze_session` 成功后前端**乐观更新**对应 `session.snoozed` 并立即重排；后端下次 poll（≤3s）用真实 derived 对齐（推导一致，无闪烁）。

### 3.6 前端改动

- `types.ts`：`Session` 加 `snoozed: boolean`。
- `App.vue`：拉 `list_snoozed` 不再必要（derived 已在 Session 里）；`activeCount` 等沿用。
- `SessionList.vue`：排序加项目维度 + 搁置档；分组渲染（一级状态 + 二级项目）；dead 上限 3 + 折叠提示；waitingForInput 行加「搁置」按钮、已搁置加「恢复」，调 `snooze_session`/`unsnooze_session`。
- `Overlay.vue`：补 `ago`/`perm`/`fresh`；搜索重写（扁平 + 高亮 + 计数）；同样分组 + 搁置按钮。
- `StatusIcon.vue`：不变。

## 4. 功能二：等权限 Tray Badge 常驻

### 4.1 概念

tray icon 上常驻显示 `needsPermission` 计数 badge，**只要还有等权限会话就一直显示**，全部处理完（状态离开等权限）才消失。不依赖用户点击清除，而是依赖会话状态变化——这正是"用户不点（不处理）就不消失"。

### 4.2 聚合（在现有 poll_loop 内）

新增 `perm_count` = `alive && status==NeedsPermission && !snoozed`（`!snoozed` 用 derived 字段；等权限是硬阻塞，snoozed 的会话按失效规则本应已自动 unsnooze，此处 `!snoozed` 作保险）。

> `perm_count` 是现有 `need_attention` 的真子集（need_attention 还含 waitingForInput）。

### 4.3 tray icon 更新（扩展现有 lib.rs:110-128）

- `perm_count > 0` → tray icon = **底图 + 右上角红圆 + 白色数字**（count>9 显示 `9+`）。
- `perm_count == 0 && need_attention > 0` → 现有橙色实色逻辑（waitingForInput 提醒，无 badge）。
- 都为 0 → 现有单色剪影。
- tooltip：`perm_count>0` 时标"N 等权限 · M 等我 · K 工作"（或合并表述），否则沿用现有。

### 4.4 与系统通知的关系

- **保留** `notify.rs` 现有首次通知（迁移到 NeedsPermission/WaitingForInput 发一条系统通知，提醒用户来看）。
- badge 负责"常驻视觉"。`WaitingForInput` 不进 badge（只发系统通知、正常消失）；`NeedsPermission` 既发首次通知又有常驻 badge。
- 现状"等权限通知常驻做不到（macOS 横幅无法 per-notification 不消失）"的缺口，由 badge 补上。

### 4.5 实现路径（二选一，实现时定）

- **路径 A（推荐）动态合成 icon**：用 `image` crate 在 `tray.png` 底图右上角合成红圆 + 数字（数字 0-9 + `9+`）。纯 Rust、可控、不依赖原生 hack。代价：需处理字体/数字绘制（可预生成 0-9+ 的圆点 PNG，运行时按 count 选并贴到底图，规避运行时文字渲染）。
- **路径 B NSStatusItem title**：用 `objc`/`cocoa` 直接拿 NSStatusItem 的 button 设 `title`（原生数字，无需画图）。更优雅但绕过 Tauri 抽象，需桥接，风险高。

推荐 A：与现有 `tint_orange`（`lib.rs:69`）合成 icon 的思路一致，复用图像处理模式。

### 4.6 "常驻"语义边界

badge 不响应用户点击清除（tray 左键仍是 toggle HUD）。清除条件仅 `perm_count` 归零，即所有等权限会话都已离开该状态（用户在终端批准/拒绝了权限请求，或会话死亡）。这保证"不处理就一直提示"。

## 5. 测试策略

- **`snoozed.rs` 单测**（镜像 `hidden.rs` tests）：load/save 往返、add 去重、remove；`is_effectively_snoozed` 的失效边界（静止不失效、新一轮 WaitingForInput 失效、NeedsPermission 失效、`statusUpdatedAt == snoozedAt` 不失效）。
- **collector/reducer 不变**（snoozed 是 cc-view 自己的标记层，不改 Claude 数据源解析）。
- **badge 聚合单测**：`perm_count` 在含/不含 snoozed、dead 会话时的计数。
- **前端**：原型已用 gstack 验证分组/项目聚类/dead 折叠/搜索（过滤+高亮+计数+清空恢复）全部通过；正式实现后复跑同组断言。
- **Tauri 命令**：`snooze_session`/`unsnooze_session`/`list_snoozed` 的基本往返。

## 6. 影响文件清单

**新增**
- `src-tauri/src/snoozed.rs`（SnoozeMap + is_effectively_snoozed）
- tray badge 图标素材（路径 A 的预生成圆点数字 PNG，或运行时合成）

**后端修改**
- `src-tauri/src/models.rs`：`Session` 加 `snoozed: bool`（derived，`#[serde(default)]` 兼容旧缓存）
- `src-tauri/src/lib.rs`：poll_loop 算 derived `snoozed` + `perm_count`；tray icon badge 合成与切换；注册 `snooze_session`/`unsnooze_session`/`list_snoozed` 命令
- `src-tauri/src/notify.rs`：不变（保留首次通知）

**前端修改**
- `src/types.ts`、`src/App.vue`、`src/components/SessionList.vue`、`src/components/Overlay.vue`

**不改**：`collector.rs`、`reducer.rs`、`statemachine.rs`、`StatusIcon.vue`、`tauri.conf.json`

## 7. 未决 / 风险

- **R1 badge 实现路径**（已定）：A（动态合成 icon）。运行时用 Rust 把红圆 + 数字画到 tray 图标 PNG 上再 `set_icon`，复用 `tint_orange` 图像处理模式。退化方案：若 `image` crate 合成成本高，预生成 0-9+ 共 11 张完整 badge icon 按 count 切换。
- **R2 项目聚类粒度**（已定）：二级标题用 `~/ai/fang`（`/Users/<user>/` → `~/`）。未来加配置项让用户自定义格式（本次不实现，预留）。
- **R3 dead 上限**（已定）：5。
- **R4 搁置与 hidden 并存**：一个会话同时被 hide + snooze 时，`hidden` 优先（直接不在 `visible` 里），与现有行为一致，无需特殊处理。
