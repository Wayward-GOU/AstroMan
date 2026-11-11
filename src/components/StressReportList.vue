<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import {
  FundProjectionScreenOutlined,
  ArrowRightOutlined,
  DeleteOutlined,
  SwapOutlined,
} from '@ant-design/icons-vue'
import { formatDateTime } from '@/utils/datetime'
import * as api from '@/api'
import type { StressReport } from '@/types/models'

const router = useRouter()
const reports = ref<StressReport[]>([])
const loading = ref(true)

async function load() {
  loading.value = true
  try {
    reports.value = await api.reportList(200)
  } catch (error) {
    message.error(String(error))
  } finally {
    loading.value = false
  }
}

onMounted(load)
defineExpose({ reload: load })

const stats = computed(() => {
  const total = reports.value.length
  const requests = reports.value.reduce((sum, item) => sum + item.totalRequests, 0)
  const success = reports.value.reduce((sum, item) => sum + item.successCount, 0)
  return { total, requests, rate: requests > 0 ? (success / requests) * 100 : 0 }
})

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

function fmtNum(value: number, digits = 1) {
  return value >= 1000 ? value.toFixed(0) : value.toFixed(digits)
}

function remove(report: StressReport) {
  Modal.confirm({
    title: `删除报告「${report.planName}」？`,
    content: '报告以及对应的每秒采样与错误明细都会被删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.reportDelete(report.id)
      selectedIds.value = selectedIds.value.filter((id) => id !== report.id)
      await load()
      message.success('已删除')
    },
  })
}

/* ---------- 对比 ---------- */
const selectedIds = ref<number[]>([])
const compareOpen = ref(false)

function toggleSelect(id: number, checked: boolean) {
  if (checked) {
    if (selectedIds.value.length >= 4) {
      message.warning('最多选择 4 份报告对比')
      return
    }
    selectedIds.value = [...selectedIds.value, id]
  } else {
    selectedIds.value = selectedIds.value.filter((item) => item !== id)
  }
}

const compareReports = computed(() =>
  selectedIds.value
    .map((id) => reports.value.find((item) => item.id === id))
    .filter((item): item is StressReport => Boolean(item)),
)

function compareMetricRows() {
  const list = compareReports.value
  if (!list.length) return []
  const percent = (value: number) => `${value.toFixed(1)}%`
  const ms = (value: number) => `${fmtNum(value)} ms`
  return [
    { label: '总请求', values: list.map((item) => item.totalRequests.toLocaleString()) },
    { label: '成功率', values: list.map((item) => percent(100 - item.errorRate)) },
    { label: '错误率', values: list.map((item) => percent(item.errorRate)) },
    { label: '平均延迟', values: list.map((item) => ms(item.avgRt)) },
    { label: 'P95', values: list.map((item) => ms(item.p95Rt)) },
    { label: 'P99', values: list.map((item) => ms(item.p99Rt)) },
    { label: '平均 QPS', values: list.map((item) => fmtNum(item.avgQps)) },
    { label: '最大 QPS', values: list.map((item) => fmtNum(item.maxQps)) },
    { label: 'APDEX', values: list.map((item) => item.apdex.toFixed(3)) },
    { label: '并发', values: list.map((item) => String(item.concurrency)) },
    { label: '状态', values: list.map((item) => (item.status === 'stopped' ? '已停止' : '已完成')) },
    { label: '时间', values: list.map((item) => fmtTime(item.createdAt)) },
  ]
}
</script>

