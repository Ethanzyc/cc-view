<script setup lang="ts">
// 偏好设置（VSCode Settings 风格：左 nav 分类 + 右设置项行）。
// 分类：通用 / 显示 / 更新。⌘, 全局快捷键打开（lib.rs 注册）。
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { Channel } from '@tauri-apps/api/core';
import { shallowRef } from 'vue';
import { useI18n } from 'vue-i18n';
import { relaunch } from '@tauri-apps/plugin-process';
import type { Prefs, ResidentLayout, Theme, TokenUnit, UpdateSource, Locale } from '../types';
import { applyTheme } from '../utils/theme';
import iconUrl from '../assets/cc-view-icon.png';

const { t } = useI18n();

type Category = 'general' | 'display' | 'update';
const activeCategory = ref<Category>('general');

const notify = ref(true);
const shortcut = ref('alt+space');
const interval = ref(3);
const autostart = ref(false);
const theme = ref<Theme>('light');
const residentLayout = ref<ResidentLayout>('b');
const showSnoozed = ref(true);
const showIdle = ref(true);
const opacity = ref(55);
const tokenUnitPref = ref<TokenUnit>('km');
const residentWidth = ref<number>(285);
const showHost = ref(false);
const showTokens = ref(true);
const showActions = ref(true);
const updateSourcePref = ref<UpdateSource>('auto');
const localePref = ref<Locale>('auto');
const saving = ref<string | null>(null);
const error = ref<string | null>(null);
const appVersion = ref('');
const checking = ref(false);
// 自定义更新检查：invoke('check_update_custom') → { rid, version, body }
// downloadAndInstall 走插件原生 plugin:updater|download_and_install（通过 rid）
interface CustomUpdate { version: string; body: string | null; rid: number }
const updateAvailable = shallowRef<CustomUpdate | null>(null);
const upToDate = ref(false);
const installing = ref(false);
const installPhase = ref<'idle' | 'downloading' | 'installing' | 'restarting'>('idle');
const downloadProgress = ref(0); // 0-100
const downloadTotal = ref(0);    // bytes
const downloadLoaded = ref(0);   // bytes
const installError = ref<string | null>(null);

function defaultWidthForLayout(layout: ResidentLayout): number {
  return layout === 'a' ? 180 : 285;
}

const shortcuts = computed(() => [
  { value: 'alt+space', label: t('prefs.shortcutDefault') },
  { value: 'cmd+alt+space', label: '⌘⌥Space' },
  { value: 'ctrl+space', label: '⌃Space' },
  { value: 'off', label: t('prefs.shortcutOff') },
]);

