# cc-view Plan 9: 检查更新（tauri-plugin-updater） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax. **Task 1/4/5 标注 [controller]——由主控执行（涉及凭证/对外操作），不 dispatch subagent。**

**Goal:** tray「检查更新」占位 → 基于 tauri-plugin-updater 的端到端更新（检查/下载/安装/重启），GitHub Releases 托管 `latest.json` + ed25519 签名 bundle。

**Architecture:** 基于官方文档（https://v2.tauri.app/plugin/updater/），**改用前端 JS API**（`@tauri-apps/plugin-updater` 的 `check` + `downloadAndInstall` + `@tauri-apps/plugin-process` 的 `relaunch`），替代 spec §2.3 设想的 Rust `check_update`/`install_update` command——JS API 是文档推荐方式，无需自写 command、更简洁。`tauri signer generate` 生成 ed25519 密钥；`tauri.conf.json` `plugins.updater`（pubKey + endpoint）+ `bundle.createUpdaterArtifacts:true`；`capabilities` `updater:default` + `process:default`。Preferences.vue 加更新区；tray「检查更新」菜单 → `open_prefs`。发布：建 GitHub repo `Ethanzyc/cc-view`（公开）+ push + `tauri build` 签名 + 生成 `latest.json` + `gh release v0.1.0`。

**Tech Stack:** tauri-plugin-updater + tauri-plugin-process（Rust + npm）、ed25519 签名、GitHub Releases、gh CLI。

## Global Constraints

- macOS；代码英文 / 注释中文；fail fast；**签名强制（不可禁）**。
- **私钥 `~/.tauri/cc-view.key` 绝不入库**；公钥写 `tauri.conf.json`（内容，非路径）。
- `createUpdaterArtifacts: true`（cc-view 是 v2 新项目，非迁移——不用 `"v1Compatible"`）。
- endpoint：`https://github.com/Ethanzyc/cc-view/releases/latest/download/latest.json`。
- **前端 JS API**（check/downloadAndInstall/relaunch），不自写 Rust command（偏离 spec §2.3，基于官方文档优化）。
- [controller] task 由主控执行（凭证/对外操作），不 dispatch subagent。
- dev 模式 updater 端到端不可用（需 build 后 `.app` + 已发布 release）。

## File Structure

- `src-tauri/Cargo.toml` —— `+ tauri-plugin-updater = "2"` + `tauri-plugin-process = "2"`
- `package.json` —— `+ @tauri-apps/plugin-updater` + `@tauri-apps/plugin-process`
- `src-tauri/tauri.conf.json` —— `bundle.createUpdaterArtifacts: true` + `plugins.updater`
- `src-tauri/capabilities/default.json` —— `+ updater:default` + `process:default`
- `src-tauri/src/lib.rs` —— `.plugin(updater)` + `.plugin(process)`；菜单 `update_item` enabled；菜单事件 `update` → `open_prefs`
- `src/components/Preferences.vue` —— 加「检查更新」区（check/downloadAndInstall/relaunch）

---

### Task 1: 生成 ed25519 签名密钥 [controller]

**Files:** 无项目文件（私钥落 `~/.tauri/cc-view.key`，公钥记入 Task 2）

- [ ] **Step 1: 生成密钥对**

Run:
```bash
npm run tauri signer generate -- -w ~/.tauri/cc-view.key
```
按提示设置密码（**建议无密码**以简化后续 build/release，spec 已确认）。输出含：
- `public key`（一段 base64，形如 `dW50cnVzdGVkIGNvbW1l...`）—— **记录它，Task 2 填 tauri.conf.json**
- 私钥写入 `~/.tauri/cc-view.key`（+ `.key.pub`）

- [ ] **Step 2: 确认私钥不入库**

确认 `~/.tauri/cc-view.key` 在 home 目录（不在 repo）。repo 的 `.gitignore` 无需改（私钥不在 repo 内）。若后续 CI 需用，通过 secret 注入。

