# cc-view

> 跨终端 Claude Code 会话状态总览 · macOS menubar

[![release](https://img.shields.io/github/v/release/Ethanzyc/cc-view?color=blue)](https://github.com/Ethanzyc/cc-view/releases)
[![platform](https://img.shields.io/badge/platform-macOS%2013%2B-black)](#安装)
[![arch](https://img.shields.io/badge/arch-Apple%20Silicon-silver)](#安装)
[![built with Tauri](https://img.shields.io/badge/built%20with-Tauri-orange)](https://tauri.app)
[![license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

并行开多个 Claude Code 终端会话时，状态散落在各 tab / 窗口——哪个在干活、哪个等你回复 / 权限、哪个已经 idle，得逐个切窗口看。容易漏掉阻塞：一个 claude 等你确认权限 10 分钟你没注意，它就干等。

cc-view 把所有会话聚合到 **menubar 一个图标**（hover 看「N 等我 · M 工作」，需介入时染橙 + 红圆 badge）+ **⌥Space 命令面板**（搜索 / focus 跳终端 / 搁置 / 归档 / 复制 ID，失焦收起）+ **常驻精简面板**（贴桌面、状态色闪动提醒）+ 系统通知——不用切窗口就能掌握全部会话、快速跳到要处理的那个。

## 截图

**命令面板（⌥Space 呼出）— 多项目多状态 + token 列**

![命令面板](doc/screenshots/overlay.png)

**会话详情（点行内 📊）— 上下文曲线 + 回合消耗明细**

![会话详情](doc/screenshots/detail.png)

**常驻模式（贴桌面 · 待介入闪动提醒）**

![常驻](doc/screenshots/resident.png)

**偏好设置（token 单位 · 外观 · 按钮组）**

![偏好设置](doc/screenshots/preferences.png)

## 功能

### 🎛️ 命令面板（⌥Space）
- 全局快捷键呼出居中 overlay，**搜索**（名称 / 项目）/ **聚焦**（跳终端）/ **搁置** / **归档** / **复制 ID**
- 失焦自动收起；可图钉钉住；位置记忆，拖动后持久化恢复

### 📌 常驻模式
- 同一 overlay 的精简形态——贴桌面常驻、失焦不收起、背景透明度可调（0–100%）
- **B 精简**（分组 + 状态文字）/ **A 极简**（仅图标 + 名称）两种布局，可切换显示搁置 / 闲置
- **待介入闪动提醒**：会话从「非待介入」切到「待介入」时，目标行 + 整框用状态色脉动 3 次（仅常驻模式，节流防狂闪）
- 右上角一键展开成完整命令面板，面板内一键收起

### 🎨 外观主题（v0.2.0 新）
- 偏好设置手动切**浅色 / 深色**，默认浅色、**不跟随系统**
- 毛玻璃（vibrancy）material 优化为 `UnderWindowBackground`：深色下文字清晰不糊、浅色透明度 0% 不再突兀白

### 📊 Token 统计 & 上下文详情（v0.3.0 新）
- 列表每行显示累计 token（输入↑ / 输出↓），一眼看出哪个会话烧得多
- 点详情看会话 token 明细：**当前上下文**大数字 + sparkline 增长曲线 + 消耗三格（输入 / 输出 / 缓存命中）+ 按回合列表（背景进度条 + 上下文列）
- compact 自动检测：相邻回合上下文大幅下降（降 30%+）即推断一次压缩（**不依赖 `compact_boundary` 标记**，新版 claude-code 也能识别）
- token 单位可配置：**k/M**（国际，默认）或 **万/亿**（中文），偏好设置切换

### 🔔 menubar 聚合 + 系统通知
- menubar 图标 tooltip「N 等我 · M 工作 / 等权限 / 等回答」；有 NeedsPermission / WaitingForReply 时染橙 + 红圆 badge
- 会话进入「等权限 / 等输入 / 需要关注」时弹系统通知（按会话名区分，可关闭）

### 🗂️ 搁置 & 归档
- **搁置**：暂时不管（不催促、不通知）；会话被重新输入（状态更新）后**自动取消搁置**、冒泡回待介入
- **归档**：收起不常看的会话（持久化，可恢复）

### 💤 闲置降级
- 等输入超 30min 自动灰显 + 标「闲置」，全闲置项目整组下沉；超时等回答同样灰显——不抢注意力

### ⚙️ 偏好设置 & 自动更新
- 开机自启动 / 通知开关 / 轮询间隔（1–30s）
- 选择控件统一为**可点击按钮组**：全局快捷键、常驻布局、外观
- 基于 [tauri-plugin-updater](https://v2.tauri.app/plugin/updater/) 的自动检查 + 下载安装 + 重启

## 快捷键

| 快捷键 | 作用 | 可配置 |
| --- | --- | :---: |
| `⌥Space` | 呼出 / 收起命令面板 | ✅ 可改 `⌘⌥Space` / `⌃Space` / 禁用 |
| `Enter`（面板内） | 聚焦选中会话（跳终端） | — |
| 点击 / 回车 会话行 | 聚焦该会话 | — |

## 安装

1. 下载最新 [release dmg](https://github.com/Ethanzyc/cc-view/releases/latest)（`cc-view_<ver>_aarch64.dmg`）。
2. 打开 dmg，拖 cc-view 到 **Applications**。
3. 启动 cc-view——menubar 出现图标（平时无 dock 图标），`⌥Space` 呼出命令面板。

> **首次打开提示「已损坏」？** cc-view 未做 Apple 公证（个人开源项目，无开发者证书），macOS Gatekeeper 会拦。终端跑一下即可放开：
> ```bash
> xattr -dr com.apple.quarantine /Applications/cc-view.app
> ```

**要求**：macOS 13+，Apple Silicon（aarch64）。首次运行在系统设置里授权：
- **通知**：系统通知
- **辅助功能**：focus 跳全屏 app（点 Dock 切全屏 Space 需要）；**首次 focus 会弹系统授权窗**引导

## 数据源

cc-view 聚合多路数据源拼出最准确的会话视图：

- `~/.claude/sessions/<pid>.json`：前台交互会话状态
- roster / 后台 agent 会话列表
- `claude agents --json`：fleet agent 精确状态（运行中 / 等输入 / 等权限）
- JSONL 尾部文本：等权限预测 + Compacting 检测（识别 `compact_boundary`）

## 开发

```bash
npm install
npm run tauri dev     # 开发
npm run tauri build   # 产出 .app / dmg / updater artifacts
```

**技术栈**：Tauri 2（Rust）+ Vue 3 + TypeScript。后端每 3s 采集 → 状态机 reduce → 通知 → hash 去重 → emit `sessions` 事件；前端 Vue 监听渲染。

**推荐 IDE**：[VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 已知限制

- **focus 为 app 级**：点会话行 activate 宿主终端 **app**（Terminal / iTerm / Ghostty / Otty …），不区分同 app 的多窗口 / tab / pane——同一终端 app 多窗口时跳到哪个不确定。精确切 tab / window 待后续终端 app 集成（见路线图）。
- **Compacting 检测为 post-compact**：详情面板用上下文跳降启发式（不依赖 `compact_boundary`，适配新版 claude-code）；状态机的"压缩中"判定仍用 `compact_boundary`，compaction 进行中的 ~2min 内无法实时检测。
- **updater 依赖网络**：检查更新走 `github.com`，不可达时报错（中文提示），需 GitHub 可达或代理。
- **仅 Apple Silicon**：universal binary（Intel）待后续。

## 路线图

- [ ] **终端 app 精确切 tab / window**：iTerm2 / Otty / Ghostty / Warp / VSCode / IntelliJ / Terminal 各自的 AppleScript / CLI 集成（当前 app 级 activate，同 app 多窗口不精确）
- [ ] Universal binary（Intel）支持
- [ ] 更多终端 host 自动识别
- [ ] 透明度 / 毛玻璃效果进一步可调

## 致谢

- [Tauri](https://tauri.app) · [Vue](https://vuejs.org) · [Claude Code](https://claude.com/claude-code)（Anthropic）

## 许可

[MIT](LICENSE)
