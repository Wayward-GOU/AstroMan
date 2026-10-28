<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import type { Dayjs } from 'dayjs'
import { ClockCircleOutlined, DeleteOutlined, ReloadOutlined, RobotOutlined, SearchOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { useTabsStore } from '@/stores/tabs'
import { formatDateTime } from '@/utils/datetime'
import { historyDiagnosePrompt } from '@/utils/aiScenarios'
import * as api from '@/api'
import type { CleanupResult, HistoryItem, HistoryStats } from '@/types/models'

const router = useRouter()
const tabs = useTabsStore()

const items = ref<HistoryItem[]>([])
const stats = ref<HistoryStats | null>(null)
const loading = ref(true)
const keyword = ref('')
const displayLimit = ref(500)

onMounted(load)

watch(displayLimit, () => {
  void load()
})

async function load() {
  loading.value = true
  try {
    const [list, overview] = await Promise.all([
      api.historyList(displayLimit.value),
      api.historyStats(),
    ])
    items.value = list
    stats.value = overview
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
  const total = stats.value?.total ?? items.value.length
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
  return { total, failed, avg, runs: stats.value?.runsTotal ?? 0 }
})

const aiAssist = useAiAssistStore()

function aiAnalyze() {
  if (!rows.value.length) {
    message.warning('当前没有可分析的历史记录')
    return
  }
  void aiAssist.show({
    title: '请求历史分析',
    prompt: historyDiagnosePrompt(rows.value, stats.value),
    hint: `当前筛选结果 ${rows.value.length} 条，失败 ${summary.value.failed} 条，平均耗时 ${summary.value.avg.toFixed(0)}ms`,
  })
}

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

function fmtDay(ts?: number | null) {
  if (!ts) return '-'
  return formatDateTime(ts).slice(0, 10)
}

function fmtMs(value?: number | null) {
  if (value === null || value === undefined) return '-'
  return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${value} ms`
}

/* ---------- 清理历史数据 ---------- */
type CleanupRange = '1d' | '7d' | '30d' | '90d' | 'all' | 'custom'
type CleanupScope = 'history' | 'runs'

const RANGE_DAYS: Record<Exclude<CleanupRange, 'all' | 'custom'>, number> = {
  '1d': 1,
  '7d': 7,
  '30d': 30,
  '90d': 90,
}

const cleanupOpen = ref(false)
const cleanupRange = ref<CleanupRange>('30d')
const customRange = ref<[Dayjs, Dayjs] | null>(null)
const cleanupScope = ref<CleanupScope[]>(['history'])
const cleanupVacuum = ref(true)
const preview = ref<CleanupResult | null>(null)
const previewLoading = ref(false)
const cleaning = ref(false)

const hasData = computed(() => (stats.value?.total ?? 0) + (stats.value?.runsTotal ?? 0) > 0)
const canClean = computed(
  () => cleanupScope.value.length > 0 && (cleanupRange.value !== 'custom' || !!customRange.value),
)

function rangeBounds(): { from: number | null; to: number | null } {
  if (cleanupRange.value === 'all') return { from: null, to: null }
  if (cleanupRange.value === 'custom') {
    if (!customRange.value) return { from: null, to: null }
    return { from: customRange.value[0].valueOf(), to: customRange.value[1].valueOf() }
  }
  const days = RANGE_DAYS[cleanupRange.value]
  return { from: Date.now() - days * 86_400_000, to: null }
}

const previewText = computed(() => {
  if (!canClean.value) return '请至少选择一项清理内容'
  if (previewLoading.value) return '正在统计…'
  if (!preview.value) return ''
  const parts: string[] = []
  if (cleanupScope.value.includes('history')) parts.push(`请求历史 ${preview.value.history} 条`)
  if (cleanupScope.value.includes('runs')) parts.push(`接口运行记录 ${preview.value.runs} 条`)
  if (!parts.length) return '请至少选择一项清理内容'
  return `将删除 ${parts.join('、')}`
})

async function refreshPreview() {
  if (!cleanupOpen.value || !canClean.value) {
    preview.value = null
    return
  }
  previewLoading.value = true
  try {
    const { from, to } = rangeBounds()
    preview.value = await api.dataCleanup({
      from,
      to,
      history: cleanupScope.value.includes('history'),
      runs: cleanupScope.value.includes('runs'),
      dryRun: true,
    })
  } catch {
    preview.value = null
  } finally {
    previewLoading.value = false
  }
}

watch([cleanupOpen, cleanupRange, customRange, cleanupScope], () => {
  void refreshPreview()
})

function openCleanup() {
  cleanupRange.value = '30d'
  customRange.value = null
  cleanupScope.value = ['history']
  cleanupVacuum.value = true
  preview.value = null
  cleanupOpen.value = true
}

async function runCleanup() {
  if (!canClean.value) return
  cleaning.value = true
  try {
    const { from, to } = rangeBounds()
    const result = await api.dataCleanup({
      from,
      to,
      history: cleanupScope.value.includes('history'),
      runs: cleanupScope.value.includes('runs'),
      vacuum: cleanupVacuum.value,
    })
    const parts: string[] = []
    if (cleanupScope.value.includes('history')) parts.push(`请求历史 ${result.history} 条`)
    if (cleanupScope.value.includes('runs')) parts.push(`接口运行记录 ${result.runs} 条`)
    message.success(`已清理 ${parts.join('、')}`)
    cleanupOpen.value = false
    await load()
  } catch (e) {
    message.error(String(e))
  } finally {
    cleaning.value = false
  }
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
      subtitle="本机保留最近 100000 条发送记录，默认展示最新 500 条，点击可跳回原接口；支持按时间范围清理"
    >
      <a-button :disabled="!rows.length" @click="aiAnalyze">
        <template #icon><RobotOutlined /></template>
        AI 分析
      </a-button>
      <a-button :loading="loading" @click="load">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
      <a-button danger :disabled="!hasData" @click="openCleanup">
        <template #icon><DeleteOutlined /></template>
        清理历史
      </a-button>
    </PageHeader>

    <div class="summary">
      <div class="summary-item">
        <span class="summary-label">请求历史总数</span>
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
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">接口运行记录</span>
        <span class="summary-value mono">{{ summary.runs }}</span>
      </div>
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">最早记录</span>
        <span class="summary-value mono date">{{ fmtDay(stats?.oldestAt) }}</span>
      </div>
    </div>

    <div class="toolbar">
      <a-input v-model:value="keyword" allow-clear placeholder="搜索接口名称、URL、方法或状态码" class="search">
        <template #prefix><SearchOutlined /></template>
      </a-input>
      <a-select v-model:value="displayLimit" class="limit-select">
        <a-select-option :value="500">展示最近 500 条</a-select-option>
        <a-select-option :value="1000">展示最近 1000 条</a-select-option>
        <a-select-option :value="2000">展示最近 2000 条</a-select-option>
        <a-select-option :value="5000">展示最近 5000 条</a-select-option>
      </a-select>
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

    <a-modal
      v-model:open="cleanupOpen"
      title="清理历史数据"
      :width="560"
      ok-text="开始清理"
      cancel-text="取消"
      :confirm-loading="cleaning"
      :ok-button-props="{ danger: true, disabled: !canClean || previewLoading }"
      @ok="runCleanup"
    >
      <div class="cleanup-tip">
        只清理本机保存的请求历史与接口运行记录，项目、集合、接口、环境变量与压测报告不受影响。
      </div>

      <div class="cleanup-field">
        <div class="cleanup-label">清理范围</div>
        <a-radio-group v-model:value="cleanupRange" button-style="solid" class="cleanup-radios">
          <a-radio-button value="1d">最近 1 天</a-radio-button>
          <a-radio-button value="7d">最近 7 天</a-radio-button>
          <a-radio-button value="30d">最近 30 天</a-radio-button>
          <a-radio-button value="90d">最近 90 天</a-radio-button>
          <a-radio-button value="all">全部清除</a-radio-button>
          <a-radio-button value="custom">自定义</a-radio-button>
        </a-radio-group>
        <a-range-picker
          v-if="cleanupRange === 'custom'"
          v-model:value="customRange"
          show-time
          :allow-clear="false"
          format="YYYY-MM-DD HH:mm:ss"
          class="cleanup-range"
        />
      </div>

      <div class="cleanup-field">
        <div class="cleanup-label">清理内容</div>
        <a-checkbox-group v-model:value="cleanupScope">
          <a-checkbox value="history">请求历史</a-checkbox>
          <a-checkbox value="runs">接口运行记录</a-checkbox>
        </a-checkbox-group>
      </div>

      <div class="cleanup-field">
        <a-checkbox v-model:checked="cleanupVacuum">清理后压缩数据库文件，释放磁盘空间（耗时稍长）</a-checkbox>
      </div>

      <div class="cleanup-preview">{{ previewText }}</div>
    </a-modal>
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

.summary-value.date {
  font-size: 13px;
  font-weight: 500;
}

.cleanup-tip {
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.7;
  margin-bottom: 16px;
}

.cleanup-field {
  margin-bottom: 14px;
}

.cleanup-label {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 8px;
}

.cleanup-radios :deep(.ant-radio-button-wrapper) {
  padding-inline: 12px;
}

.cleanup-range {
  display: block;
  width: 100%;
  margin-top: 10px;
}

.cleanup-preview {
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--fm-surface-2, rgba(127, 127, 127, 0.08));
  border: 1px dashed var(--fm-border);
  font-size: 12px;
  color: var(--fm-text-2);
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

.limit-select {
  width: 158px;
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
