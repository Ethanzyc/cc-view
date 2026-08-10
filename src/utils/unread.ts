// 未读红点状态（任务完成提醒，未读消息式）。
// 触发：会话从「非待介入」切到「待介入」（与 ResidentView detectAndFlash 同条件）。
// 清除：① focus（点击跳转）② 待介入→非待介入（用户已处理）。
// 内存态（重启清——重启后 re-detect 当前待介入）。PanelView + ResidentView 共享。
import { ref } from 'vue';
import type { Session } from '../types';

const ATTENTION = new Set<string>([
  'waitingForInput',
  'waitingForReply',
  'needsPermission',
]);

// 上次状态基线（检测切换用）。首轮只填基线不标记，避免启动时已待介入的全标红。
const prevStatus = new Map<string, string>();

export const unread = ref<Set<string>>(new Set());

/// 处理新 sessions：更新 prevStatus + 标记/清除 unread（非待介入↔待介入切换）。
/// PanelView / ResidentView 在 sessions 事件到来时调用。
export function processUnread(next: Session[]) {
  const nextIds = new Set(next.map((s) => s.id));
  let changed = false;
  for (const s of next) {
    const p = prevStatus.get(s.id);
    if (p !== undefined && s.alive && !s.snoozed) {
      if (!ATTENTION.has(p) && ATTENTION.has(s.status)) {
        unread.value.add(s.id); // 非待介入→待介入：未读
        changed = true;
      } else if (ATTENTION.has(p) && !ATTENTION.has(s.status)) {
        unread.value.delete(s.id); // 待介入→非待介入：已处理，清除
        changed = true;
      }
    }
    prevStatus.set(s.id, s.status);
  }
  // 清除已不在的会话
  for (const id of [...prevStatus.keys()]) {
    if (!nextIds.has(id)) {
      prevStatus.delete(id);
      if (unread.value.delete(id)) changed = true;
    }
  }
  if (changed) unread.value = new Set(unread.value);
}

/// focus（点击跳转）后清除：用户已看到。
export function clearUnread(id: string) {
  if (unread.value.delete(id)) {
    unread.value = new Set(unread.value);
  }
}
