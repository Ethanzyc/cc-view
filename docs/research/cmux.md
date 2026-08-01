# cmux 深度调研报告

> 调研日期: 2026-08-01 | 实测版本: cmux 0.64.16 (commit 5321becb6)

## 1. 概述

**cmux 是什么**: macOS 原生终端，专为并行运行多个 AI coding agent（Claude Code / Codex / Grok / OpenCode 等）设计。基于 libghostty 渲染，Swift + AppKit 构建（非 Electron），支持垂直标签、分屏、内置浏览器、SSH 工作区、智能通知。

- **开源**: 是，GPL-3.0-or-later 许可证
- **GitHub**: [manaflow-ai/cmux](https://github.com/manaflow-ai/cmux) | 25,455 stars | Swift | 3,925 open issues
- **公司**: Manaflow, Inc. (founders@manaflow.com)
- **Bundle ID**: `com.cmuxterm.app`
- **基于 Ghostty**: 使用 libghostty 做终端渲染（Ghostty 本身是 MIT 许可），但 cmux 是独立应用，不依赖外部 Ghostty 安装。内置 `ghostty` 可执行文件在 `Resources/bin/`
- **核心第三方依赖**: libghostty (MIT)、Bonsplit 分屏库 (MIT)、Sparkle 更新框架、Sentry 崩溃报告、PostHog 分析
- **URL scheme**: `cmux://`（用于 auth 回调）
- **最低系统**: macOS 14.0

### 架构概念（tmux-like 模型）

```
Application
  └─ Window (macOS 窗口)
       └─ Workspace / Tab (标签页，类似 tmux window)
            └─ Pane (分屏容器)
                 └─ Surface (面板：terminal / browser / agent-session)
```

- **Window**: 顶层 macOS 窗口
- **Workspace**: 窗口内的标签页，类似 tmux window
- **Pane**: workspace 内的分屏区域
- **Surface**: pane 内的标签面（终端、浏览器、或 agent-session UI）

---

## 2. Pane 控制 / 聚焦接口（最关键）

cmux 提供两条外部控制路径：**AppleScript** 和 **CLI + Unix Socket**。

### 2.1 AppleScript / OSA Scripting（推荐 cc-view 使用）

**关键优势：从任何进程都能调用，不需要 socket 连接权限。**

cmux 内置完整的 AppleScript Scripting Definition（`/Applications/cmux.app/Contents/Resources/cmux.sdef`），暴露的对象层级和命令：

**对象层级**:
| 类 | 属性 | 说明 |
|---|---|---|
| `application` | `name`, `frontmost`, `front window`, `version` | cmux 应用本身 |
| `window` | `id` (UUID), `name`, `selected tab` | macOS 窗口 |
| `tab` | `id` (UUID), `name`, `index`, `selected`, `focused terminal` | workspace/标签页 |
| `terminal` | `id` (UUID), `name`, `working directory` | 终端面板 |

**命令**:
| 命令 | 作用 |
|---|---|
| `focus` (terminal) | **聚焦某 terminal，同时将其窗口置前** |
| `activate window` | 将 window 置前 |
| `select tab` | 选中某 workspace |
| `split` (direction: left/right/up/down) | 分屏 |
| `close` (terminal/tab/window) | 关闭 |
| `input text` (to terminal) | 向终端粘贴文本 |
| `perform action` (Ghostty action string, on terminal) | 执行 Ghostty 动作 |
| `new window` / `new tab` | 创建 |

**实测验证（成功）**:

```applescript
-- 列出所有窗口/标签/终端
tell application "cmux"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with term in terminals of t
        -- term 有 id、name、working directory
      end repeat
    end repeat
  end repeat
end tell

-- 按 cwd 定位并聚焦 terminal（实测成功）
tell application "cmux"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with term in terminals of t
        if (working directory of term) contains "cc-view" then
          focus term          -- 聚焦 pane
          select tab t        -- 切换到该 workspace
          activate window w   -- 窗口置前
        end if
      end repeat
    end repeat
  end repeat
end tell
```

调用方式（从 cc-view 的 Swift 代码）:
```swift
// 使用 NSAppleScript 或 NSUserAppleScriptTask
// 或直接 subprocess 调用 osascript
let script = """
tell application "cmux"
    repeat with w in windows
        ...
    end repeat
end tell
"""
// Process: /usr/bin/osascript -e <script>
```

### 2.2 CLI + Unix Socket（功能更全，但有访问限制）

cmux CLI 位于 `/Applications/cmux.app/Contents/Resources/bin/cmux`（55MB Go 二进制）。

**Socket 路径**: `~/.local/state/cmux/cmux-502.sock`（502 = 当前用户 UID）

**访问限制（关键限制）**: socket 拒绝非 cmux 内部启动的进程连接：

```
ERROR: Access denied — only processes started inside cmux can connect
```

这通过进程审计实现（cmux 追踪它 spawn 的进程树）。`--password` / `CMUX_SOCKET_PASSWORD` 可能绕过限制（文档提到三种认证方式，但未测试密码是否 bypass 进程检查）。

**Socket 发现**: `~/.local/state/cmux/last-socket-path` 文件记录最后使用的 socket 路径。

**CLI 关键命令**:
```bash
cmux tree --all                          # 打印完整窗口/工作区/面板树
cmux list-panes [--workspace <id>]       # 列出面板
cmux list-pane-surfaces --pane <id>      # 列出面
cmux list-windows / list-workspaces      # 列出窗口/工作区
cmux top --all                           # 进程/资源使用
cmux focus-pane --pane <id|ref|index>    # 聚焦面板
cmux identify --json                     # 当前调用者上下文
cmux events [--after <seq>]              # 事件流 (NDJSON)
cmux rpc <method> [json-params]          # 原始 RPC 调用
cmux notify --title <t> --body <b>       # 发送通知
cmux read-screen --surface <id>          # 读取终端文本
cmux send --surface <id> "text"          # 发送文本
cmux set-status <key> <value>            # 设置侧栏状态
```

**Handle 模型**: 支持 UUID、短引用（`window:1/workspace:2/pane:3/surface:4`）、索引三种格式。输出默认短引用，可用 `--id-format uuids|both` 切换。

**tmux 兼容层**: CLI 内置 `__tmux-compat` 调度器，支持 `capture-pane`、`resize-pane`、`swap-pane`、`break-pane`、`join-pane` 等 tmux 命令。

### 2.3 两种接口对比

| 维度 | AppleScript | CLI + Socket |
|---|---|---|
| 外部进程可用 | **是** | **否**（仅 cmux 内部进程） |
| 聚焦 pane | `focus term` | `focus-pane --pane <id>` |
| 列出 pane | 遍历 windows/tabs/terminals | `list-panes` / `tree` |
| 读取终端内容 | 不支持 | `read-screen` |
| 发送输入 | `input text` | `send` / `send-key` |
| 事件流 | 不支持 | `events` (NDJSON) |
| RPC | 不支持 | `rpc <method>` |
| 通知 | 不支持 | `notify` / `set-status` |
| 进程信息 | 不支持 | `top`（CPU/内存） |
| 密码绕过 | 不需要 | 可能有 `--password` |

---

## 3. 按 session / pid / cwd / agent 定位 pane 的方案（cc-view focus 落地）

### 方案 A: AppleScript + cwd 匹配（推荐，已验证可行）

cc-view 已知 Claude Code 会话的工作目录（从 Claude Code 的 session 文件获取），用 AppleScript 匹配：

```applescript
tell application "cmux"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with term in terminals of t
        if (working directory of term) is "/Users/zhuyuchen/some/project" then
          focus term
          select tab t
          activate window w
          exit repeat
        end if
      end repeat
    end repeat
  end repeat
end tell
```

**限制**: 如果多个 pane 的 cwd 相同，只能聚焦第一个匹配的。需要额外信息（如 PID）消歧时不适用。

### 方案 B: AppleScript + UUID 匹配

如果 cc-view 能预先获取 cmux 中某 pane 的 UUID（通过 session JSON 或其他方式），可直接按 id 定位：

```applescript
tell application "cmux"
  -- terminal 的 id 属性是稳定的 UUID
  repeat with term in terminals
    if (id of term) is "E72AE953-DD71-4645-A35E-F260C39044DD" then
      focus term
    end if
  end repeat
end tell
```

### 方案 C: 读取 cmux session JSON + AppleScript 聚焦

cmux 在 `~/Library/Application Support/cmux/session-com.cmuxterm.app.json` 保存了完整的窗口/工作区/面板树，包含每个 terminal 的 UUID、cwd、tty 名称。

cc-view 可以:
1. 读取 session JSON 获取 pane → UUID/cwd/tty 映射
2. 用 tty 名称（如 `ttys012`）或 cwd 匹配目标 Claude 会话
3. 用 AppleScript `focus` 聚焦

### 方案 D: 如果能获取 socket 密码

如果用户在 cmux Settings 里设置了 socket 密码，或 cc-view 能获取 `CMUX_SOCKET_PASSWORD`，则可直接用 CLI：
```bash
cmux --password <pwd> --socket ~/.local/state/cmux/cmux-502.sock focus-pane --pane <uuid>
```

**可行性待验证**：密码是否绕过进程树检查未确认。

### 推荐落地路径

**cc-view 的 cmux focus 实现应该:**

1. **检测 cmux 是否运行** — 检查 `~/.local/state/cmux/cmux-502.sock` 是否存在，或 `NSWorkspace` 查 `com.cmuxterm.app` 进程
2. **用 AppleScript 实现聚焦** — 按 cwd 或 UUID 匹配 terminal 并 focus
3. **辅助: 读取 session JSON** — 从 `~/Library/Application Support/cmux/session-com.cmuxterm.app.json` 构建 pane 索引（UUID → cwd/tty/title）
4. **回退: `osascript` subprocess** — 如果 NSAppleScript 有沙盒限制，用 `/usr/bin/osascript` subprocess

---

## 4. Socket API（路径 / 协议 / 能力）

### Socket 路径

- **主 socket**: `~/.local/state/cmux/cmux-502.sock`（UID 后缀，多用户安全）
- **旧版 socket**: `~/.local/state/cmux/cmux.sock`（向后兼容）
- **Lock 文件**: `~/.local/state/cmux/cmux-502.sock.lock`（内容: `cmux-socket-lock-v1`）
- **发现文件**: `~/.local/state/cmux/last-socket-path`

### 协议

- **CLI ↔ cmux**: JSON-RPC v2 风格，行分隔 JSON（从二进制分析可见 `{"ok":true,"id":"..."}` 模式）
- **Shell integration ↔ cmux**: 纯文本命令（如 `report_pr 123 https://... --tab=... --panel=...`），通过 zsocket / ncat / socat / nc 发送
- **事件流**: NDJSON（newline-delimited JSON），每行一个事件对象
- **Socket 权限**: `srw-------`（仅 owner），加上进程审计双重保护

### 认证

1. `--password <value>` — CLI 参数，最高优先级
2. `CMUX_SOCKET_PASSWORD` — 环境变量
3. Settings 中保存的密码 — 最低优先级

### 能力

| 能力 | CLI 命令 | 说明 |
|---|---|---|
| 列出拓扑 | `tree`, `list-windows`, `list-workspaces`, `list-panes`, `list-pane-surfaces` | 完整 pane/surface 树 |
| 聚焦 | `focus-pane`, `focus-window`, `select-workspace` | |
| 创建 | `new-window`, `new-workspace`, `new-split`, `new-pane`, `new-surface` | |
| 移动/重排 | `move-surface`, `move-workspace-to-window`, `reorder-workspace(s)`, `split-off` | |
| 读取终端 | `read-screen` | 读取 surface 文本 |
| 写入终端 | `send`, `send-key`, `send-panel`, `send-key-panel` | |
| 通知 | `notify`, `list-notifications`, `open-notification`, `jump-to-unread` | |
| 状态 | `set-status`, `clear-status`, `set-progress`, `log` | 侧栏状态/进度/日志 |
| 事件流 | `events --after <seq> --name <event>` | 实时 NDJSON |
| 进程监控 | `top --all --processes` | CPU/内存/进程 |
| 原始 RPC | `rpc <method> [json-params]` | 直接调用 v2 socket 方法 |
| 标识 | `identify --json` | 当前调用者的 workspace/surface 上下文 |
| 能力声明 | `capabilities` | 服务器能力 JSON |

---

## 5. cmux 的 Agent 状态模型（外部可读性）

### Claude Code 集成机制

cmux 通过 **wrapper 脚本**拦截 `claude` 命令（`/Applications/cmux.app/Contents/Resources/bin/cmux-claude-wrapper`）：

1. 检测 `CMUX_SURFACE_ID` 环境变量（cmux 内部终端会设置）
2. 如果在 cmux 内，找到真正的 `claude` 二进制
3. 注入 `--session-id <UUID>` 和 `--settings <hooks_json>` 参数
4. hooks JSON 包含事件回调:
   - **SessionStart** → `cmux hooks claude session-start`
   - **Stop** → `cmux hooks claude stop` + `cmux hooks feed --source claude` + `cmux hooks claude auto-name`
   - **SubagentStop** → `cmux hooks feed --source claude`
   - **SessionEnd** → `cmux hooks claude session-end`
   - **Notification** → `cmux hooks claude notification`（PushNotification tool）
   - **UserPromptSubmit** → `cmux hooks claude prompt-submit`
   - **PreToolUse** (CronCreate guard + general) → `cmux hooks claude pre-tool-use`
   - **PermissionRequest** → `cmux hooks feed --source claude`（阻塞式，125s 超时）

### Session 数据存储

| 文件 | 内容 | 格式 |
|---|---|---|
| `~/.cmuxterm/claude-hook-sessions.json` | Claude 会话索引（surface → session ID, PID, cwd, lifecycle） | JSON |
| `~/.cmuxterm/events.jsonl` | 全局事件流 | NDJSON |
| `~/.cmuxterm/workstream.jsonl` | Feed 工作流事件 | NDJSON |

### Agent 生命周期状态

cmux 跟踪四种状态: `running`、`idle`、`needsInput`、`unknown`

这些状态由 hooks 驱动:
- SessionStart → `running`
- Stop → `idle`
- PermissionRequest / Notification → `needsInput`

### Agent Hibernation（休眠机制）

cmux 支持 agent 休眠：空闲的 background agent 进程被 SIGTERM 终止释放内存，用户切回时用保存的 session ID 自动 resume。配置:

```json
{
  "terminal": {
    "agentHibernation": {
      "enabled": false,  // 默认关闭
      "idleSeconds": 5,
      "maxLiveTerminals": 12
    }
  }
}
```

### 外部可读性

- **Session JSON**: `~/.cmuxterm/claude-hook-sessions.json` — **可直接读**，但当前为空（当前用户没有活跃 Claude 会话在此 cmux 中）。结构:
  ```json
  {
    "activeSessionsBySurface": {},
    "activeSessionsByWorkspace": {},
    "sessions": {},
    "version": 1
  }
  ```
- **Events JSONL**: `~/.cmuxterm/events.jsonl` — **可直接读**，NDJSON 格式，包含窗口创建、面板聚焦等事件。事件结构:
  ```json
  {
    "boot_id": "UUID",
    "category": "window|workspace|terminal|...",
    "id": "event-UUID",
    "name": "window.created|window.keyed|...",
    "occurred_at": "ISO8601",
    "payload": { ... }
  }
  ```
- **Session manifest**: `~/Library/Application Support/cmux/session-com.cmuxterm.app.json` — **可直接读**，包含完整窗口/工作区/面板树
- **Socket events**: `cmux events` 命令提供实时事件流（但需要 socket 访问权限）

---

## 6. 对 cc-view 的可借鉴点

### 6.1 Focus cmux pane（核心需求）

**结论: 使用 AppleScript，不需要 socket 权限。**

```swift
// cc-view 的 cmux focus 实现
func focusCmuxPane(cwd: String) {
    let script = """
    tell application "cmux"
        repeat with w in windows
            repeat with t in tabs of w
                repeat with term in terminals of t
                    if (working directory of term) is "\(cwd)" then
                        focus term
                        select tab t
                        activate window w
                        return "ok"
                    end if
                end repeat
            end repeat
        end repeat
        return "not found"
    end tell
    """
    // 执行: /usr/bin/osascript -e <script>
}
```

**消歧策略**: 同一 cwd 多 pane 时，可结合 session JSON 的 `ttyName` 字段或 terminal UUID 来定位。

### 6.2 Socket 能否复用

**结论: 不能直接复用。** Socket 限制为 cmux 内部进程。可选方案:
- 让用户在 cmux 设置中配置 socket 密码，cc-view 用密码认证（未验证是否绕过进程检查）
- 在 cmux 终端内安装一个 helper agent，通过 socket 转发 cc-view 的请求

### 6.3 cmux 状态补充 cc-view

**可以直接读取的文件**:
1. `~/Library/Application Support/cmux/session-com.cmuxterm.app.json` — pane 拓扑（UUID → cwd/tty/title）
2. `~/.cmuxterm/claude-hook-sessions.json` — Claude 会话 → cmux surface 映射
3. `~/.cmuxterm/events.jsonl` — 实时事件流（文件 tail，无需 socket）
4. `~/Library/Logs/cmux-focus.log` — focus 日志（当前为空）

cc-view 可以:
- **监控 events.jsonl** — 获取面板创建/聚焦/通知事件，补充 cc-view 的状态感知
- **读取 session JSON** — 建立 cmux pane 索引，关联 Claude Code 会话
- **读取 claude-hook-sessions.json** — 知道哪些 cmux pane 正在跑 Claude，以及 PID/session ID

### 6.4 cmux 的设计模式可借鉴

- **通知环 (Notification rings)**: pane 有未读通知时显示蓝色环 — cc-view 可以在 menubar 用类似视觉提示
- **Feed 审批系统**: PermissionRequest 作为阻塞 hook，在侧栏显示审批卡片 — cc-view 可参考这种非侵入式审批
- **Agent Hibernation**: 自动休眠空闲 agent，cc-view 可参考做资源管理
- **Workspace 自动命名**: turn-end 时用 AI 给 workspace 起名

---

## 7. 踩坑

1. **`/Applications/cmux.app/Contents/MacOS/cmux --help` 会启动 GUI**：这不是纯 CLI，而是 GUI app binary。`--help` 会打开 cmux 窗口。真正的 CLI 是 `Contents/Resources/bin/cmux`（Go 编译的独立二进制）。

2. **Socket 访问限制比预期严格**: 即使是同一用户的进程，只要不是 cmux spawn 的就被拒绝。`~/.local/state/cmux/cmux-502.sock` 虽然权限是 owner-rw，但有应用层进程审计。

3. **Session JSON 可能不实时**: `session-com.cmuxterm.app.json` 的 `createdAt` 时间戳显示它可能是定期快照而非实时。Focus 操作后 JSON 可能不会立即更新。

4. **`claude-hook-sessions.json` 可能为空**: 如果用户没有在 cmux 中通过 wrapper 启动 Claude Code（比如直接在 cmux 终端里运行系统 PATH 中的 claude 而非 wrapper），session 追踪不会生效。

5. **AppleScript `working directory` 匹配**: 需要精确路径匹配或 contains 匹配。如果 cmux terminal 的 cwd 与 Claude Code session 的 project 路径不一致（比如 Claude 在子目录运行），contains 更鲁棒。

6. **多窗口场景**: AppleScript 需要遍历所有窗口的所有标签的所有终端。面板数量大时可能有性能开销，但实测正常使用场景下可以接受。

7. **GPL-3.0 许可证传染性**: 如果 cc-view 要直接引用 cmux 的代码或二进制，需注意 GPL 传染性。但通过 AppleScript 交互不构成链接，不受 GPL 约束。

---

## 8. App Bundle 关键文件

```
/Applications/cmux.app/Contents/
├── Info.plist                           # Bundle ID: com.cmuxterm.app, URL scheme: cmux://
├── MacOS/cmux                           # GUI app binary (233MB, Swift)
├── Resources/
│   ├── cmux.sdef                        # ★ AppleScript Scripting Dictionary
│   ├── bin/
│   │   ├── cmux                         # ★ CLI 二进制 (55MB, Go) — 主要控制接口
│   │   ├── cmux-claude-wrapper          # Claude Code 拦截 wrapper (bash)
│   │   ├── ghostty                      # Ghostty 终端引擎 (12MB)
│   │   ├── grok                         # Grok wrapper (bash)
│   │   └── open                         # URL→浏览器路由 wrapper (bash)
│   ├── shell-integration/
│   │   ├── cmux-zsh-integration.zsh     # ★ zsh 集成 (63KB) — socket 通信、通知、PR 检测
│   │   ├── cmux-bash-integration.bash   # bash 集成
│   │   ├── ghostty-integration.zsh      # Ghostty shell 集成
│   │   └── fish/config.fish             # fish 集成
│   ├── agent-session-react/             # agent session UI (React)
│   ├── agent-session-solid/             # agent session UI (SolidJS)
│   ├── feed-tui/index.ts                # Feed TUI
│   ├── opencode-plugin.js               # OpenCode 插件
│   ├── markdown-viewer/                 # Markdown 渲染器
│   ├── ghostty/                         # Ghostty 配置/terminfo
│   └── terminfo/                        # 终端信息
├── Frameworks/
│   ├── Sentry.framework                 # 崩溃报告
│   └── Sparkle.framework                # 自动更新
└── PlugIns/
    └── CmuxDockTilePlugin.plugin        # Dock 图标插件
```

### 配置与数据位置

| 路径 | 内容 |
|---|---|
| `~/.config/cmux/cmux.json` | cmux 主配置 (JSONC) |
| `~/.config/ghostty/config` | Ghostty 终端配置（字体/主题/透明度等） |
| `~/Library/Application Support/cmux/session-com.cmuxterm.app.json` | ★ Session 快照（完整拓扑） |
| `~/Library/Application Support/cmux/closed-item-history-*.json` | 关闭历史 |
| `~/Library/Application Support/cmux/search.db` | 搜索索引 (SQLite) |
| `~/.local/state/cmux/cmux-502.sock` | ★ Unix socket |
| `~/.local/state/cmux/last-socket-path` | Socket 路径发现文件 |
| `~/.cmuxterm/claude-hook-sessions.json` | ★ Claude 会话索引 |
| `~/.cmuxterm/events.jsonl` | ★ 全局事件流 (NDJSON) |
| `~/.cmuxterm/workstream.jsonl` | Feed 工作流事件 |
| `~/Library/Preferences/com.cmuxterm.app.plist` | 用户偏好 |
| `~/Library/Logs/cmux-update.log` | 更新日志 |
| `~/Library/Logs/cmux-focus.log` | Focus 日志 |
