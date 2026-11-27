<script setup lang="ts">
import { computed } from 'vue'
import type { WorkspaceRow } from '@/utils/tokenUsageAgg'
import { fmtInt, fmtPct, fmtTokens } from '@/utils/tokenUsageFormat'

const props = withDefaults(defineProps<{ rows: WorkspaceRow[]; limit?: number; noun?: string }>(), {
  limit: 8,
  noun: '工作空间',
})

const visible = computed(() => props.rows.slice(0, props.limit))
const maxTokens = computed(() => Math.max(1, ...visible.value.map((r) => r.tokens)))
const restCount = computed(() => Math.max(0, props.rows.length - visible.value.length))
</script>

<template>
  <div v-if="visible.length" class="ws">
    <div v-for="row in visible" :key="row.name" class="ws-row">
      <span class="ws-name" :title="row.name">{{ row.name }}</span>
      <span class="ws-track">
        <span class="ws-fill" :style="{ width: `${Math.max(2, (row.tokens / maxTokens) * 100)}%` }" />
      </span>
      <span class="ws-share">{{ fmtPct(row.share) }}</span>
      <span class="ws-tokens">{{ fmtTokens(row.tokens) }}</span>
      <span class="ws-req">{{ fmtInt(row.requests) }} 次</span>
    </div>
    <p v-if="restCount" class="ws-rest">另有 {{ restCount }} 个{{ props.noun }}占比更小</p>
  </div>
  <div v-else class="ws-empty">所选窗口内没有{{ props.noun }}记录</div>
</template>

<style scoped>
.ws {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ws-row {
  display: grid;
  grid-template-columns: 116px minmax(0, 1fr) 46px 62px 52px;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}

.ws-name {
  color: var(--fm-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ws-track {
  height: 8px;
  border-radius: var(--fm-radius-pill);
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.ws-fill {
  display: block;
  height: 100%;
  border-radius: var(--fm-radius-pill);
  background: linear-gradient(90deg, var(--fm-chart-1), var(--fm-chart-2));
}

.ws-share,
.ws-tokens,
.ws-req {
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
}

.ws-req {
  color: var(--fm-muted);
}

.ws-rest {
  margin: 2px 0 0;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.ws-empty {
  padding: 34px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-card);
}
</style>
