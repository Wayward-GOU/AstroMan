import { writeFileSync } from 'node:fs'
import { buildReportHtml } from '../src/utils/reportExport'
import type { ReportDetail, ReportError, ReportRequestRecord, ReportSample } from '../src/types/models'

/**
 * 生成一份「看起来像真实交付物」的示例报告：
 * 5 阶段阶梯加压，第 4 阶段起吞吐不再增长（饱和拐点），并伴随 503 突发。
 */

interface StageDef {
  concurrent: number
  qps: number
  avgRt: number
  p95: number
  p99: number
  ttfb: number
}

const stageDefs: StageDef[] = [
  { concurrent: 100, qps: 2600, avgRt: 21, p95: 42, p99: 84, ttfb: 12 },
  { concurrent: 300, qps: 6200, avgRt: 24, p95: 48, p99: 92, ttfb: 13 },
  { concurrent: 600, qps: 8900, avgRt: 29, p95: 60, p99: 112, ttfb: 15 },
  { concurrent: 900, qps: 9250, avgRt: 38, p95: 94, p99: 170, ttfb: 19 },
  { concurrent: 1200, qps: 9350, avgRt: 50, p95: 128, p99: 235, ttfb: 24 },
]

const STAGE_SECONDS = 60
const RAMP_SECONDS = 12
const totalSeconds = stageDefs.length * STAGE_SECONDS

const mix = (from: number, to: number, ratio: number) => from + (to - from) * ratio

const samples: ReportSample[] = []
for (let index = 1; index <= totalSeconds; index += 1) {
  const stageIndex = Math.floor((index - 1) / STAGE_SECONDS)
  const current = stageDefs[stageIndex]
  const previous = stageDefs[stageIndex - 1] ?? { ...current, qps: 0, concurrent: 0, p95: 0, p99: 0 }
  const position = (index - 1) % STAGE_SECONDS
  const raw = Math.min(1, (position + 1) / RAMP_SECONDS)
  const ease = raw * raw * (3 - 2 * raw)

  const jitter = 1 + Math.sin(index / 5) * 0.028 + Math.sin(index / 1.7) * 0.012
  const successCount = Math.round(mix(previous.qps, current.qps, ease) * jitter)

  // 第 4、5 阶段出现 503 突发
  let errorCount = 0
  if (stageIndex >= 3 && position > 6) {
    const period = stageIndex === 3 ? 3 : 2
    const burst = stageIndex === 3 ? 40 : 120
    if (position % period === 0) errorCount = burst
  }

  samples.push({
    elapsedSec: index,
    concurrent: Math.round(mix(previous.concurrent, current.concurrent, ease)),
    qps: successCount + errorCount,
    avgRt: Number(mix(previous.avgRt, current.avgRt, ease).toFixed(1)),
    p50Rt: Number((mix(previous.avgRt, current.avgRt, ease) * 0.86).toFixed(1)),
    p95Rt: Number(mix(previous.p95, current.p95, ease).toFixed(1)),
    p99Rt: Number(mix(previous.p99, current.p99, ease).toFixed(1)),
    ttfbAvg: Number(mix(previous.ttfb, current.ttfb, ease).toFixed(1)),
    bytes: Math.round(successCount * 24 * 1024),
    successCount,
    errorCount,
  })
}

const totalRequests = samples.reduce((sum, item) => sum + item.successCount + item.errorCount, 0)
const errorCount = samples.reduce((sum, item) => sum + item.errorCount, 0)
const successCount = totalRequests - errorCount
const avgRt =
  samples.reduce((sum, item) => sum + item.avgRt * (item.successCount + item.errorCount), 0) /
  totalRequests
const bytesTotal = samples.reduce((sum, item) => sum + item.bytes, 0)

/** 阶段汇总 */
const stages = stageDefs.map((def, index) => {
  const slice = samples.slice(index * STAGE_SECONDS, (index + 1) * STAGE_SECONDS)
  const requests = slice.reduce((sum, item) => sum + item.successCount + item.errorCount, 0)
  return {
    index,
    concurrent: def.concurrent,
    durationSec: STAGE_SECONDS,
    requests,
    errors: slice.reduce((sum, item) => sum + item.errorCount, 0),
    avgQps: Number((requests / STAGE_SECONDS).toFixed(1)),
    avgRt: Number((slice.reduce((sum, item) => sum + item.avgRt, 0) / slice.length).toFixed(1)),
    p95Rt: Number((slice.reduce((sum, item) => sum + item.p95Rt, 0) / slice.length).toFixed(1)),
  }
})

/** 响应时间直方图，按比例分配并保证合计等于总请求数 */
const histogramPlan: Array<[string, number, number, number]> = [
  ['≤10ms', 0, 10, 0.04],
  ['10-25ms', 10, 25, 0.44],
  ['25-50ms', 25, 50, 0.33],
  ['50-100ms', 50, 100, 0.13],
  ['100-200ms', 100, 200, 0.04],
  ['200-350ms', 200, 350, 0.014],
  ['>350ms', 350, 900, 0.006],
]
let assigned = 0
const histogram = histogramPlan.map(([label, minMs, maxMs, ratio], index) => {
  const count =
    index === histogramPlan.length - 1 ? totalRequests - assigned : Math.round(totalRequests * ratio)
  assigned += count
  return { label, minMs, maxMs, count }
})

