import { describe, expect, it } from 'vitest'
import type { ReportDetail, ReportRequestRecord, ReportSample } from '@/types/models'
import type { ClusterReportMeta, ClusterReportNode } from '@/types/cluster'
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
      metricsPeaks: {
        qps: { value: 5030, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
        concurrent: { value: 200, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
        bandwidth: { value: 2_048_000, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
        p95: { value: 52, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
        p99: { value: 96, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
        errorRate: { value: 0.4, atMs: 1_700_000_030_000, second: 30, heldSec: 5 },
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
      '结果汇总',
      '测试环境与配置',
      '吞吐与并发',
      '响应时间分析',
      '错误与状态码',
      'URL 与施压机',
      '指标口径定义',
      '数据完整性与自洽校验',
      '明细数据与导出',
    ]
    sections.forEach((title) => expect(html).toContain(title))
    // 导出报告只呈现事实数据，不再输出主观分析与建议
    expect(html).not.toContain('关键发现')
    expect(html).not.toContain('优化建议')
    expect(html).not.toContain('容量评估与瓶颈定位')
    expect(html).not.toContain('结论与优化建议')
    expect(html).not.toContain('综合体检')
  })

  it('图表带完整坐标轴与参考线', () => {
    // 时间轴使用报告开始时间换算出的完整日期时间，并旋转标签避免重叠
    expect(html).toMatch(
      /rotate\(-35 [\d.]+ [\d.]+\)" font-size="9.5"[^>]*>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/,
    )
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

  it('结果汇总包含完整 QPS 指标组', () => {
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

  it('峰值 QPS 独立成卡并标注出现时刻', () => {
    expect(html).toContain('class="kpi-k">峰值 QPS')
    expect(html).toContain('第 30 秒 · 平台期 5 秒')
  })

  it('指标总览包含带宽、Body 与丢弃计数', () => {
    expect(html).toContain('接收带宽')
    expect(html).toContain('发送带宽')
    expect(html).toContain('Body 平均')
    expect(html).toContain('Body P95')
    expect(html).toContain('明细 / 错误丢弃')
    // 入 / 出顺序与标签一致：接收 11.72 MB / 发送 878.91 MB
    expect(html).toContain('11.72 MB / 878.91 MB')
  })

  it('补充错误数与并发曲线，且不输出评分建议', () => {
    expect(html).toContain('错误数趋势')
    expect(html).toContain('并发数趋势')
    expect(html).not.toContain('四维等权评分')
    expect(html).not.toContain('P0 阻断上线')
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

describe('buildReportHtml · URL 统计', () => {
  function withUrlStats() {
    const detail = makeDetail(60)
    detail.urlStats = [
      {
        method: 'GET',
        url: 'http://127.0.0.1:8080/api/orders?token=******',
        statusCode: 200,
        requests: 1000,
        successCount: 995,
        errorCount: 5,
        avgMs: 21.5,
        maxMs: 320,
        maxAtMs: 12_345,
        totalHours: 0.006,
        avgKb: 2.4,
        totalMb: 2.3,
        statusSummary: '200×995, 502×5',
        errorMessage: 'HTTP 502',
      },
      {
        method: 'POST',
        url: 'http://127.0.0.1:8080/api/pay',
        statusCode: 500,
        requests: 10,
        successCount: 0,
        errorCount: 10,
        avgMs: 120,
        maxMs: 800,
        maxAtMs: 60_000,
        totalHours: 0.0003,
        avgKb: 1.1,
        totalMb: 0.01,
        statusSummary: '500×10',
        errorMessage: 'internal error',
      },
    ]
    detail.urlStatsComplete = true
    return detail
  }

  it('渲染 URL 聚合表与错误地址表', () => {
    const html = buildReportHtml(withUrlStats(), [])
    expect(html).toContain('按 URL 聚合统计')
    expect(html).toContain('错误地址')
    expect(html).toContain('200×995, 502×5')
    expect(html).toContain('HTTP 502')
    expect(html).toContain('99.50%')
    // 多 URL 时输出平均响应对比图
    expect(html).toContain('URL 平均响应时间对比')
  })

  it('旧报告没有 URL 数据时标注未记录', () => {
    const detail = makeDetail(60)
    detail.urlStats = []
    detail.urlStatsComplete = false
    const html = buildReportHtml(detail, [])
    expect(html).toContain('该报告生成于旧版本，未记录按 URL 统计')
  })

  it('Excel 增加 URL 统计与错误地址工作表', () => {
    const sheets = buildReportSheets(withUrlStats(), [])
    const names = sheets.map((sheet) => sheet.name)
    expect(names).toContain('URL 统计')
    expect(names).toContain('错误地址')

    const urlSheet = sheets.find((sheet) => sheet.name === 'URL 统计')!
    expect(urlSheet.rows[0]).toContain('平均耗时(ms)')
    expect(urlSheet.rows[1]).toContain('http://127.0.0.1:8080/api/orders?token=******')
    expect(urlSheet.rows[1]).toContain(995)

    const errorSheet = sheets.find((sheet) => sheet.name === '错误地址')!
    expect(errorSheet.rows.length).toBe(3)
    expect(errorSheet.rows[1]).toContain(5)
  })
})

describe('接口 QPS 导出', () => {
  function withInterfaces(): ReportDetail {
    const detail = makeDetail(3)
    detail.config = {
      ...(detail.config ?? {}),
      requests: [
        { name: '查询订单', method: 'GET', url: 'https://a.test/orders' },
        { name: '创建订单', method: 'POST', url: 'https://a.test/orders' },
      ],
    } as ReportDetail['config']
    detail.interfaceSamples = [
      { elapsedSec: 1, index: 0, qps: 3000, successCount: 3000, errorCount: 0, avgRt: 18 },
      { elapsedSec: 1, index: 1, qps: 2000, successCount: 2000, errorCount: 0, avgRt: 25 },
      { elapsedSec: 2, index: 0, qps: 3100, successCount: 3100, errorCount: 0, avgRt: 19 },
      { elapsedSec: 2, index: 1, qps: 1900, successCount: 1900, errorCount: 0, avgRt: 26 },
    ]
    detail.interfaceSamplesComplete = true
    return detail
  }

  it('HTML 输出接口 QPS 趋势图', () => {
    const html = buildReportHtml(withInterfaces(), [])
    expect(html).toContain('接口 QPS 趋势')
    expect(html).toContain('查询订单')
    expect(html).toContain('创建订单')
  })

  it('Excel 输出接口采样工作表', () => {
    const sheets = buildReportSheets(withInterfaces(), [])
    const sheet = sheets.find((item) => item.name === '接口采样')
    expect(sheet).toBeTruthy()
    expect(sheet!.rows[0]).toEqual(['秒', '接口', 'QPS', '成功数', '失败数', '平均耗时(ms)'])
    expect(sheet!.rows).toHaveLength(5)
    expect(sheet!.rows[1]).toEqual([1, '查询订单', 3000, 3000, 0, 18])
  })
})

describe('buildReportHtml · 施压机明细', () => {
  function clusterNode(overrides: Partial<ClusterReportNode> = {}): ClusterReportNode {
    return {
      name: 'node-a',
      role: 'slave',
      addr: '10.0.0.2',
      weight: 1,
      concurrent: 10,
      status: 'completed',
      qpsAvg: 100,
      cpuPeak: 1,
      memPeakMb: 128,
      hostCpuPeak: 10,
      hostMemPeakPercent: 20,
      netPeakKbps: 100,
      clockOffsetMs: 0,
      clockUncertaintyMs: 0,
      clockDriftMs: 0,
      droppedSamples: 0,
      startMs: 1_700_000_000_000,
      endMs: 1_700_000_010_000,
      peakQps: 120,
      peakConcurrent: 10,
      share: 1,
      health: { state: 'done', expected: 10, received: 10, lost: 0, gapSec: 0 },
      ...overrides,
    }
  }

  it('单机报告输出本机 1 行', () => {
    const html = buildReportHtml(makeDetail(60), [])
    expect(html).toContain('施压机明细')
    expect(html).toContain('本机')
    expect(html).toContain('bench-01')
    expect(html).toContain('TPS')
    // 300000 请求 / 60 秒 = 5000 TPS
    expect(html).toContain('5,000')
  })

  it('旧节点缺失字段显示「—」', () => {
    const detail = makeDetail(60)
    detail.loadGenerators = [
      clusterNode({
        name: 'node-new',
        totalRequests: 100,
        successCount: 90,
        errorCount: 10,
        avgRtMs: 20,
        tps: 10,
        egressAvgKbps: 5,
      }),
      clusterNode({ name: 'node-old', totalRequests: null, successCount: null, errorCount: null }),
    ]
    const html = buildReportHtml(detail, [])
    expect(html).toContain('node-new')
    expect(html).toContain('node-old')
    expect(html).toContain('90.00%')
    expect(html).toContain('5.0 KB/s')
    expect(html).toContain('—')
  })

  it('Excel 增加施压机详情工作表', () => {
    const sheets = buildReportSheets(makeDetail(60), [])
    const sheet = sheets.find((item) => item.name === '施压机详情')!
    expect(sheet).toBeTruthy()
    expect(sheet.rows[0]).toContain('TPS')
    expect(sheet.rows[0]).toContain('出站带宽平均(KB/s)')
    expect(sheet.rows[1]).toContain('本机')
    // 300000 请求 / 60 秒 = 5000 TPS
    expect(sheet.rows[1]).toContain(5000)
  })
})

describe('buildReportHtml · 标题与免责声明', () => {
  it('优先展示用户标题，为空时回退计划名称', () => {
    const detail = makeDetail(10)
    detail.report.title = '  自定义标题  '
    const html = buildReportHtml(detail, [])
    expect(html).toContain('自定义标题')

    const sheets = buildReportSheets(detail, [])
    const overview = sheets.find((sheet) => sheet.name === '结果汇总')!
    expect(overview.rows[0]).toContain('自定义标题')

    detail.report.title = '   '
    const fallback = buildReportHtml(detail, [])
    expect(fallback).toContain('order-query 压测')
  })

  it('HTML 与 Excel 说明页包含免责声明和脱敏说明', () => {
    const detail = makeDetail(10)
    const html = buildReportHtml(detail, [])
    expect(html).toContain('该报告仅真实体现当时环境下的测试情况')

    const sheets = buildReportSheets(detail, [])
    const notes = sheets.find((sheet) => sheet.name === '说明')!
    expect(notes).toBeTruthy()
    const labels = notes.rows.map((row) => row[0])
    expect(labels).toContain('免责声明')
    expect(labels).toContain('脱敏说明')
    expect(labels).toContain('数据完整性')
  })
})

describe('buildReportHtml · 新增图表', () => {
  it('CDF 忽略零计数与开放区间，避免横轴被压成 0~1', () => {
    const detail = makeDetail(1)
    detail.histogram = [
      { label: '≤10ms', minMs: 0, maxMs: 10, count: 0 },
      { label: '200-350ms', minMs: 200, maxMs: 350, count: 90 },
      { label: '>60s', minMs: 60_000, maxMs: Number.MAX_VALUE, count: 0 },
    ]
    const html = buildReportHtml(detail, [])
    const start = html.indexOf('响应时间累积分布')
    expect(start).toBeGreaterThanOrEqual(0)
    const figure = html.slice(start, start + 6000)
    expect(figure).toContain('<polyline')
    expect(figure).toContain('P50')
    expect(figure).not.toContain('NaN')
    expect(figure).not.toContain('e+308')
  })

  it('CDF 对 >60s 计数使用 60s 作为可读上界', () => {
    const detail = makeDetail(1)
    detail.histogram = [
      { label: '200-350ms', minMs: 200, maxMs: 350, count: 90 },
      { label: '>60s', minMs: 60_000, maxMs: Number.MAX_VALUE, count: 10 },
    ]
    const html = buildReportHtml(detail, [])
    const start = html.indexOf('响应时间累积分布')
    const figure = html.slice(start, start + 6000)
    expect(figure).toContain('<polyline')
    expect(figure).not.toContain('NaN')
    expect(figure).not.toContain('e+308')
  })

  it('输出 QPS-延迟散点与 TTFB/Body 趋势', () => {
    const html = buildReportHtml(makeDetail(30), [])
    expect(html).toContain('<span class="fig-t">平均响应时间</span>')
    expect(html).toContain('QPS 与平均响应时间')
    expect(html).toContain('TTFB 与响应体读取耗时趋势')
  })

  it('输出构成饼图与阶段对比图', () => {
    const detail = makeDetail(3)
    detail.stages = [
      {
        index: 0,
        concurrent: 100,
        durationSec: 10,
        requests: 1000,
        errors: 0,
        avgQps: 100,
        avgRt: 20,
        p95Rt: 40,
      },
      {
        index: 1,
        concurrent: 200,
        durationSec: 10,
        requests: 1800,
        errors: 5,
        avgQps: 180,
        avgRt: 30,
        p95Rt: 60,
      },
    ]
    const html = buildReportHtml(detail, [])
    expect(html).toContain('请求结果构成')
    expect(html).toContain('状态码分布')
    expect(html).toContain('错误类型分布')
    expect(html).toContain('流量构成')
    expect(html).toContain('class="pie-grid"')
    expect(html.match(/class="pie-card"/g)?.length).toBe(4)
    expect(html).toContain('阶段平均 QPS 对比')
    expect(html).toContain('阶段 P95 响应时间对比')
    expect(html).not.toContain('NaN')
  })

  it('多施压机时输出 TPS 对比图', () => {
    const detail = makeDetail(10)
    detail.loadGenerators = [
      {
        name: 'master-a',
        role: 'master',
        addr: '10.0.0.1',
        weight: 1,
        concurrent: 10,
        status: 'completed',
        qpsAvg: 100,
        cpuPeak: 1,
        memPeakMb: 100,
        hostCpuPeak: 10,
        hostMemPeakPercent: 20,
        netPeakKbps: 100,
        clockOffsetMs: 0,
        clockUncertaintyMs: 0,
        clockDriftMs: 0,
        droppedSamples: 0,
        startMs: 1_700_000_000_000,
        endMs: 1_700_000_010_000,
        peakQps: 120,
        peakConcurrent: 10,
        share: 0.5,
        health: { state: 'done', expected: 10, received: 10, lost: 0, gapSec: 0 },
        totalRequests: 1000,
        successCount: 990,
        errorCount: 10,
        avgRtMs: 20,
        tps: 100,
        egressAvgKbps: 5,
      },
      {
        name: 'slave-b',
        role: 'slave',
        addr: '10.0.0.2',
        weight: 1,
        concurrent: 10,
        status: 'completed',
        qpsAvg: 90,
        cpuPeak: 1,
        memPeakMb: 100,
        hostCpuPeak: 10,
        hostMemPeakPercent: 20,
        netPeakKbps: 100,
        clockOffsetMs: 0,
        clockUncertaintyMs: 0,
        clockDriftMs: 0,
        droppedSamples: 0,
        startMs: 1_700_000_000_000,
        endMs: 1_700_000_010_000,
        peakQps: 110,
        peakConcurrent: 10,
        share: 0.5,
        health: { state: 'done', expected: 10, received: 10, lost: 0, gapSec: 0 },
        totalRequests: 900,
        successCount: 900,
        errorCount: 0,
        avgRtMs: 22,
        tps: 90,
        egressAvgKbps: 4,
      },
    ]
    const html = buildReportHtml(detail, [])
    expect(html).toContain('施压机 TPS 对比')
    expect(html).toContain('master-a')
    expect(html).toContain('slave-b')
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

describe('buildReportHtml · 分布式压测章节', () => {
  function makeClusterDetail(nodeSeries: ClusterReportMeta['nodeSeries']): ReportDetail {
    const detail = makeDetail(60)
    const baseConfig = detail.config!
    detail.config = {
      ...baseConfig,
      mode: 'cluster',
      cluster: {
        protocolVersion: 1,
        masterNode: 'master-a',
        totalConcurrent: 400,
        incomplete: false,
        startedAt: 1_700_000_000_000,
        finishedAt: 1_700_000_060_000,
        droppedSamples: 2,
        nodes: [
          {
            name: 'master-a',
            role: 'master',
            addr: '10.0.0.1:17801',
            weight: 1,
            concurrent: 200,
            status: 'completed',
            qpsAvg: 5000,
            cpuPeak: 3.2,
            memPeakMb: 860,
            hostCpuPeak: 70,
            hostMemPeakPercent: 60,
            netPeakKbps: 1024,
            clockOffsetMs: 1,
            clockUncertaintyMs: 0.5,
            clockDriftMs: 0.2,
            droppedSamples: 0,
            startMs: 1_700_000_000_000,
            endMs: 1_700_000_060_000,
            peakQps: 5030,
            peakConcurrent: 200,
            share: 0.5,
            health: { state: 'ok', expected: 60, received: 60, lost: 0, gapSec: 0 },
          },
          {
            name: 'slave-b',
            role: 'slave',
            addr: '10.0.0.2:17801',
            weight: 1,
            concurrent: 200,
            status: 'completed',
            qpsAvg: 4800,
            cpuPeak: 2.8,
            memPeakMb: 720,
            hostCpuPeak: 65,
            hostMemPeakPercent: 55,
            netPeakKbps: 980,
            clockOffsetMs: -1,
            clockUncertaintyMs: 0.4,
            clockDriftMs: -0.3,
            droppedSamples: 2,
            startMs: 1_700_000_000_000,
            endMs: 1_700_000_060_000,
            peakQps: 4900,
            peakConcurrent: 200,
            share: 0.5,
            health: { state: 'gap', expected: 60, received: 58, lost: 2, gapSec: 2 },
          },
        ],
        nodeSeries,
        peaks: baseConfig.metricsPeaks,
      },
    }
    return detail
  }

  const baseSeries: ClusterReportMeta['nodeSeries'] = [
    {
      node: 'master-a',
      role: 'master',
      points: [
        { sec: 1, qps: 4000, concurrent: 200, p95: 40, errors: 0 },
        { sec: 2, qps: 5030, concurrent: 200, p95: 52, errors: 0 },
      ],
    },
    {
      node: 'slave-b',
      role: 'slave',
      points: [
        { sec: 1, qps: 3800, concurrent: 200, p95: 44, errors: 0 },
        { sec: 2, qps: 4900, concurrent: 200, p95: 58, errors: 1 },
      ],
    },
  ]

  it('集群报告导出概览、峰值专题、节点贡献与基础曲线', () => {
    const html = buildReportHtml(makeClusterDetail(baseSeries), requests)
    expect(html).toContain('分布式压测')
    expect(html).toContain('集群总 QPS')
    expect(html).toContain('集群峰值专题')
    expect(html).toContain('节点贡献')
    expect(html).toContain('master-a')
    expect(html).toContain('slave-b')
    expect(html).toContain('采样缺口')
    // 集群报告的压测模式不再落成固定并发/原始 cluster
    expect(html).toContain('分布式压测（固定）')
    // 基础三条曲线始终导出
    expect(html).toContain('节点 QPS 曲线')
    expect(html).toContain('节点并发曲线')
    expect(html).toContain('节点 P95 曲线')
    // 未采集整机资源时不画全 0 曲线
    expect(html).not.toContain('节点整机 CPU 曲线')
    expect(html).not.toContain('节点进程内存曲线')
    expect(html).not.toContain('节点网络曲线')
  })

  it('采集到整机资源时补出 CPU / 内存 / 网络曲线', () => {
    const withHost = baseSeries.map((item) => ({
      ...item,
      points: item.points.map((point) => ({
        ...point,
        hostCpuPercent: 60,
        memMb: 720,
        netRxKbps: 500,
        netTxKbps: 400,
      })),
    }))
    const html = buildReportHtml(makeClusterDetail(withHost), requests)
    expect(html).toContain('节点整机 CPU 曲线')
    expect(html).toContain('节点进程内存曲线')
    expect(html).toContain('节点网络曲线')
  })

  it('节点采样按秒并集对齐，晚加入节点不会错位', () => {
    const skewed: ClusterReportMeta['nodeSeries'] = [
      {
        node: 'master-a',
        role: 'master',
        points: [
          { sec: 2, qps: 5030, concurrent: 200, p95: 52, errors: 0 },
          { sec: 3, qps: 6000, concurrent: 200, p95: 60, errors: 0 },
        ],
      },
      {
        node: 'slave-b',
        role: 'slave',
        points: [
          { sec: 1, qps: 3800, concurrent: 200, p95: 44, errors: 0 },
          { sec: 2, qps: 4900, concurrent: 200, p95: 58, errors: 1 },
        ],
      },
    ]
    const sheets = buildReportSheets(makeClusterDetail(skewed), requests)
    const sheet = sheets.find((item) => item.name === '节点采样')
    expect(sheet?.rows[1]?.slice(0, 3)).toEqual(['秒', 'master-a QPS', 'slave-b QPS'])
    const row = (sec: number) => sheet?.rows.find((item) => item[0] === sec)
    expect(row(1)?.[1]).toBeNull()
    expect(row(1)?.[2]).toBe(3800)
    expect(row(2)?.[1]).toBe(5030)
    expect(row(2)?.[2]).toBe(4900)
    expect(row(3)?.[1]).toBe(6000)
    expect(row(3)?.[2]).toBeNull()
  })
})

describe('buildReportHtml · 基线对比', () => {
  function makeBaseline(): ReportDetail {
    const base = makeDetail(30)
    base.report.id = 3
    base.report.planName = '上周基线'
    base.report.avgQps = 4000
    base.report.p95Rt = 30
    base.report.p99Rt = 60
    base.report.avgRt = 15
    base.report.errorRate = 0.2
    return base
  }

  it('提供基线时输出本次 / 基线与变化', () => {
    const html = buildReportHtml(makeDetail(60), requests, { baseline: makeBaseline() })
    expect(html).toContain('基线对比')
    expect(html).toContain('上周基线')
    expect(html).toContain('平均 QPS')
    // 本次 5000 相对基线 4000 提升 25%
    expect(html).toContain('+25.0%')
  })

  it('未提供基线时不输出对比区块', () => {
    expect(buildReportHtml(makeDetail(60), requests)).not.toContain('基线对比')
  })

  it('基线指向当前报告自身时跳过，避免自我对比', () => {
    const current = makeDetail(60)
    const same = makeDetail(60)
    expect(buildReportHtml(current, requests, { baseline: same })).not.toContain('基线对比')
  })
})

describe('buildReportSheets', () => {
  it('包含结果汇总与配置快照工作表，且不再生成优化建议页', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const names = sheets.map((sheet) => sheet.name)
    expect(names).toContain('结果汇总')
    expect(names).toContain('配置快照')
    expect(names).not.toContain('优化建议')
  })

  it('结果汇总页与采样表包含 QPS 指标组', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const overview = sheets.find((sheet) => sheet.name === '结果汇总')
    const labels = (overview?.rows ?? []).map((row) => row[0])
    expect(labels).toContain('平均 QPS')
    expect(labels).toContain('峰值 QPS')
    expect(labels).toContain('90% 分布 QPS')
    expect(labels).toContain('单并发 QPS')
    expect(labels).toContain('目标 QPS')
    expect(labels).toContain('目标达成率(%)')
    expect(labels).toContain('峰值 QPS 出现秒')
    expect(labels).toContain('接收流量(B)')
    expect(labels).toContain('发送流量(B)')
    expect(labels).toContain('明细丢弃(条)')

    const value = (label: string) =>
      overview?.rows.find((row) => row[0] === label)?.[1]
    expect(value('平均 QPS')).toBe(5000)
    expect(value('峰值 QPS')).toBe(5030)
    expect(value('峰值 QPS 出现秒')).toBe(30)
    expect(value('单并发 QPS')).toBe(25)
    // 目标 5000 ÷ 平均 5000 = 100%
    expect(value('目标达成率(%)')).toBe(100)

    const sampleSheet = sheets.find((sheet) => sheet.name === '采样明细')
    expect(sampleSheet?.rows[0]).toContain('QPS')
    expect(sampleSheet?.rows[0]).toContain('Body 平均(ms)')
    const percentileSheet = sheets.find((sheet) => sheet.name === '延迟分位')
    expect(percentileSheet?.rows.map((row) => row[0])).toContain('Body P95')
    const stageSheet = sheets.find((sheet) => sheet.name === '分阶段汇总')
    expect(stageSheet?.rows[0]).toContain('平均 QPS')
  })

  it('提供基线时生成基线对比工作表', () => {
    const base = makeDetail(30)
    base.report.id = 3
    base.report.planName = '上周基线'
    base.report.avgQps = 4000
    const sheets = buildReportSheets(makeDetail(60), requests, { baseline: base })
    const sheet = sheets.find((item) => item.name === '基线对比')
    expect(sheet).toBeTruthy()
    const labels = (sheet?.rows ?? []).map((row) => row[0])
    expect(labels).toContain('平均 QPS')
    expect(labels).toContain('P95 延迟')
  })

  it('压测模式显示为中文而不是原始枚举', () => {
    const sheets = buildReportSheets(makeDetail(60), requests)
    const snapshot = sheets.find((sheet) => sheet.name === '配置快照')
    const modeRow = snapshot?.rows.find((row) => row[0] === '压测模式')
    expect(modeRow?.[1]).toBe('固定并发')
  })

  it('集群报告的压测模式导出为「分布式压测」而不是 cluster', () => {
    const detail = makeDetail(60)
    detail.config = { ...detail.config!, mode: 'cluster', stages: [] }
    const sheets = buildReportSheets(detail, requests)
    const snapshot = sheets.find((sheet) => sheet.name === '配置快照')
    const modeRow = snapshot?.rows.find((row) => row[0] === '压测模式')
    expect(modeRow?.[1]).toBe('分布式压测（固定）')
  })

  it('后端生成 Excel 时请求/采样明细保留空占位，顺序不变', () => {
    const sheets = buildReportSheets(makeDetail(60), requests, {
      includeRequestRows: false,
      includeSampleRows: false,
    })
    const names = sheets.map((sheet) => sheet.name)
    const sampleIndex = names.indexOf('采样明细')
    const requestIndex = names.indexOf('请求明细')
    expect(sampleIndex).toBeGreaterThan(0)
    expect(requestIndex).toBe(sampleIndex + 1)
    expect(sheets[sampleIndex]?.rows).toEqual([])
    expect(sheets[requestIndex]?.rows).toEqual([])
  })
})