onMounted(async () => {
  try {
    const p = await invoke<Prefs>('get_prefs');
    notify.value = p.notify;
    shortcut.value = p.shortcut;
    interval.value = p.poll_interval;
    theme.value = p.theme;
    residentLayout.value = p.resident_layout;
    showSnoozed.value = p.resident_show_snoozed;
    showIdle.value = p.resident_show_idle;
    opacity.value = p.resident_opacity;
    tokenUnitPref.value = p.token_unit;
    residentWidth.value = p.resident_width ?? defaultWidthForLayout(p.resident_layout);
    showHost.value = p.show_host;
    showTokens.value = p.show_tokens;
    showActions.value = p.show_actions;
    updateSourcePref.value = p.update_source;
    localePref.value = p.locale;
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

// 菜单「检查更新…」→ 后端 emit "prefs_action" → 切更新 tab + 自动检查
let unlistenPrefsAction: (() => void) | undefined;
onMounted(async () => {
  unlistenPrefsAction = await listen<string>('prefs_action', (e) => {
    if (e.payload === 'check_update') {
      activeCategory.value = 'update';
      checkForUpdates();
    }
  });
});
onBeforeUnmount(() => { unlistenPrefsAction?.(); });

async function wrap(key: string, fn: () => Promise<unknown>) {
  error.value = null;
  saving.value = key;
  try {
    await fn();
  } catch (e: unknown) {
    error.value = typeof e === 'string' ? e : (e as Error)?.message ?? t('prefs.saveFailed');
  } finally {
    saving.value = null;
  }
}

const onNotify = (v: boolean) => wrap('notify', async () => { await invoke('set_notify', { notify: v }); notify.value = v; });
const onAutostart = (v: boolean) => wrap('autostart', async () => { await invoke('toggle_autostart', { enable: v }); autostart.value = v; });
const onShortcut = (v: string) => wrap('shortcut', async () => { await invoke('set_shortcut', { shortcut: v }); shortcut.value = v; });
const onInterval = (v: number) => wrap('interval', async () => { await invoke('set_interval', { seconds: v }); interval.value = v; });
const onTheme = (v: Theme) => wrap('theme', async () => {
  await invoke('set_theme', { theme: v });
  theme.value = v;
  applyTheme(v);
});
const onLayout = (v: ResidentLayout) => wrap('layout', async () => { await invoke('set_resident_layout', { layout: v }); residentLayout.value = v; });
const onShowSnoozed = (v: boolean) => wrap('showSnoozed', async () => { await invoke('set_resident_show_snoozed', { show: v }); showSnoozed.value = v; });
const onShowIdle = (v: boolean) => wrap('showIdle', async () => { await invoke('set_resident_show_idle', { show: v }); showIdle.value = v; });
const onTokenUnit = (v: TokenUnit) => wrap('tokenUnit', async () => { await invoke('set_token_unit', { unit: v }); tokenUnitPref.value = v; });
const onShowHost = (v: boolean) => wrap('showHost', async () => { await invoke('set_show_host', { show: v }); showHost.value = v; });
const onShowTokens = (v: boolean) => wrap('showTokens', async () => { await invoke('set_show_tokens', { show: v }); showTokens.value = v; });
const onShowActions = (v: boolean) => wrap('showActions', async () => { await invoke('set_show_actions', { show: v }); showActions.value = v; });
const onUpdateSource = (v: UpdateSource) => wrap('updateSource', async () => { await invoke('set_update_source', { source: v }); updateSourcePref.value = v; });
const onLocale = (v: Locale) => wrap('locale', async () => { await invoke('set_locale', { locale: v }); localePref.value = v; });

let opacityTimer: number | undefined;
const onOpacity = (v: number) => {
  opacity.value = v;
  clearTimeout(opacityTimer);
  opacityTimer = window.setTimeout(async () => {
    saving.value = 'opacity';
    try {
      await invoke('set_resident_opacity', { opacity: v });
    } catch (e: unknown) {
      error.value = typeof e === 'string' ? e : (e as Error)?.message ?? t('prefs.saveFailed');
    } finally {
      saving.value = null;
    }
  }, 150);
};

let widthTimer: number | undefined;
const onWidth = (v: number) => {
  residentWidth.value = v;
  clearTimeout(widthTimer);
  widthTimer = window.setTimeout(async () => {
    saving.value = 'width';
    try {
      await invoke('set_resident_width', { width: v });
    } catch (e: unknown) {
      error.value = typeof e === 'string' ? e : (e as Error)?.message ?? t('prefs.saveFailed');
    } finally {
      saving.value = null;
    }
  }, 150);
};

// 自定义 slider（div + pointer）。
const sliderTrack = ref<HTMLElement>();
const sliderTrackWidth = ref<HTMLElement>();
const sliderPct = computed(() => opacity.value);
const widthPct = computed(() => ((residentWidth.value - 140) / (480 - 140)) * 100);
function setFromClientX(clientX: number, el: HTMLElement | undefined, max: number, min: number, cb: (v: number) => void) {
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  cb(Math.round(min + ratio * (max - min)));
}
function bindSliderDown(e: PointerEvent, track: HTMLElement | undefined, max: number, min: number, cb: (v: number) => void) {
  e.preventDefault();
  const el = e.currentTarget as HTMLElement;
  el.setPointerCapture(e.pointerId);
  setFromClientX(e.clientX, track, max, min, cb);
  const onMove = (ev: PointerEvent) => setFromClientX(ev.clientX, track, max, min, cb);
  const onUp = (ev: PointerEvent) => {
    el.releasePointerCapture(ev.pointerId);
    el.removeEventListener('pointermove', onMove);
    el.removeEventListener('pointerup', onUp);
  };
  el.addEventListener('pointermove', onMove);
  el.addEventListener('pointerup', onUp);
}
function onSliderDown(e: PointerEvent) { bindSliderDown(e, sliderTrack.value, 100, 0, onOpacity); }
function onSliderDownWidth(e: PointerEvent) { bindSliderDown(e, sliderTrackWidth.value, 480, 140, onWidth); }

async function checkForUpdates() {
  error.value = null;
  installError.value = null;
  checking.value = true;
  upToDate.value = false;
  updateAvailable.value = null;
  try {
    const upd = await invoke<CustomUpdate | null>('check_update_custom');
    if (upd) updateAvailable.value = upd;
    else upToDate.value = true;
  } catch (e: unknown) {
    const msg = typeof e === 'string' ? e : (e as Error)?.message ?? t('prefs.checkFailed');
    error.value = /sending request|fetch|network|timeout|connect/i.test(msg)
      ? t('prefs.networkError')
      : msg;
  } finally {
    checking.value = false;
  }
}

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function downloadAndInstall() {
  if (!updateAvailable.value) return;
  installing.value = true;
  installError.value = null;
  installPhase.value = 'downloading';
  downloadProgress.value = 0;
  downloadTotal.value = 0;
  downloadLoaded.value = 0;
  try {
    const channel = new Channel();
    channel.onmessage = (event: unknown) => {
      const e = event as { event: string; data: { contentLength?: number; chunkLength?: number } };
      if (e.event === 'Started' && e.data.contentLength) {
        downloadTotal.value = e.data.contentLength;
      } else if (e.event === 'Progress' && e.data.chunkLength) {
        downloadLoaded.value += e.data.chunkLength;
        if (downloadTotal.value > 0) {
          downloadProgress.value = Math.round((downloadLoaded.value / downloadTotal.value) * 100);
        }
      } else if (e.event === 'Finished') {
        installPhase.value = 'installing';
        downloadProgress.value = 100;
      }
    };
    await invoke('plugin:updater|download_and_install', {
      rid: updateAvailable.value.rid,
      onEvent: channel,
    });
    installPhase.value = 'restarting';
    // 保存更新信息，重启后弹「更新成功」提示
    try {
      await invoke('set_pending_update', {
        version: updateAvailable.value.version,
        notes: updateAvailable.value.body || '',
      });
    } catch { /* non-critical */ }
    await relaunch();
  } catch (e: unknown) {
    installError.value = typeof e === 'string' ? e : (e as Error)?.message ?? t('prefs.installFailed');
    installing.value = false;
    installPhase.value = 'idle';
  }
}

const installLabel = computed(() => {
  switch (installPhase.value) {
    case 'downloading': return t('prefs.downloading', { percent: downloadProgress.value });
    case 'installing': return t('prefs.installing');
    case 'restarting': return t('prefs.restarting');
    default: return t('prefs.downloadInstall');
  }
});
</script>

<template>
  <div class="prefs">
    <div class="body">
      <nav class="cats">
        <div class="brand">
          <img :src="iconUrl" alt="CC View" class="brand-icon" />
          <span class="brand-name">CC View</span>
        </div>
        <div class="cat" :class="{ active: activeCategory === 'general' }" @click="activeCategory = 'general'">{{ t('prefs.general') }}</div>
        <div class="cat" :class="{ active: activeCategory === 'display' }" @click="activeCategory = 'display'">{{ t('prefs.display') }}</div>
        <div class="cat" :class="{ active: activeCategory === 'update' }" @click="activeCategory = 'update'">{{ t('prefs.update') }}</div>
      </nav>

      <main class="settings">
        <!-- 通用 -->
        <section v-show="activeCategory === 'general'">
          <h2 class="group">{{ t('prefs.general') }}</h2>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.autostart') }}</div><div class="d">{{ t('prefs.autostartDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: autostart }" :disabled="saving === 'autostart'" @click="onAutostart(!autostart)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.notify') }}</div><div class="d">{{ t('prefs.notifyDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: notify }" :disabled="saving === 'notify'" @click="onNotify(!notify)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.language') }}</div><div class="d">{{ t('prefs.languageDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: localePref === 'auto' }" :disabled="saving === 'locale'" @click="onLocale('auto')"><span class="opt-title">{{ t('prefs.langAuto') }}</span></button>
                <button type="button" class="opt-btn" :class="{ active: localePref === 'zh' }" :disabled="saving === 'locale'" @click="onLocale('zh')"><span class="opt-title">{{ t('prefs.langZh') }}</span></button>
                <button type="button" class="opt-btn" :class="{ active: localePref === 'en' }" :disabled="saving === 'locale'" @click="onLocale('en')"><span class="opt-title">{{ t('prefs.langEn') }}</span></button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.shortcut') }}</div><div class="d">{{ t('prefs.shortcutDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button v-for="s in shortcuts" :key="s.value" type="button" class="opt-btn" :class="{ active: shortcut === s.value }" :disabled="saving === 'shortcut'" @click="onShortcut(s.value)">
                  <span class="opt-title">{{ s.label }}</span>
                </button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.interval') }}</div><div class="d">{{ t('prefs.intervalDesc') }}</div></div>
            <div class="ctl"><input type="number" class="num" min="1" max="30" :value="interval" :disabled="saving === 'interval'" @change="onInterval(Number(($event.target as HTMLInputElement).value))" /> <span class="unit">{{ t('prefs.seconds') }}</span></div>
          </div>
        </section>

        <!-- 显示（合并外观 + 常驻面板） -->
        <section v-show="activeCategory === 'display'">
          <h2 class="group">{{ t('prefs.appearance') }}</h2>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.theme') }}</div><div class="d">{{ t('prefs.themeDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: theme === 'light' }" :disabled="saving === 'theme'" @click="onTheme('light')"><span class="opt-title">{{ t('prefs.light') }}</span></button>
                <button type="button" class="opt-btn" :class="{ active: theme === 'dark' }" :disabled="saving === 'theme'" @click="onTheme('dark')"><span class="opt-title">{{ t('prefs.dark') }}</span></button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.tokenUnit') }}</div><div class="d">{{ t('prefs.tokenUnitDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: tokenUnitPref === 'km' }" :disabled="saving === 'tokenUnit'" @click="onTokenUnit('km')"><span class="opt-title">k / M</span></button>
                <button type="button" class="opt-btn" :class="{ active: tokenUnitPref === 'wan' }" :disabled="saving === 'tokenUnit'" @click="onTokenUnit('wan')"><span class="opt-title">万 / 亿</span></button>
              </div>
            </div>
          </div>

          <h2 class="group">{{ t('prefs.listDisplay') }}</h2>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.showHost') }}</div><div class="d">{{ t('prefs.showHostDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showHost }" :disabled="saving === 'showHost'" @click="onShowHost(!showHost)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.showTokens') }}</div><div class="d">{{ t('prefs.showTokensDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showTokens }" :disabled="saving === 'showTokens'" @click="onShowTokens(!showTokens)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.showActions') }}</div><div class="d">{{ t('prefs.showActionsDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showActions }" :disabled="saving === 'showActions'" @click="onShowActions(!showActions)"><span class="switch-knob"></span></button></div>
          </div>

          <h2 class="group">{{ t('prefs.residentPanel') }} <span class="tag">{{ t('prefs.residentOnly') }}</span></h2>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.residentLayout') }}</div><div class="d">{{ t('prefs.residentLayoutDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: residentLayout === 'b' }" :disabled="saving === 'layout'" @click="onLayout('b')"><span class="opt-title">{{ t('prefs.layoutB') }}</span></button>
                <button type="button" class="opt-btn" :class="{ active: residentLayout === 'a' }" :disabled="saving === 'layout'" @click="onLayout('a')"><span class="opt-title">{{ t('prefs.layoutA') }}</span></button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.showSnoozed') }}</div><div class="d">{{ t('prefs.showSnoozedDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showSnoozed }" :disabled="saving === 'showSnoozed'" @click="onShowSnoozed(!showSnoozed)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.showIdle') }}</div><div class="d">{{ t('prefs.showIdleDesc') }}</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showIdle }" :disabled="saving === 'showIdle'" @click="onShowIdle(!showIdle)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.opacity') }}</div><div class="d">{{ t('prefs.opacityDesc') }}</div></div>
            <div class="ctl">
              <div class="slider" @pointerdown="onSliderDown">
                <div class="slider-track" ref="sliderTrack">
                  <div class="slider-fill" :style="{ width: sliderPct + '%' }"></div>
                  <div class="slider-knob" :style="{ left: sliderPct + '%' }"></div>
                </div>
              </div>
              <span class="val">{{ opacity }}%</span>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.panelWidth') }}</div><div class="d">{{ t('prefs.panelWidthDesc') }}</div></div>
            <div class="ctl">
              <div class="slider" @pointerdown="onSliderDownWidth">
                <div class="slider-track" ref="sliderTrackWidth">
                  <div class="slider-fill" :style="{ width: widthPct + '%' }"></div>
                  <div class="slider-knob" :style="{ left: widthPct + '%' }"></div>
                </div>
              </div>
              <span class="val">{{ residentWidth }}px</span>
            </div>
          </div>
        </section>

        <!-- 更新 -->
        <section v-show="activeCategory === 'update'">
          <h2 class="group">{{ t('prefs.update') }}</h2>
          <div class="row">
            <div class="txt"><div class="t">{{ t('prefs.updateSource') }}</div><div class="d">{{ t('prefs.updateSourceDesc') }}</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: updateSourcePref === 'auto' }" :disabled="saving === 'updateSource'" @click="onUpdateSource('auto')"><span class="opt-title">{{ t('prefs.sourceAuto') }}</span></button>
                <button type="button" class="opt-btn" :class="{ active: updateSourcePref === 'gitee' }" :disabled="saving === 'updateSource'" @click="onUpdateSource('gitee')"><span class="opt-title">{{ t('prefs.sourceGitee') }}</span></button>
              </div>
            </div>
          </div>
          <div class="update-box">
            <div>
              <div class="t">{{ t('prefs.currentVersion', { version: appVersion }) }}</div>
              <p v-if="upToDate" class="muted">{{ t('prefs.upToDate') }}</p>
            </div>
            <button class="btn" @click="checkForUpdates" :disabled="checking">{{ checking ? t('prefs.checking') : t('prefs.checkUpdate') }}</button>
          </div>

          <!-- 发现新版本 -->
          <div v-if="updateAvailable" class="update-available">
            <div class="update-header">
              <div class="update-version">
                <span class="update-arrow">↑</span>
                <span>{{ t('prefs.newVersion') }} <strong>{{ updateAvailable.version }}</strong></span>
                <span class="update-diff">{{ t('prefs.updateFrom', { version: appVersion }) }}</span>
              </div>
              <button class="btn btn-primary" @click="downloadAndInstall" :disabled="installing">{{ installLabel }}</button>
            </div>
            <!-- 下载进度条 -->
            <div v-if="installing && installPhase === 'downloading'" class="download-progress">
              <div class="progress-track">
                <div class="progress-fill" :style="{ width: downloadProgress + '%' }"></div>
              </div>
              <span class="progress-size">{{ fmtBytes(downloadLoaded) }} / {{ fmtBytes(downloadTotal) }}</span>
            </div>
            <div v-if="installing && installPhase !== 'downloading'" class="download-progress">
              <span class="progress-size">{{ installLabel }}</span>
            </div>
            <div v-if="updateAvailable.body" class="changelog">
              <div class="changelog-title">{{ t('prefs.changelog') }}</div>
              <pre class="changelog-body">{{ updateAvailable.body }}</pre>
            </div>
            <a class="full-changelog" href="https://github.com/Ethanzyc/cc-view/releases" target="_blank" rel="noopener">{{ t('prefs.fullChangelog') }}</a>
          </div>

          <p v-if="installError" class="error">⚠ {{ installError }}</p>
        </section>
      </main>
    </div>
    <p v-if="error" class="error footer-error">⚠ {{ error }}</p>
    <p class="repo">
      <a href="https://github.com/Ethanzyc/cc-view" target="_blank" rel="noopener">GitHub →</a>
      <span class="repo-sep">·</span>
      <a href="https://gitee.com/Ethanzyc/cc-view" target="_blank" rel="noopener">Gitee →</a>
    </p>
  </div>
