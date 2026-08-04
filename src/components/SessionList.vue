<script setup lang="ts">
// HUD 会话列表：一级分组（待介入 / 已搁置 / 已退出）+ 二级项目聚类 + dead 限 5 + 行内搁置/恢复。
// 排序/分组/折叠算法照抄 docs/superpowers/prototypes/snooze-prototype.html 的
// sorted / sectionHtml / capDead（已 gstack 验证交互通过）。
// 行内「搁置」调 snooze_session、「恢复」调 unsnooze_session；成功后 emit 让 App.vue
// 乐观更新 all[i].snoozed（不等 3s 轮询），分组/灰显立即生效。
// 隐藏（× / +）保留原 hide_session / unhide_session 流程。
import { computed } from 'vue';
import type { Session } from '../types';
import { invoke } from '@tauri-apps/api/core';
import StatusIcon from './StatusIcon.vue';
import { STATUS_ZH, statusRank, projShort, agoF, isFresh } from '../utils/session';

const props = withDefaults(defineProps<{ sessions: Session[]; hidden?: string[] }>(), {
  hidden: () => [] as string[],
});
const emit = defineEmits<{
  (e: 'hide', id: string): void;
  (e: 'unhide', id: string): void;
  (e: 'snooze', id: string): void;
  (e: 'unsnooze', id: string): void;
}>();

// 排序：rank → project 字母序 → statusUpdatedAt 降序（最近变更靠前；与原型 agoIdx 升序等价，
// 同时保证 dead 限 5 时保留最近的——slice(0, N) 取头部）。statusRank/projShort/agoF/isFresh/STATUS_ZH 见 utils/session。
const sorted = computed(() =>
  [...props.sessions].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    const pc = a.project.localeCompare(b.project);
    if (pc !== 0) return pc;
    return b.statusUpdatedAt - a.statusUpdatedAt;
  }),
);

// dead 限 5：超过的只留最近的 5 个，其余折叠为 "+N 个更早的已隐藏"
const DEAD_LIMIT = 5;

type Section = {
  key: string;
  label: string;
  total: number;
  projs: [string, Session[]][];
  hidden: number;
};

// 分组：一级（待介入 / 已搁置 / 已退出）× 二级（同 project 聚类）
// 空组跳过（与原型 sectionHtml 早退一致）；group-sep 只在非首组前渲染
const sections = computed<Section[]>(() => {
  const list = sorted.value;
  const active = list.filter(s => s.alive && !s.snoozed);
  const snoozedAlive = list.filter(s => s.alive && s.snoozed);
  let dead = list.filter(s => !s.alive);
  let deadHidden = 0;
  if (dead.length > DEAD_LIMIT) {
    deadHidden = dead.length - DEAD_LIMIT;
    dead = dead.slice(0, DEAD_LIMIT);
  }
  // 同组按 project 聚类（Map 保留首次出现顺序 = 字母序，因 sorted 已按 project 排）
  const byProj = (rows: Session[]): [string, Session[]][] => {
    const m = new Map<string, Session[]>();
    for (const s of rows) {
      const arr = m.get(s.project);
      if (arr) arr.push(s);
      else m.set(s.project, [s]);
    }
    return [...m.entries()];
  };
  const result: Section[] = [];
  if (active.length) result.push({ key: 'active', label: '待介入', total: active.length, projs: byProj(active), hidden: 0 });
  if (snoozedAlive.length) result.push({ key: 'snoozed', label: '已搁置', total: snoozedAlive.length, projs: byProj(snoozedAlive), hidden: 0 });
  if (dead.length) result.push({ key: 'dead', label: '已退出', total: dead.length, projs: byProj(dead), hidden: deadHidden });
  return result;
});