- [ ] **Step 3: 记录公钥到 ledger**

把公钥粘贴进 progress ledger（Task 2 主控会传给 subagent）：
```
- P9-T1 密钥: pubkey="<粘贴公钥>"
```

---

### Task 2: updater + process 依赖 + 配置 + 插件 + 菜单

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `package.json`（npm install）
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 的公钥（主控在 dispatch 时作为 verbatim 值传入）
- Produces: updater + process 插件注册；`update_item` enabled；菜单事件 `update` → `open_prefs`

- [ ] **Step 1: Cargo 依赖**

`src-tauri/Cargo.toml` `[dependencies]` 加（紧跟 autostart）：
```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

- [ ] **Step 2: npm 依赖**

Run: `npm install @tauri-apps/plugin-updater @tauri-apps/plugin-process`

- [ ] **Step 3: tauri.conf.json updater 配置**

`tauri.conf.json` 加两处：
1. `bundle` 对象加 `"createUpdaterArtifacts": true`：
```json
"bundle": {
  "createUpdaterArtifacts": true,
  "active": true,
  ...（其余不变）
}
```
2. 顶层加 `plugins`（与 `app`/`bundle` 同级）：
```json
"plugins": {
  "updater": {
    "pubkey": "<TASK1_PUBLIC_KEY>",
    "endpoints": [
      "https://github.com/Ethanzyc/cc-view/releases/latest/download/latest.json"
    ]
  }
}
```
> `<TASK1_PUBLIC_KEY>` 由主控在 dispatch 时用 Task 1 的实际公钥替换（verbatim）。

- [ ] **Step 4: capabilities 权限**

`capabilities/default.json` `permissions` 数组加：
```json
"updater:default",
"process:default"
```
（`updater:default` 含 allow-check/download/install/download-and-install；`process:default` 含 relaunch）

- [ ] **Step 5: lib.rs 插件注册 + 菜单**

builder 链 `.plugin(...)` 区加（紧跟 autostart plugin）：
```rust
.plugin(tauri_plugin_updater::init())
.plugin(tauri_plugin_process::init())
```
> 核对：v2 插件 init 函数名（`tauri_plugin_updater::init` / `tauri_plugin_process::init`）。若为 `Builder::new().build()` 形式，用文档版。

`update_item` 构造的 `enabled` 由 `false` 改 `true`。

菜单事件 match 加 `"update"` 分支：
```rust
"update" => open_prefs(app),
```
（复用 P8-T3 的 `open_prefs`——tray 检查更新 = 打开偏好设置，更新区在 Preferences.vue 内）

- [ ] **Step 6: 编译确认**

Run: `cargo build --manifest-path src-tauri/Cargo.toml` + `npm run build`
Expected: 通过。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs package.json package-lock.json
git commit -m "feat(updater): tauri-plugin-updater + process 配置（pubKey + endpoint + capabilities + 菜单）"
```

---

### Task 3: Preferences.vue 更新区

**Files:**
- Modify: `src/components/Preferences.vue`

**Interfaces:**
- Consumes: `@tauri-apps/plugin-updater` 的 `check` / `Update`；`@tauri-apps/plugin-process` 的 `relaunch`
- Produces: 「检查更新」按钮 + 版本/notes 显示 + 下载安装流程

- [ ] **Step 1: 加更新区 UI + 逻辑**

在 Preferences.vue `<script setup>` 加 import + 状态 + 函数；`<template>` 在 4 设置项 section 之后加更新区。

`<script setup>` 顶部加：
```ts
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
```

加状态（与现有 `saving`/`error` 同级）：
```ts
const appVersion = __APP_VERSION__; // 见 Step 2 注入
const checking = ref(false);
const updateAvailable = ref<Update | null>(null);
const upToDate = ref(false);
const installing = ref(false);
const installError = ref<string | null>(null);
```

