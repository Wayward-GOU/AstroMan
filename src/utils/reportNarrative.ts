import type { ReportDetail } from '@/types/models'
import { detectKnee, median, qpsP10, sampleSummary, scoreReport } from './reportAnalytics'

/**
 * 报告叙述层：把指标翻译成「结论 / 发现 / 建议」
 *
 * 与 reportAnalytics 的分工：
 *   - analytics 只做数值与判定（拐点、打分、勾稽）；
 *   - narrative 负责组织成可读的结论句子，供 HTML 报告与页面复用。
 *
 * 所有规则均为确定性推导，不含随机与外部依赖，可单测。
 */

export type FindingLevel = 'ok' | 'warn' | 'bad' | 'info'

export interface Finding {
  level: FindingLevel
  /** 一句话结论（报告里作粗体标题） */
  title: string
  /** 支撑该结论的数据依据 */
  detail: string
}

export interface Advice {
  priority: 'P0' | 'P1' | 'P2'
  title: string
  detail: string
}

/** 报告结论等级 */
export type ReportGrade = 'pass' | 'conditional' | 'fail'

export interface GradeResult {
  grade: ReportGrade
  label: string
  /** 结论正文（一句话） */
  statement: string
  /** 触发该结论的首要原因 */
  reason: string
}

const pct = (value: number, digits = 2) => `${value.toFixed(digits)}%`
const ms = (value: number) => `${value.toFixed(value >= 100 ? 0 : 1)} ms`

/** 时间点格式化为 分:秒，用于错误窗口描述 */
function clock(sec: number): string {
  const total = Math.max(0, Math.round(sec))
  const minute = Math.floor(total / 60)
  const second = total % 60
  return `${minute}:${String(second).padStart(2, '0')}`
}

/** 前后半程平均 RT 的相对漂移，正值表示越跑越慢 */
export function latencyDrift(detail: ReportDetail): number {
  const samples = detail.samples ?? []
  if (samples.length < 20) return 0
  const mid = Math.floor(samples.length / 2)
  const head = median(samples.slice(0, mid).map((item) => item.avgRt))
  const tail = median(samples.slice(mid).map((item) => item.avgRt))
  return head > 0 ? (tail - head) / head : 0
}

/** 错误发生的时间窗口；无错误时返回 null */
export function errorWindow(detail: ReportDetail): { from: number; to: number; count: number } | null {
  const hits = (detail.samples ?? []).filter((item) => item.errorCount > 0)
  if (!hits.length) return null
  return {
    from: hits[0].elapsedSec,
    to: hits[hits.length - 1].elapsedSec,
    count: hits.reduce((sum, item) => sum + item.errorCount, 0),
  }
}

/** 响应时间累积分布（由直方图分桶还原，避免依赖逐请求明细） */
export function cdfPoints(detail: ReportDetail): Array<{ ms: number; pct: number }> {
  const buckets = [...(detail.histogram ?? [])].sort((a, b) => a.minMs - b.minMs)
  const total = buckets.reduce((sum, item) => sum + item.count, 0)
  if (!total) return []
  let cumulative = 0
  const points = [{ ms: 0, pct: 0 }]
  buckets.forEach((bucket) => {
    cumulative += bucket.count
    points.push({ ms: Math.max(bucket.maxMs, bucket.minMs), pct: (cumulative / total) * 100 })
  })
  return points
}

/**
 * 结论等级
 *
 * 判定顺序（先命中先返回）：
 *   1. SLA 存在未达标项 / 错误率 > 5% / 综合得分 < 60  →  fail
 *   2. 存在告警级发现（尾部延迟放大、RT 漂移、施压端吃满）→ conditional
 *   3. 其余 → pass
 */
