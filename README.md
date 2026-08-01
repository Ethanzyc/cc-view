# cc-view

跨终端 Claude Code 会话状态总览 macOS menubar 应用。

cc-view 在系统菜单栏常驻一个图标，点击展开 popover，实时展示当前用户所有活跃 Claude Code 会话的状态（运行中 / 等待输入 / 等待权限 / Shell 模式 / Compacting；已退出的会话行自动置灰），让你一眼看到哪个 claude 在干活、哪个在等你回复。

## 功能

所有核心功能已完成：

- **会话总览**：菜单栏 popover 实时展示全部活跃 Claude Code 会话，状态变化 3 秒内刷新（内容 hash 去重，状态不变不刷屏）。
- **状态通知**：会话进入「等待权限确认 / 等待输入 / 需要关注」等需要人工介入的状态时弹 macOS 系统通知，按会话名区分。
- **点击 Focus（MVP）**：在 popover 中点击任意会话行，自动 activate 该会话所在终端 app（基于 sysinfo 进程树回溯 + osascript），无需手动切窗口。
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

- 后端：Tauri (Rust)，3 秒轮询 + 状态机 reduce + 系统通知。
- 前端：Vue 3 + TypeScript，监听 `sessions` 事件驱动 popover 列表。

## 已知限制

以下两点是当前实现的真实边界，后续 Plan 会继续打磨：

- **Focus 为 MVP**：点击会话行只 activate 宿主终端 app（Terminal / iTerm / Ghostty 等），尚未精确到 tab / pane 级别。同 app 多会话时仍需手动切到目标 tab。精细 focus（每终端 AppleScript tab/pane 定位）计划在后续增强。
- **Compacting 检测为 post-compact 窗口**：当前通过识别 JSONL 末尾的 `compact_boundary` 标记判定进入 Compacting 状态——这是 compaction **完成**后写入的标记。实际 compaction 进行中的 ~2min 内 JSONL 无任何写入，无法实时检测「正在 compacting」。即用户看到的 `Compacting` 状态会比真实开始晚一段时间。

GUI 视觉精修（popover 尺寸、动画、脱离菜单栏等细节）留待后续统一打磨。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