<template>
  <div class="report-history">
    <div class="history-head">
      <div class="history-stats">
        <span><b>{{ stats.total }}</b> 份报告</span>
        <span>累计请求 <b>{{ stats.requests.toLocaleString() }}</b></span>
        <span>整体成功率 <b>{{ stats.rate.toFixed(1) }}%</b></span>
      </div>
      <a-button size="small" :disabled="selectedIds.length < 2" @click="compareOpen = true">
        <SwapOutlined /> 对比{{ selectedIds.length ? `（${selectedIds.length}）` : '' }}
      </a-button>
    </div>

    <a-spin :spinning="loading">
      <div v-if="reports.length" class="report-list">
        <div v-for="report in reports" :key="report.id" class="fm-card report-card">
          <div class="report-head">
            <div class="report-title">
              <a-checkbox
                :checked="selectedIds.includes(report.id)"
                @change="toggleSelect(report.id, !selectedIds.includes(report.id))"
              />
              <span class="status-dot" :class="{ stopped: report.status === 'stopped' }" />
              <span class="name">{{ report.planName }}</span>
              <span class="status-pill" :class="{ stopped: report.status === 'stopped' }">
                {{ report.status === 'stopped' ? '已停止' : '已完成' }}
              </span>
            </div>
            <div class="report-time">{{ fmtTime(report.createdAt) }}</div>
          </div>

          <div class="report-metrics">
            <div class="metric">
              <span class="m-label">总请求</span>
              <span class="m-value mono">{{ report.totalRequests.toLocaleString() }}</span>
            </div>
            <div class="metric">
              <span class="m-label">成功率</span>
              <span class="m-value mono good">{{ (100 - report.errorRate).toFixed(1) }}<i>%</i></span>
            </div>
            <div class="metric">
              <span class="m-label">平均延迟</span>
              <span class="m-value mono">{{ fmtNum(report.avgRt) }}<i>ms</i></span>
            </div>
            <div class="metric">
              <span class="m-label">P95</span>
              <span class="m-value mono warn">{{ fmtNum(report.p95Rt) }}<i>ms</i></span>
            </div>
            <div class="metric">
              <span class="m-label">平均 QPS</span>
              <span class="m-value mono">{{ fmtNum(report.avgQps) }}</span>
            </div>
            <div class="metric">
              <span class="m-label">APDEX</span>
              <span class="m-value mono">{{ report.apdex.toFixed(3) }}</span>
            </div>
            <div class="metric">
              <span class="m-label">并发</span>
              <span class="m-value mono">{{ report.concurrency }}</span>
            </div>
          </div>

          <div class="report-foot">
            <span class="error-note" :class="{ bad: report.errorCount > 0 }">
              {{
                report.errorCount > 0
                  ? `失败 ${report.errorCount} 次 · 错误率 ${report.errorRate.toFixed(1)}%`
                  : '全部请求成功'
              }}
            </span>
            <div class="report-actions">
              <a-button size="small" type="text" danger @click="remove(report)"><DeleteOutlined /></a-button>
              <a-button size="small" @click="router.push(`/reports/${report.id}`)">
                查看详情<ArrowRightOutlined />
              </a-button>
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="!loading" class="fm-card empty">
        <div class="empty-icon"><FundProjectionScreenOutlined /></div>
        <div class="empty-title">还没有压测报告</div>
        <div class="empty-desc">完成一次压测后，吞吐、延迟分位与错误明细会自动出现在这里。</div>
      </div>
    </a-spin>

    <a-modal v-model:open="compareOpen" title="压测报告对比" :footer="null" width="860px">
      <div v-if="compareReports.length" class="compare-table">
        <div class="compare-row compare-head">
          <span class="compare-label">指标</span>
          <span v-for="report in compareReports" :key="report.id" class="compare-col">
            <b>{{ report.planName }}</b>
            <i>{{ fmtTime(report.createdAt) }}</i>
          </span>
        </div>
        <div v-for="row in compareMetricRows()" :key="row.label" class="compare-row">
          <span class="compare-label">{{ row.label }}</span>
          <span v-for="(value, index) in row.values" :key="index" class="compare-col mono">{{ value }}</span>
        </div>
        <div class="compare-tip">勾选报告卡片左上角即可加入对比，最多 4 份。</div>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.report-history {
  margin-top: 20px;
}
.history-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 10px;
  margin-bottom: 12px;
  border-bottom: 1px solid var(--fm-border);
}
.history-stats {
  display: flex;
  gap: 18px;
  font-size: 11.5px;
  color: var(--fm-muted);
}
.history-stats b {
  color: var(--fm-text);
  font-weight: 600;
}
.report-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.report-card {
  padding: 16px 18px;
  transition: border-color 0.18s;
}
.report-card:hover {
  border-color: var(--fm-border-strong);
}
.report-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.report-title {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}
.status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--fm-success);
  flex-shrink: 0;
}
.status-dot.stopped {
  background: var(--fm-warning);
}
.name {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.status-pill {
  font-size: 10px;
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
  flex-shrink: 0;
}
.status-pill.stopped {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}
.report-time {
  font-size: 11px;
  color: var(--fm-muted);
  flex-shrink: 0;
}
.report-metrics {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 8px;
  margin-top: 14px;
}
.metric {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}
.m-label {
  font-size: 10px;
  color: var(--fm-muted);
}
.m-value {
  font-size: 14px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.m-value i {
  font-style: normal;
  font-size: 9px;
  font-weight: 500;
  color: var(--fm-muted);
  margin-left: 1px;
}
.m-value.good {
  color: var(--fm-success);
}
.m-value.warn {
  color: var(--fm-warning);
}
.report-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px dashed var(--fm-border);
}
.error-note {
  font-size: 11px;
  color: var(--fm-muted);
}
.error-note.bad {
  color: var(--fm-danger);
}
.report-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}
.empty {
  padding: 40px 24px;
  text-align: center;
}
.empty-icon {
  width: 48px;
  height: 48px;
  margin: 0 auto 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.empty-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 6px;
}
.empty-desc {
  font-size: 12px;
  color: var(--fm-muted);
}
@media (max-width: 1180px) {
  .report-metrics {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}
.compare-table {
  border: 1px solid var(--fm-border);
  overflow: hidden;
}
.compare-row {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(0, 1fr);
  grid-template-columns: 110px;
  border-bottom: 1px solid var(--fm-border);
}
.compare-row:last-of-type {
  border-bottom: none;
}
.compare-head {
  background: var(--fm-surface-alt);
}
.compare-label {
  padding: 9px 12px;
  font-size: 11.5px;
  color: var(--fm-muted);
  border-right: 1px solid var(--fm-border);
}
.compare-col {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 9px 12px;
  font-size: 12px;
  border-right: 1px solid var(--fm-border);
  overflow: hidden;
  text-overflow: ellipsis;
}
.compare-col:last-child {
  border-right: none;
}
.compare-head .compare-col b {
  font-size: 12.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.compare-head .compare-col i {
  font-style: normal;
  font-size: 10.5px;
  color: var(--fm-muted);
}
.compare-tip {
  padding: 8px 12px;
  font-size: 10.5px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
}
</style>