export function gradeReport(detail: ReportDetail): GradeResult {
  const report = detail.report
  const sla = detail.config?.sla
  const score = scoreReport(detail)

  const failed: string[] = []
  if (sla?.p95Ms && report.p95Rt > sla.p95Ms) failed.push('P95 延迟')
  if (sla?.p99Ms && report.p99Rt > sla.p99Ms) failed.push('P99 延迟')
  if (sla?.errorRate && report.errorRate > sla.errorRate) failed.push('错误率')
  if (sla?.minRps && report.avgQps < sla.minRps) failed.push('平均 QPS')

  if (failed.length) {
    return {
      grade: 'fail',
      label: '未通过',
      statement: `本次压测未达成 SLA 目标，${failed.join('、')} 未达标。`,
      reason: `未达标项：${failed.join('、')}`,
    }
  }
  if (report.errorRate > 5) {
    return {
      grade: 'fail',
      label: '未通过',
      statement: `错误率 ${pct(report.errorRate)} 超出可接受范围，系统在该压力下不具备可用性。`,
      reason: `错误率 ${pct(report.errorRate)} > 5%`,
    }
  }
  if (score.total !== null && score.total < 60) {
    return {
      grade: 'fail',
      label: '未通过',
      statement: `综合体检 ${score.total.toFixed(0)} 分（${score.grade}），多项指标同时偏离健康区间。`,
      reason: `综合得分 ${score.total.toFixed(0)} < 60`,
    }
  }

  const tailRatio = report.avgRt > 0 ? report.p99Rt / report.avgRt : 1
  const drift = latencyDrift(detail)
  const warnReasons: string[] = []
  if (tailRatio >= 2.5) warnReasons.push(`P99 为平均值的 ${tailRatio.toFixed(1)} 倍`)
  if (drift > 0.15) warnReasons.push(`后半程响应时间较前半程上升 ${pct(drift * 100, 1)}`)
  if (score.total !== null && score.total < 75) warnReasons.push(`综合体检 ${score.total.toFixed(0)} 分`)

  if (warnReasons.length) {
    return {
      grade: 'conditional',
      label: '有条件通过',
      statement: '本次压测达成主要目标，但存在需要跟进的性能风险。',
      reason: warnReasons.join('；'),
    }
  }

  return {
    grade: 'pass',
    label: '通过',
    statement: '本次压测达到预期目标，指标处于健康区间。',
    reason: `错误率 ${pct(report.errorRate)}，APDEX ${report.apdex.toFixed(3)}`,
  }
}

/**
 * 关键发现：按「先坏后好」排序，最多 6 条，保证报告首页信息密度合理
 */
