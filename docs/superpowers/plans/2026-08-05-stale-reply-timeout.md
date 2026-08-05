# 等回答超时降级 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 等回答会话晾置超 30 分钟时，在 overlay「待介入」section 内降级——project 内排末尾 + 灰显 + 「超时」标签；整组全超时的 project 沉到 section 底部。纯前端 derived，不动后端。

**Architecture:** 前端纯函数 `isStaleReply` 判定 → `sorted` 加超时排序键（project 内排末尾）→ `groups` active 段 project 重排（全超时 project 沉底）→ 行 `stale` class + 标签 + 样式。

**Tech Stack:** Vue 3（Composition API）+ TypeScript，Tauri 2，Vite。

## Global Constraints

- **无测试框架**：项目 `package.json` 无 test script；spec 决定不引入测试框架（YAGNI）。验证 = `npx vue-tsc --noEmit` 类型检查 + 手动渲染验证。
- **纯前端**：只动 `src/`，不碰 `src-tauri/`、`SnoozeMap`、`tauri.conf.json`、prefs。
- **注释中文，代码英文**（遵循用户全局规范）。
- **手动验证技巧**：dev 模式下临时把 `STALE_REPLY_MS` 改成 `3_000`（3 秒），打开 overlay 观察一个 `等回答` 会话在 3s 后变灰 + 排后；验证后改回 `30 * 60 * 1000`。
- **commit 规范**：Conventional Commits + 中文描述；在 feature 分支（如 `stale-reply-timeout`）上提交，除非用户指定 main。

---

## File Structure

- `src/utils/session.ts`（改）—— 加 `STALE_REPLY_MS` 常量 + `isStaleReply` 纯函数。职责：会话展示判定（已有 `statusRank` / `isFresh` 等）。
- `src/components/Overlay.vue`（改）—— `sorted` computed 加超时键；`groups` active 段 project 重排；搜索态 + 非搜索态两个 row 模板加 `stale` class + 「超时」标签；`.row.stale` / `.stale-tag` 样式。

---

### Task 1: isStaleReply 判定函数

**Files:**
- Modify: `src/utils/session.ts`（在 `isFresh` 函数附近）

**Interfaces:**
- Produces: `export const STALE_REPLY_MS: number`（`30 * 60 * 1000`）；`export function isStaleReply(s: Session): boolean`

- [ ] **Step 1: 加常量 + 函数**

在 `src/utils/session.ts` 的 `isFresh` 函数后追加（`Session` 类型已在文件顶部 import）：

```ts
// 等回答超时阈值：超过此时长未处理的等回答在 overlay 降级（灰显 + 排后）。
export const STALE_REPLY_MS = 30 * 60 * 1000;

// 等回答晾置超时：waitingForReply + 非搁置 + 距 statusUpdatedAt 超阈值。
// 纯前端 derived，不动后端 / SnoozeMap。仅对等回答（等输入是正常工作流间隔，等权限太重要）。
export function isStaleReply(s: Session): boolean {
  return s.status === 'waitingForReply'
    && !s.snoozed
    && Date.now() - s.statusUpdatedAt > STALE_REPLY_MS;
}
```

- [ ] **Step 2: 类型检查**

Run: `npx vue-tsc --noEmit`
Expected: 无错误退出（exit 0）。

- [ ] **Step 3: 手动验证（可选，快速确认逻辑）**

dev 模式下临时把 `STALE_REPLY_MS` 改成 `3_000`，浏览器 console 执行（`Session` 对象可从 overlay 的 Vue devtools 拿，或临时 `window.__s = all` 暴露）。验证后改回 `30 * 60 * 1000`。

- [ ] **Step 4: Commit**

```bash
git add src/utils/session.ts
git commit -m "feat: 加 isStaleReply 判定（等回答超 30min 降级依据）"
```

---

### Task 2: sorted 排序加超时等回答沉底

**Files:**
- Modify: `src/components/Overlay.vue`（`sorted` computed，`isFresh` 等已 import 的那行附近）

**Interfaces:**
- Consumes: `isStaleReply` from `../utils/session`（Task 1 产出）