const errors: ReportError[] = samples
  .filter((item) => item.errorCount > 0)
  .slice(0, 14)
  .map((item) => ({
    elapsedSec: item.elapsedSec,
    errorType: item.errorCount > 100 ? 'HTTP' : item.elapsedSec % 7 === 0 ? 'Timeout' : 'HTTP',
    errorMessage:
      item.errorCount > 100
        ? 'HTTP 503 Service Unavailable: upstream connect error'
        : 'HTTP 503 Service Unavailable',
    requestUrl: 'https://api.example.com/v1/orders?page=1&size=20',
    responseStatus: 503,
    responseBody: '{"code":503,"message":"upstream connect error or disconnect/reset before headers"}',
  }))

const errorTypes = [
  { errorType: 'HTTP', count: Math.round(errorCount * 0.86) },
  { errorType: 'Timeout', count: errorCount - Math.round(errorCount * 0.86) },
]

/** 模拟 40 条已记录明细，用于 Top 10 慢请求 */
const requests: ReportRequestRecord[] = Array.from({ length: 40 }, (_, index) => {
  const tail = index < 6
  const latency = tail ? 380 + index * 46 : 18 + ((index * 7) % 26)
  return {
    elapsedMs: 20_000 + index * 6_700,
    timeMs: Date.UTC(2026, 9, 30, 12, 0, 20) + index * 6_700,
    method: 'GET',
    url: `https://api.example.com/v1/orders?page=${(index % 5) + 1}&size=20&tenant=t-${100 + index}`,
    status: tail ? 200 : 200,
    latencyMs: latency,
    ttfbMs: Math.round(latency * 0.62),
    bytes: 24 * 1024,
    success: true,
    body: null,
    params: `page=${(index % 5) + 1}&size=20`,
    headers: null,
    responseHeaders: null,
    responseBody: '{"code":0,"data":{"total":1820,"items":[]}}',
    errorType: null,
    errorMessage: null,
  }
})

const detail: ReportDetail = {
  report: {
    id: 231,
    planName: 'order-query 容量摸底与饱和点定位',
    totalRequests,
    successCount,
    errorCount,
    avgRt: Number(avgRt.toFixed(2)),
    minRt: 9,
    maxRt: 684,
    p50Rt: 26,
    p75Rt: 34,
    p90Rt: 48,
    p95Rt: 64,
    p99Rt: 142,
    p999Rt: 310,
    stddevRt: 10.8,
    ttfbAvg: 16,
    ttfbP95: 34,
    apdex: 0.91,
    bytesTotal,
    maxQps: Math.max(...samples.map((item) => item.qps)),
    avgQps: Number((totalRequests / totalSeconds).toFixed(1)),
    targetRps: 10000,
    errorRate: Number(((errorCount / totalRequests) * 100).toFixed(3)),
    concurrency: 1200,
    status: 'completed',
    startTime: Date.UTC(2026, 9, 30, 12, 0, 0),
    endTime: Date.UTC(2026, 9, 30, 12, 0, 0) + totalSeconds * 1000,
    createdAt: Date.UTC(2026, 9, 30, 12, 5, 0),
  },
  samples,
  errors,
  statusCodes: [
    { code: 200, count: successCount },
    { code: 503, count: errorCount },
  ],
  errorTypes,
  histogram,
  stages,
  config: {
    mode: 'stages',
    method: 'GET',
    url: 'https://api.example.com/v1/orders?page=1&size=20',
    headers: 4,
    params: 2,
    bodyType: 'none',
    timeoutMs: 3000,
    connectTimeoutMs: 1000,
    followRedirects: false,
    thinkTimeMs: 0,
    rampUpSec: RAMP_SECONDS,
    targetRps: 10000,
    stages: stageDefs.map((item) => ({ concurrent: item.concurrent, durationSec: STAGE_SECONDS })),
    sla: { p95Ms: 100, p99Ms: 200, errorRate: 1, minRps: 6000 },
    recordDetail: 'sample',
    recordDetailRequested: 'auto',
    agent: {
      tool: 'AstroMan',
      version: '1.0.8',
      os: 'Windows',
      osVersion: '11 (26100)',
      arch: 'x86_64',
      host: 'BENCH-01',
      cpuCores: 16,
      memTotalMb: 32768,
    },
    metrics: {
      bytesSent: totalRequests * 186,
      errorsDropped: 0,
      recordsDropped: 0,
      recordMode: 'sample',
      cpuPeakCores: 9.4,
      memPeakMb: 1180,
      sampleIntervalSec: 1,
    },
  },
  scenarioText:
    '目标：摸清订单查询接口在单实例部署下的容量上限，并定位饱和拐点。\n' +
    '环境：预发环境 4 副本部署，MySQL 8.0 主从，Redis 6 缓存命中率约 92%；施压端与目标同机房内网直连。\n' +
    '策略：5 阶段阶梯加压（100 → 300 → 600 → 900 → 1200 并发），每阶段稳态 60 秒，阶段内 12 秒线性升压；明细按 1% 采样记录。',
  summaryText:
    '系统在 600 并发时达到约 8,900 req/s，继续加至 900 / 1200 并发后吞吐基本停滞（增幅 < 5%），而 P95 从 60ms 抬升至 94 / 128ms，判定饱和拐点在 600 并发附近。\n' +
    '第 4 阶段起出现 503 突发，错误集中在 upstream connect error，指向应用侧连接池或上游依赖耗尽，建议优先扩大数据库连接池并复核 Redis 连接复用情况。',
}

const html = buildReportHtml(detail, requests)
writeFileSync('docs/sample-stress-report.html', html, 'utf8')
console.log('written', html.length, 'bytes', '| requests', totalRequests, '| errors', errorCount, '| errorRate', detail.report.errorRate)
