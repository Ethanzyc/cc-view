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
      @click="focus(s.id)"
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

/* 紧凑行：36px 高 */
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 10px;
  cursor: pointer;
  border-left: 2px solid transparent;
  transition: background 0.12s ease;
}
.row:hover {
  background: rgba(255, 255, 255, 0.06);
}
@media (prefers-color-scheme: light) {
  .row:hover {
    background: rgba(0, 0, 0, 0.05);
  }
}

/* NeedsPermission 行：左侧 2px 橙边框 + 浅橙背景 */
.row.perm-row {
  border-left-color: #FF9F0A;
  background: rgba(255, 159, 10, 0.10);
}
.row.perm-row:hover {
  background: rgba(255, 159, 10, 0.16);
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
  line-height: 1.2;
}
.line1 {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.name {
  font-size: 13px;
  font-weight: 600;
  color: #E5E5E7;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.status-zh {
  font-size: 10px;
  font-weight: 400;
  color: #8E8E93;
  flex-shrink: 0;
}
.line2 {
  font-size: 11px;
  color: #8E8E93;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}

@media (prefers-color-scheme: light) {
  .name { color: #1D1D1F; }
  .status-zh { color: #8E8E93; }
  .line2 { color: #8E8E93; }
}

/* ago 时间 */
.ago {
  font-size: 11px;
  color: #6E6E73;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-variant-numeric: tabular-nums;
}
@media (prefers-color-scheme: light) {
  .ago { color: #6E6E73; }
}

/* "刚完成"高亮：蓝色 + 小圆点 */
.ago.fresh {
  color: #0A84FF;
}
.fresh-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #0A84FF;
  display: inline-block;
}

/* 隐藏/恢复按钮 */
.hide-btn {
  flex-shrink: 0;
  background: none;
  border: none;
  color: #6E6E73;
  cursor: pointer;
  font-size: 15px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 4px;
  transition: color 0.12s ease, background 0.12s ease;
}
.hide-btn:hover {
  color: #E5E5E7;
  background: rgba(255, 255, 255, 0.1);
}
@media (prefers-color-scheme: light) {
  .hide-btn { color: #6E6E73; }
  .hide-btn:hover {
    color: #1D1D1F;
    background: rgba(0, 0, 0, 0.08);
  }
}
</style>
