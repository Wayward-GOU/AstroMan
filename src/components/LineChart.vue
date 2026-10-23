<script setup lang="ts">
import { computed } from 'vue'

export interface ChartSeries {
  name: string
  color: string
  values: number[]
}

const props = withDefaults(
  defineProps<{
    series: ChartSeries[]
    labels?: string[]
    unit?: string
    area?: boolean
    height?: number
  }>(),
  { labels: () => [], unit: '', area: true, height: 230 },
)

const W = 720
const padL = 52
const padR = 18
const padT = 16
const padB = 26

const uid = `fm-${Math.random().toString(36).slice(2, 9)}`

const max = computed(() => {
  const values = props.series.flatMap((s) => s.values)
  const raw = Math.max(0, ...values)
  if (raw <= 0) return 1
  const magnitude = Math.pow(10, Math.floor(Math.log10(raw)))
  return Math.ceil(raw / magnitude) * magnitude
})

const count = computed(() => Math.max(...props.series.map((s) => s.values.length), 0))

function px(index: number) {
  const inner = W - padL - padR
  if (count.value <= 1) return padL + inner / 2
  return padL + (index / (count.value - 1)) * inner
}

function py(value: number) {
  const inner = props.height - padT - padB
  const ratio = Math.min(1, Math.max(0, value / max.value))
  return padT + inner - ratio * inner
}

const lines = computed(() =>
  props.series.map((s) => {
    const points = s.values.map((v, i) => `${px(i).toFixed(1)},${py(v).toFixed(1)}`)
    const line = points.length ? `M${points.join(' L')}` : ''
    const first = px(0)
    const last = px(Math.max(0, s.values.length - 1))
    const baseline = props.height - padB
    const area = points.length
      ? `${line} L${last.toFixed(1)},${baseline} L${first.toFixed(1)},${baseline} Z`
      : ''
    return { ...s, line, area, lastValue: s.values[s.values.length - 1] ?? 0 }
  }),
)

function fmtAxis(v: number) {
  if (v >= 1000) return `${(v / 1000).toFixed(v >= 10000 ? 0 : 1)}k`
  if (v >= 10) return v.toFixed(0)
  if (v >= 1) return v.toFixed(1)
  return v.toFixed(2)
}

function fmtValue(v: number | undefined) {
  if (v === undefined) return '-'
  if (v >= 1000) return v.toFixed(0)
  return v >= 10 ? v.toFixed(1) : v.toFixed(2)
}

const gridRows = computed(() => [
  { value: max.value, y: py(max.value) },
  { value: max.value / 2, y: py(max.value / 2) },
  { value: 0, y: py(0) },
])

const xTicks = computed(() => {
  const n = count.value
  if (n <= 1) return [{ x: px(0), label: props.labels[0] ?? '0s' }]
  const wanted = Math.min(6, n)
  const ticks: Array<{ x: number; label: string }> = []
  for (let i = 0; i < wanted; i++) {
    const index = Math.round((i / (wanted - 1)) * (n - 1))
    if (ticks.some((t) => t.label === (props.labels[index] ?? `${index}s`))) continue
    ticks.push({ x: px(index), label: props.labels[index] ?? `${index}s` })
  }
  return ticks
})

const hasData = computed(() => props.series.some((s) => s.values.length > 0))
</script>

<template>
  <div class="chart">
    <div class="legend">
      <div v-for="s in lines" :key="s.name" class="legend-item">
        <span class="dot" :style="{ background: s.color }" />
        <span class="name">{{ s.name }}</span>
        <span class="value mono">{{ fmtValue(s.lastValue) }}{{ unit }}</span>
      </div>
    </div>

    <div v-if="hasData" class="plot">
      <svg :viewBox="`0 0 ${W} ${height}`" preserveAspectRatio="xMidYMid meet">
        <defs>
          <linearGradient v-for="s in series" :id="`${uid}-${s.name}`" :key="s.name" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" :stop-color="s.color" stop-opacity="0.22" />
            <stop offset="100%" :stop-color="s.color" stop-opacity="0.01" />
          </linearGradient>
        </defs>

        <g v-for="row in gridRows" :key="row.value">
          <line :x1="padL" :x2="W - padR" :y1="row.y" :y2="row.y" class="grid-line" />
          <text :x="padL - 10" :y="row.y + 3.5" class="axis-text" text-anchor="end">
            {{ fmtAxis(row.value) }}
          </text>
        </g>

        <g v-for="tick in xTicks" :key="tick.label">
          <text :x="tick.x" :y="height - 8" class="axis-text" text-anchor="middle">{{ tick.label }}</text>
        </g>

        <template v-for="s in lines" :key="`shape-${s.name}`">
          <path v-if="area" :d="s.area" :fill="`url(#${uid}-${s.name})`" />
          <path :d="s.line" fill="none" :stroke="s.color" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
        </template>
      </svg>
    </div>
    <div v-else class="chart-empty">暂无数据</div>
  </div>
</template>

<style scoped>
.chart {
  width: 100%;
}

.legend {
  display: flex;
  flex-wrap: wrap;
  gap: 18px;
  margin-bottom: 6px;
}

.legend-item {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 3px;
}

.name {
  color: var(--fm-muted);
}

.value {
  font-weight: 600;
  color: var(--fm-text);
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

.chart-empty {
  padding: 42px 0;
  text-align: center;
  color: var(--fm-muted);
  font-size: 12px;
  background: var(--fm-surface-alt);
  border-radius: 10px;
}
</style>
