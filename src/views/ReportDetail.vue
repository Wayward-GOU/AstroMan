<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { save } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeftOutlined,
  DeleteOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  DownloadOutlined,
  DownOutlined,
  EyeOutlined,
  CopyOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { reportReviewPrompt } from '@/utils/aiScenarios'
import { formatDateTime } from '@/utils/datetime'
import * as api from '@/api'
import type { ReportDetail, ReportRequestRecord, ReportRequestsPage } from '@/types/models'
import { buildReportHtml, buildReportSheets } from '@/utils/reportExport'
import { detectKnee, qpsOverview, scoreReport, selfCheck } from '@/utils/reportAnalytics'
import { buildFindings, gradeReport, latencyDrift, type Finding } from '@/utils/reportNarrative'
import { buildXlsx } from '@/utils/xlsx'

const route = useRoute()
const router = useRouter()

const detail = ref<ReportDetail | null>(null)
const loading = ref(true)
const exporting = ref(false)

const reportId = computed(() => Number(route.params.id))

const requestFilter = ref<'all' | 'success' | 'failed'>('all')
const requestPage = ref(1)
const requestPageSize = ref(50)
const requestsLoading = ref(false)
const requestPageData = ref<ReportRequestsPage | null>(null)

const requestItems = computed(() => requestPageData.value?.items ?? [])
const requestTotal = computed(() => requestPageData.value?.total ?? 0)

/* ---------- 请求明细详情 ---------- */
const recordOpen = ref(false)
const recordItem = ref<ReportRequestRecord | null>(null)
const recordTab = ref<'request' | 'response'>('request')

type HeaderPair = { key: string; value: string }

function parsePairs(json?: string | null): HeaderPair[] {
  if (!json) return []
  try {
    const parsed = JSON.parse(json)
    if (!Array.isArray(parsed)) return []
    return parsed.map((item) => {
      if (Array.isArray(item)) {
        return { key: String(item[0] ?? ''), value: String(item[1] ?? '') }
      }
      return { key: String(item?.key ?? ''), value: String(item?.value ?? '') }
    })
  } catch {
    return []
  }
}

function prettyText(raw?: string | null) {
  if (!raw) return ''
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch {
    return raw
  }
}

const recordBody = computed(() => prettyText(recordItem.value?.body))
const recordResBody = computed(() => prettyText(recordItem.value?.responseBody))
const recordParams = computed(() => {
  const query = recordItem.value?.params ?? ''
  if (!query) return [] as HeaderPair[]
  const out: HeaderPair[] = []
  new URLSearchParams(query).forEach((value, key) => out.push({ key, value }))
  return out
})
const recordReqHeaders = computed(() => parsePairs(recordItem.value?.headers))
const recordResHeaders = computed(() => parsePairs(recordItem.value?.responseHeaders))

function openRecord(item: ReportRequestRecord) {
  recordItem.value = item
  recordTab.value = 'request'
  recordOpen.value = true
}

async function copyText(text: string, label: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success(`已复制${label}`)
  } catch {
    message.error('复制失败')
  }
}

async function copyRecordBody() {
  await copyText(recordBody.value, '请求体')
}

async function copyRecordResBody() {
  await copyText(recordResBody.value, '响应体')
}

function fmtFullTime(ts?: number | null) {
  return formatDateTime(ts)
}

async function loadRequests() {
  requestsLoading.value = true
  try {
    const success =
      requestFilter.value === 'all' ? null : requestFilter.value === 'success'
    requestPageData.value = await api.reportRequests(
      reportId.value,
      requestPage.value,
      requestPageSize.value,
      success,
    )
  } catch (e) {
    requestPageData.value = { total: 0, items: [] }
    console.error('请求明细加载失败', e)
  } finally {
    requestsLoading.value = false
  }
}

function onRequestFilterChange() {
  requestPage.value = 1
  loadRequests()
}

function onRequestPageChange(page: number, pageSize: number) {
  const size = Number(pageSize)
  requestPage.value = size === requestPageSize.value ? page : 1
  requestPageSize.value = size
  loadRequests()
}

onMounted(async () => {
  try {
    detail.value = await api.reportGet(reportId.value)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
  await loadRequests()
})

const report = computed(() => detail.value?.report ?? null)

const aiAssist = useAiAssistStore()

function aiReview() {
  if (!detail.value) {
    message.warning('报告还在加载中')
    return
  }
  const current = detail.value.report
  void aiAssist.show({
    title: '压测报告解读',
    prompt: reportReviewPrompt(detail.value),
    source: 'report',
    hint: `${current.planName} · ${current.concurrency} 并发 · ${current.totalRequests} 次请求 · 错误率 ${((current.errorRate ?? 0) * 100).toFixed(2)}%`,
  })
}
const samples = computed(() => detail.value?.samples ?? [])
const errors = computed(() => detail.value?.errors ?? [])
const statusCodes = computed(() => detail.value?.statusCodes ?? [])
const errorTypes = computed(() => detail.value?.errorTypes ?? [])
const histogram = computed(() => detail.value?.histogram ?? [])
const bodyHistogram = computed(() => detail.value?.bodyHistogram ?? [])
const slowRequests = computed(() => detail.value?.slowRequests ?? [])
const stages = computed(() => detail.value?.stages ?? [])
const config = computed(() => detail.value?.config ?? null)
const metrics = computed(() => config.value?.metrics ?? null)
const bytesSent = computed(() => metrics.value?.bytesSent ?? 0)

/** QPS 指标组：平均 / 峰值 / 90% 分布 / 单并发 / 目标达成率，页面与导出同一口径 */
const qps = computed(() =>
  detail.value
    ? qpsOverview(detail.value, { rampUpSec: config.value?.rampUpSec ?? 0 })
    : null,
)
const knee = computed(() => (detail.value ? detectKnee(detail.value) : null))
const score = computed(() => (detail.value ? scoreReport(detail.value) : null))
const checks = computed(() => (detail.value ? selfCheck(detail.value) : []))
const checksPassed = computed(() => checks.value.every((item) => item.pass))

/* ---------- 关键发现与结论（3.4.1） ---------- */
const findings = computed<Finding[]>(() => (detail.value ? buildFindings(detail.value) : []))
const gradeInfo = computed(() => (detail.value ? gradeReport(detail.value) : null))

const FINDING_TAG: Record<Finding['level'], string> = {
  bad: 'P0',
  warn: 'P1',
  info: '提示',
  ok: '正常',
}

/** 前后半程 RT 漂移：>15% warn、>40% bad（与导出报告同一口径） */
const drift = computed(() => (detail.value ? latencyDrift(detail.value) : 0))
const driftTone = computed(() => (drift.value > 0.4 ? 'bad' : drift.value > 0.15 ? 'warn' : 'ok'))

/** 采样/计数/流量不自洽的项前置到顶部（3.4.3） */
const biasChecks = computed(() => checks.value.filter((item) => !item.pass))

/* ---------- 施压端能力快照（3.7.1） ---------- */
const capacity = computed(() => config.value?.machineCapacity ?? null)
const capacityUsage = computed(() => {
  const snapshot = capacity.value
  const avg = report.value?.avgQps ?? 0
  if (!snapshot?.maxQps || avg <= 0) return null
  return (avg / snapshot.maxQps) * 100
})

/* ---------- 目标地址兜底（3.5.2） ---------- */
const targetUrlInfo = computed(() => {
  const cfg = config.value
  const raw = cfg?.url?.trim() ?? ''
  const resolved = cfg?.target?.resolvedUrl?.trim() ?? ''
  const isUnresolved = (text: string) => !text || text.includes('{{')
  if (raw && !isUnresolved(raw)) {
    return { url: raw, source: 'config' as const, note: '' }
  }
  if (resolved && !isUnresolved(resolved)) {
    return { url: resolved, source: 'resolved' as const, note: '' }
  }
  const first = requestItems.value.find((item) => item.url && !isUnresolved(item.url))
  if (first) {
    return {
      url: first.url,
      source: 'record' as const,
      note: '旧报告未保存解析后地址，这里取首条请求明细的 URL',
    }
  }
  return {
    url: '',
    source: 'none' as const,
    note: '未记录解析后地址，请重新压测',
  }
})

/* ---------- 基线对比（3.4.4） ---------- */
const baseline = ref<ReportDetail | null>(null)
const baselineLoading = ref(false)

async function loadBaseline() {
  const id = config.value?.baselineReportId
  if (!id || id === reportId.value) {
    baseline.value = null
    return
  }
  baselineLoading.value = true
  try {
    baseline.value = await api.reportGet(id)
  } catch {
    baseline.value = null
  } finally {
    baselineLoading.value = false
  }
}

watch(
  () => config.value?.baselineReportId,
  () => {
    void loadBaseline()
  },
)

interface BaselineRow {
  label: string
  current: string
  base: string
  delta: string
  tone: 'ok' | 'warn' | 'bad' | 'muted'
}

const baselineRows = computed<BaselineRow[]>(() => {
  const current = report.value
  const base = baseline.value?.report
  if (!current || !base) return []
  const ratio = (value: number, origin: number) =>
    origin > 0 ? ((value - origin) / origin) * 100 : null
  const deltaText = (value: number | null) =>
    value === null ? '—' : `${value >= 0 ? '+' : ''}${value.toFixed(1)}%`
  const latencyRow = (label: string, value: number, origin: number) => {
    const pct = ratio(value, origin)
    return {
      label,
      current: `${fmtNum(value)} ms`,
      base: `${fmtNum(origin)} ms`,
      delta: deltaText(pct),
      tone: pct === null ? 'muted' : pct > 40 ? 'bad' : pct > 15 ? 'warn' : 'ok',
    } as BaselineRow
  }
  const qpsPct = ratio(current.avgQps, base.avgQps)
  const errPct = ratio(current.errorRate, base.errorRate)
  return [
    latencyRow('P95 延迟', current.p95Rt, base.p95Rt),
    latencyRow('P99 延迟', current.p99Rt, base.p99Rt),
    latencyRow('平均延迟', current.avgRt, base.avgRt),
    {
      label: '平均 QPS',
      current: fmtNum(current.avgQps),
      base: fmtNum(base.avgQps),
      delta: deltaText(qpsPct),
      tone: qpsPct === null ? 'muted' : qpsPct < -20 ? 'bad' : qpsPct < -5 ? 'warn' : 'ok',
    },
    {
      label: '错误率',
      current: `${current.errorRate.toFixed(2)}%`,
      base: `${base.errorRate.toFixed(2)}%`,
      delta: deltaText(errPct),
      tone:
        errPct === null
          ? 'muted'
          : current.errorRate - base.errorRate > 0.5
            ? 'bad'
            : current.errorRate - base.errorRate > 0.1
              ? 'warn'
              : 'ok',
    },
  ]
})

const recordModeText = computed(() => {
  const mode = metrics.value?.recordMode ?? config.value?.recordDetail
  const map: Record<string, string> = {
    full: '明细：完整',
    sample: '明细：采样 1%',
    off: '明细：已关闭',
    auto: '明细：自动',
  }
  return mode ? map[mode] ?? mode : ''
})

const statsRows = computed(() => {
  const current = report.value
  if (!current) return []
  const seconds = Math.max(1, durationSec.value)
  return [
    { label: '样本数', value: current.totalRequests.toLocaleString() },
    { label: '错误率', value: `${current.errorRate.toFixed(2)}%` },
    { label: '平均响应时间', value: `${fmtNum(current.avgRt)} ms` },
    { label: '中位数 P50', value: `${fmtNum(current.p50Rt)} ms` },
    { label: 'P90', value: `${fmtNum(current.p90Rt)} ms` },
    { label: 'P95', value: `${fmtNum(current.p95Rt)} ms` },
    { label: 'P99', value: `${fmtNum(current.p99Rt)} ms` },
    { label: '标准差', value: `${fmtNum(current.stddevRt)} ms` },
    { label: '平均 QPS', value: fmtNum(qps.value?.avg) },
    { label: '峰值 QPS', value: fmtNum(qps.value?.max) },
    { label: '90% 分布 QPS', value: fmtNum(qps.value?.ninety) },
    { label: '单并发 QPS', value: fmtNum(qps.value?.perConcurrent, 2) },
    ...(qps.value?.target
      ? [
          { label: '目标 QPS', value: fmtNum(qps.value.target) },
          { label: '目标达成率', value: `${fmtNum(qps.value.attainment, 1)}%` },
        ]
      : []),
    { label: '接收带宽', value: `${(current.bytesTotal / seconds / 1024).toFixed(1)} KB/s` },
    { label: '发送带宽', value: `${(bytesSent.value / seconds / 1024).toFixed(1)} KB/s` },
  ]
})

const scenarioDraft = ref('')
const summaryDraft = ref('')
const textSaving = ref(false)

watch(
  detail,
  (value) => {
    scenarioDraft.value = value?.scenarioText ?? ''
    summaryDraft.value = value?.summaryText ?? ''
  },
  { immediate: true },
)

async function saveTexts() {
  if (!report.value) return
  textSaving.value = true
  try {
    await api.reportUpdateTexts(report.value.id, scenarioDraft.value, summaryDraft.value)
    if (detail.value) {
      detail.value.scenarioText = scenarioDraft.value
      detail.value.summaryText = summaryDraft.value
    }
    message.success('已保存场景与总结')
  } catch (e) {
    message.error(String(e))
  } finally {
    textSaving.value = false
  }
}

function clearDetails() {
  if (!report.value) return
  Modal.confirm({
    title: '清理逐请求明细？',
    content: '汇总、采样与错误类型统计会保留，仅删除逐条请求明细（不可恢复）。',
    okText: '清理',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      const result = await api.reportClearDetails(report.value!.id)
      const freed = result.freedBytes > 0 ? `，释放 ${fmtBytes(result.freedBytes)}` : ''
      message.success(`已清理 ${result.removed.toLocaleString()} 条明细${freed}`)
      if (result.compactError) {
        message.warning(`数据库压缩未完成：${result.compactError}`)
      }
      await loadRequests()
    },
  })
}

