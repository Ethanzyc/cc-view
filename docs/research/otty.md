# Otty 调研报告

> 调研日期 2026-08-01。所有命令输出均为本机实测（Otty 1.2.3，app bundle `/Applications/Otty.app`）。命令里的 `otty-cli` 实际路径是 `/Applications/Otty.app/Contents/MacOS/otty-cli`（它默认不在 PATH，需在 Otty 里 "Install CLI"，或直接用全路径，详见「踩坑」）。

## TL;DR（给 cc-view focus 的明确结论）

**cc-view 想一键 focus 到跑某个 Claude 会话的 Otty pane，最可靠落地路径（零侵入）**：

1. 拿 Claude `session-id`（cc-view 本来就有，或从 `~/.claude/projects/` 反查）。
2. session-id → cwd：扫 `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`，反解父目录名（如 `-Users-zhuyuchen-ai-cc-job` → `/Users/zhuyuchen/ai/cc-job`）。
3. cwd → pane id：`otty-cli pane list --json` 按 `cwd` 字段过滤，拿 `id`（`p_xxx`）/`tab_id` / `window_id`。多个同 cwd 时用 `process`（含 Claude 任务标题）/ `title` 二次消歧。
4. focus 三连（顺序重要，先 window 再 tab 再 pane）：
   ```
   otty-cli window focus <window_id>
   otty-cli tab focus <tab_id>
   otty-cli pane focus <pane_id>
   ```
   或更简洁的一步式（推荐，Otty 没跑时会先启动它）：
   ```
   open "otty://pane/<pane_id>"
   ```

**不能用 Claude session-id UUID 直接定位**：实测 `otty-cli pane show <claude-uuid>` 报 `No pane matched selector`（exit 4）。Otty 文档里 URL scheme 说的 "session id" 指 **Otty 自己的 pane session id**（`p_xxx` 后缀那种），不是 Claude UUID——措辞陷阱。

**更精确的增强方案**：包装 `otty-hook.sh`，让它在每次报 state 时把 `$OTTY_PANE_ID`（Otty 注入到每个 pane 的环境变量）连同 session-id 一起发给 cc-view，直接建立 `session-id → pane_id` 精确映射，绕开 cwd 歧义。

---

## 1. 概述

### Otty 是什么
- **定位**：「Otty — a native, beautiful terminal app」，原生 macOS 终端。bundle id `io.appmakes.otty`，v1.2.3（Info.plist `OttyGitHash=dc80e013`）。
- **开发者**：appmakes.io（同时做 Typora、Paletro；more-from-us.json 列了这三款）。Typora 是知名付费 Markdown 编辑器，说明 appmakes 是成熟商业开发者。
- **技术栈**：Swift（GUI）+ Rust（终端核心）。CREDITS.md 显示用 `alacritty_terminal` / `vte` / `portable-pty`（来自 wezterm）做终端模拟，`clap` 做 CLI，`rusqlite` 存状态。SPM 依赖含 CodeEditTextView、swift-markdown。
- **官网 / 文档**：`otty.sh`（产品站）、`docs.otty.sh`（VitePress 文档站，含 `/reference/cli`、`/reference/applescript`、`/reference/url-scheme`、`/agents/*`、`/license`、`/pricing`）。

### 开源情况：闭源商业
- `Info.plist` 写明 `Copyright © 2026 appmakes. All rights reserved.`
- GitHub `appmakes/otty` 仓库 **Not Found**（API 实测）；搜索 "otty terminal emulator" 无对应官方开源仓库。
- 有 `/pricing` 页（闭源商业软件的典型特征）。
- CREDITS.md 只列第三方依赖，不列 Otty 自身源码。
- **结论：Otty 是闭源商业软件，本调研只能靠 binary + 文档 + 实测行为，看不到源码。**

