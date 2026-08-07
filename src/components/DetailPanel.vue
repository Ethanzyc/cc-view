<script setup lang="ts">
// 会话 token 详情：上下文 hero + sparkline + 消耗 + 按回合。
// PanelView 内部子状态切入，不动全局 mode/窗口。
import { computed } from 'vue';
import type { SessionDetail, TurnStat } from '../types';
import { fmtTok, agoF } from '../utils/session';

const props = defineProps<{ detail: SessionDetail; name: string }>();
defineEmits<{ back: [] }>();

// sparkline：turns 的 ctx 归一化到 200×56 viewBox（纯相对增长，不需硬编码上限）。
// 空数据返回空串，模板 v-if 不渲染 svg。
const spark = computed(() => {
  const ctxs = props.detail.turns.map(t => t.ctx);
  if (ctxs.length === 0) return { line: '', fill: '', cx: 0, cy: 0 };
  const max = Math.max(...ctxs);
  const min = Math.min(...ctxs);
  const range = max - min || 1;
  const n = ctxs.length;
  const pts = ctxs.map((c, i) => {
    const x = n === 1 ? 200 : (i / (n - 1)) * 200;
    const y = 52 - ((c - min) / range) * 49; // 顶部留 3px
    return [x, y] as const;
  });
  const line = pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  const [lx, ly] = pts[pts.length - 1];
  const fill = `${line} ${lx.toFixed(1)},56 0,56`;
  return { line, fill, cx: +lx.toFixed(1), cy: +ly.toFixed(1) };
});

// 回合进度条：该回合 in+out 占所有回合最大值的比例（背景浅色条，直观对比回合大小）
const maxTurnTok = computed(() =>
  Math.max(1, ...props.detail.turns.map(t => t.tokensIn + t.tokensOut)),
);
function barPercent(t: TurnStat): number {
  return Math.round(((t.tokensIn + t.tokensOut) / maxTurnTok.value) * 100);
}
</script>

<template>
  <div class="detail">
    <div class="bar" data-tauri-drag-region="deep">
      <button
        class="back-btn"
        title="返回列表"
        aria-label="返回列表"
        data-tauri-drag-region="false"
        @click="$emit('back')"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5" /><path d="M12 19l-7-7 7-7" /></svg>
      </button>
      <span class="title">{{ name }}</span>
      <span v-if="detail.model" class="model">{{ detail.model }}</span>
    </div>
    <div class="divider" />

    <!-- Hero：当前上下文 + sparkline -->
    <div class="hero">
      <div class="hero-label">当前上下文</div>
      <div class="hero-big">{{ fmtTok(detail.contextCurrent) }}</div>
      <svg v-if="spark.line" class="spark" viewBox="0 0 200 56" preserveAspectRatio="none" aria-hidden="true">
        <polygon :points="spark.fill" fill="var(--color-primary)" opacity="0.08" />
        <polyline :points="spark.line" fill="none" stroke="var(--color-primary)" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" />
        <circle :cx="spark.cx" :cy="spark.cy" r="2.5" fill="var(--color-primary)" />
      </svg>
      <div class="hero-sub">
        <span>峰值 <b>{{ fmtTok(detail.contextPeak) }}</b></span>
        <span>压缩 <b>{{ detail.compactCount }}</b> 次</span>
        <span>{{ detail.turnCount }} 回合</span>
      </div>
    </div>

    <div class="section-label">消耗</div>
    <div class="cost-grid">
      <div class="cost-cell"><span class="v">{{ fmtTok(detail.tokensIn) }}</span><span class="k">输入</span></div>
      <div class="cost-cell"><span class="v">{{ fmtTok(detail.tokensOut) }}</span><span class="k">输出</span></div>
      <div class="cost-cell"><span class="v">{{ fmtTok(detail.cacheRead) }}</span><span class="k">缓存命中</span></div>
    </div>

    <div class="divider" />
    <div class="section-label">
      按回合 · {{ detail.toolCalls }} 工具
      <span v-if="detail.webSearches || detail.webFetches"> · {{ detail.webSearches }} 搜 / {{ detail.webFetches }} 抓</span>
    </div>
    <div class="turns">
      <div v-for="t in detail.turns" :key="t.idx" class="turn" :style="{ '--bar': barPercent(t) + '%' }">
        <span class="t-idx">#{{ t.idx }}</span>
        <span class="t-prompt">{{ t.prompt || '—' }}</span>
        <span class="t-tok">{{ fmtTok(t.tokensIn) }}<span class="arr">↑</span>{{ fmtTok(t.tokensOut) }}<span class="arr">↓</span></span>
        <span class="t-ctx">{{ fmtTok(t.ctx) }}</span>
        <span class="t-ago">{{ t.ts ? agoF(new Date(t.ts).getTime()) : '' }}</span>
      </div>
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