const chartLabels = computed(() => samples.value.map((s) => `${s.elapsedSec}s`))

const qpsSeries = computed(() => [
  { name: 'QPS', color: 'var(--fm-primary)', values: samples.value.map((s) => s.qps) },
  { name: '并发', color: 'var(--fm-muted)', values: samples.value.map((s) => s.concurrent) },
])

const latencySeries = computed(() => [
  { name: '平均', color: 'var(--fm-primary)', values: samples.value.map((s) => s.avgRt) },
  { name: 'P50', color: 'var(--fm-json-key)', values: samples.value.map((s) => s.p50Rt) },
  { name: 'P95', color: 'var(--fm-warning)', values: samples.value.map((s) => s.p95Rt) },
  { name: 'P99', color: 'var(--fm-danger)', values: samples.value.map((s) => s.p99Rt) },
])

const errorSeries = computed(() => [
  {
    name: '错误率',
    color: 'var(--fm-danger)',
    values: samples.value.map((s) => {
      const total = s.successCount + s.errorCount
      return total > 0 ? (s.errorCount / total) * 100 : 0
    }),
  },
])

const durationSec = computed(() => {
  if (!report.value?.startTime || !report.value?.endTime) return 0
  return Math.max(0, Math.round((report.value.endTime - report.value.startTime) / 1000))
})

const successRate = computed(() => 100 - (report.value?.errorRate ?? 0))

const percentileRows = computed(() => {
  const r = report.value
  if (!r) return []
  return [
    { label: '最小值 Min', value: r.minRt },
    { label: '平均值 Avg', value: r.avgRt },
    { label: 'P50', value: r.p50Rt },
    { label: 'P75', value: r.p75Rt },
    { label: 'P90', value: r.p90Rt },
    { label: 'P95', value: r.p95Rt },
    { label: 'P99', value: r.p99Rt },
    { label: 'P99.9', value: r.p999Rt },
    { label: '最大值 Max', value: r.maxRt },
    { label: '标准差 σ', value: r.stddevRt },
  ]
})

const apdexGrade = computed(() => {
  const value = report.value?.apdex ?? 0
  if (value >= 0.94) return { label: '优秀', tone: 'ok' }
  if (value >= 0.85) return { label: '良好', tone: 'ok' }
  if (value >= 0.7) return { label: '可接受', tone: 'warn' }
  return { label: '需优化', tone: 'bad' }
})

const verdict = computed(() => {
  const r = report.value
  if (!r) return { title: '-', desc: '', tone: 'ok' }
  if (r.status === 'stopped') {
    return {
      title: '压测被手动停止',
      desc: '数据仅覆盖停止前的时段，结论仅作参考',
      tone: 'warn',
    }
  }
  if (r.errorRate > 5) {
    return {
      title: '服务稳定性异常',
      desc: `错误率达到 ${r.errorRate.toFixed(2)}%，建议先排查失败请求再评估性能`,
      tone: 'bad',
    }
  }
  if (apdexGrade.value.tone === 'bad') {
    return {
      title: '响应偏慢',
      desc: `APDEX ${r.apdex.toFixed(3)}，多数请求超出可容忍阈值`,
      tone: 'warn',
    }
  }
  return {
    title: '压测通过',
    desc: `APDEX ${r.apdex.toFixed(3)}，错误率 ${r.errorRate.toFixed(2)}%`,
    tone: 'ok',
  }
})

const slaChecks = computed(() => {
  const r = report.value
  const sla = config.value?.sla
  if (!r || !sla) return []
  const rows: Array<{ label: string; target: string; actual: string; pass: boolean }> = []
  if (sla.p95Ms) {
    rows.push({
      label: 'P95 延迟',
      target: `≤ ${sla.p95Ms} ms`,
      actual: `${fmtNum(r.p95Rt)} ms`,
      pass: r.p95Rt <= sla.p95Ms,
    })
  }
  if (sla.p99Ms) {
    rows.push({
      label: 'P99 延迟',
      target: `≤ ${sla.p99Ms} ms`,
      actual: `${fmtNum(r.p99Rt)} ms`,
      pass: r.p99Rt <= sla.p99Ms,
    })
  }
  if (sla.errorRate) {
    rows.push({
      label: '错误率',
      target: `≤ ${sla.errorRate}%`,
      actual: `${r.errorRate.toFixed(2)}%`,
      pass: r.errorRate <= sla.errorRate,
    })
  }
  if (sla.minRps) {
    rows.push({
      label: '平均 QPS',
      target: `≥ ${sla.minRps}`,
      actual: fmtNum(r.avgQps),
      pass: r.avgQps >= sla.minRps,
    })
  }
  return rows
})

const slaPassed = computed(() => slaChecks.value.length === 0 || slaChecks.value.every((r) => r.pass))

const maxHistogram = computed(() => Math.max(1, ...histogram.value.map((b) => b.count)))
const maxBodyHistogram = computed(() => Math.max(1, ...bodyHistogram.value.map((b) => b.count)))
const slowMaxLatency = computed(() =>
  Math.max(1, ...slowRequests.value.map((item) => item.latencyMs)),
)

/** 按请求数加权的 Body 平均（逐秒采样点加权，旧报告无该字段时按 0 处理） */
const avgBody = computed(() => {
  let sum = 0
  let weight = 0
  for (const sample of samples.value) {
    const count = sample.successCount + sample.errorCount
    sum += (sample.bodyAvg || 0) * count
    weight += count
  }
  return weight ? sum / weight : 0
})

/** Body P95 从报告层直方图算；分位数不能由两个分位数相减 */
const p95Body = computed(() => {
  const buckets = bodyHistogram.value
  const total = buckets.reduce((acc, item) => acc + item.count, 0)
  if (!total) {
    return Math.max(0, ...samples.value.map((sample) => sample.bodyP95 || 0))
  }
  const target = total * 0.95
  let acc = 0
  for (const bucket of buckets) {
    acc += bucket.count
    if (acc >= target) {
      return Number.isFinite(bucket.maxMs) ? (bucket.minMs + bucket.maxMs) / 2 : bucket.minMs
    }
  }
  return 0
})

