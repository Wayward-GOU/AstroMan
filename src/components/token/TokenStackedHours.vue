<script setup lang="ts">
import { computed } from 'vue'
import type { HourModel } from '@/utils/tokenUsageAgg'
import { fmtInt, fmtTokens } from '@/utils/tokenUsageFormat'

export type HourMetric = 'requests' | 'tokens'

const props = defineProps<{ data: HourModel; metric: HourMetric }>()

const HOURS = Array.from({ length: 24 }, (_, h) => h)

const series = computed(() => (props.metric === 'requests' ? props.data.requests : props.data.tokens))

/** 每小时的合计，用来定柱子的相对高度 */
const columnTotals = computed(() =>
  HOURS.map((h) => series.value.reduce((sum, row) => sum + (row[h] ?? 0), 0)),
)

const maxTotal = computed(() => Math.max(1, ...columnTotals.value))

const columns = computed(() =>
  HOURS.map((h) => {
    const total = columnTotals.value[h]
    const segments = props.data.models
      .map((model, i) => ({
        model,
        index: i,
        value: series.value[i]?.[h] ?? 0,
        tokens: props.data.tokens[i]?.[h] ?? 0,
        requests: props.data.requests[i]?.[h] ?? 0,
      }))
      .filter((s) => s.value > 0)
    return {
      hour: h,
      total,
      height: total > 0 ? Math.max(2, (total / maxTotal.value) * 100) : 0,
      segments,
    }
  }),
)

const legend = computed(() =>
  props.data.models.map((model, i) => ({ model, index: i })),
)

function fmtValue(value: number): string {
  return props.metric === 'requests' ? `${fmtInt(value)} 次` : fmtTokens(value)
}

function tooltipOf(col: (typeof columns.value)[number]): string {
  const head = `${String(col.hour).padStart(2, '0')}:00 · ${fmtValue(col.total)}`
  if (!col.segments.length) return head
  const lines = [...col.segments]
    .sort((a, b) => b.value - a.value)
    .map(
      (s) =>
        `${s.model}：${fmtValue(s.value)} · ${fmtTokens(s.tokens)} token · ${fmtInt(s.requests)} 次`,
    )
  return [head, ...lines].join('\n')
}
</script>

<template>
  <div class="stack">
    <div class="stack-plot">
      <div v-for="col in columns" :key="col.hour" class="col" :title="tooltipOf(col)">
        <div class="col-track">
          <div class="col-bar" :style="{ height: `${col.height}%` }">
            <span
              v-for="seg in col.segments"
              :key="`${col.hour}-${seg.model}`"
              class="seg"
              :class="`c-${seg.index}`"
              :style="{ flexGrow: seg.value }"
            />
          </div>
        </div>
        <span class="tick">{{ String(col.hour).padStart(2, '0') }}</span>
      </div>
    </div>

    <div v-if="legend.length" class="stack-legend">
      <span v-for="item in legend" :key="item.model" class="leg">
        <i :class="`c-${item.index}`" />
        <span class="leg-name" :title="item.model">{{ item.model }}</span>
      </span>
      <span class="peak">峰值 {{ fmtValue(maxTotal) }}</span>
    </div>
  </div>
</template>

<style scoped>
.stack {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.stack-plot {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 176px;
}

.col {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 3px;
}

.col-track {
  flex: 1;
  display: flex;
  align-items: flex-end;
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
}

.col-bar {
  width: 100%;
  display: flex;
  flex-direction: column-reverse;
  gap: 1px;
  transition: height var(--fm-dur-mid) var(--fm-ease);
}

.seg {
  display: block;
  width: 100%;
  min-height: 2px;
}

.tick {
  font-size: 9px;
  text-align: center;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.stack-legend {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  font-size: 10.5px;
  color: var(--fm-text-2);
}

.leg {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 132px;
}

.leg i {
  flex: none;
  width: 9px;
  height: 9px;
  border-radius: var(--fm-radius-sm);
}

.leg-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.peak {
  margin-left: auto;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.c-0 {
  background: var(--fm-chart-1);
}
.c-1 {
  background: var(--fm-chart-2);
}
.c-2 {
  background: var(--fm-chart-3);
}
.c-3 {
  background: var(--fm-chart-4);
}
.c-4 {
  background: var(--fm-chart-5);
}
.c-5 {
  background: var(--fm-chart-6);
}
.c-6 {
  background: var(--fm-chart-7);
}
.c-7 {
  background: var(--fm-chart-8);
}
</style>
