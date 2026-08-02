<script setup lang="ts">
// 每行渲染一个会话；右侧按钮按 hidden 判断：
//   已隐藏 → "+" 调 unhide_session，未隐藏 → "×" 调 hide_session。
// 成功后 emit('hide'|'unhide') 让父组件 App 刷新 hidden 集合 → visible computed 更新。
// 行点击调 focus_session 激活对应 host 终端；按钮用 @click.stop 阻止冒泡到行。
import { computed } from 'vue';
import type { Session, Status } from '../types';
import { invoke } from '@tauri-apps/api/core';
import StatusIcon from './StatusIcon.vue';

const props = withDefaults(defineProps<{ sessions: Session[]; hidden?: string[] }>(), {
  hidden: () => [] as string[],
});
const emit = defineEmits<{
  (e: 'hide', id: string): void;
  (e: 'unhide', id: string): void;
}>();

// 状态中文名（紧凑 popover 显示）
const STATUS_ZH: Record<Status, string> = {
  working: '工作中',
  waitingForInput: '等输入',
  needsPermission: '等权限',
  shell: 'Shell',
  compacting: '压缩中',
};

// 排序优先级：等权限 > 等输入 > 工作中 > Shell > 压缩中 > 死亡
function statusRank(s: Session): number {
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

// 排序：按 rank → ago 升序（最近的靠前）
const sorted = computed(() =>
  [...props.sessions].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    return a.statusUpdatedAt - b.statusUpdatedAt;
  }),
);

// ago 自适应：<60s→Xs, <3600→Xm, <86400→Xh, else→Xd
function agoF(ts: number): string {
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

// "刚完成"高亮：status === waitingForInput 且 ago < 120s
function isFresh(s: Session): boolean {
  return s.status === 'waitingForInput' &&
    Date.now() - s.statusUpdatedAt < 120_000;
}

// invoke 失败时 console.error 记录，UI 不崩（按钮交互不应让 app 崩溃）
async function hide(id: string) {
  try {
    await invoke('hide_session', { id });
    emit('hide', id);
  } catch (e) {
    console.error('hide failed', e);
  }
}
// invoke 失败时 console.error 记录，UI 不崩
async function unhide(id: string) {
  try {
    await invoke('unhide_session', { id });
    emit('unhide', id);
  } catch (e) {
    console.error('unhide failed', e);
  }
}
// 行点击：激活该 session 对应的 host 终端；失败仅 console.error，不阻断 UI
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
    <li
      v-for="s in sorted"
      :key="s.id"
      class="row"
      :class="{
        dead: !s.alive,
        'perm-row': s.status === 'needsPermission',
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
          <span class="status-zh">{{ STATUS_ZH[s.status] }}</span>
        </div>
        <div class="line2">{{ s.project }}</div>
      </div>
      <span class="ago" :class="{ fresh: isFresh(s) }">
        <span v-if="isFresh(s)" class="fresh-dot" />
        {{ agoF(s.statusUpdatedAt) }}
      </span>
      <button
        class="hide-btn"
        :title="hidden.includes(s.id) ? '恢复' : '隐藏'"
        @click.stop="hidden.includes(s.id) ? unhide(s.id) : hide(s.id)"
      >{{ hidden.includes(s.id) ? '+' : '×' }}</button>
    </li>
  </ul>
</template>

<style scoped>
.list {
  list-style: none;
  margin: 0;
  padding: 0;
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

/* NeedsPermission 行：左侧 2px 橙边框 + 浅橙背景（color-mix 派生自状态色 token） */
.row.perm-row {
  border-left-color: var(--status-permission);
  background: color-mix(in srgb, var(--status-permission) 10%, transparent);
}
.row.perm-row:hover {
  background: color-mix(in srgb, var(--status-permission) 16%, transparent);
}

/* dead 行半透明 */
.row.dead {
  opacity: 0.45;
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
  gap: 6px;
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
  gap: 3px;
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

/* 隐藏/恢复按钮 */
.hide-btn {
  flex-shrink: 0;
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  font-size: 15px;
  line-height: 1;
  padding: 2px 4px;
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
</style>