const maxCodeCount = computed(() => Math.max(1, ...statusCodes.value.map((s) => s.count)))
const codeTotal = computed(() => statusCodes.value.reduce((sum, s) => sum + s.count, 0))
const maxErrorTypeCount = computed(() => Math.max(1, ...errorTypes.value.map((e) => e.count)))

const modeLabel = computed(() => (config.value?.mode === 'stages' ? '阶梯加压' : '固定并发'))

function fmtNum(v: number | undefined | null, digits = 1) {
  if (v === undefined || v === null || Number.isNaN(v)) return '-'
  return v >= 1000 ? v.toFixed(0) : v.toFixed(digits)
}

function fmtBytes(v: number | undefined | null) {
  if (!v) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = v
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`
}

function fmtTime(ts?: number | null) {
  return formatDateTime(ts)
}

/** 压测内相对时间：12.3s / 1m02s / 1h02m */
function fmtDuration(ms?: number | null) {
  if (!ms || ms < 0) return '0s'
  const sec = ms / 1000
  if (sec < 60) return `${sec.toFixed(1)}s`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m${String(Math.floor(sec % 60)).padStart(2, '0')}s`
  return `${Math.floor(min / 60)}h${String(min % 60).padStart(2, '0')}m`
}

function codeTone(code: number) {
  if (code >= 500) return 'bad'
  if (code >= 400) return 'warn'
  if (code >= 300) return 'info'
  return 'ok'
}

type ExportFormat = 'json' | 'html' | 'excel' | 'csv-requests' | 'csv-errors'

const EXPORT_META: Record<ExportFormat, { extension: string; filterName: string; label: string }> = {
  json: { extension: 'json', filterName: 'AstroMan 压测报告', label: 'JSON 归档' },
  html: { extension: 'html', filterName: 'HTML 报告', label: 'HTML 报告' },
  excel: { extension: 'xlsx', filterName: 'Excel 工作簿', label: 'Excel 表格' },
  'csv-requests': { extension: 'csv', filterName: 'CSV 明细', label: '请求明细 CSV' },
  'csv-errors': { extension: 'csv', filterName: 'CSV 明细', label: '错误明细 CSV' },
}