加函数：
```ts
// 检查更新：check() 返回 Update（有更新）或 null（已是最新）
async function checkForUpdates() {
  error.value = null;
  checking.value = true;
  upToDate.value = false;
  updateAvailable.value = null;
  try {
    const upd = await check();
    if (upd) updateAvailable.value = upd;
    else upToDate.value = true;
  } catch (e: unknown) {
    error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '检查失败';
  } finally {
    checking.value = false;
  }
}

// 下载并安装 + 重启
async function downloadAndInstall() {
  if (!updateAvailable.value) return;
  installing.value = true;
  installError.value = null;
  try {
    await updateAvailable.value.downloadAndInstall();
    await relaunch();
  } catch (e: unknown) {
    installError.value = typeof e === 'string' ? e : (e as Error)?.message ?? '安装失败';
    installing.value = false;
  }
}
```

`<template>` 在 settings section 之后、error `<p>` 之前加：
```html
<section class="update-section">
  <div class="row">
    <span>版本 cc-view {{ appVersion }}</span>
    <button @click="checkForUpdates" :disabled="checking">
      {{ checking ? '检查中…' : '检查更新' }}
    </button>
  </div>
  <p v-if="upToDate" class="muted">已是最新版本</p>
  <div v-if="updateAvailable" class="update-detail">
    <p>发现新版本 {{ updateAvailable.version }}</p>
    <pre v-if="updateAvailable.body">{{ updateAvailable.body }}</pre>
    <button @click="downloadAndInstall" :disabled="installing">
      {{ installing ? '安装中…' : '下载并安装' }}
    </button>
  </div>
  <p v-if="installError" class="error">⚠ {{ installError }}</p>
</section>
```

`<style scoped>` 加（与现有风格一致）：
```css
.update-section { margin-top: 20px; padding-top: 16px; border-top: 1px solid var(--color-border); }
.update-detail { margin-top: 12px; padding: 12px; background: var(--color-hover); border-radius: 8px; }
.update-detail pre { white-space: pre-wrap; margin: 8px 0; font-size: 12px; }
```

- [ ] **Step 2: 注入 appVersion**

`__APP_VERSION__` 从 Cargo.toml version（0.1.0）注入。在 `vite.config.ts` 的 `define` 加（若无 vite.config，用 `import.meta.env` 或硬编码——**核对项目 vite 配置**）。最简：Preferences.vue 直接 `import { getVersion } from '@tauri-apps/api/app'; const appVersion = ref(''); onMounted(async () => appVersion.value = await getVersion());`（替代 `__APP_VERSION__`，无需 vite 改动）。**采用 getAppVersion 方案**：
```ts
import { getVersion } from '@tauri-apps/api/app';
const appVersion = ref('');
// onMounted 末尾加： appVersion.value = await getVersion();
```
（删去 `__APP_VERSION__` 写法，用 ref + getVersion）

- [ ] **Step 3: 类型检查**

Run: `npm run build`
Expected: 通过（vue-tsc 校验 Update 类型、check/relaunch 签名）。

- [ ] **Step 4: Commit**

```bash
git add src/components/Preferences.vue
git commit -m "feat(updater): Preferences.vue 检查更新区（check/downloadAndInstall/relaunch）"
```

---

### Task 4: 建 GitHub repo + push [controller]

**Files:** 无（git remote + push）

- [ ] **Step 1: 建 repo**

Run:
```bash
gh repo create Ethanzyc/cc-view --public --source=. --remote=origin --description "Cross-terminal Claude Code session monitor for macOS menubar"
```
（`--source=.` 用当前目录，`--remote=origin` 自动加 remote，`--push` 可选——下面单独 push）

- [ ] **Step 2: push main**

Run:
```bash
git push -u origin main
```
确认 GitHub 上 repo 创建成功、代码上传。

---

### Task 5: 首次 release v0.1.0 [controller]

**Files:** 生成 build 产物 + latest.json（上传到 GitHub Release，不入库）

