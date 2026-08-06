<script setup lang="ts">
// 偏好设置：开机自启动 / 通知 / 全局快捷键 / 轮询间隔 / 默认形态 / 常驻面板（框起来）。
// 形态 + 常驻布局用真实风格 mini mock（半透毛玻璃 + 分组 + 项目 + 状态图标 + 会话名 + 状态）。
// 透明度自定义 slider（0–100）+ 实时预览。常驻专属配置用边框框起来。
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

// 自定义 slider（div + pointer），范围 0–100。
const sliderTrack = ref<HTMLElement>();
const sliderPct = computed(() => opacity.value);
function setFromClientX(clientX: number) {
  const el = sliderTrack.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  onOpacity(Math.round(ratio * 100));
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
          <div class="mock mock-panel">
            <div class="m-search"><span class="m-search-ico">⌕</span><span>搜索会话…</span></div>
            <div class="m-group">待介入 <span class="m-cnt">3</span></div>
            <div class="m-proj">~/ai/cc-view</div>
            <div class="m-row m-perm">
              <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#FF9F0A" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.75" y="7" width="10.5" height="6.5" rx="1.25"/><path d="M4.75 7 V4.75 A3.25 3.25 0 0 1 11.25 4.75 V7"/></svg>
              <span class="m-name">refactor</span><span class="m-st">等权限</span>
            </div>
            <div class="m-row">
              <svg width="8" height="8" viewBox="0 0 16 16"><circle cx="4" cy="8" r="1.4" fill="#0A84FF"/><circle cx="8" cy="8" r="1.4" fill="#0A84FF"/><circle cx="12" cy="8" r="1.4" fill="#0A84FF"/></svg>
              <span class="m-name">session-2</span><span class="m-st">等输入</span>
            </div>
          </div>
          <span class="card-label">面板（全功能）</span>
        </button>
        <button type="button" class="choice-card" :class="{ active: mode === 'resident' }" :disabled="saving === 'mode'" @click="onMode('resident')">
          <div class="mock mock-resident">
            <div class="m-group">待介入 <span class="m-cnt">2</span></div>
            <div class="m-proj">~/ai/cc-view</div>
            <div class="m-row m-perm">
              <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#FF9F0A" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.75" y="7" width="10.5" height="6.5" rx="1.25"/><path d="M4.75 7 V4.75 A3.25 3.25 0 0 1 11.25 4.75 V7"/></svg>
              <span class="m-name">refactor</span>
            </div>
            <div class="m-row">
              <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#30D158" stroke-width="1.8" stroke-linecap="round"><circle cx="8" cy="8" r="5.5" stroke-dasharray="9 100" transform="rotate(-90 8 8)"/></svg>
              <span class="m-name">build</span>
            </div>
          </div>
          <span class="card-label">常驻（精简）</span>
        </button>
      </div>
    </section>

    <section class="resident-box">
      <h2 class="section-title">常驻面板 <span class="tag">仅常驻模式</span></h2>

      <div class="field">
        <span class="field-label">常驻布局</span>
        <div class="choice-cards">
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'b' }" :disabled="saving === 'layout'" @click="onLayout('b')">
            <div class="mock mock-layout">
              <div class="m-group">待介入</div>
              <div class="m-proj">~/ai/x</div>
              <div class="m-row m-perm">
                <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#FF9F0A" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.75" y="7" width="10.5" height="6.5" rx="1.25"/><path d="M4.75 7 V4.75 A3.25 3.25 0 0 1 11.25 4.75 V7"/></svg>
                <span class="m-name">fix-bug</span><span class="m-st">等权限</span>
              </div>
              <div class="m-row">
                <svg width="8" height="8" viewBox="0 0 16 16"><circle cx="4" cy="8" r="1.4" fill="#0A84FF"/><circle cx="8" cy="8" r="1.4" fill="#0A84FF"/><circle cx="12" cy="8" r="1.4" fill="#0A84FF"/></svg>
                <span class="m-name">test</span><span class="m-st">等输入</span>
              </div>
            </div>
            <span class="card-label">B 精简</span>
          </button>
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'a' }" :disabled="saving === 'layout'" @click="onLayout('a')">
            <div class="mock mock-layout">
              <div class="m-group">待介入</div>
              <div class="m-proj">~/ai/x</div>
              <div class="m-row m-perm">
                <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#FF9F0A" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.75" y="7" width="10.5" height="6.5" rx="1.25"/><path d="M4.75 7 V4.75 A3.25 3.25 0 0 1 11.25 4.75 V7"/></svg>
                <span class="m-name">fix-bug</span>
              </div>
              <div class="m-row">
                <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#30D158" stroke-width="1.8" stroke-linecap="round"><circle cx="8" cy="8" r="5.5" stroke-dasharray="9 100" transform="rotate(-90 8 8)"/></svg>
                <span class="m-name">build</span>
              </div>
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

/* 常驻专属配置框起来 */
.resident-box {
  border: 1px solid var(--color-border); border-radius: 10px;
  padding: 4px 14px 4px; margin-top: 24px;
}
.resident-box .section-title { margin-top: 12px; }

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

/* mock 真实风格 mini（半透毛玻璃 + 分组 + 项目 + 状态图标 + 会话名 + 状态） */
.mock {
  border-radius: 6px; padding: 6px;
  background: rgba(28, 28, 30, 0.72);
  backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  color: #E5E5E7;
  display: flex; flex-direction: column; gap: 2px;
  box-sizing: border-box;
}
.mock-panel { width: 100%; height: 92px; }
.mock-resident { width: 60%; align-self: center; height: 74px; }
.mock-layout { width: 100%; height: 74px; }

.m-search { display: flex; align-items: center; gap: 4px; padding: 2px 5px; background: rgba(255,255,255,0.07); border-radius: 3px; font-size: 7px; color: #8E8E93; margin-bottom: 2px; }
.m-search-ico { font-size: 8px; }
.m-group { font-size: 6px; text-transform: uppercase; letter-spacing: 0.08em; color: #AEAEB2; padding: 3px 1px 1px; display: flex; gap: 3px; align-items: center; }
.m-cnt { background: rgba(255,255,255,0.12); border-radius: 5px; padding: 0 4px; font-size: 5px; line-height: 8px; color: #8E8E93; }
.m-proj { font-size: 7px; color: #AEAEB2; font-family: ui-monospace, "SF Mono", monospace; padding: 1px 1px; }
.m-row { display: flex; align-items: center; gap: 5px; padding: 2px 4px; border-left: 1.5px solid transparent; }
.m-row.m-perm { border-left-color: #FF9F0A; background: rgba(255,159,10,0.12); border-radius: 0 3px 3px 0; }
.m-row svg { flex-shrink: 0; }
.m-name { flex: 1; font-size: 8px; color: #E5E5E7; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.m-st { font-size: 6px; color: #AEAEB2; flex-shrink: 0; }
.m-row.m-perm .m-st { color: #FF9F0A; }

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
