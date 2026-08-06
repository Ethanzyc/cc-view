// 把主题应用到 <html>：dark 加 .dark class（CSS html.dark 覆盖深色 token），light 移除。
// overlay 与 prefs 窗口共用同一入口 App.vue，documentElement 切换对两窗口都生效。
import type { Theme } from '../types';

export function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle('dark', theme === 'dark');
}
