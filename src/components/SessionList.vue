<script setup lang="ts">
import type { Session, Status } from '../types';
defineProps<{ sessions: Session[] }>();
const icon: Record<Status, string> = {
  working: '⚡', waitingForInput: '💤', needsPermission: '⏳', shell: '🖥️',
};
function ago(ts: number) {
  const s = Math.floor((Date.now() - ts) / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s/60)}m`;
}
</script>
<template>
  <ul class="list">
    <li v-for="s in sessions" :key="s.id" :class="{ dead: !s.alive }">
      <span class="ico">{{ icon[s.status] }}</span>
      <span class="name">{{ s.name || s.project }}</span>
      <span class="proj">{{ s.project }}</span>
      <span class="ago">{{ ago(s.statusUpdatedAt) }}</span>
    </li>
  </ul>
</template>
<style scoped>
.list { list-style: none; margin: 0; padding: 0; min-width: 360px; }
li { display: flex; gap: 8px; padding: 6px 10px; align-items: center; }
li.dead { opacity: 0.4; }
.proj, .ago { color: #888; font-size: 12px; }
.name { flex: 1; }
</style>
