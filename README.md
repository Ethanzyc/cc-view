# cc-view

跨终端 Claude Code 会话状态总览 macOS menubar 应用。

cc-view 在系统菜单栏常驻一个图标，点击展开 popover，实时展示当前用户所有活跃 Claude Code 会话的状态（运行中 / 等待输入 / 等待权限 / 已退出），让你一眼看到哪个 claude 在干活、哪个在等你回复。

- 后端：Tauri (Rust)，扫描 `~/.claude/sessions/<pid>.json` + 进程存活检测，3 秒轮询。
- 前端：Vue 3 + TypeScript，监听 `sessions` 事件驱动 popover 列表。

## 开发

```bash
npm install
npm run tauri dev
```

## 已知限制（Plan 1）

当前为 Plan 1 阶段，仅覆盖核心数据链路与最小可用 UI，以下能力明确不在本阶段范围内，将在 Plan 2 及之后补齐：

- **数据源单一**：仅消费前台 `~/.claude/sessions/<pid>.json`；尚未接入 `claude agents --json`、`roster.json`、JSONL 末尾尾部等来源。
- **无通知**：状态变化不推送系统通知（Plan 2 的 A 项）。
- **无 focus 跳转**：点击会话行不会唤起对应终端窗口（Plan 2 的 C 项）。
- **无隐藏 / 归档**：不能把噪音会话从列表里隐藏或归档（Plan 2 的 E 项）。
- **NeedsPermission 暂不区分**：`PermissionChecker` 在 Plan 2 落地，当前 status 直接透出后端字段，不区分等待权限的具体类型。
- **Compacting 状态未实现**：状态机第四态 `Compacting` 暂未接入，Plan 2 补。
- **GUI 形态为 popover**：使用 NSPanel 形态的 popover，NSPanel 的精修（尺寸、动画、脱离菜单栏等）在 Plan 2。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
