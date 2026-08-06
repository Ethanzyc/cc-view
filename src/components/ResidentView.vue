<script setup lang="ts">
// 常驻模式视图：精简会话列表，贴桌面常驻、失焦不收起（后端控制）。
// A/B 都按项目分组（项目名作二级标题，醒目）；B 行带状态文字，A 行不带（极简）。
// 展开入口（右上角，四角向外 maximize 图标，与面板收起 minimize 成对）调 set_mode(panel)。
// 透明度作用于 --resident-bg；高度自适应 ResizeObserver → set_resident_height。
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { Session, ResidentLayout } from '../types';
import StatusIcon from './StatusIcon.vue';
import { STATUS_ZH, projShort, isStaleInput } from '../utils/session';

const all = ref<Session[]>([]);
// now tick：isStaleInput 依赖时间，需前端定期刷新（后端 emit 有 hash 去重不随时间触发）。
const now = ref(Date.now());
let nowTimer: number | undefined;
const rootEl = ref<HTMLElement>();
const layout = ref<ResidentLayout>('b');
const showSnoozed = ref(true);
const showIdle = ref(true);
const opacity = ref(55);
let unlistenSessions: (() => void) | undefined;
let unlistenPrefs: (() => void) | undefined;

// 分组（待介入 / 已搁置）→ 项目聚类 → 行。A/B 共用，行状态文字按 layout 显隐。
type Section = { key: string; label: string; total: number; projs: [string, Session[]][] };
const groups = computed<Section[]>(() => {
  const list = all.value
    .filter(s => s.alive)
    .filter(s => showSnoozed.value || !s.snoozed)
    .filter(s => showIdle.value || !isStaleInput(s, now.value));
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
    // 全闲置 project 沉底
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

async function expandToPanel() {
  try {
    await invoke('set_mode', { mode: 'panel' });
  } catch (e) {
    console.error('set_mode(panel) failed', e);
  }
}

function applyOpacity() {
  const a = (opacity.value / 100).toFixed(3);
  const light = window.matchMedia('(prefers-color-scheme: light)').matches;
  const rgb = light ? '255, 255, 255' : '28, 28, 30';
  document.documentElement.style.setProperty('--resident-bg', `rgba(${rgb}, ${a})`);
}

let resizeRaf = 0;
let ro: ResizeObserver | undefined;
function syncHeight() {
  cancelAnimationFrame(resizeRaf);
  resizeRaf = requestAnimationFrame(async () => {
    const el = rootEl.value;
    if (!el) return;
    const maxCap = Math.round(window.screen.availHeight * 0.6);
    const h = Math.min(Math.round(el.scrollHeight), maxCap);
    if (h > 0) {
      try {
        await invoke('set_resident_height', { height: h });
      } catch (e) {
        console.error('set_resident_height failed', e);
      }
    }
  });
}

onMounted(async () => {
  try {
    const p = await invoke<{
      resident_layout: ResidentLayout;
      resident_show_snoozed: boolean;
      resident_show_idle: boolean;
      resident_opacity: number;
    }>('get_prefs');
    layout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
    opacity.value = p.resident_opacity;
    applyOpacity();
  } catch (e) {
    console.error('get_prefs resident config failed', e);
  }
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
  }
  try {
    unlistenSessions = await listen<Session[]>('sessions', e => { all.value = e.payload; });
  } catch (e) {
    console.error('resident listen sessions failed', e);
  }
  try {
    unlistenPrefs = await listen('prefs_changed', async () => {
      try {
        const p = await invoke<{
          resident_layout: ResidentLayout;
          resident_show_snoozed: boolean;
          resident_show_idle: boolean;
          resident_opacity: number;
        }>('get_prefs');
        layout.value = p.resident_layout;
        showSnoozed.value = p.resident_show_snoozed;
        showIdle.value = p.resident_show_idle;
        opacity.value = p.resident_opacity;
        applyOpacity();
      } catch (e) {
        console.error('prefs_changed reload failed', e);
      }
    });
  } catch (e) {
    console.error('listen prefs_changed failed', e);
  }
  nowTimer = window.setInterval(() => { now.value = Date.now(); }, 60_000);

  if (rootEl.value) {
    ro = new ResizeObserver(() => syncHeight());
    ro.observe(rootEl.value);
  }
  syncHeight();
});

onBeforeUnmount(() => {
  if (nowTimer) clearInterval(nowTimer);
  if (ro) ro.disconnect();
  if (unlistenSessions) unlistenSessions();
  if (unlistenPrefs) unlistenPrefs();
  cancelAnimationFrame(resizeRaf);
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
      <!-- 四角向外（maximize），与面板收起的 minimize（四角向内）成对 -->
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 8V5a2 2 0 0 1 2-2h3" />
        <path d="M16 3h3a2 2 0 0 1 2 2v3" />
        <path d="M21 16v3a2 2 0 0 1-2 2h-3" />
        <path d="M8 21H5a2 2 0 0 1-2-2v-3" />
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
          <span v-if="layout === 'b'" class="st" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
        </div>
      </template>
    </template>
    <div v-if="!groups.length" class="empty">暂无会话</div>
  </div>
</template>

<style scoped>
.resident {
  position: relative;
  background: var(--resident-bg);
  color: var(--color-fg);
  max-height: 100vh;
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
