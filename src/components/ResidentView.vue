<script setup lang="ts">
// 常驻模式视图：精简会话列表，贴桌面常驻、失焦不收起（后端控制）。
// 数据自管（listen sessions），排序/分组/ago 复用 utils/session.ts（MVP 与 PanelView 重复可接受）。
// 展开入口（右上角）调 set_mode(panel) → App.vue 切 PanelView。
// 本骨架先做 B 布局（分组 + 项目标题 + 图标+名称+状态）；A 布局/过滤/透明度/高度后续 task 接入。
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { Session } from '../types';
import StatusIcon from './StatusIcon.vue';
import { STATUS_ZH, projShort, isStaleInput } from '../utils/session';

const all = ref<Session[]>([]);
// now tick：isStaleInput 依赖时间，需前端定期刷新（后端 emit 有 hash 去重不随时间触发）。
const now = ref(Date.now());
let nowTimer: number | undefined;
const rootEl = ref<HTMLElement>();

// 非搜索态分组（与 PanelView 一致算法，MVP 重复）：待介入 / 已搁置；常驻只看活会话。
type Section = { key: string; label: string; total: number; projs: [string, Session[]][] };
const groups = computed<Section[]>(() => {
  const list = all.value.filter(s => s.alive); // 常驻只看活会话
  const active = list.filter(s => !s.snoozed);
  const snoozedAlive = list.filter(s => s.snoozed);
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
  if (active.length) {
    const n = now.value;
    // 全闲置 project 沉底（与 PanelView 一致）
    const activeProjs = byProj(active).sort((a, b) => {
      const aStale = a[1].every(s => isStaleInput(s, n));
      const bStale = b[1].every(s => isStaleInput(s, n));
      if (aStale !== bStale) return aStale ? 1 : -1;
      return 0;
    });
    result.push({ key: 'active', label: '待介入', total: active.length, projs: activeProjs });
  }
  if (snoozedAlive.length) {
    result.push({ key: 'snoozed', label: '已搁置', total: snoozedAlive.length, projs: byProj(snoozedAlive) });
  }
  return result;
});

async function focusSession(id: string) {
  try {
    await invoke('focus_session', { id });
  } catch (e) {
    console.error('focus_session failed', e);
  }
}

// 展开成面板模式：后端 set_mode + emit mode_changed → App 切 PanelView。
async function expandToPanel() {
  try {
    await invoke('set_mode', { mode: 'panel' });
  } catch (e) {
    console.error('set_mode(panel) failed', e);
  }
}

onMounted(async () => {
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
  }
  try {
    await listen<Session[]>('sessions', e => { all.value = e.payload; });
  } catch (e) {
    console.error('resident listen sessions failed', e);
  }
  nowTimer = window.setInterval(() => { now.value = Date.now(); }, 60_000);
});

onBeforeUnmount(() => {
  if (nowTimer) clearInterval(nowTimer);
});
</script>

<template>
  <div class="resident" ref="rootEl" data-tauri-drag-region="deep">
    <button
      class="expand-btn"
      title="展开成命令面板"
      aria-label="展开成命令面板"
      data-tauri-drag-region="false"
      @click="expandToPanel"
    >
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M5 11 L11 5" /><path d="M6 5 H11 V10" />
      </svg>
    </button>
    <template v-for="(section, si) in groups" :key="section.key">
      <div v-if="si > 0" class="group-sep" />
      <div class="group-head">
        {{ section.label }} <span class="cnt">{{ section.total }}</span>
      </div>
      <template v-for="[proj, rows] in section.projs" :key="section.key + '|' + proj">
        <div class="proj-head">{{ projShort(proj) }}</div>
        <div
          v-for="s in rows"
          :key="s.id"
          class="row"
          :class="{
            perm: s.status === 'needsPermission' && !s.snoozed,
            reply: s.status === 'waitingForReply' && !s.snoozed,
            dim: s.snoozed || isStaleInput(s, now),
          }"
          role="button"
          tabindex="0"
          :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
          data-tauri-drag-region="false"
          @click="focusSession(s.id)"
          @keydown.enter.prevent="focusSession(s.id)"
        >
          <StatusIcon :status="s.status" class="icon" />
          <span class="name">{{ s.name || s.project }}</span>
          <span class="st" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
        </div>
      </template>
    </template>
    <div v-if="!groups.length" class="empty">暂无会话</div>
  </div>
</template>

<style scoped>
.resident {
  position: relative;
  background: var(--color-bg-overlay);
  color: var(--color-fg);
  min-height: 100vh;
  max-height: 60vh;
  overflow-y: auto;
  border-radius: var(--radius-overlay);
  font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
  padding: 6px 0 8px;
}
.resident::-webkit-scrollbar { width: 6px; }
.resident::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }

.expand-btn {
  position: absolute; top: 5px; right: 6px;
  width: 18px; height: 18px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 5px; border: none; background: none;
  color: var(--color-tertiary); cursor: pointer; padding: 0;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.expand-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.expand-btn:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 1px; }

.group-head {
  display: flex; align-items: center; gap: 5px;
  padding: 9px 12px 3px;
  font: 600 9px/1 var(--font-utility);
  letter-spacing: 0.06em; text-transform: uppercase;
  color: var(--color-muted);
}
.group-head .cnt {
  color: var(--color-tertiary); background: var(--color-border);
  border-radius: 7px; padding: 0 5px; font-size: 9px; line-height: 13px;
}
.proj-head {
  padding: 4px 12px 1px;
  font: 600 10px/1 var(--font-utility);
  color: var(--color-muted);
}
.group-sep { height: 1px; background: var(--color-border); margin: 4px 10px 0; }

.row {
  display: flex; align-items: center; gap: 8px;
  height: 26px; padding: 0 12px;
  border-left: 2px solid transparent;
  cursor: pointer;
  transition: background var(--motion-duration) var(--motion-easing);
}
.row:hover { background: var(--color-hover); }
.row:focus-visible { outline: 2px solid var(--color-primary); outline-offset: -2px; }
.row.perm { border-left-color: var(--status-permission); background: color-mix(in srgb, var(--status-permission) 10%, transparent); }
.row.perm:hover { background: color-mix(in srgb, var(--status-permission) 16%, transparent); }
.row.reply { border-left-color: var(--status-reply); }
.row.dim { opacity: 0.45; }

.icon { flex-shrink: 0; }
.name {
  flex: 1; min-width: 0;
  font: 600 12px/1 var(--font-body);
  color: var(--color-fg);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.st {
  flex-shrink: 0; padding-left: 8px;
  font: 400 10px/1 var(--font-body);
  color: var(--color-muted);
}
.st.perm { color: var(--status-permission); }

.empty {
  padding: 32px 12px; text-align: center;
  font: 600 12px/1.3 var(--font-body);
  color: var(--color-tertiary);
}
</style>
