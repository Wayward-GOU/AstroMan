<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import * as api from '@/api'
import type { DashboardMetrics } from '@/types/models'

const router = useRouter()
const metrics = ref<DashboardMetrics | null>(null)

onMounted(async () => {
  try {
    metrics.value = await api.dashboardMetrics()
  } catch (error) {
    message.error(String(error))
  }
})

function fmtInt(value: number) {
  return Math.round(value).toLocaleString()
}

function fmtMs(value?: number | null) {
  if (value == null) return '-'
  return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${Math.round(value)} ms`
}

/* ---------- 数据大屏 ---------- */

const dashDays = computed(() => metrics.value?.days ?? [])
const codeBuckets = computed(() => metrics.value?.codeBuckets ?? [])
const topRequests = computed(() => metrics.value?.topRequests ?? [])
const reportPoints = computed(() => metrics.value?.reportPoints ?? [])
const methodBuckets = computed(() => metrics.value?.methodBuckets ?? [])
const hourBuckets = computed(() => metrics.value?.hourBuckets ?? [])
const latencyBuckets = computed(() => metrics.value?.latencyBuckets ?? [])
const topProjects = computed(() => metrics.value?.topProjects ?? [])

const trendTotal = computed(() => dashDays.value.reduce((sum, d) => sum + d.total, 0))
const trendSuccess = computed(() => dashDays.value.reduce((sum, d) => sum + d.success, 0))
const trendRate = computed(() =>
  trendTotal.value ? (trendSuccess.value / trendTotal.value) * 100 : 0,
)
const trendAvgRt = computed(() => {
  const weighted = dashDays.value.reduce((sum, d) => sum + d.avgRt * d.total, 0)
  return trendTotal.value ? weighted / trendTotal.value : 0
})
const trendPeak = computed(() => Math.max(1, ...dashDays.value.map((d) => d.total)))

const historyTotal = computed(() => codeBuckets.value.reduce((sum, b) => sum + b.count, 0))
const reportPeak = computed(() =>
  Math.max(1, ...reportPoints.value.map((r) => r.p95Rt)),
)
const todayTotal = computed(() => hourBuckets.value.reduce((sum, h) => sum + h.count, 0))
const hourPeak = computed(() => Math.max(1, ...hourBuckets.value.map((h) => h.count)))
const methodPeak = computed(() => Math.max(1, ...methodBuckets.value.map((b) => b.count)))
const latencyTotal = computed(() => latencyBuckets.value.reduce((sum, b) => sum + b.count, 0))
const latencyPeak = computed(() => Math.max(1, ...latencyBuckets.value.map((b) => b.count)))
const projectPeak = computed(() => Math.max(1, ...topProjects.value.map((p) => p.count)))
const todayPeakHour = computed(() => {
  const peak = hourBuckets.value.reduce(
    (best, item) => (item.count > best.count ? item : best),
    hourBuckets.value[0] ?? { hour: 0, count: 0 },
  )
  return peak.count ? `${String(peak.hour).padStart(2, '0')}:00` : '—'
})

function trendBarHeight(point: { total: number }) {
  return point.total ? Math.max(6, (point.total / trendPeak.value) * 100) : 0
}

function trendSegment(point: { total: number }, value: number) {
  return point.total ? (value / point.total) * 100 : 0
}

function monthDay(date: string) {
  return date.slice(5)
}

function trendTitle(point: { date: string; total: number; success: number; avgRt: number }) {
  const rate = point.total ? ((point.success / point.total) * 100).toFixed(0) : 0
  return `${point.date} · ${point.total} 次请求 · 成功率 ${rate}% · 平均 ${fmtMs(point.avgRt)}`
}

function bucketClass(label: string) {
  if (label === '2xx') return 'ok'
  if (label === '3xx') return 'info'
  if (label === '4xx') return 'warn'
  if (label === '5xx') return 'danger'
  return 'other'
}

function bucketPercent(count: number) {
  return historyTotal.value ? `${((count / historyTotal.value) * 100).toFixed(2)}%` : '0%'
}

function bucketText(count: number) {
  if (!historyTotal.value) return '0%'
  return `${((count / historyTotal.value) * 100).toFixed(1)}%`
}

function reportBarWidth(report: { p95Rt: number }) {
  return `${Math.max(6, (report.p95Rt / reportPeak.value) * 100)}%`
}

function reportTone(report: { errorRate: number }) {
  if (report.errorRate >= 1) return 'danger'
  if (report.errorRate >= 0.1) return 'warn'
  return 'ok'
}

function hourHeight(point: { count: number }) {
  return point.count ? Math.max(4, (point.count / hourPeak.value) * 100) : 0
}

function hourTitle(point: { hour: number; count: number }) {
  return `${String(point.hour).padStart(2, '0')}:00 - ${String(point.hour).padStart(2, '0')}:59 · ${point.count} 次`
}

function methodWidth(bucket: { count: number }) {
  return `${bucket.count ? Math.max(6, (bucket.count / methodPeak.value) * 100) : 0}%`
}

function methodTone(label: string) {
  if (label === 'GET') return 'ok'
  if (label === 'POST') return 'info'
  if (label === 'PUT' || label === 'PATCH') return 'warn'
  if (label === 'DELETE') return 'danger'
  return 'other'
}

function latencyWidth(bucket: { count: number }) {
  return `${bucket.count ? Math.max(6, (bucket.count / latencyPeak.value) * 100) : 0}%`
}

function latencyTone(index: number) {
  return ['ok', 'ok', 'info', 'warn', 'danger'][index] ?? 'other'
}

function latencyText(bucket: { count: number }) {
  if (!latencyTotal.value) return '0%'
  return `${((bucket.count / latencyTotal.value) * 100).toFixed(1)}%`
}

function projectWidth(item: { count: number }) {
  return `${Math.max(6, (item.count / projectPeak.value) * 100)}%`
}

</script>

<template>
  <section class="dashboard-metrics">
    <!-- 数据大屏 -->
    <div class="section-head dash-head">
      <div>
        <h2 class="section-title">数据大屏</h2>
        <div class="section-desc">根据本机请求历史与压测报告实时汇总，清空历史后同步归零</div>
      </div>
      <div class="dash-legend">
        <span class="legend-dot ok" />成功
        <span class="legend-dot err" />失败
      </div>
    </div>

    <div class="dash-grid">
      <!-- 请求趋势 -->
      <div class="fm-card dash-card trend-card">
        <div class="dash-card-head">
          <div>
            <div class="dash-title">近 14 天请求趋势</div>
            <div class="dash-sub">按发送时间统计，包含单接口调试与批量运行</div>
          </div>
          <div class="dash-kpis">
            <span><b class="mono">{{ fmtInt(trendTotal) }}</b> 次请求</span>
            <span><b class="mono">{{ trendRate.toFixed(1) }}%</b> 成功率</span>
            <span><b class="mono">{{ fmtMs(trendAvgRt) }}</b> 平均耗时</span>
          </div>
        </div>
        <div v-if="trendTotal" class="trend-plot">
          <div v-for="d in dashDays" :key="d.date" class="trend-col" :title="trendTitle(d)">
            <div class="trend-slot">
              <div class="trend-bar" :style="{ height: `${trendBarHeight(d)}%` }">
                <span class="trend-seg ok" :style="{ height: `${trendSegment(d, d.success)}%` }" />
                <span
                  class="trend-seg err"
                  :style="{ height: `${trendSegment(d, d.total - d.success)}%` }"
                />
              </div>
            </div>
            <span class="trend-day mono">{{ monthDay(d.date) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">
          还没有请求记录。发送一次请求或跑一轮批量运行后，这里会自动生成趋势图。
        </div>
      </div>

      <!-- 状态码分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">状态码分布</div>
          <span class="dash-total mono">{{ fmtInt(historyTotal) }} 次</span>
        </div>
        <div v-if="historyTotal" class="code-stack">
          <span
            v-for="b in codeBuckets"
            :key="b.label"
            class="code-seg"
            :class="bucketClass(b.label)"
            :style="{ width: bucketPercent(b.count) }"
          />
        </div>
        <div class="code-legend">
          <div v-for="b in codeBuckets" :key="b.label" class="code-row">
            <span class="legend-dot" :class="bucketClass(b.label)" />
            <span class="code-label">{{ b.label }}</span>
            <span class="code-count mono">{{ fmtInt(b.count) }}</span>
            <span class="code-pct mono">{{ bucketText(b.count) }}</span>
          </div>
        </div>
        <div v-if="!historyTotal" class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 方法分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">请求方法分布</div>
          <span class="dash-total mono">{{ fmtInt(historyTotal) }} 次</span>
        </div>
        <div v-if="historyTotal" class="bar-list">
          <div v-for="b in methodBuckets" :key="b.label" class="bar-row">
            <span class="bar-label method-label" :class="`tone-text-${methodTone(b.label)}`">
              {{ b.label }}
            </span>
            <span class="bar-track">
              <span
                class="bar-fill"
                :class="methodTone(b.label)"
                :style="{ width: methodWidth(b) }"
              />
            </span>
            <span class="bar-value mono">{{ fmtInt(b.count) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 今日时段分布 -->
      <div class="fm-card dash-card hour-card">
        <div class="dash-card-head">
          <div>
            <div class="dash-title">今日各时段请求量</div>
            <div class="dash-sub">按本地时间 0-23 点，峰值时段 {{ todayPeakHour }}</div>
          </div>
          <div class="dash-kpis">
            <span><b class="mono">{{ fmtInt(todayTotal) }}</b> 次</span>
          </div>
        </div>
        <div v-if="todayTotal" class="hour-plot">
          <div v-for="h in hourBuckets" :key="h.hour" class="hour-col" :title="hourTitle(h)">
            <div class="hour-slot">
              <div class="hour-bar" :style="{ height: `${hourHeight(h)}%` }" />
            </div>
            <span v-if="h.hour % 3 === 0" class="hour-tick mono">{{ h.hour }}</span>
            <span v-else class="hour-tick mono" />
          </div>
        </div>
        <div v-else class="dash-empty">今天还没有请求，发一次就会出现在这里。</div>
      </div>

      <!-- 耗时分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">响应耗时分布</div>
          <span class="dash-total mono">共 {{ fmtInt(latencyTotal) }} 次</span>
        </div>
        <div v-if="latencyTotal" class="bar-list">
          <div v-for="(b, i) in latencyBuckets" :key="b.label" class="bar-row">
            <span class="bar-label">{{ b.label }}</span>
            <span class="bar-track">
              <span
                class="bar-fill"
                :class="latencyTone(i)"
                :style="{ width: latencyWidth(b) }"
              />
            </span>
            <span class="bar-value mono">{{ latencyText(b) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 项目调用量 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">项目调用量 Top 5</div>
          <a class="dash-link" @click="router.push('/projects')">我的项目</a>
        </div>
        <div v-if="topProjects.length" class="bar-list">
          <div v-for="p in topProjects" :key="p.name" class="bar-row">
            <span class="bar-label project-label" :title="p.name">{{ p.name }}</span>
            <span class="bar-track">
              <span class="bar-fill ok" :style="{ width: projectWidth(p) }" />
            </span>
            <span class="bar-value mono">{{ fmtInt(p.count) }}</span>
            <span class="bar-value mono" :class="{ 'tone-danger': p.successRate < 95 }">
              {{ p.successRate.toFixed(0) }}%
            </span>
          </div>
        </div>
        <div v-else class="dash-empty">还没有调用记录。</div>
      </div>

      <!-- 高频接口 -->
      <div class="fm-card dash-card top-card">
        <div class="dash-card-head">
          <div class="dash-title">高频接口 Top 5</div>
          <a class="dash-link" @click="router.push('/history')">全部历史</a>
        </div>
        <div v-if="topRequests.length" class="top-list">
          <div v-for="(t, i) in topRequests" :key="`${t.method}-${t.url}`" class="top-row">
            <span class="top-rank mono">{{ i + 1 }}</span>
            <span class="method-tag" :class="`method-${t.method}`">{{ t.method }}</span>
            <span class="top-name" :title="t.url">{{ t.name }}</span>
            <span class="top-meta mono">{{ fmtInt(t.count) }} 次</span>
            <span class="top-meta mono">{{ fmtMs(t.avgRt) }}</span>
            <span class="top-rate mono" :class="{ low: t.successRate < 95 }">
              {{ t.successRate.toFixed(0) }}%
            </span>
          </div>
        </div>
        <div v-else class="dash-empty">还没有调用记录。</div>
      </div>

      <!-- 压测走势 -->
      <div class="fm-card dash-card report-card">
        <div class="dash-card-head">
          <div class="dash-title">最近压测 P95 走势</div>
          <a class="dash-link" @click="router.push('/stress')">全部报告</a>
        </div>
        <div v-if="reportPoints.length" class="report-chart">
          <div
            v-for="r in reportPoints"
            :key="r.id"
            class="report-chart-row"
            @click="router.push(`/reports/${r.id}`)"
          >
            <span class="report-chart-name" :title="r.name">{{ r.name }}</span>
            <span class="report-chart-track">
              <span
                class="report-chart-fill"
                :class="reportTone(r)"
                :style="{ width: reportBarWidth(r) }"
              />
            </span>
            <span class="report-chart-value mono">{{ fmtMs(r.p95Rt) }}</span>
            <span class="report-chart-value mono" :class="`tone-${reportTone(r)}`">
              {{ r.errorRate.toFixed(1) }}%
            </span>
          </div>
          <div class="report-chart-foot">柱长为 P95 延迟，右侧为错误率；红色表示错误率 ≥ 1%</div>
        </div>
        <div v-else class="dash-empty">
          还没有压测报告。<a class="dash-link" @click="router.push('/stress')">去发起压测</a>
        </div>
      </div>
    </div>

  </section>
</template>

<style scoped>
.dashboard-metrics {
  margin-bottom: 18px;
}
/* ---------- 数据大屏 ---------- */
.dash-head {
  margin-top: 20px;
  margin-bottom: 12px;
}

.dash-legend {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--fm-muted);
}

.legend-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.legend-dot.ok {
  background: var(--fm-success);
}

.legend-dot.err {
  background: var(--fm-danger);
}

.legend-dot.info {
  background: var(--fm-primary);
}

.legend-dot.warn {
  background: var(--fm-warning);
}

.legend-dot.danger {
  background: var(--fm-danger);
}

.legend-dot.other {
  background: var(--fm-border-strong);
}

.dash-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

/* 宽图占两列，其余卡片一列，整体更紧凑 */
.trend-card,
.hour-card,
.top-card,
.report-card {
  grid-column: span 2;
}

.dash-card {
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 13px 15px;
}

.dash-card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.dash-title {
  font-size: 12.5px;
  font-weight: 600;
}

.dash-sub {
  margin-top: 2px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.dash-kpis {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.dash-kpis b {
  color: var(--fm-text);
  font-weight: 600;
}

.dash-total {
  font-size: 11px;
  color: var(--fm-muted);
}

.dash-link {
  font-size: 11px;
  color: var(--fm-primary);
  cursor: pointer;
}

.dash-empty {
  padding: 28px 0 14px;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
  text-align: center;
}

/* 通用横向条形图：方法 / 耗时 / 项目共用 */
.bar-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin-top: 12px;
}

.bar-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.bar-label {
  flex: 0 0 62px;
  overflow: hidden;
  font-size: 10.5px;
  color: var(--fm-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.method-label {
  font-weight: 600;
  color: var(--fm-text);
}

.project-label {
  flex: 0 0 78px;
  color: var(--fm-text);
}

.bar-track {
  position: relative;
  flex: 1 1 auto;
  height: 7px;
  overflow: hidden;
  border-radius: 4px;
  background: var(--fm-surface-alt);
}

.bar-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--fm-border-strong);
  transition: width 0.35s ease;
}

.bar-fill.ok {
  background: var(--fm-success);
}

.bar-fill.info {
  background: var(--fm-primary);
}

.bar-fill.warn {
  background: var(--fm-warning);
}

.bar-fill.danger {
  background: var(--fm-danger);
}

.bar-value {
  flex: 0 0 42px;
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.tone-text-ok {
  color: var(--fm-success);
}

.tone-text-info {
  color: var(--fm-primary);
}

.tone-text-warn {
  color: var(--fm-warning);
}

.tone-text-danger {
  color: var(--fm-danger);
}

.tone-danger {
  color: var(--fm-danger);
}

/* 今日各时段柱状图 */
.hour-plot {
  display: flex;
  gap: 2px;
  height: 96px;
  margin-top: 12px;
}

.hour-col {
  display: flex;
  flex: 1 1 0;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}

.hour-slot {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.hour-bar {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  border-radius: 3px 3px 1px 1px;
  background: var(--fm-primary);
  opacity: 0.85;
  transition: height 0.35s ease;
}

.hour-tick {
  height: 12px;
  font-size: 9px;
  color: var(--fm-muted);
  text-align: center;
}

/* 请求趋势 */
.trend-plot {
  display: flex;
  gap: 6px;
  height: 128px;
  margin-top: 12px;
}

.trend-col {
  display: flex;
  flex: 1 1 0;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.trend-slot {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.trend-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  width: 100%;
  max-width: 24px;
  margin: 0 auto;
  display: flex;
  flex-direction: column-reverse;
  overflow: hidden;
  border-radius: 4px 4px 2px 2px;
  background: var(--fm-surface-alt);
  transition: height 0.35s ease;
}

.trend-seg {
  display: block;
  width: 100%;
  flex-shrink: 0;
}

.trend-seg.ok {
  background: var(--fm-primary);
}

.trend-seg.err {
  background: var(--fm-danger);
}

.trend-day {
  font-size: 9.5px;
  color: var(--fm-muted);
  text-align: center;
  white-space: nowrap;
}

/* 状态码分布 */
.code-stack {
  display: flex;
  height: 8px;
  margin: 14px 0 12px;
  border-radius: 4px;
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.code-seg {
  height: 100%;
  transition: width 0.3s;
}

.code-seg.ok {
  background: var(--fm-success);
}

.code-seg.info {
  background: var(--fm-primary);
}

.code-seg.warn {
  background: var(--fm-warning);
}

.code-seg.danger {
  background: var(--fm-danger);
}

.code-seg.other {
  background: var(--fm-border-strong);
}

.code-legend {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.code-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}

.code-label {
  width: 34px;
  color: var(--fm-text-2);
}

.code-count {
  flex: 1;
  text-align: right;
  color: var(--fm-text-2);
}

.code-pct {
  width: 48px;
  text-align: right;
  color: var(--fm-muted);
}

/* 高频接口 */
.top-list {
  display: flex;
  flex-direction: column;
  margin-top: 10px;
}

.top-row {
  display: grid;
  grid-template-columns: 16px 46px minmax(0, 1fr) 54px 58px 40px;
  align-items: center;
  gap: 8px;
  padding: 7px 0;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11px;
}

.top-row:last-child {
  border-bottom: none;
}

.top-rank {
  font-size: 10px;
  color: var(--fm-muted);
}

.top-row .method-tag {
  justify-self: start;
}

.top-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text);
}

.top-meta {
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.top-rate {
  font-size: 10.5px;
  color: var(--fm-success);
  text-align: right;
}

.top-rate.low {
  color: var(--fm-danger);
}

/* 压测走势 */
.report-chart {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.report-chart-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 86px 66px 46px;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

.report-chart-name {
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.report-chart-row:hover .report-chart-name {
  color: var(--fm-primary);
}

.report-chart-track {
  height: 6px;
  border-radius: 4px;
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.report-chart-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--fm-primary);
}

.report-chart-fill.warn {
  background: var(--fm-warning);
}

.report-chart-fill.danger {
  background: var(--fm-danger);
}

.report-chart-value {
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.report-chart-value.tone-ok {
  color: var(--fm-success);
}

.report-chart-value.tone-warn {
  color: var(--fm-warning);
}

.report-chart-value.tone-danger {
  color: var(--fm-danger);
}

.report-chart-foot {
  margin-top: 2px;
  font-size: 10px;
  color: var(--fm-muted);
}
</style>
