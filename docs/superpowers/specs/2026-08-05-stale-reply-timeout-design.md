# 等回答超时降级 — 设计

- 日期：2026-08-05
- 状态：已批准，待实现

## 背景

overlay 命令面板的「待介入」section 按状态优先级展示需要用户处理的会话。`waitingForReply`（等回答，Claude 过程中提问）排 rank 2，仅次于等权限。一个等回答会话如果长时间没人处理，会一直霸占待介入区的高优先级位置，挤占注意力——用户大概率已经切任务或离开，不再关注这个会话。

需要：等回答晾置超过阈值，在 UI 层降级，让待介入区聚焦"近期需要处理的"。

## 目标 / 非目标

**目标**
- 等回答晾置超过 30 分钟 → 在待介入 section 内降级（灰显 + 排后）
- 不丢失：降级会话仍留在待介入 section，不进已搁置 / 已退出
- 自动恢复：用户回去处理后自动取消降级
- 纯前端 derived，不动后端状态机

**非目标**
- 不改会话后端状态、不进 SnoozeMap（这不是真 snooze）
- 不对等输入 / 等权限生效（等输入是正常工作流间隔；等权限太重要）
- 不做可配置阈值（先硬编码，YAGNI）

## 设计

### ① 判定（`src/utils/session.ts`）

新增纯函数 `isStaleReply(s: Session): boolean`：

```
s.status === 'waitingForReply' && !s.snoozed && Date.now() - s.statusUpdatedAt > STALE_REPLY_MS
```

- 常量 `STALE_REPLY_MS = 30 * 60 * 1000`（30 分钟）
- 复用现有 `statusUpdatedAt`，无新数据
- `!s.snoozed`：手动搁置走 SnoozeMap 另一套展示，不重复判定

### ② 排序（`src/components/Overlay.vue`）

两层调整：

**sorted computed ——** 第一排序键加 `isStaleReply`（超时排后），其后仍是 rank → project → statusUpdatedAt desc：

```
1. isStaleReply (false < true)
2. statusRank
3. project (localeCompare)
4. statusUpdatedAt desc
```

效果：同一 project 内，超时等回答排到该 project 组的末尾。

**groups computed 的 active 段 ——** byProj 聚类后，对 project 数组重排：

- 一个 project 组 `every(s => isStaleReply(s))`（组内全部是超时等回答）→ 整组沉到 active section 底部
- 其余 project 保持原序（字母序，来自 sorted）
- 用稳定排序保证同档 project 间保持字母序

### ③ 视觉（行）

超时行加 `stale` class：

- `opacity: 0.5`（同 `.row.snoozed` 灰显档）
- ago 旁加「超时」小标签，复用 `.hidden-tag` 样式
- fresh 蓝点天然不冲突（fresh <2min，超时 >30min，互斥）
- 搜索态（flatResults）与非搜索态两个模板分支都加 `stale` class 处理；flatResults 源自 sorted，超时排序自动继承

### ④ 自动恢复

无需显式恢复逻辑：

- 状态变化（用户回答 → working）→ 不再是 waitingForReply → `isStaleReply` 返回 false → 自动归位
- `statusUpdatedAt` 刷新进 30 分钟内 → `isStaleReply` 返回 false → 自动归位

不持久化（纯 derived，每次渲染 / poll 重算）。

## 不动

- 后端（`lib.rs` / `snoozed.rs` / `models.rs`）
- SnoozeMap（手动搁置机制）
- `tauri.conf.json`
- prefs 偏好设置

## 影响的文件

- `src/utils/session.ts` —— 加 `STALE_REPLY_MS` 常量 + `isStaleReply` 函数
- `src/components/Overlay.vue` —— sorted 加 stale 键；groups active 段 project 重排；搜索态 + 非搜索态行加 `stale` class + 超时标签；`.row.stale` 样式

## 边界情况

- **搜索态**：`flatResults` 基于 `sorted` filter，超时排序自动继承；行灰显靠 `stale` class（两个模板分支都覆盖）
- **hidden 优先级**：`is-hidden`（opacity 0.35）比 `stale`（0.5）更淡；同时命中时（一个会话既被手动隐藏又超时）让 `.row.is-hidden` 覆盖 `.row.stale`，靠样式定义顺序保证 hidden 语义更强
- **跨阈值**：会话在 30min 边界附近每轮 poll（3s）重算，最多 3s 延迟切换，不做滞回
- **now 基准**：用 `Date.now()`，每次渲染 / poll 重算，无需全局时钟

## 测试策略

项目无前端测试设施（`package.json` 无 test script）。本次不引入测试框架（YAGNI，避免 scope 膨胀）。验证方式：

- `isStaleReply` 是纯函数，实现后手动构造 `statusUpdatedAt` 场景验证边界（29min / 31min / snoozed / 非等回答）
- 排序 / 视觉 / 自动恢复靠 overlay 实际渲染验证（构造一个等回答会话，等 / 改时间戳观察）
