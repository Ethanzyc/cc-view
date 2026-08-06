<script setup lang="ts">
// 模式切换过渡层：窗口缩放动画期间显示（隐藏真实视图，避免内容在变化窗口里错位）。
// 纯展示，spinner 居中（窗口任意尺寸都居中）。App.vue transitioning 控制。
</script>

<template>
  <div class="transition-overlay" aria-hidden="true">
    <div class="spinner" />
  </div>
</template>

<style scoped>
.transition-overlay {
  display: flex; align-items: center; justify-content: center;
  min-height: 100vh;
}
.spinner {
  width: 18px; height: 18px;
  border: 2px solid rgba(255, 255, 255, 0.18);
  border-top-color: var(--color-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@media (prefers-reduced-motion: reduce) {
  .spinner { animation: none; border-color: var(--color-primary); }
}
@keyframes spin { to { transform: rotate(1turn); } }
</style>
