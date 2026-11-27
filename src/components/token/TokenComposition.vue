<script setup lang="ts">
/**
 * Token 构成：一次调用里「新输入 / 缓存读 / 缓存写 / 输出」各占多少。
 * 用一条 100% 堆叠条 + 四项明细，比堆叠柱状更好读，也不占高度。
 */
import { computed } from 'vue'
import type { Composition } from '@/utils/tokenUsageAgg'
import { fmtInt, fmtPct, fmtTokens } from '@/utils/tokenUsageFormat'

const props = defineProps<{ composition: Composition }>()

const rows = computed(() => {
  const c = props.composition
  const total = c.input + c.cacheRead + c.cacheWrite + c.output
  const base = total > 0 ? total : 1
  return [
    {
      key: 'input',
      name: '新输入',
      note: '未命中缓存',
      value: c.input,
      share: (c.input / base) * 100,
      cls: 'c2',
    },
    {
      key: 'cacheRead',
      name: '缓存读',
      note: '命中前缀，按低价计费',
      value: c.cacheRead,
      share: (c.cacheRead / base) * 100,
      cls: 'c1',
    },
    {
      key: 'cacheWrite',
      name: '缓存写',
      note: '写入缓存，按 1.25 倍计费',
      value: c.cacheWrite,
      share: (c.cacheWrite / base) * 100,
      cls: 'c5',
    },
    {
      key: 'output',
      name: '输出',
      note: '模型生成的 token',
      value: c.output,
      share: (c.output / base) * 100,
      cls: 'c3',
    },
  ]
})

const total = computed(
  () =>
    props.composition.input +
    props.composition.cacheRead +
    props.composition.cacheWrite +
    props.composition.output,
)

const cacheHit = computed(() => {
  const inputAll = props.composition.input + props.composition.cacheRead
  return inputAll > 0 ? props.composition.cacheRead / inputAll : 0
})
/** 占比不到 0.1% 时不显示成刺眼的 0.0% */
function shareText(share: number): string {
  if (share > 0 && share < 0.1) return '<0.1%'
  return `${share.toFixed(1)}%`
}
</script>

<template>
  <div class="comp">
    <div class="comp-bar" role="img" :aria-label="`Token 构成，合计 ${fmtTokens(total)}`">
      <span
        v-for="row in rows"
        :key="row.key"
        class="comp-seg"
        :class="row.cls"
        :style="{ width: `${row.share}%` }"
        :title="`${row.name}：${fmtTokens(row.value)}（${row.share.toFixed(1)}%）`"
      />
      <span v-if="total === 0" class="comp-seg empty" />
    </div>

    <div class="comp-legend">
      <div v-for="row in rows" :key="row.key" class="comp-item">
        <i class="dot" :class="row.cls" />
        <span class="name">{{ row.name }}</span>
        <span class="value">{{ fmtTokens(row.value) }}</span>
        <span class="share">{{ shareText(row.share) }}</span>
        <em class="note">{{ row.note }}</em>
      </div>
    </div>

    <p class="comp-foot">
      缓存命中率 <b>{{ fmtPct(cacheHit * 100) }}</b>
      <span v-if="composition.reasoning > 0">
        · 其中推理 token {{ fmtInt(composition.reasoning) }}（已含在输出内）
      </span>
      <span class="total">合计 {{ fmtTokens(total) }}</span>
    </p>
  </div>
</template>

<style scoped>
.comp {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.comp-bar {
  display: flex;
  height: 22px;
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.comp-seg {
  height: 100%;
  transition: width var(--fm-dur-mid) var(--fm-ease);
}

.comp-seg.empty {
  width: 100%;
  background: var(--fm-surface-alt);
}

.c1 { background: var(--fm-chart-1); }
.c2 { background: var(--fm-chart-2); }
.c3 { background: var(--fm-chart-3); }
.c4 { background: var(--fm-chart-4); }
.c5 { background: var(--fm-chart-5); }

.comp-legend {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px 16px;
}

.comp-item {
  display: grid;
  grid-template-columns: 7px auto 1fr auto;
  grid-template-areas:
    'dot name value share'
    '. note note note';
  align-items: center;
  gap: 2px 7px;
  font-size: 10.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.comp-item .dot {
  grid-area: dot;
  width: 7px;
  height: 7px;
  border-radius: var(--fm-radius-pill);
}

.comp-item .name {
  grid-area: name;
  color: var(--fm-text);
}

.comp-item .value {
  grid-area: value;
  text-align: right;
}

.comp-item .share {
  grid-area: share;
  min-width: 42px;
  text-align: right;
  color: var(--fm-muted);
}

.comp-item .note {
  grid-area: note;
  font-style: normal;
  font-size: 9.5px;
  color: var(--fm-muted);
}

.comp-foot {
  margin: 0;
  padding-top: 8px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.comp-foot b {
  color: var(--fm-text-2);
  font-weight: 600;
}

.comp-foot .total {
  float: right;
}
</style>
