<script setup lang="ts">
import { computed, onMounted, ref, shallowRef, triggerRef, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import dayjs, { type Dayjs } from 'dayjs'
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
  PlayCircleOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import CdfChart from '@/components/CdfChart.vue'
import ScatterChart from '@/components/ScatterChart.vue'
import BarChart from '@/components/BarChart.vue'
import PieChart from '@/components/PieChart.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { reportReviewPrompt } from '@/utils/aiScenarios'
import { formatDateTime } from '@/utils/datetime'
import * as api from '@/api'
import type {
  InterfaceSamplePoint,
  PoolReportStat,
  ReportDetail,
  ReportRequestRecord,
  ReportRequestsPage,
  UrlStat,
} from '@/types/models'
import type { ClusterReportMeta, ClusterReportNode, SeriesPoint } from '@/types/cluster'
import {
  buildReportHtml,
  buildReportSheets,
  loadGeneratorsForReport,
  REPORT_DISCLAIMER,
} from '@/utils/reportExport'
import { buildClassicReportHtml } from '@/utils/reportClassic'
import { detectKnee, qpsOverview, reportPeaks, scoreReport, selfCheck } from '@/utils/reportAnalytics'
import { buildFindings, cdfPoints, gradeReport, latencyDrift, type Finding } from '@/utils/reportNarrative'
import { copyToClipboard } from '@/utils/clipboard'

const route = useRoute()
const router = useRouter()

/**
 * 报告详情是整块替换的只读数据（长压测可达数万条采样），
 * 用 shallowRef 避免为每个采样点创建深层响应式代理。
 */
const detail = shallowRef<ReportDetail | null>(null)
const loading = ref(true)
const exporting = ref(false)
/** 导出是否包含参数 / Token 池统计（只含计数，不含明文） */
const includePoolStats = ref(true)

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
    await copyToClipboard(text)
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
/** 每秒明细分页：避免 8 小时压测把数万行一次性塞进 DOM */
const samplePage = ref(1)
const samplePageSize = ref(200)
const samplePageRows = computed(() => {
  const start = (samplePage.value - 1) * samplePageSize.value
  return samples.value.slice(start, start + samplePageSize.value)
})
watch(
  () => detail.value?.report.id,
  () => {
    samplePage.value = 1
  },
)
watch(samples, (list) => {
  const maxPage = Math.max(1, Math.ceil(list.length / samplePageSize.value))
  samplePage.value = Math.min(samplePage.value, maxPage)
})
const errors = computed(() => detail.value?.errors ?? [])
const statusCodes = computed(() => detail.value?.statusCodes ?? [])
const errorTypes = computed(() => detail.value?.errorTypes ?? [])
const histogram = computed(() => detail.value?.histogram ?? [])
const bodyHistogram = computed(() => detail.value?.bodyHistogram ?? [])
const slowRequests = computed(() => detail.value?.slowRequests ?? [])
/** 按 URL 聚合的统计：与是否记录请求明细无关 */
const urlStats = computed(() => detail.value?.urlStats ?? [])
const urlStatsComplete = computed(
  () => detail.value?.urlStatsComplete ?? urlStats.value.length > 0,
)
/** 错误地址：只展示存在失败请求的 URL */
const errorUrls = computed(() => urlStats.value.filter((item) => item.errorCount > 0))

/** 接口匹配键：忽略查询串差异（URL 统计会脱敏敏感参数） */
function requestUrlKey(method: string, url: string): string {
  let path = url
  try {
    const parsed = new URL(url)
    path = `${parsed.origin}${parsed.pathname}`
  } catch {
    path = url.split('?')[0]
  }
  return `${(method || '').toUpperCase()} ${path}`
}

/** 多接口报告：把 URL 统计按接口名称归并，回答「每个接口各跑得怎么样」 */
const requestSummaries = computed(() => {
  const targets = config.value?.requests ?? []
  if (targets.length < 2) return []
  return targets.map((target) => {
    const key = requestUrlKey(target.method ?? '', target.url ?? '')
    const rows = urlStats.value.filter(
      (row) => requestUrlKey(row.method, row.url) === key,
    )
    const requests = rows.reduce((sum, row) => sum + row.requests, 0)
    const success = rows.reduce((sum, row) => sum + row.successCount, 0)
    const errors = rows.reduce((sum, row) => sum + row.errorCount, 0)
    const sumMs = rows.reduce((sum, row) => sum + row.avgMs * row.requests, 0)
    const maxMs = rows.reduce((sum, row) => Math.max(sum, row.maxMs), 0)
    return {
      name: target.name || target.url || '未命名接口',
      method: target.method ?? '',
      url: target.url ?? target.urlTemplate ?? '',
      weight: target.weight ?? 0,
      requests,
      success,
      errors,
      successRate: requests > 0 ? (success / requests) * 100 : 0,
      avgMs: requests > 0 ? sumMs / requests : 0,
      maxMs,
    }
  })
})

function urlSuccessRate(row: UrlStat) {
  return row.requests > 0 ? (row.successCount / row.requests) * 100 : 0
}

function urlMaxAt(row: UrlStat) {
  return row.maxAtMs == null || row.maxAtMs < 0 ? '—' : fmtDuration(row.maxAtMs)
}
/** 导出体积预估（1.4.3）：明细按 1KB 请求体 + 2KB 响应体上限估算 */
const exportEstimate = computed(() => {
  const data = detail.value
  if (!data) return '-'
  const mode = data.config?.recordDetail
  const records = mode === 'off' ? 0 : Math.min(data.report.totalRequests, 50_000)
  const bytes =
    records * 3_200 +
    (data.samples?.length ?? 0) * 160 +
    (data.slowRequests?.length ?? 0) * 800 +
    64 * 1024
  if (bytes >= 1024 * 1024) return `≈ ${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `≈ ${Math.max(1, Math.round(bytes / 1024))} KB`
})
const stages = computed(() => detail.value?.stages ?? [])
const config = computed(() => detail.value?.config ?? null)
const metrics = computed(() => config.value?.metrics ?? null)
const bytesSent = computed(() => metrics.value?.bytesSent ?? 0)

/* ---------- 分布式压测元数据（v1.1.1 一期） ---------- */
const cluster = computed<ClusterReportMeta | null>(() => config.value?.cluster ?? null)
const clusterNodes = computed(() => cluster.value?.nodes ?? [])
const clusterPeaks = computed(() => cluster.value?.peaks ?? null)
/** 施压机明细：单机 1 行、集群 N 行；旧报告缺失字段显示「—」 */
const loadGenerators = computed(() => (detail.value ? loadGeneratorsForReport(detail.value) : []))
/** 单机报告峰值专题（集群报告由 cluster.peaks 展示，不重复） */
const singlePeaks = computed(() => (cluster.value ? null : detail.value ? reportPeaks(detail.value) : null))
/** 集群总 QPS：各节点平均 QPS 相加，与合并口径一致 */
const clusterTotalQps = computed(() =>
  clusterNodes.value.reduce((sum, node) => sum + (node.qpsAvg ?? 0), 0),
)

const CLUSTER_SERIES_COLORS = [
  'var(--fm-chart-1)',
  'var(--fm-chart-2)',
  'var(--fm-chart-3)',
  'var(--fm-chart-4)',
  'var(--fm-chart-5)',
  'var(--fm-chart-6)',
]

/* 节点曲线可切换的口径：吞吐看 QPS，定位瓶颈看并发/延迟，看机器瓶颈看资源 */
type ClusterMetricKey = 'qps' | 'concurrent' | 'p95' | 'hostCpu' | 'mem' | 'net'

const CLUSTER_METRICS: {
  key: ClusterMetricKey
  label: string
  unit: string
  hint: string
  pick: (point: SeriesPoint) => number
}[] = [
  { key: 'qps', label: 'QPS', unit: 'QPS', hint: '各节点每秒完成的请求数', pick: (point) => point.qps },
  {
    key: 'concurrent',
    label: '并发',
    unit: '并发',
    hint: '各节点上报的并发数，用来看分配是否倾斜',
    pick: (point) => point.concurrent,
  },
  { key: 'p95', label: 'P95', unit: 'ms', hint: '节点级 P95 延迟', pick: (point) => point.p95 },
  {
    key: 'hostCpu',
    label: '整机 CPU',
    unit: '%',
    hint: '从节点整机 CPU 使用率，判断机器是否被其他进程拖慢',
    pick: (point) => point.hostCpuPercent ?? 0,
  },
  {
    key: 'mem',
    label: '进程内存',
    unit: 'MB',
    hint: '施压进程 RSS，持续增长说明有泄漏',
    pick: (point) => point.memMb ?? 0,
  },
  {
    key: 'net',
    label: '网络',
    unit: 'KB/s',
    hint: '网卡收发合计，用来判断是否先被带宽卡住',
    pick: (point) => (point.netRxKbps ?? 0) + (point.netTxKbps ?? 0),
  },
]

const clusterMetric = ref<ClusterMetricKey>('qps')
const clusterMetricInfo = computed(
  () => CLUSTER_METRICS.find((item) => item.key === clusterMetric.value) ?? CLUSTER_METRICS[0],
)
/** 老报告没有 host 字段，资源类口径直接禁用，避免画出一条全是 0 的线 */
const clusterHasHost = computed(() =>
  (cluster.value?.nodeSeries ?? []).some((item) =>
    item.points?.some((point) => (point.hostCpuPercent ?? 0) > 0 || (point.memMb ?? 0) > 0),
  ),
)

function clusterMetricDisabled(key: ClusterMetricKey) {
  return !clusterHasHost.value && (key === 'hostCpu' || key === 'mem' || key === 'net')
}

/** 节点曲线：各节点采样长度可能不同，按最右侧对齐，左侧补 0 */
const clusterNodeSeries = computed(() => {
  const pick = clusterMetricInfo.value.pick
  const list = (cluster.value?.nodeSeries ?? []).filter((item) => item.points?.length)
  const maxPoints = Math.max(0, ...list.map((item) => item.points.length))
  return list.map((item, index) => {
    const values = item.points.slice(-maxPoints).map(pick)
    while (values.length < maxPoints) values.unshift(0)
    return {
      name: item.node || `节点 ${index + 1}`,
      color: CLUSTER_SERIES_COLORS[index % CLUSTER_SERIES_COLORS.length],
      values,
    }
  })
})

const CLUSTER_NODE_STATUS: Record<string, string> = {
  pairing: '配对中',
  pairReady: '已配对',
  preparing: '下发中',
  preflight: '预检中',
  ready: '就绪',
  running: '施压中',
  done: '已完成',
  lost: '失联',
  left: '已退出',
  rejected: '被拒绝',
}

function clusterStatusText(status: string) {
  return CLUSTER_NODE_STATUS[status] ?? status
}

function clusterStatusTone(status: string) {
  if (status === 'done') return 'ok'
  if (status === 'lost' || status === 'rejected') return 'bad'
  if (status === 'left') return 'warn'
  return ''
}

function clusterRoleText(role: string) {
  return role === 'master' ? '主节点' : '从节点'
}

/** 节点资源峰值：进程口径 + 整机口径 + 网卡，老报告没采集的自动省略 */
function clusterResourceText(node: ClusterReportNode): string {
  const parts: string[] = []
  if (node.cpuPeak) parts.push(`${node.cpuPeak.toFixed(2)} 核`)
  if (node.memPeakMb) parts.push(fmtBytes(node.memPeakMb * 1024 * 1024))
  if (node.hostCpuPeak) parts.push(`整机 ${node.hostCpuPeak.toFixed(0)}%`)
  if (node.netPeakKbps) parts.push(`↓↑ ${(node.netPeakKbps / 1024).toFixed(1)} MB/s`)
  return parts.length ? parts.join(' · ') : '未采集'
}

const CLUSTER_HEALTH: Record<string, string> = {
  ok: '正常',
  waiting: '等待首样',
  gap: '采样缺口',
  lost: '无数据',
  done: '正常结束',
}

function clusterHealthText(node: ClusterReportNode) {
  const state = node.health?.state ?? ''
  const label = CLUSTER_HEALTH[state] ?? state ?? '—'
  return node.health?.message ? `${label}（${node.health.message}）` : label
}

function clusterHealthTone(node: ClusterReportNode) {
  const state = node.health?.state
  if (state === 'lost') return 'bad'
  if (state === 'gap' || state === 'waiting') return 'warn'
  if (state === 'ok' || state === 'done') return 'ok'
  return ''
}

/** QPS 指标组：平均 / 峰值 / 90% 分布 / 单并发 / 目标达成率，页面与导出同一口径 */
const qps = computed(() =>
  detail.value
    ? qpsOverview(detail.value, { rampUpSec: config.value?.rampUpSec ?? 0 })
    : null,
)
const knee = computed(() => (detail.value ? detectKnee(detail.value) : null))

/** 阶段汇总：叠加稳态 QPS 与扩展效率（口径与 HTML 导出一致） */
const stageRows = computed(() => {
  const steadyStages = knee.value?.stages ?? []
  return stages.value.map((stage, index) => {
    const steady = steadyStages[index]
    const prev = stages.value[index - 1]
    const concurGain =
      prev && prev.concurrent > 0 ? (stage.concurrent - prev.concurrent) / prev.concurrent : null
    return {
      ...stage,
      steadyQps: steady?.qps ?? null,
      gain: steady?.gain ?? null,
      efficiency:
        steady && concurGain != null && concurGain > 0 ? steady.gain / concurGain : null,
    }
  })
})
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
  const count = cfg?.requestCount ?? cfg?.requests?.length ?? 0
  // 多接口报告不展示首个地址，直接标注「全部接口」，明细见「按接口汇总」
  if (count > 1) {
    return {
      url: '',
      source: 'multi' as const,
      note: `全部接口（共 ${count} 个）`,
    }
  }
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

const titleDraft = ref('')
const scenarioDraft = ref('')
const summaryDraft = ref('')
const textSaving = ref(false)

watch(
  detail,
  (value) => {
    titleDraft.value = value?.report.title ?? ''
    scenarioDraft.value = value?.scenarioText ?? ''
    summaryDraft.value = value?.summaryText ?? ''
  },
  { immediate: true },
)

async function saveTexts() {
  if (!report.value) return
  textSaving.value = true
  try {
    await api.reportUpdateTexts(
      report.value.id,
      titleDraft.value,
      scenarioDraft.value,
      summaryDraft.value,
    )
    if (detail.value) {
      detail.value.report.title = titleDraft.value.trim() || null
      detail.value.scenarioText = scenarioDraft.value
      detail.value.summaryText = summaryDraft.value
      // shallowRef 不会追踪嵌套字段变更，这里手动触发一次视图更新
      triggerRef(detail)
    }
    message.success('已保存标题、场景与总结')
  } catch (e) {
    message.error(String(e))
  } finally {
    textSaving.value = false
  }
}

function rerunReport() {
  const current = detail.value
  if (!current) return
  const query: Record<string, string> = { reportId: String(current.report.id) }
  if (current.report.planId) {
    // 计划最完整：直接按计划重新加载；计划被删时压测页会回退到报告快照
    query.planId = String(current.report.planId)
  } else if (current.report.requestId) {
    // 无计划时按报告快照回填；带 requestId 时可复用请求定义
    query.requestId = String(current.report.requestId)
  }
  void router.push({ name: 'stress', query })
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

/** 趋势图时间范围：只作用于基于每秒采样的折线/散点图，构成图仍按全报告口径 */
const chartRange = ref<[number, number]>([0, 0])
const sampleMin = computed(() => samples.value[0]?.elapsedSec ?? 0)
const sampleMax = computed(() => samples.value[samples.value.length - 1]?.elapsedSec ?? 0)
watch(
  samples,
  (list) => {
    chartRange.value = list.length
      ? [list[0].elapsedSec, list[list.length - 1].elapsedSec]
      : [0, 0]
  },
  { immediate: true },
)
const chartSamples = computed(() => {
  const list = samples.value
  if (!list.length) return []
  const [first, second] = chartRange.value
  const start = Math.min(first, second)
  const end = Math.max(first, second)
  return list.filter((item) => item.elapsedSec >= start && item.elapsedSec <= end)
})
function resetChartRange() {
  chartRange.value = [sampleMin.value, sampleMax.value]
}
function onChartRangeChange(value: number | [number, number]) {
  if (Array.isArray(value)) chartRange.value = [value[0], value[1]]
}

/** 采样秒数换算成报告开始后的完整时钟时间；缺失开始时间时回退到秒数 */
function elapsedClock(sec: number) {
  const start = report.value?.startTime
  if (!start) return `${sec}s`
  const date = new Date(start + sec * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/** 时间选择器与滑块共用 chartRange（elapsedSec），保证两种入口口径一致 */
const rangePickerValue = computed<[Dayjs, Dayjs] | null>(() => {
  const start = report.value?.startTime
  if (!start || samples.value.length < 2) return null
  return [dayjs(start + chartRange.value[0] * 1000), dayjs(start + chartRange.value[1] * 1000)]
})

function onRangePickerChange(value: unknown) {
  const start = report.value?.startTime
  if (!start || !Array.isArray(value) || value.length !== 2) return
  const [first, second] = value as Array<Dayjs | null>
  if (!first || !second) return
  const clamp = (ms: number) =>
    Math.min(sampleMax.value, Math.max(sampleMin.value, Math.round((ms - start) / 1000)))
  const begin = clamp(first.valueOf())
  const end = clamp(second.valueOf())
  chartRange.value = [Math.min(begin, end), Math.max(begin, end)]
}

const rangeText = computed(
  () =>
    `${elapsedClock(chartRange.value[0])} – ${elapsedClock(chartRange.value[1])} · ` +
    `${chartSamples.value.length} / ${samples.value.length} 个采样点`,
)

const chartLabels = computed(() => chartSamples.value.map((s) => elapsedClock(s.elapsedSec)))

const qpsSeries = computed(() => [
  { name: 'QPS', color: 'var(--fm-primary)', values: chartSamples.value.map((s) => s.qps) },
])

/**
 * 多接口报告的接口切换：index 对应配置快照 requests 的下标。
 * 旧报告没有接口曲线时返回空，界面不出现切换器。
 */
const interfaceTargets = computed(() => {
  const targets = config.value?.requests ?? []
  const points = detail.value?.interfaceSamples ?? []
  if (targets.length < 2 || !points.length) return []
  return targets.map((target, index) => ({
    index,
    label: target.name || target.url || `接口 ${index + 1}`,
  }))
})
/** -1 表示全部接口汇总（等同全局 QPS） */
const selectedInterface = ref(-1)
const interfaceQpsSeries = computed(() => {
  if (!interfaceTargets.value.length || selectedInterface.value < 0) return qpsSeries.value
  const bySec = new Map<number, InterfaceSamplePoint>()
  for (const point of detail.value?.interfaceSamples ?? []) {
    if (point.index === selectedInterface.value) bySec.set(point.elapsedSec, point)
  }
  const label =
    interfaceTargets.value.find((item) => item.index === selectedInterface.value)?.label ?? '接口 QPS'
  return [
    {
      name: label,
      color: 'var(--fm-primary)',
      values: chartSamples.value.map((sample) => bySec.get(sample.elapsedSec)?.qps ?? 0),
    },
  ]
})

/** 并发请求数单独成图，避免与 QPS 混在同一纵轴 */
const concurrencySeries = computed(() => [
  {
    name: '并发请求数',
    color: 'var(--fm-muted)',
    values: chartSamples.value.map((s) => s.concurrent),
  },
])

/** 平均响应时间单独成图，与分位图分开看趋势 */
const avgRtSeries = computed(() => [
  {
    name: '平均响应时间',
    color: 'var(--fm-primary)',
    values: chartSamples.value.map((s) => s.avgRt),
  },
])

const latencySeries = computed(() => [
  { name: '平均', color: 'var(--fm-primary)', values: chartSamples.value.map((s) => s.avgRt) },
  { name: 'P50', color: 'var(--fm-json-key)', values: chartSamples.value.map((s) => s.p50Rt) },
  { name: 'P95', color: 'var(--fm-warning)', values: chartSamples.value.map((s) => s.p95Rt) },
  { name: 'P99', color: 'var(--fm-danger)', values: chartSamples.value.map((s) => s.p99Rt) },
])

const errorSeries = computed(() => [
  {
    name: '错误率',
    color: 'var(--fm-danger)',
    values: chartSamples.value.map((s) => {
      const total = s.successCount + s.errorCount
      return total > 0 ? (s.errorCount / total) * 100 : 0
    }),
  },
])

/** 响应时间累积分布：直接复用导出口径，忽略零计数与开放区间上限 */
const cdfSeries = computed(() => (detail.value ? cdfPoints(detail.value) : []))
/** QPS 与平均响应时间散点：观察吞吐上升时延迟是否恶化 */
const qpsLatencyPoints = computed(() =>
  chartSamples.value
    .filter((item) => item.qps > 0 && Number.isFinite(item.avgRt))
    .map((item) => ({ x: item.qps, y: item.avgRt })),
)
/** 接收带宽趋势（KB/s） */
const bandwidthSeries = computed(() => [
  {
    name: '接收带宽',
    color: 'var(--fm-primary)',
    values: chartSamples.value.map((item) => item.bytes / 1024),
  },
])
/** TTFB 与响应体读取耗时 */
const ttfbBodySeries = computed(() => {
  const series = [
    {
      name: 'TTFB',
      color: 'var(--fm-chart-violet)',
      values: chartSamples.value.map((item) => item.ttfbAvg),
    },
  ]
  if (chartSamples.value.some((item) => item.bodyAvg > 0)) {
    series.push({
      name: 'Body 平均',
      color: 'var(--fm-chart-6)',
      values: chartSamples.value.map((item) => item.bodyAvg),
    })
  }
  return series
})
/** URL 平均响应对比：单 URL 时没有对比意义 */
const urlCompareItems = computed(() =>
  urlStats.value.length > 1
    ? urlStats.value.slice(0, 12).map((row) => ({
        label: `${row.method} ${row.url}`,
        value: row.avgMs,
        sub: `${fmtNum(row.requests, 0)} 次`,
      }))
    : [],
)
/** 施压机 TPS 对比：单机时省略 */
const generatorCompareItems = computed(() =>
  loadGenerators.value.length > 1
    ? loadGenerators.value
        .filter((node) => node.tps != null)
        .map((node) => ({
          label: node.name,
          value: node.tps ?? 0,
          sub: `${fmtNum(node.concurrent, 0)} 并发`,
        }))
    : [],
)

/** 成功 / 失败 QPS：每秒成功与失败请求数 */
const successQpsSeries = computed(() => [
  {
    name: '成功 QPS',
    color: 'var(--fm-success)',
    values: chartSamples.value.map((item) => item.successCount),
  },
])
const failureQpsSeries = computed(() => [
  {
    name: '失败 QPS',
    color: 'var(--fm-danger)',
    values: chartSamples.value.map((item) => item.errorCount),
  },
])
const successFailureSeries = computed(() => [
  {
    name: '成功 QPS',
    color: 'var(--fm-success)',
    values: chartSamples.value.map((item) => item.successCount),
  },
  {
    name: '失败 QPS',
    color: 'var(--fm-danger)',
    values: chartSamples.value.map((item) => item.errorCount),
  },
])
/** 请求结果构成 */
const resultPieItems = computed(() =>
  report.value
    ? [
        { label: '成功', value: report.value.successCount, color: 'var(--fm-success)' },
        { label: '失败', value: report.value.errorCount, color: 'var(--fm-danger)' },
      ]
    : [],
)
/** 状态码构成：按数量取前 8 个 */
const statusPieItems = computed(() =>
  [...statusCodes.value]
    .sort((a, b) => b.count - a.count)
    .slice(0, 8)
    .map((item) => ({
      label: `状态码 ${item.code}`,
      value: item.count,
      color:
        item.code >= 500
          ? 'var(--fm-danger)'
          : item.code >= 400
            ? 'var(--fm-warning)'
            : item.code >= 300
              ? 'var(--fm-chart-2)'
              : 'var(--fm-chart-1)',
    })),
)
/** 错误类型构成：按数量取前 8 个 */
const errorTypePieItems = computed(() =>
  [...errorTypes.value]
    .sort((a, b) => b.count - a.count)
    .slice(0, 8)
    .map((item) => ({ label: item.errorType, value: item.count })),
)
/** 流量构成：接收 / 发送 */
const trafficPieItems = computed(() =>
  report.value
    ? [
        { label: '接收', value: report.value.bytesTotal, color: 'var(--fm-chart-2)' },
        { label: '发送', value: bytesSent.value, color: 'var(--fm-chart-3)' },
      ]
    : [],
)
const stageQpsItems = computed(() =>
  stages.value.map((item, index) => ({
    label: `阶段 ${index + 1}`,
    value: item.avgQps,
    sub: `${fmtNum(item.concurrent, 0)} 并发`,
  })),
)
const stageP95Items = computed(() =>
  stages.value.map((item, index) => ({
    label: `阶段 ${index + 1}`,
    value: item.p95Rt,
    sub: `${fmtNum(item.concurrent, 0)} 并发`,
  })),
)

const durationSec = computed(() => {
  if (!report.value?.startTime || !report.value?.endTime) return 0
  return Math.max(0, Math.round((report.value.endTime - report.value.startTime) / 1000))
})

/** 420 秒这类原始秒数对人不友好，概览按小时/分/秒展示 */
const durationLabel = computed(() => {
  const total = durationSec.value
  if (total <= 0) return '—'
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  if (hours > 0) return `${hours} 小时 ${String(minutes).padStart(2, '0')} 分`
  if (minutes > 0) return `${minutes} 分 ${String(seconds).padStart(2, '0')} 秒`
  return `${seconds} 秒`
})

const successRate = computed(() => 100 - (report.value?.errorRate ?? 0))
/** 成功率配色：≥99% 正常、≥95% 关注、其余风险 */
const successTone = computed(() =>
  successRate.value >= 99 ? 'ok' : successRate.value >= 95 ? 'warn' : 'bad',
)

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
    desc: `APDEX ${r.apdex.toFixed(3)} · 错误率 ${r.errorRate.toFixed(2)}%`,
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

/** 旧报告没有 body_avg / body_p95 / body_histogram，不能按 0 展示 */
const bodyMetricsAvailable = computed(
  () =>
    bodyHistogram.value.length > 0 ||
    samples.value.some((sample) => sample.bodyAvg != null || sample.bodyP95 != null),
)

/** 按请求数加权的 Body 平均（逐秒采样点加权，旧报告无该字段时返回 null） */
const avgBody = computed<number | null>(() => {
  if (!bodyMetricsAvailable.value) return null
  let sum = 0
  let weight = 0
  for (const sample of samples.value) {
    // Body 只对拿到响应的请求有值，旧报告没有 responseCount 时退回成功+失败数
    const count =
      sample.responseCount && sample.responseCount > 0
        ? sample.responseCount
        : sample.successCount + sample.errorCount
    sum += (sample.bodyAvg || 0) * count
    weight += count
  }
  return weight ? sum / weight : null
})

/** Body P95 从报告层直方图算；分位数不能由两个分位数相减 */
const p95Body = computed<number | null>(() => {
  if (!bodyMetricsAvailable.value) return null
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

/** 压测模式文案：集群报告 mode 为 cluster，用阶梯阶段数还原成「分布式压测（阶梯/固定）」 */
const modeLabel = computed(() => {
  const cfg = config.value
  const stages = cfg?.stages?.length ?? 0
  if (cfg?.mode === 'cluster') return `分布式压测（${stages > 0 ? '阶梯' : '固定'}）`
  return cfg?.mode === 'stages' ? '阶梯加压' : '固定并发'
})

function fmtNum(v: number | undefined | null, digits = 1) {
  if (v === undefined || v === null || Number.isNaN(v)) return '-'
  // 千位分隔，避免 3870 / 3988 这类吞吐数字难以扫读
  if (Math.abs(v) >= 1000) return v.toLocaleString('zh-CN', { maximumFractionDigits: 0 })
  return v.toFixed(digits)
}

function poolScopeLabel(pool: PoolReportStat) {
  if (pool.nodeScope === 'cluster') {
    return pool.nodes?.length ? `全部节点（${pool.nodes.length}）` : '全部节点'
  }
  return pool.nodeScope === 'master' ? '仅主节点' : '本机'
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

type ExportFormat = 'json' | 'html' | 'classic-html' | 'excel' | 'csv-requests' | 'csv-errors'

const EXPORT_META: Record<ExportFormat, { extension: string; filterName: string; label: string }> = {
  json: { extension: 'json', filterName: 'AstroMan 压测报告', label: 'JSON 归档' },
  html: { extension: 'html', filterName: 'HTML 报告', label: 'HTML 报告' },
  'classic-html': { extension: 'html', filterName: 'HTML 报告', label: '经典报告' },
  excel: { extension: 'xlsx', filterName: 'Excel 工作簿', label: 'Excel 表格' },
  'csv-requests': { extension: 'csv', filterName: 'CSV 明细', label: '请求明细 CSV' },
  'csv-errors': { extension: 'csv', filterName: 'CSV 明细', label: '错误明细 CSV' },
}

async function exportReport(format: ExportFormat) {
  const current = report.value
  const data = detail.value
  if (!current || !data) return
  const meta = EXPORT_META[format]
  const safeName = (current.title || current.planName).replace(/[\\/:*?"<>|]/g, '_')
  exporting.value = true
  try {
    const path = await save({
      title: `导出压测报告（${meta.label}）`,
      defaultPath: `${safeName}${format === 'classic-html' ? '-经典报告' : '-report'}.${meta.extension}`,
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
      // HTML 报告只用到首屏明细兜底目标地址与 Top10 慢请求；
      // Excel 的请求明细改由后端直接查库写入，前端同样只需要一条地址兜底。
      const htmlMode = format === 'html' || format === 'classic-html'
      const recordLimit = htmlMode
        ? (data.slowRequests?.length ?? 0) > 0
          ? 200
          : 2000
        : 1
      const records = await api.reportRequests(current.id, 1, recordLimit, null)
      // 基线报告单独拉取，导出时直接写进报告，避免阅读者再去翻另一份
      let baselineDetail: ReportDetail | null = null
      const baselineId = data.config?.baselineReportId
      if (baselineId && baselineId !== current.id) {
        try {
          baselineDetail = await api.reportGet(baselineId)
        } catch {
          baselineDetail = null
          message.warning('基线报告不存在或已删除，本次导出不含基线对比')
        }
      }
      if (format === 'html') {
        const bytes = new TextEncoder().encode(
          buildReportHtml(data, records.items, {
            includePools: includePoolStats.value,
            baseline: baselineDetail,
          }),
        )
        await api.reportWriteFile(target, Array.from(bytes))
      } else if (format === 'classic-html') {
        const bytes = new TextEncoder().encode(buildClassicReportHtml(data, records.items))
        await api.reportWriteFile(target, Array.from(bytes))
      } else {
        const sheets = buildReportSheets(data, records.items, {
          includePools: includePoolStats.value,
          baseline: baselineDetail,
          includeRequestRows: false,
          includeSampleRows: false,
        })
        // undefined 在 IPC 序列化前统一成 null，避免后端按缺失字段处理
        const payload = sheets.map((sheet) => ({
          name: sheet.name,
          rows: sheet.rows.map((row) => row.map((cell) => (cell === undefined ? null : cell))),
        }))
        await api.reportExportXlsx(current.id, target, payload)
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
      <a-button :disabled="!detail" @click="rerunReport">
        <template #icon><PlayCircleOutlined /></template>
        再次运行
      </a-button>
      <a-checkbox
        v-if="detail?.config?.pools?.length"
        v-model:checked="includePoolStats"
        class="pool-export-toggle"
      >
        包含池统计
      </a-checkbox>
      <a-dropdown :trigger="['click']" placement="bottomRight">
        <a-button :loading="exporting">
          <template #icon><DownloadOutlined /></template>
          导出报告
          <DownOutlined />
        </a-button>
        <template #overlay>
          <a-menu @click="onExportMenu">
            <a-menu-item key="estimate" disabled>预估导出体积 {{ exportEstimate }}</a-menu-item>
            <a-menu-divider />
            <a-menu-item key="json">JSON 归档（完整数据）</a-menu-item>
            <a-menu-item key="html">HTML 报告（可视化）</a-menu-item>
            <a-menu-item key="classic-html">经典报告</a-menu-item>
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
              <h2 class="report-name">{{ report.title?.trim() || report.planName }}</h2>
              <span class="status-pill" :class="{ stopped: report.status === 'stopped' }">
                {{ report.status === 'stopped' ? '已停止' : '已完成' }}
              </span>
              <span class="mode-pill">{{ modeLabel }}</span>
            </div>
            <div class="meta-row">
              <span>开始于 {{ fmtTime(report.startTime) }}</span>
              <span class="dot-sep">·</span>
              <span>耗时 {{ durationLabel }}</span>
              <span class="dot-sep">·</span>
              <span>峰值并发 {{ fmtNum(report.concurrency, 0) }}</span>
              <span v-if="qps?.target" class="dot-sep">·</span>
              <span v-if="qps?.target">目标 {{ fmtNum(qps.target, 0) }} QPS</span>
              <span v-if="qps?.attainment != null" class="dot-sep">·</span>
              <span v-if="qps?.attainment != null">达成 {{ fmtNum(qps.attainment, 0) }}%</span>
            </div>
            <div v-if="targetUrlInfo.url || targetUrlInfo.note" class="target-line">
              <span v-if="targetUrlInfo.source !== 'multi'" class="method-chip">{{
                config?.method || 'GET'
              }}</span>
              <span
                v-if="targetUrlInfo.url"
                class="target-url mono"
                :title="`${targetUrlInfo.url}（点击复制）`"
                @click="copyText(targetUrlInfo.url, '地址')"
                >{{ targetUrlInfo.url }}</span
              >
              <span
                v-if="targetUrlInfo.url"
                class="target-copy"
                title="复制地址"
                @click="copyText(targetUrlInfo.url, '地址')"
              >
                <CopyOutlined />
              </span>
              <span v-else class="target-url multi-target">{{ targetUrlInfo.note }}</span>
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
              <div class="card-sub">按风险从高到低排列，P0/P1 标签与导出报告一致</div>
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
          <div class="metric-card fm-card tone-accent">
            <span class="m-label">总请求</span>
            <span class="m-value mono accent">{{ report.totalRequests.toLocaleString() }}</span>
          </div>
          <div class="metric-card fm-card" :class="`tone-${successTone}`">
            <span class="m-label">成功率</span>
            <span class="m-value mono" :class="successTone">{{ successRate.toFixed(2) }}<i>%</i></span>
          </div>
          <div class="metric-card fm-card tone-accent">
            <span class="m-label">平均 QPS</span>
            <span class="m-value mono accent">{{ fmtNum(qps?.avg) }}</span>
          </div>
          <div class="metric-card fm-card tone-info">
            <span class="m-label">峰值 QPS</span>
            <span class="m-value mono info">{{ fmtNum(qps?.max) }}</span>
          </div>
          <div class="metric-card fm-card tone-ok" title="每秒采样 QPS 的 P10：90% 的时间吞吐不低于该值，已剔除升压段与首末不完整秒">
            <span class="m-label">90% 分布 QPS</span>
            <span class="m-value mono ok">{{ fmtNum(qps?.ninety) }}</span>
          </div>
          <div class="metric-card fm-card tone-violet" title="平均 QPS ÷ 并发数，用于对比不同并发档位下单连接的吞吐效率">
            <span class="m-label">单并发 QPS</span>
            <span class="m-value mono violet">{{ fmtNum(qps?.perConcurrent, 2) }}</span>
          </div>
          <div class="metric-card fm-card tone-info">
            <span class="m-label">平均延迟</span>
            <span class="m-value mono info">{{ fmtNum(report.avgRt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card tone-warn">
            <span class="m-label">P95 延迟</span>
            <span class="m-value mono warn">{{ fmtNum(report.p95Rt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card tone-bad">
            <span class="m-label">P99 延迟</span>
            <span class="m-value mono bad">{{ fmtNum(report.p99Rt) }}<i>ms</i></span>
          </div>
          <div class="metric-card fm-card" :class="`tone-${apdexGrade.tone}`">
            <span class="m-label">APDEX</span>
            <span class="m-value mono" :class="apdexGrade.tone">{{ report.apdex.toFixed(3) }}
              <i class="m-tag" :class="apdexGrade.tone">{{ apdexGrade.label }}</i>
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
              <span>并发 {{ fmtNum(baseline.report.concurrency, 0) }}</span>
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
              <div class="card-title">报告标题、测试场景与总结</div>
              <div class="card-sub">可编辑，保存后随报告归档，并同步进入 HTML / Excel 报告</div>
            </div>
            <a-button size="small" :loading="textSaving" @click="saveTexts">保存</a-button>
          </div>
          <div class="text-field">
            <span class="text-label">报告标题</span>
            <a-input
              v-model:value="titleDraft"
              :maxlength="160"
              allow-clear
              placeholder="留空时使用压测计划名称"
            />
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
                <span class="p-value mono">{{ avgBody == null ? '—' : `${fmtNum(avgBody)} ms` }}</span>
              </div>
              <div class="extra-item">
                <span class="p-label">Body P95</span>
                <span class="p-value mono">{{ p95Body == null ? '—' : `${fmtNum(p95Body)} ms` }}</span>
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
                <span class="code-count mono">{{ item.count.toLocaleString() }}</span>
                <span class="code-ratio mono">
                  {{ report.errorCount > 0 ? ((item.count / report.errorCount) * 100).toFixed(1) : '0.0' }}%
                </span>
              </div>
            </div>
          </div>
        </div>

        <div class="section-head">
          <div>
            <h2 class="section-title">统计分析</h2>
            <div class="section-desc">
              成功 / 失败 QPS、请求结果、状态码、错误类型、流量构成与阶段对比；时间范围只作用于趋势图
            </div>
          </div>
        </div>

        <div v-if="samples.length > 1" class="chart-range">
          <span class="range-title">趋势图时间范围</span>
          <a-range-picker
            v-if="rangePickerValue"
            class="range-picker"
            show-time
            :value="rangePickerValue"
            :allow-clear="false"
            format="YYYY-MM-DD HH:mm:ss"
            @change="onRangePickerChange"
          />
          <a-slider
            class="range-slider"
            range
            :min="sampleMin"
            :max="Math.max(sampleMin + 1, sampleMax)"
            :value="chartRange"
            @change="onChartRangeChange"
          />
          <span class="range-text mono">{{ rangeText }}</span>
          <a-button size="small" @click="resetChartRange">全部</a-button>
        </div>

        <template v-if="samples.length > 1">
          <div class="period-head">
            <span class="period-title">分时段趋势</span>
            <span class="period-sub">
              按上方所选时间段展示成功 QPS、失败 QPS、并发请求数与平均响应时间
            </span>
          </div>
          <div class="charts period-charts">
            <div class="fm-card chart-card">
              <div class="chart-head">
                <span class="chart-title">成功 QPS</span>
                <span class="chart-sub">每秒成功请求数</span>
              </div>
              <LineChart
                :series="successQpsSeries"
                :labels="chartLabels"
                :height="200"
                :area="false"
              />
            </div>
            <div class="fm-card chart-card">
              <div class="chart-head">
                <span class="chart-title">失败 QPS</span>
                <span class="chart-sub">每秒失败请求数</span>
              </div>
              <LineChart
                :series="failureQpsSeries"
                :labels="chartLabels"
                :height="200"
                :area="false"
              />
            </div>
            <div class="fm-card chart-card">
              <div class="chart-head">
                <span class="chart-title">并发请求数</span>
                <span class="chart-sub">所选时间段内的并发趋势</span>
              </div>
              <LineChart
                :series="concurrencySeries"
                :labels="chartLabels"
                :height="200"
                :area="false"
              />
            </div>
            <div class="fm-card chart-card">
              <div class="chart-head">
                <span class="chart-title">平均响应时间</span>
                <span class="chart-sub">单位 ms</span>
              </div>
              <LineChart
                :series="avgRtSeries"
                :labels="chartLabels"
                unit="ms"
                :height="200"
                :area="false"
              />
            </div>
          </div>
        </template>

        <div class="charts">
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">吞吐量</span>
              <span class="chart-sub">每秒完成请求数（QPS）</span>
            </div>
            <LineChart :series="qpsSeries" :labels="chartLabels" :area="false" />
          </div>
          <div v-if="interfaceTargets.length" class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">接口 QPS</span>
              <a-select v-model:value="selectedInterface" size="small" style="width: 200px">
                <a-select-option :value="-1">全部接口（汇总）</a-select-option>
                <a-select-option v-for="item in interfaceTargets" :key="item.index" :value="item.index">
                  {{ item.label }}
                </a-select-option>
              </a-select>
            </div>
            <LineChart :series="interfaceQpsSeries" :labels="chartLabels" :area="false" />
            <div v-if="detail?.interfaceSamplesComplete === false" class="chart-sub">
              部分旧节点未上报接口曲线，该曲线可能偏低
            </div>
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">并发请求数</span>
              <span class="chart-sub">每秒实际并发数，观察加压与降压过程</span>
            </div>
            <LineChart :series="concurrencySeries" :labels="chartLabels" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">平均响应时间</span>
              <span class="chart-sub">每秒平均耗时，单位 ms</span>
            </div>
            <LineChart :series="avgRtSeries" :labels="chartLabels" unit="ms" :area="false" />
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
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">响应时间累积分布</span>
              <span class="chart-sub">横轴 ms · 纵轴累积占比，由直方图还原</span>
            </div>
            <CdfChart :points="cdfSeries" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">QPS 与平均响应时间</span>
              <span class="chart-sub">每点为一秒采样，观察吞吐上升时延迟是否恶化</span>
            </div>
            <ScatterChart :points="qpsLatencyPoints" x-unit="req/s" y-unit="ms" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">接收带宽趋势</span>
              <span class="chart-sub">每秒接收字节折算，单位 KB/s</span>
            </div>
            <LineChart :series="bandwidthSeries" :labels="chartLabels" unit="KB/s" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">TTFB 与响应体读取耗时</span>
              <span class="chart-sub">区分首字节等待与服务端/传输读取，单位 ms</span>
            </div>
            <LineChart :series="ttfbBodySeries" :labels="chartLabels" unit="ms" :area="false" />
          </div>
          <div v-if="urlCompareItems.length" class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">URL 平均响应时间对比</span>
              <span class="chart-sub">按地址对比平均耗时，最多展示前 12 个</span>
            </div>
            <BarChart :items="urlCompareItems" unit="ms" :digits="0" />
          </div>
          <div v-if="generatorCompareItems.length" class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">施压机 TPS 对比</span>
              <span class="chart-sub">各施压机自身窗口内的总请求 / 施压时长</span>
            </div>
            <BarChart :items="generatorCompareItems" unit="TPS" :digits="1" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">成功 / 失败 QPS</span>
              <span class="chart-sub">每秒成功与失败请求数</span>
            </div>
            <LineChart :series="successFailureSeries" :labels="chartLabels" :area="false" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">请求结果构成</span>
              <span class="chart-sub">成功与失败请求占比</span>
            </div>
            <PieChart :items="resultPieItems" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">状态码构成</span>
              <span class="chart-sub">按状态码请求数占比，最多展示前 8 个</span>
            </div>
            <PieChart :items="statusPieItems" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">错误类型构成</span>
              <span class="chart-sub">按错误类型次数占比，最多展示前 8 个</span>
            </div>
            <PieChart :items="errorTypePieItems" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">流量构成</span>
              <span class="chart-sub">接收与发送字节占比</span>
            </div>
            <PieChart
              :items="trafficPieItems"
              :center-text="fmtBytes((report?.bytesTotal ?? 0) + bytesSent)"
            />
          </div>
          <div v-if="stages.length > 1" class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">阶段平均 QPS 对比</span>
              <span class="chart-sub">各阶段平均吞吐，观察扩展效率</span>
            </div>
            <BarChart :items="stageQpsItems" unit="QPS" :digits="0" />
          </div>
          <div v-if="stages.length > 1" class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">阶段 P95 响应时间对比</span>
              <span class="chart-sub">各阶段尾延迟，观察并发上升后的恶化</span>
            </div>
            <BarChart :items="stageP95Items" unit="ms" :digits="0" />
          </div>
        </div>

        <div v-if="stages.length" class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">阶段汇总</div>
              <div class="card-sub">阶梯加压各阶段的执行情况；扩展效率 = 吞吐增幅 ÷ 并发增幅</div>
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
                  <th>稳态 QPS</th>
                  <th>平均延迟</th>
                  <th>P95</th>
                  <th>吞吐增幅</th>
                  <th>扩展效率</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(stage, i) in stageRows" :key="stage.index">
                  <td class="mono">{{ i + 1 }}</td>
                  <td class="mono">{{ fmtNum(stage.concurrent, 0) }}</td>
                  <td class="mono">{{ stage.durationSec }}s</td>
                  <td class="mono">{{ stage.requests.toLocaleString() }}</td>
                  <td class="mono" :class="{ bad: stage.errors > 0 }">{{ stage.errors }}</td>
                  <td class="mono">{{ fmtNum(stage.avgQps) }}</td>
                  <td class="mono">{{ stage.steadyQps == null ? '—' : fmtNum(stage.steadyQps) }}</td>
                  <td class="mono">{{ fmtNum(stage.avgRt) }} ms</td>
                  <td class="mono">{{ fmtNum(stage.p95Rt) }} ms</td>
                  <td class="mono">{{ stage.gain == null ? '—' : `${(stage.gain * 100).toFixed(1)}%` }}</td>
                  <td
                    class="mono"
                    :class="{ bad: stage.efficiency != null && stage.efficiency < 0.7 }"
                  >
                    {{ stage.efficiency == null ? '—' : `${(stage.efficiency * 100).toFixed(0)}%` }}
                  </td>
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
                <tr v-for="s in samplePageRows" :key="s.elapsedSec">
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
            <a-pagination
              v-if="samples.length > samplePageSize"
              v-model:current="samplePage"
              :page-size="samplePageSize"
              :total="samples.length"
              size="small"
              show-less-items
              class="table-pager"
            />
            <div v-if="!samples.length" class="inline-empty">没有采样数据</div>
          </div>
        </div>

        <div v-if="config?.pools?.length" class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">参数 / Token 池</div>
              <div class="card-sub">只统计用量，不包含明文；集群报告按节点汇总</div>
            </div>
          </div>
          <div class="data-table">
            <table>
              <thead>
                <tr>
                  <th>池名</th>
                  <th>条目</th>
                  <th>已用</th>
                  <th>注入请求</th>
                  <th>401 刷新</th>
                  <th>失效</th>
                  <th>范围</th>
                </tr>
              </thead>
              <tbody>
                <template v-for="pool in config.pools" :key="pool.name">
                  <tr>
                    <td class="mono">{{ pool.name }}</td>
                    <td class="mono">{{ pool.items }}</td>
                    <td class="mono">{{ pool.used }}</td>
                    <td class="mono">{{ pool.requests }}</td>
                    <td class="mono">{{ pool.refreshed }}</td>
                    <td class="mono" :class="{ bad: pool.invalid > 0 }">{{ pool.invalid }}</td>
                    <td>{{ poolScopeLabel(pool) }}</td>
                  </tr>
                  <tr v-for="node in pool.byNode ?? []" :key="`${pool.name}:${node.name}`" class="pool-node-row">
                    <td class="mono pool-node">↳ {{ node.name }}</td>
                    <td />
                    <td class="mono">{{ node.used }}</td>
                    <td class="mono">{{ node.requests }}</td>
                    <td class="mono">{{ node.refreshed }}</td>
                    <td />
                    <td />
                  </tr>
                </template>
              </tbody>
            </table>
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
              <span class="c-value mono">{{ modeLabel }}</span>
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

        <div v-if="singlePeaks" class="fm-card table-card cluster-card">
          <div class="card-head">
            <div>
              <div class="card-title">峰值指标</div>
              <div class="card-sub">按每秒采样统计，出现时刻相对起跑点</div>
            </div>
          </div>
          <div class="cluster-hero">
            <div class="hero-item">
              <span class="hero-label">峰值 QPS</span>
              <span class="hero-value">{{ fmtNum(singlePeaks.qps.value) }}</span>
              <span class="hero-sub">
                第 {{ singlePeaks.qps.second }} 秒 · 平台期 {{ singlePeaks.qps.heldSec }} 秒
              </span>
            </div>
            <div class="hero-item">
              <span class="hero-label">峰值并发</span>
              <span class="hero-value">{{ fmtNum(singlePeaks.concurrent.value, 0) }}</span>
              <span class="hero-sub">
                第 {{ singlePeaks.concurrent.second }} 秒 · 平台期
                {{ singlePeaks.concurrent.heldSec }} 秒
              </span>
            </div>
            <div class="hero-item">
              <span class="hero-label">最差 P95 / P99</span>
              <span class="hero-value">
                {{ fmtNum(singlePeaks.p95.value, 0) }} / {{ fmtNum(singlePeaks.p99.value, 0) }}
                <small>ms</small>
              </span>
              <span class="hero-sub">取自峰值时刻的延迟分位</span>
            </div>
            <div class="hero-item">
              <span class="hero-label">峰值带宽</span>
              <span class="hero-value">{{ fmtBytes(singlePeaks.bandwidth.value) }}/s</span>
              <span class="hero-sub">最高错误率 {{ singlePeaks.errorRate.value.toFixed(2) }}%</span>
            </div>
          </div>
        </div>

        <div v-if="loadGenerators.length" class="fm-card table-card">
          <div class="card-head">
            <div>
              <div class="card-title">施压机详情</div>
              <div class="card-sub">单机 1 行、集群按节点展开；缺失字段显示「—」，不做推断</div>
            </div>
          </div>
          <div class="data-table cluster-nodes">
            <table>
              <thead>
                <tr>
                  <th>施压机</th>
                  <th class="col-narrow">角色</th>
                  <th>地址</th>
                  <th>开始时间</th>
                  <th>总请求</th>
                  <th>成功 / 失败</th>
                  <th class="col-narrow">并发</th>
                  <th>成功率</th>
                  <th>平均耗时</th>
                  <th>TPS</th>
                  <th>出站带宽平均</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="node in loadGenerators" :key="`lg-${node.role}-${node.name}`">
                  <td class="mono">{{ node.name }}</td>
                  <td class="col-narrow">{{ node.role === 'master' ? '主节点' : '从节点' }}</td>
                  <td class="mono">{{ node.addr || '—' }}</td>
                  <td class="mono">{{ node.startMs ? fmtTime(node.startMs) : '—' }}</td>
                  <td class="mono">{{ node.totalRequests == null ? '—' : fmtNum(node.totalRequests, 0) }}</td>
                  <td class="mono">
                    {{ node.successCount == null ? '—' : fmtNum(node.successCount, 0) }}
                    /
                    <span :class="{ bad: (node.errorCount ?? 0) > 0 }">
                      {{ node.errorCount == null ? '—' : fmtNum(node.errorCount, 0) }}
                    </span>
                  </td>
                  <td class="col-narrow mono">{{ fmtNum(node.concurrent, 0) }}</td>
                  <td class="mono">
                    {{
                      node.totalRequests != null && node.totalRequests > 0 && node.successCount != null
                        ? `${((node.successCount / node.totalRequests) * 100).toFixed(2)}%`
                        : '—'
                    }}
                  </td>
                  <td class="mono">{{ node.avgRtMs == null ? '—' : `${fmtNum(node.avgRtMs, 0)} ms` }}</td>
                  <td class="mono">{{ node.tps == null ? '—' : fmtNum(node.tps, 1) }}</td>
                  <td class="mono">
                    {{ node.egressAvgKbps == null ? '—' : `${fmtNum(node.egressAvgKbps, 1)} KB/s` }}
                  </td>
                  <td class="mono">
                    {{ node.status === 'stopped' ? '已停止' : node.status === 'completed' ? '已完成' : node.status || '—' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div v-if="cluster" class="fm-card table-card cluster-card">
          <div class="card-head">
            <div>
              <div class="card-title">分布式压测</div>
              <div class="card-sub">主从节点合并口径，页面数值为主节点汇聚结果</div>
            </div>
            <span v-if="cluster.incomplete" class="head-note cluster-note-bad">结果不完整</span>
          </div>

          <a-alert
            v-if="cluster.incomplete"
            type="warning"
            show-icon
            class="cluster-alert"
            message="本次合并结果不完整"
            :description="cluster.note || '有节点失联或提前退出，吞吐与延迟可能偏低。'"
          />

          <div class="cluster-hero">
            <div class="hero-item">
              <span class="hero-label">集群总 QPS</span>
              <span class="hero-value">{{ fmtNum(clusterTotalQps) }}</span>
              <span class="hero-sub">{{ clusterNodes.length }} 个节点合并</span>
            </div>
            <div class="hero-item">
              <span class="hero-label">峰值 QPS</span>
              <span class="hero-value">
                {{ clusterPeaks ? fmtNum(clusterPeaks.qps.value) : '—' }}
              </span>
              <span class="hero-sub">
                <template v-if="clusterPeaks">
                  第 {{ clusterPeaks.qps.second }} 秒 · 持续 {{ clusterPeaks.qps.heldSec }} 秒
                  <template v-if="clusterPeaks.qps.topNode">
                    · {{ clusterPeaks.qps.topNode }}
                  </template>
                </template>
                <template v-else>未记录</template>
              </span>
            </div>
            <div class="hero-item">
              <span class="hero-label">峰值并发</span>
              <span class="hero-value">
                {{ clusterPeaks ? fmtNum(clusterPeaks.concurrent.value, 0) : fmtNum(cluster.totalConcurrent, 0) }}
              </span>
              <span class="hero-sub">
                <template v-if="clusterPeaks">
                  第 {{ clusterPeaks.concurrent.second }} 秒 · 持续 {{ clusterPeaks.concurrent.heldSec }} 秒
                </template>
                <template v-else>分配总并发</template>
              </span>
            </div>
            <div class="hero-item">
              <span class="hero-label">最差 P95 / P99</span>
              <span class="hero-value">
                {{ clusterPeaks ? `${fmtNum(clusterPeaks.p95.value, 0)} / ${fmtNum(clusterPeaks.p99.value, 0)}` : '—' }}
                <small>ms</small>
              </span>
              <span class="hero-sub">按合并直方图重算，非节点平均</span>
            </div>
            <div class="hero-item">
              <span class="hero-label">峰值带宽 / 错误率</span>
              <span class="hero-value">
                {{ clusterPeaks ? fmtBytes(clusterPeaks.bandwidth.value) + '/s' : '—' }}
              </span>
              <span class="hero-sub">
                {{ clusterPeaks ? `最高错误率 ${clusterPeaks.errorRate.value.toFixed(2)}%` : '未记录' }}
              </span>
            </div>
          </div>

          <div class="config-grid">
            <div class="config-item">
              <span class="c-label">协议版本</span>
              <span class="c-value mono">v{{ cluster.protocolVersion }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">主节点</span>
              <span class="c-value mono">{{ cluster.masterNode || '—' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">总并发</span>
              <span class="c-value mono">{{ fmtNum(cluster.totalConcurrent, 0) }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">起跑时间</span>
              <span class="c-value mono">{{ cluster.startedAt ? fmtTime(cluster.startedAt) : '—' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">结束时间</span>
              <span class="c-value mono">{{ cluster.finishedAt ? fmtTime(cluster.finishedAt) : '—' }}</span>
            </div>
            <div class="config-item">
              <span class="c-label">丢弃采样</span>
              <span class="c-value mono">{{ fmtNum(cluster.droppedSamples, 0) }} 条</span>
            </div>
          </div>

          <div v-if="clusterNodeSeries.length" class="cluster-series">
            <div class="cluster-series-head">
              <span>节点曲线 · {{ clusterMetricInfo.label }}</span>
              <a-radio-group v-model:value="clusterMetric" size="small" button-style="solid">
                <a-radio-button
                  v-for="item in CLUSTER_METRICS"
                  :key="item.key"
                  :value="item.key"
                  :disabled="clusterMetricDisabled(item.key)"
                >
                  {{ item.label }}
                </a-radio-button>
              </a-radio-group>
            </div>
            <div class="cluster-series-hint">
              {{ clusterMetricInfo.hint }} · 按节点上报的秒级采样绘制，右侧对齐、缺采样补 0
            </div>
            <LineChart :series="clusterNodeSeries" :unit="clusterMetricInfo.unit" :height="220" />
          </div>

          <div class="data-table cluster-nodes">
            <table>
              <thead>
                <tr>
                  <th>节点</th>
                  <th class="col-narrow">角色</th>
                  <th>地址</th>
                  <th class="col-narrow">权重</th>
                  <th class="col-narrow">分配并发</th>
                  <th class="col-narrow">状态</th>
                  <th>平均 QPS</th>
                  <th>峰值 QPS</th>
                  <th class="col-narrow">峰值并发</th>
                  <th class="col-narrow">贡献占比</th>
                  <th>采集健康</th>
                  <th>资源峰值</th>
                  <th>时钟偏移</th>
                  <th class="col-narrow">丢弃采样</th>
                  <th>施压窗口</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(node, index) in clusterNodes" :key="`${node.name}-${index}`">
                  <td>{{ node.name || '—' }}</td>
                  <td class="mono">{{ clusterRoleText(node.role) }}</td>
                  <td class="mono cluster-addr">{{ node.addr || '—' }}</td>
                  <td class="mono">{{ fmtNum(node.weight, 2) }}</td>
                  <td class="mono">{{ fmtNum(node.concurrent, 0) }}</td>
                  <td class="mono" :class="clusterStatusTone(node.status)">
                    {{ clusterStatusText(node.status) }}
                  </td>
                  <td class="mono">{{ fmtNum(node.qpsAvg) }}</td>
                  <td class="mono">{{ fmtNum(node.peakQps) }}</td>
                  <td class="mono">{{ fmtNum(node.peakConcurrent, 0) }}</td>
                  <td class="mono">{{ `${((node.share ?? 0) * 100).toFixed(1)}%` }}</td>
                  <td class="mono" :class="clusterHealthTone(node)" :title="clusterHealthText(node)">
                    {{ clusterHealthText(node) }}
                  </td>
                  <td class="mono">{{ clusterResourceText(node) }}</td>
                  <td class="mono">
                    {{ node.clockOffsetMs != null ? `${node.clockOffsetMs} ± ${node.clockUncertaintyMs} ms` : '—' }}
                    <div v-if="Math.abs(node.clockDriftMs ?? 0) >= 1" class="cluster-drift">
                      漂移 {{ node.clockDriftMs > 0 ? '+' : '' }}{{ node.clockDriftMs.toFixed(1) }} ms
                    </div>
                  </td>
                  <td class="mono" :class="{ bad: node.droppedSamples > 0 }">{{ node.droppedSamples }}</td>
                  <td class="mono cluster-window">
                    {{ node.startMs ? fmtTime(node.startMs) : '—' }}
                    <template v-if="node.endMs && node.startMs">
                      · {{ fmtDuration(node.endMs - node.startMs) }}
                    </template>
                  </td>
                </tr>
              </tbody>
            </table>
            <div v-if="!clusterNodes.length" class="inline-empty">未记录节点明细</div>
          </div>
        </div>

        <template v-if="requestSummaries.length">
          <div class="section-head">
            <div>
              <h2 class="section-title">按接口汇总</h2>
              <div class="section-desc">
                本次压测共 {{ requestSummaries.length }} 个接口，共享同一 Token / 参数池；整体 KPI 为全接口合并口径。
              </div>
            </div>
          </div>
          <div class="fm-card request-card">
            <div class="data-table request-table">
              <table>
                <thead>
                  <tr>
                    <th>#</th>
                    <th>接口</th>
                    <th>方法</th>
                    <th>权重</th>
                    <th>请求数</th>
                    <th>成功 / 失败</th>
                    <th>成功率</th>
                    <th>平均耗时</th>
                    <th>最长耗时</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(row, index) in requestSummaries" :key="`${row.method}-${row.url}`">
                    <td class="mono">{{ index + 1 }}</td>
                    <td class="req-url-col" :title="row.url">
                      <div>{{ row.name }}</div>
                      <div class="mono target-note">{{ row.url }}</div>
                    </td>
                    <td class="req-method-col">
                      <span class="req-method">{{ row.method }}</span>
                    </td>
                    <td class="mono">{{ (row.weight * 100).toFixed(1) }}%</td>
                    <td class="mono">{{ fmtNum(row.requests, 0) }}</td>
                    <td class="mono">
                      <span>{{ fmtNum(row.success, 0) }}</span>
                      /
                      <span :class="{ bad: row.errors > 0 }">{{ fmtNum(row.errors, 0) }}</span>
                    </td>
                    <td class="mono" :class="{ bad: row.errors > 0 }">
                      {{ row.successRate.toFixed(2) }}%
                    </td>
                    <td class="mono">{{ fmtNum(row.avgMs, 0) }} ms</td>
                    <td class="mono">{{ fmtNum(row.maxMs, 0) }} ms</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </template>

        <div class="section-head">
          <div>
            <h2 class="section-title">URL 统计</h2>
            <div class="section-desc">
              按方法、地址与状态码聚合，与是否记录请求明细无关；URL 中的敏感参数已脱敏。
              <template v-if="urlStats.length && !urlStatsComplete">
                统计不完整：URL 键超过上限或部分从节点未上报。
              </template>
            </div>
          </div>
        </div>
        <div class="fm-card request-card">
          <div v-if="urlStats.length" class="data-table request-table">
            <table>
              <thead>
                <tr>
                  <th class="req-method-col">方法</th>
                  <th class="req-url-col">URL</th>
                  <th>请求数</th>
                  <th>成功 / 失败</th>
                  <th>成功率</th>
                  <th>平均耗时</th>
                  <th>最长耗时</th>
                  <th>最长耗时时间</th>
                  <th>总耗时</th>
                  <th>平均包</th>
                  <th>总流量</th>
                  <th>状态码</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in urlStats" :key="`${row.method}-${row.url}-${row.statusCode}`">
                  <td class="req-method-col">
                    <span class="req-method">{{ row.method }}</span>
                  </td>
                  <td class="req-url-col mono" :title="row.url">{{ row.url }}</td>
                  <td class="mono">{{ fmtNum(row.requests, 0) }}</td>
                  <td class="mono">
                    <span>{{ fmtNum(row.successCount, 0) }}</span>
                    /
                    <span :class="{ bad: row.errorCount > 0 }">{{ fmtNum(row.errorCount, 0) }}</span>
                  </td>
                  <td class="mono" :class="{ bad: row.errorCount > 0 }">
                    {{ urlSuccessRate(row).toFixed(2) }}%
                  </td>
                  <td class="mono">{{ fmtNum(row.avgMs, 0) }} ms</td>
                  <td class="mono">{{ fmtNum(row.maxMs, 0) }} ms</td>
                  <td class="mono">{{ urlMaxAt(row) }}</td>
                  <td class="mono">{{ row.totalHours.toFixed(3) }} h</td>
                  <td class="mono">{{ row.avgKb.toFixed(1) }} KB</td>
                  <td class="mono">{{ row.totalMb.toFixed(1) }} MB</td>
                  <td class="mono" :title="row.statusSummary">{{ row.statusSummary || '—' }}</td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else class="inline-empty">
            {{ urlStatsComplete ? '本次压测没有可统计的 URL 数据' : '该报告生成于旧版本，未记录按 URL 统计' }}
          </div>
        </div>

        <div class="section-head">
          <div>
            <h2 class="section-title">错误地址</h2>
            <div class="section-desc">只列出存在失败请求的 URL；错误信息已脱敏。</div>
          </div>
        </div>
        <div class="fm-card request-card">
          <div v-if="errorUrls.length" class="data-table request-table">
            <table>
              <thead>
                <tr>
                  <th class="req-method-col">方法</th>
                  <th class="req-url-col">URL</th>
                  <th>错误数</th>
                  <th>平均耗时</th>
                  <th>最长耗时</th>
                  <th>最长耗时时间</th>
                  <th class="req-error-col">错误信息</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in errorUrls" :key="`err-${row.method}-${row.url}-${row.statusCode}`">
                  <td class="req-method-col">
                    <span class="req-method">{{ row.method }}</span>
                  </td>
                  <td class="req-url-col mono" :title="row.url">{{ row.url }}</td>
                  <td class="mono bad">{{ fmtNum(row.errorCount, 0) }}</td>
                  <td class="mono">{{ fmtNum(row.avgMs, 0) }} ms</td>
                  <td class="mono">{{ fmtNum(row.maxMs, 0) }} ms</td>
                  <td class="mono">{{ urlMaxAt(row) }}</td>
                  <td class="req-error-col mono" :title="row.errorMessage ?? ''">
                    {{ row.errorMessage || '—' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else class="inline-empty">
            {{ urlStats.length ? '本次压测没有失败请求' : '未记录错误地址统计' }}
          </div>
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

    <footer v-if="detail" class="report-disclaimer">
      <b>免责声明：</b>{{ REPORT_DISCLAIMER }}
    </footer>
  </div>
</template>

<style scoped>
.overview {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 22px 24px;
  position: relative;
  overflow: hidden;
}

/* 顶部主题色细条：和导出报告的封面呼应，快速区分「概览区」 */
.overview::before {
  content: '';
  position: absolute;
  inset: 0 0 auto;
  height: 3px;
  background: linear-gradient(90deg, var(--fm-primary), var(--fm-chart-2));
  opacity: 0.85;
}

.overview-main {
  flex: 1;
  min-width: 0;
}

.title-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 10px;
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
  font-size: 19px;
  font-weight: 700;
  margin: 0;
  letter-spacing: -0.2px;
  line-height: 1.35;
  word-break: break-word;
}

.status-pill {
  font-size: 10px;
  padding: 1px 9px;
  border-radius: var(--fm-radius-pill);
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
  border-radius: var(--fm-radius-pill);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.meta-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 8px;
  margin-top: 8px;
  font-size: 11px;
  line-height: 1.6;
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

.target-copy {
  flex-shrink: 0;
  margin-top: 2px;
  font-size: 12px;
  color: var(--fm-muted);
  cursor: pointer;
}

.target-copy:hover {
  color: var(--fm-primary);
}

/* 多接口报告：主目标后补充「共 N 个接口」的说明 */
.target-note {
  flex-shrink: 0;
  align-self: center;
  font-size: 10.5px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.target-url.missing {
  color: var(--fm-muted);
  cursor: default;
  font-size: 12px;
}

/* 多接口：目标直接写「全部接口（共 N 个）」 */
.target-url.multi-target {
  color: var(--fm-text-2);
  cursor: default;
  font-size: 12.5px;
  font-weight: 600;
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
  margin-top: 18px;
}

.metric-card {
  padding: 14px 15px 13px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  position: relative;
  overflow: hidden;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.metric-card:hover {
  border-color: var(--fm-border-strong);
  box-shadow: 0 2px 10px -4px var(--fm-border-strong);
}

/* 指标顶部语义色条：吞吐主色、峰值蓝、成功率/APDEX 按结果取色、延迟警示色 */
.metric-card[class*='tone-']::before {
  content: '';
  position: absolute;
  inset: 0 0 auto;
  height: 2px;
  background: var(--tone);
  opacity: 0.75;
}

.metric-card.tone-accent {
  --tone: var(--fm-primary);
}

.metric-card.tone-info {
  --tone: var(--fm-chart-2);
}

.metric-card.tone-ok {
  --tone: var(--fm-success);
}

.metric-card.tone-warn {
  --tone: var(--fm-warning);
}

.metric-card.tone-bad {
  --tone: var(--fm-danger);
}

.metric-card.tone-violet {
  --tone: var(--fm-chart-violet);
}

.m-value.accent {
  color: var(--fm-primary);
}

.m-value.info {
  color: var(--fm-chart-2);
}

.m-value.violet {
  color: var(--fm-chart-violet);
}

.m-label {
  font-size: 10px;
  letter-spacing: 0.02em;
  color: var(--fm-muted);
}

.m-value {
  font-size: 18px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1.25;
  white-space: nowrap;
}

.m-value i {
  font-style: normal;
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
  margin-left: 4px;
}

/* APDEX 评级标签：与数值拉开间距，避免 1.000优秀 糊在一起 */
.m-tag {
  display: inline-block;
  margin-left: 6px;
  padding: 1px 7px;
  font-size: 10px;
  font-weight: 600;
  vertical-align: 1px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.m-tag.warn {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.m-tag.bad {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
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
  border-radius: var(--fm-radius-pill);
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
  border-radius: var(--fm-radius-pill);
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
  gap: 16px;
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
  border-radius: var(--fm-radius-pill);
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.code-bar {
  height: 100%;
  border-radius: var(--fm-radius-pill);
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
.chart-range {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  margin-top: 16px;
  padding: 10px 14px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
}

.range-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  white-space: nowrap;
}

.range-picker {
  width: 400px;
}

.range-slider {
  flex: 1;
  min-width: 180px;
  margin: 0;
}

.range-text {
  font-size: 11px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.period-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-top: 18px;
}

.period-title {
  font-size: 13px;
  font-weight: 600;
}

.period-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

.period-charts {
  margin-top: 8px;
}

.charts {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
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
  border-radius: var(--fm-radius-pill);
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.hist-bar {
  height: 100%;
  border-radius: var(--fm-radius-pill);
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
.table-pager {
  margin-top: 8px;
  text-align: right;
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
  border-radius: var(--fm-radius-pill);
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

/* ---------- 分布式压测 ---------- */
.cluster-card {
  margin-top: 16px;
}

.cluster-alert {
  margin: 4px 0 12px;
}

.cluster-note-bad {
  color: var(--fm-danger);
}

.cluster-nodes {
  margin-top: 12px;
}

.cluster-series {
  margin-top: 14px;
  padding: 12px 14px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
}

.cluster-series-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
}

.cluster-series-hint {
  margin-bottom: 6px;
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.cluster-drift {
  font-size: 11px;
  color: var(--fm-warning);
}

.cluster-hero {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 10px;
  margin: 12px 0;
}

.hero-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 12px 14px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
  background: var(--fm-surface-2);
}

.hero-label {
  font-size: 11.5px;
  color: var(--fm-muted);
}

.hero-value {
  font-family: var(--fm-mono);
  font-size: 20px;
  font-weight: 600;
  color: var(--fm-text);
}

.hero-value small {
  font-size: 11px;
  color: var(--fm-muted);
  font-weight: 400;
}

.hero-sub {
  font-size: 11.5px;
  color: var(--fm-muted);
}

.cluster-addr {
  word-break: break-all;
}

.cluster-window {
  white-space: nowrap;
}

/* ---------- 错误明细 ---------- */
.section-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 12px;
  margin: 28px 2px 12px;
  padding-bottom: 9px;
  border-bottom: 1px solid var(--fm-border);
}

.section-title {
  position: relative;
  font-size: 15px;
  font-weight: 700;
  margin: 0;
  padding-left: 11px;
  letter-spacing: 0.01em;
}

.section-title::before {
  content: '';
  position: absolute;
  left: 0;
  top: 3px;
  bottom: 3px;
  width: 3px;
  border-radius: var(--fm-radius-pill);
  background: linear-gradient(180deg, var(--fm-primary), var(--fm-chart-2));
}

.section-desc {
  margin-top: 5px;
  font-size: 11px;
  line-height: 1.65;
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
  .overview {
    flex-direction: column;
    align-items: stretch;
  }
  .verdict {
    max-width: none;
  }
  .metrics {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .charts {
    grid-template-columns: 1fr;
  }
  .range-picker {
    width: 100%;
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

@media (max-width: 760px) {
  .metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .percentile-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .drift-rule {
    margin-left: 0;
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
  margin-top: 14px;
  padding: 16px 18px;
}
.grade-badge {
  font-size: 11px;
  padding: 2px 10px;
  border-radius: var(--fm-radius-pill);
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
  align-items: flex-start;
  gap: 10px;
  padding: 8px 10px;
  margin-bottom: 10px;
  font-size: 11.5px;
  line-height: 1.6;
  background: var(--fm-warning-soft);
  border: 1px solid var(--fm-warning);
  border-radius: var(--fm-radius-sm);
}
.bias-banner b {
  flex-shrink: 0;
  padding: 0 7px;
  font-size: 10.5px;
  line-height: 1.7;
  white-space: nowrap;
  color: var(--fm-warning);
  border: 1px solid var(--fm-warning);
  background: var(--fm-surface);
}
.bias-banner span {
  color: var(--fm-text-2);
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
  flex-wrap: wrap;
  gap: 4px 10px;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
  font-size: 12px;
}
.drift-label {
  color: var(--fm-text-2);
  font-weight: 500;
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
  margin-top: 14px;
  padding: 16px 18px;
}
.capacity-badge {
  font-size: 11px;
  padding: 2px 10px;
  border-radius: var(--fm-radius-pill);
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
  margin-top: 14px;
  padding: 16px 18px;
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

/* 池统计：节点明细行弱化展示 */
.pool-node-row td {
  background: var(--fm-surface-sunken);
  font-size: 11.5px;
  color: var(--fm-muted);
}

.pool-node-row .pool-node {
  color: var(--fm-text-2);
}

.report-disclaimer {
  margin: 18px 0 8px;
  padding: 12px 14px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
  font-size: 12px;
  line-height: 1.7;
}

@media (max-width: 1180px) {
  .text-grid {
    grid-template-columns: 1fr;
  }
}
</style>
