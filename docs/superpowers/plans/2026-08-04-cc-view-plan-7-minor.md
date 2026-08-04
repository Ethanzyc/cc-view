# cc-view Plan 7: Minor 收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 补全 `overlay_position` 的文件 I/O 单测（路径参数化）、前端 `hidden` 改 `Set`、清理 `lib.rs` 提已删 HUD/main 窗口的过时注释。

**Architecture:** `overlay_position.rs` 抽取 `load_from`/`save_to`/`save_all_to` 路径参数化方法（便捷版 `load`/`save`/`save_all` 签名不变 → `lib.rs` 调用零改动），`tempfile` 隔离测 `save` 保留 pinned 与写盘往返；`Overlay.vue` 的 `hidden` 从 `ref<string[]>` 改 `ref<Set<string>>`、`.includes` → `.has`；`lib.rs` 4 处注释块清理。

**Tech Stack:** Rust（`cargo test`）、`tempfile` dev-dep、Vue 3（`npm run build` 做类型检查）。

## Global Constraints

- macOS；代码英文 / 注释中文；fail fast；DRY/YAGNI。
- `overlay_position` 便捷版 `load()` / `save(x,y)` / `save_all(x,y,pinned)` **签名不变**——`lib.rs` 现有调用（`save(p.x,p.y)` L602、`save_all(...)` L330、`load()` L335/474/593）必须零改动兼容。
- 前端无单测框架：`hiddenSet` 改动用 `npm run build`（vue-tsc 类型检查）+ 冒烟验证，**不引入测试框架**。

## File Structure

- `src-tauri/Cargo.toml` —— 新增 `[dev-dependencies] tempfile`
- `src-tauri/src/overlay_position.rs` —— 路径参数化 + 3 个新测试（整体重写，文件小）
- `src/components/Overlay.vue` —— `hidden` 改 `ref<Set<string>>`，`.includes` → `.has`
- `src-tauri/src/lib.rs` —— 4 处过时注释清理

---

### Task 1: overlay_position 路径参数化 + 单测补全

**Files:**
- Modify: `src-tauri/Cargo.toml`（加 `[dev-dependencies] tempfile`）
- Modify: `src-tauri/src/overlay_position.rs`（参数化 + 测试，整体重写）
- Test: `src-tauri/src/overlay_position.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `OverlayPosition::load_from(path: &Path) -> Option<Self>`、`save_to(path: &Path, x: i32, y: i32)`、`save_all_to(path: &Path, x: i32, y: i32, pinned: bool)`（新，单测用）；`load()` / `save(i32,i32)` / `save_all(i32,i32,bool)` 签名不变。

- [ ] **Step 1: 加 tempfile dev-dependency**

`src-tauri/Cargo.toml` 末尾追加（当前文件无 `[dev-dependencies]` 段）：
```toml

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: 写新测试（先红）**

整体重写 `src-tauri/src/overlay_position.rs` 为下面内容。测试调用的 `save_all_to` / `save_to` / `load_from` 此刻还不存在 → 编译失败（红）。