</template>

<style scoped>
.prefs {
  display: flex; flex-direction: column; height: 100vh;
  color: var(--color-fg); font-family: var(--font-body);
  -webkit-font-smoothing: antialiased;
}

/* 双栏（无 topbar——native title bar 已显示标题） */
.body { flex: 1; display: flex; overflow: hidden; }
nav.cats {
  width: 180px; border-right: 1px solid var(--color-border);
  padding: 0 0 8px; overflow-y: auto; flex-shrink: 0;
  background: color-mix(in srgb, var(--prefs-bg) 60%, var(--color-hover));
}
/* 品牌区（logo + 名称） */
.brand {
  display: flex; align-items: center; gap: 8px;
  padding: 14px 16px 12px;
}
.brand-icon { width: 20px; height: 20px; border-radius: 5px; }
.brand-name { font-size: 14px; font-weight: 700; color: var(--color-fg); letter-spacing: -0.01em; }
.cat {
  padding: 8px 16px 8px 14px; cursor: pointer; color: var(--color-muted);
  font-size: 13px; border-left: 2px solid transparent; transition: background var(--motion-duration) var(--motion-easing);
}
.cat:hover { background: var(--color-hover); color: var(--color-fg); }
.cat.active {
  background: var(--color-hover); color: var(--color-fg); font-weight: 600;
  border-left-color: var(--color-primary);
}

