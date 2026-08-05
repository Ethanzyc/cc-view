// Overlay 会话展示工具（排序/分组/状态文案等，从原两组件去重合并而来）。
import type { Session, Status } from '../types';

// 状态中文名：保留 cc 真实状态（不因 snoozed 改成"已搁置"——分组标题已表达）
export const STATUS_ZH: Record<Status, string> = {
  working: '工作中',
  waitingForInput: '等输入',
  waitingForReply: '等回答',
  needsPermission: '等权限',
  shell: 'Shell',
  compacting: '压缩中',
};

// 排序档：等权限 > 等回答 > 等输入 > 工作 > Shell > 压缩 > 搁置(alive 6.5) > 已退出(7) > 搁置(dead 7.5)
export function statusRank(s: Session): number {
  if (s.snoozed) return s.alive ? 6.5 : 7.5;
  if (!s.alive) return 7;
  switch (s.status) {
    case 'needsPermission': return 1;
    case 'waitingForReply': return 2;
    case 'waitingForInput': return 3;
    case 'working': return 4;
    case 'shell': return 5;
    case 'compacting': return 6;
    default: return 99;
  }
}

// 项目路径缩短：/Users/<name>/ai/fang → ~/ai/fang（二级小标题 + 行 line2 + 搜索匹配共用）
export const projShort = (p: string): string =>
  p.replace(/^\/Users\/[^/]+\//, '~/');

// ago 自适应：<60s→Xs, <3600→Xm, <86400→Xh, else→Xd
export function agoF(ts: number): string {
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

// "刚完成/刚提问"高亮：等输入或等回答 且 ago < 120s 且未搁置（搁置行不显示蓝点）
export function isFresh(s: Session): boolean {
  return !s.snoozed &&
    (s.status === 'waitingForInput' || s.status === 'waitingForReply') &&
    Date.now() - s.statusUpdatedAt < 120_000;
}

// 等输入闲置阈值：超过此时长未给下一条指令的等输入在 overlay 降级（灰显 + 排后）。
export const STALE_INPUT_MS = 30 * 60 * 1000;

// 等输入闲置：waitingForInput + 非搁置 + 距 statusUpdatedAt 超阈值。
// now 由调用方传入（响应式 now ref），让前端能定期重算——后端 emit 有 hash 去重，
// 不传时间晾着的等输入跨阈值时不会自动触发。仅对等输入（等回答是阻塞提问该醒目，等权限太重要）。
export function isStaleInput(s: Session, now: number): boolean {
  return s.status === 'waitingForInput'
    && !s.snoozed
    && now - s.statusUpdatedAt > STALE_INPUT_MS;
}

// 高亮 span 拆分（搜索匹配高亮，不用 v-html 防 XSS）。k='' 或未匹配返回单段无高亮。
export type HlSeg = { text: string; hl: boolean };
export function hlParts(text: string, k: string): HlSeg[] {
  if (!k) return [{ text, hl: false }];
  const i = text.toLowerCase().indexOf(k);
  if (i < 0) return [{ text, hl: false }];
  return [
    { text: text.slice(0, i), hl: false },
    { text: text.slice(i, i + k.length), hl: true },
    { text: text.slice(i + k.length), hl: false },
  ].filter(seg => seg.text.length > 0);
}