### 和 Ghostty / cmux / Dinotty 的关系
- **Ghostty**：Otty 的直接灵感来源（CREDITS "Direct Inspirations" 明列）。借鉴了 Ghostty 的 **配置格式**（`config.toml`、key 命名）、bundled 了 `xterm-ghostty` terminfo、settings UI 灵感受 `ghostty-config`（zerebos）启发。**不是 fork，是独立实现**（Otty 用 Swift+Rust，Ghostty 是 Zig）。
- **cmux**：用户环境里另一个独立终端/复用器产品，和 Otty 无技术关联。Otty 有自己的 IPC/socket 体系，不复用 cmux。
- **Dinotty**（`/Applications/Dinotty.app`）：**完全无关**。bundle id `com.dinotty.terminal`（不同公司），v0.13.4，可执行文件名 `dinotty-desktop`，Resources 只有 icon（Electron/Tauri 风格单二进制）。只是恰好同目录。不要假设两者同源。

---

## 2. window / tab / pane 控制接口（实测 help + 用法）

三个命令都 **requires running app**（通过 IPC socket 和 GUI 通信，Otty 没跑时不可用）。通用全局 flag：`--format text|json`、`--json`、`--no-headers`、`--socket <path>`、`--timeout <ms>`（默认 3000）、`-q`、`-y`。

### 2.1 window 子命令

```
Commands:
  show    Show one window
  list    List all windows
  new     Create a new window
  close   Close a window
  focus   Focus a window
  rename  Set a window title
```

- `window list`：无参数。`window list --json` 返回字段：`focused` / `id`（`w_<UUID>`）/ `index` / `pane_count` / `tab_count` / `title`。
- `window show|focus|close` 接受 `[WINDOW]` 位置参数或 `--window <sel>`（"Window id or selector"）。
- `window new`：flags `--cwd` / `--command` / `--title` / `--no-focus`。
- `window focus --help` 实测：
  ```
  Usage: otty-cli window focus [OPTIONS] [WINDOW]
  Arguments:
    [WINDOW]  Window id or selector (positional shorthand for --window)
  ```

实测 `window list --json`（用户当前 1 个 window、13 个 tab 全在跑 Claude）：
```json
{
  "command": "window list",
  "data": [{
    "focused": true,
    "id": "w_E224FDBD-60EC-4B77-ACD7-DC2150FE964F",
    "index": 0,
    "pane_count": 13,
    "tab_count": 13,
    "title": "✳ 调查 GitHub 访问失败原因"
  }],
  "ok": true
}
```

### 2.2 tab 子命令

```
Commands:
  show    Show one tab
  list    List tabs
  new     Create a new tab
  close   Close a tab
  focus   Focus a tab
  rename  Rename a tab
  move    Move a tab
  badge   Set or clear the tab's status badge
  help    ...
```

- `tab list` 支持 `--window <sel>` 过滤。字段：`active` / `cwd` / `id`（`t_<hex>_<hex>`）/ `index` / `pane_count` / `process` / `title` / `window_id`。
- `tab badge --kind <KIND>` 的 kind 枚举（**正好对应 Claude 状态，Otty 已经在做 cc-view 想做的 badge**）：
  - `running` — spinning activity（处理中）
  - `completed` — transient success checkmark（完成）
  - `finished` / `unread` — accent-colored unread dot（命令结束/有未读输出，两者渲染相同）
  - `error` — error triangle
  - `awaiting-input` — hand icon（等待输入）
  - `--clear` 清除
- `tab move` flags：`--to-window <sel>` / `--index <n>`。
- `tab new` flags：`--cwd` / `--after <cwd>` / `--title`。

实测 `tab list --json`（节选）：
```json
{
  "active": false,
  "cwd": "/Users/zhuyuchen/ai/cc-job",
  "id": "t_19f91c4f8e8_6",
  "index": 0,
  "pane_count": 1,
  "process": "✳ Implement schedule import/export functionality",
  "title": "✳ Implement schedule import/export functionality",
  "window_id": "w_E224FDBD-60EC-4B77-ACD7-DC2150FE964F"
}
```

注意 `process` 字段对 Claude 会话显示的是 **Claude 当前任务标题**（带 `✳` 前缀，由 Otty 的 agent integration 注入），不是 `claude` 进程名。

