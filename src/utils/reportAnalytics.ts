import type { ReportDetail, ReportSample, StageSummary } from '@/types/models'

/**
 * 压测报告分析算法集合
 *
 * 这里的全部函数只做「纯计算」，不依赖 Vue / DOM，便于单测与在 HTML 导出中复用。
 * 指标口径与《AstroMan压测能力提升与报告优化技术方案》4.0 节保持一致。
 */

/** 线性插值分位数，values 需已升序 */
export function percentileSorted(values: number[], p: number): number {
  if (!values.length) return 0
  const clamped = Math.min(1, Math.max(0, p))
  const index = (values.length - 1) * clamped
  const lower = Math.floor(index)
  const upper = Math.ceil(index)
  if (lower === upper) return values[lower]
  const weight = index - lower
  return values[lower] * (1 - weight) + values[upper] * weight
}

/** 复制并升序排序后取分位数 */
export function percentile(values: number[], p: number): number {
  return percentileSorted([...values].sort((a, b) => a - b), p)
}

export function median(values: number[]): number {
  return percentile(values, 0.5)
}

/**
 * 90% 分布 QPS：每秒采样 QPS 的 P10
 *
 * 参考样本中的「90% QPS」含义是「90% 的时间不低于该值」，因此取 P10。
 * 必须剔除三种失真：
 *   1. 增压（ramp-up）阶段的低值；
 *   2. 首秒（采样窗口不完整）；
 *   3. 末尾不完整秒。
 */
export function qpsP10(
  samples: ReportSample[],
  options: { rampUpSec?: number } = {},
): { value: number; used: number; dropped: number } {
  if (!samples.length) return { value: 0, used: 0, dropped: 0 }
  const rampUp = Math.max(0, Math.round(options.rampUpSec ?? 0))
  // 无明确 ramp-up 配置时，至少剔除首 5%（且不少于 1 个、不多于 5 个）采样点
  const fallbackWarmup = Math.min(5, Math.max(1, Math.round(samples.length * 0.05)))
  const warmupBoundary = rampUp > 0 ? rampUp : samples[fallbackWarmup - 1]?.elapsedSec ?? 0
  const usable = samples.filter(
    (item, index) => item.elapsedSec > warmupBoundary && index < samples.length - 1,
  )
  const values = (usable.length >= 3 ? usable : samples).map((item) => item.qps)
  return {
    value: percentile(values, 0.1),
    used: values.length,
    dropped: samples.length - values.length,
  }
}

/** 每秒采样序列的统计摘要 */
export function sampleSummary(samples: ReportSample[]): {
  count: number
  avgQps: number
  maxQps: number
  minQps: number
  p95Rt: number
  avgRt: number
} {
  if (!samples.length) {
    return { count: 0, avgQps: 0, maxQps: 0, minQps: 0, p95Rt: 0, avgRt: 0 }
  }
  const qpsValues = samples.map((item) => item.qps)
  const rtValues = samples.map((item) => item.avgRt)
  return {
    count: samples.length,
    avgQps: qpsValues.reduce((sum, value) => sum + value, 0) / samples.length,
    maxQps: Math.max(...qpsValues),
    minQps: Math.min(...qpsValues),
    p95Rt: percentile(
      samples.map((item) => item.p95Rt),
      0.95,
    ),
    avgRt: rtValues.reduce((sum, value) => sum + value, 0) / rtValues.length,
  }
}

/** 等距抽稀到 maxPoints 个点，保留首尾与极值，用于 SVG 报告降采样 */
export function downsample<T>(items: T[], maxPoints = 300): T[] {
  if (items.length <= maxPoints || maxPoints < 3) return items
  const step = (items.length - 1) / (maxPoints - 1)
  const result: T[] = []
  for (let index = 0; index < maxPoints; index += 1) {
    result.push(items[Math.round(index * step)])
  }
  return result
}

export interface KneeResult {
  /** 是否检出饱和拐点 */
  saturated: boolean
  /** 最大有效并发（未饱和时为最高阶段并发） */
  concurrent: number
  /** 最大有效 QPS */
  qps: number
  /** 面向用户的结论描述 */
  note: string
  /** 每阶段的稳态窗口统计，用于报告附注 */
  stages: Array<{ index: number; concurrent: number; qps: number; p95: number; gain: number }>
}

