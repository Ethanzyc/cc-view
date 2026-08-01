# cc-view

跨终端 Claude Code 会话状态总览 macOS menubar 应用。

cc-view 在系统菜单栏常驻一个图标，点击展开 popover，实时展示当前用户所有活跃 Claude Code 会话的状态（运行中 / 等待输入 / 等待权限 / Shell 模式；已退出的会话行自动置灰），让你一眼看到哪个 claude 在干活、哪个在等你回复。

- 后端：Tauri (Rust)，扫描 `~/.claude/sessions/<pid>.json` + 进程存活检测，3 秒轮询。
- 前端：Vue 3 + TypeScript，监听 `sessions` 事件驱动 popover 列表。

## 开发

```bash
npm install
npm run tauri dev
```

## 已知限制

当前仅覆盖核心数据链路与最小可用 UI，以下能力尚未实现，将在后续 Plan 中补齐：

- **数据源**：已接入前台 `~/.claude/sessions/<pid>.json` 与后台 roster 源；尚未接入 `claude agents --json`、JSONL 尾部等来源。
- **无 focus 跳转**：点击会话行不会唤起对应终端窗口（Plan 4 的 C 项）。
- **Compacting 状态未实现**：状态机第四态 `Compacting` 暂未接入。
- **GUI 精修**：popover（NSPanel 形态）的尺寸、动画、脱离菜单栏等细节尚待精修。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