### 2.3 pane 子命令（最丰富）

```
Commands:
  show       Show one pane
  list       List panes
  split      Split a pane
  close      Close a pane
  focus      Focus a pane
  zoom       Toggle zoom on a pane
  resize     Resize a pane
  send-keys  Send keys to a pane
  capture    Capture pane text
  badge      Set or clear the badge on the pane's tab
  help       ...
```

- `pane list` flags：`--window <sel>` / `--tab <sel>` 过滤。字段：`active` / `cols` / `cwd` / `id`（`p_<hex>_<hex>`，和 tab id 共享后缀）/ `index` / `process` / `rows` / `tab_id` / `window_id`。
- `pane split` flags：`--direction left|right|top|bottom`、`--pane <anchor>`。
- `pane focus` 额外有 `--direction <DIRECTION>`（按方向切焦点）。
- `pane send-keys [PARTS]...`：文本 + `key:Enter` 混合，flag `--bracketed-paste`。例：`otty pane send-keys --pane 3 -- "echo hi" key:Enter`。
- `pane capture [PANE]`：flags `--scope` / `--lines <n>` / `--ansi` / `--trim`。
- `pane resize` flags：`--up` / `--down` / `--left` / `--right`（文档列）或 `--direction`。

### 2.4 selector 语法（关键，且文档和实测有出入）

`--window` / `--tab` / `--pane` 接受的 selector 形式（综合 `reference/cli` + `reference/url-scheme` 文档 + 实测）：

| selector 形式 | window | tab | pane | 说明 |
|---|---|---|---|---|
| `w_<UUID>` / `t_<hex>_<hex>` / `p_<hex>_<hex>` 完整 id | ✓ | ✓ | ✓ | 最可靠 |
| 裸 id（去前缀，如 `19f91c4f8e8_6`） | ? | ✓（实测） | ✓（实测） | Otty 会做前缀匹配 |
| 1-based index | ✓ | ✓（文档） | ✗（实测 `pane show 0` 报错） | pane index 不带 tab 上下文无意义 |
| `title:<pattern>` | ✓（文档 `otty://window/title:<pattern>`） | ? | ? | 按 title 模糊匹配 |
| `current` / `last`（仅 window） | ✓（url-scheme 文档） | — | — | — |
| **Claude session-id UUID** | ✗ | ✗ | ✗ | **实测不识别，exit 4** |

> **注意 URL scheme 文档原文**：`otty://pane/<sel>` 接受 "p_<id>, **or a session id such as $OTTY_PANE_ID**"。这里的 "session id" 指 **Otty pane 自己的 session id**（`p_` 后缀那段，即 `$OTTY_PANE_ID` 的值），**不是 Claude 的 UUID**。这是个措辞陷阱，别被误导。

---

## 3. 按 session-id / pid / cwd 定位 pane 的可行方案（cc-view focus 落地）

核心难点：**Otty 的 selector 和 list/show 输出都不暴露 Claude session-id**，agent state 查询（见第 4 节）也不返回 pane id。Otty 内部一定存了 session-id→pane 映射（tab badge 能正确反映 Claude 状态），但没把它暴露给外部。所以需要桥接。

### 方案 A（推荐，零侵入）：session-id → cwd → pane list 匹配

**链路**：
1. **session-id → cwd**：Claude 的 session 文件存在 `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`，目录名把 cwd 的 `/` 换成 `-`（如 `-Users-zhuyuchen-ai-cc-job` → `/Users/zhuyuchen/ai/cc-job`）。cc-view 拿到 session-id 后，遍历 projects 目录找 `<session-id>.jsonl`，反解父目录名即得 cwd。实测第一行 jsonl 含 `{"type":"ai-title","aiTitle":"...","sessionId":"..."}`，可用于二次确认 / 拿任务标题。
2. **cwd → pane**：`otty-cli pane list --json`，按 `cwd` 字段过滤，拿 `id` + `tab_id` + `window_id`。
3. **focus**（顺序：window → tab → pane）：
   ```bash
   /Applications/Otty.app/Contents/MacOS/otty-cli window focus <window_id>
   /Applications/Otty.app/Contents/MacOS/otty-cli tab focus <tab_id>
   /Applications/Otty.app/Contents/MacOS/otty-cli pane focus <pane_id>
   ```
   或一步（推荐）：
   ```bash
   open "otty://pane/<pane_id>"     # Otty 没跑会先启动它；只移动焦点，不执行命令
   ```

