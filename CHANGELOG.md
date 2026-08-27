# Changelog

本仓库未打 git tag，以下版本边界基于 commit message 中的版本标注（v0.1.x / bump / 升 vX）与发布日期推断。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [0.9.0] - 2026-08-27

### Added
- 支持 ZCode 桌面 App——偏好设置开启「启用 ZCode 会话」后,ZCode 会话与 Claude Code 会话同面板管理:实时状态、token 用量、回合消耗明细(只读采集 ~/.zcode 本地数据库,默认关闭)
- 点击 ZCode 会话一键切换到 ZCode 应用(应用级激活)

## [0.8.1] - 2026-08-27

### Removed
- 全局 ⌘, 打开偏好设置——该组合键注册为系统级热键后会抢占其他 App 的 ⌘,（各 App 的偏好快捷键本应只在自己前台时生效）。偏好设置改由 menubar 图标菜单打开

## [0.4.1] - 2026-08-10

### Changed
- 打开 app 后默认显示面板——启动即呼出 overlay，无需先点 menubar 图标（panel 居中 / resident 右上角，按当前模式走）

## [0.4.0] - 2026-08-10

### Added
- ⌘, 全局快捷键打开偏好设置（VSCode/macOS 习惯，独立于 ⌥Space overlay 快捷键）
- 任务完成未读红点——会话切到待介入时行前红点提醒（未读消息式），focus 跳转或该会话恢复处理后自动消除；常驻 + 命令面板共用
- 常驻面板宽度可调——拖右边缘 140–480px，右锚定不动，拖完持久化
- 常驻面板拖拽防抖（move 事件 IO write debounce）

### Changed
- 偏好设置重做为 VSCode 风格——左侧分类导航（通用 / 外观 / 常驻面板 / 更新）+ 右侧设置项行
- 开关统一 macOS toggle switch；搁置 / 闲置拆两行各自带说明；配置项字号缩小

### Known Limitations
- 常驻拖动需先点击——nonActivating panel 失焦后需先点一下回归焦点才能拖（输入可用性 becomesKeyOnlyIfNeeded 与拖动便利的折衷）

## [0.3.1] - 2026-08-10

### Added
- Intel (x86_64) 双架构支持——新增 Intel Mac 安装包（与 Apple Silicon 并行发版）
- `make-dmg.sh` 手动打包脚本（含 Gatekeeper 提示）

### Changed
- 重做 README 截图——外框美化 + 多场景（命令面板 / 详情 / 常驻 / 偏好）+ token 详情，固化 demo HTML

### Fixed
- 修复 claude-code 新版进程被误判已退出（liveness 检测）

## [0.3.0] - 2026-08-10

### Added
- 会话 token 统计——列表每行显示累计输入↑/输出↓ token
- 会话 token 详情视图——当前上下文大数字 + sparkline 增长曲线 + 消耗三格（输入/输出/缓存命中）+ 按回合列表（背景进度条 + 上下文列）
- token 单位可配置：**k/M**（国际，默认）或 **万/亿**（中文）
- 归档显示开关持久化

### Changed
- actions 按钮由文字改为 Lucide 图标（搁置 / 归档 / 复制 ID）

### Fixed
- compact 自动检测启发式——相邻回合上下文降 30%+ 即推断压缩（不依赖 `compact_boundary`，适配新版 claude-code）
- 中断回合（stop_sequence）上下文继承前一回合值，不归零
- `ctx=0` 异常 assistant（usage 全 0）过滤，避免 sparkline 掉底 / compact 误判
- 搁置后任意状态更新即取消搁置（重新输入也冒泡回待介入）
- 模式切换 Focused 宽限期（动画期间窗口 resign key 抖动不再触发 hide）

## [0.2.0] - 2026-08-07

### Added
- **常驻模式**——overlay 精简形态贴桌面常驻、失焦不收起、背景透明度可调（0–100%）
- 常驻两种布局：**B 精简**（分组 + 状态文字）/ **A 极简**（仅图标 + 名称），可切换显示搁置 / 闲置
- 待介入闪动提醒——会话切入待介入时目标行 + 整框用状态色脉动 3 次（仅常驻模式，节流防狂闪）
- 外观主题——手动切浅色 / 深色（默认浅色、不跟随系统）
- 偏好设置选择控件统一为可点击按钮组（快捷键 / 常驻布局 / 外观）
- 模式切换原生窗口缩放动画（`setFrame:display:animate:`，Core Animation 平滑）
- 显示搁置 / 闲置开关过滤

### Changed
- vibrancy material 改 `UnderWindowBackground`——深色下文字清晰不糊、浅色透明度 0% 不再突兀白

### Fixed
- 模式切换动画顿挫（手动逐帧 → 原生 `setFrame` 一次插值）
- code review 修复：高度自适应反馈循环 / listener 泄漏 / slider debounce / set_mode lock
- 深色 prefs 背景 / 透明度预览 / 跳转诊断

## [0.1.2] - 2026-08-04

### Fixed
- 面板按钮实色（毛玻璃下文字不清）
- 检查更新错误提示（网络不可达中文提示）

## [0.1.1] - 2026-08-04

### Fixed
- dock 常驻（accessory app 激活策略）
- 偏好设置入口按钮
- 复制 ID 文案
- 检查更新 timeout
- 深色模式 tray 图标

## [0.1.0] - 2026-08-01

### Added
- menubar 图标聚合——hover 看「N 等我 · M 工作」；需介入（等权限 / 等回答）染橙 + 红圆 badge
- 3s 轮询采集 `~/.claude/sessions/*.json` + content hash 去重 + emit `sessions` 事件
- 状态机：Working / WaitingForPermission / WaitingForReply / WaitingForInput / Shell
- 会话列表 popover（按项目分组）
- 系统通知——状态迁移时按会话名区分推送（可关闭）
- 权限预测 `PermissionChecker`——读 user+project+local 三层 settings + JSONL 末尾 pending tool_use
- 归档——隐藏不常看会话（持久化，可恢复）
- roster.json fleet / slash workers 合并
- pid 存活校验（`proc_pidpath`）
