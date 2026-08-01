<script setup lang="ts">
// 每行渲染一个会话；右侧按钮按 hidden 判断：
//   已隐藏 → "+" 调 unhide_session，未隐藏 → "×" 调 hide_session。
// 成功后 emit('hide'|'unhide') 让父组件 App 刷新 hidden 集合 → visible computed 更新。
import type { Session, Status } from '../types';
import { invoke } from '@tauri-apps/api/core';

withDefaults(defineProps<{ sessions: Session[]; hidden?: string[] }>(), {
  hidden: () => [] as string[],
});
const emit = defineEmits<{ (e: 'hide', id: string): void; (e: 'unhide', id: string): void }>();

const icon: Record<Status, string> = {
  working: '⚡', waitingForInput: '💤', needsPermission: '⏳', shell: '🖥️',
};
function ago(ts: number) {
  const s = Math.floor((Date.now() - ts) / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m`;
}
// invoke 失败时 console.error 记录，UI 不崩（按钮交互不应让 app 崩溃）
async function hide(id: string) {
  try {
    await invoke('hide_session', { id });
    emit('hide', id);
  } catch (e) {
    console.error('hide failed', e);
  }
}
// invoke 失败时 console.error 记录，UI 不崩
async function unhide(id: string) {
  try {
    await invoke('unhide_session', { id });
    emit('unhide', id);
  } catch (e) {
    console.error('unhide failed', e);
  }
}
</script>
<template>
  <ul class="list">
    <li v-for="s in sessions" :key="s.id" :class="{ dead: !s.alive }">
      <span class="ico">{{ icon[s.status] }}</span>
      <span class="name">{{ s.name || s.project }}</span>
      <span class="proj">{{ s.project }}</span>
      <span class="ago">{{ ago(s.statusUpdatedAt) }}</span>
      <button v-if="hidden.includes(s.id)" class="hide-btn" @click="unhide(s.id)" title="恢复">+</button>
      <button v-else class="hide-btn" @click="hide(s.id)" title="隐藏">×</button>
    </li>
  </ul>
</template>
<style scoped>
.list { list-style: none; margin: 0; padding: 0; min-width: 380px; }
li { display: flex; gap: 8px; padding: 6px 10px; align-items: center; }
li.dead { opacity: 0.4; }
.proj, .ago { color: #888; font-size: 12px; }
.name { flex: 1; }
.hide-btn { background: none; border: none; color: #888; cursor: pointer; }
.hide-btn:hover { color: #333; }
</style>
