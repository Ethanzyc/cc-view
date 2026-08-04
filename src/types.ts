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
}
