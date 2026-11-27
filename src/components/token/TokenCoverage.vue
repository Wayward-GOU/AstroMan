<script setup lang="ts">
import { computed } from 'vue'
import { FolderOpenOutlined } from '@ant-design/icons-vue'
import type { ClientProbe, ClientStatus } from '@/types/tokenUsage'
import { UNOBSERVABLE } from '@/types/tokenUsage'
import { fmtInt, fmtMB, fmtTokens } from '@/utils/tokenUsageFormat'

const props = defineProps<{
  clients: ClientProbe[]
  probing: boolean
}>()

const emit = defineEmits<{ reveal: [path: string] }>()

const STATUS_LABEL: Record<ClientStatus, string> = {
  covered: '已接入',
  idle: '待使用',
  blind: '读不到',
  absent: '未安装',
}

const KIND_LABEL: Record<ClientProbe['kind'], string> = {
  cli: 'CLI',
  desktop: '桌面端',
  ide: 'IDE',
  proxy: '代理',
}

/** 已接入的排前面，未安装的收起来不占版面 */
const visible = computed(() =>
  props.clients.filter((c) => c.status !== 'absent').slice(0, 8),
)

const absentCount = computed(() => props.clients.filter((c) => c.status === 'absent').length)

function metricOf(client: ClientProbe): string {
  if (client.status === 'covered') return fmtTokens(client.tokens)
  if (client.status === 'blind') return fmtMB(client.dataMB)
  return '—'
}
</script>

<template>
  <div class="cov">
    <p v-if="probing" class="cov-loading">正在探测本机 AI 客户端…</p>

    <div v-for="client in visible" :key="client.id" class="cov-row">
      <span class="cov-name">
        {{ client.name }}
        <em>{{ KIND_LABEL[client.kind] }}</em>
      </span>
      <span class="cov-status" :class="`s-${client.status}`">{{ STATUS_LABEL[client.status] }}</span>
      <span class="cov-metric">{{ metricOf(client) }}</span>
      <span class="cov-detail" :title="client.detail">{{ client.detail }}</span>
      <button
        v-if="client.paths.length"
        class="cov-reveal"
        type="button"
        title="在访达中显示日志目录"
        @click="emit('reveal', client.paths[0])"
      >
        <FolderOpenOutlined />
      </button>
    </div>

    <p v-if="absentCount" class="cov-absent">另有 {{ absentCount }} 个客户端未安装，不计入统计</p>

    <div class="cov-note">
      <p class="cov-note-title">无论如何都统计不到</p>
      <p v-for="item in UNOBSERVABLE" :key="item.name" class="cov-note-row">
        <b>{{ item.name }}</b>
        <span>{{ item.detail }}</span>
      </p>
      <p class="cov-note-foot">
        判定标准只有一条：本机是否存在该客户端写下的 token 用量记录。读不到的客户端如实标注为「读不到」，
        不用文件体积或次数估算冒充用量。
      </p>
    </div>

    <p v-if="!probing && !clients.length" class="cov-loading">未探测到任何 AI 客户端</p>
  </div>
</template>

<style scoped>
.cov {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.cov-loading {
  margin: 0 0 4px;
  font-size: 11.5px;
  color: var(--fm-muted);
}

.cov-row {
  display: grid;
  grid-template-columns: 132px 62px 72px minmax(0, 1fr) 22px;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  padding: 3px 0;
}

.cov-name {
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cov-name em {
  margin-left: 5px;
  font-style: normal;
  font-size: 9.5px;
  color: var(--fm-muted);
}

.cov-status {
  font-size: 10px;
  text-align: center;
  padding: 1px 0;
  border-radius: var(--fm-radius-pill);
  border: 1px solid var(--fm-border);
  color: var(--fm-muted);
}

.cov-status.s-covered {
  color: var(--fm-primary);
  border-color: color-mix(in srgb, var(--fm-primary) 42%, var(--fm-border));
  background: var(--fm-primary-soft);
}

.cov-status.s-idle {
  color: var(--fm-text-2);
  background: var(--fm-surface-alt);
}

.cov-status.s-blind {
  color: var(--fm-warning);
  border-color: color-mix(in srgb, var(--fm-warning) 42%, var(--fm-border));
  background: var(--fm-warning-soft);
}

.cov-metric {
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
  white-space: nowrap;
}

.cov-detail {
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cov-reveal {
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: var(--fm-radius-sm);
  background: transparent;
  color: var(--fm-muted);
  cursor: pointer;
  font-size: 11px;
}

.cov-reveal:hover {
  color: var(--fm-primary);
  border-color: var(--fm-border);
  background: var(--fm-surface-alt);
}

.cov-absent {
  margin: 2px 0 0;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.cov-note {
  margin-top: 6px;
  padding: 10px 12px;
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-sm);
}

.cov-note-title {
  margin: 0 0 6px;
  font-size: 10.5px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.cov-note-row {
  display: flex;
  gap: 8px;
  margin: 0 0 3px;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.cov-note-row b {
  flex: none;
  width: 118px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.cov-note-foot {
  margin: 8px 0 0;
  font-size: 10px;
  line-height: 1.65;
  color: var(--fm-muted);
}

@media (max-width: 1080px) {
  .cov-row {
    grid-template-columns: 110px 56px 64px minmax(0, 1fr) 22px;
  }
}
</style>
