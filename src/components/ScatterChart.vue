<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    points: Array<{ x: number; y: number }>
    xUnit?: string
    yUnit?: string
    height?: number
  }>(),
  { xUnit: '', yUnit: '', height: 230 },
)

const W = 720
const padL = 52
const padR = 18
const padT = 16
const padB = 28

const valid = computed(() =>
  props.points.filter((item) => Number.isFinite(item.x) && Number.isFinite(item.y) && item.x > 0),
)

function niceMax(value: number) {
  if (!Number.isFinite(value) || value <= 0) return 1
  const magnitude = Math.pow(10, Math.floor(Math.log10(value)))
  return Math.ceil(value / magnitude) * magnitude
}

const xMax = computed(() => niceMax(Math.max(1, ...valid.value.map((item) => item.x)) * 1.04))
const yMax = computed(() => niceMax(Math.max(1, ...valid.value.map((item) => item.y)) * 1.08))

function px(value: number) {
  return padL + (Math.min(value, xMax.value) / xMax.value) * (W - padL - padR)
}

function py(value: number) {
  const inner = props.height - padT - padB
  return padT + inner - (Math.min(value, yMax.value) / yMax.value) * inner
}

function fmtAxis(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10000 ? 0 : 1)}k`
  if (value >= 10) return value.toFixed(0)
  if (value >= 1) return value.toFixed(1)
  return value.toFixed(2)
}

const dots = computed(() =>
  valid.value.map((item) => ({ x: px(item.x), y: py(item.y) })),
)

const gridRows = computed(() =>
  [1, 0.5, 0].map((ratio) => ({
    value: yMax.value * ratio,
    y: py(yMax.value * ratio),
  })),
)

const xTicks = computed(() =>
  Array.from({ length: 5 }, (_, index) => {
    const value = (xMax.value / 4) * index
    return { value, x: px(value) }
  }),
)
</script>

<template>
  <div class="scatter">
    <div class="legend">
      <span class="dot" />
      <span class="name">每秒采样</span>
      <span class="value mono">{{ valid.length }} 点</span>
    </div>
    <div class="axis-hint">
      <span>横轴 {{ xUnit || 'x' }}</span>
      <span>纵轴 {{ yUnit || 'y' }}</span>
    </div>
    <div v-if="dots.length" class="plot">
      <svg :viewBox="`0 0 ${W} ${height}`" preserveAspectRatio="xMidYMid meet">
        <g v-for="row in gridRows" :key="row.value">
          <line :x1="padL" :x2="W - padR" :y1="row.y" :y2="row.y" class="grid-line" />
          <text :x="padL - 10" :y="row.y + 3.5" class="axis-text" text-anchor="end">
            {{ fmtAxis(row.value) }}
          </text>
        </g>
        <g v-for="tick in xTicks" :key="tick.value">
          <text :x="tick.x" :y="height - 8" class="axis-text" text-anchor="middle">
            {{ fmtAxis(tick.value) }}
          </text>
        </g>
        <circle
          v-for="(item, index) in dots"
          :key="index"
          :cx="item.x"
          :cy="item.y"
          r="2.4"
          class="dot-point"
        />
      </svg>
    </div>
    <div v-else class="chart-empty">暂无采样数据</div>
  </div>
</template>

<style scoped>
.scatter {
  width: 100%;
}
.legend {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  color: var(--fm-text-2);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--fm-primary);
}
.name {
  color: var(--fm-muted);
}
.value {
  font-weight: 600;
  color: var(--fm-text);
}
.axis-hint {
  display: flex;
  gap: 18px;
  margin: 2px 0 6px;
  font-size: 11px;
  color: var(--fm-muted);
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
.dot-point {
  fill: var(--fm-primary);
  opacity: 0.45;
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
