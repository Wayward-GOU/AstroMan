import { describe, expect, it } from 'vitest'
import type { ReportDetail, ReportRequestRecord, ReportSample } from '@/types/models'
import { buildReportHtml, buildReportSheets } from './reportExport'

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
      id: 7,
      planName: 'order-query 压测',
      totalRequests: 5000 * sampleCount,
      successCount: 5000 * sampleCount - 5,
      errorCount: 5,
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
      bytesTotal: 1024 * 200 * sampleCount,
      maxQps: 5030,
      avgQps: 5000,
      targetRps: 5000,
      errorRate: 0.1,
      concurrency: 200,
      status: 'completed',
      startTime: 1_700_000_000_000,
      endTime: 1_700_000_000_000 + sampleCount * 1000,
      createdAt: 1_700_000_000_000,
    },
    samples,
    errors: [
      {
        elapsedSec: 10,
        errorType: 'HTTP',
        errorMessage: 'HTTP 502',
        requestUrl: 'http://127.0.0.1:8080/api/orders',
        responseStatus: 502,
        responseBody: 'bad gateway',
      },
    ],
    statusCodes: [
      { code: 200, count: 5000 * sampleCount - 5 },
      { code: 502, count: 5 },
    ],
    errorTypes: [{ errorType: 'HTTP', count: 5 }],
    histogram: [{ label: '10-25ms', minMs: 10, maxMs: 25, count: 5000 * sampleCount }],
    stages: [],
    config: {
      mode: 'fixed',
      method: 'GET',
      url: 'http://127.0.0.1:8080/api/orders',
      timeoutMs: 30000,
      thinkTimeMs: 0,
      targetRps: 5000,
      rampUpSec: 5,
      recordDetail: 'sample',
      recordDetailRequested: 'auto',
      agent: {
        tool: 'AstroMan',
        version: '1.0.8',
        os: 'Windows',
        osVersion: '11',
        arch: 'x86_64',
        host: 'bench-01',
        cpuCores: 8,
        memTotalMb: 32768,
      },
      metrics: {
        bytesSent: 1024 * 3 * sampleCount * 5000,
        errorsDropped: 0,
        recordsDropped: 12,
        recordMode: 'sample',
        cpuPeakCores: 3.2,
        memPeakMb: 860,
        sampleIntervalSec: 1,
      },
    },
    scenarioText: '订单查询接口容量摸底，预热 5 秒后以 200 并发压测。',
    summaryText: '整体平稳，P99 在 80ms 内。',
  }
}

const requests: ReportRequestRecord[] = [
  {
    elapsedMs: 12345,
    timeMs: 1_700_000_012_345,
    method: 'GET',
    url: 'http://127.0.0.1:8080/api/orders?id=1',
    status: 200,
    latencyMs: 320,
    ttfbMs: 120,
    bytes: 2048,
    success: true,
    body: null,
    params: 'id=1',
    headers: null,
    responseHeaders: null,
    responseBody: '{"ok":true}',
    errorType: null,
    errorMessage: null,
  },
]

describe('buildReportHtml · 正式报告结构', () => {
  const html = buildReportHtml(makeDetail(60), requests)

  it('包含封面与目录', () => {
    expect(html).toContain('Performance Test Report')
    expect(html).toContain('报告编号 R-7')
    expect(html).toContain('Contents')
  })

  it('章节按编号完整呈现', () => {
    const sections = [
      '执行摘要',
      '测试环境与配置',
      '吞吐量与稳定性',
      '响应时间分析',
      '错误分析',
      '容量评估与瓶颈定位',
      '结论与优化建议',
      '指标口径定义',
      '数据完整性与自洽校验',
      '明细数据与导出',
    ]
    sections.forEach((title) => expect(html).toContain(title))
  })

  it('图表带完整坐标轴与参考线', () => {
    // 时间轴标签
    expect(html).toContain('0s')
    // 目标 RPS 参考线
    expect(html).toContain('目标 5,000 req/s')
    // 升压期高亮
    expect(html).toContain('升压期')
    // 累积分布图
    expect(html).toContain('响应时间累积分布')
  })

  it('不再内嵌三张明细表', () => {
    expect(html).not.toContain('<th>采样明细</th>')
    expect(html).not.toContain('>请求明细<')
    expect(html).not.toContain('>错误明细<')
    expect(html).toContain('明细数据与导出')
  })

  it('执行摘要包含完整 QPS 指标组', () => {
    expect(html).toContain('平均 QPS')
    expect(html).toContain('90% 分布 QPS')
    expect(html).toContain('单并发 QPS')
    // 平均 5000 / 并发 200 = 25
    expect(html).toContain('25.00')
    // 目标 5000 / 平均 5000 = 100%
    expect(html).toContain('5,000 / 100.0%')
    // 口径定义同步给出单并发 QPS 的解释
    expect(html).toContain('单连接吞吐效率')
  })

  it('自动生成的结论、发现与应用建议非空', () => {
    expect(html).toContain('关键发现')
    expect(html).toContain('优化建议')
    expect(html).toContain('P0')
    expect(html).toContain('P2')
    // 存在错误时应给出错误相关发现
    expect(html).toContain('错误率')
  })

  it('输出不含未定义值', () => {
    expect(html).not.toContain('undefined')
    expect(html).not.toContain('NaN')
    expect(html).not.toContain('[object Object]')
  })
})

