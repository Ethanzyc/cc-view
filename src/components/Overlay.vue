<script setup lang="ts">
// Overlay 命令面板：搜索框 + 会话列表 + 每行操作（focus / 复制 ID / 搁置 / 恢复）。
// 数据自管——直接 listen "sessions" event（与 HUD 同事件，互不干扰）。
// 排序/分组/ago/isFresh 算法与 SessionList 一致（MVP 重复可接受；不抽 composable）。
// 搜索态：扁平列表 + span 拆分高亮 + 计数；非搜索态：分组（同 HUD）。
// 隐藏列表不过滤——Overlay 是"快速启动器"，hidden 也能搜到（恢复走 HUD）。
// 搁置/恢复：成功后直接改 all.value 里对应 session.snoozed（Overlay 自管乐观更新，不等 poll）。
import { ref, computed, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Session, Status } from '../types';
import StatusIcon from './StatusIcon.vue';

// 状态中文名（行内 status-zh；与 SessionList 保持一致）
const STATUS_ZH: Record<Status, string> = {
  working: '工作中',
  waitingForInput: '等输入',
  needsPermission: '等权限',
  shell: 'Shell',
  compacting: '压缩中',
};

const all = ref<Session[]>([]);
const q = ref('');
// 复制成功反馈：id → true，1.2s 后清除（让用户知道复制生效）
const copiedId = ref<string | null>(null);
const searchRef = ref<HTMLInputElement>();

// 排序档：与 SessionList 一致——等权限 > 等输入 > 工作 > Shell > 压缩 > 搁置(alive 5.5) > 已退出(6) > 搁置(dead 6.5)
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

// 项目路径缩短：/Users/<name>/ai/fang → ~/ai/fang（行 line2 + 搜索匹配共用）
const projShort = (p: string) => p.replace(/^\/Users\/[^/]+\//, '~/');

// 全集排序：rank → project 字母序 → statusUpdatedAt 降序（最近变更靠前）
const sorted = computed(() =>
  [...all.value].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    const pc = a.project.localeCompare(b.project);
    if (pc !== 0) return pc;
    return b.statusUpdatedAt - a.statusUpdatedAt;
  }),
);

// 搜索：trim 非空即激活；filter name + projShort（大小写不敏感）
const searchActive = computed(() => q.value.trim().length > 0);
// 当前关键字（小写，传给 hlParts；模板里复用避免重复 trim）
const kLower = computed(() => q.value.trim().toLowerCase());
const flatResults = computed(() => {
  const k = kLower.value;
  if (!k) return [];
  return sorted.value.filter(s =>
    (s.name + ' ' + projShort(s.project)).toLowerCase().includes(k),
  );
});

// 非搜索态：分组（同 SessionList —— 待介入 / 已搁置 / 已退出；dead 限 5）
const DEAD_LIMIT = 5;
type Section = {
  key: string;
  label: string;
  total: number;
  projs: [string, Session[]][];
  hidden: number;
};
const groups = computed<Section[]>(() => {
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

// 计数：搜索态显示结果数；非搜索态显示待介入数（active 组总数，无则 0）
const overlayCount = computed(() =>
  searchActive.value
    ? `${flatResults.value.length} 个结果`
    : `${groups.value.find(g => g.key === 'active')?.total ?? 0} 待介入`,
);

// ago 自适应：<60s→Xs, <3600→Xm, <86400→Xh, else→Xd
function agoF(ts: number): string {
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

// "刚完成"高亮：waitingForInput 且 ago < 120s 且未搁置（搁置行不显示蓝点）
function isFresh(s: Session): boolean {
  return !s.snoozed &&
    s.status === 'waitingForInput' &&
    Date.now() - s.statusUpdatedAt < 120_000;
}

// 高亮 span 拆分：返回段数组（匹配段 hl:true）。
// 不用 v-html——生产安全，避免 XSS（session name 可能含恶意内容）。
// 未匹配/无关键字时返回单段，模板只渲染一个普通 span。
type HlSeg = { text: string; hl: boolean };
function hlParts(text: string, k: string): HlSeg[] {
  if (!k) return [{ text, hl: false }];
  const i = text.toLowerCase().indexOf(k);
  if (i < 0) return [{ text, hl: false }];
  return [
    { text: text.slice(0, i), hl: false },
    { text: text.slice(i, i + k.length), hl: true },
    { text: text.slice(i + k.length), hl: false },
  ].filter(seg => seg.text.length > 0);
}

// 搁置/恢复：成功后直接改 all.value 对应 session.snoozed（Overlay 自管，不等 3s poll）
async function snooze(id: string) {
  try {
    await invoke('snooze_session', { id });
    const s = all.value.find(x => x.id === id);
    if (s) s.snoozed = true;
  } catch (e) {
    console.error('snooze failed', e);
  }
}
async function unsnooze(id: string) {
  try {
    await invoke('unsnooze_session', { id });
    const s = all.value.find(x => x.id === id);
    if (s) s.snoozed = false;
  } catch (e) {
    console.error('unsnooze failed', e);
  }
}

// focus 成功后立即 hide overlay（Alfred 行为：选中即收起）。
// focus 失败时 console.error，不 hide（让用户看到错误痕迹）。
async function focusSession(id: string) {
  try {
    await invoke('focus_session', { id });
    await getCurrentWebviewWindow().hide();
  } catch (e) {
    console.error('focus_session failed', e);
  }
}

// 复制 ID：优先 navigator.clipboard（WKWebView 支持，secure context）。
// 失败时 fallback 用 deprecated execCommand，再不行 console.error。
async function copyId(id: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(id);
    } else {
      // fallback：旧 webview 可能无 navigator.clipboard
      const ta = document.createElement('textarea');
      ta.value = id;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
    }
    copiedId.value = id;
    setTimeout(() => {
      if (copiedId.value === id) copiedId.value = null;
    }, 1200);
  } catch (e) {
    console.error('copyId failed', e);
  }
}

onMounted(async () => {
  // 打开即拉当前会话，不等 3s 轮询/hash 变化——避免空列表。
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
  }
  try {
    await listen<Session[]>('sessions', e => { all.value = e.payload; });
  } catch (e) {
    console.error('overlay listen sessions failed', e);
  }

  // 窗口获焦时 focus + select 搜索框（overlay show/hide 复用，autofocus 仅首次生效）
  const win = getCurrentWebviewWindow();
  await win.onFocusChanged(({ payload: focused }) => {
    if (focused && searchRef.value) {
      searchRef.value.focus();
      searchRef.value.select();
    }
  });
});
</script>

