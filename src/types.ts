export type Status = 'working' | 'waitingForInput' | 'needsPermission' | 'shell';
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
}