main.settings { flex: 1; overflow-y: auto; padding: 8px 28px 28px; }

/* 分组标题（一级，醒目） */
.group {
  font-size: 17px; font-weight: 700; margin: 20px 0 6px;
  padding-bottom: 8px; border-bottom: 1px solid var(--color-border);
  letter-spacing: -0.01em; display: flex; align-items: center; gap: 8px;
}
.group:first-child { margin-top: 8px; }
.tag { font-size: 10px; font-weight: 600; color: var(--color-primary); border: 1px solid color-mix(in srgb, var(--color-primary) 40%, transparent); border-radius: 8px; padding: 1px 7px; letter-spacing: 0; }

/* 设置项行 */
.row {
  display: flex; align-items: center; justify-content: space-between;
  padding: 12px 0; border-bottom: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
  gap: 24px;
}
.txt { flex: 1; min-width: 0; }
.txt .t { font-weight: 600; color: var(--color-fg); font-size: 12px; }
.txt .d { font-size: 11px; color: var(--color-muted); margin-top: 2px; }
.ctl { flex-shrink: 0; display: flex; align-items: center; gap: 8px; }

/* controls */
/* macOS 风格 toggle switch（替代 checkbox） */
.toggle { display: inline-flex; align-items: center; background: none; border: none; padding: 0; cursor: pointer; }
.toggle:disabled { cursor: default; opacity: 0.5; }
.switch-knob { position: relative; width: 28px; height: 16px; background: var(--color-border); border-radius: 8px; transition: background var(--motion-duration) var(--motion-easing); }
.switch-knob::after { content: ''; position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; background: #fff; border-radius: 50%; box-shadow: 0 1px 2px rgba(0,0,0,0.25); transition: transform var(--motion-duration) var(--motion-easing); }
.toggle.on .switch-knob { background: var(--color-primary); }
.toggle.on .switch-knob::after { transform: translateX(12px); }
.num { width: 56px; padding: 4px 8px; border: 1px solid var(--color-border); border-radius: 6px; font: inherit; text-align: center; background: var(--prefs-bg); color: var(--color-fg); }
.unit { font-size: 12px; color: var(--color-muted); }

/* 按钮组 */
.opt-group { display: flex; gap: 6px; }
.opt-btn {
  padding: 6px 14px; border: 1.5px solid var(--color-border); border-radius: 6px;
  background: var(--prefs-bg); cursor: pointer; font: inherit; color: var(--color-fg);
  transition: border-color var(--motion-duration) var(--motion-easing), background var(--motion-duration) var(--motion-easing);
}
.opt-btn:hover { border-color: var(--color-primary); }
.opt-btn.active { border-color: var(--color-primary); background: color-mix(in srgb, var(--color-primary) 10%, var(--prefs-bg)); }
.opt-btn:disabled { opacity: 0.5; cursor: default; }
.opt-btn .opt-title { font-size: var(--fs-body); font-weight: 600; }

.check-line { display: flex; align-items: center; gap: 4px; font-size: 12px; color: var(--color-muted); cursor: pointer; }
.check-line input { width: 16px; height: 16px; }

/* slider */
.slider { width: 140px; padding: 10px 0; cursor: pointer; touch-action: none; }
.slider-track { position: relative; height: 4px; background: var(--color-border); border-radius: 2px; }
.slider-fill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--color-primary); border-radius: 2px; }
.slider-knob {
  position: absolute; top: 50%; width: 14px; height: 14px; border-radius: 50%;
  background: #fff; border: 1px solid var(--color-primary); transform: translate(-50%, -50%);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25); transition: transform var(--motion-duration) var(--motion-easing);
}
.slider:active .slider-knob { transform: translate(-50%, -50%) scale(1.12); }
.val { font-size: 12px; color: var(--color-muted); min-width: 44px; text-align: right; font-variant-numeric: tabular-nums; }