- [ ] **Step 1: import isStaleReply**

当前 import 行（约 line 14）：
```ts
import { STATUS_ZH, statusRank, projShort, agoF, isFresh, hlParts } from '../utils/session';
```
改为：
```ts
import { STATUS_ZH, statusRank, projShort, agoF, isFresh, isStaleReply, hlParts } from '../utils/session';
```

- [ ] **Step 2: sorted 加超时第一键**

当前 `sorted` computed（约 line 32-40）：
```ts
const sorted = computed(() =>
  [...visible.value].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    const pc = a.project.localeCompare(b.project);
    if (pc !== 0) return pc;
    return b.statusUpdatedAt - a.statusUpdatedAt;
  }),
);
```
改为（第一键加 `isStaleReply`，超时排后）：
```ts
const sorted = computed(() =>
  [...visible.value].sort((a, b) => {
    // 超时等回答沉底（配合 groups 的全超时 project 整组下沉）
    const sa = isStaleReply(a), sb = isStaleReply(b);
    if (sa !== sb) return sa ? 1 : -1;
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    const pc = a.project.localeCompare(b.project);
    if (pc !== 0) return pc;
    return b.statusUpdatedAt - a.statusUpdatedAt;
  }),
);
```

- [ ] **Step 3: 类型检查**

Run: `npx vue-tsc --noEmit`
Expected: exit 0。

- [ ] **Step 4: 手动验证**

dev 模式，临时 `STALE_REPLY_MS = 3_000`。找一个有两个等回答会话的 project（或造一个刚变的 + 一个晾着的），观察：晾的那个在 3s 后排到该 project 组末尾（此时全超时 project 还没整组下沉，那是 Task 3）。验证后改回 `30 * 60 * 1000`。

- [ ] **Step 5: Commit**

```bash
git add src/components/Overlay.vue src/utils/session.ts
git commit -m "feat: sorted 排序加超时等回答排到 project 组末尾"
```

---

### Task 3: 待介入组全超时 project 整组下沉

**Files:**
- Modify: `src/components/Overlay.vue`（`groups` computed 的 active 段，约 line 83-84）

**Interfaces:**
- Consumes: `isStaleReply`（Task 1）；`byProj`（同 computed 内已有）

- [ ] **Step 1: active 段 project 重排**

当前 active 段（约 line 83-84）：
```ts
  if (active.length) result.push({ key: 'active', label: '待介入', total: active.length, projs: byProj(active), hidden: 0 });
```
改为（byProj 后对 project 数组重排，全超时的整组沉底）：
```ts
  if (active.length) {
    // 全超时 project（组内 every isStaleReply）沉到 active section 底部；
    // 其余保持 byProj 的字母序（Array.sort ES2019+ 稳定，同档不动）。
    const activeProjs = byProj(active).sort((a, b) => {
      const aStale = a[1].every(isStaleReply);
      const bStale = b[1].every(isStaleReply);
      if (aStale !== bStale) return aStale ? 1 : -1;
      return 0;
    });
    result.push({ key: 'active', label: '待介入', total: active.length, projs: activeProjs, hidden: 0 });
  }
```

注意：`byProj` 函数本身不改（snoozedAlive / dead 段仍用原 `byProj`，不重排）。

- [ ] **Step 2: 类型检查**

Run: `npx vue-tsc --noEmit`
Expected: exit 0。

- [ ] **Step 3: 手动验证**

dev 模式，临时 `STALE_REPLY_MS = 3_000`。造一个 project 下全是等回答会话（都晾超 3s），观察：该整组从原字母序位置沉到「待介入」section 最底；而含任一非超时项（等权限 / 等输入 / 新等回答）的 project 保持原位。验证后改回 `30 * 60 * 1000`。

- [ ] **Step 4: Commit**

```bash
git add src/components/Overlay.vue
git commit -m "feat: 待介入组全超时 project 整组下沉"
```

---

### Task 4: 行 stale 视觉（灰显 + 超时标签）