// invoke 失败仅 console.error，UI 不崩（按钮交互不应让 app 崩溃）
async function hide(id: string) {
  try {
    await invoke('hide_session', { id });
    emit('hide', id);
  } catch (e) {
    console.error('hide failed', e);
  }
}
async function unhide(id: string) {
  try {
    await invoke('unhide_session', { id });
    emit('unhide', id);
  } catch (e) {
    console.error('unhide failed', e);
  }
}
// 搁置/恢复：成功后 emit 让父组件乐观更新 all[i].snoozed（不等 3s poll）
async function snooze(id: string) {
  try {
    await invoke('snooze_session', { id });
    emit('snooze', id);
  } catch (e) {
    console.error('snooze failed', e);
  }
}
async function unsnooze(id: string) {
  try {
    await invoke('unsnooze_session', { id });
    emit('unsnooze', id);
  } catch (e) {
    console.error('unsnooze failed', e);
  }
}
// 行点击：激活该 session 对应的 host 终端
async function focus(id: string) {
  try {
    await invoke('focus_session', { id });
  } catch (e) {
    console.error('focus failed', e);
  }
}
</script>

<template>
  <ul class="list">
    <template v-for="(section, si) in sections" :key="section.key">
      <li v-if="si > 0" class="group-sep" />
      <li class="group-head">
        {{ section.label }}
        <span class="cnt">{{ section.total }}</span>
      </li>
      <template v-for="[proj, rows] in section.projs" :key="section.key + '|' + proj">
        <li class="proj-head">{{ projShort(proj) }}</li>
        <li
          v-for="s in rows"
          :key="s.id"
          class="row"
          :class="{
            dead: !s.alive,
            snoozed: s.snoozed,
            'perm-row': s.status === 'needsPermission' && !s.snoozed,
            'reply-row': s.status === 'waitingForReply' && !s.snoozed,
          }"
          role="button"
          tabindex="0"
          @click="focus(s.id)"
          @keydown.enter.prevent="focus(s.id)"
          @keydown.space.prevent="focus(s.id)"
        >
          <StatusIcon :status="s.status" class="icon" />
          <div class="info">
            <div class="line1">
              <span class="name">{{ s.name || s.project }}</span>
              <span class="status-zh" :class="{ perm: s.status === 'needsPermission', reply: s.status === 'waitingForReply' }">{{ STATUS_ZH[s.status] }}</span>
            </div>
            <div class="line2">{{ projShort(s.project) }}</div>
          </div>
          <span class="ago" :class="{ fresh: isFresh(s) }">
            <span v-if="isFresh(s)" class="fresh-dot" />
            {{ agoF(s.statusUpdatedAt) }}
          </span>
          <div class="actions">
            <button
              v-if="s.alive && s.snoozed"
              class="snooze-btn"
              title="恢复（取消搁置）"
              @click.stop="unsnooze(s.id)"
            >恢复</button>
            <button
              v-else-if="s.alive && (s.status === 'waitingForInput' || s.status === 'waitingForReply')"
              class="snooze-btn"
              title="搁置（暂时不管）"
              @click.stop="snooze(s.id)"
            >搁置</button>
            <button
              class="hide-btn"
              :title="hidden.includes(s.id) ? '取消隐藏' : '隐藏'"
              @click.stop="hidden.includes(s.id) ? unhide(s.id) : hide(s.id)"
            >{{ hidden.includes(s.id) ? '+' : '×' }}</button>
          </div>
        </li>
      </template>
      <li v-if="section.hidden > 0" class="dead-more">
        +{{ section.hidden }} 个更早的已隐藏
      </li>
    </template>
    <li v-if="sections.length === 0" class="empty">暂无会话</li>
  </ul>
</template>

<style scoped>
.list {
  list-style: none;
  margin: 0;
  padding: 0;
}