- [ ] **Step 1: 签名 build**

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/cc-view.key)"
# 若 Task 1 设了密码：export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="..."
npm run tauri build
```
Expected: `src-tauri/target/release/bundle/macos/cc-view.app.tar.gz` + `.cc-view.app.tar.gz.sig` 生成（createUpdaterArtifacts:true）。

- [ ] **Step 2: 读 .sig 内容**

```bash
cat src-tauri/target/release/bundle/macos/cc-view.app.tar.gz.sig
```
记录签名内容（填 latest.json `signature`）。

- [ ] **Step 3: 确定架构 + 上传 bundle 到 release 先**

本机架构：`uname -m`（apple silicon = arm64 → darwin-aarch64；Intel = x86_64 → darwin-x86_64）。cc-view 首个 release 只填本机架构（另一架构留空或后续补 universal build）。

- [ ] **Step 4: 创建 release + 上传 bundle**

```bash
gh release create v0.1.0 \
  src-tauri/target/release/bundle/macos/cc-view.app.tar.gz \
  src-tauri/target/release/bundle/macos/cc-view.app.tar.gz.sig \
  --title "cc-view v0.1.0" \
  --notes "首个公开发布。偏好设置 + 命令面板 + menubar 聚合 + 通知 + 隐藏/搁置 + focus。"
```

- [ ] **Step 5: 生成并上传 latest.json**

本机架构变量 `$ARCH`（darwin-aarch64 或 darwin-x86_64）。bundle 在 release 的下载 URL：
`https://github.com/Ethanzyc/cc-view/releases/download/v0.1.0/cc-view.app.tar.gz`

写 `latest.json`（本地临时文件）：
```json
{
  "version": "0.1.0",
  "notes": "首个公开发布。",
  "pub_date": "<RFC3339 日期，date -u +%Y-%m-%dT%H:%M:%SZ>",
  "platforms": {
    "<ARCH>": {
      "signature": "<STEP2 的 .sig 全部内容>",
      "url": "https://github.com/Ethanzyc/cc-view/releases/download/v0.1.0/cc-view.app.tar.gz"
    }
  }
}
```

上传到 release：
```bash
gh release upload v0.1.0 latest.json --clobber
```

- [ ] **Step 6: 验证 endpoint 可达**

```bash
curl -sL https://github.com/Ethanzyc/cc-view/releases/latest/download/latest.json | head
```
Expected: 返回 latest.json 内容（version/platforms/signature/url）。

> 首个 release v0.1.0 时，已装 v0.1.0 的用户检查更新会得到"已是最新"（无更高版本）。真正的"有更新"链路在 v0.1.1 验证。若需立即验证更新流程，临时把本地 Cargo.toml version 降到 `0.1.0-rc` 再 build 检查。

---

## Self-Review 结论

- **Spec coverage**：§2.1 依赖配置 → Task 2；§2.2 签名密钥 → Task 1；§2.3 流程（check/download/install/relaunch）→ Task 3（JS API 替代 Rust command，架构说明已标注偏离）；§2.4 发布流程 → Task 4+5。✅
- **JS API 偏离说明**：spec §2.3 设想 Rust check_update/install_update command；Plan 基于官方文档改用前端 JS API（check/downloadAndInstall/relaunch），更简洁、文档推荐。capabilities `updater:default` + `process:default` 覆盖前端权限。✅
- **Placeholder scan**：Task 2 Step 3 `<TASK1_PUBLIC_KEY>` 由主控 verbatim 替换（Task 1 产出）；Task 5 `<ARCH>`/`<STEP2 sig>` 由主控执行时填——非占位，是跨 controller-task 的动态值。Task 3 Step 2 明确选用 getAppVersion 方案（删 __APP_VERSION__ 写法）。✅
- **Type consistency**：`check()` → `Update | null`；`update.downloadAndInstall()`；`relaunch()`——签名与文档一致。`getVersion()` from `@tauri-apps/api/app`。✅
- **风险**：Task 5 首个 release 只覆盖本机架构（非 universal）；updater init 函数名核对（Task 2 Step 5）；私钥密码若设置需在 build 时注入（Task 5 Step 1）。
