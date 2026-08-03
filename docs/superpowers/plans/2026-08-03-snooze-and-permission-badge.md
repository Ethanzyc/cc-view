# 搁置(Snooze) + 等权限 Tray Badge 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 cc-view 能手动搁置"暂时不管"的会话（有新动静自动冒泡），并在 menu bar tray icon 上常驻显示等权限计数 badge（不处理不消失）。

**Architecture:** 后端新增 `snoozed.rs`（镜像 `hidden.rs`）管 `~/.claude/cc-view/snoozed.json`，`Session` 加 derived `snoozed` 字段由 poll_loop 每轮用 `is_effectively_snoozed` 算好 emit；前端 `SessionList`/`Overlay` 加分组（一级状态/二级项目）/灰显沉底/搁置按钮/搜索重写；badge 用 `tauri::image::Image` RGBA 像素操作（复用 `tint_orange` 模式）动态合成红圆+数字。

**Tech Stack:** Tauri 2（Rust，edition 2021）+ Vue 3 + TypeScript（vue-tsc 严格）。无新 crate 依赖。

## Global Constraints

- Rust edition 2021；tauri 2 features: `tray-icon`, `macos-private-api`, `image-png`（Cargo.toml 现状，不改）
- **无新 crate 依赖**：badge 合成用 `tauri::image::Image::rgba()` 像素操作，复用 `src-tauri/src/lib.rs:37` `tint_orange` 模式
- macOS ≥ 10.13；`LSUIElement=true` accessory app（无 dock icon，badge 只能画在 menu bar tray icon）
- 模型 serde `rename_all = "camelCase"`；前端 `src/types.ts` 镜像后端 `models::Session`
- dead 上限 **5**；项目二级标题 `~/ai/fang`（`/Users/<user>/` → `~/`）
- 中文注释/交互文案，英文标识符；fail fast（lock poisoned 静默跳过不崩）
- 参考原型 `docs/superpowers/prototypes/snooze-prototype.html`（gstack 已验证分组/聚类/搜索全通过）
- 参考 spec `docs/superpowers/specs/2026-08-03-snooze-and-permission-badge-design.md`

## File Structure

**新增**
- `src-tauri/src/snoozed.rs` — `SnoozeMap`（load/save/add/remove/get/is_effectively_snoozed）+ 单测。镜像 `hidden.rs`。
- `src-tauri/src/badge.rs` — `draw_badge(Image, count) -> Image`（RGBA 画红圆+点阵数字）+ 单测。

**后端修改**
- `src-tauri/src/models.rs` — `Session` 加 `pub snoozed: bool`（derived，`#[serde(default)]` 兼容旧缓存）。
- `src-tauri/src/lib.rs` — poll_loop 算 derived `snoozed` + `perm_count`；tray badge 合成切换；`snooze_session`/`unsnooze_session`/`list_snoozed` 命令；`invoke_handler` 注册 + `.manage(SnoozeMap)`。

**前端修改**
- `src/types.ts` — `Session` 加 `snoozed: boolean`。
- `src/components/SessionList.vue` — 排序加项目维度+搁置档；分组渲染（一级状态/二级项目）；灰显沉底；dead 限5折叠；waitingForInput 行「搁置」按钮。
- `src/components/Overlay.vue` — 补 ago/perm/fresh；搜索重写（扁平+高亮+计数）；搁置按钮。

**不改**：`collector.rs`、`reducer.rs`、`statemachine.rs`、`notify.rs`、`hidden.rs`、`StatusIcon.vue`、`tauri.conf.json`、`Cargo.toml`。

---

### Task 1: snoozed 数据层（models.snoozed + snoozed.rs）

**Files:**
- Modify: `src-tauri/src/models.rs`（`Session` 加 `snoozed` 字段）
- Create: `src-tauri/src/snoozed.rs`
- Test: `src-tauri/src/snoozed.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `models::{Session, Status}`（现有）
- Produces: `snoozed::SnoozeMap`，方法 `load() -> Self`、`save(&self)`、`add(&mut self, id: &str, at: i64)`、`remove(&mut self, id: &str)`、`get(&self, id: &str) -> Option<i64>`、`is_effectively_snoozed(&self, &Session) -> bool`、`to_map(&self) -> HashMap<String,i64>`。Task 2 poll_loop 与命令依赖这些。

- [ ] **Step 1: models.rs 加 snoozed 字段**

修改 `src-tauri/src/models.rs` `Session` struct，在 `focus_hint` 后加（derived，不参与构造逻辑默认值由 poll_loop 设）：

```rust
pub struct Session {
    pub id: String,
    pub source: Source,
    pub pid: u32,
    pub project: String,
    pub cwd: String,
    pub name: String,
    pub status: Status,
    pub started_at: i64,
    pub status_updated_at: i64,
    pub alive: bool,
    pub focus_hint: FocusHint,
    /// derived：由 poll_loop 每轮用 snoozed::is_effectively_snoozed 算，不持久化。
    /// serde default 兼容旧缓存/前端旧版。
    #[serde(default)]
    pub snoozed: bool,
}
```

同步更新 `models.rs` 内所有构造 `Session` 的测试（`mk`/`session_serializes_to_camel_case` 等）补 `snoozed: false`（编译会报错指引逐处补）。

- [ ] **Step 2: 写 snoozed.rs 失败测试**

创建 `src-tauri/src/snoozed.rs`，先只写测试（TDD）：

```rust
use crate::models::{Session, Status};
use std::collections::HashMap;

