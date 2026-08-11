# cc-view 项目规范

## 项目概述

跨终端 Claude Code 会话状态总览，macOS menubar app。Tauri 2（Rust）+ Vue 3 + TypeScript。

## 技术栈与约定

- **后端**：Rust（`src-tauri/`），每 3s collect → reduce → notify → hash 去重 → emit `sessions`
- **前端**：Vue 3 + TypeScript（`src/`），监听 `sessions` 事件渲染
- **交互和注释用中文，代码用英文**
- 代码风格：简洁优于巧妙，可读性第一
- 错误处理遵循 fail fast 原则

## 常用命令

```bash
npm run dev              # 前端 dev
npm run tauri dev        # 全栈 dev
npm run tauri build      # 生产构建
cargo test --lib         # 后端测试（在 src-tauri/ 下）
npm run build            # 前端构建（含 vue-tsc 类型检查）
cargo fmt                # 格式化（提交前必跑，CI 会检查）
```

## 关键架构决策

### 窗口模式
- **overlay**：命令面板（⌥Space）/ 常驻面板，nonActivating panel（贴桌面不抢焦点）
- **prefs**：偏好设置窗口，Regular（有 dock + traffic lights）
- app 平时 Accessory（无 dock），打开偏好设置时切 Regular

### 签名与发布
- **代码签名**：Apple Development 证书（非 ad-hoc），DR = identifier + certificate（跨构建稳定，解决 TCC 更新后失效）
- **签名流程**：`scripts/resign.sh` 在 Tauri build 后重签 .app + 重打包 updater archive
- **Gatekeeper**：未公证（免费 Apple Developer 账号），DMG 安装后需隐私设置「仍要打开」一次；`strip_quarantine()` 启动时自动清除
- **双平台发布**：GitHub（主）+ Gitee（国内兜底），`git push` 自动双推
- **一键发布**：`./scripts/publish.sh <version> [changelog_file]`，详见 `.claude/skills/release/SKILL.md`

### 更新器
- **双源兜底**：用户可选「自动（GitHub 优先）」或「Gitee 优先」，均带 fallback
- `check_update_custom` 命令用 `UpdaterBuilder.endpoints()` 动态构造 endpoint 顺序
- 下载安装仍走插件原生 `plugin:updater|download_and_install`（通过 rid）
- **latest.json**：GitHub 用 release asset（`releases/latest/download/`）；Gitee 用仓库根目录 raw（`raw/main/latest.json`，每次发版更新）

### Vue + Tauri 注意事项
- Tauri `Update` / `Resource` 对象**不要**用 `ref()` 存——Vue Proxy 会破坏 WeakMap 私有字段。用 `shallowRef()`
- 偏好设置 `updateAvailable` 用 `shallowRef<CustomUpdate | null>`

## 文件结构要点

- `src-tauri/src/lib.rs`：主入口，所有 Tauri commands、窗口管理、菜单
- `src-tauri/src/prefs.rs`：偏好设置（`~/.claude/cc-view/prefs.json`），所有字段 serde lowercase
- `src-tauri/src/focus.rs`：终端精确切换（TTY 匹配 / OSC 7 marker）
- `src/components/Preferences.vue`：偏好设置 UI（VSCode 风格：左 nav + 右设置项）
- `src/types.ts`：前后端共享类型（与 Rust serde 对齐）
- `scripts/publish.sh`：一键发布（构建 + 重签 + GitHub/Gitee 双平台）
- `scripts/resign.sh`：Apple Development 证书重签
- `latest.json`：仓库根目录，Gitee updater 兜底（`.gitignore` 排除，需 `git add -f`）

## 品牌规范

- **显示名**：CC View（UI 文字、菜单、通知）
- **技术名**：cc-view（repo、bundle identifier、npm 包名）
