<script setup lang="ts">
// 单色 SVG 图标组件：按 status 着色 + 选形状，16x16 stroke 风格。
// 所有 path 用 stroke=currentColor + linecap/linejoin round，颜色由父级 style 注入。
import type { Status } from '../types';

const props = defineProps<{ status: Status }>();

// 状态色板（读 :root token，macOS system colors 风格）
const COLOR: Record<Status, string> = {
  working:         'var(--status-working-ink)',
  waitingForInput: 'var(--status-waiting)',
  waitingForReply: 'var(--status-reply)',
  needsPermission: 'var(--status-permission)',
  shell:           'var(--status-shell)',
  compacting:      'var(--status-compacting)',
};
</script>

<template>
  <svg
    :width="16" :height="16" viewBox="0 0 16 16"
    :style="{ color: COLOR[props.status] }"
    :class="{ 'status-icon--spinning': status === 'working' }"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="status-icon"
    aria-hidden="true"
  >
    <!-- spinner：约 90° 弧顺时针旋转（loading 语义，替代原闪电）。
         circle 默认从 3 点钟起笔 dash，rotate(-90) 移到 12 点；dasharray 9≈94° 显示段，gap 100 远大于周长保证只一段。 -->
    <circle v-if="status === 'working'" cx="8" cy="8" r="5.5"
      stroke-dasharray="9 100"
      transform="rotate(-90 8 8)" />

    <!-- 三点：WaitingForInput（轮到你打字）。圆润横向、视觉最安静——
         waitingForInput 是最高频的常态待操作，不该抢眼（"等输入"文字标签消歧）。 -->
    <g v-else-if="status === 'waitingForInput'">
      <circle cx="4" cy="8" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="12" cy="8" r="1.3" fill="currentColor" stroke="none" />
    </g>

    <!-- 问号：WaitingForReply（过程中提问）。去掉气泡外壳，纯符号更简洁；
         Lucide help-circle 问号缩放。黄色 + 行左边框已足够表达"需回答"。 -->
    <g v-else-if="status === 'waitingForReply'">
      <path d="M6.06 6 a2 2 0 0 1 3.89 0.67 c0 1.33 -2 2 -2 2" />
      <circle cx="8" cy="11.3" r="0.6" fill="currentColor" stroke="none" />
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

    <!-- 向内收敛箭头：Compacting（压缩上下文，向中心收敛语义）。静态，
         与 working spinner（弧形旋转）在形状+动效上完全区分。 -->
    <g v-else-if="status === 'compacting'">
      <path d="M8 2.5 V6 M6.2 4.3 L8 6 L9.8 4.3" />
      <path d="M13.5 8 H10 M11.7 6.2 L10 8 L11.7 9.8" />
      <path d="M8 13.5 V10 M6.2 11.7 L8 10 L9.8 11.7" />
      <path d="M2.5 8 H6 M4.3 6.2 L6 8 L4.3 9.8" />
    </g>
  </svg>
</template>

<style scoped>
.status-icon {
  flex-shrink: 0;
  display: block;
}
/* signature：working 旋转（loading），其余静止；reduced-motion 由 App.vue 全局降级 */
.status-icon.status-icon--spinning {
  transform-origin: center;
  animation: spin 900ms linear infinite;
}
@keyframes spin {
  to { transform: rotate(1turn); }
}
</style>