/**
 * 拐点检测
 *
 * 优先按「阶段稳态窗口」判定，而不是对整条曲线做分段线性拟合：
 * 每阶段剔除前 20%（升压）与后 10%（切换抖动）采样，取中位数 QPS / P95，
 * 当相邻阶段「QPS 增幅 < 5% 且 P95 增幅 > 20%」时判定该阶段已饱和。
 * 这个方法对噪声不敏感，且结论可解释（能直接落到报告里）。
 */
export function detectKnee(detail: ReportDetail): KneeResult {
  const stages: StageSummary[] = detail.stages ?? []
  const samples = detail.samples ?? []

  if (!stages.length || samples.length < 4) {
    const summary = sampleSummary(samples)
    return {
      saturated: false,
      concurrent: detail.report.concurrency,
      qps: summary.maxQps,
      note: stages.length
        ? '采样点不足，无法判定拐点'
        : '固定并发模式不做容量判定，如需测量容量请改用阶梯加压',
      stages: [],
    }
  }

  const rows: KneeResult['stages'] = []
  let startSec = 0
  stages.forEach((stage, index) => {
    const endSec = startSec + Math.max(1, stage.durationSec)
    const window = samples.filter(
      (item) => item.elapsedSec > startSec && item.elapsedSec <= endSec,
    )
    const trimHead = Math.floor(window.length * 0.2)
    const trimTail = Math.floor(window.length * 0.1)
    const steady = window.slice(trimHead, Math.max(trimHead + 1, window.length - trimTail))
    const qps = steady.length ? median(steady.map((item) => item.qps)) : stage.avgQps
    const p95 = steady.length ? median(steady.map((item) => item.p95Rt)) : stage.p95Rt
    const prev = rows[rows.length - 1]
    rows.push({
      index: index + 1,
      concurrent: stage.concurrent,
      qps,
      p95,
      gain: prev && prev.qps > 0 ? (qps - prev.qps) / prev.qps : 1,
    })
    startSec = endSec
  })

  let saturatedAt = -1
  for (let index = 1; index < rows.length; index += 1) {
    const current = rows[index]
    const prev = rows[index - 1]
    const p95Gain = prev.p95 > 0 ? (current.p95 - prev.p95) / prev.p95 : 0
    if (current.gain < 0.05 && p95Gain > 0.2) {
      saturatedAt = index - 1
      break
    }
  }

  if (saturatedAt >= 0) {
    const capacity = rows[saturatedAt]
    return {
      saturated: true,
      concurrent: capacity.concurrent,
      qps: capacity.qps,
      note: `第 ${capacity.index} 阶段起吞吐不再增长而 P95 明显抬升，最大有效容量约 ${capacity.concurrent} 并发 / ${Math.round(
        capacity.qps,
      )} QPS`,
      stages: rows,
    }
  }

  const best = rows.reduce((peak, item) => (item.qps > peak.qps ? item : peak), rows[0])
  return {
    saturated: false,
    concurrent: best.concurrent,
    qps: best.qps,
    note: `各阶段吞吐仍在增长，未检出饱和拐点；当前最高 ${best.concurrent} 并发 / ${Math.round(
      best.qps,
    )} QPS，建议继续加压至吞吐不再增长`,
    stages: rows,
  }
}

export interface ScoreDimension {
  key: 'capacity' | 'stability' | 'errors' | 'tail'
  label: string
  score: number | null
  detail: string
}

export interface ScoreResult {
  dimensions: ScoreDimension[]
  total: number | null
  grade: '优秀' | '良好' | '一般' | '较差'
  ruleNote: string
}

const clamp = (value: number, min = 0, max = 100) => Math.min(max, Math.max(min, value))

/**
 * 四维体检打分
 *
 * 规则（随报告附注，可解释，系数按真实业务接口的分布校准）：
 * - 容量：阶梯加压取「各阶段稳态 RPS 的峰值」，固定并发取平均 RPS；
 *         有目标 RPS 时与目标相比并封顶 100，无目标时不评分
 * - 稳定性：100 − RT 变异系数 × 150（CV 0.2 → 70 分，0.4 → 40 分，0.67 → 0 分）
 * - 错误：100 − 错误率 × 20（0% → 100，1% → 80，5% → 0）
 * - 尾部延迟：100 − (P99/avg − 1) × 25（比值 1.5 → 87，2 → 75，3 → 50，5 → 0）
 *
 * 系数取真实互联网服务的常见分布而非理想值：典型业务接口 P99/平均在 2~3 倍、
 * 变异系数在 0.3~0.6 之间，若按线性 100 分基准会给所有真实系统打到 0 分，失去区分度。
 *
 * 容量为什么用稳态峰值：阶梯加压的「平均 RPS」被低压阶段稀释，用它比目标会系统性低估容量；
 * 而固定并发没有阶段概念，平均 RPS 才是真实承载水平。
 */
