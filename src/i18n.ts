// vue-i18n 实例：默认 zh，App.vue 从 prefs 同步 resolved locale 后切换。
import { createI18n } from 'vue-i18n';
import zh from './locales/zh';
import en from './locales/en';

export type AppLocale = 'zh' | 'en';

export const i18n = createI18n({
  legacy: false,
  locale: 'zh',
  fallbackLocale: 'en',
  messages: { zh, en },
});

// 便捷切换（App.vue prefs 同步时调）
export function setLocale(locale: AppLocale) {
  i18n.global.locale.value = locale;
}

// auto → 按浏览器/系统语言 resolve（与后端 sys_locale 逻辑一致）
export function applyPrefsLocale(pref: 'auto' | 'zh' | 'en') {
  const resolved: AppLocale = pref === 'zh' ? 'zh' : pref === 'en' ? 'en'
    : navigator.language.startsWith('zh') ? 'zh' : 'en';
  setLocale(resolved);
}
