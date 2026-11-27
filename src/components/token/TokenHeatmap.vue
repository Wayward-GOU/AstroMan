<script setup lang="ts">
import { computed } from 'vue'
import type { Heat } from '@/utils/tokenUsageAgg'
import { fmtDateShort, fmtTokens } from '@/utils/tokenUsageFormat'

const props = defineProps<{ heat: Heat }>()

const HOURS = Array.from({ length: 24 }, (_, h) => h)

interface Row {
  date: string
  short: string
  total: number
  cells: { hour: number; value: number; level: number }[]
}

/** 色阶 0–5：0 是空格，5 是当日峰值 */
function levelOf(value: number, max: number): number {
  if (value <= 0) return 0
  const ratio = value / max
  return Math.min(5, Math.max(1, Math.ceil(ratio * 5)))
}

const rows = computed<Row[]>(() =>
  props.heat.dates.map((date, i) => {
    const values = props.heat.matrix[i] ?? []
    return {
      date,
      short: fmtDateShort(date),
      total: props.heat.dailyTotal[i] ?? 0,
      cells: HOURS.map((hour) => {
        const value = values[hour] ?? 0
        return { hour, value, level: levelOf(value, props.heat.max) }
      }),
    }
  }),
)

const empty = computed(() => !rows.value.length)
</script>

<template>
  <div v-if="!empty" class="heat">
    <div class="heat-grid">
      <span class="corner" />
      <span v-for="h in HOURS" :key="`h-${h}`" class="hour" :class="{ on: h % 3 === 0 }">
        {{ h % 3 === 0 ? String(h).padStart(2, '0') : '' }}
      </span>

      <template v-for="row in rows" :key="row.date">
        <span class="date" :title="`${row.date} · ${fmtTokens(row.total)} token`">
          {{ row.short }}
        </span>
        <span
          v-for="cell in row.cells"
          :key="`${row.date}-${cell.hour}`"
          class="cell"
          :class="`lv-${cell.level}`"
          :title="`${row.date} ${String(cell.hour).padStart(2, '0')}:00 · ${fmtTokens(cell.value)}`"
        />
      </template>
    </div>

    <div class="legend">
      <span>低</span>
      <i v-for="lv in [0, 1, 2, 3, 4, 5]" :key="lv" :class="`cell lv-${lv}`" />
      <span>高</span>
    </div>
  </div>
  <div v-else class="heat-empty">所选窗口内没有调用记录</div>
</template>

<style scoped>
.heat-grid {
  display: grid;
  grid-template-columns: 46px repeat(24, minmax(0, 1fr));
  gap: 2px;
  align-items: center;
}

.corner {
  height: 14px;
}

.hour {
  font-size: 9px;
  line-height: 1;
  text-align: center;
  color: transparent;
  font-variant-numeric: tabular-nums;
}

.hour.on {
  color: var(--fm-muted);
}

.date {
  font-size: 9.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.cell {
  height: 13px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
}

.cell.lv-1 {
  background: color-mix(in srgb, var(--fm-chart-1) 16%, var(--fm-surface));
}

.cell.lv-2 {
  background: color-mix(in srgb, var(--fm-chart-1) 32%, var(--fm-surface));
}

.cell.lv-3 {
  background: color-mix(in srgb, var(--fm-chart-1) 52%, var(--fm-surface));
}

.cell.lv-4 {
  background: color-mix(in srgb, var(--fm-chart-1) 74%, var(--fm-surface));
}

/* 最高一档与正文色混合：浅色主题更沉、深色主题更亮，两套主题都保证「越满越醒目」 */
.cell.lv-5 {
  background: color-mix(in srgb, var(--fm-chart-1) 86%, var(--fm-text));
}

.legend {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 8px;
  font-size: 9.5px;
  color: var(--fm-muted);
}

.legend i {
  display: block;
  width: 16px;
  height: 9px;
  border-radius: var(--fm-radius-sm);
}

.heat-empty {
  padding: 34px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-card);
}
</style>
