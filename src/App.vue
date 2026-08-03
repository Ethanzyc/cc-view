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

// 真刷新：同时拉 sessions + hidden（refresh-btn 调它，不再只刷 hidden）。
async function refreshAll() {
  try {
    const [sessions, hiddenIds] = await Promise.all([
      invoke<Session[]>('get_sessions'),
      invoke<string[]>('list_hidden'),
    ]);
    all.value = sessions;
    hidden.value = hiddenIds;
  } catch (e) {
    console.error('refreshAll failed', e);
  }
}

// overlay 自管 sessions 监听，HUD 才需要 hidden/listen 逻辑。
onMounted(async () => {
  if (isOverlay) return; // Overlay 组件自己 listen
  try {
    await refreshHidden();
  } catch (e) {
    console.error('refreshHidden on mount failed', e);
  }
  // 打开即拉当前会话，不等 3s 轮询/hash 变化——避免空列表。
  try {
    all.value = await invoke<Session[]>('get_sessions');
  } catch (e) {
    console.error('get_sessions on mount failed', e);
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
      <button class="refresh-btn" title="刷新" @click="refreshAll">
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
/* 设计 token（dark 默认 + light 覆盖） */
:root {
  /* color（dark 默认） */
  --color-bg: transparent;
  --color-fg: #E5E5E7;
  --color-muted: #8E8E93;
  --color-tertiary: #6E6E73;
  --color-primary: #0A84FF;
  --color-accent: #0A84FF;
  --color-border: rgba(255, 255, 255, 0.08);
  --color-hover: rgba(255, 255, 255, 0.08);
  /* 状态语义色（StatusIcon） */
  --status-working: #30D158;
  --status-waiting: #0A84FF;
  --status-permission: #FF9F0A;
  --status-shell: #BF5AF2;
  --status-compacting: #64D2FF;
  /* 字体 */
  --font-body: -apple-system, "PingFang SC", "SF Pro Text", sans-serif;
  --font-utility: "SF Mono", ui-monospace, "Menlo", monospace;
  /* 字号标度（compact） */
  --fs-display: 13px; --fw-display: 700; --lh-display: 1.3;
  --fs-body: 13px;    --fw-body: 600;    --lh-body: 1.25;
  --fs-caption: 11px; --fw-caption: 400; --lh-caption: 1.3;
  --fs-utility: 10px; --fw-utility: 400; --lh-utility: 1.3;
  /* 布局 */
  --radius-hud: 10px; --radius-overlay: 12px;
  --row-hud: 36px; --row-overlay: 36px;
  --pad-x: 12px; --pad-y: 8px; --gap: 8px;
  /* 动效 */
  --motion-duration: 160ms;
  --motion-easing: cubic-bezier(0.22, 1, 0.36, 1);
}

@media (prefers-color-scheme: light) {
  :root {
    --color-fg: #1D1D1F;
    --color-border: rgba(0, 0, 0, 0.08);
    --color-hover: rgba(0, 0, 0, 0.06);
    /* muted 提暗达标 AA（light 毛玻璃上原 #8E8E93 ~3.3:1 < 4.5）；其余系统色明暗一致，不覆盖 */
    --color-muted: #6E6E73;
  }
}

/* signature：Working 状态指示灯呼吸 */
@keyframes breathe {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.5; }
}
@media (prefers-reduced-motion: reduce) {
  .status-icon--working { animation: none !important; }
}

* { box-sizing: border-box; }

html, body {
  margin: 0;
  padding: 0;
  background: var(--color-bg);
  font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  /* 防止用户选中 popover 中的文字（原生 popover 体验） */
  user-select: none;
  -webkit-user-select: none;
}

/* popover 容器：背景透明，由后端 NSVisualEffectView vibrancy 提供毛玻璃 */
.app {
  background: var(--color-bg);
  border-radius: var(--radius-hud);
  overflow: hidden;
  color: var(--color-fg);
  min-height: 100vh;
}

/* title-bar：整条可拖动 HUD（-webkit-app-region: drag），
   内部按钮/checkbox 标 no-drag 以保留点击。 */
.title-bar {
  -webkit-app-region: drag;
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: var(--pad-y) var(--pad-x);
}
.title-bar .toggle,
.title-bar .refresh-btn {
  -webkit-app-region: no-drag;
}
.title {
  font: var(--fw-display) var(--fs-display)/var(--lh-display) var(--font-body);
  color: var(--color-fg);
  letter-spacing: -0.01em;
}
/* count 走等宽：仪表质感的数据列 */
.count {
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-tertiary);
  font-variant-numeric: tabular-nums;
}
.spacer { flex: 1; }

.toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
  cursor: pointer;
}
.toggle input {
  margin: 0;
  width: 12px;
  height: 12px;
  accent-color: var(--color-primary);
  cursor: pointer;
}

.refresh-btn {
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  padding: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.refresh-btn:hover {
  color: var(--color-fg);
  background: var(--color-hover);
}
.refresh-btn:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 1px;
}

.divider {
  height: 1px;
  background: var(--color-border);
  margin: 0 var(--gap);
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
  background: var(--color-border);
  border-radius: 3px;
}
</style>