// SnoozeMap 与 is_effectively_snoozed 待实现

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FocusHint, Source};

    fn sess(id: &str, st: Status, updated_at: i64) -> Session {
        Session {
            id: id.into(), source: Source::Interactive, pid: 1, project: "p".into(),
            cwd: "/c".into(), name: id.into(), status: st, started_at: 0,
            status_updated_at: updated_at, alive: true, focus_hint: FocusHint::default(),
            snoozed: false,
        }
    }

    #[test]
    fn add_then_get() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert_eq!(m.get("a"), Some(1000));
        assert!(m.get("b").is_none());
    }

    #[test]
    fn remove_clears() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        m.remove("a");
        assert!(m.get("a").is_none());
    }

    #[test]
    fn not_snoozed_when_absent() {
        let m = SnoozeMap::empty();
        assert!(!m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 500)));
    }

    #[test]
    fn snoozed_when_stale_status_unchanged() {
        // 搁置时 statusUpdatedAt=1000，之后未变 → 仍有效搁置
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 1000)));
    }

    #[test]
    fn auto_unsnooze_when_new_waiting_input() {
        // 搁置(at=1000)后状态更新(updated_at=2000)且停在 WaitingForInput → 失效冒泡
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(!m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 2000)));
    }

    #[test]
    fn auto_unsnooze_when_new_needs_permission() {
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(!m.is_effectively_snoozed(&sess("a", Status::NeedsPermission, 2000)));
    }

    #[test]
    fn stays_snoozed_when_new_status_is_working() {
        // 搁置后更新但停在 Working（非介入态）→ 仍搁置
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(m.is_effectively_snoozed(&sess("a", Status::Working, 2000)));
    }

    #[test]
    fn boundary_equal_not_stale() {
        // statusUpdatedAt == snoozedAt（同刻）→ 不视为"更新过"，仍搁置
        let mut m = SnoozeMap::empty();
        m.add("a", 1000);
        assert!(m.is_effectively_snoozed(&sess("a", Status::WaitingForInput, 1000)));
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib snoozed`
Expected: 编译失败（`SnoozeMap` 未定义）。

- [ ] **Step 4: 实现 SnoozeMap**

在 `src-tauri/src/snoozed.rs` 顶部（测试之前）加实现，镜像 `hidden.rs` 结构：

```rust
use crate::models::{Session, Status};
use std::collections::HashMap;

/// 会话搁置表：{ session_id: snoozed_at_ms }。持久化到 ~/.claude/cc-view/snoozed.json。
/// 与 hidden.rs 同构，但存时间戳（自动失效需要）。
pub struct SnoozeMap {
    map: HashMap<String, i64>,
}

impl SnoozeMap {
    pub fn empty() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else { return Self::empty(); };
        let path = home.join(".claude/cc-view/snoozed.json");
        let Ok(json) = std::fs::read_to_string(&path) else {
            eprintln!("snoozed load: failed to read ~/.claude/cc-view/snoozed.json");
            return Self::empty();
        };
        Self {
            map: serde_json::from_str(&json).unwrap_or_else(|e| {
                eprintln!("snoozed load: invalid json, ignoring: {e}");
                HashMap::new()
            }),
        }
    }

    pub fn save(&self) {
        let Some(home) = dirs::home_dir() else { return; };
        let dir = home.join(".claude/cc-view");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string(&self.map) {
            let _ = std::fs::write(dir.join("snoozed.json"), json);
        }
    }

    pub fn add(&mut self, id: &str, at: i64) {
        self.map.insert(id.to_string(), at);
    }

    pub fn remove(&mut self, id: &str) {
        self.map.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<i64> {
        self.map.get(id).copied()
    }

    pub fn to_map(&self) -> HashMap<String, i64> {
        self.map.clone()
    }

    /// 有效搁置：有 snoozedAt，且未触发自动失效。
    /// 失效 = 搁置后状态又更新(statusUpdatedAt > snoozedAt) 且停在需要介入的状态
    ///        (WaitingForInput | NeedsPermission) → 自动冒泡回待介入。
    /// 边界：statusUpdatedAt == snoozedAt 不算更新（同刻搁置不立即失效）。
    pub fn is_effectively_snoozed(&self, s: &Session) -> bool {
        let Some(at) = self.map.get(&s.id).copied() else { return false; };
        let stale = s.status_updated_at > at
            && matches!(s.status, Status::WaitingForInput | Status::NeedsPermission);
        !stale
    }
}
```

- [ ] **Step 5: 注册模块 + 跑测试通过**

在 `src-tauri/src/lib.rs` 顶部 `mod hidden;` 旁加 `mod snoozed;`（参考现有 `mod hidden;` 行）。

Run: `cd src-tauri && cargo test --lib snoozed`
Expected: PASS（8 个测试全过）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/snoozed.rs src-tauri/src/models.rs src-tauri/src/lib.rs
git commit -m "feat(snoozed): SnoozeMap 数据层 + Session.snoozed derived 字段"
```

---

### Task 2: lib.rs 集成（poll_loop derived + 命令 + 注册）

**Files:**
- Modify: `src-tauri/src/lib.rs`（命令 155-187 旁加 snooze 命令；poll_loop 88-128 加 derived；355-368 注册/manage）

**Interfaces:**
- Consumes: `snoozed::SnoozeMap`（Task 1）、`models::Session.snoozed`（Task 1）
- Produces: 前端可调 `snooze_session(id)`/`unsnooze_session(id)`/`list_snoozed()`；emit 的 `Session` 含 `snoozed` 字段。

- [ ] **Step 1: 加 snooze 命令（镜像 hide_session）**

在 `src-tauri/src/lib.rs` `list_hidden`（179-187）之后加三命令。注意 `add` 用当前时间（`std::time::SystemTime::now()` ms）：

```rust
// --- Tauri commands：搁置/取消搁置/查询搁置表 ---
// 镜像 hide_session/unhide_session/list_hidden，但存时间戳（is_effectively_snoozed 需要）。

/// 标记会话搁置（记当前时间戳），持久化。前端乐观更新后由 poll_loop 对齐。
#[tauri::command]
fn snooze_session(state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>, id: String) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    match state.lock() {
        Ok(mut m) => {
            m.add(&id, now_ms);
            m.save();
        }
        Err(_) => eprintln!("snooze_session: snoozed state lock poisoned"),
    }
}

/// 取消搁置（手动恢复），持久化。
#[tauri::command]
fn unsnooze_session(state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>, id: String) {
    match state.lock() {
        Ok(mut m) => {
            m.remove(&id);
            m.save();
        }
        Err(_) => eprintln!("unsnooze_session: snoozed state lock poisoned"),
    }
}

/// 返回搁置表 {id: snoozedAt}（前端用于乐观更新/调试）。
#[tauri::command]
fn list_snoozed(state: tauri::State<'_, Mutex<snoozed::SnoozeMap>>) -> std::collections::HashMap<String, i64> {
    state
        .lock()
        .map(|m| m.to_map())
        .unwrap_or_else(|_| {
            eprintln!("list_snoozed: snoozed state lock poisoned");
            std::collections::HashMap::new()
        })
}
```

- [ ] **Step 2: 注册命令 + manage state**

在 `src-tauri/src/lib.rs` builder（355-368）：

`.manage(Mutex::new(hidden::HiddenList::load()))`（358）后加：
```rust
.manage(Mutex::new(snoozed::SnoozeMap::load()))
```

`invoke_handler`（360-368）的 `generate_handler!` 列表加三命令：
```rust
.invoke_handler(tauri::generate_handler![
    hide_session,
    unhide_session,
    list_hidden,
    focus_session,
    get_sessions,
    get_hud_pinned,
    set_hud_pinned,
    snooze_session,
    unsnooze_session,
    list_snoozed
])
```

- [ ] **Step 3: poll_loop 算 derived snoozed**

在 `src-tauri/src/lib.rs` poll_loop 内，`merged` 算出后（现有 88 行 `need_attention` 聚合之前）、emit/cache 之前，插入 derived 计算。把现有对 `merged` 的直接使用改为 `derived`：

```rust
// derived snoozed：每轮基于 SnoozeMap 算，随 Session emit。
// lock 失败静默（视为无搁置）——不阻塞 poll。
let snoozed_map = handle.try_state::<Mutex<snoozed::SnoozeMap>>();
let derived: Vec<models::Session> = merged
    .iter()
    .map(|s| {
        let mut s = s.clone();
        s.snoozed = snoozed_map
            .as_ref()
            .and_then(|m| m.lock().ok())
            .map(|m| m.is_effectively_snoozed(&s))
            .unwrap_or(false);
        s
    })
    .collect();
```

随后把后续 `merged` 引用改为 `derived`（`need_attention`/`working` 聚合、tooltip、tray 更新、cache、emit、notify observe）。注意 `notify::observe` 用 derived 也行（snoozed 的不发通知——`observe` 现有逻辑只看 status 迁移，加 `!s.snoozed` 守卫更稳，见 Step 4）。

- [ ] **Step 4: notify 排除 snoozed（避免搁置的还弹通知）**

`src-tauri/src/notify.rs` `observe`（33-37）的判定加 `!s.snoozed`：

```rust
if s.alive && !s.snoozed && matches!(s.status, Status::NeedsPermission | Status::WaitingForInput) {
```

（`Session` 现在带 `snoozed` 字段，`observe` 接收的 `&[Session]` 已含 derived。）

- [ ] **Step 5: 编译 + 跑全部测试**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: 编译通过（若 `merged` 改 `derived` 漏改某处，编译错会指引）。

Run: `cd src-tauri && cargo test --lib`
Expected: 全 PASS（含 hidden/notify/snoozed/statemachine 现有 + 新增测试）。

- [ ] **Step 6: 手动验证命令往返（dev 模式）**

Run（后台）: `cd /Users/zhuyuchen/ai/cc-view && npm run tauri dev`
打开 app，在 HUD 列表存在时，用浏览器 devtools console 或临时按钮调：
```js
await window.__TAURI__.core.invoke('snooze_session', { id: '<某会话id>' });
await window.__TAURI__.core.invoke('list_snoozed');
```
Expected: `list_snoozed` 返回 `{ "<id>": <时间戳> }`，`~/.claude/cc-view/snoozed.json` 文件出现。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/notify.rs
git commit -m "feat(snoozed): poll_loop 算 derived + snooze 命令 + notify 排除搁置"
```

---

### Task 3: 前端 SessionList.vue（分组/聚类/灰显/搁置按钮）

**Files:**
- Modify: `src/types.ts`（加 `snoozed`）
- Modify: `src/components/SessionList.vue`
- Reference: `docs/superpowers/prototypes/snooze-prototype.html` 的 `hudRow`/`sorted`/`sectionHtml`/`capDead` 逻辑（已 gstack 验证）

**Interfaces:**
- Consumes: 后端 emit 的 `Session.snoozed`（Task 2）
- Produces: 行内「搁置」/「恢复」按钮 → 调 `snooze_session`/`unsnooze_session`，乐观更新。

- [ ] **Step 1: types.ts 加 snoozed**

`src/types.ts`：
```ts
export interface Session {
  id: string;
  source: string;
  pid: number;
  project: string;
  cwd: string;
  name: string;
  status: Status;
  startedAt: number;
  statusUpdatedAt: number;
  alive: boolean;
  focusHint: { host: string };
  snoozed: boolean; // derived，后端 poll_loop 算
}
```

- [ ] **Step 2: SessionList.vue 排序加项目维度 + 搁置档**

替换 `SessionList.vue` 现有 `statusRank`（29-39）为（搁置 5.5，dead 6，snoozed-dead 6.5）+ 同档内按 project + ago：

```ts
function statusRank(s: Session): number {
  if (s.snoozed) return s.alive ? 5.5 : 6.5;
  if (!s.alive) return 6;
  switch (s.status) {
    case 'needsPermission': return 1;
    case 'waitingForInput': return 2;
    case 'working': return 3;
    case 'shell': return 4;
    case 'compacting': return 5;
    default: return 99;
  }
}
const projShort = (p: string) => p.replace(/^\/Users\/[^/]+\//, '~/');
const AGO_ORDER = ['30s','1m','2m','3m','8m','12m','2h','5h','1d','2d','3d','4d'];
const agoIdx = (a: string) => { const i = AGO_ORDER.indexOf(a); return i < 0 ? 999 : i; };
// sorted：rank → project → ago
```

调整 `sorted` computed 的 sort 回调为 `statusRank(a)-statusRank(b) || a.project.localeCompare(b.project) || agoIdx(agoStr(a))-agoIdx(agoStr(b))`（`agoStr` 从 `statusUpdatedAt` 算现有 `agoF` 的输出，或复用 `agoF` 比较 timestamp 更稳——改用 `a.statusUpdatedAt - b.statusUpdatedAt` 作第三键）。

- [ ] **Step 3: 分组渲染（一级状态 + 二级项目）+ dead 限5**

把 `<ul><li v-for>` 扁平结构改为 computed `groups`（返回 `[{label, rows}]`，每组内再按 project 子分组）。模板用 `v-for` 嵌套：一级 head → 二级 proj-head → 行。参考原型 `sectionHtml` + `capDead`：

```ts
const DEAD_LIMIT = 5;
const groups = computed(() => {
  const sorted = [...props.sessions].sort(...);
  const active = sorted.filter(s => s.alive && !s.snoozed);
  const snoozedAlive = sorted.filter(s => s.alive && s.snoozed);
  let dead = sorted.filter(s => !s.alive);
  let deadHidden = 0;
  if (dead.length > DEAD_LIMIT) { deadHidden = dead.length - DEAD_LIMIT; dead = dead.slice(0, DEAD_LIMIT); }
  // 每组按 project 子分组
  const byProj = (rows: Session[]) => {
    const m = new Map<string, Session[]>();
    for (const s of rows) { const k = s.project; if (!m.has(k)) m.set(k, []); m.get(k)!.push(s); }
    return [...m.entries()];
  };
  return {
    active: byProj(active), snoozed: byProj(snoozedAlive),
    dead: byProj(dead), deadHidden,
  };
});
```

模板：`待介入`（v-for proj → proj-head + rows）→ 分隔线 → `已搁置` → 分隔线 → `已退出`（+ `+N 个更早的已隐藏` 提示）。

- [ ] **Step 4: 灰显 + 状态文字保留 cc 真实状态**

行 class 加 `snoozed`（`opacity: 0.5`）；`STATUS_ZH` 仍用 `s.status`（不改成"已搁置"）；已搁置行不显示 `isFresh` 蓝点。CSS：

```css
.row.snoozed { opacity: 0.5; }
```

- [ ] **Step 5: 「搁置」/「恢复」按钮**

waitingForInput 行加按钮，调命令 + 乐观更新（emit 让父刷新或本地改 session.snoozed）。参考原型 `snoozeBtn` + `hide()`/`unhide()` 模式：

```ts
async function snooze(id: string) {
  try { await invoke('snooze_session', { id }); emit('snooze', id); }
  catch (e) { console.error('snooze failed', e); }
}
async function unsnooze(id: string) {
  try { await invoke('unsnooze_session', { id }); emit('unsnooze', id); }
  catch (e) { console.error('unsnooze failed', e); }
}
```

模板（waitingForInput 且 alive 且 !snoozed → 「搁置」；snoozed → 「恢复」）：
```html
<button v-if="s.alive && s.snoozed" class="act copy" @click.stop="unsnooze(s.id)">恢复</button>
<button v-else-if="s.alive && s.status === 'waitingForInput'" class="act copy" @click.stop="snooze(s.id)">搁置</button>
```

父 `App.vue` 在 `<SessionList @snooze="?" @unsnooze="?">` 处加刷新（乐观：直接改 `all.value` 里对应 session 的 `snoozed`，不等 3s poll）：
```ts
// App.vue
function onSnooze(id: string) {
  const s = all.value.find(x => x.id === id); if (s) s.snoozed = true;
}
function onUnsnooze(id: string) {
  const s = all.value.find(x => x.id === id); if (s) s.snoozed = false;
}
```
`<SessionList ... @snooze="onSnooze" @unhide="onUnsnooze">` —— 注意 unsnooze emit 名与现有 `unhide` 区分，用 `@unsnooze="onUnsnooze"`。

- [ ] **Step 6: 类型检查**

Run: `cd /Users/zhuyuchen/ai/cc-view && npx vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 7: gstack 验证 HUD 分组/聚类/搁置交互**

启动 dev，用 gstack 打开 HUD 窗口（`file://` 或 webview devtools）。断言：
- 一级分组标题：`待介入 N` / `已搁置 N` / `已退出 N`
- 二级项目标题：同项目会话相邻
- dead 超过 5：显示 `+N 个更早的已隐藏`
- 点「搁置」：行移到已搁置组、灰显；点「恢复」：回待介入组

（参考已验证的原型断言：`docs/superpowers/prototypes/snooze-prototype.html` 通过 gstack 跑过同样检查。）

- [ ] **Step 8: Commit**

```bash
git add src/types.ts src/components/SessionList.vue src/App.vue
git commit -m "feat(snoozed): HUD 分组/项目聚类/灰显/搁置按钮"
```

---

### Task 4: 前端 Overlay.vue（ago/perm/fresh + 搜索重写 + 搁置）

**Files:**
- Modify: `src/components/Overlay.vue`
- Reference: 原型 `overlayRow`/`renderOverlay`（搜索扁平+高亮+计数，已 gstack 验证）

**Interfaces:**
- Consumes: `Session.snoozed`（Task 2）、`Session.statusUpdatedAt`（现有）
- Produces: Overlay = HUD 全集 + 搜索（扁平/高亮/计数），含搁置按钮。

- [ ] **Step 1: 补 ago + perm + fresh（与 HUD 一致）**

`Overlay.vue` 行模板加 `ago` span（用现有 `agoF`）、`needsPermission` 行 `perm` class（橙边）、`fresh`（<120s waitingForInput）蓝点。参考 `SessionList.vue` 的 `isFresh`/`agoF`（直接复用，或抽到共享 util——MVP 重复可接受，现有代码注释已允许）。

行 class：`{ dead: !s.alive, snoozed: s.snoozed, perm: s.status==='needsPermission' && !s.snoozed }`。

- [ ] **Step 2: 搜索重写（扁平 + 高亮 + 计数）**

替换 `Overlay.vue` `visible` computed（42-53）为：搜索态扁平、否则分组。加 `q` ref（已有）、`overlayCount` computed、`hl(text)` 高亮函数：

```ts
const searchActive = computed(() => q.value.trim().length > 0);
const flatResults = computed(() => {
  const k = q.value.trim().toLowerCase();
  if (!k) return [];
  return [...all.value]
    .sort(/* 同 SessionList rank+project+ago */)
    .filter(s => (s.name + ' ' + projShort(s.project)).toLowerCase().includes(k));
});
const groups = computed(() => { /* 同 SessionList 分组逻辑，可抽 composable 或重复 */ });
const overlayCount = computed(() => searchActive.value ? `${flatResults.value.length} 个结果` : `${groups.value.active.flat().length} 待介入`);
function hl(text: string, k: string) {
  if (!k) return text;
  const i = text.toLowerCase().indexOf(k.toLowerCase());
  return i < 0 ? text : text.slice(0, i) + '【' + text.slice(i, i + k.length) + '】' + text.slice(i + k.length);
  // 注：实际用 <mark>，Vue 模板里 v-html 或拆 span；MVP 用 <mark> via :innerHTML 或细分 span。
}
```

> 高亮实现细节：Vue 里 `{{ }}` 会转义。最简：拆成三段 span（前/匹配/后），匹配段加 `class="hl"`，CSS `.hl { background: rgba(255,214,121,.28); color: #ffd479; }`。避免 `v-html`（XSS）。原型用 `v-html`-等价是因为 mock 可信；生产用 span 拆分。

模板：搜索态 `v-for="s in flatResults"` 扁平行（无分组头）；否则分组（同 HUD）。`overlay-count` span 显示在搜索栏右侧。

- [ ] **Step 3: 搁置按钮**

Overlay 行 `actions` 加「搁置」/「恢复」（同 SessionList 逻辑，调 `snooze_session`/`unsnooze_session`，乐观更新 `all.value` 对应 `s.snoozed`）。与「复制 ID」并列，hover 显示（复用现有 `.actions` opacity 模式）。

- [ ] **Step 4: 类型检查**

Run: `npx vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 5: gstack 验证搜索**

断言（参考原型已验证用例）：
- 输入 'fang' → `overlay-count` 显示 `2 个结果`，列表扁平 2 行，匹配文字高亮
- 清空 → 恢复分组，计数 `N 待介入`
- Overlay 显示全集（含已搁置 + 已退出，不过滤 hidden）

- [ ] **Step 6: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "feat(snoozed): Overlay 补时间颜色 + 搜索重写 + 搁置"
```

---

### Task 5: tray badge 合成 + perm_count

**Files:**
- Create: `src-tauri/src/badge.rs`（`draw_badge` + 点阵数字 + 单测）
- Modify: `src-tauri/src/lib.rs`（`mod badge`；poll_loop `perm_count` 聚合 + tray 切换）

**Interfaces:**
- Consumes: `tauri::image::Image`（`TRAY_PNG` 现有）、`Session.snoozed`/`status`（Task 2）
- Produces: tray icon 在 `perm_count > 0` 时显示红圆+数字 badge，归零恢复。

- [ ] **Step 1: 写 badge.rs 失败测试**

创建 `src-tauri/src/badge.rs`，先测试：

```rust
// draw_badge 待实现
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_zero_returns_plain() {
        // count=0 不画 badge，返回原图（像素等同）
        let src = tauri::image::Image::new_owned(vec![0u8; 4], 1, 1);
        let out = draw_badge(&src, 0);
        assert_eq!(out.rgba(), src.rgba());
    }

    #[test]
    fn count_positive_draws_red_pixels() {
        // count>0 在右上角画红圆 → 至少一个像素是红 (255,69,58)
        let src = tauri::image::Image::new_owned(vec![0u8; 22*22*4], 22, 22);
        let out = draw_badge(&src, 3);
        let rgba = out.rgba();
        let red_count = rgba.chunks_exact(4).filter(|p| p[0]==255 && p[1]==69 && p[2]==58 && p[3]==255).count();
        assert!(red_count > 0, "badge must paint red pixels");
    }

    #[test]
    fn count_over_9_capped() {
        // >9 显示 9+，不 panic
        let src = tauri::image::Image::new_owned(vec![0u8; 22*22*4], 22, 22);
        let _ = draw_badge(&src, 99); // 不 panic 即可
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib badge`
Expected: 编译失败（`draw_badge` 未定义）。

- [ ] **Step 3: 实现 draw_badge（RGBA 画红圆 + 点阵数字）**

`src-tauri/src/badge.rs`：

```rust
/// 3x5 点阵数字 0-9（行优先，每行 3 bit，bit=1 为点亮）。
/// 用于在 menu bar 小图标上画 badge 数字。
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b010, 0b010, 0b010], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];
// "+" 点阵（用于 9+）
const PLUS: [u8; 5] = [0b010, 0b010, 0b111, 0b010, 0b010];

/// 在 src 右上角画红圆 + 数字 count。count=0 返回原图副本（无 badge）。
/// count>9 显示 "9+"。纯 RGBA 像素操作，复用 tint_orange 模式，无 image crate。
pub fn draw_badge(src: &tauri::image::Image<'_>, count: usize) -> tauri::image::Image<'static> {
    let w = src.width() as i32;
    let h = src.height() as i32;
    let mut out = src.rgba().to_vec();
    if count == 0 {
        return tauri::image::Image::new_owned(out, w as u32, h as u32);
    }
    let put = |out: &mut [u8], x: i32, y: i32, (r, g, b): (u8, u8, u8)| {
        if x >= 0 && y >= 0 && x < w && y < h {
            let i = (y as usize * w as usize + x as usize) * 4;
            out[i] = r; out[i + 1] = g; out[i + 2] = b; out[i + 3] = 255;
        }
    };
    // badge 圆：右上角，圆心 (w-6, 6)，半径 6
    let cx = w - 6;
    let cy = 6;
    let rad = 6;
    for y in (cy - rad)..=(cy + rad) {
        for x in (cx - rad)..=(cx + rad) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= rad * rad {
                put(&mut out, x, y, (255, 69, 58)); // 系统红 #FF453A
            }
        }
    }
    // 数字：count>9 显示 9+（两位：'9' 与 '+'），否则单数字
    let digits: Vec<usize> = if count > 9 { vec![9, 10] } else { vec![count.min(9)] };
    // 在圆内居中画点阵（每个数字 3 宽，像素步长 1，居中起点）
    let scale = 1; // menu bar 图标小，1px/点
    let total_w = (digits.len() as i32 * 3 * scale) as i32;
    let start_x = cx - total_w / 2;
    let start_y = cy - 2; // 5 行点阵居中
    for (di, &d) in digits.iter().enumerate() {
        let glyph: [u8; 5] = if d == 10 { PLUS } else { DIGITS[d] };
        let ox = start_x + (di as i32 * 3 * scale);
        for row in 0..5 {
            for col in 0..3 {
                if (glyph[row] >> (2 - col)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            put(&mut out, ox + col * scale + sx, start_y + row * scale + sy, (255, 255, 255));
                        }
                    }
                }
            }
        }
    }
    tauri::image::Image::new_owned(out, w as u32, h as u32)
}
```

- [ ] **Step 4: 跑测试通过**

`src-tauri/src/lib.rs` 加 `mod badge;`。Run: `cd src-tauri && cargo test --lib badge`
Expected: PASS（3 测试）。

- [ ] **Step 5: poll_loop 聚合 perm_count + tray 切换**

`src-tauri/src/lib.rs` poll_loop，在 derived 算好后（Task 2 Step 3 之后）、tray 更新处（现有 110-128），加 perm_count + badge 切换。扩展现有 `last_attention` 防抖模式加 `last_perm_count`：

```rust
// perm_count：等权限（硬阻塞）计数，用于 tray badge。排除 snoozed（按失效规则应已 unsnooze，保险）。
let perm_count = derived
    .iter()
    .filter(|s| s.alive && !s.snoozed && matches!(s.status, models::Status::NeedsPermission))
    .count();

if let Some(tray) = handle.tray_by_id("main") {
    let tip = if perm_count > 0 {
        format!("{} 等权限 · {} 等我 · {} 工作", perm_count, need_attention, working)
    } else if need_attention > 0 {
        format!("{} 等我 · {} 工作", need_attention, working)
    } else {
        format!("{} 工作", working)
    };
    let _ = tray.set_tooltip(Some(tip));

    // tray icon 三态：perm>0 → badge icon（红圆数字，template=false）；
    //                  attention>0 → 橙色（现有）；否则单色剪影。
    let has_attention = need_attention > 0;
    if perm_count != last_perm_count || has_attention != last_attention {
        last_perm_count = perm_count;
        last_attention = has_attention;
        let (icon, as_template) = if perm_count > 0 {
            // badge 合成：基于单色剪影底图画红圆数字
            let base = tray_icon.as_ref().map(|img| badge::draw_badge(img, perm_count));
            (base.as_ref(), false)
        } else if has_attention {
            (orange_icon.as_ref(), false)
        } else {
            (tray_icon.as_ref(), true)
        };
        if let Some(img) = icon {
            let _ = tray.set_icon_with_as_template(Some(img.clone()), as_template);
        }
    }
}
```

在 poll_loop 顶部声明 `let mut last_perm_count: usize = 0;`（紧挨现有 `last_attention`）。

- [ ] **Step 6: 编译 + 测试**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: 全 PASS。

- [ ] **Step 7: 手动验证 badge**

Run: `npm run tauri dev`，制造一个 needsPermission 会话（在某个 Claude 会话触发权限请求）。
Expected: menu bar tray icon 右上角出现红圆+数字；处理权限后数字减/消失。

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/badge.rs src-tauri/src/lib.rs
git commit -m "feat(badge): tray 等权限计数 badge（动态合成红圆数字，常驻至归零）"
```

---

### Task 6: 端到端验证

**Files:** 无改动，只验证。

- [ ] **Step 1: 全量测试 + 类型检查**

```bash
cd src-tauri && cargo test --lib    # 全 Rust 测试
cd .. && npx vue-tsc --noEmit        # 前端类型
```
Expected: 全 PASS。

- [ ] **Step 2: gstack 端到端**

启动 dev，gstack 打开 HUD + Overlay，跑断言：
- 分组/项目聚类/dead 限5（Task 3 Step 7）
- 搜索扁平+高亮+计数+清空恢复（Task 4 Step 5）
- 搁置→移组灰显、恢复→回组（乐观更新）
- needsPermission 会话：tray badge 显示红圆数字，处理消失

- [ ] **Step 3: 最终 commit（如有 fixup）**

```bash
git add -A
git commit -m "chore: snooze + badge 端到端验证通过" --allow-empty
```

---

## Self-Review（写计划后自查）

**1. Spec 覆盖：**
- 搁置概念/失效规则 → Task 1 `is_effectively_snoozed`（8 测试覆盖边界）✓
- 灰显沉底+项目聚类+dead限5 → Task 3 ✓
- HUD+Overlay 都做 + Overlay 全集+搜索 → Task 3/4 ✓
- Overlay 补 ago/perm/fresh → Task 4 Step 1 ✓
- snoozed derived 字段 + 前后端共用 → Task 1/2 ✓
- 乐观更新时效 → Task 3 Step 5（App.vue onSnooze/onUnsnooze）✓
- badge perm_count + 常驻至归零 → Task 5 ✓
- 保留首次通知 + waitingForInput 不进 badge → Task 2 Step 4（notify 排除 snoozed）+ Task 5（badge 仅 NeedsPermission）✓
- badge 路径 A 动态合成 + 退化 → Task 5 draw_badge（点阵数字，无 image crate）✓

**2. Placeholder 扫描：** 无 TBD/TODO；前端 Task 3/4 的分组逻辑指向原型参考 + 给了 computed 关键代码，未留空。`hl` 高亮从原型 `v-html` 改为 span 拆分（生产安全），已在 Task 4 Step 2 注明。✓

**3. 类型一致：** `Session.snoozed` 前后端均 `bool`；`SnoozeMap` 方法签名（`add(&str,i64)`/`get(&str)->Option<i64>`/`is_effectively_snoozed(&Session)->bool`）Task 1 定义、Task 2/5 使用一致；`draw_badge(&Image, usize)->Image` Task 5 内部一致。✓

**潜在风险（实现时注意）：**
- Task 2 Step 3 把 `merged` 改 `derived` 涉及多处引用，编译错会指引，逐处改。
- Task 3/4 分组逻辑重复（HUD/Overlay），MVP 可接受（现有注释允许）；若想 DRY 可抽 `composables/useSessions.ts`，但非本次必需。
- badge 点阵数字在 22px 图标上可能偏小，实现时调 `scale`/圆心位置。