```rust
// overlay 窗口位置 + pin 持久化：load/save 读写 ~/.claude/cc-view/overlay-position.json。
// 用户拖动 overlay 后存位，下次呼出恢复——不再每次 center。pin（失焦是否收起）一并持久化。
// 路径参数化（load_from/save_to/save_all_to）供单测用 tempdir 隔离；便捷版 load/save/save_all
// 走默认路径，签名不变（lib.rs 调用兼容）。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// pinned 的 serde 默认值：false（开机隐藏 + 呼出默认未钉 = 失焦收起）。
fn default_pinned() -> bool {
    false
}

const FILENAME: &str = "overlay-position.json";

/// 默认配置目录 ~/.claude/cc-view（无 home 时 None）。
fn default_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude/cc-view"))
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OverlayPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_pinned")]
    pub pinned: bool,
}

impl OverlayPosition {
    /// 从默认路径加载（~/.claude/cc-view/overlay-position.json）；无 home / 文件不存在 / 解析失败 → None。
    pub fn load() -> Option<Self> {
        Self::load_from(&default_dir()?.join(FILENAME))
    }

    /// 从指定路径加载（单测用 tempdir 隔离）。
    pub fn load_from(path: &Path) -> Option<Self> {
        let txt = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// 拖动保存（默认路径）：保留磁盘上已有的 pinned（无值时默认 false）。
    pub fn save(x: i32, y: i32) {
        let Some(dir) = default_dir() else { return };
        Self::save_to(&dir.join(FILENAME), x, y);
    }

    /// 拖动保存（指定路径）：保留该文件已有的 pinned（单测用）。
    pub fn save_to(path: &Path, x: i32, y: i32) {
        let pinned = Self::load_from(path).map(|p| p.pinned).unwrap_or(false);
        Self::save_all_to(path, x, y, pinned);
    }

    /// 显式保存完整位置（含 pinned），默认路径，供 set_overlay_pinned command 调用。
    pub fn save_all(x: i32, y: i32, pinned: bool) {
        let Some(dir) = default_dir() else { return };
        Self::save_all_to(&dir.join(FILENAME), x, y, pinned);
    }

    /// 显式保存完整位置（指定路径，含建父目录），单测用。
    pub fn save_all_to(path: &Path, x: i32, y: i32, pinned: bool) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&OverlayPosition { x, y, pinned }) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn serde_roundtrip() {
        let pos = OverlayPosition { x: 100, y: 200, pinned: true };
        let json = serde_json::to_string(&pos).unwrap();
        let back: OverlayPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, 100);
        assert_eq!(back.y, 200);
        assert!(back.pinned);
    }

    #[test]
    fn old_json_without_pinned_defaults_false() {
        // 向后兼容：无 pinned 字段时默认 false（区别于旧 hud-position.json 的 true）。
        let old = r#"{"x":42,"y":99}"#;
        let pos: OverlayPosition = serde_json::from_str(old).unwrap();
        assert_eq!(pos.x, 42);
        assert_eq!(pos.y, 99);
        assert!(!pos.pinned);
    }

    #[test]
    fn load_invalid_json_returns_none() {
        let pos: Option<OverlayPosition> = serde_json::from_str("not json").ok();
        assert!(pos.is_none());
    }

    #[test]
    fn save_all_to_roundtrip() {
        // save_all_to 写盘 → load_from 读回，字段全等。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_all_to(&path, 1, 2, true);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y, pos.pinned), (1, 2, true));
    }

    #[test]
    fn save_to_preserves_existing_pinned() {
        // 磁盘已有 pinned:true，save_to(新坐标) 必须保留 pinned（拖动不改 pin 的核心不变量）。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_all_to(&path, 5, 5, true);
        OverlayPosition::save_to(&path, 10, 20);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y), (10, 20));
        assert!(pos.pinned, "save_to must preserve existing pinned");
    }

    #[test]
    fn save_to_defaults_pinned_false_when_no_file() {
        // 无文件时 save_to，pinned 默认 false（新装用户首次拖动）。
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(FILENAME);
        OverlayPosition::save_to(&path, 7, 8);
        let pos = OverlayPosition::load_from(&path).expect("should load");
        assert_eq!((pos.x, pos.y), (7, 8));
        assert!(!pos.pinned);
    }
}
```

- [ ] **Step 3: 跑测试确认红（方法尚不存在 → 编译失败）**

Run: `cargo test --manifest-path src-tauri/Cargo.toml overlay_position`
Expected: 编译失败（`save_all_to` / `save_to` / `load_from` 在 Step 2 重写后已存在 —— 实际这步验证重写落地后测试能编译；若仍引用旧 API 则 FAIL）。> 注：因 Step 2 已整体重写含实现，此步实际为"编译 + 跑测试"——若重写正确则直接绿。如需严格 TDD 红→绿，可先只加测试不加 impl，但本任务重构幅度小，整体重写后一次跑通更稳。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test --manifest-path src-tauri/Cargo.toml overlay_position`
Expected: 6 个测试全 PASS（原 3 + 新 3）。

- [ ] **Step 5: 确认 lib.rs 调用兼容（零改动）**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过（`save(p.x,p.y)` / `save_all(...)` / `load()` 便捷版签名未变）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/overlay_position.rs
git commit -m "test(overlay_position): 路径参数化 + 补 save 保留 pinned/写盘往返单测"
```

---

### Task 2: hiddenSet（前端 hidden 改 Set）

**Files:**
- Modify: `src/components/Overlay.vue`

**Interfaces:**
- Consumes: `invoke<string[]>('list_hidden')`（后端不变）
- Produces: `hidden: Ref<Set<string>>`；模板 / 脚本用 `.has(s.id)` 替代 `.includes(s.id)`。

> 注意：`Section.hidden: number`（Overlay.vue:61/68/84-86/401-402）是"已退出"分组折叠计数，与 hiddenSet 无关，**不动**。`.includes(s.id)` 模式不会误伤 `section.hidden`。

- [ ] **Step 1: hidden ref 类型改 Set**

Edit `src/components/Overlay.vue`：
- old: `const hidden = ref<string[]>([]);`
- new: `const hidden = ref<Set<string>>(new Set());`

- [ ] **Step 2: visible 过滤改 .has**

- old: `  showHidden.value ? all.value : all.value.filter(s => !hidden.value.includes(s.id)),`
- new: `  showHidden.value ? all.value : all.value.filter(s => !hidden.value.has(s.id)),`

- [ ] **Step 3: 两处赋值改 new Set（replace_all）**

