import { describe, expect, it } from 'vitest'
import type { ReportDetail, ReportSample } from '@/types/models'
import { buildAdvice, buildFindings, cdfPoints, gradeReport, latencyDrift } from './reportNarrative'

function samples(count: number, patch: (index: number) => Partial<ReportSample> = () => ({})): ReportSample[] {
  return Array.from({ length: count }, (_, index) => ({
    elapsedSec: index + 1,
    concurrent: 200,
    qps: 5000,
    avgRt: 20,
    p50Rt: 18,
    p95Rt: 40,
    p99Rt: 80,
    ttfbAvg: 12,
    bytes: 1024,
    successCount: 5000,
    errorCount: 0,
    ...patch(index),
  }))
}

function makeDetail(patch: Partial<ReportDetail['report']> = {}, samplePatch?: (index: number) => Partial<ReportSample>): ReportDetail {
  const rows = samples(120, samplePatch)
  const total = rows.reduce((sum, item) => sum + item.successCount, 0)
  const errors = rows.reduce((sum, item) => sum + item.errorCount, 0)
  return {
    report: {
      id: 1,
      planName: 'p',
      totalRequests: total + errors,
      successCount: total,
      errorCount: errors,
      avgRt: 20,
      minRt: 3,
      maxRt: 300,
      p50Rt: 18,
      p75Rt: 22,
      p90Rt: 30,
      p95Rt: 40,
      p99Rt: 30,
      p999Rt: 60,
      stddevRt: 4,
      ttfbAvg: 12,
      ttfbP95: 25,
      apdex: 0.97,
      bytesTotal: rows.reduce((sum, item) => sum + item.bytes, 0),
      maxQps: 5000,
      avgQps: 5000,
      targetRps: 5000,
      errorRate: 0,
      concurrency: 200,
      status: 'completed',
      startTime: 1_700_000_000_000,
      endTime: 1_700_000_000_000 + 120_000,
      createdAt: 1_700_000_000_000,
      ...patch,
    },
    samples: rows,
    errors: [],
    statusCodes: [{ code: 200, count: total }],
    errorTypes: [],
    histogram: [
      { label: '0-10ms', minMs: 0, maxMs: 10, count: 1000 },
      { label: '10-25ms', minMs: 10, maxMs: 25, count: 8000 },
      { label: '25-50ms', minMs: 25, maxMs: 50, count: 1000 },
    ],
    stages: [],
    config: { mode: 'fixed', targetRps: 5000, rampUpSec: 5 },
  }
}

describe('gradeReport', () => {
  it('SLA 未达标判定为未通过', () => {
    const detail = makeDetail({ p95Rt: 120 })
    detail.config = { ...detail.config, sla: { p95Ms: 50 } }
    const grade = gradeReport(detail)
    expect(grade.grade).toBe('fail')
    expect(grade.reason).toContain('P95')
  })

  it('错误率超过 5% 直接未通过', () => {
    expect(gradeReport(makeDetail({ errorRate: 8 })).grade).toBe('fail')
  })

  it('指标健康时通过', () => {
    expect(gradeReport(makeDetail()).grade).toBe('pass')
  })

  it('尾部延迟显著放大时降级为有条件通过', () => {
    const grade = gradeReport(makeDetail({ p99Rt: 90, avgRt: 20 }))
    expect(grade.grade).toBe('conditional')
    expect(grade.reason).toContain('P99')
  })
})

describe('latencyDrift', () => {
  it('后半程变慢时返回正漂移', () => {
    const detail = makeDetail({}, (index) => ({ avgRt: index > 60 ? 40 : 20 }))
    expect(latencyDrift(detail)).toBeCloseTo(1, 1)
  })

  it('平稳时漂移接近 0', () => {
    expect(Math.abs(latencyDrift(makeDetail()))).toBeLessThan(0.01)
  })
})

describe('buildFindings', () => {
  it('零错误时给出正向结论', () => {
    const titles = buildFindings(makeDetail()).map((item) => item.title)
    expect(titles.some((title) => title.includes('零错误'))).toBe(true)
  })

  it('错误率高时排在最前', () => {
    const findings = buildFindings(makeDetail({ errorRate: 9, errorCount: 900 }))
    expect(findings[0].level).toBe('bad')
  })

  it('最多返回 6 条', () => {
    const detail = makeDetail({ errorRate: 12, p99Rt: 200, avgRt: 20 }, (index) => ({
      avgRt: index > 60 ? 60 : 20,
      errorCount: index > 40 ? 10 : 0,
    }))
    expect(buildFindings(detail).length).toBeLessThanOrEqual(6)
  })
})

describe('buildAdvice', () => {
  it('错误率高时输出 P0 建议', () => {
    const advice = buildAdvice(makeDetail({ errorRate: 6, errorCount: 600 }))
    expect(advice[0].priority).toBe('P0')
  })

  it('始终以环境复测建议收尾', () => {
    const advice = buildAdvice(makeDetail())
    expect(advice[advice.length - 1].title).toContain('复测')
  })
})

describe('cdfPoints', () => {
  it('累积占比收敛到 100%', () => {
    const points = cdfPoints(makeDetail())
    expect(points[0]).toEqual({ ms: 0, pct: 0 })
    expect(points[points.length - 1].pct).toBeCloseTo(100, 5)
  })

  it('无直方图时返回空数组', () => {
    const detail = makeDetail()
    detail.histogram = []
    expect(cdfPoints(detail)).toEqual([])
  })
})
