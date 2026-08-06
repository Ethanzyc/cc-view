<script setup lang="ts">
// 偏好设置：开机自启动 / 通知 / 全局快捷键 / 轮询间隔 / 常驻面板（形态/布局/显隐/透明度）。
// 形态 + 布局用 mock 数据的迷你真实 UI 选择；透明度用自定义 slider（pointer capture，
// 拖出条外仍跟踪）+ 下方实时预览。常驻专属配置（布局/显隐/透明度）标注「仅常驻模式」。
import { ref, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import type { Prefs, OverlayMode, ResidentLayout } from '../types';

const notify = ref(true);
const shortcut = ref('alt+space');
const interval = ref(3);
const autostart = ref(false);
const mode = ref<OverlayMode>('panel');
const residentLayout = ref<ResidentLayout>('b');
const showSnoozed = ref(true);
const showIdle = ref(true);
const opacity = ref(55);
const saving = ref<string | null>(null);
const error = ref<string | null>(null);
const appVersion = ref('');
const checking = ref(false);
const updateAvailable = ref<Update | null>(null);
const upToDate = ref(false);
const installing = ref(false);
const installError = ref<string | null>(null);

const shortcuts = [
  { value: 'alt+space', label: '⌥Space（默认）' },
  { value: 'cmd+alt+space', label: '⌘⌥Space' },
  { value: 'ctrl+space', label: '⌃Space' },
  { value: 'off', label: '禁用' },
];

onMounted(async () => {
  try {
    const p = await invoke<Prefs>('get_prefs');
    notify.value = p.notify;
    shortcut.value = p.shortcut;
    interval.value = p.poll_interval;
    mode.value = p.mode;
    residentLayout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
    opacity.value = p.resident_opacity;
  } catch (e) {
    console.error('get_prefs failed', e);
  }
  try {
    autostart.value = await invoke<boolean>('get_autostart');
  } catch (e) {
    console.error('get_autostart failed', e);
  }
  try {
    appVersion.value = await getVersion();
  } catch (e) {
    console.error('getVersion failed', e);
  }
});

async function wrap(key: string, fn: () => Promise<unknown>) {
  error.value = null;
  saving.value = key;
  try {
    await fn();
  } catch (e: unknown) {
    error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '保存失败';
  } finally {
    saving.value = null;
  }
}

const onNotify = (v: boolean) => wrap('notify', async () => { await invoke('set_notify', { notify: v }); notify.value = v; });
const onAutostart = (v: boolean) => wrap('autostart', async () => { await invoke('toggle_autostart', { enable: v }); autostart.value = v; });
const onShortcut = (v: string) => wrap('shortcut', async () => { await invoke('set_shortcut', { shortcut: v }); shortcut.value = v; });
const onInterval = (v: number) => wrap('interval', async () => { await invoke('set_interval', { seconds: v }); interval.value = v; });
const onMode = (v: OverlayMode) => wrap('mode', async () => { await invoke('set_mode', { mode: v }); mode.value = v; });
const onLayout = (v: ResidentLayout) => wrap('layout', async () => { await invoke('set_resident_layout', { layout: v }); residentLayout.value = v; });
const onShowSnoozed = (v: boolean) => wrap('showSnoozed', async () => { await invoke('set_resident_show_snoozed', { show: v }); showSnoozed.value = v; });
const onShowIdle = (v: boolean) => wrap('showIdle', async () => { await invoke('set_resident_show_idle', { show: v }); showIdle.value = v; });

// 透明度本地乐观（slider/预览即时）+ debounce 150ms 持久化。
let opacityTimer: number | undefined;
const onOpacity = (v: number) => {
  opacity.value = v;
  clearTimeout(opacityTimer);
  opacityTimer = window.setTimeout(async () => {
    saving.value = 'opacity';
    try {
      await invoke('set_resident_opacity', { opacity: v });
    } catch (e: unknown) {
      error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '保存失败';
    } finally {
      saving.value = null;
    }
  }, 150);
};

// 自定义 slider：div + pointer 事件。range input 在 WKWebView 拖动不稳，自己掌控。
const sliderTrack = ref<HTMLElement>();
const sliderPct = computed(() => ((opacity.value - 20) / 80) * 100);
function setFromClientX(clientX: number) {
  const el = sliderTrack.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  onOpacity(Math.round(20 + ratio * 80));
}
function onSliderDown(e: PointerEvent) {
  e.preventDefault();
  const el = e.currentTarget as HTMLElement;
  el.setPointerCapture(e.pointerId);
  setFromClientX(e.clientX);
  const onMove = (ev: PointerEvent) => setFromClientX(ev.clientX);
  const onUp = (ev: PointerEvent) => {
    el.releasePointerCapture(ev.pointerId);
    el.removeEventListener('pointermove', onMove);
    el.removeEventListener('pointerup', onUp);
  };
  el.addEventListener('pointermove', onMove);
  el.addEventListener('pointerup', onUp);
}

const previewBg = computed(() => {
  const light = window.matchMedia('(prefers-color-scheme: light)').matches;
  const rgb = light ? '255, 255, 255' : '28, 28, 30';
  return `rgba(${rgb}, ${(opacity.value / 100).toFixed(3)})`;
});

async function checkForUpdates() {
  error.value = null;
  installError.value = null;
  checking.value = true;
  upToDate.value = false;
  updateAvailable.value = null;
  try {
    const upd = await check({ timeout: 8000 });
    if (upd) updateAvailable.value = upd;
    else upToDate.value = true;
  } catch (e: unknown) {
    const msg = typeof e === 'string' ? e : (e as Error)?.message ?? '检查失败';
    error.value = /sending request|fetch|network|timeout|connect/i.test(msg)
      ? '⚠ 无法连接 GitHub（网络/代理问题）'
      : msg;
  } finally {
    checking.value = false;
  }
}

async function downloadAndInstall() {
  if (!updateAvailable.value) return;
  installing.value = true;
  installError.value = null;
  try {
    await updateAvailable.value.downloadAndInstall();
    await relaunch();
  } catch (e: unknown) {
    installError.value = typeof e === 'string' ? e : (e as Error)?.message ?? '安装失败';
    installing.value = false;
  }
}
</script>

<template>
  <div class="prefs">
    <h1>cc-view 偏好设置</h1>
    <section>
      <label class="row">
        <span>开机自启动</span>
        <input type="checkbox" :checked="autostart" :disabled="saving === 'autostart'"
               @change="onAutostart(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>通知</span>
        <input type="checkbox" :checked="notify" :disabled="saving === 'notify'"
               @change="onNotify(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>全局快捷键</span>
        <select :value="shortcut" :disabled="saving === 'shortcut'"
                @change="onShortcut(($event.target as HTMLSelectElement).value)">
          <option v-for="s in shortcuts" :key="s.value" :value="s.value">{{ s.label }}</option>
        </select>
      </label>
      <label class="row">
        <span>轮询间隔（秒，1–30）</span>
        <input type="number" min="1" max="30" :value="interval" :disabled="saving === 'interval'"
               @change="onInterval(Number(($event.target as HTMLInputElement).value))" />
      </label>
    </section>

    <section>
      <h2 class="section-title">默认形态</h2>
      <p class="hint">⌥Space 呼出时显示哪种形态。两者可随时互切。</p>
      <div class="choice-cards">
        <button type="button" class="choice-card" :class="{ active: mode === 'panel' }" :disabled="saving === 'mode'" @click="onMode('panel')">
          <!-- 面板 mock：搜索条 + 3 行（状态点 + 文字线） -->
          <div class="mock mock-panel">
            <div class="mock-search"></div>
            <div class="mock-row"><span class="dot working"></span><span class="mock-text"></span></div>
            <div class="mock-row"><span class="dot perm"></span><span class="mock-text"></span></div>
            <div class="mock-row"><span class="dot input"></span><span class="mock-text"></span></div>
          </div>
          <span class="card-label">面板（全功能）</span>
        </button>
        <button type="button" class="choice-card" :class="{ active: mode === 'resident' }" :disabled="saving === 'mode'" @click="onMode('resident')">
          <!-- 常驻 mock：窄 + 分组条 + 2 行 -->
          <div class="mock mock-resident">
            <div class="mock-group-bar"></div>
            <div class="mock-row"><span class="dot perm"></span><span class="mock-text"></span></div>
            <div class="mock-row"><span class="dot working"></span><span class="mock-text"></span></div>
          </div>
          <span class="card-label">常驻（精简）</span>
        </button>
      </div>
    </section>

    <section class="resident-only">
      <h2 class="section-title">常驻面板 <span class="tag">仅常驻模式</span></h2>

      <div class="field">
        <span class="field-label">常驻布局</span>
        <div class="choice-cards">
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'b' }" :disabled="saving === 'layout'" @click="onLayout('b')">
            <!-- B 精简 mock：分组 + 项目标题 + 行（带状态文字短线） -->
            <div class="mock mock-layout">
              <div class="mock-group-bar"></div>
              <div class="mock-proj-bar"></div>
              <div class="mock-row"><span class="dot perm"></span><span class="mock-text"></span><span class="mock-status"></span></div>
              <div class="mock-row"><span class="dot input"></span><span class="mock-text"></span><span class="mock-status"></span></div>
            </div>
            <span class="card-label">B 精简</span>
          </button>
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'a' }" :disabled="saving === 'layout'" @click="onLayout('a')">
            <!-- A 极简 mock：分组 + 项目标题 + 行（无状态文字） -->
            <div class="mock mock-layout">
              <div class="mock-group-bar"></div>
              <div class="mock-proj-bar"></div>
              <div class="mock-row"><span class="dot perm"></span><span class="mock-text"></span></div>
              <div class="mock-row"><span class="dot working"></span><span class="mock-text"></span></div>
            </div>
            <span class="card-label">A 极简</span>
          </button>
        </div>
      </div>

      <div class="field">
        <span class="field-label">显示哪些会话</span>
        <label class="check-line">
          <input type="checkbox" :checked="showSnoozed" :disabled="saving === 'showSnoozed'"
                 @change="onShowSnoozed(($event.target as HTMLInputElement).checked)" />
          <span>显示搁置的会话</span>
        </label>
        <label class="check-line">
          <input type="checkbox" :checked="showIdle" :disabled="saving === 'showIdle'"
                 @change="onShowIdle(($event.target as HTMLInputElement).checked)" />
          <span>显示闲置的会话（等输入超时）</span>
        </label>
      </div>

      <div class="field">
        <span class="field-label">背景透明度 <span class="value">{{ opacity }}%</span></span>
        <div class="slider" @pointerdown="onSliderDown">
          <div class="slider-track" ref="sliderTrack">
            <div class="slider-fill" :style="{ width: sliderPct + '%' }"></div>
            <div class="slider-knob" :style="{ left: sliderPct + '%' }"></div>
          </div>
        </div>
        <div class="opacity-preview">
          <div class="preview-desktop">
            <div class="preview-window" :style="{ background: previewBg }">cc-view</div>
          </div>
        </div>
      </div>
    </section>

    <section class="update-section">
      <div class="row">
        <span>版本 cc-view {{ appVersion }}</span>
        <button @click="checkForUpdates" :disabled="checking">
          {{ checking ? '检查中…' : '检查更新' }}
        </button>
      </div>
      <p v-if="upToDate" class="muted">已是最新版本</p>
      <div v-if="updateAvailable" class="update-detail">
        <p>发现新版本 {{ updateAvailable.version }}</p>
        <pre v-if="updateAvailable.body">{{ updateAvailable.body }}</pre>
        <button @click="downloadAndInstall" :disabled="installing">
          {{ installing ? '安装中…' : '下载并安装' }}
        </button>
      </div>
      <p v-if="installError" class="error">⚠ {{ installError }}</p>
    </section>
    <p v-if="error" class="error">⚠ {{ error }}</p>
    <p class="repo"><a href="https://github.com/Ethanzyc/cc-view" target="_blank" rel="noopener">GitHub →</a></p>
  </div>
