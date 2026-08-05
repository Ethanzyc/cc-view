# 等输入闲置降级 — 设计

- 日期：2026-08-05
- 状态：已实现

> **修正记录**：最初设计目标误定为等回答（waitingForReply），实现 + 验证后修正为等输入（waitingForInput）。
> 等回答是过程中阻塞提问（少而紧急，该醒目），等输入才是 Claude Code 常态停顿、长期晾着该降级清理的。
> 同步把标签文案从「超时」改为「闲置」，class `stale` → `idle`。

## 背景

overlay 命令面板的「待介入」section 按状态优先级展示需要用户处理的会话。`waitingForInput`（等输入，Claude 完成一轮、等用户给下一条指令）排 rank 3。这是 Claude Code 的常态停顿——用户做完一个 task 切走后，会话长期停在等输入，一直霸占待介入区，挤占注意力。

需要：等输入晾置超过阈值，在 UI 层降级，让待介入区聚焦"近期需要处理的"。

## 目标 / 非目标

**目标**
- 等输入晾置超过 30 分钟 → 在待介入 section 内降级（灰显 + 排后）
- 不丢失：降级会话仍留在待介入 section，不进已搁置 / 已退出
- 自动恢复：用户回去处理后自动取消降级
- 纯前端 derived，不动后端状态机

**非目标**
- 不改会话后端状态、不进 SnoozeMap（这不是真 snooze）
- 不对等回答 / 等权限生效（等回答是阻塞提问该醒目；等权限太重要）
- 不做可配置阈值（先硬编码，YAGNI）

## 设计

### ① 判定（`src/utils/session.ts`）

纯函数 `isStaleInput(s: Session, now: number): boolean`：

```
s.status === 'waitingForInput' && !s.snoozed && now - s.statusUpdatedAt > STALE_INPUT_MS
```

- 常量 `STALE_INPUT_MS = 30 * 60 * 1000`（30 分钟）
- `now` 由调用方传入（响应式 now ref，见 ②）
- 复用现有 `statusUpdatedAt`，无新数据
- `!s.snoozed`：手动搁置走 SnoozeMap 另一套展示，不重复判定

### ② 排序 + 时间响应（`src/components/Overlay.vue`）

**响应式 now（关键）**：后端 `sessions` emit 有 hash 去重（`lib.rs hash_sessions` 按 `id/status/alive/snoozed`，数据不变不 emit）。`isStaleInput` 依赖时间，必须前端加 `now` ref + `setInterval(60s)` 定期触发重算，否则晾着的等输入跨阈值时不会自动变闲置。

**sorted computed**：第一排序键加 `isStaleInput`（闲置排后），其后仍是 rank → project → statusUpdatedAt desc。同一 project 内，闲置等输入排到该 project 组末尾。

**groups computed active 段**：byProj 聚类后对 project 数组重排——组内 `every(isStaleInput)` 的 project 整组沉到 active section 底部，其余保持字母序（稳定排序）。

### ③ 视觉（行）

闲置行加 `idle` class：

- `opacity: 0.5`（同 `.row.snoozed` 灰显档）
- ago 旁加「闲置」小标签（`.idle-tag`，样式同 `.hidden-tag`）
- fresh 蓝点天然不冲突（fresh <2min，闲置 >30min）
- 搜索态 + 非搜索态两个模板分支都覆盖

### ④ 自动恢复

无需显式恢复：状态变化（用户给下一条指令 → working）或 `statusUpdatedAt` 刷新进 30min 内 → `isStaleInput` 返回 false → 自动归位。不持久化（纯 derived，每次 tick / poll 重算）。

## 不动

- 后端（`lib.rs` / `snoozed.rs` / `models.rs`）、SnoozeMap、`tauri.conf.json`、prefs

## 影响的文件

- `src/utils/session.ts` —— `STALE_INPUT_MS` + `isStaleInput(s, now)`
- `src/components/Overlay.vue` —— `now` ref + 60s 定时器；sorted 闲置键；groups project 重排；行 `idle` class + 「闲置」标签；`.row.idle` / `.idle-tag` 样式

## 边界情况

- **搜索态**：`flatResults` 基于 `sorted` filter，闲置排序自动继承
- **hidden 优先级**：`is-hidden`（0.35）源序在 `.row.idle`（0.5）之后，覆盖 idle——hidden 语义更强
- **跨阈值**：60s tick，最多 60s 延迟切换
- **now 基准**：`Date.now()`，每次 tick / poll 重算

## 测试策略

项目无前端测试设施（`package.json` 无 test script）。`isStaleInput` 为纯函数，手动构造 `statusUpdatedAt` 验证边界（29min / 31min / snoozed / 非等输入）；排序 / 视觉 / 恢复靠 overlay 渲染验证（临时调 `STALE_INPUT_MS = 3_000`）。
