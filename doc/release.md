# 发布流程

cc-view 手动发布（无 CI）。每个版本产出**两个架构**的 dmg + updater artifacts，
同步发布到 **GitHub + Gitee** 双平台（Gitee 为国内网络兜底）。

## 架构说明

| 架构 | dmg | updater platform key | 状态 |
|------|-----|----------------------|------|
| Apple Silicon | `cc-view_<ver>_aarch64.dmg` | `darwin-aarch64` | 已验证 |
| Intel | `cc-view_<ver>_x86_64.dmg` | `darwin-x86_64` | **未经实机测试** |

## 双平台 + updater 兜底架构

```
updater endpoints (tauri.conf.json):
  1. github.com/.../releases/latest/download/latest.json   ← 主（GitHub 有 latest 重定向）
  2. gitee.com/.../raw/main/latest.json                     ← 兜底（Gitee 无 latest 重定向，文件提交到仓库）
```

Tauri v2 updater 按序尝试 endpoints，首个网络失败自动 fallback 到下一个。

| 平台 | latest.json 来源 | updater 包 URL |
|------|-----------------|----------------|
| GitHub | release asset（`releases/latest/download/latest.json` 自动重定向到最新版） | `github.com/.../releases/download/v<ver>/...` |
| Gitee | 仓库根目录 `latest.json`（`raw/main/latest.json`，每次发版 commit 更新） | `gitee.com/.../releases/download/v<ver>/...` |

## 一键发布

```bash
# 1. 改版本号（Cargo.toml + tauri.conf.json），commit + push
# 2. 运行发布脚本
./scripts/publish.sh 0.5.1
```

脚本自动完成：构建双架构 → 打 DMG → GitHub release → Gitee release → 更新 latest.json → commit + push。

## 手动发布（脚本内部步骤）

以版本 `0.5.0`、tag `v0.5.0` 为例。

### 前置（一次性）

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

updater 签名密钥——构建时设环境变量（密钥本身不进仓库）：

- `TAURI_SIGNING_PRIVATE_KEY`：minisign 私钥（`~/.tauri/cc-view.key`）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：空（本项目密钥无密码）

Gitee token：`~/.git-credentials` 中 gitee.com 条目的密码部分。

### 1. 改版本号

同步两处：`src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version`。`cargo generate-lockfile` 更新 Cargo.lock。commit + push。

### 2. 构建两个架构

```bash
TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/cc-view.key) TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  npm run tauri build -- --target aarch64-apple-darwin --bundles app

TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/cc-view.key) TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  npm run tauri build -- --target x86_64-apple-darwin --bundles app
```

### 3. 打 DMG

```bash
./scripts/make-dmg.sh aarch64
./scripts/make-dmg.sh x86_64
```

### 4. GitHub release

```bash
gh release create v0.5.0 --latest --title "v0.5.0" --notes "..."
gh release upload v0.5.0 \
  cc-view_0.5.0_aarch64.dmg cc-view_0.5.0_x86_64.dmg \
  <aarch64 updater 包 + sig> \
  <x86_64 updater 包 + sig> \
  github-latest.json#latest.json   # GitHub 版 latest.json（URL 指向 GitHub）
```

### 5. Gitee release

```bash
# 创建 release（或用已有的 tag 自动创建的）
GITEE_TOKEN=$(grep gitee.com ~/.git-credentials | head -1 | sed 's|.*:\([^@]*\)@.*|\1|')
GITEE_API="https://gitee.com/api/v5/repos/Ethanzyc/cc-view/releases/<id>/attach_files"

curl -X POST "${GITEE_API}?access_token=${GITEE_TOKEN}" -F "file=@<file>" -F "name=<name>"
# 上传所有 assets（dmg + updater 包 + sig），重命名带后缀避免覆盖
```

### 6. 更新 latest.json 到仓库（Gitee updater 兜底）

仓库根目录的 `latest.json`（`.gitignore` 排除了，用 `git add -f`）：
URL 指向 Gitee release assets。commit + push（双推自动同步到 GitHub + Gitee）。

## 注意事项

- **updater 包命名**：双架构起，updater 包必须带架构后缀（`cc-view_aarch64.app.tar.gz` / `cc-view_x86_64.app.tar.gz`），否则同名覆盖。
- **Gitee latest.json**：提交到仓库根目录（`raw/main/latest.json`），每次发版更新。`.gitignore` 排除 `latest.json`（本地构建产物），需 `git add -f`。
- **git 双推**：`git push origin` 自动推 GitHub + Gitee（`origin` 配了双 push URL）。
- **macOS Gatekeeper**：.app 未做 Apple 公证，用户需 `xattr -dr com.apple.quarantine`。
