<script setup lang="ts">
// 偏好设置：开机自启动 / 通知 / 全局快捷键 / 轮询间隔 / 常驻面板（形态/布局/显隐/透明度）。
// 形态 + 布局用卡片缩略图选择（能看到 UI 样式）；透明度 slider pointer capture（拖出条外仍跟踪）
// + 下方实时预览块。常驻各项 emit prefs_changed → ResidentView 实时响应。
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
// 常驻面板配置
const mode = ref<OverlayMode>('panel');
const residentLayout = ref<ResidentLayout>('b');
const showSnoozed = ref(true);
const showIdle = ref(true);
const opacity = ref(55);
const saving = ref<string | null>(null); // 正在保存的项 key（反馈）
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

// 悲观更新：invoke 成功后再改本地 ref，失败保留旧值 + 显示 error。
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

// 常驻面板：set_mode/set_resident_* 后端均 emit prefs_changed（+ set_mode 额外 mode_changed）。
const onMode = (v: OverlayMode) => wrap('mode', async () => { await invoke('set_mode', { mode: v }); mode.value = v; });
const onLayout = (v: ResidentLayout) => wrap('layout', async () => { await invoke('set_resident_layout', { layout: v }); residentLayout.value = v; });
const onShowSnoozed = (v: boolean) => wrap('showSnoozed', async () => { await invoke('set_resident_show_snoozed', { show: v }); showSnoozed.value = v; });
const onShowIdle = (v: boolean) => wrap('showIdle', async () => { await invoke('set_resident_show_idle', { show: v }); showIdle.value = v; });

// 透明度拖动高频：本地乐观更新（slider/预览即时）+ debounce 150ms 持久化，
// 避免每次 @input 都触发 set_resident_opacity → prefs_changed → ResidentView get_prefs 风暴。
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
// slider pointer capture：按住拖动时即使鼠标超出滑条仍跟踪（WKWebView 默认不 capture，会"失效"）。
function onSliderDown(e: PointerEvent) {
  (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
}

// 透明度预览背景（实时跟随 opacity ref，按系统深浅色选 rgb）。
const previewBg = computed(() => {
  const light = window.matchMedia('(prefers-color-scheme: light)').matches;
  const rgb = light ? '255, 255, 255' : '28, 28, 30';
  return `rgba(${rgb}, ${(opacity.value / 100).toFixed(3)})`;
});

// 检查更新：check() 返回 Update（有更新）或 null（已是最新）
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

// 下载并安装 + 重启
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
        <input type="checkbox" :checked="autostart"
               :disabled="saving === 'autostart'"
               @change="onAutostart(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>通知</span>
        <input type="checkbox" :checked="notify"
               :disabled="saving === 'notify'"
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
        <input type="number" min="1" max="30" :value="interval"
               :disabled="saving === 'interval'"
               @change="onInterval(Number(($event.target as HTMLInputElement).value))" />
      </label>
    </section>
    <section>
      <h2 class="section-title">常驻面板</h2>
      <div class="field">
        <span class="field-label">默认形态</span>
        <div class="choice-cards">
          <button type="button" class="choice-card" :class="{ active: mode === 'panel' }" :disabled="saving === 'mode'" @click="onMode('panel')">
            <div class="card-preview preview-panel"></div>
            <span class="card-label">面板</span>
          </button>
          <button type="button" class="choice-card" :class="{ active: mode === 'resident' }" :disabled="saving === 'mode'" @click="onMode('resident')">
            <div class="card-preview preview-resident"></div>
            <span class="card-label">常驻</span>
          </button>
        </div>
      </div>
      <div class="field">
        <span class="field-label">常驻布局</span>
        <div class="choice-cards">
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'b' }" :disabled="saving === 'layout'" @click="onLayout('b')">
            <div class="card-preview preview-layout-b"></div>
            <span class="card-label">B 精简</span>
          </button>
          <button type="button" class="choice-card" :class="{ active: residentLayout === 'a' }" :disabled="saving === 'layout'" @click="onLayout('a')">
            <div class="card-preview preview-layout-a"></div>
            <span class="card-label">A 极简</span>
          </button>
        </div>
      </div>
      <label class="row">
        <span>显示搁置的会话</span>
        <input type="checkbox" :checked="showSnoozed" :disabled="saving === 'showSnoozed'"
               @change="onShowSnoozed(($event.target as HTMLInputElement).checked)" />
      </label>
      <label class="row">
        <span>显示闲置的会话</span>
        <input type="checkbox" :checked="showIdle" :disabled="saving === 'showIdle'"
               @change="onShowIdle(($event.target as HTMLInputElement).checked)" />
      </label>
      <div class="field">
        <span class="field-label">背景透明度（20–100，当前 {{ opacity }}%）</span>
        <input class="opacity-slider" type="range" min="20" max="100" :value="opacity" :disabled="saving === 'opacity'"
               @pointerdown="onSliderDown" @input="onOpacity(Number(($event.target as HTMLInputElement).value))" />
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
.section-title { font-size: 13px; font-weight: 700; margin: 20px 0 4px; color: var(--color-muted); letter-spacing: 0.03em; }
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