export function buildFindings(detail: ReportDetail): Finding[] {
  const report = detail.report
  const config = detail.config
  const samples = detail.samples ?? []
  const findings: Finding[] = []
  const knee = detectKnee(detail)
  const summary = sampleSummary(samples)
  const ninety = qpsP10(samples, { rampUpSec: config?.rampUpSec ?? 0 })
  const drift = latencyDrift(detail)
  const window = errorWindow(detail)

  if (knee.saturated) {
    findings.push({
      level: 'warn',
      title: `吞吐在约 ${knee.concurrent} 并发处饱和`,
      detail: `饱和度拐点之后继续加压，QPS 不再增长而 P95 明显抬升，实测最大有效吞吐 ${Math.round(
        knee.qps,
      )} req/s。该并发量即为当前部署下的容量上限参考值。`,
    })
  } else if (detail.stages.length) {
    findings.push({
      level: 'info',
      title: '未检出饱和拐点',
      detail: `各阶段吞吐持续增长，最高 ${knee.concurrent} 并发 / ${Math.round(
        knee.qps,
      )} req/s。容量上限尚未触达，建议以更高并发复测以定位拐点。`,
    })
  }

  if (report.errorRate > 0) {
    const top = [...detail.errorTypes].sort((a, b) => b.count - a.count)[0]
    const level: FindingLevel = report.errorRate > 5 ? 'bad' : report.errorRate > 1 ? 'warn' : 'info'
    findings.push({
      level,
      title: `错误率 ${pct(report.errorRate)}，共 ${report.errorCount.toLocaleString('zh-CN')} 次失败`,
      detail: [
        top ? `主要错误类型为 ${top.errorType}（${top.count.toLocaleString('zh-CN')} 次）` : '',
        window ? `错误集中在第 ${clock(window.from)}–${clock(window.to)}（压测第 ${window.from}–${window.to} 秒）` : '',
        report.errorRate <= 1 ? '占比已低于 1%，属于可接受范围，但建议确认是否与重试/超时策略有关' : '',
      ]
        .filter(Boolean)
        .join('；'),
    })
  } else {
    findings.push({
      level: 'ok',
      title: '全程零错误',
      detail: `累计 ${report.totalRequests.toLocaleString(
        'zh-CN',
      )} 次请求全部成功，未出现超时、连接失败或 5xx 响应。`,
    })
  }

  const tailRatio = report.avgRt > 0 ? report.p99Rt / report.avgRt : 1
  if (tailRatio >= 2) {
    findings.push({
      level: tailRatio >= 3 ? 'warn' : 'info',
      title: `尾部延迟放大，P99 是平均值的 ${tailRatio.toFixed(1)} 倍`,
      detail: `平均 ${ms(report.avgRt)}、P95 ${ms(report.p95Rt)}、P99 ${ms(
        report.p99Rt,
      )}。长尾通常来自连接池等待、慢查询或 GC 停顿，建议按 Top 慢请求逐条归因。`,
    })
  }

  if (drift > 0.15) {
    findings.push({
      level: drift > 0.4 ? 'bad' : 'warn',
      title: `响应时间随时间漂移，后半程上升 ${pct(drift * 100, 1)}`,
      detail: `压测后半程平均响应时间显著高于前半程，常见原因为内存增长、连接泄漏或下游缓存击穿。建议延长单次压测时长至 30 分钟以上确认是否收敛。`,
    })
  } else if (samples.length >= 60) {
    findings.push({
      level: 'ok',
      title: '性能表现稳定',
      detail: `前后半程响应时间中位数差异 ${pct(Math.abs(drift) * 100, 1)}，未见随时间劣化，长时间运行风险较低。`,
    })
  }

  if (ninety.value > 0 && summary.maxQps > 0) {
    findings.push({
      level: 'info',
      title: `90% 时间吞吐不低于 ${Math.round(ninety.value)} req/s`,
      detail: `峰值 ${Math.round(summary.maxQps)} req/s，均值 ${Math.round(
        summary.avgQps,
      )} req/s。该值已剔除升压段与首末不完整采样（有效采样 ${ninety.used} 点，剔除 ${ninety.dropped} 点），比平均值更能代表稳定承载能力。`,
    })
  }

  const agent = config?.agent
  const metrics = config?.metrics

  // 施压端能力联动（3.7.3）：达到检测极限 80% 时提示结论谨慎
  const capacity = config?.machineCapacity
  if (capacity?.maxQps && capacity.maxQps > 0) {
    const usage = report.avgQps / capacity.maxQps
    if (usage >= 0.8) {
      findings.push({
        level: 'warn',
        title: `客户端负载已达检测极限的 ${pct(usage * 100, 0)}`,
        detail: `本机回环检测极限 ${
          capacity.reachedLimit === false ? '≥ ' : ''
        }${Math.round(capacity.maxQps)} QPS，本次平均 ${Math.round(
          report.avgQps,
        )} QPS。客户端接近能力上限，结论需谨慎；建议关闭明细采集或拆分施压机后复测。`,
      })
    } else if (usage < 0.5) {
      findings.push({
        level: 'ok',
        title: '客户端余量充足，瓶颈不在施压端',
        detail: `本机检测极限 ${
          capacity.reachedLimit === false ? '≥ ' : ''
        }${Math.round(capacity.maxQps)} QPS，本次仅用到 ${pct(
          usage * 100,
          0,
        )}。压力瓶颈应在本机之外的目标系统或网络上。`,
      })
    }
  }

  // 同主机叠加压测留痕（1.3.2）：只提示口径，不判失败
  if (config?.sameHostWarning) {
    findings.push({
      level: 'warn',
      title: '同主机叠加压测，容量结论需谨慎',
      detail: `${config.sameHostWarning}。建议改串行执行后复测，或使用独立施压机再做容量结论。`,
    })
  }

  if (agent?.cpuCores && metrics?.cpuPeakCores) {
    const usage = metrics.cpuPeakCores / agent.cpuCores
    if (usage >= 0.8) {
      findings.push({
        level: 'warn',
        title: `施压端 CPU 峰值已达 ${pct(usage * 100, 0)}，存在施压机瓶颈风险`,
        detail: `施压端 ${agent.cpuCores} 核，压测期间峰值 ${metrics.cpuPeakCores.toFixed(
          1,
        )} 核。此时实测吞吐可能受限于施压端而非被测系统，建议增加施压节点或降低明细采集开销后复测。`,
      })
    }
  }

  const order: Record<FindingLevel, number> = { bad: 0, warn: 1, info: 2, ok: 3 }
  return findings.sort((a, b) => order[a.level] - order[b.level]).slice(0, 6)
}