/* 顶栏 */
.bar {
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
.title {
  flex: 1; min-width: 0;
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-body);
  color: var(--color-fg);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.model {
  font: var(--fw-utility) var(--fs-caption)/var(--lh-caption) var(--font-utility);
  color: var(--color-muted);
  background: var(--color-hover);
  padding: 3px 7px;
  border-radius: 5px;
  flex-shrink: 0;
}

.divider { height: 1px; background: var(--color-border); margin: 0 var(--gap); }

/* Hero：上下文大数字 + sparkline */
.hero {
  display: grid;
  grid-template-columns: auto 1fr;
  grid-template-rows: auto auto auto;
  column-gap: 16px; row-gap: 4px;
  padding: 18px var(--pad-x) 16px;
  align-items: end;
}
.hero-label {
  grid-column: 1; grid-row: 1;
  align-self: end;
  padding-bottom: 4px;
  font: var(--fw-caption) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-tertiary);
}
.hero-big {
  grid-column: 1; grid-row: 2;
  font: 700 30px/1 var(--font-utility);
  color: var(--color-fg);
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
}
.spark {
  grid-column: 2; grid-row: 1 / span 2;
  width: 100%; height: 56px;
  align-self: stretch;
}
.hero-sub {
  grid-column: 1 / -1; grid-row: 3;
  margin-top: 6px;
  display: flex; gap: 14px;
  font: var(--fw-utility) var(--fs-caption)/var(--lh-caption) var(--font-body);
  color: var(--color-muted);
}
.hero-sub b { color: var(--color-fg); font-weight: 600; font-variant-numeric: tabular-nums; }

/* 区块标题 */
.section-label {
  padding: 12px var(--pad-x) 5px;
  font: 600 var(--fs-utility)/var(--lh-utility) var(--font-utility);
  color: var(--color-tertiary);
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

/* 消耗三格 */
.cost-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--gap-xs);
  padding: 0 var(--pad-x) 8px;
}
.cost-cell {
  display: flex; flex-direction: column; gap: 2px;
  padding: 8px 10px;
  background: var(--color-hover);
  border-radius: 7px;
}
.cost-cell .v {
  font: var(--fw-body) var(--fs-body)/var(--lh-body) var(--font-utility);
  font-variant-numeric: tabular-nums;
  color: var(--color-fg);
}
.cost-cell .k {
  font: var(--fw-caption) var(--fs-utility)/var(--lh-utility) var(--font-body);
  color: var(--color-tertiary);
}

/* 回合列表 */
.turns { padding-bottom: var(--pad-y); }
.turn {
  position: relative;
  display: grid;
  grid-template-columns: 28px 1fr auto auto auto;
  align-items: center;
  gap: var(--gap);
  padding: 5px var(--pad-x);
  font: var(--fw-utility) var(--fs-caption)/var(--lh-caption) var(--font-body);
}
/* 进度条背景：宽度 = 该回合 in+out 占峰值比例，浅 primary 填充（深浅背景自适应） */
.turn::before {
  content: '';
  position: absolute;
  inset: 0 auto 0 0;
  width: var(--bar, 0%);
  background: color-mix(in srgb, var(--color-primary) 9%, transparent);
  z-index: 0;
}
.turn > * { position: relative; z-index: 1; }
.turn:hover { background: var(--color-hover); }
.t-idx {
  color: var(--color-tertiary);
  font-variant-numeric: tabular-nums;
  font-family: var(--font-utility);
}
.t-prompt {
  min-width: 0;
  color: var(--color-fg);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.t-tok {
  font-family: var(--font-utility);
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
}
.t-tok .arr { color: var(--color-tertiary); margin: 0 1px; }
.t-ctx {
  font-family: var(--font-utility);
  font-weight: 600;
  color: var(--color-primary);
  font-variant-numeric: tabular-nums;
  min-width: 34px; text-align: right;
}
.t-ago {
  font-family: var(--font-utility);
  color: var(--color-tertiary);
  font-variant-numeric: tabular-nums;
  min-width: 20px; text-align: right;
}
</style>