</template>

<style scoped>
.prefs { padding: 24px 28px; color: var(--color-fg); font-family: var(--font-body); }
h1 { font-size: 18px; font-weight: 700; margin: 0 0 20px; }
section { margin-top: 20px; }
.section-title { font-size: 13px; font-weight: 700; margin: 0 0 4px; color: var(--color-muted); letter-spacing: 0.03em; display: flex; align-items: center; gap: 8px; }
.hint { font-size: 11px; color: var(--color-tertiary); margin: 0 0 10px; }
.tag { font-size: 10px; font-weight: 600; color: var(--color-primary); border: 1px solid color-mix(in srgb, var(--color-primary) 40%, transparent); border-radius: 8px; padding: 1px 7px; letter-spacing: 0; }

.row {
  display: flex; justify-content: space-between; align-items: center;
  padding: 12px 0; border-bottom: 1px solid var(--color-border);
  font-size: var(--fs-body);
}
.row input[type="checkbox"] { width: 18px; height: 18px; }
.row select, .row input[type="number"] {
  font-size: var(--fs-control); padding: 4px 8px;
  background: var(--color-bg); color: var(--color-fg);
  border: 1px solid var(--color-border); border-radius: 6px;
}

/* 卡片式选择 */
.choice-cards { display: flex; gap: 10px; }
.choice-card {
  flex: 1; background: var(--color-hover);
  border: 1.5px solid var(--color-border); border-radius: 8px;
  padding: 10px; cursor: pointer;
  display: flex; flex-direction: column; align-items: stretch; gap: 8px;
  font-family: inherit; color: var(--color-fg);
  transition: border-color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.choice-card.active {
  border-color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 10%, var(--color-hover));
}
.choice-card:not(:disabled):hover { border-color: var(--color-primary); }
.choice-card:disabled { opacity: 0.5; cursor: default; }
.card-label { font-size: var(--fs-caption); color: var(--color-muted); text-align: center; }
.choice-card.active .card-label { color: var(--color-fg); }