export function scoreReport(detail: ReportDetail): ScoreResult {
  const report = detail.report
  const cv = report.avgRt > 0 ? report.stddevRt / report.avgRt : 0
  const tailRatio = report.avgRt > 0 ? report.p99Rt / report.avgRt : 1
  const target = report.targetRps ?? 0
  const stageRows = detectKnee(detail).stages
  const steadyPeak = stageRows.length ? Math.max(...stageRows.map((item) => item.qps)) : 0
  const capacityQps = steadyPeak > 0 ? steadyPeak : report.avgQps
  const capacityLabel = steadyPeak > 0 ? '阶段稳态峰值' : '平均'

  const dimensions: ScoreDimension[] = [
    {
      key: 'capacity',
      label: '容量',
      score: target > 0 ? clamp((capacityQps / target) * 100) : null,
      detail:
        target > 0
          ? `${capacityLabel} ${capacityQps.toFixed(0)} / 目标 ${target.toFixed(0)} QPS`
          : '未设置目标 RPS，本项不评分',
    },
    {
      key: 'stability',
      label: '稳定性',
      score: clamp(100 - cv * 150),
      detail: `响应时间变异系数 ${(cv * 100).toFixed(1)}%`,
    },
    {
      key: 'errors',
      label: '错误',
      score: clamp(100 - report.errorRate * 20),
      detail: `错误率 ${report.errorRate.toFixed(2)}%`,
    },
    {
      key: 'tail',
      label: '尾部延迟',
      score: clamp(100 - (tailRatio - 1) * 25),
      detail: `P99/平均 = ${tailRatio.toFixed(2)}`,
    },
  ]

  const scored = dimensions.filter((item) => item.score !== null)
  const total = scored.length
    ? scored.reduce((sum, item) => sum + (item.score ?? 0), 0) / scored.length
    : null
  const grade: ScoreResult['grade'] =
    total === null ? '一般' : total >= 90 ? '优秀' : total >= 75 ? '良好' : total >= 60 ? '一般' : '较差'

  return {
    dimensions,
    total,
    grade,
    ruleNote:
      '容量 = 阶段稳态 RPS 峰值（固定并发取平均）/ 目标 × 100（无目标不评分）；稳定性 = 100 − 变异系数 × 150；错误 = 100 − 错误率 × 20；尾部延迟 = 100 − (P99/平均 − 1) × 25',
  }
}

export interface SelfCheckItem {
  key: string
  label: string
  pass: boolean
  detail: string
}

/**
 * 数据自洽校验：报告内三处口径必须互相勾稽，否则说明采集/聚合有问题
 */
export function selfCheck(detail: ReportDetail): SelfCheckItem[] {
  const report = detail.report
  const samples = detail.samples ?? []
  const sumSuccess = samples.reduce((sum, item) => sum + item.successCount, 0)
  const sumErrors = samples.reduce((sum, item) => sum + item.errorCount, 0)
  const sumBytes = samples.reduce((sum, item) => sum + item.bytes, 0)

  const countOk = report.totalRequests === report.successCount + report.errorCount
  const sampleGap = Math.abs(sumSuccess + sumErrors - report.totalRequests)
  // 采样窗口会漏掉最后不足一秒的请求，容差按 1 秒的量级（avgQps）放宽
  const sampleTolerance = Math.max(1, report.avgQps)
  const sampleOk = sampleGap <= sampleTolerance
  const byteGap = report.bytesTotal > 0 ? Math.abs(sumBytes - report.bytesTotal) / report.bytesTotal : 0
  const byteOk = byteGap <= 0.01

  return [
    {
      key: 'count',
      label: '计数自洽（总请求 = 成功 + 失败）',
      pass: countOk,
      detail: `${report.totalRequests} = ${report.successCount} + ${report.errorCount}`,
    },
    {
      key: 'samples',
      label: '采样自洽（Σ 每秒采样 ≈ 总请求）',
      pass: sampleOk,
      detail: `采样合计 ${sumSuccess + sumErrors}，与总数相差 ${sampleGap}`,
    },
    {
      key: 'bytes',
      label: '流量自洽（Σ 每秒字节 ≈ 总流量）',
      pass: byteOk,
      detail: `偏差 ${(byteGap * 100).toFixed(2)}%`,
    },
  ]
}