/* 更新 */
.update-box {
  display: flex; align-items: center; justify-content: space-between;
  padding: 14px 16px; background: var(--color-hover); border-radius: 8px; margin-top: 8px;
}
.update-box .t { font-weight: 600; font-size: 12px; color: var(--color-fg); }
.update-box .muted { color: var(--status-working-ink); font-size: 12px; margin-top: 4px; }

/* 发现新版本面板 */
.update-available {
  margin-top: 12px; border: 1px solid var(--color-border); border-radius: 10px; overflow: hidden;
}
.update-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 14px 16px; background: color-mix(in srgb, var(--color-primary) 8%, var(--prefs-bg));
}
.update-version { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--color-fg); }
.update-arrow { font-size: 16px; color: var(--color-primary); font-weight: 700; }
.update-version strong { font-size: 14px; }
.update-diff { font-size: 11px; color: var(--color-muted); margin-left: 4px; }

.changelog { padding: 12px 16px; border-top: 1px solid var(--color-border); }
.changelog-title { font-size: 11px; font-weight: 600; color: var(--color-muted); text-transform: uppercase; letter-spacing: 0.04em; margin-bottom: 8px; }
.changelog-body { white-space: pre-wrap; font-size: 12px; line-height: 1.6; color: var(--color-fg); margin: 0; font-family: var(--font-body); }

