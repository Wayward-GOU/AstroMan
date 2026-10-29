<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Modal, message as notify } from 'ant-design-vue'
import { ArrowLeftOutlined, DeleteOutlined, ReloadOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { aiSessionList, aiToolLogClear, aiToolLogList, aiToolLogStats } from '@/api'
import { TOOL_LABELS, toolLabel } from '@/utils/aiTools'
import type { AiSession, AiToolDayStat, AiToolLog, AiToolLogStats } from '@/types/models'

const router = useRouter()

const sessions = ref<AiSession[]>([])
const logs = ref<AiToolLog[]>([])
const total = ref(0)
const loading = ref(false)
const stats = ref<AiToolLogStats | null>(null)
const statsLoading = ref(false)

const page = ref(1)
const pageSize = ref(20)
const statusFilter = ref<'all' | 'ok' | 'fail'>('all')
const toolFilter = ref<string | undefined>(undefined)
const sessionFilter = ref<number | undefined>(undefined)
const daysFilter = ref(30)
const activeTab = ref<'logs' | 'tools'>('logs')

/** 右侧详情抽屉 */
const detailOpen = ref(false)
const detail = ref<AiToolLog | null>(null)

function openDetail(record: AiToolLog) {
  detail.value = record
  detailOpen.value = true
}

/** 行点击整行打开抽屉 */
function rowProps(record: AiToolLog) {
  return {
    onClick: () => openDetail(record),
    style: 'cursor: pointer;',
  }
}

const dayOptions = [
  { label: '近 7 天', value: 7 },
  { label: '近 30 天', value: 30 },
  { label: '近 90 天', value: 90 },
  { label: '近 1 年', value: 365 },
]

const toolOptions = Object.entries(TOOL_LABELS).map(([value, label]) => ({ value, label }))

const sessionOptions = computed(() =>
  sessions.value.map((item) => ({ value: item.id, label: item.title })),
)

const columns = [
  { title: '时间', key: 'createdAt', width: 148 },
  { title: '会话', key: 'session', width: 200 },
  { title: '工具', key: 'tool', width: 190 },
  { title: '状态', key: 'status', width: 80 },
  { title: '耗时', key: 'durationMs', width: 92, align: 'right' as const },
  { title: '请求参数', key: 'args' },
  { title: '返回结果', key: 'result' },
]

const toolColumns = [
  { title: '工具', key: 'tool' },
  { title: '调用次数', key: 'total', width: 110, align: 'right' as const },
  { title: '失败', key: 'failed', width: 90, align: 'right' as const },
  { title: '成功率', key: 'rate', width: 110, align: 'right' as const },
  { title: '平均耗时', key: 'avg', width: 120, align: 'right' as const },
]

/* ---------------- 数据加载 ---------------- */

async function loadLogs() {
  loading.value = true
  try {
    const result = await aiToolLogList({
      sessionId: sessionFilter.value ?? null,
      tool: toolFilter.value ?? null,
      ok: statusFilter.value === 'all' ? null : statusFilter.value === 'ok',
      days: daysFilter.value,
      page: page.value,
      pageSize: pageSize.value,
    })
    logs.value = result.items
    total.value = result.total
  } catch (error) {
    notify.error(String(error))
  } finally {
    loading.value = false
  }
}

async function loadStats() {
  statsLoading.value = true
  try {
    stats.value = await aiToolLogStats(daysFilter.value)
  } catch (error) {
    notify.error(String(error))
  } finally {
    statsLoading.value = false
  }
}

async function loadSessions() {
  try {
    sessions.value = await aiSessionList()
  } catch {
    sessions.value = []
  }
}

function refresh() {
  void loadLogs()
  void loadStats()
}

function resetPage() {
  page.value = 1
}

watch([statusFilter, toolFilter, sessionFilter], () => {
  resetPage()
  void loadLogs()
})

watch(daysFilter, () => {
  resetPage()
  refresh()
})

watch(page, () => void loadLogs())

watch(pageSize, () => {
  if (page.value === 1) {
    void loadLogs()
  } else {
    resetPage()
  }
})

/* ---------------- 汇总 ---------------- */

const successRate = computed(() => {
  if (!stats.value || !stats.value.total) return '—'
  return `${((stats.value.ok / stats.value.total) * 100).toFixed(1)}%`
})

const avgCost = computed(() => {
  if (!stats.value) return '—'
  return `${stats.value.avgDurationMs} ms`
})

/** 按窗口生成趋势柱：窗口超过 60 天时按周聚合，保证柱子数量适中 */
const series = computed(() => {
  const rows = stats.value?.days ?? []
  const days = daysFilter.value
  const bucket = Math.max(1, Math.ceil(days / 30))
  const step = bucket * 86_400_000
  const map = new Map<string, AiToolDayStat>(rows.map((row) => [row.date, row]))
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  const buckets = Math.ceil(days / bucket)
  const list: { label: string; title: string; total: number; failed: number }[] = []

  for (let index = buckets - 1; index >= 0; index -= 1) {
    const end = new Date(today.getTime() - index * step)
    let totalCount = 0
    let failedCount = 0
    for (let offset = 0; offset < bucket; offset += 1) {
      const day = new Date(end.getTime() - offset * 86_400_000)
      const row = map.get(dateKey(day))
      if (row) {
        totalCount += row.total
        failedCount += row.failed
      }
    }
    const start = new Date(end.getTime() - (bucket - 1) * 86_400_000)
    const range = bucket === 1 ? dateKey(end).slice(5) : `${dateKey(start).slice(5)} ~ ${dateKey(end).slice(5)}`
    list.push({ label: `${end.getMonth() + 1}/${end.getDate()}`, title: `${range} · ${totalCount} 次`, total: totalCount, failed: failedCount })
  }

  const max = Math.max(1, ...list.map((item) => item.total))
  return list.map((item, index) => ({
    ...item,
    showLabel: list.length <= 12 || index % Math.ceil(list.length / 10) === 0,
    okPct: ((item.total - item.failed) / max) * 100,
    failPct: (item.failed / max) * 100,
    empty: item.total === 0,
  }))
})

function dateKey(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function fmtFullTime(ms: number): string {
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function sessionTitle(id: number | null | undefined): string {
  if (!id) return '—'
  return sessions.value.find((item) => item.id === id)?.title ?? `#${id}`
}

function oneLine(value: string | null | undefined): string {
  if (!value) return '—'
  const text = value.replace(/\s+/g, ' ').trim()
  return text || '—'
}

function toolRate(record: { total: number; failed: number }): string {
  if (!record.total) return '—'
  return `${(((record.total - record.failed) / record.total) * 100).toFixed(1)}%`
}

/* ---------------- 操作 ---------------- */

function clearLogs() {
  Modal.confirm({
    title: '清空全部工具调用记录？',
    content: '只删除航仔 的工具调用流水，不会影响会话消息。记录默认保留近 1 年，超出部分会自动清理。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      try {
        const affected = await aiToolLogClear(null, null)
        notify.success(`已清理 ${affected} 条记录`)
        refresh()
      } catch (error) {
        notify.error(String(error))
      }
    },
  })
}

function backToAi() {
  void router.push({ name: 'ai' })
}

onMounted(() => {
  void loadSessions()
  refresh()
})
</script>

<template>
  <div class="page logs-page">
    <PageHeader title="工具调用记录" subtitle="航仔 每次调用系统功能都会留下一条流水，保留近 1 年">
      <a-button size="small" @click="backToAi">
        <template #icon><ArrowLeftOutlined /></template>
        返回航仔
      </a-button>
      <a-button size="small" :loading="loading || statsLoading" @click="refresh">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
      <a-button size="small" danger :disabled="!total && !stats?.total" @click="clearLogs">
        <template #icon><DeleteOutlined /></template>
        清空记录
      </a-button>
    </PageHeader>

    <!-- 状态筛选 + 维度筛选 -->
    <div class="log-filter">
      <div class="status-chips">
        <button
          v-for="item in [
            { key: 'all', label: '全部' },
            { key: 'ok', label: '成功' },
            { key: 'fail', label: '失败' },
          ]"
          :key="item.key"
          class="chip"
          :class="{ active: statusFilter === item.key }"
          type="button"
          @click="statusFilter = item.key as 'all' | 'ok' | 'fail'"
        >
          {{ item.label }}
        </button>
      </div>
      <div class="filter-right">
        <a-select
          v-model:value="toolFilter"
          size="small"
          style="width: 160px"
          placeholder="全部工具"
          allow-clear
          :options="toolOptions"
        />
        <a-select
          v-model:value="sessionFilter"
          size="small"
          style="width: 170px"
          placeholder="全部会话"
          allow-clear
          :options="sessionOptions"
        />
        <a-select v-model:value="daysFilter" size="small" style="width: 116px" :options="dayOptions" />
      </div>
    </div>

    <!-- 关键指标 -->
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-label">总调用次数</div>
        <div class="stat-value">{{ stats?.total ?? '—' }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">成功率</div>
        <div class="stat-value">{{ successRate }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">平均耗时</div>
        <div class="stat-value">{{ avgCost }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">近 24 小时</div>
        <div class="stat-value">{{ stats?.last24h ?? '—' }}</div>
      </div>
    </div>

    <!-- 趋势 -->
    <div class="panel">
      <div class="panel-head">
        <span class="panel-title">使用趋势</span>
        <span class="panel-sub">近 {{ daysFilter }} 天</span>
        <span class="legend">
          <i class="dot dot-ok" />调用
          <i class="dot dot-fail" />失败
        </span>
        <span class="flex-spacer" />
        <a-spin v-if="statsLoading" size="small" />
      </div>
      <div v-if="stats?.total" class="chart">
        <div v-for="(bar, index) in series" :key="index" class="chart-col" :title="bar.title">
          <div class="chart-track">
            <div class="chart-bar" :class="{ empty: bar.empty }">
              <span class="bar-ok" :style="{ height: `${bar.okPct}%` }" />
              <span class="bar-fail" :style="{ height: `${bar.failPct}%` }" />
            </div>
          </div>
          <span v-if="bar.showLabel" class="chart-tick">{{ bar.label }}</span>
        </div>
      </div>
      <div v-else class="panel-empty">暂无可统计的调用，和航仔 聊一句并让它执行一个系统操作试试</div>
    </div>

    <!-- 明细 / 分工具 -->
    <div class="panel table-panel">
      <a-tabs v-model:active-key="activeTab" size="small" class="log-tabs">
        <a-tab-pane key="logs" tab="调用日志" />
        <a-tab-pane key="tools" tab="工具分布" />
      </a-tabs>

      <template v-if="activeTab === 'logs'">
        <a-table
          :data-source="logs"
          :columns="columns"
          row-key="id"
          size="small"
          :loading="loading"
          :pagination="false"
          :scroll="{ x: 1020 }"
          :custom-row="rowProps"
          :locale="{ emptyText: '当前筛选条件下没有记录' }"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'createdAt'">
              <span class="cell-time">{{ fmtFullTime(record.createdAt) }}</span>
            </template>
            <template v-else-if="column.key === 'session'">
              <span class="cell-session">{{ sessionTitle(record.sessionId) }}</span>
            </template>
            <template v-else-if="column.key === 'tool'">
              <div class="cell-tool">
                <span class="cell-tool-name">{{ toolLabel(record.tool) }}</span>
                <span class="cell-tool-id">{{ record.tool }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'status'">
              <a-tag :color="record.ok ? 'green' : 'red'">{{ record.ok ? '成功' : '失败' }}</a-tag>
            </template>
            <template v-else-if="column.key === 'durationMs'">
              <span class="cell-cost">{{ record.durationMs }} ms</span>
            </template>
            <template v-else-if="column.key === 'args'">
              <span class="cell-text" :title="record.args">{{ oneLine(record.args) }}</span>
            </template>
            <template v-else-if="column.key === 'result'">
              <span class="cell-text" :title="record.resultPreview">
                {{ oneLine(record.resultPreview) }}
              </span>
            </template>
          </template>
        </a-table>

        <div class="pager">
          <a-pagination
            v-model:current="page"
            v-model:page-size="pageSize"
            :total="total"
            :page-size-options="[20, 50, 100]"
            show-size-changer
            :show-total="(count: number) => `共 ${count} 条`"
          />
        </div>
      </template>

      <template v-else>
        <a-table
          :data-source="stats?.tools ?? []"
          :columns="toolColumns"
          row-key="tool"
          size="small"
          :pagination="false"
          :locale="{ emptyText: '当前时间窗口内没有工具调用' }"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'tool'">
              <div class="cell-tool">
                <span class="cell-tool-name">{{ toolLabel(record.tool) }}</span>
                <span class="cell-tool-id">{{ record.tool }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'total'">
              <span class="cell-num">{{ record.total }}</span>
            </template>
            <template v-else-if="column.key === 'failed'">
              <span class="cell-num" :class="{ danger: record.failed > 0 }">{{ record.failed }}</span>
            </template>
            <template v-else-if="column.key === 'rate'">
              {{ toolRate(record) }}
            </template>
            <template v-else-if="column.key === 'avg'">
              <span class="cell-cost">{{ record.avgDurationMs }} ms</span>
            </template>
          </template>
        </a-table>
      </template>
    </div>

    <!-- 单条记录详情 -->
    <a-drawer
      v-model:open="detailOpen"
      title="调用详情"
      placement="right"
      :width="560"
      wrap-class-name="log-detail-drawer"
    >
      <template v-if="detail">
        <div class="detail-head">
          <div class="detail-title">
            <span class="detail-tool">{{ toolLabel(detail.tool) }}</span>
            <span class="detail-id">{{ detail.tool }}</span>
          </div>
          <a-tag :color="detail.ok ? 'green' : 'red'">{{ detail.ok ? '成功' : '失败' }}</a-tag>
        </div>

        <div class="detail-meta">
          <div class="detail-meta-item">
            <span class="detail-meta-label">时间</span>
            <span class="detail-meta-value">{{ fmtFullTime(detail.createdAt) }}</span>
          </div>
          <div class="detail-meta-item">
            <span class="detail-meta-label">耗时</span>
            <span class="detail-meta-value">{{ detail.durationMs }} ms</span>
          </div>
          <div class="detail-meta-item">
            <span class="detail-meta-label">会话</span>
            <span class="detail-meta-value ellipsis">{{ sessionTitle(detail.sessionId) }}</span>
          </div>
          <div class="detail-meta-item">
            <span class="detail-meta-label">记录 ID</span>
            <span class="detail-meta-value">#{{ detail.id }}</span>
          </div>
        </div>

        <div class="detail-block">
          <div class="detail-block-head">
            <span>请求参数</span>
            <span class="detail-size">{{ detail.args.length }} 字符</span>
          </div>
          <pre>{{ detail.args || '{}' }}</pre>
        </div>

        <div class="detail-block">
          <div class="detail-block-head">
            <span>返回结果</span>
            <span class="detail-size">{{ detail.resultPreview.length }} 字符</span>
          </div>
          <pre>{{ detail.resultPreview || '（无输出）' }}</pre>
        </div>
      </template>
    </a-drawer>
  </div>
</template>

<style scoped>
.logs-page {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

/* ---------- 筛选 ---------- */

.log-filter {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 12px;
}

.status-chips {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px;
  border-radius: 8px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.chip {
  border: none;
  background: transparent;
  color: var(--fm-muted);
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.chip:hover {
  color: var(--fm-text);
}

.chip.active {
  background: var(--fm-surface);
  color: var(--fm-text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.08);
}

.filter-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

/* ---------- 指标卡 ---------- */

.stat-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
  margin-bottom: 10px;
}

.stat-card {
  padding: 10px 14px 12px;
  border-radius: 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
}

.stat-label {
  font-size: 11.5px;
  color: var(--fm-muted);
}

.stat-value {
  margin-top: 4px;
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.3px;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
}

/* ---------- 面板 ---------- */

.panel {
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  margin-bottom: 10px;
}

.panel-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}

.panel-title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
}

.panel-sub {
  font-size: 11px;
  color: var(--fm-muted);
}

.legend {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10.5px;
  color: var(--fm-muted);
  margin-left: 6px;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 2px;
  display: inline-block;
}

.dot-ok {
  background: var(--fm-primary);
}

.dot-fail {
  background: color-mix(in srgb, var(--fm-danger) 80%, transparent);
  margin-left: 6px;
}

.flex-spacer {
  flex: 1;
}

.panel-empty {
  padding: 26px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

/* ---------- 趋势柱 ---------- */

.chart {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 132px;
  padding: 0 2px;
}

.chart-col {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  align-items: center;
  gap: 4px;
}

.chart-track {
  width: 100%;
  flex: 1;
  display: flex;
  align-items: flex-end;
  justify-content: center;
}

.chart-bar {
  width: 100%;
  max-width: 26px;
  height: 100%;
  display: flex;
  flex-direction: column-reverse;
  border-radius: 3px 3px 0 0;
  overflow: hidden;
}

.chart-bar.empty {
  background: color-mix(in srgb, var(--fm-border) 55%, transparent);
  height: 3px;
  align-self: flex-end;
}

.bar-ok {
  display: block;
  width: 100%;
  background: linear-gradient(180deg, color-mix(in srgb, var(--fm-primary) 78%, #fff), var(--fm-primary));
  border-radius: 3px 3px 0 0;
  min-height: 0;
}

.bar-fail {
  display: block;
  width: 100%;
  background: var(--fm-danger);
  opacity: 0.8;
}

.chart-tick {
  font-size: 9.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

/* ---------- 表格 ---------- */

.table-panel {
  padding-top: 4px;
}

.log-tabs :deep(.ant-tabs-nav) {
  margin-bottom: 6px;
}

.log-tabs :deep(.ant-tabs-tab) {
  font-size: 12px;
  padding: 6px 0;
}

:deep(.ant-table) {
  background: transparent;
  font-size: 12px;
}

:deep(.ant-table-thead > tr > th) {
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 11.5px;
  font-weight: 500;
  padding: 7px 10px;
  border-bottom: 1px solid var(--fm-border);
}

:deep(.ant-table-thead > tr > th::before) {
  display: none;
}

:deep(.ant-table-tbody > tr > td) {
  padding: 7px 10px;
  border-bottom: 1px solid var(--fm-border);
  vertical-align: middle;
}

:deep(.ant-table-tbody > tr:hover > td) {
  background: var(--fm-surface-alt);
}

:deep(.ant-table-expanded-row > td) {
  background: transparent;
  padding: 8px 12px 10px;
}

:deep(.ant-table-placeholder > td) {
  border-bottom: none;
}

:deep(.ant-table-cell .ant-tag) {
  margin-inline-end: 0;
  font-size: 11px;
  line-height: 17px;
  padding: 0 5px;
}

.cell-time {
  font-size: 11.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.cell-session {
  display: block;
  font-size: 12px;
  color: var(--fm-text-2);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.cell-tool {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.cell-tool-name {
  font-size: 12.5px;
  font-weight: 500;
  color: var(--fm-text);
  white-space: nowrap;
}

.cell-tool-id {
  font-size: 10px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  color: var(--fm-muted);
}

.cell-cost {
  font-size: 11.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.cell-num {
  font-variant-numeric: tabular-nums;
}

.cell-num.danger {
  color: var(--fm-danger);
  font-weight: 600;
}

.cell-text {
  display: block;
  font-size: 11px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  color: var(--fm-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

/* ---------- 详情抽屉 ---------- */

.detail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--fm-border);
}

.detail-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.detail-tool {
  font-size: 15px;
  font-weight: 600;
  color: var(--fm-text);
}

.detail-id {
  font-size: 11px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  color: var(--fm-muted);
}

.detail-meta {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px 14px;
  padding: 12px 0 14px;
  border-bottom: 1px solid var(--fm-border);
  margin-bottom: 12px;
}

.detail-meta-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.detail-meta-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.detail-meta-value {
  font-size: 12.5px;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.detail-meta-value.ellipsis {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.detail-block {
  margin-bottom: 14px;
}

.detail-block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.detail-size {
  font-size: 10.5px;
  font-weight: 400;
  color: var(--fm-muted);
}

.detail-block pre {
  margin: 0;
  max-height: 320px;
  overflow: auto;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.pager {
  display: flex;
  justify-content: flex-end;
  padding-top: 10px;
}

@media (max-width: 900px) {
  .stat-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
