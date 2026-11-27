<script setup lang="ts">
import { computed } from 'vue'
import type { TokenKpis } from '@/utils/tokenUsageAgg'
import {
  fmtCnyFromUsd,
  fmtCredit,
  fmtDelta,
  fmtInt,
  fmtPct,
  fmtTokens,
  fmtUsd,
  USD_TO_CNY,
} from '@/utils/tokenUsageFormat'

const props = defineProps<{ kpis: TokenKpis }>()

interface Tile {
  label: string
  value: string
  sub: string
  delta?: number | null
  compact?: boolean
}

const tiles = computed<Tile[]>(() => {
  const k = props.kpis
  return [
    {
      label: '合计 Token',
      value: fmtTokens(k.total),
      sub: fmtInt(k.total),
      delta: k.totalDelta,
    },
    {
      label: '请求数',
      value: fmtInt(k.requests),
      sub: '次模型调用',
      delta: k.reqDelta,
    },
    {
      label: '会话 / 活跃天',
      value: `${fmtInt(k.sessions)} / ${k.activeDays}`,
      sub: `日均 ${fmtTokens(k.avgPerActiveDay)}（按活跃日）`,
    },
    {
      label: '缓存命中率',
      value: fmtPct(k.cacheRate * 100),
      sub: '缓存读取 / 输入 token',
    },
    {
      label: '官方实际积分',
      value: fmtCredit(k.credit),
      sub: '逐请求 credit 汇总',
      delta: k.creditDelta,
    },
    {
      label: '费用（估算）',
      value: fmtUsd(k.costUsd),
      sub:
        k.unpriced > 0
          ? `≈ ${fmtCnyFromUsd(k.costUsd)} · ${fmtInt(k.unpriced)} 条无价目`
          : `≈ ${fmtCnyFromUsd(k.costUsd)} · 固定汇率 ${USD_TO_CNY}`,
      delta: k.costDelta,
    },
    {
      label: '主力模型',
      value: k.topModel,
      sub: `占 ${fmtPct(k.topShare)}`,
      compact: true,
    },
  ]
})
</script>

<template>
  <div class="kpi-row">
    <article v-for="tile in tiles" :key="tile.label" class="kpi">
      <span class="kpi-label">{{ tile.label }}</span>
      <strong class="kpi-value" :class="{ compact: tile.compact }" :title="tile.value">
        {{ tile.value }}
      </strong>
      <span class="kpi-sub" :title="tile.sub">{{ tile.sub }}</span>
      <span v-if="tile.delta !== undefined" class="kpi-delta" :class="fmtDelta(tile.delta ?? null).dir">
        {{ fmtDelta(tile.delta ?? null).dir === 'up' ? '▲' : fmtDelta(tile.delta ?? null).dir === 'down' ? '▼' : '—' }}
        {{ fmtDelta(tile.delta ?? null).text }}
        <em>环比</em>
      </span>
    </article>
  </div>
</template>

<style scoped>
.kpi-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(148px, 1fr));
  gap: 10px;
  margin-bottom: 12px;
}

.kpi {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  padding: 12px 14px 11px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
  box-shadow: var(--fm-shadow);
}

.kpi-label {
  font-size: 11px;
  letter-spacing: 0.02em;
  color: var(--fm-muted);
}

.kpi-value {
  font-size: 21px;
  font-weight: 600;
  line-height: 1.25;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.kpi-value.compact {
  font-size: 14px;
  letter-spacing: 0;
}

.kpi-sub {
  font-size: 10.5px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.kpi-delta {
  margin-top: 1px;
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
  color: var(--fm-muted);
}

.kpi-delta.up {
  color: var(--fm-danger);
}

.kpi-delta.down {
  color: var(--fm-success);
}

.kpi-delta em {
  margin-left: 3px;
  font-style: normal;
  color: var(--fm-muted);
}

@media (max-width: 1280px) {
  .kpi-row {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}
</style>