.full-changelog { display: block; padding: 10px 16px; border-top: 1px solid var(--color-border); font-size: 11px; color: var(--color-primary); text-decoration: none; }
.full-changelog:hover { text-decoration: underline; }

.btn {
  padding: 6px 14px; border: 1px solid var(--color-border); border-radius: 6px;
  background: var(--prefs-bg); color: var(--color-fg); cursor: pointer; font: inherit;
  transition: background var(--motion-duration) var(--motion-easing);
}
.btn:not(:disabled):hover { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
.btn:disabled { opacity: 0.5; cursor: default; }
.btn-primary { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
.btn-primary:not(:disabled):hover { filter: brightness(1.1); }

/* 下载进度 */
.download-progress { padding: 0 16px 14px; display: flex; align-items: center; gap: 12px; }
.progress-track { flex: 1; height: 4px; background: var(--color-border); border-radius: 2px; overflow: hidden; }
.progress-fill { height: 100%; background: var(--color-primary); border-radius: 2px; transition: width 0.2s ease; }
.progress-size { font-size: 11px; color: var(--color-muted); white-space: nowrap; font-variant-numeric: tabular-nums; }

.error { color: var(--status-permission); }
.footer-error { margin: 12px 20px 0; }
.repo { padding: 10px 20px; border-top: 1px solid var(--color-border); text-align: center; flex-shrink: 0; display: flex; justify-content: center; gap: 8px; align-items: center; }
.repo a { color: var(--color-primary); text-decoration: none; font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body); }
.repo a:hover { text-decoration: underline; }
.repo-sep { color: var(--color-border); font-size: 12px; }
</style>
