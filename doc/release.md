# 发布流程

cc-view 手动发布（无 CI）。每个版本产出**两个架构**的 dmg + updater artifacts。

## 架构说明

| 架构 | dmg | updater platform key | 状态 |
|------|-----|----------------------|------|
| Apple Silicon | `cc-view_<ver>_aarch64.dmg` | `darwin-aarch64` | 已验证 |
| Intel | `cc-view_<ver>_x86_64.dmg` | `darwin-x86_64` | **未经实机测试** |

> x86_64 包由 Apple Silicon 开发机**交叉编译**产出。依赖均为纯 Rust + macOS 系统框架绑定（无 C 库、无 sidecar），交叉编译无障碍；但未在真实 Intel Mac 上运行验证。遇到问题请[提 issue](https://github.com/Ethanzyc/cc-view/issues)。

## 前置（一次性）

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

updater 签名密钥——构建时设环境变量（密钥本身不进仓库）：

- `TAURI_SIGNING_PRIVATE_KEY`：minisign 私钥
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：空（本项目密钥无密码）

## 每次发版

以版本 `0.3.1`、tag `v0.3.1` 为例。

### 1. 改版本号

同步两处：`src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version`。

### 2. 构建两个架构

```bash
# Apple Silicon
TAURI_SIGNING_PRIVATE_KEY=<key> TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  npm run tauri build -- --target aarch64-apple-darwin --bundles app

# Intel（交叉编译）
TAURI_SIGNING_PRIVATE_KEY=<key> TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  npm run tauri build -- --target x86_64-apple-darwin --bundles app
```

每个架构产出三样（在 `src-tauri/target/<triple>/release/bundle/macos/`）：

- `cc-view.app` — 应用本体
- `cc-view.app.tar.gz` — updater 增量包
- `cc-view.app.tar.gz.sig` — minisign 签名

### 3. 打 dmg

```bash
./scripts/make-dmg.sh aarch64   # → cc-view_0.3.1_aarch64.dmg
./scripts/make-dmg.sh x86_64    # → cc-view_0.3.1_x86_64.dmg
```

### 4. 上传 release assets

两个架构的 updater 包都叫 `cc-view.app.tar.gz`，**上传前必须重命名**避免互相覆盖：

| 源文件 | 上传为 |
|--------|--------|
| `target/aarch64-apple-darwin/.../cc-view.app.tar.gz` | `cc-view_aarch64.app.tar.gz` |
| `target/x86_64-apple-darwin/.../cc-view.app.tar.gz` | `cc-view_x86_64.app.tar.gz` |
| `cc-view_0.3.1_aarch64.dmg` | 同名 |
| `cc-view_0.3.1_x86_64.dmg` | 同名 |

### 5. 写 latest.json

两个 platform 条目；`signature` 取各自 `.sig` 文件的完整内容（一整段 base64）：

```json
{
  "version": "0.3.1",
  "notes": "...",
  "pub_date": "2026-08-10T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<target/aarch64-apple-darwin/.../cc-view.app.tar.gz.sig 的内容>",
      "url": "https://github.com/Ethanzyc/cc-view/releases/download/v0.3.1/cc-view_aarch64.app.tar.gz"
    },
    "darwin-x86_64": {
      "signature": "<target/x86_64-apple-darwin/.../cc-view.app.tar.gz.sig 的内容>",
      "url": "https://github.com/Ethanzyc/cc-view/releases/download/v0.3.1/cc-view_x86_64.app.tar.gz"
    }
  }
}
```

把 `latest.json` 也作为 asset 上传到 release。

### 6. 发布

Release 由 draft 改 published。updater 客户端按自身架构匹配 `darwin-aarch64` / `darwin-x86_64` 条目，自动更新。

> **注意 asset 命名**：从双架构版本起，aarch64 的 updater 包也带后缀（`cc-view_aarch64.app.tar.gz`），不再是裸 `cc-view.app.tar.gz`——否则与 x86_64 同名覆盖。新 `latest.json` 整体替换旧文件，对存量 aarch64 用户的下一次更新无影响。
