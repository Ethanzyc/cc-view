// 会话展示工具：SessionList 与 Overlay 共用（去重，原两组件各自重复一份）。
import type { Session, Status } from '../types';

// 状态中文名：保留 cc 真实状态（不因 snoozed 改成"已搁置"——分组标题已表达）
export const STATUS_ZH: Record<Status, string> = {
  working: '工作中',
  waitingForInput: '等输入',
  needsPermission: '等权限',
  shell: 'Shell',
  compacting: '压缩中',
};

// 排序档：等权限 > 等输入 > 工作 > Shell > 压缩 > 搁置(alive 5.5) > 已退出(6) > 搁置(dead 6.5)
export function statusRank(s: Session): number {
  if (s.snoozed) return s.alive ? 5.5 : 6.5;
  if (!s.alive) return 6;
  switch (s.status) {
    case 'needsPermission': return 1;
    case 'waitingForInput': return 2;
    case 'working': return 3;
    case 'shell': return 4;
    case 'compacting': return 5;
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

// "刚完成"高亮：waitingForInput 且 ago < 120s 且未搁置（搁置行不显示蓝点）
export function isFresh(s: Session): boolean {
  return !s.snoozed &&
    s.status === 'waitingForInput' &&
    Date.now() - s.statusUpdatedAt < 120_000;
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
