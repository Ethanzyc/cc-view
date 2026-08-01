<script setup lang="ts">
// 每行渲染一个会话；右侧隐藏按钮调 invoke hide_session，
// 成功后 emit('hide') 让父组件 App 刷新 hidden 集合 → visible computed 更新。
import type { Session, Status } from '../types';
import { invoke } from '@tauri-apps/api/core';

defineProps<{ sessions: Session[]; showHidden?: boolean }>();
const emit = defineEmits<{ (e: 'hide', id: string): void }>();

const icon: Record<Status, string> = {
  working: '⚡', waitingForInput: '💤', needsPermission: '⏳', shell: '🖥️',
};
function ago(ts: number) {
  const s = Math.floor((Date.now() - ts) / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m`;
}
// fail fast：invoke 抛错时 console.error 暴露问题，不吞异常；不 emit 也就不刷新。
async function hide(id: string) {
  try {
    await invoke('hide_session', { id });
    emit('hide', id);
  } catch (err) {
    console.error('hide_session failed:', err);
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
      <button class="hide-btn" @click="hide(s.id)" title="隐藏">×</button>
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
