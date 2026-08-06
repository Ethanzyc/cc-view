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
import type { OverlayMode, Theme } from './types';
import { applyTheme } from './utils/theme';

const isPrefs = computed(() => getCurrentWebviewWindow().label === 'prefs');
const mode = ref<OverlayMode>('resident');
// 模式切换过渡态：true 时显示 loading，隐藏真实视图（避免动画期间内容在变窗口里错位）。
const transitioning = ref(false);

onMounted(async () => {
  // theme 两窗口都要应用（prefs 窗口也渲染偏好 UI），先于 isPrefs 分支。
  try {
    const p = await invoke<{ mode: OverlayMode; theme: Theme }>('get_prefs');
    applyTheme(p.theme);
    if (isPrefs.value) return;
    mode.value = p.mode;
  } catch (e) {
    console.error('get_prefs failed', e);
    if (isPrefs.value) return;
  }
  try {
    // prefs 变更（如 set_theme）→ 重读 theme 应用（overlay 跟随 prefs 窗口的切换）。
    await listen<{ theme?: Theme }>('prefs_changed', async () => {
      try {
        const p = await invoke<{ theme: Theme }>('get_prefs');
        applyTheme(p.theme);
      } catch (e) {
        console.error('prefs_changed theme reload failed', e);
      }
    });
  } catch (e) {
    console.error('listen prefs_changed failed', e);
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
/* 设计 token（light 默认 + html.dark 覆盖）——applyTheme 给 <html> 加 .dark 切深色，不跟随系统 */
:root {
  --color-bg: transparent;
  /* overlay 专用 tint：叠在 vibrancy 之上提亮 + 托住文字渲染（transparent 会让半透明背景上字发虚）。
     prefs 窗口本身不透明，仍用 transparent。0.55 保留透过感、不至于变成实色块。 */
  --color-bg-overlay: rgba(255, 255, 255, 0.55);
  --resident-bg: rgba(255, 255, 255, 0.55);
  --color-fg: #1D1D1F;
  --color-muted: #6E6E73;
  --color-tertiary: #6E6E73;
  --color-primary: #0A84FF;
  --color-accent: #0A84FF;
  --color-border: rgba(0, 0, 0, 0.08);
  --color-hover: rgba(0, 0, 0, 0.06);
  --status-working: #30D158;
  --status-working-ink: #1A7F37; /* 工作中文字/图标色：浅色降饱和提对比 */
  --status-waiting: #0A84FF;
  --status-reply: #FFD60A;
  --status-reply-ink: #FFB300; /* 等回答文字色：浅色降饱和提对比（图标/背景仍用 --status-reply） */
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
/* 深色覆盖：applyTheme 给 <html> 加 .dark 时生效（不再跟随系统）。 */
html.dark {
  --color-bg-overlay: rgba(28, 28, 30, 0.45);
  --resident-bg: rgba(28, 28, 30, 0.55);
  --color-fg: #E5E5E7;
  --color-muted: #AEAEB2;
  --color-tertiary: #8E8E93;
  --color-border: rgba(255, 255, 255, 0.08);
  --color-hover: rgba(255, 255, 255, 0.08);
  --status-working-ink: #3FB950;
  --status-reply-ink: #FFD60A;
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
