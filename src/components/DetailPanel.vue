<script setup lang="ts">
// 会话 token 详情：汇总 + 按回合。PanelView 内部子状态切入，不动全局 mode/窗口。
import type { SessionDetail } from '../types';
import { fmtTok, agoF } from '../utils/session';

defineProps<{ detail: SessionDetail; name: string }>();
defineEmits<{ back: [] }>();
</script>

<template>
  <div class="detail">
    <div class="detail-bar" data-tauri-drag-region="deep">
      <button
        class="back-btn"
        title="返回列表"
        aria-label="返回列表"
        data-tauri-drag-region="false"
        @click="$emit('back')"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M19 12H5" /><path d="M12 19l-7-7 7-7" />
        </svg>
      </button>
      <span class="detail-title">{{ name }}</span>
    </div>
    <div class="divider" />
    <div class="detail-scroll">
      <div class="summary">
        <div class="sum-row">
          <span class="sum"><b>{{ fmtTok(detail.tokensIn) }}</b><i>输入</i></span>
          <span class="sum"><b>{{ fmtTok(detail.tokensOut) }}</b><i>输出</i></span>
          <span class="sum"><b>{{ fmtTok(detail.cacheRead) }}</b><i>缓存命中</i></span>
        </div>
        <div class="sum-row sub">
          <span>{{ detail.model || '—' }}</span>
          <span>{{ detail.turnCount }} 回合</span>
          <span>{{ detail.toolCalls }} 工具</span>
          <span v-if="detail.webSearches || detail.webFetches">{{ detail.webSearches }} 搜 / {{ detail.webFetches }} 抓</span>
        </div>
      </div>
      <div class="divider" />
      <div class="turns-head">按回合</div>
      <ul class="turns">
        <li v-for="t in detail.turns" :key="t.idx" class="turn">
          <span class="t-idx">#{{ t.idx }}</span>
          <span class="t-prompt">{{ t.prompt || '—' }}</span>
          <span class="t-tok">{{ fmtTok(t.tokensIn) }}<span class="arr">↑</span>{{ fmtTok(t.tokensOut) }}<span class="arr">↓</span></span>
          <span v-if="t.toolCalls" class="t-tools">🔧{{ t.toolCalls }}</span>
          <span class="t-ago">{{ t.ts ? agoF(new Date(t.ts).getTime()) : '' }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.detail {
  background: var(--color-bg-overlay);
  color: var(--color-fg);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-overlay);
  overflow: hidden;
}
.detail-bar {
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: var(--pad-y) var(--pad-x);
}
.back-btn {
  background: none;
  border: none;
  color: var(--color-tertiary);
  cursor: pointer;
  padding: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  transition: color var(--motion-duration) var(--motion-easing),
              background var(--motion-duration) var(--motion-easing);
}
.back-btn:hover { color: var(--color-fg); background: var(--color-hover); }
.back-btn:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 1px; }
.detail-title {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.divider { height: 1px; background: var(--color-border); margin: 0 var(--gap); }
.detail-scroll { flex: 1; overflow-y: auto; padding: var(--gap) 0 var(--pad-y); }
.detail-scroll::-webkit-scrollbar { width: 6px; }
.detail-scroll::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }
.summary { padding: 0 var(--pad-x); }
.sum-row { display: flex; flex-wrap: wrap; gap: var(--gap) var(--pad-x); }
.sum-row.sub {
  margin-top: 6px;
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-muted);
}
.sum { display: inline-flex; align-items: baseline; gap: 4px; }
.sum b {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-utility);
  font-variant-numeric: tabular-nums;
  color: var(--color-fg);
}
.sum i {
  font-style: normal;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-tertiary);
}
.turns-head {
  padding: 10px var(--pad-x) 4px;
  font: 600 var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-muted);
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.turns { list-style: none; margin: 0; padding: 0; }
.turn {
  display: flex;
  align-items: center;
  gap: var(--gap);
  padding: 4px var(--pad-x);
  font: var(--fw-utility) var(--fs-utility)/var(--lh-utility) var(--font-body);
}
.turn:hover { background: var(--color-hover); }
.t-idx { color: var(--color-tertiary); flex-shrink: 0; font-variant-numeric: tabular-nums; width: 28px; }
.t-prompt {
  flex: 1; min-width: 0;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  color: var(--color-fg);
}
.t-tok {
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
}
.t-tok .arr { opacity: 0.6; margin: 0 1px; }
.t-tools { flex-shrink: 0; color: var(--color-tertiary); }
.t-ago { flex-shrink: 0; color: var(--color-tertiary); font-variant-numeric: tabular-nums; }
</style>
