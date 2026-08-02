# cc-view

跨终端 Claude Code 会话状态总览 macOS menubar 应用——**常驻指挥台 + 快捷呼出**形态。

cc-view 在桌面常驻一个 always-on-top 的悬浮 HUD（可拖动、位置记忆），在系统菜单栏聚合一个动态图标（鼠标悬停即看「N 等我 · M 工作」，一旦有会话需要人工介入图标立刻变橙提醒），并通过 **⌥Space（Option+Space）** 全局快捷键随时呼出一个居中的命令面板 overlay——Alfred / uTools 风格：搜索会话、focus 跳转、隐藏、复制会话 ID，失焦自动收起。HUD 内实时展示当前用户所有活跃 Claude Code 会话的状态（运行中 / 等待输入 / 等待权限 / Shell 模式 / Compacting；已退出的会话行自动置灰），让你一眼看到哪个 claude 在干活、哪个在等你回复。

## 功能

所有核心功能已完成：

- **桌面常驻 HUD**：always-on-top 悬浮窗口，启动即显示，不抢焦点（不打断终端输入）；标题区可拖动到屏幕任意位置，位置持久化到 `~/.claude/cc-view/hud-position.json`，重启自动恢复。
- **menubar 聚合图标**：菜单栏图标动态聚合全部会话状态——tooltip 显示「N 等我 · M 工作」；只要存在 NeedsPermission / WaitingForInput 会话，图标立即染成 macOS system orange（RGB 255,149,0），状态清零后自动还原。左键点击 menubar 图标 toggle HUD 显示 / 隐藏。
- **会话总览**：HUD 内实时展示全部活跃 Claude Code 会话，状态变化 3 秒内刷新（内容 hash 去重，状态不变不刷屏）。
- **状态通知**：会话进入「等待权限确认 / 等待输入 / 需要关注」等需要人工介入的状态时弹 macOS 系统通知，按会话名区分。
- **⌥Space 命令面板 overlay**：任意位置按 **⌥Space（Option+Space）** 全局快捷键呼出居中浮层（Alfred / uTools 风格）。支持搜索过滤会话、点选会话行 focus 跳转终端、复制会话 ID；点击外部自动收起，再按 ⌥Space 切换显示。
- **点击 Focus**：在 HUD 或命令面板中点击任意会话行，自动 activate 该会话所在终端 app（基于 sysinfo 进程树回溯 + osascript），无需手动切窗口。
- **隐藏 / 归档**：右键会话行可隐藏（持久化到 `~/.claude/cc-view/hidden.json`），已退出会话自动置灰，列表保持干净。

### 数据源

cc-view 聚合多路数据源拼出最准确的会话视图：

- `~/.claude/sessions/<pid>.json`：前台交互会话状态。
- roster 源：后台 agent 会话列表。
- `claude agents --json`：fleet agent 的精确状态（运行中 / 等待输入 / 等待权限）。
- JSONL 尾部文本：用于「等待权限」预测与 Compacting 检测（识别 `compact_boundary` 标记）。

## 开发

```bash
npm install
npm run tauri dev
```

技术栈：

- 后端：Tauri (Rust)，3 秒轮询 + 状态机 reduce + 系统通知 + tray 动态 tooltip/icon + 窗口位置记忆。
- 前端：Vue 3 + TypeScript，监听 `sessions` 事件驱动 HUD 列表；`-webkit-app-region: drag` 让标题区拖动 HUD。

## 已知限制

以下是当前实现的真实边界（**Plan 1-6 全部功能已完成**），下列项目为可选后续增强：

- **精细 Focus（可选增强）**：点击会话行默认 activate 宿主终端 app（Terminal / iTerm / Ghostty 等），尚未精确到 tab / pane 级别。同 app 多会话时仍需手动切到目标 tab。后续可选增强：iTerm2 AppleScript tab/pane 定位、Ghostty 按 cwd 选 terminal、tmux `select-pane` 等。
- **Compacting 检测为 post-compact 窗口**：当前通过识别 JSONL 末尾的 `compact_boundary` 标记判定进入 Compacting 状态——这是 compaction **完成**后写入的标记。实际 compaction 进行中的 ~2min 内 JSONL 无任何写入，无法实时检测「正在 compacting」。即用户看到的 `Compacting` 状态会比真实开始晚一段时间。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
