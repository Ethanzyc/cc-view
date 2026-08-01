<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import SessionList from './components/SessionList.vue';
import type { Session } from './types';
const sessions = ref<Session[]>([]);
onMounted(async () => {
  await listen<Session[]>('sessions', e => { sessions.value = e.payload; });
});
</script>
<template>
  <div class="app">
    <h3>Claude Code 会话</h3>
    <SessionList :sessions="sessions" />
  </div>
</template>
<style>
body { margin: 0; font-family: -apple-system, sans-serif; }
.app { padding: 8px; }
</style>