`refreshHidden`（L167）与 `onMounted`（L196）都是 `hidden.value = await invoke<string[]>('list_hidden');`，整体替换触发响应式：
- old: `  hidden.value = await invoke<string[]>('list_hidden');`
- new: `  hidden.value = new Set(await invoke<string[]>('list_hidden'));`
- `replace_all: true`（两处一并改）

- [ ] **Step 4: 模板 .includes → .has（replace_all）**

模板里 `hidden.includes(s.id)` 共 10 处（L272/304/321/322/323/353/372/389/390/391），Vue 模板自动解包 ref，Set 同样 `.has`：
- old: `hidden.includes(s.id)`
- new: `hidden.has(s.id)`
- `replace_all: true`

- [ ] **Step 5: 类型检查**

Run: `npm run build`
Expected: 通过（vue-tsc：`Set.has` 与 `Array.includes` 都返回 `boolean`，类型兼容；`ref<Set<string>>` 赋值 `new Set(...)` 合法）。

- [ ] **Step 6: 冒烟**

Run: `npm run tauri dev`
验证步骤：
1. ⌥Space 呼出 overlay
2. 某行点「隐藏」→ 该行立即从列表消失（`visible` 过滤生效）
3. 顶栏勾选「显示已隐藏」→ 被隐藏的行重新出现且灰显（`is-hidden` class）
4. 灰行点「取消隐藏」→ 恢复正常显示
5. 取消勾选「显示已隐藏」→ 隐藏的行再次过滤掉
Expected: 全部正常，无 console 报错。

- [ ] **Step 7: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "refactor(overlay): hidden 列表改 Set，.includes→.has（O(1) 查询）"
```

---

### Task 3: 过时注释清理（lib.rs 提已删 HUD/main）

**Files:**
- Modify: `src-tauri/src/lib.rs`

> HUD "main" 窗口已在 commit 405adfd 删除，overlay 成为唯一 UI。lib.rs 仍有多处注释提及 HUD/main，语义过时。本任务清理 4 处（基于通读 lib.rs 全文识别）。

- [ ] **Step 1: join_all_spaces doc 末行删 HUD 引用**

- old: `/// 合计 = 1 | 256 = 257。仅对 overlay 调；HUD（main）保持默认（不跨全屏，避免干扰沉浸）。`
- new: `/// 合计 = 1 | 256 = 257。`

- [ ] **Step 2: 删 make_key 过渡注释整段**

- old:
```
// make_key 已移除：show_overlay 改用 set_focus（激活 app + makeKey）替代。
// 原理见 show_overlay 注释（删 main 后需显式激活 app，否则 WKWebView input 不接受键盘）。

/// 返回当前前台 app 的 bundle id
```
- new:
```
/// 返回当前前台 app 的 bundle id
```

- [ ] **Step 3: show_overlay 注释删 "HUD 已删无牵连"**

- old: `    // key；activateIgnoringOtherApps 激活 app（WKWebView input 需 app active）。HUD 已删无牵连。`
- new: `    // key；activateIgnoringOtherApps 激活 app（WKWebView input 需 app active）。`

- [ ] **Step 4: setup vibrancy 注释改 "与 overlay 视觉一致"**

- old:
```
            // 同时套同款 Popover vibrancy——与 HUD 视觉一致；radius 12 比 main 略大，
            // 命令面板观感更柔和。EffectState::Active 保证失焦时仍保持毛玻璃（不灰化）。
```
- new:
```
            // 同时套同款 Popover vibrancy——与 overlay 视觉一致；radius 12，命令面板观感更柔和。
            // EffectState::Active 保证失焦时仍保持毛玻璃（不灰化）。
```

- [ ] **Step 5: 确认编译（注释改动不影响编译，但兜底）**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 通过。

- [ ] **Step 6: 确认无遗漏**

Run: `git diff src-tauri/src/lib.rs` 人工确认改动仅注释、无逻辑变更。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "docs(lib): 清理提已删 HUD/main 窗口的过时注释"
```

---

## Self-Review 结论

- **Spec coverage**：spec §3.1（overlay_position 单测）→ Task 1；§3.2（hiddenSet）→ Task 2；§3.3（过时注释）→ Task 3。全覆盖。✅
- **Placeholder scan**：无 TBD/TODO；每步含完整代码或确切 Edit old/new。Task 1 Step 3 的"红"说明标注了整体重写的实际情况（非占位，是 TDD 在小重构上的务实变体）。✅
- **Type consistency**：`load_from(&Path)` / `save_to(&Path,i32,i32)` / `save_all_to(&Path,i32,i32,bool)` 在 Task 1 接口块与实现、测试中签名一致；`hidden: Ref<Set<string>>` 在 Task 2 各步骤一致。✅
- **兼容性**：Task 1 便捷版 `load`/`save`/`save_all` 签名不变，lib.rs 零改动（Step 5 验证）。✅
