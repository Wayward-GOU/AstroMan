<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    points: Array<{ ms: number; pct: number }>
    height?: number
  }>(),
  { height: 220 },
)

const W = 720
const padL = 52
const padR = 18
const padT = 16
const padB = 28

const valid = computed(() =>
  props.points.filter((item) => Number.isFinite(item.ms) && Number.isFinite(item.pct)),
)

const xMax = computed(() => {
  const raw = Math.max(1, ...valid.value.map((item) => item.ms))
  const magnitude = Math.pow(10, Math.floor(Math.log10(raw)))
  return Math.ceil((raw * 1.02) / magnitude) * magnitude
})

function px(ms: number) {
  return padL + (Math.min(ms, xMax.value) / xMax.value) * (W - padL - padR)
}

function py(pct: number) {
  const inner = props.height - padT - padB
  return padT + inner - (Math.min(100, Math.max(0, pct)) / 100) * inner
}

function fmtAxis(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10000 ? 0 : 1)}k`
  if (value >= 10) return value.toFixed(0)
  if (value >= 1) return value.toFixed(1)
  return value.toFixed(2)
}

const line = computed(() =>
  valid.value.map((item) => `${px(item.ms).toFixed(1)},${py(item.pct).toFixed(1)}`).join(' '),
)

const area = computed(() => {
  const list = valid.value
  if (list.length < 2) return ''
  const baseline = (props.height - padB).toFixed(1)
  return `M${px(list[0].ms).toFixed(1)},${baseline} L${line.value.split(' ').join(' L')} L${px(
    list[list.length - 1].ms,
  ).toFixed(1)},${baseline} Z`
})

const gridRows = computed(() =>
  [100, 75, 50, 25, 0].map((pct) => ({ pct, y: py(pct) })),
)

const xTicks = computed(() =>
  Array.from({ length: 5 }, (_, index) => {
    const value = (xMax.value / 4) * index
    return { value, x: px(value) }
  }),
)

function msAt(target: number) {
  const list = valid.value
  for (let index = 1; index < list.length; index += 1) {
    const prev = list[index - 1]
    const current = list[index]
    if (current.pct >= target) {
      const ratio = current.pct === prev.pct ? 0 : (target - prev.pct) / (current.pct - prev.pct)
      return prev.ms + (current.ms - prev.ms) * ratio
    }
  }
  return list[list.length - 1]?.ms ?? 0
}

const markers = computed(() =>
  [
    { pct: 50, label: 'P50', color: 'var(--fm-purple)' },
    { pct: 95, label: 'P95', color: 'var(--fm-warning)' },
    { pct: 99, label: 'P99', color: 'var(--fm-danger)' },
  ].map((item) => {
    const value = msAt(item.pct)
    const x = px(value)
    return {
      ...item,
      value,
      x,
      y: py(item.pct),
      text: `${item.label} ${value.toFixed(0)}ms`,
    }
  }),
)
</script>

<template>
  <div class="cdf">
    <div v-if="valid.length >= 2" class="plot">
      <svg :viewBox="`0 0 ${W} ${height}`" preserveAspectRatio="xMidYMid meet">
        <g v-for="row in gridRows" :key="row.pct">
          <line :x1="padL" :x2="W - padR" :y1="row.y" :y2="row.y" class="grid-line" />
          <text :x="padL - 10" :y="row.y + 3.5" class="axis-text" text-anchor="end">
            {{ row.pct }}%
          </text>
        </g>

        <g v-for="tick in xTicks" :key="tick.value">
          <text :x="tick.x" :y="height - 8" class="axis-text" text-anchor="middle">
            {{ fmtAxis(tick.value) }}
          </text>
        </g>

        <path :d="area" class="area" />
        <polyline :points="line" class="line" />

        <g v-for="item in markers" :key="item.label">
          <line
            :x1="item.x"
            :x2="item.x"
            :y1="padT"
            :y2="height - padB"
            :stroke="item.color"
            stroke-width="1"
            stroke-dasharray="3 3"
            opacity=".55"
          />
          <circle :cx="item.x" :cy="item.y" r="2.6" :fill="item.color" />
          <text :x="item.x + 5" :y="item.y - 5" class="marker-text" :fill="item.color">
            {{ item.text }}
          </text>
        </g>
      </svg>
    </div>
    <div v-else class="chart-empty">没有足够的直方图数据</div>
  </div>
</template>

<style scoped>
.cdf {
  width: 100%;
}
.plot svg {
  width: 100%;
  height: auto;
  display: block;
}
.grid-line {
  stroke: var(--fm-border);
  stroke-width: 1;
  stroke-dasharray: 3 4;
}
.axis-text {
  fill: var(--fm-muted);
  font-size: 10px;
  font-family: 'Inter', sans-serif;
}
.marker-text {
  font-size: 10px;
  font-weight: 600;
}
.area {
  fill: var(--fm-primary);
  fill-opacity: 0.08;
  stroke: none;
}
.line {
  fill: none;
  stroke: var(--fm-primary);
  stroke-width: 2;
  stroke-linejoin: round;
  stroke-linecap: round;
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
