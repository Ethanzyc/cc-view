// 截图 mock：浏览器渲染 cc-view UI（Overlay/Preferences）+ mock 数据。
// CC_VIEW_MOCK=1 时 vite alias 把 @tauri-apps/api/{core,event,webviewWindow,app} 指向本文件。
import type { Session } from '../types';

// === mock 数据 ===
const now = Date.now();
const MIN = 60_000, HOUR = 60 * MIN, DAY = 24 * HOUR;

function mk(o: Partial<Session> & Pick<Session, 'id' | 'project' | 'name' | 'status'>): Session {
  return {
    source: 'interactive',
    pid: 1,
    cwd: o.project,
    startedAt: now - HOUR,
    statusUpdatedAt: now - 5 * MIN,
    alive: true,
    focusHint: { host: 'Ghostty' },
    snoozed: false,
    ...o,
  } as Session;
}

const scenes: Record<string, Session[]> = {
  // 默认：混合各状态（最能展示）
  default: [
    mk({ id: 's1', project: '/Users/dev/api-server', name: 'refactor-auth', status: 'needsPermission', statusUpdatedAt: now - 30_000 }),
    mk({ id: 's2', project: '/Users/dev/api-server', name: 'add-tests', status: 'working', statusUpdatedAt: now - 2 * MIN }),
    mk({ id: 's3', project: '/Users/dev/web-client', name: 'fix-navbar', status: 'waitingForReply', statusUpdatedAt: now - 60_000 }),
    mk({ id: 's4', project: '/Users/dev/cli-tool', name: 'write-docs', status: 'waitingForInput', statusUpdatedAt: now - 90_000 }),
    mk({ id: 's5', project: '/Users/dev/legacy-app', name: 'cleanup-logs', status: 'working', statusUpdatedAt: now - DAY, alive: false }),
  ],
  // 等权限突出（橙边）
  permission: [
    mk({ id: 's1', project: '/Users/dev/api-server', name: 'deploy-prod', status: 'needsPermission', statusUpdatedAt: now - 20_000 }),
    mk({ id: 's2', project: '/Users/dev/api-server', name: 'migrate-db', status: 'needsPermission', statusUpdatedAt: now - 80_000 }),
    mk({ id: 's3', project: '/Users/dev/web-client', name: 'feature-x', status: 'working', statusUpdatedAt: now - 3 * MIN }),
  ],
  // 闲置（等输入超 30min 灰显 + 整组下沉）
  idle: [
    mk({ id: 's1', project: '/Users/dev/web-client', name: 'feature-x', status: 'working', statusUpdatedAt: now - 2 * MIN }),
    mk({ id: 's2', project: '/Users/dev/legacy-app', name: 'waiting-task', status: 'waitingForInput', statusUpdatedAt: now - 45 * MIN }),
    mk({ id: 's3', project: '/Users/dev/legacy-app', name: 'another-old', status: 'waitingForInput', statusUpdatedAt: now - 2 * HOUR }),
  ],
};

function sceneSessions(): Session[] {
  const scene = new URLSearchParams(location.search).get('scene') || 'default';
  return scenes[scene] || scenes.default;
}

// === 毛玻璃模拟：body 深色渐变（模拟桌面）+ overlay backdrop-filter ===
if (typeof document !== 'undefined') {
  const style = document.createElement('style');
  style.textContent = `
    html, body { margin: 0; height: 100vh; }
    body {
      background: linear-gradient(135deg, #2d2d44 0%, #1a1a2e 50%, #16213e 100%);
      display: flex; align-items: center; justify-content: center;
    }
    /* 模拟真 app 的窗口边框 + 系统阴影（NSWindow shadow）*/
    .overlay, .prefs {
      min-height: auto !important;
      backdrop-filter: blur(24px) saturate(180%);
      -webkit-backdrop-filter: blur(24px) saturate(180%);
      border: 1px solid rgba(255, 255, 255, 0.12);
      box-shadow: 0 24px 70px rgba(0, 0, 0, 0.55);
    }
    .overlay { width: 560px; }
    .prefs { width: 480px; }
  `;
  document.head.appendChild(style);
}

// === @tauri-apps/api/core: invoke ===
export async function invoke<T = unknown>(cmd: string, _args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case 'get_sessions': return sceneSessions() as unknown as T;
    case 'list_archived':
    case 'list_hidden': return [] as unknown as T;
    case 'get_overlay_pinned': return false as unknown as T;
    case 'get_prefs': return { notify: true, shortcut: 'alt+space', poll_interval: 3, show_archived: false } as unknown as T;
    case 'get_autostart': return false as unknown as T;
    default: return undefined as unknown as T; // 写操作 noop
  }
}

// === @tauri-apps/api/event: listen（mock：不触发，数据固定）===
export async function listen<T = unknown>(_event: string, _handler: (e: { payload: T }) => void): Promise<() => void> {
  return () => {};
}

// === @tauri-apps/api/webviewWindow: getCurrentWebviewWindow ===
export function getCurrentWebviewWindow() {
  const view = new URLSearchParams(location.search).get('view') || 'overlay';
  return {
    label: view,
    hide: async () => {},
    show: async () => {},
    setFocus: async () => {},
    onFocusChanged: async (_cb: (e: { payload: boolean }) => void) => () => {},
  };
}

// === @tauri-apps/api/app: getVersion ===
export async function getVersion(): Promise<string> {
  return '0.1.2';
}
