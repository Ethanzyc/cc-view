# cc-view

> 跨终端 Claude Code 会话状态总览 · macOS menubar

[English](README.en.md) · 简体中文

[![release](https://img.shields.io/github/v/release/Ethanzyc/cc-view?color=blue)](https://github.com/Ethanzyc/cc-view/releases)
[![platform](https://img.shields.io/badge/platform-macOS%2013%2B-black)](#安装)
[![arch](https://img.shields.io/badge/arch-Apple%20Silicon%20%2B%20Intel-silver)](#安装)
[![built with Tauri](https://img.shields.io/badge/built%20with-Tauri-orange)](https://tauri.app)
[![license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

并行开多个 Claude Code 终端会话时，状态散落在各 tab / 窗口——哪个在干活、哪个等你回复 / 权限、哪个已经 idle，得逐个切窗口看。容易漏掉阻塞：一个 claude 等你确认权限 10 分钟你没注意，它就干等。

cc-view 把所有会话聚合到 **menubar 一个图标**（hover 看「N 等我 · M 工作」，需介入时染橙 + 红圆 badge）+ **⌥Space 命令面板**（搜索 / focus 跳终端 / 搁置 / 归档 / 复制 ID，失焦收起）+ **常驻精简面板**（贴桌面、状态色闪动提醒）+ **⌘, 偏好设置**（VSCode 风格）+ 系统通知——不用切窗口就能掌握全部会话、快速跳到要处理的那个。

**点会话直接跳到正确的终端 tab**——不再是 app 级 activate（iTerm2 / Terminal / Otty 按 TTY 精确到 tab，Ghostty / cmux 按 cwd 精确到 terminal）。

**自动更新双源兜底**——GitHub 为主，Gitee 为国内网络兜底，GitHub 不可达时自动切换。

## 截图

**搜索 + 跳终端（⌥Space → 输入 → Enter）**

![搜索 + 跳终端](doc/screenshots/demo-search.gif)

**任务完成提醒 → 点击跳转**

![任务完成提醒](doc/screenshots/demo-notify.gif)

**常驻面板（贴桌面 · 展开收起）**

![常驻面板](doc/screenshots/demo-resident.gif)

---

**命令面板（⌥Space 呼出）— 多项目多状态 + token 列**

![命令面板](doc/screenshots/overlay.png)

**会话详情（点行内 📊）— 上下文曲线 + 回合消耗明细**

![会话详情](doc/screenshots/detail.png)

**常驻模式（贴桌面 · 待介入闪动提醒）**

![常驻](doc/screenshots/resident.png)

**偏好设置（⌘, · VSCode 风格 · toggle switch）**

![偏好设置](doc/screenshots/preferences.png)

## 功能

### 🎛️ 命令面板（⌥Space）
- 全局快捷键呼出居中 overlay，**搜索**（名称 / 项目）/ **聚焦**（跳终端）/ **搁置** / **归档** / **复制 ID**
- 失焦自动收起；可图钉钉住；位置记忆，拖动后持久化恢复
- 列表显示可配置：终端名 / token 用量 / 操作按钮均可独立开关（偏好设置 → 显示）

### 🎯 精确切终端 tab
- 点会话行直接跳到**正确的终端 tab/window**，不再是 app 级 activate
- **iTerm2 / Terminal.app / Otty**：TTY 匹配（`ps` 取控制 TTY → AppleScript 遍历 tab/session 找匹配）
- **Ghostty ≥ 1.3.0 / cmux**：OSC 7 marker 精确匹配（往 TTY 写唯一 cwd 标记 → AppleScript 匹配 → 恢复）。cmux 基于 libghostty，继承了 Ghostty 的 AppleScript 模型
- **其余终端**（Warp / VSCode / IntelliJ / WezTerm / Alacritty / Kitty）：app 级 activate（无可编程 API 或需额外配置）
- 全屏 Space 切换：统一 click Dock 图标（唯一可靠方式）

### 📌 常驻模式
- 同一 overlay 的精简形态——贴桌面常驻、失焦不收起、背景透明度可调（0–100%）、**宽度可调**（拖右边缘 140–480px，右锚定不动，拖完持久化）
- **B 精简**（分组 + 状态文字）/ **A 极简**（仅图标 + 名称）两种布局，可切换显示搁置 / 闲置
- **待介入闪动提醒**：会话从「非待介入」切到「待介入」时，目标行 + 整框用状态色脉动 3 次（仅常驻模式，节流防狂闪）
- **未读红点**：会话切到待介入时行前红点提醒（未读消息式），focus 跳转或该会话恢复处理后自动消除——常驻 + 命令面板都有
- 右上角一键展开成完整命令面板，面板内一键收起

### 🎨 外观主题
- 偏好设置手动切**浅色 / 深色**，默认浅色、**不跟随系统**
- 毛玻璃（vibrancy）material 优化为 `UnderWindowBackground`：深色下文字清晰不糊、浅色透明度 0% 不再突兀白

### 📊 Token 统计 & 上下文详情
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
- **VSCode 风格**：左侧分类导航（通用 / 显示 / 更新）+ 右侧设置项行，`⌘,` 全局快捷键打开
- **显示**分类：主题 / token 单位 / **显示终端名** / **显示 token 用量** / **显示操作按钮** / 常驻布局 / 搁置 / 闲置 / 透明度 / 面板宽度
- 开关统一 **macOS toggle switch**；搁置 / 闲置分项带说明（搁置＝手动暂停不催促不通知；闲置＝等输入超 30min 自动降级）
- 基于 [tauri-plugin-updater](https://v2.tauri.app/plugin/updater/) 的自动检查 + 下载安装 + 重启
- **双源兜底**：GitHub 为主 → Gitee 为国内网络兜底（updater 按序自动 fallback）

### 🌐 国际化
- 支持**简体中文**和**英语**，自动检测系统语言
- 偏好设置 → 通用 → 语言切换（跟随系统 / 简体中文 / English）
- menubar 菜单、tooltip、通知文本全部本地化

## 快捷键

| 快捷键 | 作用 | 可配置 |
| --- | --- | :---: |
| `⌥Space` | 呼出 / 收起命令面板 | ✅ 可改 `⌘⌥Space` / `⌃Space` / 禁用 |
| `⌘,` | 打开偏好设置 | — |
| `Enter`（面板内） | 聚焦选中会话（跳终端） | — |
| 点击 / 回车 会话行 | 聚焦该会话 | — |

## 安装

1. 下载最新 [release dmg](https://github.com/Ethanzyc/cc-view/releases/latest)：按 CPU 架构选——Apple Silicon（M 系列）用 `cc-view_<ver>_aarch64.dmg`，Intel 用 `cc-view_<ver>_x86_64.dmg`。
2. 打开 dmg，拖 cc-view 到 **Applications**。
3. 启动 cc-view——menubar 出现图标（平时无 dock 图标），`⌥Space` 呼出命令面板。

### Gatekeeper「无法验证」提示

cc-view 是个人开源项目，未做 Apple 公证（公证需 $99/年 Apple Developer Program），macOS Gatekeeper 会拦截 DMG 安装的 app。处理方式：

1. **双击打开** → 提示「Apple 无法验证…」→ 点「完成」关闭
2. **系统设置 → 隐私与安全性** → 滚到底 → 点「仍要打开」→ 确认
3. cc-view 启动后会**自动清除 quarantine 标记**（v0.5.4+），后续启动不再弹此提示

> 也可以直接终端跑 `xattr -dr com.apple.quarantine /Applications/cc-view.app`，效果一样。

> **自动更新不受影响**：cc-view 的内置更新器下载的 app 不带 quarantine 标记，更新后直接启动，不会弹 Gatekeeper。只有从 DMG 手动安装时才有此提示。

**要求**：macOS 13+，Apple Silicon（aarch64，已验证）或 Intel（x86_64，**未经实机测试**，有问题[提 issue](https://github.com/Ethanzyc/cc-view/issues)）。首次运行在系统设置里授权：
- **通知**：系统通知
- **辅助功能**：focus 跳全屏 app（点 Dock 切全屏 Space 需要）；**首次 focus 会弹系统授权窗**引导
- **自动化**：控制终端 app（iTerm / Terminal / Otty / Ghostty 的 AppleScript）；**首次切换到对应终端会弹授权窗**

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

- **DMG 安装触发 Gatekeeper**：cc-view 未做 Apple 公证（个人项目，公证需 $99/年 Apple Developer Program）。DMG 安装后首次打开会提示「Apple 无法验证…」，需在隐私与安全性里点「仍要打开」。app 启动后自动清除 quarantine（v0.5.4+），后续启动不再弹。**自动更新不受影响**——更新器下载的 app 不带 quarantine，直接启动。彻底消除需 Apple 公证。
- **常驻拖动需先点击**：常驻面板是 nonActivating panel（贴桌面不抢焦点），失焦后再拖动需先点一下面板回归焦点才能拖——输入可用性（becomesKeyOnlyIfNeeded）与拖动便利的折衷，无法在不破坏终端输入的前提下消除。
- **精确切 tab 覆盖范围**：iTerm2 / Terminal / Otty（TTY 匹配）和 Ghostty / cmux（OSC 7 marker）已精确到 tab/terminal；Warp / VSCode / IntelliJ / WezTerm / Alacritty / Kitty 因无可编程 API 或需额外配置，仍为 app 级 activate（同 app 多窗口不精确）。
- **Compacting 检测为 post-compact**：详情面板用上下文跳降启发式（不依赖 `compact_boundary`，适配新版 claude-code）；状态机的"压缩中"判定仍用 `compact_boundary`，compaction 进行中的 ~2min 内无法实时检测。
- **Intel 版未经实机测试**：x86_64 包由 Apple Silicon 交叉编译产出（纯 Rust + 系统框架依赖，理论可用），未在真实 Intel Mac 上验证运行；遇到问题请[提 issue](https://github.com/Ethanzyc/cc-view/issues)。

## 路线图

- [ ] Apple 公证（消除 Gatekeeper 警告，需 $99/年 Developer Program）
- [ ] Kitty remote control 精确切换（需用户开 `allow_remote_control`）
- [ ] WezTerm `wezterm cli focus-pane` 精确切换
- [ ] VSCode / IntelliJ 打开项目窗口增强
- [ ] 透明度 / 毛玻璃效果进一步可调

## 致谢

- [Tauri](https://tauri.app) · [Vue](https://vuejs.org) · [Claude Code](https://claude.com/claude-code)（Anthropic）

## 许可

[MIT](LICENSE)