**歧义风险与消歧**：多个 tab 同 cwd（实测用户有 6 个 tab 都在 `/Users/zhuyuchen/code/zto/voucher-engine`）时，单靠 cwd 分不清。消歧手段：
- `tab.process` / `tab.title` 字段含 Claude 当前任务标题（带 `✳` 前缀），可用 Claude session 的 `aiTitle`（jsonl 第一行）做二次匹配。
- Claude 进程 pid：`ps` 拿 claude pid → `lsof -p <pid>` 拿 cwd → 再走方案 A（但 Otty pane list 没暴露 pane 内进程的 pid，只能靠 cwd 间接对应）。
- 最稳的还是方案 B。

### 方案 B（精确，需轻改 hook）：包装 otty-hook.sh 注入 pane_id

Otty 给每个 pane 的 shell 导出 `$OTTY_PANE_ID` 环境变量（文档明确：「Otty exports into every pane」）。但 Claude 的 hook 脚本目前只报 session-id + state，没报 pane_id。

**做法**：cc-view 在用户的 Claude hook 配置里插一层 wrapper，在原 `otty-hook.sh` 调用基础上，额外把 `$OTTY_PANE_ID` + `CLAUDE_SESSION_ID` 报给 cc-view（比如写一行到 cc-view 的状态文件 / IPC）。这样 cc-view 就有精确的 `session-id → pane_id` 映射，focus 时直接 `open otty://pane/<OTTY_PANE_ID>`，零歧义、零 cwd 反查。

**注意**：用户的 otty-hook.sh 是 Otty 在 Settings → Agents → Install Hooks 时自动注册到 `~/.claude/settings.json` 的（脚本注释明说）。cc-view 的 wrapper 要包住它而不是替换它，避免 Otty 升级时覆盖。

### 方案 C（不可行）

- `otty-cli pane show/focus <claude-session-id>` — 实测 exit 4，selector 不认 Claude UUID。
- `otty-cli ipc agent.query` — 能查 state，但**返回不含 pane id**（见第 4 节），不能用于 focus。

---

## 4. agent state 机制（外部可读性）

### 写入：`state` 命令（由 hook 调用）

`otty-cli state --help`：
```
Usage: otty-cli state [OPTIONS] <KIND> [ARGS]...
Arguments:
  <KIND>     Agent kind: claude / codex / opencode
  [ARGS]...  Lifecycle params as key=value pairs (e.g. `session-id=abc state=idle`)
```

实际调用（从 `/Applications/Otty.app/Contents/Resources/agent-integration/claude/otty-hook.sh` 实读）：
```sh
otty-cli state:claude session-id="$sid" state="$state" bypass="$bypass" context-b64="$ctx"
```
- `state` 取值：`processing` / `idle` / `awaiting`（hook 的 $1）。
- `bypass`：0/1，用 `ps -o args= -p $claude_pid | grep -- --dangerously-skip-permissions` 检测。
- `context-b64`：仅 PermissionRequest 时传，是完整 hook stdin 的 base64（供 Otty 提供 auto-approve 上下文）。
- `session-id` 来源：优先 `$CLAUDE_SESSION_ID`，回退 stdin JSON 的 `session_id` 字段。

**hook 的精妙处理**：当 Claude 的 Stop 事件 payload 里含 `"type":"subagent","status":"running"`（Task 子 agent 还在跑）时，会把 `idle` 改报成 `processing`，避免每开一个子 agent 就误触发完成通知。这段逻辑值得 cc-view 借鉴。