<template>
  <div class="overlay">
    <div class="search-bar">
      <svg class="search-icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="7" cy="7" r="4.5" />
        <path d="M10.5 10.5 L14 14" />
      </svg>
      <input
        ref="searchRef"
        class="search"
        v-model="q"
        placeholder="搜索会话（名称 / 项目）..."
        autofocus
        spellcheck="false"
      />
      <!-- 计数：搜索态→结果数；非搜索态→待介入数 -->
      <span class="overlay-count">{{ overlayCount }}</span>
    </div>
    <div class="divider" />
    <div class="list-scroll">
      <!-- 搜索态：扁平列表（不分组）+ span 拆分高亮 -->
      <ul class="list" v-if="searchActive">
        <li
          v-for="s in flatResults"
          :key="s.id"
          class="row"
          :class="{
            dead: !s.alive,
            snoozed: s.snoozed,
            perm: s.status === 'needsPermission' && !s.snoozed,
          }"
          role="button"
          tabindex="0"
          :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
          @click="focusSession(s.id)"
          @keydown.enter.prevent="focusSession(s.id)"
          @keydown.space.prevent="focusSession(s.id)"
        >
          <StatusIcon :status="s.status" class="icon" />
          <div class="info">
            <div class="line1">
              <span class="name">
                <span
                  v-for="(seg, i) in hlParts(s.name || s.project, kLower)"
                  :key="i"
                  :class="{ hl: seg.hl }"
                >{{ seg.text }}</span>
              </span>
              <span class="status-zh" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
            </div>
            <div class="line2">
              <span
                v-for="(seg, i) in hlParts(projShort(s.project), kLower)"
                :key="i"
                :class="{ hl: seg.hl }"
              >{{ seg.text }}</span>
            </div>
          </div>
          <span class="ago" :class="{ fresh: isFresh(s) }">
            <span v-if="isFresh(s)" class="fresh-dot" />
            {{ agoF(s.statusUpdatedAt) }}
          </span>
          <div class="actions">
            <button
              v-if="s.alive && s.snoozed"
              class="act-btn snooze"
              title="恢复（取消搁置）"
              @click.stop="unsnooze(s.id)"
            >恢复</button>
            <button
              v-else-if="s.alive && s.status === 'waitingForInput'"
              class="act-btn snooze"
              title="搁置（暂时不管）"
              @click.stop="snooze(s.id)"
            >搁置</button>
            <button
              class="act-btn copy"
              :class="{ done: copiedId === s.id }"
              :title="copiedId === s.id ? '已复制' : '复制 ID'"
              @click.stop="copyId(s.id)"
            >{{ copiedId === s.id ? '已复制' : '复制 ID' }}</button>
          </div>
        </li>
        <li v-if="!flatResults.length" class="empty">无匹配 "{{ q.trim() }}"</li>
      </ul>
      <!-- 非搜索态：分组（同 HUD） -->
      <ul class="list" v-else-if="groups.length">
        <template v-for="(section, si) in groups" :key="section.key">
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
                perm: s.status === 'needsPermission' && !s.snoozed,
              }"
              role="button"
              tabindex="0"
              :aria-label="`${s.name || s.project}，${STATUS_ZH[s.status]}`"
              @click="focusSession(s.id)"
              @keydown.enter.prevent="focusSession(s.id)"
              @keydown.space.prevent="focusSession(s.id)"
            >
              <StatusIcon :status="s.status" class="icon" />
              <div class="info">
                <div class="line1">
                  <span class="name">{{ s.name || s.project }}</span>
                  <span class="status-zh" :class="{ perm: s.status === 'needsPermission' }">{{ STATUS_ZH[s.status] }}</span>
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
                  class="act-btn snooze"
                  title="恢复（取消搁置）"
                  @click.stop="unsnooze(s.id)"
                >恢复</button>
                <button
                  v-else-if="s.alive && s.status === 'waitingForInput'"
                  class="act-btn snooze"
                  title="搁置（暂时不管）"
                  @click.stop="snooze(s.id)"
                >搁置</button>
                <button
                  class="act-btn copy"
                  :class="{ done: copiedId === s.id }"
                  :title="copiedId === s.id ? '已复制' : '复制 ID'"
                  @click.stop="copyId(s.id)"
                >{{ copiedId === s.id ? '已复制' : '复制 ID' }}</button>
              </div>
            </li>
          </template>
          <li v-if="section.hidden > 0" class="dead-more">
            +{{ section.hidden }} 个更早的已隐藏
          </li>
        </template>
      </ul>
      <div v-else class="empty">暂无会话</div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  background: var(--color-bg);
  color: var(--color-fg);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

