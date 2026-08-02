<script setup lang="ts">
// Overlay 命令面板：搜索框 + 会话列表 + 每行操作（focus/隐藏/复制 ID）。
// 数据自管——直接 listen "sessions" event（与 HUD 同事件，互不干扰）。
// 排序逻辑与 SessionList 一致（statusRank + ago 升序）；MVP 重复可接受。
// 隐藏列表不做过滤——overlay 是"快速启动器"，隐藏项仍可搜索到（恢复走 HUD）。
import { ref, computed, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { Session } from '../types';
import StatusIcon from './StatusIcon.vue';

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

// 隐藏会话：失败 console.error，UI 不崩。
async function hideSession(id: string) {
  try {
    await invoke('hide_session', { id });
  } catch (e) {
    console.error('hide_session failed', e);
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
          @click="focusSession(s.id)"
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
              class="act-btn"
              title="隐藏"
              @click.stop="hideSession(s.id)"
            >隐藏</button>
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
  background: transparent;
  color: var(--text-primary);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

/* 搜索栏：顶部贴边，不随列表滚动 */
.search-bar {
  -webkit-app-region: drag;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 14px 10px;
}
.search-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.search {
  -webkit-app-region: no-drag;
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  font-size: 15px;
  color: var(--text-primary);
  font-family: inherit;
  -webkit-font-smoothing: antialiased;
}
.search::placeholder {
  color: var(--text-tertiary);
}

.divider {
  height: 1px;
  background: var(--divider);
  margin: 0 8px;
}

.list-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0 8px;
}
.list-scroll::-webkit-scrollbar { width: 6px; }
.list-scroll::-webkit-scrollbar-track { background: transparent; }
.list-scroll::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.15);
  border-radius: 3px;
}
@media (prefers-color-scheme: light) {
  .list-scroll::-webkit-scrollbar-thumb { background: rgba(0, 0, 0, 0.15); }
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
}

/* 行：44px 高（比 HUD 的 36 大些，命令面板风格） */
.row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 44px;
  padding: 0 14px;
  cursor: pointer;
  transition: background 0.1s ease;
}
.row:hover {
  background: rgba(255, 255, 255, 0.08);
}
@media (prefers-color-scheme: light) {
  .row:hover { background: rgba(0, 0, 0, 0.05); }
}
.row.dead { opacity: 0.45; }

.icon { flex-shrink: 0; }

.info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  line-height: 1.2;
}
.line1 { display: flex; align-items: baseline; gap: 6px; }
.name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.line2 {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 2px;
}

/* 操作按钮组：默认半透明，hover 行时凸显 */
.actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
  opacity: 0.6;
  transition: opacity 0.12s ease;
}
.row:hover .actions { opacity: 1; }

.act-btn {
  background: rgba(255, 255, 255, 0.06);
  border: none;
  color: var(--text-secondary);
  font-size: 11px;
  font-family: inherit;
  padding: 4px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
  -webkit-font-smoothing: antialiased;
}
.act-btn:hover {
  background: rgba(255, 255, 255, 0.14);
  color: var(--text-primary);
}
@media (prefers-color-scheme: light) {
  .act-btn {
    background: rgba(0, 0, 0, 0.05);
    color: var(--text-secondary);
  }
  .act-btn:hover {
    background: rgba(0, 0, 0, 0.1);
    color: var(--text-primary);
  }
}

/* 复制成功状态：蓝色 */
.act-btn.copy.done {
  color: #0A84FF;
  background: rgba(10, 132, 255, 0.12);
}

.empty {
  padding: 40px 16px;
  text-align: center;
  font-size: 13px;
  color: var(--text-tertiary);
}
</style>
