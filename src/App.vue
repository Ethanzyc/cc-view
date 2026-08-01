<script setup lang="ts">
// App 维护两份状态：all（后端 emit 的全量 merged）+ hidden（list_hidden 拉到的隐藏 id 集）。
// visible computed 按 showHidden toggle 决定是否过滤。SessionList hide 成功后 emit 'hide'
// 触发 refreshHidden 重新拉列表，让 visible 立即反映新状态（无需等 3s 轮询）。
import { ref, onMounted, computed } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import SessionList from './components/SessionList.vue';
import type { Session } from './types';

const all = ref<Session[]>([]);
const hidden = ref<string[]>([]);
const showHidden = ref(false);
const visible = computed(() =>
  showHidden.value ? all.value : all.value.filter(s => !hidden.value.includes(s.id)),
);

async function refreshHidden() {
  // fail fast：invoke 失败抛出由 onMounted/@click 调用者兜底；这里不吞异常
  hidden.value = await invoke<string[]>('list_hidden');
}

onMounted(async () => {
  await refreshHidden();
  await listen<Session[]>('sessions', e => { all.value = e.payload; });
});
</script>
<template>
  <div class="app">
    <h3>Claude Code 会话
      <button @click="refreshHidden" title="刷新隐藏列表">↻</button>
      <label class="toggle"><input type="checkbox" v-model="showHidden" /> 显示已隐藏</label>
    </h3>
    <SessionList :sessions="visible" :hidden="hidden" @hide="refreshHidden" @unhide="refreshHidden" />
  </div>
</template>
<style>
body { margin: 0; font-family: -apple-system, sans-serif; }
.app { padding: 8px; }
.toggle { font-size: 12px; font-weight: normal; margin-left: 8px; color: #555; }
</style>
