<script setup lang="ts">
// 多视图单入口：
//   prefs 窗口 → Preferences
//   overlay 窗口 → 按 mode 分发 PanelView / ResidentView；
//                  切换时进 transitioning（动画期间显示 TransitionOverlay loading，避免
//                  内容在变化窗口里错位），animate_done / 兜底 400ms 后切目标视图。
import { ref, onMounted, computed } from 'vue';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import PanelView from './components/PanelView.vue';
import ResidentView from './components/ResidentView.vue';
import TransitionOverlay from './components/TransitionOverlay.vue';
import Preferences from './components/Preferences.vue';
import type { OverlayMode } from './types';

const isPrefs = computed(() => getCurrentWebviewWindow().label === 'prefs');
const mode = ref<OverlayMode>('resident');
// 模式切换过渡态：true 时显示 loading，隐藏真实视图（避免动画期间内容在变窗口里错位）。
const transitioning = ref(false);

onMounted(async () => {
  if (isPrefs.value) return;
  try {
    const p = await invoke<{ mode: OverlayMode }>('get_prefs');
    mode.value = p.mode;
  } catch (e) {
    console.error('get_prefs mode failed', e);
  }
  try {
    // set_mode → 后端 emit mode_changed（动画开始）：切 mode + 进过渡态；兜底 400ms 退出。
    await listen<OverlayMode>('mode_changed', e => {
      mode.value = e.payload;
      transitioning.value = true;
      // spinner 先就位 100ms（窗口未动），再触发后端窗口动画。
      setTimeout(() => { invoke('do_animate'); }, 50);
      setTimeout(() => { transitioning.value = false; }, 1200); // 兜底防卡死
    });
  } catch (e) {
    console.error('listen mode_changed failed', e);
  }
  try {
    // 后端动画结束 emit animate_done：精确退出过渡态（动画时长系统定，比兜底准）。
    await listen('animate_done', () => { transitioning.value = false; });
  } catch (e) {
    console.error('listen animate_done failed', e);
  }
});
</script>

<template>
  <Preferences v-if="isPrefs" />
  <TransitionOverlay v-else-if="transitioning" />
  <PanelView v-else-if="mode === 'panel'" />
  <ResidentView v-else />
</template>

<style>
/* 设计 token（dark 默认 + light 覆盖）——全局变量供 Overlay 使用 */
:root {
  --color-bg: transparent;
  /* overlay 专用 tint：叠在 vibrancy 之上提亮 + 托住文字渲染（transparent 会让半透明背景上字发虚）。
     prefs 窗口本身不透明，仍用 transparent。0.45 保留透过感、不至于变成实色块。 */
  --color-bg-overlay: rgba(28, 28, 30, 0.45);
  --resident-bg: rgba(28, 28, 30, 0.55);
  --color-fg: #E5E5E7;
  --color-muted: #AEAEB2;
  --color-tertiary: #8E8E93;
  --color-primary: #0A84FF;
  --color-accent: #0A84FF;
  --color-border: rgba(255, 255, 255, 0.08);
  --color-hover: rgba(255, 255, 255, 0.08);
  --status-working: #30D158;
  --status-waiting: #0A84FF;
  --status-reply: #FFD60A;
  --status-permission: #FF9F0A;
  --status-shell: #BF5AF2;
  --status-compacting: #64D2FF;
  --font-body: -apple-system, "PingFang SC", "SF Pro Text", sans-serif;
  --font-utility: "SF Mono", ui-monospace, "Menlo", monospace;
  --fs-display: 13px; --fw-display: 700; --lh-display: 1.3;
  --fs-body: 13px;    --fw-body: 600;    --lh-body: 1.25;
  --fs-caption: 11px; --fw-caption: 400; --lh-caption: 1.3;
  --fs-utility: 10px; --fw-utility: 400; --lh-utility: 1.3;
  --radius-hud: 10px; --radius-overlay: 12px;
  --row-hud: 36px; --row-overlay: 36px;
  --pad-x: 12px; --pad-y: 8px; --gap: 8px;
  --gap-sm: 6px; --gap-xs: 4px;
  --fs-control: 15px;
  --space-empty: 40px;
  --motion-duration: 160ms;
  --motion-easing: cubic-bezier(0.22, 1, 0.36, 1);
}
@media (prefers-color-scheme: light) {
  :root {
    --color-fg: #1D1D1F;
    --color-bg-overlay: rgba(255, 255, 255, 0.55);
    --resident-bg: rgba(255, 255, 255, 0.55);
    --color-border: rgba(0, 0, 0, 0.08);
    --color-hover: rgba(0, 0, 0, 0.06);
    --color-muted: #6E6E73;
    --color-tertiary: #6E6E73;
  }
}
@media (prefers-reduced-motion: reduce) {
  .status-icon--spinning { animation: none !important; }
}
* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--color-bg);
  font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  user-select: none;
  -webkit-user-select: none;
}
</style>
