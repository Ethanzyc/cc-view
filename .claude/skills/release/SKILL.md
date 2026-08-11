---
name: release
description: 发布 cc-view 新版本——版本号 bump、双架构构建、GitHub+Gitee 双平台 release、updater latest.json 同步
---

# cc-view 发布

## 何时使用

用户说"发布"、"发版"、"release"、"打新版本"时调用此 skill。

## 前置检查

1. **确认 CI 绿**：`gh run list --limit 3`，最近的 push CI 必须是 success
2. **确认工作区干净**：`git status`，不能有未提交的改动
3. **确认当前版本号**：`grep '"version"' src-tauri/tauri.conf.json`

## 发布步骤

### 1. 确定版本号

- patch（bug fix）：`0.5.0 → 0.5.1`
- minor（新功能）：`0.5.0 → 0.6.0`
- 与用户确认版本号

### 2. Bump 版本号

同步改两处 + 更新 Cargo.lock：

```
src-tauri/tauri.conf.json → "version": "x.y.z"
src-tauri/Cargo.toml      → version = "x.y.z"
cargo generate-lockfile（在 src-tauri/ 目录）
```

提交 + push（双推自动同步 GitHub + Gitee）：

```
git add -A && git commit -m "release: vX.Y.Z 版本号 bump" && git push
```

### 3. 一键发布

```bash
./scripts/publish.sh X.Y.Z
```

脚本自动完成：
- 构建双架构（aarch64 + x86_64）+ 签名
- 打 DMG（`scripts/make-dmg.sh`）
- GitHub release + assets + latest.json
- Gitee release + assets + latest.json
- 更新仓库根目录 `latest.json`（Gitee updater 兜底用）+ commit + push

### 4. 验证

```bash
# GitHub assets
gh release view vX.Y.Z --json assets --jq '.assets[].name'

# Gitee raw latest.json 可访问
curl -sL "https://gitee.com/Ethanzyc/cc-view/raw/main/latest.json" | python3 -m json.tool | head -5
```

## 常见问题

- **CI 失败**：先修 CI 再发版。常见原因：`cargo fmt --check`（跑 `cargo fmt`）、测试依赖真实环境（如 tty map 在 CI 无终端）。
- **publish.sh 失败**：脚本支持断点续跑——GitHub/Gitee release 已存在时跳过创建，`--clobber` 覆盖 assets。
- **Gitee 新仓库需改公开**：`PATCH /api/v5/repos/{owner}/{repo}` 设 `{"name":"cc-view","private":false}`。

## 架构说明

| 组件 | 详情 |
|------|------|
| 双平台 | GitHub（主）+ Gitee（兜底，国内网络） |
| updater fallback | tauri.conf.json endpoints 数组按序尝试：GitHub → Gitee |
| GitHub latest.json | release asset（`releases/latest/download/latest.json` 自动重定向） |
| Gitee latest.json | 仓库根目录（`raw/main/latest.json`，每次发版 commit 更新） |
| 签名密钥 | `~/.tauri/cc-view.key`（无密码） |
| Gitee token | `~/.git-credentials` 中 gitee.com 条目的密码部分 |

详细流程见 `doc/release.md`。
