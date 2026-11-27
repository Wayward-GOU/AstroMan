<script setup lang="ts">
/**
 * 占比环形图：模型 / 来源这类「分类占总量多少」的场景。
 * 环 + 右侧图例，中间放总量，不额外引入图表库。
 */
import { computed } from 'vue'
import { fmtInt, fmtTokens } from '@/utils/tokenUsageFormat'

export interface DonutItem {
  name: string
  value: number
}

const props = withDefaults(
  defineProps<{
    items: DonutItem[]
    total: number
    /** 中心标题，如「Token」 */
    centerLabel: string
    /** 数值显示方式：tokens 用 k/M 缩写，count 用千分位 */
    unit?: 'tokens' | 'count'
  }>(),
  { unit: 'tokens' },
)

const SIZE = 168
const RADIUS = 58
const CIRC = 2 * Math.PI * RADIUS

const fmt = (v: number): string =>
  props.unit === 'count' ? fmtInt(Math.round(v)) : fmtTokens(v)

/** 占比不到 0.1% 时不显示成刺眼的 0.0% */
const shareText = (share: number): string =>
  share > 0 && share < 0.1 ? '<0.1' : share.toFixed(1)

const segments = computed(() => {
  const total = props.total > 0 ? props.total : 1
  let cursor = 0
  return props.items.map((item, index) => {
    const frac = Math.max(0, item.value / total)
    const arc = frac * CIRC
    // 留 2px 缝，段与段之间不粘连；极小段至少给 1.5px，否则完全看不见
    const dash = item.value > 0 ? Math.max(1.5, arc - 2) : 0
    const seg = {
      ...item,
      index,
      dash,
      gap: Math.max(0, CIRC - dash),
      offset: -cursor,
      share: total > 0 ? (item.value / total) * 100 : 0,
    }
    cursor += arc
    return seg
  })
})

const leading = computed(() => props.items[0] ?? null)
</script>

<template>
  <div class="donut">
    <div class="donut-ring">
      <svg :width="SIZE" :height="SIZE" :viewBox="`0 0 ${SIZE} ${SIZE}`" role="img" aria-label="占比环形图">
        <circle
          :cx="SIZE / 2"
          :cy="SIZE / 2"
          :r="RADIUS"
          class="track"
        />
        <g :transform="`rotate(-90 ${SIZE / 2} ${SIZE / 2})`">
          <circle
            v-for="seg in segments"
            :key="seg.name"
            :cx="SIZE / 2"
            :cy="SIZE / 2"
            :r="RADIUS"
            class="seg"
            :class="`c${(seg.index % 8) + 1}`"
            :stroke-dasharray="`${seg.dash} ${seg.gap}`"
            :stroke-dashoffset="seg.offset"
          >
            <title>{{ seg.name }}：{{ fmt(seg.value) }}（{{ seg.share.toFixed(1) }}%）</title>
          </circle>
        </g>
      </svg>
      <div class="donut-center">
        <strong>{{ fmt(total) }}</strong>
        <span>{{ centerLabel }}</span>
      </div>
    </div>

    <div class="donut-side">
      <ul class="donut-legend">
        <li v-for="seg in segments" :key="seg.name">
          <i class="dot" :class="`c${(seg.index % 8) + 1}`" />
          <span class="name" :title="seg.name">{{ seg.name }}</span>
          <span class="share">{{ shareText(seg.share) }}%</span>
          <span class="value">{{ fmt(seg.value) }}</span>
        </li>
        <li v-if="!segments.length" class="empty">所选窗口内没有记录</li>
      </ul>

      <p v-if="leading" class="donut-foot">
        占比最高：<b>{{ leading.name }}</b>
        {{ shareText(segments[0]?.share ?? 0) }}%
      </p>
    </div>
  </div>
</template>

<style scoped>
.donut {
  display: flex;
  align-items: center;
  gap: 16px;
}

.donut-ring {
  position: relative;
  flex: none;
  line-height: 0;
}

.track {
  fill: none;
  stroke: var(--fm-surface-alt);
  stroke-width: 16;
}

.seg {
  fill: none;
  stroke-width: 16;
  stroke-linecap: butt;
  transition: stroke-dasharray var(--fm-dur-mid) var(--fm-ease);
}

.c1 { stroke: var(--fm-chart-1); background: var(--fm-chart-1); }
.c2 { stroke: var(--fm-chart-2); background: var(--fm-chart-2); }
.c3 { stroke: var(--fm-chart-3); background: var(--fm-chart-3); }
.c4 { stroke: var(--fm-chart-4); background: var(--fm-chart-4); }
.c5 { stroke: var(--fm-chart-5); background: var(--fm-chart-5); }
.c6 { stroke: var(--fm-chart-6); background: var(--fm-chart-6); }
.c7 { stroke: var(--fm-chart-7); background: var(--fm-chart-7); }
.c8 { stroke: var(--fm-chart-8); background: var(--fm-chart-8); }

.donut-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  line-height: 1.1;
  pointer-events: none;
}

.donut-center strong {
  font-size: 18px;
  font-weight: 600;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.donut-center span {
  font-size: 9.5px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.donut-side {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.donut-legend {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.donut-legend li {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 10.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.donut-legend .dot {
  width: 7px;
  height: 7px;
  border-radius: var(--fm-radius-pill);
  flex: none;
}

.donut-legend .name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.donut-legend .share {
  margin-left: auto;
  color: var(--fm-muted);
}

.donut-legend .value {
  min-width: 58px;
  text-align: right;
  color: var(--fm-text);
}

.donut-foot {
  margin: 0;
  padding-top: 8px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  color: var(--fm-muted);
}

.donut-foot b {
  color: var(--fm-text-2);
  font-weight: 600;
}

.empty {
  justify-content: center;
  color: var(--fm-muted);
}
</style>