/* 一级分组小标题：待介入 / 已搁置 / 已退出 */
.group-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 9px var(--pad-x) 4px;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-tertiary);
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.group-head .cnt {
  color: var(--color-tertiary);
  background: var(--color-border);
  border-radius: 8px;
  padding: 0 6px;
  font-size: 10px;
  line-height: 14px;
  font-variant-numeric: tabular-nums;
}
/* 二级项目小标题 */
.proj-head {
  padding: 5px var(--pad-x) 2px;
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-tertiary);
}
.group-sep {
  height: 1px;
  background: var(--color-border);
  margin: 4px var(--gap) 0;
}
.dead-more {
  padding: 4px var(--pad-x) 7px;
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-tertiary);
}

/* 紧凑行：var(--row-hud) 高 */
.row {
  display: flex;
  align-items: center;
  gap: var(--gap);
  height: var(--row-hud);
  padding: 0 var(--pad-x);
  cursor: pointer;
  border-left: 2px solid transparent;
  transition: background var(--motion-duration) var(--motion-easing);
}
.row:hover {
  background: var(--color-hover);
}
.row:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: -2px;
}

/* NeedsPermission 行（非搁置）：左侧 2px 橙边框 + 浅橙背景 */
.row.perm-row {
  border-left-color: var(--status-permission);
  background: color-mix(in srgb, var(--status-permission) 10%, transparent);
}
.row.perm-row:hover {
  background: color-mix(in srgb, var(--status-permission) 16%, transparent);
}

/* WaitingForReply 行（非搁置）：左侧 2px 黄边框 + 浅黄背景（过程中提问，次紧急）*/
.row.reply-row {
  border-left-color: var(--status-reply);
  background: color-mix(in srgb, var(--status-reply) 12%, transparent);
}
.row.reply-row:hover {
  background: color-mix(in srgb, var(--status-reply) 18%, transparent);
}

/* dead 行半透明 */
.row.dead {
  opacity: 0.45;
}
/* 搁置行灰显沉底（与 dead 区分：0.5 vs 0.45） */
.row.snoozed {
  opacity: 0.5;
}

.icon {
  flex-shrink: 0;
}

/* 会话名 + 状态中文 */
.info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  line-height: var(--lh-body);
}
.line1 {
  display: flex;
  align-items: baseline;
  gap: var(--gap-sm);
}
.name {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.status-zh {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-muted);
  flex-shrink: 0;
}
/* needsPermission 状态文字染橙（与 Overlay 一致，强调最紧急） */
.status-zh.perm {
  color: var(--status-permission);
}
/* waitingForReply 状态文字染黄（次紧急，过程中提问） */
.status-zh.reply {
  color: var(--status-reply);
}
.line2 {
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}

/* ago 时间：等宽数据列 */
.ago {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-tertiary);
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: var(--gap-xs);
  font-variant-numeric: tabular-nums;
}

/* "刚完成"高亮：主色蓝 + 小圆点 */
.ago.fresh {
  color: var(--color-primary);
}
.fresh-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--color-primary);
  display: inline-block;
}

/* 按钮区：搁置/恢复（文字）+ 隐藏（×/+）并列 */
.actions {
  display: flex;
  gap: var(--gap-xs);
  flex-shrink: 0;
  align-items: center;
}

/* 搁置/恢复按钮：文字按钮，淡背景区分于图标按钮 */
.snooze-btn {
  background: var(--color-border);
  border: none;
  color: var(--color-muted);
  cursor: pointer;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  padding: 3px var(--gap-sm);
  border-radius: 4px;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.snooze-btn:hover {
  background: var(--color-hover);
  color: var(--color-fg);
}
.snooze-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}

/* 隐藏/恢复按钮（× / +） */
.hide-btn {
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  font-size: var(--fs-control);
  line-height: 1;
  padding: 2px var(--gap-xs);
  border-radius: 4px;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.hide-btn:hover {
  color: var(--color-fg);
  background: var(--color-hover);
}
.hide-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}

/* 空状态 */
.empty {
  padding: var(--space-empty) var(--pad-x);
  text-align: center;
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-tertiary);
}
</style>
