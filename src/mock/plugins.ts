// 截图 mock：@tauri-apps/plugin-updater + plugin-process
// CC_VIEW_MOCK=1 时 vite alias 指向本文件。

export type Update = {
  version: string;
  body?: string;
  date?: string;
  downloadAndInstall: (_onEvent?: (e: unknown) => void) => Promise<void>;
};

// mock：无更新（README 偏好设置截图显示「已是最新」）
export async function check(): Promise<Update | null> {
  return null;
}

// @tauri-apps/plugin-process: relaunch
export async function relaunch(): Promise<void> {}