### 读取（零侵入，cc-view 可用）：`agent.query` IPC

Otty 的 `ipc` 是隐藏子命令（`--help` 不在顶层列出）：
```
Usage: otty-cli ipc [OPTIONS] <COMMAND>
Arguments:
  <COMMAND>  IPC command name (e.g. "agent.state")
Options:
  -p, --param <KEY=VALUE>   Parameters as key=value pairs
```

**实测可读 agent 状态**（注意 key 用下划线 `session_id`，不是连字符；用 `-p` 传参，不是位置参数）：
```
$ otty-cli ipc agent.query -p session_id=21eb6f0e-8687-479e-9e04-6599d98bce43
{
  "agent": "claude",
  "found": true,
  "session_id": "21eb6f0e-8687-479e-9e04-6599d98bce43",
  "state": "processing"
}
```
- `found:false` 表示该 session 不在 Otty 当前内存（Otty 重启过 / 会话已结束）。
- **返回不含 pane id**——只能查状态，不能定位。
- 这是 cc-view 零侵入读 Otty agent 状态的补充数据源（比 cc-view 自己解析 Claude transcript 更轻）。

### 监听（阻塞）：`watch:<agent>` 隐藏命令

```
$ otty-cli watch:claude --help
Usage: otty-cli __watch-agent [OPTIONS] <AGENT> <SESSION_ID>
Arguments:
  <AGENT>       [possible values: claude, codex, opencode]
  <SESSION_ID>  Agent session ID to wait on (exact match against the ID reported
                by the agent's hooks — usually the full UUID)
Options:
  --interval-ms <N>      [default: 5000]
  --timeout-secs <N>     0 means wait forever [default: 0]
  -v, --verbose          Print state transitions to stderr
```
- 行为：阻塞到该 session 到达 `idle`，然后退出。退出码：**0 = idle（或 session 已关闭）/ 4 = session ID 从未见过 / 9 = timeout**。
- 用途：编排一个 agent 等另一个 agent 完成。
- 对 cc-view：可用于"等某会话完成后再 focus/通知"，但不是 focus 的核心接口。
- 实测：对一个历史 session id 调 `watch:claude <id> --timeout-secs 3 -v` 报 `error: Agent claude session '...' was never registered within 3s`（因为 Otty 内存里已没有它）。

### Otty 已内置的 agent 集成配置项（摘自 otty-cli binary strings）

Otty 已经把 agent 状态深度集成进 UI/通知/电源管理，相关 config key（可 `otty config get/set`）：
- `privilege-badge-agent-processing` / `privilege-badge-agent-task-complete` / `privilege-badge-agent-awaiting-input`（tab badge）
- `privilege-notify-agent-task-complete` / `privilege-notify-agent-awaiting-input`（系统通知）
- `privilege-caffeinate-agent-processing`（agent 工作时防系统休眠——cc-view 可借鉴）
- `privilege-resume-agent-session`（恢复 agent 会话）
- `detect-awaiting-input` / `show-auto-approve` / `auto-approve-enabled` / `hide-auto-approve-pill`

---

## 5. app bundle 关键发现

