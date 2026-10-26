<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { ClockCircleOutlined, DeleteOutlined, ReloadOutlined, SearchOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useTabsStore } from '@/stores/tabs'
import { formatDateTime } from '@/utils/datetime'
import * as api from '@/api'
import type { HistoryItem } from '@/types/models'

const router = useRouter()
const tabs = useTabsStore()

const items = ref<HistoryItem[]>([])
const loading = ref(true)
const keyword = ref('')

onMounted(load)

async function load() {
  loading.value = true
  try {
    items.value = await api.historyList(500)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

const rows = computed(() => {
  const word = keyword.value.trim().toLowerCase()
  if (!word) return items.value
  return items.value.filter((item) =>
    [item.requestName, item.method, item.url, item.statusCode].some((field) =>
      String(field ?? '')
        .toLowerCase()
        .includes(word),
    ),
  )
})

const summary = computed(() => {
  const total = items.value.length
  const failed = items.value.filter(
    (item) => item.statusCode === null || item.statusCode === undefined || item.statusCode >= 400,
  ).length
  const durations = items.value
    .map((item) => item.durationMs ?? 0)
    .filter((value) => value > 0)
    .sort((a, b) => a - b)
  const avg = durations.length
    ? durations.reduce((sum, value) => sum + value, 0) / durations.length
    : 0
  return { total, failed, avg }
})

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

function fmtMs(value?: number | null) {
  if (value === null || value === undefined) return '-'
  return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${value} ms`
}

function clearAll() {
  Modal.confirm({
    title: '清空全部请求历史？',
    content: '只会删除本机保存的历史记录，项目与接口配置不受影响。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.historyClear()
      await load()
      message.success('已清空请求历史')
    },
  })
}

async function openItem(item: HistoryItem) {
  if (!item.requestId) {
    message.info('该记录对应的接口已被删除')
    return
  }
  tabs.ensureProject(item.projectId ?? 0)
  router.push({
    name: 'request',
    params: { id: item.requestId },
    query: item.projectId ? { projectId: item.projectId } : {},
  })
}

const columns = [
  { title: '时间', dataIndex: 'createdAt', key: 'createdAt', width: 158 },
  { title: '方法', dataIndex: 'method', key: 'method', width: 84 },
  { title: '接口', dataIndex: 'requestName', key: 'requestName', width: 180, ellipsis: true },
  { title: 'URL', dataIndex: 'url', key: 'url', ellipsis: true },
  { title: '状态码', dataIndex: 'statusCode', key: 'statusCode', width: 88 },
  { title: '耗时', dataIndex: 'durationMs', key: 'durationMs', width: 96 },
]
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="History"
      title="请求历史"
      subtitle="本机保留最近 5000 条发送记录，此处展示最新 500 条，点击可跳回原接口"
    >
      <a-button :loading="loading" @click="load">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
      <a-button danger :disabled="!items.length" @click="clearAll">
        <template #icon><DeleteOutlined /></template>
        清空历史
      </a-button>
    </PageHeader>

    <div class="summary">
      <div class="summary-item">
        <span class="summary-label">记录总数</span>
        <span class="summary-value mono">{{ summary.total }}</span>
      </div>
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">失败 / 异常</span>
        <span class="summary-value mono">{{ summary.failed }}</span>
      </div>
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">平均耗时</span>
        <span class="summary-value mono">{{ summary.avg.toFixed(0) }}<i>ms</i></span>
      </div>
    </div>

    <div class="toolbar">
      <a-input v-model:value="keyword" allow-clear placeholder="搜索接口名称、URL、方法或状态码" class="search">
        <template #prefix><SearchOutlined /></template>
      </a-input>
      <span class="hint">
        <ClockCircleOutlined />
        共 {{ rows.length }} 条
      </span>
    </div>

    <a-spin :spinning="loading">
      <div class="card table-wrap">
        <a-table
          v-if="rows.length"
          :data-source="rows"
          :columns="columns"
          :pagination="{ pageSize: 50, showSizeChanger: false }"
          row-key="id"
          size="middle"
          :custom-row="(record: HistoryItem) => ({ onClick: () => openItem(record), style: 'cursor:pointer' })"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'createdAt'">
              <span class="muted">{{ fmtTime((record as HistoryItem).createdAt) }}</span>
            </template>
            <template v-else-if="column.key === 'method'">
              <span class="method-tag" :class="`method-${(record as HistoryItem).method}`">
                {{ (record as HistoryItem).method }}
              </span>
            </template>
            <template v-else-if="column.key === 'requestName'">
              {{ (record as HistoryItem).requestName || '临时请求' }}
            </template>
            <template v-else-if="column.key === 'url'">
              <span class="mono url">{{ (record as HistoryItem).url }}</span>
            </template>
            <template v-else-if="column.key === 'statusCode'">
              <span
                class="status"
                :class="{
                  ok: ((record as HistoryItem).statusCode ?? 0) < 400 && ((record as HistoryItem).statusCode ?? 0) > 0,
                  bad: !((record as HistoryItem).statusCode ?? 0) || ((record as HistoryItem).statusCode ?? 0) >= 400,
                }"
              >
                {{ (record as HistoryItem).statusCode ?? 'ERR' }}
              </span>
            </template>
            <template v-else-if="column.key === 'durationMs'">
              <span class="mono">{{ fmtMs((record as HistoryItem).durationMs) }}</span>
            </template>
          </template>
        </a-table>

        <div v-else class="empty">
          <div class="empty-icon">⏱</div>
          <div class="empty-title">{{ keyword.trim() ? '没有匹配的历史记录' : '还没有发送过请求' }}</div>
          <div class="empty-desc">
            {{ keyword.trim() ? '换个关键词试试' : '在接口编辑器里点击「发送」后，记录会自动出现在这里' }}
          </div>
        </div>
      </div>
    </a-spin>
  </div>
</template>

<style scoped>
.summary {
  display: flex;
  align-items: center;
  gap: 22px;
  padding: 14px 20px;
  margin-bottom: 14px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow);
}

.summary-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.summary-label {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.summary-value {
  font-size: 18px;
  font-weight: 600;
  letter-spacing: -0.2px;
}

.summary-value i {
  font-style: normal;
  font-size: 11px;
  color: var(--fm-muted);
  margin-left: 2px;
}

.summary-divider {
  width: 1px;
  height: 26px;
  background: var(--fm-border);
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.search {
  max-width: 320px;
}

.hint {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--fm-muted);
}

.card {
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow);
  overflow: hidden;
}

.method-tag {
  font-size: 10.5px;
  font-weight: 600;
}

.method-GET {
  color: var(--fm-success);
}

.method-POST {
  color: var(--fm-warning);
}

.method-PUT {
  color: var(--fm-primary);
}

.method-PATCH {
  color: var(--fm-purple);
}

.method-DELETE {
  color: var(--fm-danger);
}

.url {
  font-size: 11.5px;
  color: var(--fm-text-2);
}

.status {
  font-size: 11px;
  font-weight: 600;
}

.status.ok {
  color: var(--fm-success);
}

.status.bad {
  color: var(--fm-danger);
}

.empty {
  padding: 72px 24px;
  text-align: center;
}

.empty-icon {
  font-size: 26px;
  margin-bottom: 10px;
  opacity: 0.5;
}

.empty-title {
  font-size: 13px;
  font-weight: 600;
}

.empty-desc {
  font-size: 11.5px;
  color: var(--fm-muted);
  margin-top: 6px;
}
</style>