/* 搜索栏：顶部贴边，不随列表滚动 */
.search-bar {
  -webkit-app-region: drag;
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: var(--pad-y) var(--pad-x);
  transition: box-shadow var(--motion-duration) var(--motion-easing);
}
.search-bar:focus-within {
  box-shadow: inset 0 -1.5px 0 var(--color-primary);
}
.search-icon {
  color: var(--color-tertiary);
  flex-shrink: 0;
}
.search {
  -webkit-app-region: no-drag;
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  font-family: inherit;
  -webkit-font-smoothing: antialiased;
}
.search::placeholder {
  color: var(--color-tertiary);
}
.search:focus-visible {
  outline: none;
}

/* 计数标签：搜索栏右侧，等宽数据列质感 */
.overlay-count {
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-tertiary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
  -webkit-app-region: no-drag;
}

.divider {
  height: 1px;
  background: var(--color-border);
  margin: 0 var(--gap);
}

.list-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--gap-xs) 0 var(--pad-y);
}
.list-scroll::-webkit-scrollbar { width: 6px; }
.list-scroll::-webkit-scrollbar-track { background: transparent; }
.list-scroll::-webkit-scrollbar-thumb {
  background: var(--color-border);
  border-radius: 3px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
}

/* 一级分组小标题（非搜索态）：待介入 / 已搁置 / 已退出 */
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

/* 行：var(--row-overlay) 高（命令面板紧凑行） */
.row {
  display: flex;
  align-items: center;
  gap: var(--gap);
  height: var(--row-overlay);
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

/* NeedsPermission 行（非搁置）：左侧 2px 橙边框 + 浅橙背景（参考 SessionList .perm-row） */
.row.perm {
  border-left-color: var(--status-permission);
  background: color-mix(in srgb, var(--status-permission) 10%, transparent);
}
.row.perm:hover {
  background: color-mix(in srgb, var(--status-permission) 16%, transparent);
}

/* dead 行半透明 */
.row.dead {
  opacity: 0.45;
}
/* 搁置行灰显沉底（与 dead 区分：0.5 vs 0.45） */
.row.snoozed {
  opacity: 0.5;
}

.icon { flex-shrink: 0; }

.info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  line-height: var(--lh-body);
}
.line1 { display: flex; align-items: baseline; gap: var(--gap-sm); }
.name {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* 搜索匹配段高亮（span 拆分，非 v-html） */
.hl {
  background: rgba(255, 214, 121, 0.28);
  color: #ffd479;
  border-radius: 2px;
}
.status-zh {
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-muted);
  flex-shrink: 0;
}
/* needsPermission 状态中文标橙（行已橙边，文字也橙，双重视觉提示） */
.status-zh.perm {
  color: var(--status-permission);
}
.line2 {
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 2px;
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

/* 操作按钮组：默认半透明，hover 行时凸显 */
.actions {
  display: flex;
  gap: var(--gap-xs);
  flex-shrink: 0;
  opacity: 0.6;
  transition: opacity var(--motion-duration) var(--motion-easing);
}
.row:hover .actions { opacity: 1; }

.act-btn {
  background: var(--color-border);
  border: none;
  color: var(--color-muted);
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  font-family: inherit;
  padding: var(--gap-xs) var(--gap);
  border-radius: 4px;
  cursor: pointer;
  transition: background var(--motion-duration) var(--motion-easing),
              color var(--motion-duration) var(--motion-easing);
  -webkit-font-smoothing: antialiased;
}
.act-btn:hover {
  background: var(--color-hover);
  color: var(--color-fg);
}
.act-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}

/* 复制成功状态：主色蓝 */
.act-btn.copy.done {
  color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 12%, transparent);
}

.empty {
  padding: var(--space-empty) var(--pad-x);
  text-align: center;
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-tertiary);
}
</style>
