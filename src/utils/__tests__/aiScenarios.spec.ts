import { describe, expect, it } from 'vitest'
import { captureFlowPrompt, reportReviewPrompt } from '@/utils/aiScenarios'
import type { CaptureFlowDetail } from '@/types/capture'
import type { ReportDetail } from '@/types/models'
import type { ClusterReportNode } from '@/types/cluster'

function detail(overrides: Partial<CaptureFlowDetail> = {}): CaptureFlowDetail {
  return {
    id: 1,
    startedAt: 1_700_000_000_000,
    method: 'POST',
    scheme: 'https',
    host: 'api.example.com',
    path: '/login',
    httpVersion: 'HTTP/1.1',
    status: 200,
    durationMs: 128,
    requestSize: 64,
    responseSize: 2,
    contentType: 'application/json',
    clientAddr: '127.0.0.1:50000',
    error: null,
    mapped: false,
    modified: false,
    remote: null,
    requestHeaders: [
      { name: 'Authorization', value: 'Bearer abcdefghijklmn' },
      { name: 'X-Trace-Id', value: 'trace-123' },
    ],
    responseHeaders: [{ name: 'Set-Cookie', value: 'sid=topsecret' }],
    requestBody: {
      size: 42,
      truncated: false,
      binary: false,
      text: '{"username":"demo","password":"pass-123456"}',
      base64: null,
      contentType: 'application/json',
    },
    responseBody: {
      size: 2,
      truncated: false,
      binary: false,
      text: '{}',
      base64: null,
      contentType: 'application/json',
    },
    ...overrides,
  }
}

describe('captureFlowPrompt', () => {
  it('包含定位流量的关键字段', () => {
    const prompt = captureFlowPrompt(detail())
    expect(prompt).toContain('POST')
    expect(prompt).toContain('https://api.example.com/login')
    expect(prompt).toContain('状态码：200')
    expect(prompt).toContain('耗时：128 ms')
  })

  it('鉴权头与正文中的密码不会明文进入提示词', () => {
    const prompt = captureFlowPrompt(detail())
    expect(prompt).not.toContain('abcdefghijklmn')
    expect(prompt).not.toContain('pass-123456')
    expect(prompt).not.toContain('topsecret')
    expect(prompt).toContain('trace-123')
  })

  it('二进制正文只说明大小，不输出内容', () => {
    const prompt = captureFlowPrompt(
      detail({
        responseBody: {
          size: 2048,
          truncated: false,
          binary: true,
          text: null,
          base64: 'aGVsbG8=',
          contentType: 'application/octet-stream',
        },
      }),
    )
    expect(prompt).toContain('二进制内容 2048 字节')
    expect(prompt).not.toContain('aGVsbG8=')
  })
})

function reportDetail(overrides: Partial<ReportDetail> = {}): ReportDetail {
  const node: ClusterReportNode = {
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
    share: 1,
    health: { state: 'done', expected: 10, received: 10, lost: 0, gapSec: 0 },
    totalRequests: 1000,
    successCount: 990,
    errorCount: 10,
    avgRtMs: 20,
    tps: 100,
    egressAvgKbps: 5,
  }
  return {
    report: {
      id: 7,
      planName: 'order-query 压测',
      title: '订单接口容量摸底',
      totalRequests: 1000,
      successCount: 990,
      errorCount: 10,
      avgRt: 20,
      minRt: 1,
      maxRt: 300,
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
      bytesTotal: 100_000,
      maxQps: 120,
      avgQps: 100,
      targetRps: 100,
      errorRate: 1,
      concurrency: 10,
      status: 'completed',
      startTime: 1_700_000_000_000,
      endTime: 1_700_000_010_000,
      createdAt: 1_700_000_010_000,
    },
    samples: [
      {
        elapsedSec: 1,
        concurrent: 10,
        qps: 100,
        avgRt: 20,
        p50Rt: 18,
        p95Rt: 40,
        p99Rt: 80,
        ttfbAvg: 12,
        bodyAvg: 8,
        bodyP95: 10,
        bytes: 1000,
        successCount: 99,
        errorCount: 1,
      },
    ],
    errors: [],
    statusCodes: [{ code: 200, count: 990 }],
    errorTypes: [{ errorType: 'HTTP', count: 10 }],
    histogram: [{ label: '10-25ms', minMs: 10, maxMs: 25, count: 1000 }],
    stages: [],
    config: {
      recordDetail: 'sample',
      metrics: { bytesSent: 1024, recordsDropped: 0, errorsDropped: 0 },
    },
    urlStats: [
      {
        method: 'GET',
        url: 'https://api.test/orders?token=******',
        statusCode: 200,
        requests: 1000,
        successCount: 990,
        errorCount: 10,
        avgMs: 20,
        maxMs: 300,
        maxAtMs: 5000,
        totalHours: 0.006,
        avgKb: 2,
        totalMb: 2,
        statusSummary: '200×990, 500×10',
        errorMessage: 'HTTP 500',
      },
    ],
    urlStatsComplete: true,
    loadGenerators: [node],
    ...overrides,
  }
}

describe('reportReviewPrompt', () => {
  it('把 URL 统计、施压机、TTFB/Body 与完整性交给模型', () => {
    const prompt = reportReviewPrompt(reportDetail())
    expect(prompt).toContain('订单接口容量摸底')
    expect(prompt).toContain('URL 统计')
    expect(prompt).toContain('https://api.test/orders')
    expect(prompt).toContain('施压机明细')
    expect(prompt).toContain('master-a')
    expect(prompt).toContain('TPS')
    expect(prompt).toContain('Body 平均')
    expect(prompt).toContain('数据完整性')
    expect(prompt).toContain('只使用上面提供的数据')
  })

  it('旧报告未记录 URL 与施压机时不编造', () => {
    const prompt = reportReviewPrompt(
      reportDetail({ urlStats: [], urlStatsComplete: false, loadGenerators: [] }),
    )
    expect(prompt).toContain('未记录')
    expect(prompt).not.toContain('https://api.test/orders')
  })
})