/**
 * 优化建议：按优先级输出，每条都绑定触发它的数据条件
 */
export function buildAdvice(detail: ReportDetail): Advice[] {
  const report = detail.report
  const config = detail.config
  const metrics = config?.metrics
  const agent = config?.agent
  const advice: Advice[] = []
  const knee = detectKnee(detail)
  const drift = latencyDrift(detail)
  const tailRatio = report.avgRt > 0 ? report.p99Rt / report.avgRt : 1

  if (report.errorRate > 1) {
    const top = [...detail.errorTypes].sort((a, b) => b.count - a.count)[0]
    advice.push({
      priority: 'P0',
      title: `定位并消除 ${top?.errorType ?? '失败请求'} 错误`,
      detail: `错误率 ${pct(report.errorRate)} 已影响结论有效性。优先核查 ${
        top ? `「${top.errorType}」` : ''
      }对应的响应码与服务端日志，确认是限流、连接池耗尽还是下游超时；修复后以相同参数复测，要求错误率 ≤ 1%。`,
    })
  }

  if (knee.saturated) {
    advice.push({
      priority: 'P0',
      title: `以 ${knee.concurrent} 并发为基准做瓶颈下钻`,
      detail: `该并发下吞吐已饱和而延迟抬升，说明存在明确的资源瓶颈。建议在同压力下采集服务端 CPU、GC、数据库慢查询与连接池等待指标，定位是应用层、中间件还是存储层先到极限。`,
    })
  }

  if (tailRatio >= 2) {
    advice.push({
      priority: 'P1',
      title: '治理长尾延迟',
      detail: `P99/平均 = ${tailRatio.toFixed(1)}，尾部请求拖低了用户体验一致性。可从连接池预热与最大连接数、下游接口超时与重试退避、慢 SQL 与索引三方面入手，目标把 P99 压到平均值的 2 倍以内。`,
    })
  }

  if (drift > 0.15) {
    advice.push({
      priority: 'P1',
      title: '排查长时间运行下的性能劣化',
      detail: `后半程响应时间上升 ${pct(drift * 100, 1)}，需确认是否与内存增长（堆/堆外）、连接未释放或缓存失效有关。建议做 4 小时以上长稳压测并观察 RSS 与 GC 频率曲线。`,
    })
  }

  if (agent?.cpuCores && metrics?.cpuPeakCores && metrics.cpuPeakCores / agent.cpuCores >= 0.8) {
    advice.push({
      priority: 'P1',
      title: '消除施压端瓶颈后再评估服务端容量',
      detail: `施压端 CPU 峰值 ${metrics.cpuPeakCores.toFixed(1)} / ${agent.cpuCores} 核，已接近跑满。建议拆分到多台施压机、关闭或降低明细采集（recordDetail=off/sample），确保压力真正落在被测系统上。`,
    })
  }

  if (!knee.saturated && report.targetRps > 0 && report.avgQps < report.targetRps) {
    advice.push({
      priority: 'P1',
      title: '继续加压以确认目标吞吐可达',
      detail: `实测平均 ${Math.round(report.avgQps)} req/s，尚未达到目标 ${Math.round(
        report.targetRps,
      )} req/s。当前压力下未见饱和，建议按 20% 步长继续提高并发或放开限速后复测。`,
    })
  }

  if (report.apdex < 0.9) {
    advice.push({
      priority: 'P2',
      title: '提升用户可感知性能（APDEX）',
      detail: `APDEX ${report.apdex.toFixed(3)}，仍有 ${pct(
        (1 - report.apdex) * 100,
        1,
      )} 的请求超出可容忍阈值。优先优化占比最大的常规请求路径，而非只优化均值。`,
    })
  }

  if (!advice.length) {
    advice.push({
      priority: 'P2',
      title: '保持当前配置并纳入回归基线',
      detail: '本次压测未发现显著风险项。建议将该参数组合与实测指标登记为性能基线，在每次版本发布前以相同脚本执行回归，并关注 P95、P99 与错误率的漂移。',
    })
  }

  advice.push({
    priority: 'P2',
    title: '在贴近生产的环境复测',
    detail:
      '本次结果仅在所述施压端与目标环境下成立。单机施压、本机回环或缩容部署都会高估系统容量，正式容量结论应在生产等价拓扑、真实数据量与同规格机器上取得。',
  })

  return advice
}
