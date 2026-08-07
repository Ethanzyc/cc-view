export type Status = 'working' | 'waitingForInput' | 'waitingForReply' | 'needsPermission' | 'shell' | 'compacting';
export interface Session {
  id: string;
  source: string;
  pid: number;
  project: string;
  cwd: string;
  name: string;
  status: Status;
  startedAt: number;
  statusUpdatedAt: number;
  alive: boolean;
  focusHint: { host: string };
  // derived：后端 poll_loop 基于 SnoozeMap 算，随 sessions emit 一起下发（Task 1/2）
  snoozed: boolean;
  // 累计 token（后端 scan JSONL 填充；纯 shell 会话为 0）
  tokensIn: number;
  tokensOut: number;
}

// overlay 窗口模式（与后端 prefs::OverlayMode serde lowercase 对齐）
export type OverlayMode = 'resident' | 'panel';
// 常驻布局（与后端 prefs::ResidentLayout serde lowercase 对齐）
export type ResidentLayout = 'b' | 'a';
// 外观主题（与后端 prefs::Theme serde lowercase 对齐）
export type Theme = 'light' | 'dark';

// get_prefs 返回的完整偏好（与 Rust Prefs 字段一一对应）
export interface Prefs {
  notify: boolean;
  shortcut: string;
  poll_interval: number;
  mode: OverlayMode;
  resident_layout: ResidentLayout;
  resident_show_snoozed: boolean;
  resident_show_idle: boolean;
  resident_opacity: number;
  theme: Theme;
  show_archived: boolean;
}
