<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Modal, message as notify } from 'ant-design-vue'
import { ArrowLeftOutlined, DeleteOutlined, ReloadOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { aiToolLogStats, aiUsageClear, aiUsageLogs, aiUsageStats } from '@/api'
import { toolLabel } from '@/utils/aiTools'
import { USD_TO_CNY, estimateCost, fmtCny, fmtUnit, fmtUsd } from '@/utils/aiPricing'
import type { AiToolLogStats, AiUsageLog, AiUsageStat, AiUsageStats } from '@/types/models'

const router = useRouter()

const stats = ref<AiUsageStats | null>(null)
const logs = ref<AiUsageLog[]>([])
const total = ref(0)
const loading = ref(false)
const statsLoading = ref(false)
const toolStats = ref<AiToolLogStats | null>(null)

const daysFilter = ref(30)
const page = ref(1)
const pageSize = ref(10)
const statusFilter = ref<'all' | 'ok' | 'fail'>('all')
const activeTab = ref<'logs' | 'models' | 'tools'>('logs')

/** 右侧详情抽屉 */
const detailOpen = ref(false)
const detail = ref<AiUsageLog | null>(null)

const dayOptions = [
  { label: '近 7 天', value: 7 },
  { label: '近 30 天', value: 30 },
  { label: '近 90 天', value: 90 },
  { label: '近 1 年', value: 365 },
]

const columns = [
  { title: '时间', key: 'createdAt', width: 168 },
  { title: '服务商', dataIndex: 'providerName', key: 'providerName', width: 130, ellipsis: true },
  { title: '模型', key: 'model', width: 170, ellipsis: true },
  { title: '本轮提问', key: 'request' },
  { title: '输入', key: 'prompt', width: 92, align: 'right' as const },
  { title: '输出', key: 'completion', width: 92, align: 'right' as const },
  { title: '费用', key: 'cost', width: 128, align: 'right' as const },
  { title: '耗时', key: 'latency', width: 88, align: 'right' as const },
  { title: '状态', key: 'status', width: 76 },
]

const modelColumns = [
  { title: '模型', key: 'model', width: 210 },
  { title: '服务商', dataIndex: 'providerName', key: 'providerName', width: 130, ellipsis: true },
  { title: '调用', key: 'calls', width: 88, align: 'right' as const },
  { title: '成功率', key: 'rate', width: 128, align: 'right' as const },
  { title: '工具成功率', key: 'toolRate', width: 128, align: 'right' as const },
  { title: '输入 token', key: 'prompt', width: 110, align: 'right' as const },
  { title: '输出 token', key: 'completion', width: 110, align: 'right' as const },
  { title: '费用', key: 'cost', width: 128, align: 'right' as const },
  { title: '平均耗时', key: 'latency', width: 100, align: 'right' as const },
  { title: '最近使用', key: 'last', width: 168, align: 'right' as const },
]

const toolColumns = [
  { title: '工具', key: 'tool' },
  { title: '调用次数', key: 'total', width: 110, align: 'right' as const },
  { title: '失败', key: 'failed', width: 90, align: 'right' as const },
  { title: '准确率', key: 'rate', width: 110, align: 'right' as const },
  { title: '平均耗时', key: 'avg', width: 120, align: 'right' as const },
]

/* ---------------- 数据加载 ---------------- */

async function loadLogs() {
  loading.value = true
  try {
    const result = await aiUsageLogs(
      daysFilter.value,
      page.value,
      pageSize.value,
      statusFilter.value === 'all' ? null : statusFilter.value === 'ok',
    )
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
    const [usage, tools] = await Promise.all([
      aiUsageStats(daysFilter.value),
      aiToolLogStats(daysFilter.value),
    ])
    stats.value = usage
    toolStats.value = tools
  } catch (error) {
    notify.error(String(error))
  } finally {
    statsLoading.value = false
  }
}

function refresh() {
  void loadLogs()
  void loadStats()
}

watch(statusFilter, () => {
  page.value = 1
  void loadLogs()
})

watch(daysFilter, () => {
  page.value = 1
  refresh()
})

watch(page, () => void loadLogs())

watch(pageSize, () => {
  if (page.value === 1) {
    void loadLogs()
  } else {
    page.value = 1
  }
})

/* ---------------- 费用估算 ---------------- */

function logCost(record: AiUsageLog) {
  return estimateCost(record.model, record.promptTokens, record.completionTokens)
}

function modelCost(record: AiUsageStat) {
  return estimateCost(record.model, record.promptTokens, record.completionTokens)
}

const totalCost = computed(() =>
  (stats.value?.items ?? []).reduce((sum, item) => sum + modelCost(item).total, 0),
)

/** 有兜底价的模型就提示「其中含估算」 */
const hasFallback = computed(() =>
  (stats.value?.items ?? []).some((item) => modelCost(item).rule.fallback),
)

const detailCost = computed(() => (detail.value ? logCost(detail.value) : null))

/* ---------------- 趋势 ---------------- */

const daily = computed(() => stats.value?.daily ?? [])

const series = computed(() => {
  const max = Math.max(1, ...daily.value.map((item) => item.totalTokens))
  const list = daily.value.map((item) => ({
    day: item.day,
    calls: item.calls,
    totalTokens: item.totalTokens,
    inPct: (item.promptTokens / max) * 100,
    outPct: (item.completionTokens / max) * 100,
    empty: item.totalTokens === 0,
    title: `${item.day} · ${item.calls} 次调用 · 输入 ${item.promptTokens} / 输出 ${item.completionTokens} token`,
  }))
  const step = Math.max(1, Math.ceil(list.length / 10))
  return list.map((item, index) => ({
    ...item,
    label: item.day.slice(5),
    showLabel: list.length <= 12 || index % step === 0 || index === list.length - 1,
  }))
})

/** 窗口较长时按周合并，避免柱子过密 */
const buckets = computed(() => {
  if (daysFilter.value <= 45) return series.value
  const groupSize = Math.ceil(series.value.length / 30)
  const list: typeof series.value = []
  for (let i = 0; i < series.value.length; i += groupSize) {
    const group = series.value.slice(i, i + groupSize)
    list.push({
      day: group[0]?.day ?? '',
      calls: group.reduce((sum, item) => sum + item.calls, 0),
      totalTokens: group.reduce((sum, item) => sum + item.totalTokens, 0),
      inPct: Math.max(...group.map((item) => item.inPct)),
      outPct: Math.max(...group.map((item) => item.outPct)),
      empty: group.every((item) => item.empty),
      title: group
        .map((item) => item.title)
        .join('\n')
        .slice(0, 800),
      label: group[group.length - 1].day.slice(5),
      showLabel: true,
    })
  }
  const step = Math.max(1, Math.ceil(list.length / 10))
  return list.map((item, index) => ({ ...item, showLabel: index % step === 0 || index === list.length - 1 }))
})

/* ---------------- 格式化 ---------------- */

function fmtFullTime(ms: number): string {
  if (!ms) return '—'
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function pct(value: number): string {
  if (!Number.isFinite(value)) return '—'
  return `${(value * 100).toFixed(1)}%`
}

function num(value: number): string {
  return (value || 0).toLocaleString()
}

function toolRate(record: { total: number; failed: number }): string {
  if (!record.total) return '—'
  return pct((record.total - record.failed) / record.total)
}

/** 提问预览压成一行，完整内容放抽屉和 title */
function oneLine(text: string): string {
  if (!text) return '—'
  return text.replace(/\s+/g, ' ').trim() || '—'
}

function openDetail(record: AiUsageLog) {
  detail.value = record
  detailOpen.value = true
}

function rowProps(record: AiUsageLog) {
  return {
    onClick: () => openDetail(record),
    style: 'cursor: pointer;',
  }
}

/* ---------------- 操作 ---------------- */

function clearUsage() {
  Modal.confirm({
    title: '清空全部用量记录？',
    content: '会删除近 1 年内的模型调用流水与费用记录，不影响会话消息。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      try {
        const affected = await aiUsageClear(true)
        notify.success(`已清理 ${affected} 条用量记录`)
        page.value = 1
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
  refresh()
})
</script>

<template>
  <div class="page usage-page">
    <PageHeader title="用量统计" subtitle="每次模型调用各记一条，含 token 与估算费用，保留近 1 年">
      <a-button size="small" @click="backToAi">
        <template #icon><ArrowLeftOutlined /></template>
        返回航仔
      </a-button>
      <a-button size="small" :loading="loading || statsLoading" @click="refresh">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
      <a-button size="small" danger :disabled="!total && !stats?.totalCalls" @click="clearUsage">
        <template #icon><DeleteOutlined /></template>
        清空记录
      </a-button>
    </PageHeader>

    <div class="usage-filter">
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
        <span class="sub-text">费用按厂商公开单价估算，汇率 $1 ≈ ¥{{ USD_TO_CNY }}</span>
        <a-select v-model:value="daysFilter" size="small" style="width: 116px" :options="dayOptions" />
      </div>
    </div>

    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-label">调用次数</div>
        <div class="stat-value">{{ num(stats?.totalCalls ?? 0) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">模型成功率</div>
        <div class="stat-value">{{ stats ? pct(stats.successRate) : '—' }}</div>
        <div class="stat-sub">成功 {{ num(stats?.okCalls ?? 0) }} 次</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">工具准确率</div>
        <div class="stat-value">{{ stats ? pct(stats.toolSuccessRate) : '—' }}</div>
        <div class="stat-sub">{{ num(stats?.toolOk ?? 0) }} / {{ num(stats?.toolCalls ?? 0) }} 次</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">总 token</div>
        <div class="stat-value">{{ num(stats?.totalTokens ?? 0) }}</div>
        <div class="stat-sub">
          输入 {{ num(stats?.promptTokens ?? 0) }} · 输出 {{ num(stats?.completionTokens ?? 0) }}
        </div>
      </div>
      <div class="stat-card">
        <div class="stat-label">估算费用</div>
        <div class="stat-value">{{ fmtUsd(totalCost) }}</div>
        <div class="stat-sub">
          ≈ {{ fmtCny(totalCost) }}
          <span v-if="hasFallback"> · 含默认价估算</span>
        </div>
      </div>
      <div class="stat-card">
        <div class="stat-label">平均耗时</div>
        <div class="stat-value">{{ num(stats?.avgLatencyMs ?? 0) }} ms</div>
      </div>
    </div>

    <div class="panel">
      <div class="panel-head">
        <span class="panel-title">每日 token 消耗</span>
        <span class="panel-sub">近 {{ daysFilter }} 天{{ daysFilter > 45 ? '（按周聚合）' : '' }}</span>
        <span class="legend">
          <i class="dot dot-in" />输入
          <i class="dot dot-out" />输出
        </span>
        <span class="flex-spacer" />
        <a-spin v-if="statsLoading" size="small" />
      </div>
      <div v-if="daily.length" class="chart">
        <div v-for="(bar, index) in buckets" :key="`${bar.day}-${index}`" class="chart-col" :title="bar.title">
          <div class="chart-track">
            <div class="chart-bar" :class="{ empty: bar.empty }">
              <span class="bar-in" :style="{ height: `${bar.inPct}%` }" />
              <span class="bar-out" :style="{ height: `${bar.outPct}%` }" />
            </div>
          </div>
          <span class="chart-tick">{{ bar.showLabel ? bar.label : '' }}</span>
        </div>
      </div>
      <div v-else class="panel-empty">还没有模型调用记录，和航仔 聊一句就会开始统计</div>
    </div>

    <div class="panel table-panel">
      <a-tabs v-model:active-key="activeTab" size="small" class="log-tabs">
        <a-tab-pane key="logs" :tab="`调用记录（${total}）`" />
        <a-tab-pane key="models" :tab="`模型汇总（${stats?.items.length ?? 0}）`" />
        <a-tab-pane key="tools" tab="工具准确率" />
      </a-tabs>

      <template v-if="activeTab === 'logs'">
        <a-table
          :data-source="logs"
          :columns="columns"
          row-key="id"
          size="small"
          :loading="loading"
          :pagination="false"
          :scroll="{ x: 1010 }"
          :custom-row="rowProps"
          :locale="{ emptyText: '当前筛选条件下没有调用记录' }"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'createdAt'">
              <span class="cell-time">{{ fmtFullTime(record.createdAt) }}</span>
            </template>
            <template v-else-if="column.key === 'model'">
              <div class="cell-tool">
                <span class="cell-tool-name">{{ record.model || '—' }}</span>
                <span class="cell-tool-id">{{ logCost(record).rule.label }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'request'">
              <span class="cell-text" :title="record.requestPreview">
                {{ oneLine(record.requestPreview) }}
              </span>
            </template>
            <template v-else-if="column.key === 'prompt'">
              <span class="cell-num">{{ num(record.promptTokens) }}</span>
            </template>
            <template v-else-if="column.key === 'completion'">
              <span class="cell-num">{{ num(record.completionTokens) }}</span>
            </template>
            <template v-else-if="column.key === 'cost'">
              <div class="cell-cost">
                <span>{{ fmtUsd(logCost(record).total) }}</span>
                <span class="cell-cost-sub">≈ {{ fmtCny(logCost(record).total) }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'latency'">
              <span class="cell-cost">{{ record.latencyMs }} ms</span>
            </template>
            <template v-else-if="column.key === 'status'">
              <a-tag :color="record.ok ? 'green' : 'red'">{{ record.ok ? '成功' : '失败' }}</a-tag>
            </template>
          </template>
        </a-table>

        <div class="pager">
          <a-pagination
            v-model:current="page"
            v-model:page-size="pageSize"
            :total="total"
            :page-size-options="[10, 20, 50, 100]"
            show-size-changer
            :show-total="(count: number) => `共 ${count} 条`"
          />
        </div>
      </template>

      <template v-else-if="activeTab === 'models'">
        <a-table
          :data-source="stats?.items ?? []"
          :columns="modelColumns"
          row-key="model"
          size="small"
          :loading="statsLoading"
          :pagination="false"
          :scroll="{ x: 1180 }"
          :locale="{ emptyText: '当前时间窗口内没有模型调用' }"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'model'">
              <div class="cell-tool">
                <span class="cell-tool-name">{{ record.model || '—' }}</span>
                <span class="cell-tool-id">{{ modelCost(record).rule.label }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'calls'">
              <span class="cell-num">{{ num(record.calls) }}</span>
            </template>
            <template v-else-if="column.key === 'rate'">
              <span :class="record.successRate >= 0.9 ? 'cell-num' : 'cell-num danger'">
                {{ pct(record.successRate) }}
              </span>
              <span class="sub-text"> ({{ record.okCalls }}/{{ record.calls }})</span>
            </template>
            <template v-else-if="column.key === 'toolRate'">
              <span v-if="record.toolCalls" class="cell-num">{{ pct(record.toolSuccessRate) }}</span>
              <span v-else class="sub-text">—</span>
            </template>
            <template v-else-if="column.key === 'prompt'">
              <span class="cell-num">{{ num(record.promptTokens) }}</span>
            </template>
            <template v-else-if="column.key === 'completion'">
              <span class="cell-num">{{ num(record.completionTokens) }}</span>
            </template>
            <template v-else-if="column.key === 'cost'">
              <div class="cell-cost">
                <span>{{ fmtUsd(modelCost(record).total) }}</span>
                <span class="cell-cost-sub">≈ {{ fmtCny(modelCost(record).total) }}</span>
              </div>
            </template>
            <template v-else-if="column.key === 'latency'">
              <span class="cell-cost">{{ record.avgLatencyMs }} ms</span>
            </template>
            <template v-else-if="column.key === 'last'">
              <span class="cell-time">{{ fmtFullTime(record.lastUsedAt) }}</span>
            </template>
          </template>
        </a-table>
      </template>

      <template v-else>
        <a-table
          :data-source="toolStats?.tools ?? []"
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

    <a-drawer
      v-model:open="detailOpen"
      title="调用详情"
      placement="right"
      :width="560"
      wrap-class-name="usage-detail-drawer"
    >
      <template v-if="detail && detailCost">
        <div class="detail-head">
          <div class="detail-title">
            <span class="detail-tool">{{ detail.model || '未知模型' }}</span>
            <span class="detail-id">{{ detail.providerName || '未配置服务商' }}</span>
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
            <span class="detail-meta-value">{{ detail.latencyMs }} ms</span>
          </div>
          <div class="detail-meta-item">
            <span class="detail-meta-label">记录 ID</span>
            <span class="detail-meta-value">#{{ detail.id }}</span>
          </div>
          <div class="detail-meta-item">
            <span class="detail-meta-label">价格档位</span>
            <span class="detail-meta-value">
              {{ detailCost.rule.label }}
              <span v-if="detailCost.rule.fallback" class="sub-text">（未匹配到公开价，按默认价估算）</span>
            </span>
          </div>
        </div>

        <div class="detail-block">
          <div class="detail-block-head">
            <span>本轮提问</span>
            <span class="detail-size">{{ detail.requestPreview.length }} 字符</span>
          </div>
          <pre>{{ detail.requestPreview || '（旧记录未保存提问内容）' }}</pre>
        </div>

        <div class="detail-block">
          <div class="detail-block-head"><span>token 与费用</span></div>
          <table class="cost-table">
            <thead>
              <tr>
                <th>项目</th>
                <th>数量</th>
                <th>单价</th>
                <th>费用</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>输入</td>
                <td>{{ num(detail.promptTokens) }}</td>
                <td>{{ fmtUnit(detailCost.rule.input) }}</td>
                <td>{{ fmtUsd(detailCost.inputCost) }}</td>
              </tr>
              <tr>
                <td>输出</td>
                <td>{{ num(detail.completionTokens) }}</td>
                <td>{{ fmtUnit(detailCost.rule.output) }}</td>
                <td>{{ fmtUsd(detailCost.outputCost) }}</td>
              </tr>
              <tr class="total-row">
                <td>合计</td>
                <td>{{ num(detail.totalTokens) }}</td>
                <td>—</td>
                <td>{{ fmtUsd(detailCost.total) }} <span class="sub-text">≈ {{ fmtCny(detailCost.total) }}</span></td>
              </tr>
            </tbody>
          </table>
        </div>

        <div v-if="!detail.ok" class="detail-block">
          <div class="detail-block-head">
            <span>错误信息</span>
            <span class="detail-size">{{ detail.error.length }} 字符</span>
          </div>
          <pre>{{ detail.error || '（未记录错误详情）' }}</pre>
        </div>
      </template>
    </a-drawer>
  </div>
</template>

<style scoped>
.usage-page {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

.usage-filter {
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

.sub-text {
  font-size: 11px;
  color: var(--fm-muted);
}

/* ---------- 指标卡 ---------- */

.stat-grid {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
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
  font-size: 21px;
  font-weight: 600;
  letter-spacing: -0.3px;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
}

.stat-sub {
  margin-top: 2px;
  font-size: 10.5px;
  color: var(--fm-muted);
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

.dot-in {
  background: var(--fm-primary);
}

.dot-out {
  background: #a855f7;
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

.bar-in {
  display: block;
  width: 100%;
  background: linear-gradient(180deg, color-mix(in srgb, var(--fm-primary) 78%, #fff), var(--fm-primary));
}

.bar-out {
  display: block;
  width: 100%;
  background: linear-gradient(180deg, #c4b5fd, #a855f7);
  opacity: 0.9;
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
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.cell-tool-id {
  font-size: 10px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  color: var(--fm-muted);
}

.cell-cost {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 1px;
  font-size: 11.5px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.cell-cost-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

.cell-num {
  font-variant-numeric: tabular-nums;
}

.cell-text {
  display: block;
  max-width: 320px;
  font-size: 11.5px;
  color: var(--fm-text-2);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.cell-num.danger {
  color: var(--fm-danger);
  font-weight: 600;
}

.pager {
  display: flex;
  justify-content: flex-end;
  padding-top: 10px;
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
  max-height: 260px;
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

.cost-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  overflow: hidden;
}

.cost-table th,
.cost-table td {
  padding: 7px 10px;
  text-align: right;
  border-bottom: 1px solid var(--fm-border);
  font-variant-numeric: tabular-nums;
}

.cost-table th:first-child,
.cost-table td:first-child {
  text-align: left;
}

.cost-table thead th {
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 11.5px;
  font-weight: 500;
}

.cost-table tbody tr:last-child td {
  border-bottom: none;
}

.cost-table .total-row td {
  font-weight: 600;
  color: var(--fm-text);
}

@media (max-width: 1200px) {
  .stat-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}

@media (max-width: 900px) {
  .stat-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
