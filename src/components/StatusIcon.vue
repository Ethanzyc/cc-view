<script setup lang="ts">
// 单色 SVG 图标组件：按 status 着色 + 选形状，16x16 stroke 风格。
// 所有 path 用 stroke=currentColor + linecap/linejoin round，颜色由父级 style 注入。
import type { Status } from '../types';

const props = defineProps<{ status: Status }>();

// 状态色板（macOS system colors 风格）
const COLOR: Record<Status, string> = {
  working: '#30D158',        // 绿
  waitingForInput: '#0A84FF', // 蓝
  needsPermission: '#FF9F0A', // 橙
  shell: '#BF5AF2',          // 紫
  compacting: '#64D2FF',     // 青
};
</script>

<template>
  <svg
    :width="16" :height="16" viewBox="0 0 16 16"
    :style="{ color: COLOR[props.status] }"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="status-icon"
    aria-hidden="true"
  >
    <!-- 闪电 bolt: Working -->
    <path v-if="status === 'working'" d="M9 1.5 L3.5 9 L7.5 9 L7 14.5 L12.5 7 L8.5 7 Z" />

    <!-- 对话气泡 + 三点: WaitingForInput -->
    <g v-else-if="status === 'waitingForInput'">
      <path d="M3.5 3 H12.5 A1.5 1.5 0 0 1 14 4.5 V9 A1.5 1.5 0 0 1 12.5 10.5 H7.5 L5 13 V10.5 H3.5 A1.5 1.5 0 0 1 2 9 V4.5 A1.5 1.5 0 0 1 3.5 3 Z" />
      <circle cx="5.3" cy="6.75" r="0.55" fill="currentColor" stroke="none" />
      <circle cx="8" cy="6.75" r="0.55" fill="currentColor" stroke="none" />
      <circle cx="10.7" cy="6.75" r="0.55" fill="currentColor" stroke="none" />
    </g>

    <!-- 挂锁: NeedsPermission -->
    <g v-else-if="status === 'needsPermission'">
      <rect x="2.75" y="7" width="10.5" height="6.5" rx="1.25" />
      <path d="M4.75 7 V4.75 A3.25 3.25 0 0 1 11.25 4.75 V7" />
    </g>

    <!-- 终端框 >_: Shell -->
    <g v-else-if="status === 'shell'">
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M5 6.5 L7 8.5 L5 10.5" />
      <path d="M9 10.5 H11.5" />
    </g>

    <!-- 向内收敛箭头: Compacting -->
    <g v-else-if="status === 'compacting'">
      <!-- 上箭头朝下 -->
      <path d="M8 2.5 V6 M6.2 4.3 L8 6 L9.8 4.3" />
      <!-- 右箭头朝左 -->
      <path d="M13.5 8 H10 M11.7 6.2 L10 8 L11.7 9.8" />
      <!-- 下箭头朝上 -->
      <path d="M8 13.5 V10 M6.2 11.7 L8 10 L9.8 11.7" />
      <!-- 左箭头朝右 -->
      <path d="M2.5 8 H6 M4.3 6.2 L6 8 L4.3 9.8" />
    </g>
  </svg>
</template>

<style scoped>
.status-icon {
  flex-shrink: 0;
  display: block;
}
</style>