/* mock 迷你 UI（照真实组件简化） */
.mock {
  height: 64px; border-radius: 5px; padding: 6px;
  background: rgba(28, 28, 30, 0.7);
  border: 1px solid rgba(255, 255, 255, 0.10);
  display: flex; flex-direction: column; gap: 4px;
  box-sizing: border-box; overflow: hidden;
}
.mock-panel { width: 100%; }
.mock-resident { width: 52%; align-self: center; }
.mock-layout { width: 100%; }
.mock-search { height: 6px; background: rgba(255, 255, 255, 0.18); border-radius: 2px; margin-bottom: 2px; }
.mock-group-bar { height: 3px; width: 34px; background: rgba(255, 255, 255, 0.32); border-radius: 1px; }
.mock-proj-bar { height: 2px; width: 46%; background: rgba(255, 255, 255, 0.24); border-radius: 1px; }
.mock-row { display: flex; align-items: center; gap: 5px; }
.dot { width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0; }
.dot.working { background: #30D158; }
.dot.perm { background: #FF9F0A; }
.dot.input { background: #0A84FF; }
.mock-text { flex: 1; height: 3px; background: rgba(255, 255, 255, 0.22); border-radius: 1px; }
.mock-status { width: 26px; height: 3px; background: rgba(255, 255, 255, 0.14); border-radius: 1px; flex-shrink: 0; }

/* 常驻专属字段 */
.field { padding: 14px 0; border-bottom: 1px solid var(--color-border); }
.field:last-child { border-bottom: none; }
.field-label { display: flex; justify-content: space-between; align-items: baseline; font-size: var(--fs-body); margin-bottom: 10px; }
.field-label .value { font-size: var(--fs-caption); color: var(--color-tertiary); font-variant-numeric: tabular-nums; }
.check-line { display: flex; align-items: center; gap: 8px; padding: 6px 0; font-size: var(--fs-body); cursor: pointer; }
.check-line input { width: 16px; height: 16px; }

/* 自定义 slider */
.slider { padding: 10px 0; cursor: pointer; touch-action: none; }
.slider-track { position: relative; height: 5px; background: var(--color-border); border-radius: 3px; }
.slider-fill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--color-primary); border-radius: 3px; }
.slider-knob {
  position: absolute; top: 50%; width: 16px; height: 16px; border-radius: 50%;
  background: #fff; border: 1px solid var(--color-primary);
  transform: translate(-50%, -50%);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
  transition: transform var(--motion-duration) var(--motion-easing);
}
.slider:active .slider-knob { transform: translate(-50%, -50%) scale(1.12); }

/* 透明度预览 */
.opacity-preview { margin-top: 10px; }
.preview-desktop { height: 52px; border-radius: 6px; overflow: hidden;
  background: radial-gradient(120% 90% at 15% 10%, #5b3fa8 0%, transparent 55%),
              radial-gradient(120% 90% at 90% 20%, #c2445e 0%, transparent 50%),
              linear-gradient(135deg, #1b1b2e, #14213d);
  display: flex; align-items: center; justify-content: center; }
.preview-window { padding: 4px 12px; border-radius: 4px; color: #fff; font-size: 11px; border: 1px solid rgba(255,255,255,0.15); }

.error { color: var(--status-permission); margin-top: 16px; }
.update-section { margin-top: 24px; }
.update-section .row { border-bottom: none; }
.update-section button {
  font-size: var(--fs-control); padding: 5px 14px;
  background: var(--color-hover); color: var(--color-fg);
  border: 1px solid var(--color-border); border-radius: 6px;
  cursor: pointer;
  transition: background var(--motion-duration) var(--motion-easing), color var(--motion-duration) var(--motion-easing), border-color var(--motion-duration) var(--motion-easing);
}
.update-section button:not(:disabled):hover { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
.update-section button:disabled { opacity: 0.5; cursor: default; }
.update-detail { margin-top: 12px; padding: 12px; background: var(--color-hover); border-radius: 8px; }
.update-detail pre { white-space: pre-wrap; margin: 8px 0; font-size: 12px; }
.muted { color: var(--color-muted); margin-top: 8px; font-size: var(--fs-body); }
.repo { margin-top: 20px; padding-top: 16px; border-top: 1px solid var(--color-border); text-align: center; }
.repo a { color: var(--color-primary); text-decoration: none; font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body); }
.repo a:hover { text-decoration: underline; }
</style>
