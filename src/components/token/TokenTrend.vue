<script setup lang="ts">
/**
 * 每日趋势折线：一个指标一条线，面积渐变填充。
 * 宽度用 ResizeObserver 跟随卡片；高度固定，避免大屏上被拉成一片。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { DailySeries } from '@/utils/tokenUsageAgg'
import { fmtDateShort, fmtInt, fmtTokens, fmtUsd } from '@/utils/tokenUsageFormat'

export type TrendMetric = 'tokens' | 'requests' | 'cost' | 'cacheRate'

const props = defineProps<{ daily: DailySeries; metric: TrendMetric }>()

const H = 196
const PAD_L = 50
const PAD_R = 14
const PAD_T = 12
const PAD_B = 22

const host = ref<HTMLDivElement | null>(null)
const width = ref(640)
let observer: ResizeObserver | undefined

onMounted(() => {
  if (!host.value) return
  observer = new ResizeObserver((entries) => {
    const next = entries[0]?.contentRect.width ?? 640
    // 卡片还在布局时可能是 0，给一个下限免得坐标算成 NaN
    width.value = Math.max(320, next)
  })
  observer.observe(host.value)
})

onBeforeUnmount(() => observer?.disconnect())

const values = computed<number[]>(() => {
  const d = props.daily
  switch (props.metric) {
    case 'requests':
      return d.requests
    case 'cost':
      return d.cost
    case 'cacheRate':
      return d.cacheRate.map((v) => v * 100)
    default:
      return d.tokens
  }
})

const format = computed<(v: number) => string>(() => {
  switch (props.metric) {
    case 'requests':
      return (v) => fmtInt(Math.round(v))
    case 'cost':
      return (v) => fmtUsd(v)
    case 'cacheRate':
      return (v) => `${v.toFixed(1)}%`
    default:
      return (v) => fmtTokens(v)
  }
})

const unit = computed(() => {
  switch (props.metric) {
    case 'requests':
      return '次'
    case 'cost':
      return '美元'
    case 'cacheRate':
      return '%'
    default:
      return 'token'
  }
})

/** 1 / 2 / 5 阶梯，让 y 轴刻度落在整数上 */
function niceStep(raw: number): number {
  if (!Number.isFinite(raw) || raw <= 0) return 1
  const exp = Math.floor(Math.log10(raw))
  const base = Math.pow(10, exp)
  const norm = raw / base
  const step = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10
  return step * base
}

const chart = computed(() => {
  const w = width.value
  const innerW = Math.max(1, w - PAD_L - PAD_R)
  const innerH = H - PAD_T - PAD_B
  const list = values.value
  const n = list.length
  const peak = Math.max(1, ...list)
  const step = niceStep(peak / 3)
  const top = Math.max(step, Math.ceil(peak / step) * step)
  const x = (i: number): number =>
    PAD_L + (n <= 1 ? innerW / 2 : (i / (n - 1)) * innerW)
  const y = (v: number): number => PAD_T + innerH - (v / top) * innerH

  const points = list.map((v, i) => ({ x: x(i), y: y(v), v, i }))
  const line = points
    .map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(1)},${p.y.toFixed(1)}`)
    .join(' ')
  const area =
    points.length > 0
      ? `${line} L${points[points.length - 1].x.toFixed(1)},${(PAD_T + innerH).toFixed(1)} L${points[0].x.toFixed(1)},${(PAD_T + innerH).toFixed(1)} Z`
      : ''

  const ticks = [0, 1, 2, 3].map((k) => {
    const value = (top / 3) * k
    return { value, y: y(value), label: format.value(value) }
  })

  // x 轴最多 6 个标签，密了就抽稀，避免挤在一起
  const dates = props.daily.dates
  const stride = Math.max(1, Math.ceil(n / 6))
  const xLabels = dates
    .map((d, i) => ({ i, text: fmtDateShort(d) }))
    .filter((item) => item.i % stride === 0 || item.i === n - 1)
    .map((item) => ({ x: x(item.i), text: item.text }))

  return { w, innerH, points, line, area, ticks, xLabels, peak, top }
})

const summary = computed(() => {
  const list = values.value
  if (!list.length) return null
  const sum = list.reduce((a, b) => a + b, 0)
  const active = list.filter((v) => v > 0).length
  return {
    total: format.value(sum),
    avg: format.value(sum / list.length),
    peak: format.value(Math.max(...list)),
    active,
    days: list.length,
  }
})
</script>

<template>
  <div ref="host" class="trend">
    <svg
      v-if="daily.dates.length"
      class="trend-svg"
      :width="chart.w"
      :height="H"
      :viewBox="`0 0 ${chart.w} ${H}`"
      role="img"
      :aria-label="`每日${unit}趋势`"
    >
      <defs>
        <linearGradient id="trend-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--fm-chart-2)" stop-opacity="0.34" />
          <stop offset="100%" stop-color="var(--fm-chart-2)" stop-opacity="0.02" />
        </linearGradient>
      </defs>

      <g class="grid">
        <line
          v-for="tick in chart.ticks"
          :key="tick.value"
          :x1="PAD_L"
          :x2="chart.w - PAD_R"
          :y1="tick.y"
          :y2="tick.y"
        />
      </g>

      <g class="axis">
        <text
          v-for="tick in chart.ticks"
          :key="`y-${tick.value}`"
          :x="PAD_L - 8"
          :y="tick.y + 3"
          text-anchor="end"
        >
          {{ tick.label }}
        </text>
        <text
          v-for="label in chart.xLabels"
          :key="`x-${label.x}`"
          :x="label.x"
          :y="H - 6"
          text-anchor="middle"
        >
          {{ label.text }}
        </text>
      </g>

      <path v-if="chart.area" :d="chart.area" fill="url(#trend-fill)" />
      <path v-if="chart.line" :d="chart.line" class="line" />

      <g v-if="chart.points.length <= 62">
        <circle
          v-for="p in chart.points"
          :key="p.i"
          class="dot"
          :cx="p.x"
          :cy="p.y"
          r="2.4"
        >
          <title>{{ daily.dates[p.i] }}：{{ format(p.v) }} {{ unit }}</title>
        </circle>
      </g>
    </svg>

    <div v-else class="empty">所选窗口内没有记录</div>

    <div v-if="summary" class="trend-summary">
      <span><em>合计</em>{{ summary.total }}</span>
      <span><em>日均</em>{{ summary.avg }}</span>
      <span><em>峰值</em>{{ summary.peak }}</span>
      <span class="muted">{{ summary.active }} / {{ summary.days }} 天有调用</span>
    </div>
  </div>
</template>

<style scoped>
.trend {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.trend-svg {
  display: block;
  width: 100%;
  height: 196px;
  overflow: visible;
}

.grid line {
  stroke: var(--fm-border);
  stroke-width: 1;
  stroke-dasharray: 3 4;
}

.axis text {
  fill: var(--fm-muted);
  font-size: 9.5px;
  font-variant-numeric: tabular-nums;
}

.line {
  fill: none;
  stroke: var(--fm-chart-2);
  stroke-width: 1.8;
  stroke-linejoin: round;
  stroke-linecap: round;
}

.dot {
  fill: var(--fm-surface);
  stroke: var(--fm-chart-2);
  stroke-width: 1.4;
}

.trend-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 14px;
  padding-top: 8px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.trend-summary em {
  font-style: normal;
  margin-right: 5px;
  color: var(--fm-muted);
}

.trend-summary .muted {
  margin-left: auto;
  color: var(--fm-muted);
}

.empty {
  padding: 40px 0;
  text-align: center;
  font-size: 11px;
  color: var(--fm-muted);
}
</style>
