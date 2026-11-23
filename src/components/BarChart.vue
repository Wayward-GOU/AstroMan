<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    items: Array<{ label: string; value: number; sub?: string }>
    unit?: string
    digits?: number
  }>(),
  { unit: '', digits: 1 },
)

const valid = computed(() =>
  props.items.filter((item) => Number.isFinite(item.value) && item.value >= 0),
)

const max = computed(() => Math.max(1, ...valid.value.map((item) => item.value)))

function width(value: number) {
  return Math.max(1, (Math.min(value, max.value) / max.value) * 100)
}

function format(value: number) {
  return value >= 1000
    ? value.toLocaleString('zh-CN', { maximumFractionDigits: 0 })
    : value.toFixed(props.digits)
}
</script>

<template>
  <div v-if="valid.length" class="bars">
    <div v-for="item in valid" :key="`${item.label}-${item.value}`" class="bar-row">
      <div class="bar-label" :title="item.label">
        <span class="label-text">{{ item.label }}</span>
        <span v-if="item.sub" class="label-sub">{{ item.sub }}</span>
      </div>
      <div class="bar-track">
        <div class="bar-fill" :style="{ width: `${width(item.value)}%` }" />
      </div>
      <span class="bar-value mono">{{ format(item.value) }} {{ unit }}</span>
    </div>
  </div>
  <div v-else class="chart-empty">暂无可对比数据</div>
</template>

<style scoped>
.bars {
  display: flex;
  flex-direction: column;
  gap: 9px;
}
.bar-row {
  display: grid;
  grid-template-columns: 180px 1fr 96px;
  align-items: center;
  gap: 10px;
}
.bar-label {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.label-text {
  font-size: 12px;
  color: var(--fm-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.label-sub {
  font-size: 10px;
  color: var(--fm-muted);
}
.bar-track {
  height: 9px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, var(--fm-primary), var(--fm-primary-soft, var(--fm-primary)));
}
.bar-value {
  text-align: right;
  font-size: 12px;
  color: var(--fm-text);
}
.chart-empty {
  padding: 42px 0;
  text-align: center;
  color: var(--fm-muted);
  font-size: 12px;
  background: var(--fm-surface-alt);
  border-radius: 10px;
}
@media (max-width: 720px) {
  .bar-row {
    grid-template-columns: 120px 1fr 76px;
  }
}
</style>
