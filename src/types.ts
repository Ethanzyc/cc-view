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
// token 量单位（与后端 prefs::TokenUnit serde lowercase 对齐）
export type TokenUnit = 'km' | 'wan';
// 常驻布局（与后端 prefs::ResidentLayout serde lowercase 对齐）
export type ResidentLayout = 'b' | 'a';
// 外观主题（与后端 prefs::Theme serde lowercase 对齐）
export type Theme = 'light' | 'dark';
// 更新源（与后端 prefs::UpdateSource serde lowercase 对齐）
export type UpdateSource = 'auto' | 'gitee';

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
  token_unit: TokenUnit;
  // 常驻面板宽度（null = 用 layout 默认 A=180/B=285）。后端 Option<f64>。
  resident_width: number | null;
  // 显示控制
  show_host: boolean;
  show_tokens: boolean;
  show_actions: boolean;
  update_source: UpdateSource;
}

// host 枚举 → 短显示名（后端 serde camelCase）
const HOST_LABELS: Record<string, string> = {
  unknown: '',
  iTerm2: 'iTerm',
  ghostty: 'Ghostty',
  kitty: 'Kitty',
  vscode: 'VSCode',
  idea: 'IDEA',
  terminal: 'Terminal',
  otty: 'Otty',
  cmux: 'cmux',
  tmux: 'tmux',
  warp: 'Warp',
  wezTerm: 'WezTerm',
  alacritty: 'Alacritty',
};
export function hostLabel(host: string): string {
  return HOST_LABELS[host] ?? '';
}

// 按回合的消耗明细（与后端 TurnStat camelCase 对齐）
export interface TurnStat {
  idx: number;
  prompt: string;
  tokensIn: number;
  tokensOut: number;
  toolCalls: number;
  ctx: number; // 该回合最后一条 assistant 的上下文占用（sparkline 用）
  ts: string; // ISO 8601 原始字符串，前端 Date.parse
}

// get_session_detail 返回的完整详情
export interface SessionDetail {
  sessionId: string;
  tokensIn: number;
  tokensOut: number;
  cacheRead: number;
  cacheCreation: number;
  model: string;
  turnCount: number;
  toolCalls: number;
  webSearches: number;
  webFetches: number;
  // 上下文：当前 = 最后一条 assistant 的 input+cache；峰值 = 历史最高
  contextCurrent: number;
  contextPeak: number;
  compactCount: number;
  turns: TurnStat[];
}
