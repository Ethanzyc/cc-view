# cc-view 设计文档

> 跨终端的 Claude Code 会话状态总览 · macOS menubar 常驻应用
> 创建于 2026-08-01

## 1. 问题与定位

**问题**：用户同时在多个项目、多种终端/IDE（iTerm2、IDEA、VS Code、Ghostty、cmux、Otty）里开着大量 Claude Code 会话，无法一眼掌握"哪个在等我（权限/输入）、哪个在干活、哪个空闲"。

**定位**：cc-view 是一个 macOS menubar 常驻应用，**跨所有终端/IDE 统一总览**本机正在运行的 Claude Code 会话状态。

**为什么不直接用现成的**：用户已装的 Otty、cmux 本身是 agent-aware 终端，但它们**只看自己内部**的会话；iTerm2/Ghostty/VS Code/IDEA 完全没有 agent 感知。没有任何工具能给出跨这些异构宿主的统一视图。cc-view 的数据源是 Claude Code 官方本地文件，与终端无关，天然跨终端——这是 Otty/cmux 做不到的。

**非目标**（明确不做）：
- 不做用量/成本/token 分析（已有 CCDash，跳转过去即可）。
- 不做会话内容的 `kill`/删除（不可逆、危险）。"管理"只做安全的隐藏/归档。
- 不替代任何终端，不承载会话进程。

## 2. 需求

| 编号 | 需求 | 说明 |
|---|---|---|
| A | 不漏"等我" | 某会话进入 NeedsPermission / WaitingForInput 时，第一时间 macOS 通知 |
| B | 总览全局 | menubar 一眼看所有会话此刻状态 + 所属项目 + 会话名 |
| C | 一键跳回 | 点击会话 focus 到承载它的终端窗口/tab/pane |
| D | 历史/成本 | 不自造，跳 CCDash |
| E | 隐藏/归档 | 把某些会话从总览收起，数据不动、可逆 |

## 3. 技术栈与形态

- **形态**：Tauri 2 menubar app（`NSStatusItem` tray 常驻，无 dock 图标），点图标弹 `NSPanel` popover（全屏时不抢焦点）。
- **前端**：Vue 3（用户熟练栈）。
- **后端**：Rust（`src-tauri/`）。
- **分发约束（关键）**：必须**禁用 App Sandbox**——focus 依赖的 `proc_pidinfo` / `sysctl` / `osascript` 在 sandbox 内不可用。走 **Developer ID + 公证**，不进 App Store。

## 4. 架构总览

```
┌──────────────────────────────────────────────────────┐
│  Vue 前端 (NSPanel popover)                            │
│  会话列表/分组/搜索 · 隐藏管理 · 详情 · CCDash 跳转       │
└──────────────▲───────────────────────────────────────┘
               │ Tauri events: emit("sessions", Vec<Session>)
┌──────────────┴───────────────────────────────────────┐
│  Rust 后端 (src-tauri/src/)                            │
│                                                        │
│  collector → reducer → statemachine → ipc              │
│      │          │          │                           │
│   permission  liveness   discovery → focus             │
│                                                notify   │
│                                                hidden   │
└──────┬─────────────────────────────────────────────────┘
       │ 读（零侵入，不装 hook）
┌──────┴─────────────────────────────────────────────────┐
│  数据源（Claude Code 官方文件 + agent-aware 终端）        │
│  ~/.claude/sessions/<pid>.json        主源（前台）       │
│  `claude agents --json`               主源（CC≥2.1.145）│
│  sysinfo 进程扫描                      主源（发现/存活） │
│  ~/.claude/daemon/roster.json         补充（后台fleet） │
│  ~/.claude/projects/<proj>/<sid>.jsonl 末尾2KB  权限预测/兜底│
│  Otty / cmux 状态与 pane 快照          补充（agent终端） │
└────────────────────────────────────────────────────────┘
```

**数据流**：3s 轮询触发 collector → 采集所有源 → reducer 去重合并 → statemachine 算统一状态 → 内容 hash 去重 → 仅变化时 `emit("sessions")` → Vue 响应式更新。

**更新机制**：**纯轮询（默认 3s，可配）+ 内容 hash 去重**。不用 FSEvents——c9watch 实测 3.5s 纯轮询在 100+ 会话下不卡 UI、够实时，且更简单可靠（避开 macOS FSEvents 边缘 case）。

## 5. 数据源

按优先级和稳定性排序。所有源都**只读**，零侵入。

### 5.1 主源

**`~/.claude/sessions/<pid>.json`**（前台交互会话）
```json
{"pid":27074,"sessionId":"...","cwd":"/Users/.../cc-view",
 "name":"cc-view-94","nameSource":"derived","status":"busy",
 "startedAt":...,"statusUpdatedAt":...,"kind":"interactive","version":"2.1.201"}
```
- 文件名是 CLI 进程 pid；`status` 实测有 `busy`/`shell`（全集待运行时补全，见 §14）。
- 这是**最直接的前台会话状态源**。