**Files:**
- Modify: `src/components/Overlay.vue`（搜索态 + 非搜索态两个 row 模板的 `:class` 与 `ago` 区；`<style>` 加 `.row.stale` / `.stale-tag`）

**Interfaces:**
- Consumes: `isStaleReply`（Task 1）

- [ ] **Step 1: 非搜索态 row 加 stale class**

非搜索态 row 的 `:class`（约 line 348-354），在 `'is-hidden': hidden.has(s.id),` 后加一行：
```ts
            :class="{
              dead: !s.alive,
              snoozed: s.snoozed,
              perm: s.status === 'needsPermission' && !s.snoozed,
              reply: s.status === 'waitingForReply' && !s.snoozed,
              'is-hidden': hidden.has(s.id),
              stale: isStaleReply(s),
            }"
```

- [ ] **Step 2: 搜索态 row 加 stale class**

搜索态 row 的 `:class`（约 line 268-273），同样加 `stale: isStaleReply(s),`：
```ts
          :class="{
            dead: !s.alive,
            snoozed: s.snoozed,
            perm: s.status === 'needsPermission' && !s.snoozed,
            'is-hidden': hidden.has(s.id),
            stale: isStaleReply(s),
          }"
```

- [ ] **Step 3: 两个 row 的 ago 区加「超时」标签**

非搜索态 ago（约 line 369-373）和搜索态 ago（约 line 301-305），在 `<span v-if="hidden.has(s.id)" class="hidden-tag">已隐藏</span>` 后加：
```html
              <span v-if="isStaleReply(s)" class="stale-tag">超时</span>
```

两处都加（搜索态 + 非搜索态）。

- [ ] **Step 4: 加样式**

在 `<style scoped>` 里 `.row.snoozed` 规则之后（约 line 608 后，确保在 `.row.is-hidden` line 717 之前——is-hidden 源序在后才能覆盖 stale）加：
```css
/* 超时等回答：灰显（同 snoozed 档），不丢失但视觉降级。
   is-hidden（opacity 0.35）源序在后，会覆盖 stale（0.5）——hidden 语义更强。 */
.row.stale {
  opacity: 0.5;
}
.stale-tag {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-tertiary);
  margin-left: var(--gap-xs);
}
```

- [ ] **Step 5: 类型检查**

Run: `npx vue-tsc --noEmit`
Expected: exit 0。

- [ ] **Step 6: 手动验证**

dev 模式，临时 `STALE_REPLY_MS = 3_000`：
1. 一个等回答会话晾超 3s → 行 opacity 0.5 灰显 + ago 旁出现「超时」标签 + 排到 project 末尾。
2. 把该会话手动隐藏（点「隐藏」）→ opacity 变 0.35（is-hidden 覆盖 stale），「超时」+「已隐藏」标签都在。
3. 该会话状态变 working（在 cc 里回答它）→ 立即取消灰显 + 归位。
4. 验证后改回 `STALE_REPLY_MS = 30 * 60 * 1000`。

- [ ] **Step 7: Commit**

```bash
git add src/components/Overlay.vue src/utils/session.ts
git commit -m "feat: 超时等回答灰显 + 超时标签"
```

---

## Self-Review

**Spec coverage：**
- 判定（isStaleReply + 30min + 非snoozed + 仅等回答）→ Task 1 ✓
- sorted project 内排末尾 → Task 2 ✓
- 全超时 project 整组沉底 → Task 3 ✓
- 灰显 opacity 0.5 + 「超时」标签 → Task 4 ✓
- 搜索态覆盖 → Task 4 Step 2/3 ✓
- 自动恢复 → 无代码（derived 天然，Task 4 Step 6 验证项 3）✓
- is-hidden 覆盖 stale → Task 4 Step 4 注释 + Step 6 验证项 2 ✓
- 不动后端 / SnoozeMap → Global Constraints ✓

**Placeholder scan：** 无 TBD/TODO；所有代码步骤含完整代码。✓

**Type consistency：** `isStaleReply(s: Session): boolean` 在 Task 1 定义，Task 2/3/4 消费签名一致；`STALE_REPLY_MS` 常量名全程一致。✓