describe('buildReportHtml · 施压端能力与长 URL', () => {
  it('未检测本机能力时明确标注，不推断数值', () => {
    const html = buildReportHtml(makeDetail(60), requests)
    expect(html).toContain('未检测本机施压能力')
  })

  it('已检测时展示客户端极限与负载占比（3.7.2）', () => {
    const detail = makeDetail(60)
    detail.config = {
      ...detail.config,
      machineCapacity: { maxQps: 8000, reachedLimit: true, cores: 8, cpuPeak: 6.4, probedAt: 1_700_000_000_000 },
    }
    const html = buildReportHtml(detail, requests)
    expect(html).toContain('客户端极限')
    expect(html).toContain('8,000 QPS')
    expect(html).toContain('客户端负载占比')
    expect(html).toContain('回环检测值')
  })

  it('长 URL 完整保留，不截断（3.2.2 / 7.4）', () => {
    const longUrl = `http://127.0.0.1:8080/api/orders?${Array.from(
      { length: 12 },
      (_, index) => `param${index}=value${index}`,
    ).join('&')}`
    const detail = makeDetail(60)
    detail.config = { ...detail.config, url: longUrl }
    const html = buildReportHtml(detail, requests)
    // HTML 会转义 &，但只要未截断，转义后的完整地址仍应出现
    expect(html).toContain(longUrl.replace(/&/g, '&amp;'))
  })
})

describe('buildReportHtml · 体积控制', () => {
  it('报告体积与压测时长脱钩', () => {
    const short = buildReportHtml(makeDetail(60), [])
    // 模拟 8 小时：28800 个采样点
    const long = buildReportHtml(makeDetail(28_800), [])
    expect(short.length).toBeLessThan(140_000)
    expect(long.length).toBeLessThan(400_000)
    expect(long.length / short.length).toBeLessThan(3)
  })
})

describe('buildReportSheets', () => {
  it('包含优化建议与配置快照工作表', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const names = sheets.map((sheet) => sheet.name)
    expect(names).toContain('优化建议')
    expect(names).toContain('配置快照')
    expect(names).toContain('概览')
  })

  it('概览页与采样表包含 QPS 指标组', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const overview = sheets.find((sheet) => sheet.name === '概览')
    const labels = (overview?.rows ?? []).map((row) => row[0])
    expect(labels).toContain('平均 QPS')
    expect(labels).toContain('峰值 QPS')
    expect(labels).toContain('90% 分布 QPS')
    expect(labels).toContain('单并发 QPS')
    expect(labels).toContain('目标 QPS')
    expect(labels).toContain('目标达成率(%)')

    const value = (label: string) =>
      overview?.rows.find((row) => row[0] === label)?.[1]
    expect(value('平均 QPS')).toBe(5000)
    expect(value('单并发 QPS')).toBe(25)
    // 目标 5000 ÷ 平均 5000 = 100%
    expect(value('目标达成率(%)')).toBe(100)

    const sampleSheet = sheets.find((sheet) => sheet.name === '采样明细')
    expect(sampleSheet?.rows[0]).toContain('QPS')
    const stageSheet = sheets.find((sheet) => sheet.name === '分阶段汇总')
    expect(stageSheet?.rows[0]).toContain('平均 QPS')
  })

  it('压测模式显示为中文而不是原始枚举', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const snapshot = sheets.find((sheet) => sheet.name === '配置快照')
    const modeRow = snapshot?.rows.find((row) => row[0] === '压测模式')
    expect(modeRow?.[1]).toBe('固定并发')
  })
})
