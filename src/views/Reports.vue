<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import {
  FundProjectionScreenOutlined,
  ArrowRightOutlined,
  DeleteOutlined,
  ThunderboltOutlined,
  ClockCircleOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import type { StressReport } from '@/types/models'

const router = useRouter()
const reports = ref<StressReport[]>([])
const loading = ref(true)

onMounted(load)

async function load() {
  loading.value = true
  try {
    reports.value = await api.reportList(200)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

const stats = computed(() => {
  const total = reports.value.length
  const requests = reports.value.reduce((sum, r) => sum + r.totalRequests, 0)
  const success = reports.value.reduce((sum, r) => sum + r.successCount, 0)
  const rate = requests > 0 ? (success / requests) * 100 : 0
  return { total, requests, rate }
})

function fmtTime(ts: number) {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function fmtNum(v: number, digits = 1) {
  return v >= 1000 ? v.toFixed(0) : v.toFixed(digits)
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
      await load()
      message.success('已删除')
    },
  })
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Reports"
      title="压测报告"
      subtitle="每次压测结束后自动生成，包含吞吐、延迟分位与错误明细"
    >
      <a-button @click="router.push('/')">去工作台</a-button>
      <a-button type="primary" @click="router.push('/')">
        <template #icon><ThunderboltOutlined /></template>
        发起压测
      </a-button>
    </PageHeader>

    <div class="summary">
      <div class="summary-item">
        <span class="summary-label">报告总数</span>
        <span class="summary-value mono">{{ stats.total }}</span>
      </div>
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">累计请求</span>
        <span class="summary-value mono">{{ stats.requests.toLocaleString() }}</span>
      </div>
      <div class="summary-divider" />
      <div class="summary-item">
        <span class="summary-label">整体成功率</span>
        <span class="summary-value mono">{{ stats.rate.toFixed(1) }}<i>%</i></span>
      </div>
    </div>

    <a-spin :spinning="loading">
      <div v-if="reports.length" class="report-list">
        <div v-for="r in reports" :key="r.id" class="fm-card report-card">
          <div class="report-head">
            <div class="report-title">
              <span class="status-dot" :class="{ stopped: r.status === 'stopped' }" />
              <span class="name">{{ r.planName }}</span>
              <span class="status-pill" :class="{ stopped: r.status === 'stopped' }">
                {{ r.status === 'stopped' ? '已停止' : '已完成' }}
              </span>
            </div>
            <div class="report-time">
              <ClockCircleOutlined />
              {{ fmtTime(r.createdAt) }}
            </div>
          </div>

          <div class="report-metrics">
            <div class="metric">
              <span class="m-label">总请求</span>
              <span class="m-value mono">{{ r.totalRequests.toLocaleString() }}</span>
            </div>
            <div class="metric">
              <span class="m-label">成功率</span>
              <span class="m-value mono good">{{ (100 - r.errorRate).toFixed(1) }}<i>%</i></span>
            </div>
            <div class="metric">
              <span class="m-label">平均延迟</span>
              <span class="m-value mono">{{ fmtNum(r.avgRt) }}<i>ms</i></span>
            </div>
            <div class="metric">
              <span class="m-label">P95</span>
              <span class="m-value mono warn">{{ fmtNum(r.p95Rt) }}<i>ms</i></span>
            </div>
            <div class="metric">
              <span class="m-label">平均 QPS</span>
              <span class="m-value mono">{{ fmtNum(r.avgQps) }}</span>
            </div>
            <div class="metric">
              <span class="m-label">APDEX</span>
              <span class="m-value mono">{{ r.apdex.toFixed(3) }}</span>
            </div>
            <div class="metric">
              <span class="m-label">并发</span>
              <span class="m-value mono">{{ r.concurrency }}</span>
            </div>
          </div>

          <div class="report-foot">
            <span class="error-note" :class="{ bad: r.errorCount > 0 }">
              {{ r.errorCount > 0 ? `失败 ${r.errorCount} 次 · 错误率 ${r.errorRate.toFixed(1)}%` : '全部请求成功' }}
            </span>
            <div class="report-actions">
              <a-button size="small" type="text" danger @click="remove(r)">
                <template #icon><DeleteOutlined /></template>
              </a-button>
              <a-button size="small" @click="router.push(`/reports/${r.id}`)">
                查看详情<ArrowRightOutlined />
              </a-button>
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="!loading" class="fm-card empty">
        <div class="empty-icon"><FundProjectionScreenOutlined /></div>
        <div class="empty-title">还没有压测报告</div>
        <div class="empty-desc">
          打开任意请求，点击右上角「压测」，设置并发与时长并开始，结束后报告会自动出现在这里。
        </div>
        <a-button type="primary" @click="router.push('/')">去工作台选请求</a-button>
      </div>
    </a-spin>
  </div>
</template>

<style scoped>
.summary {
  display: flex;
  align-items: center;
  gap: 26px;
  padding: 16px 22px;
  margin-bottom: 18px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  box-shadow: var(--fm-shadow-sm);
  max-width: 620px;
}

.summary-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.summary-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.summary-value {
  font-size: 18px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.summary-value i {
  font-style: normal;
  font-size: 10px;
  color: var(--fm-muted);
  margin-left: 1px;
}

.summary-divider {
  width: 1px;
  height: 28px;
  background: var(--fm-border);
}

.report-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.report-card {
  padding: 16px 18px;
  transition: box-shadow 0.18s, border-color 0.18s;
}

.report-card:hover {
  box-shadow: var(--fm-shadow-lg);
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
  display: flex;
  align-items: center;
  gap: 5px;
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
  border-radius: 8px;
  background: var(--fm-surface-alt);
  border: 1px solid #f1f4fa;
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

.report-actions :deep(.anticon) {
  font-size: 10px;
}

.empty {
  padding: 66px 24px;
  text-align: center;
}

.empty-icon {
  width: 56px;
  height: 56px;
  margin: 0 auto 16px;
  border-radius: 16px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.empty-desc {
  max-width: 440px;
  margin: 0 auto 18px;
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-muted);
}

@media (max-width: 1180px) {
  .report-metrics {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}
</style>