`/Applications/Otty.app/Contents/`：
- `Info.plist`：`CFBundleIdentifier=io.appmakes.otty`、`NSAppleScriptEnabled=true`、`OSAScriptingDefinition=Otty.sdef`、`LSMinimumSystemVersion=14.0`、`OttyGitHash=dc80e013`、`NSAppleEventsUsageDescription`。声明了自定义文档类型 `.ottyrecipe` / `.ottytheme`，URL scheme 支持 `x-man-page://` / `ssh://`。
- `MacOS/Otty`（36MB，GUI 主程序）+ `MacOS/otty-cli`（5.5MB，控制 CLI）。**两个独立的 arm64 Mach-O 二进制**（universal binary with 1 architecture: arm64，仅 Apple Silicon）。
- `Resources/otty-cli` **不在 PATH**，要么用全路径，要么在 Otty 里 "Install CLI"（建 `/usr/local/bin/otty` 符号链接），要么靠 shell-integration 把 `OTTY_BIN_DIR` 加进 PATH。
- `Resources/Otty.sdef`：**AppleScript 字典，drop-in 兼容 Terminal.app**。tab class 属性：`id`（= session id，CLI/AppleScript 共用）/ `contents` / `history` / `busy` / `processes` / `selected`（**可写，set true 即聚焦**）/ `custom title` / `tty`（如 `/dev/ttys003`）/ `working directory` / `number of rows|columns`。`do script` 命令兼容 Terminal.app。**这是除了 CLI 之外的另一条 focus 路径**：`tell application "Otty" to set selected of tab X of window Y to true`。
- `Resources/agent-integration/{claude,codex,opencode}/`：三种 agent 的集成。claude/codex 是 `otty-hook.sh`（shell），opencode 是 `otty-plugin.js`（8.5KB JS）。claude hook 是 code-signed、可审计的明文 sh。
- `Resources/shell-integration/otty-integration.{zsh,bash,fish}`：注入 `~/.zshrc`，实现 **OSC 133**（prompt/command 边界）、**OSC 7**（CWD reporting：`\e]7;file://host/path\a`，每次 cd 触发——这是 Otty 实时知道 pane cwd 的机制）、**OSC 9;4**（progress badge：3=start / 5;0=success / 5;2=error）。还有 OTTY_PROGRESS_COMMANDS（匹配特定命令显示进度）。
- `Resources/completions-client.db`（11.8MB sqlite）：Fig/autocomplete 数据。
- `Resources/CREDITS.md`：完整第三方依赖清单（见概述）。
- `Resources/more-from-us.json`：开发者其他产品（Otty / Typora / Paletro）。
- `Resources/settings-ui.html`（663KB）：Settings 面板是嵌入式 HTML（Solid.js + Tailwind + Vite）。
- `Resources/terminfo/`：bundled `xterm-ghostty` / `alacritty` terminfo。
- IPC：基于 unix socket（`--socket` 可覆盖路径，默认在 runtime 目录）。

---

## 6. 对 cc-view 的可借鉴点

1. **OSC 7 是终端知道 pane cwd 的通用机制**（不只 Otty，iTerm2/Terminal.app 都支持）。cc-view 若要支持多终端，OSC 7 捕获是跨终端通用的 cwd 来源。
2. **badge 枚举设计**（running/completed/finished/unread/error/awaiting-input）是成熟的状态可视化模型，cc-view 的会话状态展示可直接复用这套语义。
3. **hook + IPC state 模式**是 agent 集成的优雅范式：agent hook 报状态 → 终端/监控存状态 → 外部零侵入可查（`agent.query`）。cc-view 自己的监控也可考虑暴露类似 query 接口。
4. **`watch:<agent>` 的"编排一个 agent 等另一个"**思路，对 cc-view 的"等会话完成再动作"场景有用。
5. **`privilege-caffeinate-agent-processing`**（agent 工作时防系统休眠）——cc-view 作为 menubar 应用可直接借鉴，避免长任务跑一半机器睡了。
6. **`$OTTY_PANE_ID` 注入到每个 pane** 是让 pane 内进程"知道自己在哪"的干净做法。cc-view 若自建 hook 体系可沿用。
7. **AppleScript drop-in Terminal.app 兼容**意味着现成的 Terminal.app 自动化脚本改个 app 名就能用，生态复用成本低。
8. **subagent spurious idle 的处理**（看 background_tasks 里有没有 running 的 subagent，有则把 idle 改报 processing）——cc-view 解析 Claude hook 事件时必须同样处理，否则会误报完成。
9. **URL scheme `otty://` 作为 focus 入口**比 CLI 三连更简洁，且 Otty 未运行时会自动启动——cc-view 触发 focus 时优先用 `open otty://pane/<id>`。

---

## 7. 踩坑

