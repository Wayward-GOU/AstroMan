import { describe, expect, it } from 'vitest'
import type { ReportDetail, ReportSample } from '@/types/models'
import { buildClassicReportHtml } from './reportClassic'

function makeSamples(count: number): ReportSample[] {
  return Array.from({ length: count }, (_, index) => ({
    elapsedSec: index + 1,
    concurrent: 200,
    qps: 5000 + (index % 30),
    avgRt: 20,
    p50Rt: 18,
    p95Rt: 40,
    p99Rt: 80,
    ttfbAvg: 12,
    bodyAvg: 8,
    bodyP95: 10,
    bytes: 1024 * 200,
    successCount: 5000,
    errorCount: index % 10 === 0 ? 5 : 0,
  }))
}

function makeDetail(sampleCount: number): ReportDetail {
  const samples = makeSamples(sampleCount)
  return {
    report: {
      id: 20260131000700,
      planName: 'order-query 压测',
      title: '订单查询容量摸底',
      totalRequests: 5000 * sampleCount,
      successCount: 5000 * sampleCount,
      errorCount: 0,
      avgRt: 20,
      minRt: 3,
      maxRt: 320,
      p50Rt: 18,
      p75Rt: 22,
      p90Rt: 30,
      p95Rt: 40,
      p99Rt: 80,
      p999Rt: 120,
      stddevRt: 6,
      ttfbAvg: 12,
      ttfbP95: 25,
      apdex: 0.96,
      bytesTotal: 1024 * 1024 * 120,
      maxQps: 5030,
      avgQps: 5000,
      targetRps: 5000,
      errorRate: 0,
      concurrency: 200,
      status: 'completed',
      startTime: 1_700_000_000_000,
      endTime: 1_700_000_000_000 + sampleCount * 1000,
      createdAt: 1_700_000_000_000,
    },
    samples,
    errors: [],
    statusCodes: [{ code: 200, count: 5000 * sampleCount }],
    errorTypes: [],
    histogram: [],
    stages: [],
    config: {
      mode: 'fixed',
      method: 'GET',
      url: 'http://127.0.0.1:8080/api/orders',
      thinkTimeMs: 1000,
      rampUpSec: 5,
      recordDetail: 'sample',
      agent: {
        tool: 'AstroMan',
        version: '1.1.6',
        os: 'Windows',
        osVersion: '11',
        arch: 'x86_64',
        host: 'bench-01',
        cpuCores: 8,
        memTotalMb: 32768,
      },
    },
    scenarioText: '订单查询接口容量摸底。',
    summaryText: '整体平稳，P99 在 80ms 内。',
    urlStats: [
      {
        method: 'GET',
        url: 'http://127.0.0.1:8080/api/orders',
        statusCode: 200,
        requests: 5000 * sampleCount,
        successCount: 5000 * sampleCount,
        errorCount: 0,
        avgMs: 20,
        maxMs: 70,
        maxAtMs: 1_700_000_005_000,
        totalHours: 1.2,
        avgKb: 0.2,
        totalMb: 120,
      },
    ],
  }
}

describe('buildClassicReportHtml · 经典报告版式', () => {
  const html = buildClassicReportHtml(makeDetail(60))

  it('保留经典报告的区块顺序与文案', () => {
    const sections = [
      '测试场景',
      '工作模式',
      '结果汇总',
      '统计分析',
      '响应时间&amp;状态分布',
      '测试机(客户端)详情',
      '测试地址详情',
      '报告总结',
    ]
    let cursor = -1
    for (const name of sections) {
      const index = html.indexOf(name)
      expect(index, `缺少区块 ${name}`).toBeGreaterThan(-1)
      expect(index, `区块 ${name} 顺序不符`).toBeGreaterThan(cursor)
      cursor = index
    }
  })

  it('沿用经典报告的表头与灰底表头样式', () => {
    expect(html).toContain('background:#C0C0C0')
    expect(html).toContain('最长耗时请求ID')
    expect(html).toContain('事务数/秒(TPS)')
    expect(html).toContain('数据包总计(MB)')
    expect(html).toContain('width:1600px')
  })

  it('结果汇总使用经典配色与口径', () => {
    // 30 万次请求 = 30.00 (万)个
    expect(html).toContain('30.00')
    expect(html).toContain('(万)个')
    expect(html).toContain('#008B8B')
    expect(html).toContain('#FF9D00')
    expect(html).toContain('#20B2AA')
    expect(html).toContain('5030')
  })

  it('四张统计图与双图区块都生成内联 SVG', () => {
    expect(html).toContain('成功QPS(每秒查询率)')
    expect(html).toContain('失败QPS(每秒查询率)')
    expect(html).toContain('并发请求数')
    expect(html).toContain('平均响应时间')
    expect(html).toContain('耗时Top10')
    expect(html).toContain('http请求状态分布')
    expect(html).toContain('<svg')
  })

  it('无地址统计与错误数据时给出占位且不渲染错误区块', () => {
    const detail = makeDetail(10)
    detail.urlStats = []
    const output = buildClassicReportHtml(detail)
    expect(output).toContain('暂无地址统计')
    expect(output).not.toContain('错误的测试地址详情')
  })

  it('有错误地址时渲染错误明细表', () => {
    const detail = makeDetail(10)
    detail.urlStats = [
      {
        method: 'GET',
        url: 'http://127.0.0.1:8080/api/orders',
        statusCode: 502,
        requests: 100,
        successCount: 90,
        errorCount: 10,
        avgMs: 30,
        maxMs: 70,
        maxAtMs: 1_700_000_005_000,
        totalHours: 0.1,
        avgKb: 0.2,
        totalMb: 0.1,
        errorMessage: 'HTTP 502',
      },
    ]
    const output = buildClassicReportHtml(detail)
    expect(output).toContain('错误的测试地址详情')
    expect(output).toContain('HTTP 502')
    expect(output).toContain('502')
  })

  it('多接口报告渲染接口 QPS 曲线图例', () => {
    const detail = makeDetail(5)
    detail.config = {
      ...(detail.config ?? {}),
      requests: [
        { name: '查询订单', method: 'GET', url: 'http://127.0.0.1:8080/api/orders' },
        { name: '创建订单', method: 'POST', url: 'http://127.0.0.1:8080/api/orders' },
      ],
    } as ReportDetail['config']
    detail.interfaceSamples = detail.samples.flatMap((sample) => [
      {
        elapsedSec: sample.elapsedSec,
        index: 0,
        qps: sample.qps,
        successCount: sample.successCount,
        errorCount: 0,
        avgRt: sample.avgRt,
      },
      {
        elapsedSec: sample.elapsedSec,
        index: 1,
        qps: Math.round(sample.qps / 2),
        successCount: sample.successCount,
        errorCount: 0,
        avgRt: sample.avgRt,
      },
    ])
    detail.interfaceSamplesComplete = true
    const output = buildClassicReportHtml(detail)
    expect(output).toContain('ifaceEChart')
    expect(output).toContain('cl-legend')
    expect(output).toContain('查询订单')
    expect(output).toContain('创建订单')
  })

  it('包含标题与免责声明', () => {
    expect(html).toContain('订单查询容量摸底')
    expect(html).toContain('免责声明')
    expect(html).toContain('本软件不承担任何法律责任')
  })
})
