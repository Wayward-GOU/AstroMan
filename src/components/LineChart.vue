<script setup lang="ts">
import { computed } from 'vue'

export interface ChartSeries {
  name: string
  color: string
  values: number[]
  /** 双 Y 轴：外部指标放右轴，缺省左轴 */
  axis?: 'left' | 'right'
  /** 单序列单位（图例），缺省用全局 unit */
  unit?: string
}

const props = withDefaults(
  defineProps<{
    series: ChartSeries[]
    labels?: string[]
    unit?: string
    area?: boolean
    height?: number
    /** 堆叠面积图：用于并发状态分布等「总量有上限」的序列 */
    stacked?: boolean
  }>(),
  { labels: () => [], unit: '', area: true, height: 230, stacked: false },
)

const W = 720
const padL = 52
const padR = 18
const padT = 16
const padB = 26

const uid = `fm-${Math.random().toString(36).slice(2, 9)}`

const count = computed(() => Math.max(...props.series.map((s) => s.values.length), 0))

/** 堆叠模式下第 i 条序列的累计边界；非堆叠模式为空 */
const cumulative = computed(() =>
  props.stacked
    ? props.series.reduce<number[][]>((acc, s, i) => {
        const prev = i === 0 ? [] : acc[i - 1]
        acc.push(Array.from({ length: count.value }, (_, x) => (prev[x] ?? 0) + (s.values[x] ?? 0)))
        return acc
      }, [])
    : [],
)

/** 双 Y 轴时右侧留出刻度空间 */
const hasRight = computed(() => props.series.some((s) => (s.axis ?? 'left') === 'right'))
const rightPad = computed(() => (hasRight.value ? 46 : padR))

function axisMax(axis: 'left' | 'right') {
  const values = props.stacked
    ? axis === 'left'
      ? cumulative.value[cumulative.value.length - 1] ?? []
      : []
    : props.series
        .filter((s) => (s.axis ?? 'left') === axis)
        .flatMap((s) => s.values)
  const raw = Math.max(0, ...values)
  if (raw <= 0) return 1
  const magnitude = Math.pow(10, Math.floor(Math.log10(raw)))
  return Math.ceil(raw / magnitude) * magnitude
}

const maxLeft = computed(() => axisMax('left'))
const maxRight = computed(() => axisMax('right'))

function px(index: number) {
  const inner = W - padL - rightPad.value
  if (count.value <= 1) return padL + inner / 2
  return padL + (index / (count.value - 1)) * inner
}

function py(value: number, axis: 'left' | 'right' = 'left') {
  const inner = props.height - padT - padB
  const max = axis === 'right' ? maxRight.value : maxLeft.value
  const ratio = Math.min(1, Math.max(0, value / max))
  return padT + inner - ratio * inner
}

const lines = computed(() =>
  props.series.map((s, index) => {
    const values = props.stacked ? cumulative.value[index] : s.values
    const axis = s.axis ?? 'left'
    const points = values.map((v, i) => `${px(i).toFixed(1)},${py(v, axis).toFixed(1)}`)
    const line = points.length ? `M${points.join(' L')}` : ''
    const baseline = props.height - padB
    let area = ''
    if (points.length) {
      if (props.stacked && index > 0) {
        const lower = cumulative.value[index - 1]
          .map((v, x) => `${px(x).toFixed(1)},${py(v, 'left').toFixed(1)}`)
          .reverse()
        area = `${line} L${lower.join(' L')} Z`
      } else {
        const first = px(0)
        const last = px(Math.max(0, values.length - 1))
        area = `${line} L${last.toFixed(1)},${baseline} L${first.toFixed(1)},${baseline} Z`
      }
    }
    return {
      ...s,
      gid: `${uid}-g${index}`,
      line,
      area,
      lastValue: s.values[s.values.length - 1] ?? 0,
      unitLabel: s.unit ?? props.unit,
    }
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
  { value: maxLeft.value, y: py(maxLeft.value) },
  { value: maxLeft.value / 2, y: py(maxLeft.value / 2) },
  { value: 0, y: py(0) },
])

/** 右轴刻度与左轴共用水平线，只换标签 */
const rightRows = computed(() =>
  hasRight.value
    ? [
        { value: maxRight.value, y: py(maxRight.value, 'right') },
        { value: maxRight.value / 2, y: py(maxRight.value / 2, 'right') },
        { value: 0, y: py(0, 'right') },
      ]
    : [],
)

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
        <span class="value mono">{{ fmtValue(s.lastValue) }}{{ s.unitLabel }}</span>
      </div>
    </div>

    <div v-if="hasData" class="plot">
      <svg :viewBox="`0 0 ${W} ${height}`" preserveAspectRatio="xMidYMid meet">
        <defs>
          <linearGradient v-for="s in lines" :id="s.gid" :key="s.gid" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" :stop-color="s.color" stop-opacity="0.22" />
            <stop offset="100%" :stop-color="s.color" stop-opacity="0.01" />
          </linearGradient>
        </defs>

        <g v-for="row in gridRows" :key="`l${row.value}`">
          <line :x1="padL" :x2="W - rightPad" :y1="row.y" :y2="row.y" class="grid-line" />
          <text :x="padL - 10" :y="row.y + 3.5" class="axis-text" text-anchor="end">
            {{ fmtAxis(row.value) }}
          </text>
        </g>

        <g v-for="row in rightRows" :key="`r${row.value}`">
          <text :x="W - rightPad + 8" :y="row.y + 3.5" class="axis-text" text-anchor="start">
            {{ fmtAxis(row.value) }}
          </text>
        </g>

        <g v-for="tick in xTicks" :key="tick.label">
          <text :x="tick.x" :y="height - 8" class="axis-text" text-anchor="middle">{{ tick.label }}</text>
        </g>

        <template v-for="s in lines" :key="`shape-${s.name}`">
          <path v-if="area" :d="s.area" :fill="`url(#${s.gid})`" />
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