/* 卡片式选择（形态/布局）：缩略图 + 标签，点击选中高亮 */
.field { padding: 12px 0; border-bottom: 1px solid var(--color-border); }
.field-label { display: block; font-size: var(--fs-body); margin-bottom: 10px; }
.choice-cards { display: flex; gap: 10px; }
.choice-card {
  flex: 1; background: var(--color-hover);
  border: 1.5px solid var(--color-border); border-radius: 8px;
  padding: 10px; cursor: pointer;
  display: flex; flex-direction: column; align-items: stretch; gap: 6px;
  font-family: inherit; color: var(--color-fg);
  transition: border-color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.choice-card.active {
  border-color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 12%, var(--color-hover));
}
.choice-card:not(:disabled):hover { border-color: var(--color-primary); }
.choice-card:disabled { opacity: 0.5; cursor: default; }
.card-preview { width: 100%; height: 52px; border-radius: 4px; background: rgba(255,255,255,0.06); position: relative; overflow: hidden; }
.card-label { font-size: var(--fs-caption); color: var(--color-muted); text-align: center; }
.choice-card.active .card-label { color: var(--color-fg); }

/* 面板缩略图：宽矩形 + 顶部搜索条 + 3 行 */
.preview-panel::before { content: ""; position: absolute; top: 6px; left: 6px; right: 6px; height: 6px; background: rgba(255,255,255,0.28); border-radius: 2px; }
.preview-panel::after { content: ""; position: absolute; top: 18px; left: 6px; right: 6px; height: 1.5px; background: rgba(255,255,255,0.16); box-shadow: 0 6px 0 rgba(255,255,255,0.16), 0 12px 0 rgba(255,255,255,0.16); }
/* 常驻缩略图：窄竖条 + 3 短行 */
.preview-resident { width: 46%; }
.preview-resident::before { content: ""; position: absolute; top: 6px; left: 5px; right: 5px; height: 1.5px; background: rgba(255,255,255,0.22); box-shadow: 0 7px 0 rgba(255,255,255,0.22), 0 14px 0 rgba(255,255,255,0.22); }
/* 布局 B：宽 + 分组条 + 行 */
.preview-layout-b::before { content: ""; position: absolute; top: 5px; left: 6px; width: 22px; height: 3px; background: rgba(255,255,255,0.30); border-radius: 1px; }
.preview-layout-b::after { content: ""; position: absolute; top: 14px; left: 6px; right: 6px; height: 1.5px; background: rgba(255,255,255,0.16); box-shadow: 0 7px 0 rgba(255,255,255,0.16), 0 14px 0 rgba(255,255,255,0.16), 0 24px 0 rgba(255,255,255,0.30); }
/* 布局 A：窄 + 扁平行（无分组） */
.preview-layout-a { width: 56%; }
.preview-layout-a::before { content: ""; position: absolute; top: 7px; left: 5px; right: 5px; height: 1.5px; background: rgba(255,255,255,0.20); box-shadow: 0 8px 0 rgba(255,255,255,0.20), 0 16px 0 rgba(255,255,255,0.20), 0 24px 0 rgba(255,255,255,0.20); }

/* 透明度 slider + 预览 */
.opacity-slider { width: 100%; }
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
