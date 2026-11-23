<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    items: Array<{ label: string; value: number; color?: string }>
    centerText?: string
    size?: number
  }>(),
  { centerText: '', size: 180 },
)

const COLORS = [
  '#14655a',
  '#37628f',
  '#9c6414',
  '#a23333',
  '#6b4c9a',
  '#17774a',
  '#8a5a2b',
  '#5b6b73',
]

const rows = computed(() =>
  props.items.filter((item) => Number.isFinite(item.value) && item.value > 0),
)
const total = computed(() => rows.value.reduce((sum, item) => sum + item.value, 0))

const R = 68
const CX = props.size / 2
const CY = props.size / 2
const CIRC = 2 * Math.PI * R

const segments = computed(() => {
  let offset = 0
  return rows.value.map((item, index) => {
    const length = (item.value / Math.max(1, total.value)) * CIRC
    const segment = {
      ...item,
      color: item.color || COLORS[index % COLORS.length],
      dash: `${length.toFixed(2)} ${(CIRC - length).toFixed(2)}`,
      offset: (-offset).toFixed(2),
      pct: (item.value / Math.max(1, total.value)) * 100,
    }
    offset += length
    return segment
  })
})

function format(value: number) {
  return value.toLocaleString('zh-CN', { maximumFractionDigits: 1 })
}
</script>

<template>
  <div v-if="rows.length" class="pie">
    <svg :viewBox="`0 0 ${size} ${size}`" :width="size" :height="size">
      <circle :cx="CX" :cy="CY" :r="R" fill="none" stroke="var(--fm-surface-alt)" stroke-width="26" />
      <circle
        v-for="item in segments"
        :key="item.label"
        :cx="CX"
        :cy="CY"
        :r="R"
        fill="none"
        :stroke="item.color"
        stroke-width="26"
        :stroke-dasharray="item.dash"
        :stroke-dashoffset="item.offset"
        :transform="`rotate(-90 ${CX} ${CY})`"
      />
      <text :x="CX" :y="CY - 2" text-anchor="middle" class="center-value">
        {{ centerText || format(total) }}
      </text>
      <text :x="CX" :y="CY + 14" text-anchor="middle" class="center-label">合计</text>
    </svg>
    <div class="legend">
      <div v-for="item in segments" :key="item.label" class="legend-row">
        <span class="dot" :style="{ background: item.color }" />
        <span class="label" :title="item.label">{{ item.label }}</span>
        <span class="value mono">{{ format(item.value) }}</span>
        <span class="pct mono">{{ item.pct.toFixed(1) }}%</span>
      </div>
    </div>
  </div>
  <div v-else class="chart-empty">暂无可分析数据</div>
</template>

<style scoped>
.pie {
  display: flex;
  align-items: center;
  gap: 18px;
  flex-wrap: wrap;
}
svg {
  flex: 0 0 auto;
}
.center-value {
  fill: var(--fm-text);
  font-size: 15px;
  font-weight: 700;
}
.center-label {
  fill: var(--fm-muted);
  font-size: 10px;
}
.legend {
  flex: 1 1 180px;
  min-width: 160px;
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.legend-row {
  display: grid;
  grid-template-columns: 8px 1fr auto auto;
  align-items: center;
  gap: 7px;
  font-size: 12px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 2px;
}
.label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-2);
}
.value {
  color: var(--fm-text);
}
.pct {
  min-width: 42px;
  text-align: right;
  color: var(--fm-muted);
}
.chart-empty {
  padding: 42px 0;
  text-align: center;
  color: var(--fm-muted);
  font-size: 12px;
  background: var(--fm-surface-alt);
  border-radius: 10px;
}
</style>
