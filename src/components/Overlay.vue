<script setup lang="ts">
// Overlay 命令面板：搜索框 + 会话列表 + 每行操作（focus/复制 ID）。
// 数据自管——直接 listen "sessions" event（与 HUD 同事件，互不干扰）。
// 排序逻辑与 SessionList 一致（statusRank + ago 升序）；MVP 重复可接受。
// 隐藏列表不做过滤——overlay 是"快速启动器"，隐藏项仍可搜索到（恢复走 HUD）。
import { ref, computed, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Session, Status } from '../types';
import StatusIcon from './StatusIcon.vue';

// 状态中文名（供 aria-label，与 SessionList 保持一致；不渲染可见文本）
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

// 状态排序优先级：等权限 > 等输入 > 工作 > Shell > 压缩 > 死亡
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

// 按搜索词过滤（name + project，大小写不敏感）→ 再按优先级排序
const visible = computed(() => {
  const sorted = [...all.value].sort((a, b) => {
    const ra = statusRank(a), rb = statusRank(b);
    if (ra !== rb) return ra - rb;
    return a.statusUpdatedAt - b.statusUpdatedAt;
  });
  const k = q.value.trim().toLowerCase();
  if (!k) return sorted;
  return sorted.filter(s =>
    (s.name + ' ' + s.project).toLowerCase().includes(k),
  );
});

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
    </div>
    <div class="divider" />
    <div class="list-scroll">
      <ul class="list" v-if="visible.length">
        <li
          v-for="s in visible"
          :key="s.id"
          class="row"
          :class="{ dead: !s.alive }"
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
            </div>
            <div class="line2">{{ s.project }}</div>
          </div>
          <div class="actions">
            <button
              class="act-btn copy"
              :class="{ done: copiedId === s.id }"
              :title="copiedId === s.id ? '已复制' : '复制 ID'"
              @click.stop="copyId(s.id)"
            >{{ copiedId === s.id ? '已复制' : '复制 ID' }}</button>
          </div>
        </li>
      </ul>
      <div v-else class="empty">
        {{ q ? '无匹配会话' : '暂无会话' }}
      </div>
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
  /* 键盘 focus 搜索框时容器显示 primary 下边框（search input 自身 outline:none） */
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
  /* 搜索框常驻焦点，不画 outline；focus 边框由容器承担 */
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

/* 行：var(--row-overlay) 高（命令面板紧凑行） */
.row {
  display: flex;
  align-items: center;
  gap: var(--gap);
  height: var(--row-overlay);
  padding: 0 var(--pad-x);
  cursor: pointer;
  transition: background var(--motion-duration) var(--motion-easing);
}
.row:hover {
  background: var(--color-hover);
}
.row:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: -2px;
}
.row.dead { opacity: 0.45; }

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
.line2 {
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 2px;
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