**`claude agents --json`**（CC ≥ 2.1.145）
- 官方命令，直接吐后台 agent 的 busy/idle 状态。覆盖 fleet agent。
- 实现时核实确切输出 schema。

**sysinfo 进程扫描**
- 发现所有 `claude` 进程，校验 pid 存活（见 §6.3 liveness）。
- 进程的 env / 父进程链用于 focus 元数据预解析（见 §8）。

### 5.2 补充源

**`~/.claude/daemon/roster.json`**（后台 fleet agent 注册表）
- ⚠️ **不作为唯一源**：有官方 stale 残留 bug（[#78454](https://github.com/anthropics/claude-code/issues/78454)）。
- 提供 worker 的 `pid`/`sessionId`/`cwd`/`intent`。
- **强制 pid 存活校验**后才采纳；死的标 stale 不展示。
- 用于补充 sessions 文件未覆盖的后台 agent。

**JSONL 末尾（`~/.claude/projects/<proj>/<sid>.jsonl`）**
- `SeekFrom::End` 只读末尾 ~2KB，用于：① PermissionChecker 判断 tool_use 是否待批准；② 状态兜底。
- 不全量解析。

**agent-aware 终端自带状态**（零侵入可读，补充 + focus 辅助）
- Otty：`otty-cli ipc agent.query -p session_id=<uuid>` → `{found,agent,state}`（**不含 pane id**）。
- cmux：`~/.cmuxterm/claude-hook-sessions.json`（session 状态）、`~/.cmuxterm/events.jsonl`（NDJSON 事件流，可 tail）。

### 5.3 去重合并

- 主键：`sessionId`。同一 sessionId 可能同时出现在 sessions 文件和 roster（前台+后台），按 sessionId 合并，字段取最丰富的。
- pid 作为辅助键（一个会话可能有多个进程：CLI + daemon worker）。

## 6. 核心模型与状态机

### 6.1 统一 Session 模型

```rust
pub struct Session {
    pub id: String,              // sessionId（主键）
    pub source: Source,          // Interactive | Fleet | Slash
    pub pid: u32,
    pub pids: Vec<u32>,          // 关联的全部进程（CLI + worker）
    pub project: String,         // cwd 末段
    pub cwd: String,
    pub name: String,            // sessions.name 或 roster intent
    pub status: Status,          // 统一状态机
    pub started_at: i64,
    pub status_updated_at: i64,
    pub alive: bool,
    pub focus_hint: FocusHint,   // discovery 预解析的终端元数据
}

pub enum Status {
    Working,            // 正在生成/执行工具
    WaitingForInput,    // 等用户输入（含"刚干完活"语义，无独立 Done）
    NeedsPermission,    // 待权限确认（PermissionChecker 预测）
    Shell,              // 交互式 shell
    Compacting,         // autocompact 进行中
}
```

### 6.2 状态机（判定规则）

**纯函数** `decide(raw_status, stale_secs, pending_permission, compacting) -> Status`，适合 TDD。

| 状态 | 判定 |
|---|---|
| Compacting | JSONL 出现 `type:system, subtype:compact_boundary`，或某条 `agentId` 以 `acompact-` 开头 |
| NeedsPermission | PermissionChecker 预测当前 tool_use 需要用户批准（最高通知优先级） |
| Working | `status=="busy"` / roster working / `claude agents --json` busy |
| Shell | `status=="shell"` |
| WaitingForInput | 其余（idle / 无挂起 tool_use / `status_updated_at` 停滞） |

> **为何没有 Done**：c9watch 实证 Claude 会话不真正结束，"干完活"即转入 `WaitingForInput`。"刚从 Working 转入 WaitingForInput"就是 done 的子语义，可用进入时刻区分，无需独立状态。

> **Compacting 抑制 replayed context**：autocompact 后 Claude 写入 "This session is being continued..." 的 user 消息，需用 compacting 标志位忽略它直到下一个 assistant 响应（借鉴 gmr）。

### 6.3 liveness（pid 存活校验）

- `kill(pid, 0) == 0` 后**再验证** `proc_pidpath` 含 `claude`/`node`，防 PID 回收误判。
- 死进程 → Session 标 `alive=false`，移入"最近结束"折叠区，不立即从列表删（便于归档语义）。

## 7. 模块划分（后端）

每个模块单一职责、可独立测试。`statemachine` / `permission` / `hidden` 是纯逻辑，TDD 优先。

| 模块 | 职责 | 依赖 |
|---|---|---|
| `collector` | 每 3s 采集所有数据源，产出原始记录 | 文件系统 / sysinfo / `claude` CLI |
| `reducer` | 按 sessionId 去重合并 → `Vec<Session>` | collector 输出 |
| `statemachine` | 纯函数：原始状态 → 统一 `Status` | 无（纯逻辑） |
| `permission` | PermissionChecker：读 settings 白名单预测 tool_use 是否需批准 | settings.json / JSONL 末尾 |
| `liveness` | pid 存活校验（kill+proc_pidpath 验证） | sysinfo / libc |
| `discovery` | focus 元数据预解析（env / 进程树 → FocusHint） | sysinfo / libc |
| `focus` | 分终端策略执行 focus（osascript / otty-cli / tmux） | discovery 输出 |
| `notify` | 状态迁移触发 macOS 通知（防抖 + per-session 静音） | statemachine 状态变化 |
| `hidden` | 隐藏列表读写（`~/.claude/cc-view/hidden.json`） | 文件系统（纯逻辑） |
| `ipc` | Tauri command/event 桥，`emit("sessions")` | Tauri runtime |

### PermissionChecker 细则

不等用户实际点"允许"，而在 tool_use 出现瞬间，读 `~/.claude/settings.json` 的 `permissions`（`allow`/`ask`/`deny`/`bash`）**预测**该 tool_use 是否要弹权限。命中 `ask` 或未匹配 → NeedsPermission。这比"等 status 变化"更早更准（借鉴 c9watch）。

## 8. 功能设计

### A · 不漏"等我"（通知）

- statemachine 检测到状态**转入** `NeedsPermission` / `WaitingForInput` 时，`notify` 发 macOS 通知。
- **防抖**：仅在状态迁移时触发，同一状态不重复弹。
- **per-session 静音**：hidden 模块维护静音集合。
- 点击通知 → 打开 popover 并定位到该会话；可顺带触发 C 的 focus。

### B · 总览全局

- popover 列表按状态分组（NeedsPermission 置顶）+ 按项目分组两种视图。
- 每行：状态图标 + 会话名 + 项目 + 宿主终端 hint + 停滞时长。
- tray 图标聚合：有 NeedsPermission 显眼色，否则按 Working/Idle 显示。

### C · 一键跳回（focus）

**核心架构（借鉴 gmr）**：focus **不是现场查 window**。`discovery` 在每轮采集时预解析每个会话的 `FocusHint` 并缓存，点击时 O(1) 取用。

**FocusHint**（从进程 env + 父进程链预解析）：
```rust
pub struct FocusHint {
    pub host: Host,                 // ITerm2 / Ghostty / VSCode / Idea / Terminal / Otty / Cmux / Tmux / Warp / Unknown
    pub iterm_session_id: Option<String>,   // env ITERM_SESSION_ID
    pub tmux_pane: Option<String>,          // env TMUX_PANE
    pub term_program: Option<String>,       // env TERM_PROGRAM
    pub cwd: String,
}
```

**进程 → 终端三层判定**（borrowed from gmr）：
1. `proc_pidpath` 特征路径（IDE 内嵌 binary）。
2. 进程 env（`TERMINAL_EMULATOR=JetBrains*` / `TERM_PROGRAM=Zed` / 等）。
3. 父进程链爬 8 层，看 `proc_pidpath` 含哪个 `.app`。
- tmux 是最大坑：server reparent 到 pid 1、祖先链断，靠漏进 session 的 env 反推外层终端（`LC_TERMINAL` / `ITERM_SESSION_ID` / `GHOSTTY_RESOURCES_DIR` / `KITTY_PID` / `WEZTERM_PANE` / `ALACRITTY_SOCKET`）。

**分终端 focus 策略 + 上界**（诚实标注能力上限）：

| 宿主 | 策略 | 精度 |
|---|---|---|
| iTerm2 | AppleScript 遍历 windows→tabs→sessions 匹配 `unique ID` | tab+window 精确 |
| Ghostty | AppleScript 按 working directory 匹配（含 symlink 归一） | terminal+window 精确 |
| tmux | `tmux select-window` + `select-pane`（+ zoom 检测） | pane 精确 |
| Otty | cwd → `otty-cli pane list --json` 过滤 → `open otty://pane/<pane_id>` | pane（按 cwd 匹配） |
| cmux | 读 session 快照建 cwd→pane 映射 → AppleScript `focus terminal` | terminal（按 cwd） |
| VS Code / IDEA / Terminal / Warp | 仅 `activate` 承载 app | app 级 |

**MVP 分层**：
- **Tier 0（全兼容保底）**：所有宿主先做 `activate 承载 app`——第一天 6 种全覆盖。
- **Tier 1（精细，优先）**：iTerm2、Ghostty、tmux、Otty、cmux 做到 tab/pane 级。
- **Tier 2（接受 app 级）**：VS Code、IDEA——不暴露终端 pane 的 scripting API，接受上限。

> **Otty 陷阱**：Otty 文档里的 "session id" 是它自己的 pane id（`p_xxx`），**不是 Claude UUID**——实测 `otty-cli pane show <claude-uuid>` exit 4。可靠路径是 cwd 匹配。要绝对精确可选"包一层 otty-hook.sh 额外报 `$OTTY_PANE_ID`"，但这破坏零侵入，列为可选增强。

### D · 历史/成本

会话详情里放"在 CCDash 看"入口（CCDash 已装在 `~/.claude/dashboard/`），或一键复制 sessionId。不自造。

### E · 隐藏 / 归档

- 本地文件 `~/.claude/cc-view/hidden.json`，存被隐藏的 sessionId。
- **只过滤 UI，不动任何官方数据，完全可逆**。
- 维度：MVP 按 **session** 隐藏；"按项目隐藏"作为增强。
- Done/Dead 会话自动折叠进"最近结束"区——隐式"不展示"，叠加手动隐藏。

## 9. 错误处理

| 风险 | 对策 |
|---|---|
| roster.json 残留死 worker（官方 bug） | 一切以 **pid 存活**为准，死的标 stale/不展示 |
| sessions 文件残留（进程已退） | 同上，liveness 兜底 |
| `claude agents --json` 失败/不存在 | 降级，仅用 sessions 文件 + 进程扫描 |
| 单源解析异常 | 隔离——skip + log，不拖垮整体（fail fast 但不吞异常） |
| roster.json 写一半被读到 | 解析失败跳过本轮，下轮重试（轮询天然兜底） |
| 通知权限被拒 | 降级：tray 图标变色 + popover 角标 |
| PID 回收 | `kill(pid,0)` 后验证 `proc_pidpath` 含 claude/node |

## 10. 测试策略

- **`statemachine` 单测**（TDD）：给定 raw_status + stale_secs + pending_permission + compacting → 断言 `Status`。纯函数。
- **`permission` 单测**（TDD）：给定 settings permissions + tool_use → 断言是否 NeedsPermission。纯函数。
- **`hidden` 单测**：隐藏/恢复/过滤逻辑。纯逻辑。
- **采集层 fixture 测试**：真实 `sessions/<pid>.json`、`roster.json`、JSONL 末尾片段做 fixture → 测解析 + 去重合并。
- **liveness**：mock sysinfo。
- **focus / notify**：AppleScript/通知难单测 → 手动冒烟脚本 + 真实起几个会话端到端验证。
- **端到端冒烟**：真实开 iTerm2/Otty/cmux 各跑一个 claude 会话，验证列表/通知/隐藏/focus。

## 11. MVP 范围与迭代

**MVP**（先解决最痛的）：
- 数据采集：sessions 文件 + `claude agents --json` + 进程扫描（roster 作补充）。
- 状态机：Working / WaitingForInput / NeedsPermission / Shell（含 PermissionChecker）。
- 总览 UI（B）+ 等我通知（A）+ 隐藏（E）。
- focus：Tier 0（全 app activate）+ iTerm2/Ghostty/tmux Tier 1 精细。

**迭代**：
- Compacting 状态（含 replayed context 抑制）。
- Otty/cmux Tier 1 精细 focus。
- roster 后台 fleet agent 完整覆盖。
- 桌面 Widget、手机端、项目级隐藏。

## 12. 合规

- **Otty**：闭源商业（appmakes 出品）。只调它的 `otty-cli` / AppleScript 接口，**不抄代码**。
- **cmux**：GPL-3.0（manaflow-ai/cmux）。走 AppleScript / 读 session 快照，**不抄代码**，避免 GPL 传染。
- **gmr/claude-status**：BSD-3。其 focus/状态思路可借鉴，引用时遵守条款。
- **c9watch**：MIT。可借鉴架构与代码片段。

## 13. 参考竞品报告

- [`docs/research/c9watch.md`](../../research/c9watch.md) — 同栈，状态判定/NSPanel/纯轮询
- [`docs/research/claude-status.md`](../../research/claude-status.md) — focus 三层判定/Compacting/Darwin notification
- [`docs/research/otty.md`](../../research/otty.md) — Otty pane 控制/session-id 陷阱
- [`docs/research/cmux.md`](../../research/cmux.md) — cmux AppleScript/session 快照/socket 不可外部用

## 14. 待实现时验证的开放问题

1. `sessions/<pid>.json` 的 `status` 字段全集（实测仅 busy/shell，运行时补全 WaitingForInput 等取值）。
2. `claude agents --json` 的确切输出 schema。
3. roster.json worker 的状态字段名/值（样例未见显式 status，可能靠 socket 或 daemon.status.json）。
4. 同一 sessionId 是否同时出现在 sessions 文件和 roster（去重验证）。
5. Otty `pane list --json` 字段、cmux session 快照确切格式（报告已探，实现时核实）。
