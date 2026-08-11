<script setup lang="ts">
// 偏好设置（VSCode Settings 风格：左 nav 分类 + 右设置项行）。
// 分类：通用 / 显示 / 更新。⌘, 全局快捷键打开（lib.rs 注册）。
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { shallowRef } from 'vue';
import { relaunch } from '@tauri-apps/plugin-process';
import type { Prefs, ResidentLayout, Theme, TokenUnit } from '../types';
import { applyTheme } from '../utils/theme';
import iconUrl from '../assets/cc-view-icon.png';

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
const saving = ref<string | null>(null);
const error = ref<string | null>(null);
const appVersion = ref('');
const checking = ref(false);
// shallowRef：Update 继承 Resource，内部用 WeakMap 存私有 rid 字段。
// ref() 会用 Proxy 包装对象 → this.rid 的 WeakMap 查不到 Proxy → "Cannot read private member"。
const updateAvailable = shallowRef<Update | null>(null);
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
    error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '保存失败';
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

let widthTimer: number | undefined;
const onWidth = (v: number) => {
  residentWidth.value = v;
  clearTimeout(widthTimer);
  widthTimer = window.setTimeout(async () => {
    saving.value = 'width';
    try {
      await invoke('set_resident_width', { width: v });
    } catch (e: unknown) {
      error.value = typeof e === 'string' ? e : (e as Error)?.message ?? '保存失败';
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
    const upd = await check({ timeout: 8000 });
    if (upd) updateAvailable.value = upd;
    else upToDate.value = true;
  } catch (e: unknown) {
    const msg = typeof e === 'string' ? e : (e as Error)?.message ?? '检查失败';
    error.value = /sending request|fetch|network|timeout|connect/i.test(msg)
      ? '⚠ 无法连接更新服务器（GitHub + Gitee 均不可达）'
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
    await updateAvailable.value.downloadAndInstall((event) => {
      if (event.event === 'Started' && event.data.contentLength) {
        downloadTotal.value = event.data.contentLength;
      } else if (event.event === 'Progress' && event.data.chunkLength) {
        downloadLoaded.value += event.data.chunkLength;
        if (downloadTotal.value > 0) {
          downloadProgress.value = Math.round((downloadLoaded.value / downloadTotal.value) * 100);
        }
      } else if (event.event === 'Finished') {
        installPhase.value = 'installing';
        downloadProgress.value = 100;
      }
    });
    installPhase.value = 'restarting';
    await relaunch();
  } catch (e: unknown) {
    installError.value = typeof e === 'string' ? e : (e as Error)?.message ?? '安装失败';
    installing.value = false;
    installPhase.value = 'idle';
  }
}

const installLabel = computed(() => {
  switch (installPhase.value) {
    case 'downloading': return `下载中 ${downloadProgress.value}%`;
    case 'installing': return '安装中…';
    case 'restarting': return '重启中…';
    default: return '下载并安装';
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
        <div class="cat" :class="{ active: activeCategory === 'general' }" @click="activeCategory = 'general'">通用</div>
        <div class="cat" :class="{ active: activeCategory === 'display' }" @click="activeCategory = 'display'">显示</div>
        <div class="cat" :class="{ active: activeCategory === 'update' }" @click="activeCategory = 'update'">更新</div>
      </nav>

      <main class="settings">
        <!-- 通用 -->
        <section v-show="activeCategory === 'general'">
          <h2 class="group">通用</h2>
          <div class="row">
            <div class="txt"><div class="t">开机自启动</div><div class="d">登录 macOS 时自动启动 cc-view</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: autostart }" :disabled="saving === 'autostart'" @click="onAutostart(!autostart)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">通知</div><div class="d">会话进入待介入时弹系统通知</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: notify }" :disabled="saving === 'notify'" @click="onNotify(!notify)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">全局快捷键</div><div class="d">呼出 / 收起命令面板（⌘, 开偏好已独立注册）</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button v-for="s in shortcuts" :key="s.value" type="button" class="opt-btn" :class="{ active: shortcut === s.value }" :disabled="saving === 'shortcut'" @click="onShortcut(s.value)">
                  <span class="opt-title">{{ s.label }}</span>
                </button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">轮询间隔</div><div class="d">采集会话状态的频率（1–30 秒）</div></div>
            <div class="ctl"><input type="number" class="num" min="1" max="30" :value="interval" :disabled="saving === 'interval'" @change="onInterval(Number(($event.target as HTMLInputElement).value))" /> <span class="unit">秒</span></div>
          </div>
        </section>

        <!-- 显示（合并外观 + 常驻面板） -->
        <section v-show="activeCategory === 'display'">
          <h2 class="group">外观</h2>
          <div class="row">
            <div class="txt"><div class="t">主题</div><div class="d">浅色 / 深色（不跟随系统）</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: theme === 'light' }" :disabled="saving === 'theme'" @click="onTheme('light')"><span class="opt-title">浅色</span></button>
                <button type="button" class="opt-btn" :class="{ active: theme === 'dark' }" :disabled="saving === 'theme'" @click="onTheme('dark')"><span class="opt-title">深色</span></button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">token 单位</div><div class="d">列表 / 详情 token 显示单位</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: tokenUnitPref === 'km' }" :disabled="saving === 'tokenUnit'" @click="onTokenUnit('km')"><span class="opt-title">k / M</span></button>
                <button type="button" class="opt-btn" :class="{ active: tokenUnitPref === 'wan' }" :disabled="saving === 'tokenUnit'" @click="onTokenUnit('wan')"><span class="opt-title">万 / 亿</span></button>
              </div>
            </div>
          </div>

          <h2 class="group">列表显示</h2>
          <div class="row">
            <div class="txt"><div class="t">显示终端名</div><div class="d">在会话名旁标注终端 app（如 Otty、iTerm）</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showHost }" :disabled="saving === 'showHost'" @click="onShowHost(!showHost)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">显示 token 用量</div><div class="d">每行的输入/输出累计 token</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showTokens }" :disabled="saving === 'showTokens'" @click="onShowTokens(!showTokens)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">显示操作按钮</div><div class="d">面板模式每行的详情/搁置/归档/复制按钮</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showActions }" :disabled="saving === 'showActions'" @click="onShowActions(!showActions)"><span class="switch-knob"></span></button></div>
          </div>

          <h2 class="group">常驻面板 <span class="tag">仅常驻</span></h2>
          <div class="row">
            <div class="txt"><div class="t">常驻布局</div><div class="d">B 精简（带状态文字）/ A 极简（仅图标名称）</div></div>
            <div class="ctl">
              <div class="opt-group">
                <button type="button" class="opt-btn" :class="{ active: residentLayout === 'b' }" :disabled="saving === 'layout'" @click="onLayout('b')"><span class="opt-title">B 精简</span></button>
                <button type="button" class="opt-btn" :class="{ active: residentLayout === 'a' }" :disabled="saving === 'layout'" @click="onLayout('a')"><span class="opt-title">A 极简</span></button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">显示搁置的会话</div><div class="d">搁置 = 你手动标记「暂时不管」的会话（不催促、不通知）</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showSnoozed }" :disabled="saving === 'showSnoozed'" @click="onShowSnoozed(!showSnoozed)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">显示闲置的会话</div><div class="d">闲置 = 等输入超过 30 分钟未给下一条指令，自动降级</div></div>
            <div class="ctl"><button class="toggle" :class="{ on: showIdle }" :disabled="saving === 'showIdle'" @click="onShowIdle(!showIdle)"><span class="switch-knob"></span></button></div>
          </div>
          <div class="row">
            <div class="txt"><div class="t">背景透明度</div><div class="d">常驻面板贴桌面的透明度</div></div>
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
            <div class="txt"><div class="t">面板宽度</div><div class="d">常驻面板宽度（右边锚定不动）</div></div>
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
          <h2 class="group">更新</h2>
          <div class="update-box">
            <div>
              <div class="t">当前版本 CC View {{ appVersion }}</div>
              <p v-if="upToDate" class="muted">✓ 已是最新版本</p>
            </div>
            <button class="btn" @click="checkForUpdates" :disabled="checking">{{ checking ? '检查中…' : '检查更新' }}</button>
          </div>

          <!-- 发现新版本 -->
          <div v-if="updateAvailable" class="update-available">
            <div class="update-header">
              <div class="update-version">
                <span class="update-arrow">↑</span>
                <span>新版本 <strong>{{ updateAvailable.version }}</strong></span>
                <span class="update-diff">从 {{ appVersion }} 更新</span>
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
              <div class="changelog-title">更新内容</div>
              <pre class="changelog-body">{{ updateAvailable.body }}</pre>
            </div>
            <a class="full-changelog" href="https://github.com/Ethanzyc/cc-view/releases" target="_blank" rel="noopener">查看完整更新日志 →</a>
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
