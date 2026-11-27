<script setup lang="ts">
import { computed } from 'vue'
import type { SizeDist } from '@/utils/tokenUsageAgg'
import { fmtInt, fmtTokens } from '@/utils/tokenUsageFormat'

const props = defineProps<{ sizes: SizeDist }>()

const maxCount = computed(() => Math.max(1, ...props.sizes.counts))

const bars = computed(() =>
  props.sizes.labels.map((label, i) => {
    const count = props.sizes.counts[i] ?? 0
    return {
      label,
      count,
      height: count > 0 ? Math.max(3, (count / maxCount.value) * 100) : 0,
      share: props.sizes.n > 0 ? (count / props.sizes.n) * 100 : 0,
    }
  }),
)

const summary = computed(() => [
  { label: 'p50', value: fmtTokens(props.sizes.p50) },
  { label: 'p90', value: fmtTokens(props.sizes.p90) },
  { label: 'p99', value: fmtTokens(props.sizes.p99) },
  { label: '最大', value: fmtTokens(props.sizes.max) },
  { label: '均值', value: fmtTokens(props.sizes.mean) },
])
</script>

<template>
  <div class="hist">
    <div class="hist-bars">
      <div v-for="bar in bars" :key="bar.label" class="bar-slot">
        <div class="bar-track">
          <div
            class="bar-fill"
            :style="{ height: `${bar.height}%` }"
            :title="`${bar.label} token：${fmtInt(bar.count)} 次，占 ${bar.share.toFixed(1)}%`"
          />
        </div>
        <span class="bar-label">{{ bar.label }}</span>
        <span class="bar-count">{{ bar.count ? fmtInt(bar.count) : '0' }}</span>
      </div>
    </div>

    <div class="hist-summary">
      <span v-for="item in summary" :key="item.label" class="pct">
        <em>{{ item.label }}</em>
        {{ item.value }}
      </span>
      <span class="total">样本 {{ fmtInt(sizes.n) }} 次</span>
    </div>
  </div>
</template>

<style scoped>
.hist {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.hist-bars {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  height: 148px;
}

.bar-slot {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-end;
  gap: 3px;
  height: 100%;
}

.bar-count {
  font-size: 9.5px;
  line-height: 1;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.bar-track {
  width: 100%;
  flex: 1;
  display: flex;
  align-items: flex-end;
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
}

.bar-fill {
  width: 100%;
  background: linear-gradient(180deg, var(--fm-chart-2), var(--fm-chart-1));
  border-radius: var(--fm-radius-sm);
  transition: height var(--fm-dur-mid) var(--fm-ease);
}

.bar-label {
  font-size: 9.5px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.hist-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding-top: 8px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.pct em {
  font-style: normal;
  margin-right: 4px;
  color: var(--fm-muted);
}

.total {
  margin-left: auto;
  color: var(--fm-muted);
}
</style>
