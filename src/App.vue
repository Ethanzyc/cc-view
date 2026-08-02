<script setup lang="ts">
// 按 window label 区分渲染：label === "overlay" → Overlay 命令面板；else（main）→ 现有 HUD。
// 两个窗口加载同一 index.html，靠 getCurrentWebviewWindow().label 分流（同步，无 IO）。
// Overlay 自管 sessions 监听 + 排序，不依赖 HUD 状态——MVP 不抽 composable。
import { ref, onMounted, computed } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import SessionList from './components/SessionList.vue';
import Overlay from './components/Overlay.vue';
import type { Session } from './types';

const isOverlay = getCurrentWebviewWindow().label === 'overlay';

// --- HUD（main 窗口）状态：all + hidden + showHidden ---
// App 维护两份状态：all（后端 emit 的全量 merged）+ hidden（list_hidden 拉到的隐藏 id 集）。
// visible computed 按 showHidden toggle 决定是否过滤。SessionList hide 成功后 emit 'hide'
// 触发 refreshHidden 重新拉列表，让 visible 立即反映新状态（无需等 3s 轮询）。
const all = ref<Session[]>([]);
const hidden = ref<string[]>([]);
const showHidden = ref(false);
const visible = computed(() =>
  showHidden.value ? all.value : all.value.filter(s => !hidden.value.includes(s.id)),
);

// 活跃计数（alive && status !== compacting 之外的都算活跃）
const activeCount = computed(() =>
  visible.value.filter(s => s.alive).length,
);

async function refreshHidden() {
  // fail fast：invoke 失败抛出由 onMounted/@click 调用者兜底；这里不吞异常
  hidden.value = await invoke<string[]>('list_hidden');
}

// overlay 自管 sessions 监听，HUD 才需要 hidden/listen 逻辑。
onMounted(async () => {
  if (isOverlay) return; // Overlay 组件自己 listen
  try {
    await refreshHidden();
  } catch (e) {
    console.error('refreshHidden on mount failed', e);
  }
  await listen<Session[]>('sessions', e => { all.value = e.payload; });
});
</script>

<template>
  <!-- overlay 分支：渲染命令面板 -->
  <Overlay v-if="isOverlay" />
  <!-- HUD 分支：现有 main 窗口 UI -->
  <div v-else class="app">
    <header class="title-bar">
      <span class="title">Claude Code 会话</span>
      <span class="count">{{ activeCount }} 个活跃</span>
      <span class="spacer" />
      <label class="toggle">
        <input type="checkbox" v-model="showHidden" />
        <span>显示已隐藏</span>
      </label>
      <button class="refresh-btn" title="刷新" @click="refreshHidden">
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M13.5 8 A5.5 5.5 0 1 1 11 3.6" />
          <path d="M13.5 2.5 V5 H11" />
        </svg>
      </button>
    </header>
    <div class="divider" />
    <div class="list-scroll">
      <SessionList
        :sessions="visible"
        :hidden="hidden"
        @hide="refreshHidden"
        @unhide="refreshHidden"
      />
    </div>
  </div>
</template>

<style>
/* 深色（默认） */
:root {
  --text-primary: #E5E5E7;
  --text-secondary: #8E8E93;
  --text-tertiary: #6E6E73;
  --divider: rgba(255, 255, 255, 0.08);
  --header-bg: transparent;
}

@media (prefers-color-scheme: light) {
  :root {
    --text-primary: #1D1D1F;
    --text-secondary: #8E8E93;
    --text-tertiary: #6E6E73;
    --divider: rgba(0, 0, 0, 0.08);
  }
}

* { box-sizing: border-box; }

html, body {
  margin: 0;
  padding: 0;
  background: transparent;
  font-family: -apple-system, "PingFang SC", "SF Pro Text", sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  /* 防止用户选中 popover 中的文字（原生 popover 体验） */
  user-select: none;
  -webkit-user-select: none;
}

/* popover 容器：背景透明，由后端 NSVisualEffectView vibrancy 提供毛玻璃 */
.app {
  background: transparent;
  border-radius: 8px;
  overflow: hidden;
  color: var(--text-primary);
  min-height: 100vh;
}

/* title-bar：整条可拖动 HUD（-webkit-app-region: drag），
   内部按钮/checkbox 标 no-drag 以保留点击。 */
.title-bar {
  -webkit-app-region: drag;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px 8px;
}
.title-bar .toggle,
.title-bar .refresh-btn {
  -webkit-app-region: no-drag;
}
.title {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}
.count {
  font-size: 11px;
  color: var(--text-tertiary);
  font-weight: 400;
}
.spacer { flex: 1; }

.toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  cursor: pointer;
  font-weight: 400;
}
.toggle input {
  margin: 0;
  width: 12px;
  height: 12px;
  accent-color: #0A84FF;
  cursor: pointer;
}

.refresh-btn {
  background: none;
  border: none;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  transition: color 0.12s ease, background 0.12s ease;
}
.refresh-btn:hover {
  color: var(--text-primary);
  background: rgba(255, 255, 255, 0.08);
}
@media (prefers-color-scheme: light) {
  .refresh-btn:hover {
    background: rgba(0, 0, 0, 0.06);
  }
}

.divider {
  height: 1px;
  background: var(--divider);
  margin: 0 8px;
}

/* 列表区可滚动 */
.list-scroll {
  max-height: 460px;
  overflow-y: auto;
  padding: 4px 0;
}

/* 自定义滚动条（macOS 风格 overlay） */
.list-scroll::-webkit-scrollbar {
  width: 6px;
}
.list-scroll::-webkit-scrollbar-track {
  background: transparent;
}
.list-scroll::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.15);
  border-radius: 3px;
}
@media (prefers-color-scheme: light) {
  .list-scroll::-webkit-scrollbar-thumb {
    background: rgba(0, 0, 0, 0.15);
  }
}
</style>