async function exportReport(format: ExportFormat) {
  const current = report.value
  const data = detail.value
  if (!current || !data) return
  const meta = EXPORT_META[format]
  const safeName = current.planName.replace(/[\\/:*?"<>|]/g, '_')
  exporting.value = true
  try {
    const path = await save({
      title: `导出压测报告（${meta.label}）`,
      defaultPath: `${safeName}-report.${meta.extension}`,
      filters: [{ name: meta.filterName, extensions: [meta.extension] }],
    })
    if (!path) return
    const suffix = `.${meta.extension}`
    const target = path.toLowerCase().endsWith(suffix) ? path : `${path}${suffix}`
    if (format === 'json') {
      await api.reportExport(current.id, target)
    } else if (format === 'csv-requests' || format === 'csv-errors') {
      // 后端逐行流式写盘，前端不经过内存
      const kind = format === 'csv-requests' ? 'requests' : 'errors'
      const rows = await api.reportExportDetailCsv(current.id, kind, target)
      message.success(`已导出 ${meta.label}，共 ${rows.toLocaleString()} 行`)
      return
    } else {
      const records = await api.reportRequests(current.id, 1, 50000, null)
      if (format === 'html') {
        const bytes = new TextEncoder().encode(buildReportHtml(data, records.items))
        await api.reportWriteFile(target, Array.from(bytes))
      } else {
        const bytes = buildXlsx(buildReportSheets(data, records.items))
        await api.reportWriteFile(target, Array.from(bytes))
      }
    }
    message.success(`已导出 ${meta.label}`)
  } catch (e) {
    message.error(String(e))
  } finally {
    exporting.value = false
  }
}

function onExportMenu(info: { key: string | number }) {
  if (info.key === 'clear-details') {
    clearDetails()
    return
  }
  exportReport(info.key as ExportFormat)
}

function remove() {
  if (!report.value) return
  Modal.confirm({
    title: '删除这份压测报告？',
    content: '报告与采样数据将被永久删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      const result = await api.reportDelete(report.value!.id)
      const freed = result.freedBytes > 0 ? `，释放 ${fmtBytes(result.freedBytes)}` : ''
      message.success(`已删除${freed}`)
      router.push('/stress')
    },
  })
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Report Detail"
      title="压测报告详情"
      subtitle="吞吐、延迟分位、状态码与错误分布的完整记录"
    >
      <a-button :disabled="!detail" @click="aiReview">
        <template #icon><AiSparkIcon /></template>
        AI 解读
      </a-button>
      <a-button @click="router.push('/stress')">
        <template #icon><ArrowLeftOutlined /></template>
        返回列表
      </a-button>
      <a-dropdown :trigger="['click']" placement="bottomRight">
        <a-button :loading="exporting">
          <template #icon><DownloadOutlined /></template>
          导出报告
          <DownOutlined />
        </a-button>
        <template #overlay>
          <a-menu @click="onExportMenu">
            <a-menu-item key="json">JSON 归档（完整数据）</a-menu-item>
            <a-menu-item key="html">HTML 报告（可视化）</a-menu-item>
            <a-menu-item key="excel">Excel 表格（多工作表）</a-menu-item>
            <a-menu-divider />
            <a-menu-item key="csv-requests">请求明细 CSV（全量流式导出）</a-menu-item>
            <a-menu-item key="csv-errors">错误明细 CSV（全量流式导出）</a-menu-item>
            <a-menu-divider />
            <a-menu-item key="clear-details">清理逐请求明细</a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-button danger @click="remove">
        <template #icon><DeleteOutlined /></template>
        删除
      </a-button>
    </PageHeader>

    <a-spin :spinning="loading">
      <template v-if="report">
        <div class="fm-card overview">
          <div class="overview-main">
            <div class="title-row">
              <span class="status-dot" :class="{ stopped: report.status === 'stopped' }" />
              <h2 class="report-name">{{ report.planName }}</h2>
              <span class="status-pill" :class="{ stopped: report.status === 'stopped' }">
                {{ report.status === 'stopped' ? '已停止' : '已完成' }}
              </span>
              <span class="mode-pill">{{ modeLabel }}</span>
            </div>
            <div class="meta-row">
              <span>开始于 {{ fmtTime(report.startTime) }}</span>
              <span class="dot-sep">·</span>
              <span>耗时 {{ durationSec }} 秒</span>
              <span class="dot-sep">·</span>
              <span>峰值并发 {{ report.concurrency }}</span>
              <span v-if="qps?.target" class="dot-sep">·</span>
              <span v-if="qps?.target">目标 {{ fmtNum(qps.target, 0) }} QPS</span>
              <span v-if="qps?.attainment != null" class="dot-sep">·</span>
              <span v-if="qps?.attainment != null">达成 {{ fmtNum(qps.attainment, 0) }}%</span>
            </div>
            <div v-if="targetUrlInfo.url || targetUrlInfo.note" class="target-line">
              <span class="method-chip">{{ config?.method || 'GET' }}</span>
              <span
                v-if="targetUrlInfo.url"
                class="target-url mono"
                :title="`${targetUrlInfo.url}（点击复制）`"
                @click="copyText(targetUrlInfo.url, '地址')"
                >{{ targetUrlInfo.url }}</span
              >
              <span v-else class="target-url missing">{{ targetUrlInfo.note }}</span>
              <span v-if="targetUrlInfo.url && targetUrlInfo.note" class="target-note">
                {{ targetUrlInfo.note }}
              </span>
            </div>
          </div>
          <div class="verdict" :class="verdict.tone">
            <component
              :is="verdict.tone === 'ok' ? CheckCircleOutlined : CloseCircleOutlined"
            />
            <div>
              <div class="verdict-title">{{ verdict.title }}</div>
              <div class="verdict-desc">{{ verdict.desc }}</div>
            </div>
          </div>
        </div>

        <div class="fm-card findings-card">
          <div class="card-head">
            <div>
              <div class="card-title">关键发现</div>
              <div class="card-sub">按风险从高到低排列，映射 P0 / P1 与导出报告一致</div>
            </div>
            <span v-if="gradeInfo" class="grade-badge" :class="gradeInfo.grade">
              {{ gradeInfo.label }}
            </span>
          </div>
          <div v-if="gradeInfo" class="grade-statement">
            {{ gradeInfo.statement }}
            <span class="grade-reason">{{ gradeInfo.reason }}</span>
          </div>

          <div v-if="biasChecks.length" class="bias-banner">
            <b>数据自洽性提示</b>
            <span>
              {{ biasChecks.map((item) => `${item.label}：${item.detail}`).join('；') }}
            </span>
          </div>

          <div class="finding-list">
            <div v-for="(item, index) in findings" :key="index" class="finding-row" :class="item.level">
              <span class="finding-tag">{{ FINDING_TAG[item.level] }}</span>
              <div class="finding-body">
                <div class="finding-title">{{ item.title }}</div>
                <div class="finding-detail">{{ item.detail }}</div>
              </div>
            </div>
            <div v-if="!findings.length" class="inline-empty">样本不足，暂未生成关键发现</div>
          </div>

          <div class="drift-row">
            <span class="drift-label">响应时间漂移（后半程 vs 前半程）</span>
            <span class="drift-value mono" :class="driftTone">
              {{ drift > 0 ? '+' : '' }}{{ (drift * 100).toFixed(1) }}%
            </span>
            <span class="drift-rule">&gt;15% 告警，&gt;40% 严重</span>
          </div>
        </div>

        <div class="metrics">
          <div class="metric-card fm-card">
            <span class="m-label">总请求</span>
            <span class="m-value mono">{{ report.totalRequests.toLocaleString() }}</span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">成功率</span>
            <span class="m-value mono" :class="{ bad: successRate < 99 }">{{ successRate.toFixed(2) }}<i>%</i></span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">平均 QPS</span>
            <span class="m-value mono">{{ fmtNum(qps?.avg) }}</span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">峰值 QPS</span>
            <span class="m-value mono">{{ fmtNum(qps?.max) }}</span>
          </div>
          <div class="metric-card fm-card" title="每秒采样 QPS 的 P10：90% 的时间吞吐不低于该值，已剔除升压段与首末不完整秒">
            <span class="m-label">90% 分布 QPS</span>
            <span class="m-value mono">{{ fmtNum(qps?.ninety) }}</span>
          </div>
          <div class="metric-card fm-card" title="平均 QPS ÷ 并发数，用于对比不同并发档位下单连接的吞吐效率">
            <span class="m-label">单并发 QPS</span>
            <span class="m-value mono">{{ fmtNum(qps?.perConcurrent, 2) }}</span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">平均延迟</span>
            <span class="m-value mono">{{ fmtNum(report.avgRt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">P95 延迟</span>
            <span class="m-value mono warn">{{ fmtNum(report.p95Rt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">P99 延迟</span>
            <span class="m-value mono bad">{{ fmtNum(report.p99Rt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card">
            <span class="m-label">APDEX</span>
            <span class="m-value mono" :class="apdexGrade.tone">{{ report.apdex.toFixed(3) }}
              <i>{{ apdexGrade.label }}</i>
            </span>
          </div>
        </div>

        <div class="fm-card capacity-card">
          <div class="card-head">
            <div>
              <div class="card-title">施压端能力</div>
              <div class="card-sub">客户端极限来自本机回环检测，只用于判断施压端余量</div>
            </div>
            <span v-if="capacity" class="capacity-badge">
              客户端极限 {{ capacity.reachedLimit === false ? '≥ ' : '' }}{{ fmtNum(capacity.maxQps, 0) }} QPS
            </span>
            <span v-else class="capacity-badge muted">未检测本机施压能力</span>
          </div>
          <div v-if="capacity" class="capacity-grid">
            <div class="capacity-cell">
              <span>本次平均</span>
              <b>{{ fmtNum(report.avgQps, 0) }} QPS</b>
            </div>
            <div class="capacity-cell">
              <span>客户端负载占比</span>
              <b :class="capacityUsage !== null && capacityUsage >= 80 ? 'bad' : 'ok'">
                {{ capacityUsage === null ? '—' : `${capacityUsage.toFixed(1)}%` }}
              </b>
            </div>
            <div class="capacity-cell">
              <span>检测时间</span>
              <b>{{ fmtTime(capacity.probedAt) }}</b>
            </div>
            <div class="capacity-cell">
              <span>CPU 核数</span>
              <b>{{ capacity.cores ?? '—' }} 核</b>
            </div>
            <div class="capacity-cell">
              <span>检测峰值 CPU</span>
              <b>{{ capacity.cpuPeak ? `${capacity.cpuPeak.toFixed(1)} 核` : '—' }}</b>
            </div>
            <div class="capacity-cell">
              <span>饱和并发</span>
              <b>{{ capacity.saturationConcurrent ?? '—' }}</b>
            </div>
          </div>
          <div v-if="capacity" class="capacity-note">
            回环检测值，非目标系统容量，实际受网络 RTT 与目标侧影响。
            <template v-if="capacityUsage !== null && capacityUsage < 50">
              客户端余量充足，瓶颈不在施压端。
            </template>
            <template v-else-if="capacityUsage !== null && capacityUsage >= 80">
              客户端接近能力上限，结论需谨慎。
            </template>
          </div>
          <div v-else class="capacity-note">
            在「设置 → 本机施压能力」完成检测后，新生成的报告会自动带上该快照；导出报告不会推断数值。
          </div>
        </div>

        <div v-if="config?.baselineReportId" class="fm-card baseline-card">
          <div class="card-head">
            <div>
              <div class="card-title">基线对比</div>
              <div class="card-sub">对照历史报告 #{{ config.baselineReportId }}，延迟上升或吞吐下降需关注</div>
            </div>
            <a-spin v-if="baselineLoading" size="small" />
            <span v-else-if="!baseline" class="capacity-badge muted">基线报告不存在或已删除</span>
          </div>
          <template v-if="baseline">
            <div class="baseline-head">
              <span>{{ baseline.report.planName }}</span>
              <span>{{ fmtTime(baseline.report.startTime) }}</span>
              <span>并发 {{ baseline.report.concurrency }}</span>
              <a-button type="link" size="small" @click="router.push(`/reports/${baseline!.report.id}`)">
                查看基线
              </a-button>
            </div>
            <div class="baseline-table">
              <div class="baseline-row baseline-head-row">
                <span>指标</span>
                <span>本次</span>
                <span>基线</span>
                <span>变化</span>
              </div>
              <div v-for="row in baselineRows" :key="row.label" class="baseline-row">
                <span class="baseline-label">{{ row.label }}</span>
                <span class="mono">{{ row.current }}</span>
                <span class="mono muted">{{ row.base }}</span>
                <span class="mono" :class="row.tone">{{ row.delta }}</span>
              </div>
            </div>
          </template>
        </div>

        <div class="fm-card text-card">
          <div class="card-head">
            <div>
              <div class="card-title">测试场景与报告总结</div>
              <div class="card-sub">可编辑，保存后随报告归档，并同步进入 HTML 报告</div>
            </div>
            <a-button size="small" :loading="textSaving" @click="saveTexts">保存</a-button>
          </div>
          <div class="text-grid">
            <div class="text-field">
              <span class="text-label">测试场景</span>
              <a-textarea
                v-model:value="scenarioDraft"
                :rows="3"
                :maxlength="2000"
                placeholder="本次压测的业务场景、目的、数据构造与前置条件…"
              />
            </div>
            <div class="text-field">
              <span class="text-label">报告总结</span>
              <a-textarea
                v-model:value="summaryDraft"
                :rows="3"
                :maxlength="3000"
                placeholder="结论、瓶颈判断与后续优化建议…"
              />
            </div>
          </div>
        </div>

        <div v-if="slaChecks.length" class="fm-card sla-card">
          <div class="card-head">
            <div>
              <div class="card-title">SLA 达标判定</div>
              <div class="card-sub">压测配置中声明的验收阈值</div>
            </div>
            <span class="sla-badge" :class="slaPassed ? 'ok' : 'bad'">
              {{ slaPassed ? '全部达标' : '存在未达标项' }}
            </span>
          </div>
          <div class="sla-rows">
            <div v-for="row in slaChecks" :key="row.label" class="sla-row">
              <span class="sla-label">{{ row.label }}</span>
              <span class="sla-target mono">{{ row.target }}</span>
              <span class="sla-actual mono">{{ row.actual }}</span>
              <span class="sla-result" :class="row.pass ? 'ok' : 'bad'">{{ row.pass ? '达标' : '未达标' }}</span>
            </div>
          </div>
        </div>

        <div class="two-col">
          <div class="fm-card table-card">
            <div class="card-head">
              <div>
                <div class="card-title">统计表</div>
                <div class="card-sub">对齐 JMeter Statistics；90% QPS 已剔除升压段</div>
              </div>
              <span v-if="recordModeText" class="head-note">{{ recordModeText }}</span>
            </div>
            <div class="percentile-grid">
              <div v-for="row in statsRows" :key="row.label" class="percentile-cell">
                <span class="p-label">{{ row.label }}</span>
                <span class="p-value mono">{{ row.value }}</span>
              </div>
            </div>
            <div v-if="metrics" class="extra-row">
              <div class="extra-item">
                <span class="p-label">明细丢弃</span>
                <span class="p-value mono">{{ (metrics.recordsDropped ?? 0).toLocaleString() }} 条</span>
              </div>
              <div class="extra-item">
                <span class="p-label">错误丢弃</span>
                <span class="p-value mono">{{ (metrics.errorsDropped ?? 0).toLocaleString() }} 条</span>
              </div>
              <div class="extra-item">
                <span class="p-label">施压端峰值 CPU</span>
                <span class="p-value mono">
                  {{ metrics.cpuPeakCores ? `${metrics.cpuPeakCores.toFixed(1)} 核` : '未采集' }}
                </span>
              </div>
            </div>
          </div>

          <div class="fm-card table-card">
            <div class="card-head">
              <div>
                <div class="card-title">自动分析</div>
                <div class="card-sub">拐点检测与四维体检，规则可解释</div>
              </div>
              <span class="head-note">
                {{ score?.total != null ? `综合 ${score.total.toFixed(0)}（${score.grade}）` : '—' }}
              </span>
            </div>
            <div v-if="knee" class="analysis-note">{{ knee.note }}</div>
            <div v-if="score" class="score-list">
              <div v-for="dim in score.dimensions" :key="dim.key" class="score-row">
                <span class="score-label">{{ dim.label }}</span>
                <div class="score-bar-wrap">
                  <div class="score-bar" :style="{ width: `${dim.score ?? 0}%` }" />
                </div>
                <span class="score-value mono">{{ dim.score === null ? '—' : dim.score.toFixed(0) }}</span>
                <span class="score-detail">{{ dim.detail }}</span>
              </div>
            </div>
            <div class="checks">
              <div v-for="item in checks" :key="item.key" class="check-row">
                <span :class="item.pass ? 'check-ok' : 'check-bad'">{{ item.pass ? '✓' : '!' }}</span>
                <span class="check-label">{{ item.label }}</span>
                <span class="check-detail">{{ item.detail }}</span>
              </div>
            </div>
            <div v-if="score" class="rule-note">{{ score.ruleNote }}</div>
          </div>
        </div>

        <div class="two-col">
          <div class="fm-card table-card">
            <div class="card-head">
              <div>
                <div class="card-title">延迟分位统计</div>
                <div class="card-sub">全量请求的分布，单位毫秒</div>
              </div>
              <span class="head-note">σ {{ fmtNum(report.stddevRt) }} ms</span>
            </div>
            <div class="percentile-grid">
              <div v-for="row in percentileRows" :key="row.label" class="percentile-cell">
                <span class="p-label">{{ row.label }}</span>
                <span class="p-value mono">{{ fmtNum(row.value) }}</span>
              </div>
            </div>
            <div class="extra-row">
              <div class="extra-item">
                <span class="p-label">TTFB 平均</span>
                <span class="p-value mono">{{ fmtNum(report.ttfbAvg) }} ms</span>
              </div>
              <div class="extra-item">
                <span class="p-label">TTFB P95</span>
                <span class="p-value mono">{{ fmtNum(report.ttfbP95) }} ms</span>
              </div>
              <div class="extra-item">
                <span class="p-label">Body 平均</span>
                <span class="p-value mono">{{ fmtNum(avgBody) }} ms</span>
              </div>
              <div class="extra-item">
                <span class="p-label">Body P95</span>
                <span class="p-value mono">{{ fmtNum(p95Body) }} ms</span>
              </div>
              <div class="extra-item">
                <span class="p-label">总传输字节</span>
                <span class="p-value mono">{{ fmtBytes(report.bytesTotal) }}</span>
              </div>
            </div>
          </div>

          <div class="fm-card table-card">
            <div class="card-head">
              <div>
                <div class="card-title">状态码分布</div>
                <div class="card-sub">按响应状态码统计的请求占比</div>
              </div>
              <span class="head-note">{{ codeTotal.toLocaleString() }} 次</span>
            </div>
            <div v-if="statusCodes.length" class="code-list">
              <div v-for="item in statusCodes" :key="item.code" class="code-row">
                <span class="code-tag" :class="codeTone(item.code)">{{ item.code }}</span>
                <div class="code-bar-wrap">
                  <div
                    class="code-bar"
                    :class="codeTone(item.code)"
                    :style="{ width: `${Math.max(2, (item.count / maxCodeCount) * 100)}%` }"
                  />
                </div>
                <span class="code-count mono">{{ item.count.toLocaleString() }}</span>
                <span class="code-ratio mono">
                  {{ codeTotal > 0 ? ((item.count / codeTotal) * 100).toFixed(2) : '0.00' }}%
                </span>
              </div>
            </div>
            <div v-else class="inline-empty">没有可用的状态码统计（历史报告可能未记录）</div>

            <div v-if="errorTypes.length" class="error-type-block">
              <div class="block-title">错误类型分布</div>
              <div v-for="item in errorTypes" :key="item.errorType" class="code-row">
                <span class="code-tag error">{{ item.errorType }}</span>
                <div class="code-bar-wrap">
                  <div
                    class="code-bar bad"
                    :style="{ width: `${Math.max(2, (item.count / maxErrorTypeCount) * 100)}%` }"
                  />
                </div>
                <span class="code-count mono">{{ item.count }}</span>
                <span class="code-ratio mono">
                  {{ report.errorCount > 0 ? ((item.count / report.errorCount) * 100).toFixed(1) : '0.0' }}%
                </span>
              </div>
            </div>
          </div>
        </div>

        <div class="charts">
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">吞吐与并发</span>
              <span class="chart-sub">QPS 与实时并发数</span>
            </div>
            <LineChart :series="qpsSeries" :labels="chartLabels" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">响应延迟分位</span>
              <span class="chart-sub">单位 ms</span>
            </div>
            <LineChart :series="latencySeries" :labels="chartLabels" unit="ms" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">错误率</span>
              <span class="chart-sub">每秒失败请求占比（%）</span>
            </div>
            <LineChart :series="errorSeries" :labels="chartLabels" unit="%" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">延迟直方图</span>
              <span class="chart-sub">请求落在各延迟区间的数量</span>
            </div>
            <div v-if="histogram.length" class="histogram">
              <div v-for="bucket in histogram" :key="bucket.label" class="hist-row">
                <span class="hist-label">{{ bucket.label }}</span>
                <div class="hist-track">
                  <div
                    class="hist-bar"
                    :style="{ width: `${(bucket.count / maxHistogram) * 100}%` }"
                  />
                </div>
                <span class="hist-count mono">{{ bucket.count.toLocaleString() }}</span>
              </div>
            </div>
            <div v-else class="inline-empty">没有直方图数据</div>
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">响应体读取耗时</span>
              <span class="chart-sub">body = 总耗时 − TTFB，单位 ms</span>
            </div>
            <div v-if="bodyHistogram.length" class="histogram">
              <div v-for="bucket in bodyHistogram" :key="bucket.label" class="hist-row">
                <span class="hist-label">{{ bucket.label }}</span>
                <div class="hist-track">
                  <div
                    class="hist-bar body"
                    :style="{ width: `${(bucket.count / maxBodyHistogram) * 100}%` }"
                  />
                </div>
                <span class="hist-count mono">{{ bucket.count.toLocaleString() }}</span>
              </div>
            </div>
            <div v-else class="inline-empty">历史报告未记录响应体耗时直方图</div>
          </div>
        </div>

        <div v-if="stages.length" class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">阶段汇总</div>
              <div class="card-sub">阶梯加压各阶段的执行情况</div>
            </div>
          </div>
          <div class="data-table">
            <table>
              <thead>
                <tr>
                  <th class="col-narrow">阶段</th>
                  <th class="col-narrow">并发</th>
                  <th class="col-narrow">时长</th>
                  <th>请求数</th>
                  <th>错误数</th>
                  <th>平均 QPS</th>
                  <th>平均延迟</th>
                  <th>P95</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(stage, i) in stages" :key="stage.index">
                  <td class="mono">{{ i + 1 }}</td>
                  <td class="mono">{{ stage.concurrent }}</td>
                  <td class="mono">{{ stage.durationSec }}s</td>
                  <td class="mono">{{ stage.requests.toLocaleString() }}</td>
                  <td class="mono" :class="{ bad: stage.errors > 0 }">{{ stage.errors }}</td>
                  <td class="mono">{{ fmtNum(stage.avgQps) }}</td>
                  <td class="mono">{{ fmtNum(stage.avgRt) }} ms</td>
                  <td class="mono">{{ fmtNum(stage.p95Rt) }} ms</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">每秒明细</div>
              <div class="card-sub">共 {{ samples.length }} 条采样，按时间顺序排列</div>
            </div>
            <span class="head-note">{{ samples.length }} 秒</span>
          </div>
          <div class="data-table sample-table">
            <table>
              <thead>
                <tr>
                  <th class="col-narrow">秒</th>
                  <th>并发</th>
                  <th>QPS</th>
                  <th>平均</th>
                  <th>P50</th>
                  <th>P95</th>
                  <th>P99</th>
                  <th>TTFB</th>
                  <th>流量</th>
                  <th>成功</th>
                  <th>失败</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="s in samples" :key="s.elapsedSec">
                  <td class="mono">{{ s.elapsedSec }}</td>
                  <td class="mono">{{ s.concurrent }}</td>
                  <td class="mono">{{ s.qps }}</td>
                  <td class="mono">{{ fmtNum(s.avgRt) }}</td>
                  <td class="mono">{{ fmtNum(s.p50Rt) }}</td>
                  <td class="mono">{{ fmtNum(s.p95Rt) }}</td>
                  <td class="mono">{{ fmtNum(s.p99Rt) }}</td>
                  <td class="mono">{{ fmtNum(s.ttfbAvg) }}</td>
                  <td class="mono">{{ fmtBytes(s.bytes) }}</td>
                  <td class="mono">{{ s.successCount }}</td>
                  <td class="mono" :class="{ bad: s.errorCount > 0 }">{{ s.errorCount }}</td>
                </tr>
              </tbody>
            </table>
            <div v-if="!samples.length" class="inline-empty">没有采样数据</div>
          </div>
        </div>

        <div class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">压测配置快照</div>
              <div class="card-sub">生成报告时使用的参数，便于复现</div>
            </div>
          </div>
          <div v-if="config" class="config-grid">
            <div class="config-item">
              <span class="c-label">加压模式</span>
              <span class="c-value mono">{{ config.mode === 'stages' ? '阶梯加压' : '固定并发' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">请求方法</span>
              <span class="c-value mono">{{ config.method }}</span>
            </div>
            <div class="config-item wide">
              <span class="c-label">请求 URL</span>
              <span
                v-if="targetUrlInfo.url"
                class="c-value mono c-value-url"
                :title="`${targetUrlInfo.url}（点击复制）`"
                @click="copyText(targetUrlInfo.url, '地址')"
                >{{ targetUrlInfo.url }}</span
              >
              <span v-else class="c-value mono c-value-url missing">{{ targetUrlInfo.note }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">请求头</span>
              <span class="c-value mono">{{ config.headers }} 项</span>
            </div>
            <div class="config-item">
              <span class="c-label">查询参数</span>
              <span class="c-value mono">{{ config.params }} 项</span>
            </div>
            <div class="config-item">
              <span class="c-label">Body 类型</span>
              <span class="c-value mono">{{ config.bodyType || 'none' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">超时</span>
              <span class="c-value mono">{{ config.timeoutMs }} ms</span>
            </div>
            <div class="config-item">
              <span class="c-label">思考时间</span>
              <span class="c-value mono">{{ config.thinkTimeMs }} ms</span>
            </div>
            <div class="config-item">
              <span class="c-label">预热</span>
              <span class="c-value mono">{{ config.rampUpSec }} s</span>
            </div>
            <div class="config-item">
              <span class="c-label">目标 QPS</span>
              <span class="c-value mono">{{ config.targetRps ? fmtNum(config.targetRps, 0) : '不限速' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">环境</span>
              <span class="c-value mono">{{ config.envId ?? '未使用' }}</span>
            </div>
          </div>
          <div v-else class="inline-empty">历史报告未保存配置快照</div>
        </div>

        <div v-if="slowRequests.length" class="section-head">
          <div>
            <h2 class="section-title">TopN 慢请求</h2>
            <div class="section-desc">
              定位模式保留的最慢 {{ slowRequests.length }} 条请求，按总耗时降序
            </div>
          </div>
        </div>
        <div v-if="slowRequests.length" class="fm-card request-card">
          <div class="data-table request-table">
            <table>
              <thead>
                <tr>
                  <th class="req-index">#</th>
                  <th class="req-time">发生时间</th>
                  <th class="req-method-col">方法</th>
                  <th class="req-url-col">URL</th>
                  <th>状态</th>
                  <th>总耗时</th>
                  <th>TTFB</th>
                  <th>Body</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(item, index) in slowRequests" :key="`${item.elapsedMs}-${index}`">
                  <td class="req-index mono">{{ index + 1 }}</td>
                  <td class="req-time mono">{{ fmtDuration(item.elapsedMs) }}</td>
                  <td class="req-method-col">
                    <span class="req-method">{{ item.method }}</span>
                  </td>
                  <td class="req-url-col mono" :title="item.url">{{ item.url }}</td>
                  <td class="mono" :class="{ bad: item.status >= 400 }">{{ item.status || '-' }}</td>
                  <td class="mono slow-cell">
                    <span class="slow-bar" :style="{ width: `${(item.latencyMs / slowMaxLatency) * 100}%` }" />
                    <span>{{ fmtNum(item.latencyMs, 0) }} ms</span>
                  </td>
                  <td class="mono">{{ fmtNum(item.ttfbMs, 0) }} ms</td>
                  <td class="mono">{{ fmtNum(item.bodyMs, 0) }} ms</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div class="section-head">
          <div>
            <h2 class="section-title">请求明细</h2>
            <div class="section-desc">
              逐条记录每次请求的时间、耗时、状态与结果{{
                requestTotal ? `，共 ${requestTotal.toLocaleString()} 条` : ''
              }}{{ requestTotal >= 50000 ? '（最多保留前 50000 条）' : '' }}
            </div>
          </div>
          <a-radio-group
            v-model:value="requestFilter"
            size="small"
            button-style="solid"
            @change="onRequestFilterChange"
          >
            <a-radio-button value="all">全部</a-radio-button>
            <a-radio-button value="success">成功</a-radio-button>
            <a-radio-button value="failed">失败</a-radio-button>
          </a-radio-group>
        </div>

        <div class="fm-card request-card">
          <a-spin :spinning="requestsLoading">
            <div v-if="requestItems.length" class="data-table request-table">
              <table>
                <thead>
                  <tr>
                    <th class="req-index">#</th>
                    <th class="req-time">请求时间</th>
                    <th class="req-method-col">方法</th>
                    <th class="req-url-col">URL</th>
                    <th class="req-body-col">请求体</th>
                    <th>状态</th>
                    <th>耗时</th>
                    <th>TTFB</th>
                    <th>大小</th>
                    <th>结果</th>
                    <th class="req-error-col">错误信息</th>
                    <th class="req-detail-col">详情</th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="(item, index) in requestItems"
                    :key="`${item.timeMs}-${index}`"
                    class="req-row"
                    @click="openRecord(item)"
                  >
                    <td class="req-index mono">
                      {{ (requestPage - 1) * requestPageSize + index + 1 }}
                    </td>
                    <td class="req-time mono">{{ fmtTime(item.timeMs) }}</td>
                    <td class="req-method-col">
                      <span class="req-method">{{ item.method }}</span>
                    </td>
                    <td class="req-url-col mono" :title="item.url">{{ item.url }}</td>
                    <td class="req-body-col mono" :title="item.body ?? ''">
                      {{ item.body || '-' }}
                    </td>
                    <td class="mono" :class="{ bad: item.status >= 400 }">
                      {{ item.status || '-' }}
                    </td>
                    <td class="mono" :class="{ bad: item.latencyMs >= 1000 }">
                      {{ fmtNum(item.latencyMs, 0) }} ms
                    </td>
                    <td class="mono">{{ fmtNum(item.ttfbMs, 0) }} ms</td>
                    <td class="mono">{{ fmtBytes(item.bytes) }}</td>
                    <td>
                      <span class="req-result" :class="item.success ? 'ok' : 'bad'">
                        {{ item.success ? '成功' : '失败' }}
                      </span>
                    </td>
                    <td class="req-error-col" :title="item.errorMessage ?? ''">
                      <span v-if="item.errorType" class="req-error-type">{{ item.errorType }}</span>
                      <span v-if="item.errorMessage" class="req-error-msg">{{ item.errorMessage }}</span>
                      <span v-else-if="!item.errorType">-</span>
                    </td>
                    <td class="req-detail-col">
                      <span class="req-detail-link"><EyeOutlined /> 查看</span>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div v-else-if="!requestsLoading" class="inline-empty">
              {{
                requestFilter === 'all'
                  ? '本次压测没有记录请求明细（旧版本报告不包含逐请求数据）'
                  : '没有符合条件的请求'
              }}
            </div>
            <div class="request-pager">
              <a-pagination
                v-model:current="requestPage"
                v-model:page-size="requestPageSize"
                :total="requestTotal"
                :page-size-options="[20, 50, 100, 200]"
                :show-total="(total: number) => `共 ${total.toLocaleString()} 条`"
                show-size-changer
                show-quick-jumper
                size="small"
                @change="onRequestPageChange"
              />
            </div>
          </a-spin>
        </div>

        <a-drawer
          v-model:open="recordOpen"
          title="请求详情"
          placement="right"
          :width="640"
          :body-style="{ padding: '18px 20px' }"
        >
          <template v-if="recordItem">
            <div class="rec-head">
              <span class="req-method">{{ recordItem.method }}</span>
              <span class="req-result" :class="recordItem.success ? 'ok' : 'bad'">
                {{ recordItem.success ? '成功' : '失败' }}
              </span>
              <span class="rec-status mono" :class="{ bad: recordItem.status >= 400 }">
                {{ recordItem.status || '无响应' }}
              </span>
              <span class="rec-elapsed">压测第 {{ recordItem.elapsedMs }} ms</span>
            </div>

            <div class="rec-url mono">{{ recordItem.url }}</div>

            <a-descriptions :column="2" size="small" bordered class="rec-desc">
              <a-descriptions-item label="请求时间" :span="2">
                {{ fmtFullTime(recordItem.timeMs) }}
              </a-descriptions-item>
              <a-descriptions-item label="耗时">
                {{ fmtNum(recordItem.latencyMs, 0) }} ms
              </a-descriptions-item>
              <a-descriptions-item label="TTFB">
                {{ fmtNum(recordItem.ttfbMs, 0) }} ms
              </a-descriptions-item>
              <a-descriptions-item label="响应大小">
                {{ fmtBytes(recordItem.bytes) }}
              </a-descriptions-item>
              <a-descriptions-item label="状态码">
                {{ recordItem.status || '无响应' }}
              </a-descriptions-item>
            </a-descriptions>

            <a-tabs v-model:activeKey="recordTab" size="small" class="rec-tabs">
              <a-tab-pane key="request">
                <template #tab>
                  请求参数
                  <span class="tab-badge">
                    {{ recordParams.length + recordReqHeaders.length + (recordBody ? 1 : 0) }}
                  </span>
                </template>

                <div class="rec-block first">
                  <div class="rec-block-head">
                    <span>查询参数</span>
                    <span class="rec-note">{{ recordParams.length }} 项</span>
                  </div>
                  <div v-if="recordParams.length" class="kv-list">
                    <div v-for="(p, i) in recordParams" :key="`p-${i}`" class="kv-row">
                      <span class="kv-key mono">{{ p.key }}</span>
                      <span class="kv-value mono">{{ p.value }}</span>
                    </div>
                  </div>
                  <div v-else class="rec-empty">该请求没有查询参数</div>
                </div>

                <div class="rec-block">
                  <div class="rec-block-head">
                    <span>请求头</span>
                    <span class="rec-note">{{ recordReqHeaders.length }} 项</span>
                  </div>
                  <div v-if="recordReqHeaders.length" class="kv-list">
                    <div v-for="(h, i) in recordReqHeaders" :key="`h-${i}`" class="kv-row">
                      <span class="kv-key mono">{{ h.key }}</span>
                      <span class="kv-value mono">{{ h.value }}</span>
                    </div>
                  </div>
                  <div v-else class="rec-empty">该请求没有额外请求头</div>
                </div>

                <div class="rec-block">
                  <div class="rec-block-head">
                    <span>请求体</span>
                    <span v-if="recordBody" class="rec-note">最多保留 1000 字符</span>
                    <a-button
                      v-if="recordBody"
                      type="link"
                      size="small"
                      class="rec-copy"
                      @click="copyRecordBody"
                    >
                      <CopyOutlined /> 复制
                    </a-button>
                  </div>
                  <pre v-if="recordBody" class="mono rec-body">{{ recordBody }}</pre>
                  <div v-else class="rec-empty">该请求没有请求体（GET / HEAD 或未配置 Body）</div>
                </div>
              </a-tab-pane>

              <a-tab-pane key="response">
                <template #tab>
                  返回结果
                  <span class="tab-badge">{{ recordResHeaders.length + (recordResBody ? 1 : 0) }}</span>
                </template>

                <div class="rec-block first">
                  <div class="rec-block-head">
                    <span>响应头</span>
                    <span class="rec-note">{{ recordResHeaders.length }} 项</span>
                  </div>
                  <div v-if="recordResHeaders.length" class="kv-list">
                    <div v-for="(h, i) in recordResHeaders" :key="`rh-${i}`" class="kv-row">
                      <span class="kv-key mono">{{ h.key }}</span>
                      <span class="kv-value mono">{{ h.value }}</span>
                    </div>
                  </div>
                  <div v-else class="rec-empty">没有记录到响应头</div>
                </div>

                <div class="rec-block">
                  <div class="rec-block-head">
                    <span>响应体</span>
                    <span v-if="recordResBody" class="rec-note">最多保留 2000 字符</span>
                    <a-button
                      v-if="recordResBody"
                      type="link"
                      size="small"
                      class="rec-copy"
                      @click="copyRecordResBody"
                    >
                      <CopyOutlined /> 复制
                    </a-button>
                  </div>
                  <pre v-if="recordResBody" class="mono rec-body">{{ recordResBody }}</pre>
                  <div v-else class="rec-empty">没有记录到响应体（连接失败或响应为空）</div>
                </div>

                <div v-if="recordItem.errorType || recordItem.errorMessage" class="rec-block">
                  <div class="rec-block-head">
                    <span>错误信息</span>
                  </div>
                  <div class="rec-error">
                    <span v-if="recordItem.errorType" class="req-error-type">
                      {{ recordItem.errorType }}
                    </span>
                    <span class="rec-error-msg">{{ recordItem.errorMessage || '-' }}</span>
                  </div>
                </div>
              </a-tab-pane>
            </a-tabs>
          </template>
        </a-drawer>

        <div class="section-head">
          <div>
            <h2 class="section-title">错误明细</h2>
            <div class="section-desc">
              {{ errors.length ? `共记录 ${errors.length} 条，最多保留 500 条` : '本次压测没有出现错误' }}
            </div>
          </div>
        </div>

        <div v-if="errors.length" class="error-list">
          <div v-for="(e, i) in errors" :key="i" class="fm-card error-item">
            <div class="error-head">
              <span class="error-type" :class="{ http: e.errorType === 'HTTP' }">{{ e.errorType }}</span>
              <span v-if="e.responseStatus" class="error-status mono">{{ e.responseStatus }}</span>
              <span class="error-time">第 {{ e.elapsedSec }} 秒</span>
              <span class="error-url mono" :title="e.requestUrl">{{ e.requestUrl }}</span>
            </div>
            <div class="error-message">{{ e.errorMessage }}</div>
            <pre v-if="e.responseBody" class="error-body mono">{{ e.responseBody }}</pre>
          </div>
        </div>
        <div v-else class="fm-card empty-hint">
          <span class="empty-icon">✓</span>
          没有错误记录，所有请求均返回 2xx / 3xx
        </div>
      </template>

      <div v-else-if="!loading" class="fm-card empty-hint">报告不存在或已被删除</div>
    </a-spin>
  </div>
</template>

<style scoped>
.overview {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 20px 22px;
}

.title-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.status-dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--fm-success);
}

.status-dot.stopped {
  background: var(--fm-warning);
}

.report-name {
  font-size: 18px;
  font-weight: 700;
  margin: 0;
  letter-spacing: -0.2px;
}

.status-pill {
  font-size: 10px;
  padding: 1px 9px;
  border-radius: 999px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.status-pill.stopped {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.mode-pill {
  font-size: 10px;
  padding: 1px 9px;
  border-radius: 999px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.meta-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  font-size: 11px;
  color: var(--fm-muted);
}

.dot-sep {
  color: var(--fm-border-strong);
}

.target-line {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: 8px;
  max-width: 100%;
}

.method-chip {
  flex-shrink: 0;
  padding: 2px 8px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.3px;
}

.target-url {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--fm-text-2);
  white-space: normal;
  word-break: break-all;
  cursor: copy;
}

.verdict {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 10px 16px;
  border-radius: var(--fm-radius);
  background: var(--fm-success-soft);
  color: var(--fm-success);
  flex-shrink: 0;
  max-width: 360px;
}

.verdict.warn {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.verdict.bad {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.verdict :deep(.anticon) {
  font-size: 22px;
  flex-shrink: 0;
}

.verdict-title {
  font-size: 13px;
  font-weight: 700;
}

.verdict-desc {
  margin-top: 2px;
  font-size: 10px;
  line-height: 1.5;
  opacity: 0.85;
}

.metrics {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 10px;
  margin-top: 16px;
}

.metric-card {
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.m-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.m-value {
  font-size: 16px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
}

.m-value i {
  font-style: normal;
  font-size: 9px;
  font-weight: 500;
  color: var(--fm-muted);
  margin-left: 2px;
}

.m-value.warn {
  color: var(--fm-warning);
}

.m-value.bad {
  color: var(--fm-danger);
}

.m-value.ok {
  color: var(--fm-success);
}

/* ---------- 通用卡片头 ---------- */
.card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.card-title {
  font-size: 13px;
  font-weight: 600;
}

.card-sub {
  margin-top: 3px;
  font-size: 10px;
  color: var(--fm-muted);
}

.head-note {
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- SLA ---------- */
.sla-card {
  margin-top: 16px;
  padding: 16px 18px;
}

.sla-badge {
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 10px;
  font-weight: 600;
}

.sla-badge.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.sla-badge.bad {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.sla-rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.sla-row {
  display: grid;
  grid-template-columns: 1.2fr 1fr 1fr auto;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  font-size: 12px;
}

.sla-label {
  color: var(--fm-text-2);
  font-weight: 500;
}

.sla-target,
.sla-actual {
  color: var(--fm-muted);
}

.sla-actual {
  color: var(--fm-text);
  font-weight: 600;
}

.sla-result {
  font-size: 10px;
  font-weight: 600;
  padding: 1px 9px;
  border-radius: 999px;
}

.sla-result.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.sla-result.bad {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

/* ---------- 两列区块 ---------- */
.two-col {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 16px;
}

.table-card {
  padding: 16px 18px;
  margin-top: 16px;
  min-width: 0;
}

.two-col .table-card {
  margin-top: 0;
}

.percentile-grid {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 8px;
}

.percentile-cell {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 10px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.p-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.p-value {
  font-size: 14px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.extra-row {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  margin-top: 10px;
}

.extra-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 10px;
  border-radius: var(--fm-radius);
  border: 1px dashed var(--fm-border);
}

/* ---------- 状态码 ---------- */
.code-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.code-row {
  display: grid;
  grid-template-columns: 58px 1fr 66px 60px;
  align-items: center;
  gap: 10px;
  font-size: 11px;
}

.code-tag {
  text-align: center;
  padding: 2px 0;
  border-radius: var(--fm-radius-sm);
  font-weight: 700;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.code-tag.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.code-tag.info {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.code-tag.warn {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.code-tag.bad,
.code-tag.error {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.code-bar-wrap {
  height: 7px;
  border-radius: 999px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.code-bar {
  height: 100%;
  border-radius: 999px;
  background: var(--fm-success);
}

.code-bar.info {
  background: var(--fm-primary);
}

.code-bar.warn {
  background: var(--fm-warning);
}

.code-bar.bad {
  background: var(--fm-danger);
}

.code-count {
  text-align: right;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.code-ratio {
  text-align: right;
  color: var(--fm-muted);
}

.error-type-block {
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px dashed var(--fm-border);
}

.block-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
  margin-bottom: 8px;
}

/* ---------- 图表 ---------- */
.charts {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 16px;
}

.chart-card {
  padding: 14px 18px 6px;
  min-width: 0;
}

.chart-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 2px;
}

.chart-title {
  font-size: 13px;
  font-weight: 600;
}

.chart-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 直方图 ---------- */
.histogram {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px 0 8px;
}

.hist-row {
  display: grid;
  grid-template-columns: 78px 1fr 62px;
  align-items: center;
  gap: 10px;
  font-size: 10px;
}

.hist-label {
  color: var(--fm-muted);
  text-align: right;
}

.hist-track {
  height: 9px;
  border-radius: 999px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.hist-bar {
  height: 100%;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--fm-primary), var(--fm-primary-dark));
  transition: width 0.3s ease;
}

.hist-bar.body {
  background: linear-gradient(90deg, var(--fm-warning), var(--fm-danger));
}

/* TopN 慢请求：耗时条嵌在单元格里，直观看出最慢的那几条 */
.slow-cell {
  position: relative;
  min-width: 110px;
}

.slow-bar {
  position: absolute;
  inset: 4px auto 4px 4px;
  border-radius: 3px;
  background: color-mix(in srgb, var(--fm-danger) 18%, transparent);
  pointer-events: none;
}

.slow-cell span:last-child {
  position: relative;
}

.hist-count {
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

/* ---------- 数据表格 ---------- */
.data-table {
  overflow: auto;
}

.data-table table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.data-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  padding: 7px 10px;
  text-align: right;
  font-weight: 500;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  white-space: nowrap;
}

.data-table th:first-child,
.data-table td:first-child {
  text-align: left;
}

.data-table td {
  padding: 6px 10px;
  text-align: right;
  border-bottom: 1px solid var(--fm-border);
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.data-table tbody tr:hover td {
  background: var(--fm-surface-alt);
}

.data-table td.bad {
  color: var(--fm-danger);
  font-weight: 600;
}

.col-narrow {
  width: 62px;
}

.sample-table {
  max-height: 360px;
}

/* ---------- 请求明细 ---------- */
.request-card {
  padding: 10px 12px 8px;
}

.request-table {
  max-height: 440px;
}

.request-table th,
.request-table td {
  padding: 6px 8px;
}

.request-table th.req-index,
.request-table td.req-index {
  width: 56px;
  text-align: right;
  color: var(--fm-muted);
}

.request-table th.req-time,
.request-table td.req-time {
  width: 158px;
  text-align: left;
  white-space: nowrap;
}

.request-table th.req-method-col,
.request-table td.req-method-col {
  width: 72px;
  text-align: left;
}

.request-table th.req-url-col,
.request-table td.req-url-col {
  text-align: left;
  min-width: 220px;
  max-width: 380px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.request-table th.req-body-col,
.request-table td.req-body-col {
  text-align: left;
  min-width: 160px;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-muted);
}

.request-table th.req-error-col,
.request-table td.req-error-col {
  text-align: left;
  min-width: 180px;
  max-width: 300px;
  white-space: normal;
  word-break: break-all;
  color: var(--fm-muted);
}

.req-method {
  display: inline-block;
  padding: 0 6px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 10px;
  line-height: 17px;
}

.req-result {
  display: inline-block;
  padding: 0 7px;
  border-radius: 999px;
  font-size: 10px;
  line-height: 17px;
}

.req-result.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.req-result.bad {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.req-error-type {
  display: inline-block;
  margin-right: 6px;
  padding: 0 5px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
  font-size: 10px;
  line-height: 16px;
}

.req-error-msg {
  font-size: 10.5px;
}

.request-table tbody tr.req-row {
  cursor: pointer;
}

.request-table tbody tr.req-row:hover td {
  background: var(--fm-primary-soft);
}

.req-detail-col {
  width: 66px;
  text-align: center;
}

.req-detail-link {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 10.5px;
  color: var(--fm-primary);
  opacity: 0.65;
  transition: opacity 0.16s;
}

.req-row:hover .req-detail-link {
  opacity: 1;
}

/* ---------- 请求详情抽屉 ---------- */
.rec-head {
  display: flex;
  align-items: center;
  gap: 9px;
}

.rec-status {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.rec-status.bad {
  color: var(--fm-danger);
}

.rec-elapsed {
  margin-left: auto;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.rec-url {
  margin: 10px 0 14px;
  padding: 9px 11px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-text-2);
  word-break: break-all;
}

.rec-desc :deep(.ant-descriptions-item-label) {
  width: 84px;
  font-size: 11px;
  color: var(--fm-muted);
}

.rec-desc :deep(.ant-descriptions-item-content) {
  font-size: 11.5px;
}

.rec-block {
  margin-top: 18px;
}

.rec-block.first {
  margin-top: 4px;
}

.rec-tabs {
  margin-top: 4px;
}

.rec-tabs :deep(.ant-tabs-nav) {
  margin-bottom: 2px;
}

.tab-badge {
  display: inline-block;
  min-width: 16px;
  margin-left: 5px;
  padding: 0 5px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 9.5px;
  line-height: 15px;
  text-align: center;
}

.kv-list {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.kv-row {
  display: flex;
  gap: 12px;
  padding: 6px 11px;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 11px;
  line-height: 1.6;
}

.kv-row:last-child {
  border-bottom: none;
}

.kv-key {
  width: 38%;
  flex-shrink: 0;
  color: var(--fm-primary);
  word-break: break-all;
}

.kv-value {
  flex: 1;
  min-width: 0;
  color: var(--fm-text-2);
  word-break: break-all;
}

.rec-block-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.rec-note {
  font-size: 10px;
  font-weight: 400;
  color: var(--fm-muted);
}

.rec-copy {
  margin-left: auto;
  font-size: 10.5px;
  padding: 0;
  height: auto;
}

.rec-body {
  margin: 0;
  padding: 12px 14px;
  max-height: 380px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  word-break: break-all;
}

.rec-empty {
  padding: 12px 14px;
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
  font-size: 11px;
  color: var(--fm-muted);
}

.rec-error {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 11px 13px;
  border-radius: var(--fm-radius);
  background: var(--fm-danger-soft);
}

.rec-error-msg {
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-danger);
  word-break: break-all;
}

.request-pager {
  display: flex;
  justify-content: flex-end;
  padding-top: 10px;
}

/* ---------- 配置快照 ---------- */
.config-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
}

.config-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 11px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  min-width: 0;
}

.config-item.wide {
  grid-column: span 2;
}

.c-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.c-value {
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 配置快照里的完整 URL：占满整行、可换行、点击复制（3.1.2） */
.c-value-url {
  grid-column: 1 / -1;
  overflow: visible;
  white-space: normal;
  word-break: break-all;
  font-weight: 500;
  line-height: 1.55;
  cursor: copy;
  user-select: all;
}

.inline-empty {
  padding: 22px 0;
  text-align: center;
  font-size: 11px;
  color: var(--fm-muted);
}

/* ---------- 错误明细 ---------- */
.section-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  margin: 20px 2px 10px;
}

.section-title {
  font-size: 14px;
  font-weight: 700;
  margin: 0;
}

.section-desc {
  margin-top: 4px;
  font-size: 11px;
  color: var(--fm-muted);
}

.error-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 520px;
  overflow: auto;
}

.error-item {
  padding: 12px 16px;
}

.error-head {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 11px;
}

.error-type {
  padding: 1px 8px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
  font-weight: 600;
  font-size: 10px;
}

.error-type.http {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.error-status {
  font-weight: 700;
  color: var(--fm-text-2);
}

.error-time {
  color: var(--fm-muted);
}

.error-url {
  flex: 1;
  min-width: 0;
  text-align: right;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-message {
  margin-top: 6px;
  font-size: 12px;
  color: var(--fm-text-2);
  word-break: break-all;
}

.error-body {
  margin: 8px 0 0;
  padding: 8px 10px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 120px;
  overflow: auto;
}

.empty-hint {
  margin-top: 16px;
  padding: 26px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.empty-icon {
  display: inline-flex;
  width: 20px;
  height: 20px;
  margin-right: 6px;
  border-radius: 50%;
  align-items: center;
  justify-content: center;
  background: var(--fm-success-soft);
  color: var(--fm-success);
  font-size: 11px;
}

@media (max-width: 1180px) {
  .metrics {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .charts {
    grid-template-columns: 1fr;
  }
  .two-col {
    grid-template-columns: 1fr;
  }
  .config-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .percentile-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}

/* ---------- 场景/总结、统计表、自动分析 ---------- */
.text-card {
  padding: 18px 20px;
}
.text-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 8px;
}
.text-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.text-label {
  font-size: 12px;
  color: var(--fm-muted);
}
.analysis-note {
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text-2);
  background: var(--fm-surface-alt);
  border-left: 3px solid var(--fm-primary);
  border-radius: var(--fm-radius-sm);
  padding: 8px 10px;
  margin-bottom: 12px;
}
.score-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.score-row {
  display: grid;
  grid-template-columns: 60px 1fr 32px;
  grid-template-areas: 'label bar value' 'detail detail detail';
  align-items: center;
  gap: 4px 8px;
  font-size: 12px;
}
.score-label {
  grid-area: label;
  color: var(--fm-text-2);
}
.score-bar-wrap {
  grid-area: bar;
  height: 6px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  overflow: hidden;
}
.score-bar {
  height: 100%;
  background: var(--fm-primary);
  border-radius: var(--fm-radius-sm);
}
.score-value {
  grid-area: value;
  text-align: right;
  font-weight: 600;
}
.score-detail {
  grid-area: detail;
  font-size: 11px;
  color: var(--fm-muted);
}
.checks {
  margin-top: 12px;
  border-top: 1px dashed var(--fm-border);
  padding-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.check-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 12px;
}
.check-ok {
  color: var(--fm-success);
  font-weight: 700;
}
.check-bad {
  color: var(--fm-warning);
  font-weight: 700;
}
.check-label {
  color: var(--fm-text-2);
  white-space: nowrap;
}
.check-detail {
  color: var(--fm-muted);
  font-size: 11px;
}
.rule-note {
  margin-top: 10px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
}

/* ---------- 关键发现 ---------- */
.findings-card {
  margin-top: 12px;
}
.grade-badge {
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 999px;
  font-weight: 600;
}
.grade-badge.pass {
  color: var(--fm-success);
  background: var(--fm-success-soft);
}
.grade-badge.conditional {
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
}
.grade-badge.fail {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
}
.grade-statement {
  font-size: 12.5px;
  color: var(--fm-text);
  line-height: 1.7;
  margin-bottom: 10px;
}
.grade-reason {
  color: var(--fm-muted);
  font-size: 11.5px;
  margin-left: 6px;
}
.bias-banner {
  display: flex;
  gap: 8px;
  padding: 8px 10px;
  margin-bottom: 10px;
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
  border: 1px solid var(--fm-warning);
  border-radius: var(--fm-radius-sm);
}
.bias-banner b {
  white-space: nowrap;
}
.finding-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.finding-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  border-left-width: 3px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
}
.finding-row.bad {
  border-left-color: var(--fm-danger);
}
.finding-row.warn {
  border-left-color: var(--fm-warning);
}
.finding-row.info {
  border-left-color: var(--fm-primary);
}
.finding-row.ok {
  border-left-color: var(--fm-success);
}
.finding-tag {
  flex-shrink: 0;
  min-width: 34px;
  text-align: center;
  font-size: 10px;
  font-weight: 700;
  padding: 1px 6px;
  border-radius: 3px;
  background: var(--fm-surface);
  color: var(--fm-text-2);
  border: 1px solid var(--fm-border);
}
.finding-row.bad .finding-tag {
  color: var(--fm-danger);
  border-color: var(--fm-danger);
}
.finding-row.warn .finding-tag {
  color: var(--fm-warning);
  border-color: var(--fm-warning);
}
.finding-row.ok .finding-tag {
  color: var(--fm-success);
  border-color: var(--fm-success);
}
.finding-title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
}
.finding-detail {
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--fm-muted);
  margin-top: 2px;
}
.drift-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
  font-size: 12px;
}
.drift-label {
  color: var(--fm-text-2);
}
.drift-value {
  font-weight: 700;
}
.drift-value.ok {
  color: var(--fm-success);
}
.drift-value.warn {
  color: var(--fm-warning);
}
.drift-value.bad {
  color: var(--fm-danger);
}
.drift-rule {
  color: var(--fm-muted);
  font-size: 11px;
  margin-left: auto;
}

/* ---------- 施压端能力 ---------- */
.capacity-card {
  margin-top: 12px;
}
.capacity-badge {
  font-size: 11px;
  padding: 2px 10px;
  border-radius: 999px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  font-weight: 600;
}
.capacity-badge.muted {
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  font-weight: 400;
}
.capacity-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 8px;
}
.capacity-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
}
.capacity-cell span {
  font-size: 10.5px;
  color: var(--fm-muted);
}
.capacity-cell b {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
}
.capacity-cell b.ok {
  color: var(--fm-success);
}
.capacity-cell b.bad {
  color: var(--fm-danger);
}
.capacity-note {
  margin-top: 8px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
}

/* ---------- 基线对比 ---------- */
.baseline-card {
  margin-top: 12px;
}
.baseline-head {
  display: flex;
  align-items: center;
  gap: 16px;
  font-size: 11.5px;
  color: var(--fm-muted);
  margin-bottom: 8px;
}
.baseline-table {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--fm-border);
}
.baseline-row {
  display: grid;
  grid-template-columns: 1.2fr 1fr 1fr 1fr;
  gap: 8px;
  padding: 6px 10px;
  font-size: 12px;
  border-top: 1px solid var(--fm-border);
}
.baseline-row:first-child {
  border-top: none;
}
.baseline-head-row {
  font-size: 11px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
}
.baseline-label {
  color: var(--fm-text-2);
}
.baseline-row .muted {
  color: var(--fm-muted);
}
.baseline-row .ok {
  color: var(--fm-success);
}
.baseline-row .warn {
  color: var(--fm-warning);
}
.baseline-row .bad {
  color: var(--fm-danger);
}
.baseline-row .muted.tone {
  color: var(--fm-muted);
}
.target-note {
  font-size: 10.5px;
  color: var(--fm-warning);
  margin-left: 8px;
}
.target-url.missing,
.c-value-url.missing {
  color: var(--fm-warning);
}

@media (max-width: 1180px) {
  .text-grid {
    grid-template-columns: 1fr;
  }
}
</style>
