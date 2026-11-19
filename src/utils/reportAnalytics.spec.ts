import { describe, expect, it } from 'vitest'
import type { ReportDetail, ReportSample, StageSummary } from '@/types/models'
import { detectKnee, downsample, percentile, qpsOverview, qpsP10, scoreReport, selfCheck } from './reportAnalytics'

function sample(elapsedSec: number, qps: number, p95: number): ReportSample {
  return {
    elapsedSec,
    concurrent: 10,
    qps,
    avgRt: 10,
    p50Rt: 8,
    p95Rt: p95,
    p99Rt: p95 * 1.2,
    ttfbAvg: 5,
    bodyAvg: 3,
    bodyP95: 4,
    bytes: 1000,
    successCount: Math.round(qps),
    errorCount: 0,
  }
}

function detail(overrides: Partial<ReportDetail> = {}): ReportDetail {
  const samples: ReportSample[] = []
  // 三个阶段：100 并发 ~1000 QPS，200 并发 ~1900 QPS，400 并发仍 ~1950 QPS（饱和）
  const plan: Array<[number, number, number]> = [
    [100, 1000, 20],
    [200, 1900, 40],
    [400, 1950, 120],
  ]
  plan.forEach(([concurrentValue, qps, p95], stageIndex) => {
    for (let second = 1; second <= 30; second += 1) {
      samples.push(sample(stageIndex * 30 + second, qps, p95))
      void concurrentValue
    }
  })

  const stages: StageSummary[] = plan.map(([concurrentValue, qps, p95], index) => ({
    index,
    concurrent: concurrentValue,
    durationSec: 30,
    requests: qps * 30,
    errors: 0,
    avgQps: qps,
    avgRt: 10,
    p95Rt: p95,
  }))

  return {
    report: {
      id: 1,
      planName: 'test',
      totalRequests: 100000,
      successCount: 100000,
      errorCount: 0,
      avgRt: 10,
      minRt: 1,
      maxRt: 200,
      p50Rt: 8,
      p75Rt: 9,
      p90Rt: 12,
      p95Rt: 20,
      p99Rt: 30,
      p999Rt: 50,
      stddevRt: 2,
      ttfbAvg: 5,
      ttfbP95: 9,
      apdex: 0.95,
      bytesTotal: 100000 * 1000,
      maxQps: 1950,
      avgQps: 1600,
      targetRps: 0,
      errorRate: 0,
      concurrency: 400,
      status: 'completed',
      startTime: 0,
      endTime: 90000,
      createdAt: 0,
    },
    samples,
    errors: [],
    statusCodes: [],
    errorTypes: [],
    histogram: [],
    stages,
    config: null,
    ...overrides,
  }
}

describe('reportAnalytics', () => {
  it('percentile 使用线性插值', () => {
    expect(percentile([1, 2, 3, 4], 0.5)).toBeCloseTo(2.5)
    expect(percentile([10], 0.9)).toBe(10)
    expect(percentile([], 0.9)).toBe(0)
  })

  it('90% QPS 取 P10 并剔除预热段与末点', () => {
    const samples = Array.from({ length: 20 }, (_, index) => sample(index + 1, index + 1, 10))
    const result = qpsP10(samples)
    expect(result.used).toBeLessThan(samples.length)
    // 首点与末点被剔除后剩 2..18，P10 落在 3~5 之间
    expect(result.value).toBeGreaterThan(2)
    expect(result.value).toBeLessThan(6)
  })

  it('显式 ramp-up 配置会剔除整段预热数据', () => {
    const samples = Array.from({ length: 40 }, (_, index) => sample(index + 1, index < 10 ? 1 : 100, 10))
    const withoutWarmup = qpsP10(samples, { rampUpSec: 10 })
    expect(withoutWarmup.used).toBe(29)
    expect(withoutWarmup.value).toBe(100)
  })

  it('最低 QPS 与 90% 分布同窗口，不被升压首秒拉低', () => {
    const samples = Array.from({ length: 30 }, (_, index) =>
      sample(index + 1, index < 5 ? 30 : 500, 10),
    )
    const overview = qpsOverview(detail({ samples }), { rampUpSec: 5 })
    // 升压段的 30 req/s 不参与，最低值应落在稳态的 500
    expect(overview.min).toBe(500)
    expect(overview.ninety).toBe(500)
  })

  it('QPS 指标组：平均取后端聚合值，90% 分布取采样 P10，达成率与 SLA 同口径', () => {
    const base = detail()
    const overview = qpsOverview(base)
    // 平均 / 峰值沿用后端聚合值，避免与 SLA、列表页数字不一致
    expect(overview.avg).toBe(1600)
    expect(overview.max).toBe(1950)
    expect(overview.min).toBe(1000)
    expect(overview.ninety).toBe(1000)
    expect(overview.target).toBe(0)
    expect(overview.attainment).toBeNull()
    expect(overview.perConcurrent).toBeCloseTo(4, 5)

    const withTarget = qpsOverview({
      ...base,
      report: { ...base.report, targetRps: 2000 },
    })
    expect(withTarget.target).toBe(2000)
    expect(withTarget.attainment).toBeCloseTo(80, 5)
  })

  it('采样为空时 90% 分布与最低 QPS 归零，平均 / 峰值退回后端聚合值', () => {
    const overview = qpsOverview(detail({ samples: [] }))
    expect(overview.ninety).toBe(0)
    expect(overview.min).toBe(0)
    expect(overview.used).toBe(0)
    expect(overview.avg).toBe(1600)
    expect(overview.max).toBe(1950)
  })

  it('阶梯数据能检出饱和拐点', () => {
    const result = detectKnee(detail())
    expect(result.saturated).toBe(true)
    expect(result.concurrent).toBe(200)
    expect(result.qps).toBeCloseTo(1900, -1)
  })

  it('固定并发模式不做容量判定', () => {
    const data = detail({ stages: [] })
    const result = detectKnee(data)
    expect(result.saturated).toBe(false)
    expect(result.note).toContain('固定并发')
  })

  it('四维打分：无目标 RPS 时容量维度不评分', () => {
    const result = scoreReport(detail())
    expect(result.dimensions[0].score).toBeNull()
    expect(result.total).not.toBeNull()
    expect(['优秀', '良好', '一般', '较差']).toContain(result.grade)
  })

  it('自洽校验能发现计数不一致', () => {
    const data = detail()
    data.report.successCount = 99999
    const checks = selfCheck(data)
    expect(checks.find((item) => item.key === 'count')?.pass).toBe(false)
  })

  it('降采样保留首尾且不超上限', () => {
    const values = Array.from({ length: 1000 }, (_, index) => index)
    const result = downsample(values, 300)
    expect(result.length).toBe(300)
    expect(result[0]).toBe(0)
    expect(result[result.length - 1]).toBe(999)
  })
})