1. **otty-cli 默认不在 PATH**。`which otty-cli` 报 not found。用 `/Applications/Otty.app/Contents/MacOS/otty-cli`，或先在 Otty 里 Install CLI。
2. **`tab show <任何 selector>` 会静默回退到 active tab，exit 0**——实测 `tab show 't_NOPE'` / `tab show --tab 't_NOPE'` 都返回当前 active tab，不报错。这和 reference/cli 文档说的 "non-matching selector fails with exit 4" **矛盾**。**结论：不能用 `tab show` 探测 selector 是否匹配**，要用 `pane show`（不匹配时 exit 4）或检查返回的 `id` 是否等于输入。
3. **`pane show 0`（index）报错**。pane 的 index selector 在不带 tab 上下文时无意义，实测不可用。tab 的 `show 0` 又回退到 active。**结论：定位用完整 id（`p_xxx`/`t_xxx`/`w_xxx`），别用 index**。
4. **selector "session id" 措辞陷阱**：`otty://pane/<sel>` 文档说接受 "a session id such as $OTTY_PANE_ID"，这里的 session id 指 **Otty pane session id（`p_` 后缀那段）**，不是 Claude UUID。实测 `pane show <claude-uuid>` exit 4。
5. **`ipc` 参数用 `-p key=value`，且 key 用下划线**（`session_id`，不是 `session-id`）。`ipc agent.query session-id=X`（位置参数）会报 `unexpected argument`；`ipc agent.query -p session-id=X`（连字符）报 `Required: session_id`。正确写法：`ipc agent.query -p session_id=X`。
6. **`agent.query` 返回不含 pane id**，只能查 state，不能用于 focus 定位。
7. **`state` 是 write-only**（required `state` 参数），不是 read 接口。读状态用 `ipc agent.query`。
8. **docs.otty.sh 是 VitePress SPA**，`curl` 拿到的 HTML 里 `/license`、`/pricing`、`/agents/supported-agents` 页面 body 是空的（前端渲染）。但 `/reference/cli`、`/reference/url-scheme` 的内容嵌在 HTML 里可 grep 出来。
9. **Claude session jsonl 第一行是 `aiTitle`**（任务标题），不是 cwd。cwd 只编码在父目录名里（`-` 替换 `/`）。反解时注意首尾 `-` 的处理。
10. **Dinotty ≠ Otty**，别当成同一产品或同一作者的早期版本。
11. **所有 `window/tab/pane` 命令 requires running app**。Otty 没跑时只有 `open` / `config` / `font` / `theme` 等可用。focus 逻辑要处理 Otty 未启动的情况（用 `open otty://...` 可自动拉起）。
12. **用户当前 13 个 Claude 会话同时在跑**，多个 tab 同 cwd（如 voucher-engine 有 6 个）。focus 时 cwd 歧义是真实问题，方案 B（hook 注入 pane_id）更稳。

---

## 附：实测可用的关键命令速查

```bash
# 列出所有 pane（含 cwd，用于 session→pane 桥接）
/Applications/Otty.app/Contents/MacOS/otty-cli pane list --json

# 查某 session 的 agent 状态（零侵入读）
/Applications/Otty.app/Contents/MacOS/otty-cli ipc agent.query -p session_id=<claude-uuid>

# focus 三连（按 id，顺序 window→tab→pane）
/Applications/Otty.app/Contents/MacOS/otty-cli window focus w_<UUID>
/Applications/Otty.app/Contents/MacOS/otty-cli tab focus t_<hex>_<hex>
/Applications/Otty.app/Contents/MacOS/otty-cli pane focus p_<hex>_<hex>

# 或一步式（推荐，Otty 没跑会先启动）
open "otty://pane/p_<hex>_<hex>"

# AppleScript 路径（Terminal.app 兼容）
osascript -e 'tell application "Otty" to set selected of (tab id "t_xxx" of window id "w_xxx") to true'

# 隐藏命令：监听某 session 直到 idle（带 verbose）
/Applications/Otty.app/Contents/MacOS/otty-cli watch:claude <claude-uuid> --timeout-secs 60 -v
```
