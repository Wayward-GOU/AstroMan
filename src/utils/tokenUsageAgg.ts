/**
 * Token 用量聚合：把扫描出的逐请求记录压成看板需要的几个切面。
 *
 * 全部是纯函数，输入记录数组 + 时间窗口 + 来源过滤，输出可直接渲染的结构。
 * 口径与 TokenPanel 保持一致：Delta 只跟「上一个等长窗口」比，没有上一窗口时为 null。
 */
import { TOKEN_SOURCE_LABEL, type TokenSourceId, type UsageRecord } from '@/types/tokenUsage'

export const HOUR = 3600_000
export const DAY = 86_400_000

export function startOfDay(ts: number): number {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

export function dayKey(ts: number): string {
  const d = new Date(ts)
  const p = (n: number): string => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

export interface TokenKpis {
  total: number
  requests: number
  sessions: number
  activeDays: number
  avgPerActiveDay: number
  cacheRate: number
  credit: number
  /** 费用合计（美元）：代理账面 + 价目表估算 */
  costUsd: number
  /** 无价目、未计入费用的记录数 */
  unpriced: number
  topModel: string
  topShare: number
  totalDelta: number | null
  reqDelta: number | null
  creditDelta: number | null
  costDelta: number | null
}

export interface WorkspaceRow {
  name: string
  tokens: number
  requests: number
  share: number
}

export interface Heat {
  dates: string[]
  matrix: number[][]
  max: number
  dailyTotal: number[]
}

export interface SizeDist {
  labels: string[]
  counts: number[]
  p50: number
  p90: number
  p99: number
  max: number
  mean: number
  n: number
}

export interface HourModel {
  models: string[]
  requests: number[][]
  tokens: number[][]
}

/** 每日趋势序列：日期连续，没有调用的那天补 0，折线才不会断开 */
export interface DailySeries {
  dates: string[]
  tokens: number[]
  requests: number[]
  cost: number[]
  /** 缓存命中率 0–1（缓存读 / 输入总量） */
  cacheRate: number[]
}

/** Token 构成：一次调用里四种 token 各占多少 */
export interface Composition {
  /** 输入中未命中缓存的部分 */
  input: number
  cacheRead: number
  cacheWrite: number
  output: number
  /** 推理 token（已含在 output 内，仅展示） */
  reasoning: number
}

export interface ModelDaily {  dates: string[]
  models: string[]
  tokens: number[][]
  other: number[]
  totals: number[]
  dayRequests: number[]
  dayCredit: number[]
  dayCost: number[]
  modelTotals: number[]
  otherTotal: number
  grandTotal: number
  grandRequests: number
  grandCredit: number
  grandCost: number
}

export interface TokenAggResult {
  kpis: TokenKpis
  workspaces: WorkspaceRow[]
  /** 按供应商 / 中转聚合（没有供应商字段的来源回落到来源名） */
  providers: WorkspaceRow[]
  /** 按数据来源聚合：WorkBuddy / Codex / Claude / 抓包 各占多少 */
  sources: WorkspaceRow[]
  heat: Heat
  sizes: SizeDist
  hourModel: HourModel
  modelDaily: ModelDaily
  /** 每日趋势（连续日期） */
  daily: DailySeries
  /** 四种 token 的构成 */
  composition: Composition
  windowLabel: string
  recordCount: number
}

const SIZE_BUCKETS: { label: string; test: (n: number) => boolean }[] = [
  { label: '<2k', test: (n) => n < 2000 },
  { label: '2–5k', test: (n) => n >= 2000 && n < 5000 },
  { label: '5–10k', test: (n) => n >= 5000 && n < 10000 },
  { label: '10–20k', test: (n) => n >= 10000 && n < 20000 },
  { label: '20–50k', test: (n) => n >= 20000 && n < 50000 },
  { label: '50–100k', test: (n) => n >= 50000 && n < 100000 },
  { label: '100–200k', test: (n) => n >= 100000 && n < 200000 },
  { label: '≥200k', test: (n) => n >= 200000 },
]

function pct(v: number, base: number): number {
  return base > 0 ? (v / base) * 100 : 0
}

function percentile(sorted: number[], p: number): number {
  if (!sorted.length) return 0
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * p) - 1))
  return sorted[idx]
}

/** 小时桶：0–23 */
function hourOf(ts: number): number {
  return new Date(ts).getHours()
}

export function aggregateTokenUsage(
  all: UsageRecord[],
  range: number,
  source: TokenSourceId | null,
  now = Date.now(),
): TokenAggResult {
  const today0 = startOfDay(now)
  const cutoff = range > 0 ? today0 - (range - 1) * DAY : 0
  const base = source ? all.filter((r) => r.source === source) : all
  const rows = cutoff ? base.filter((r) => r.ts >= cutoff) : base

  /* ---------------- KPI ---------------- */
  let total = 0
  let reqCount = 0
  let credit = 0
  let costUsd = 0
  let unpriced = 0
  let inputSum = 0
  let cachedSum = 0
  let outputSum = 0
  let cacheWriteSum = 0
  let reasoningSum = 0
  const sessions = new Set<string>()
  const modelTokens = new Map<string, number>()
  const activeDays = new Set<string>()

  for (const r of rows) {
    total += r.total
    reqCount += 1
    credit += r.credit
    costUsd += r.costUsd
    if (r.costKind === 'none') unpriced += 1
    inputSum += r.input
    cachedSum += r.cachedRead
    outputSum += r.output
    cacheWriteSum += r.cacheWrite
    reasoningSum += r.reasoning
    sessions.add(r.sessionId)
    activeDays.add(dayKey(r.ts))
    modelTokens.set(r.model, (modelTokens.get(r.model) ?? 0) + r.total)
  }

  const top = [...modelTokens.entries()].sort((a, b) => b[1] - a[1])[0]

  let totalDelta: number | null = null
  let reqDelta: number | null = null
  let creditDelta: number | null = null
  let costDelta: number | null = null
  if (range > 0) {
    const prevFrom = cutoff - range * DAY
    let pTotal = 0
    let pReq = 0
    let pCredit = 0
    let pCost = 0
    for (const r of base) {
      if (r.ts >= prevFrom && r.ts < cutoff) {
        pTotal += r.total
        pReq += 1
        pCredit += r.credit
        pCost += r.costUsd
      }
    }
    totalDelta = pTotal > 0 ? pct(total - pTotal, pTotal) : null
    reqDelta = pReq > 0 ? pct(reqCount - pReq, pReq) : null
    creditDelta = pCredit > 0 ? pct(credit - pCredit, pCredit) : null
    costDelta = pCost > 0 ? pct(costUsd - pCost, pCost) : null
  }

  const kpis: TokenKpis = {
    total,
    requests: reqCount,
    sessions: sessions.size,
    activeDays: activeDays.size,
    avgPerActiveDay: activeDays.size ? total / activeDays.size : 0,
    cacheRate: inputSum > 0 ? cachedSum / inputSum : 0,
    credit,
    costUsd,
    unpriced,
    topModel: top?.[0] ?? '—',
    topShare: top ? pct(top[1], total) : 0,
    totalDelta,
    reqDelta,
    creditDelta,
    costDelta,
  }

  /* ---------------- 1. 工作空间活跃分布 ---------------- */
  const wsMap = new Map<string, { tokens: number; requests: number }>()
  for (const r of rows) {
    const cur = wsMap.get(r.workspace) ?? { tokens: 0, requests: 0 }
    cur.tokens += r.total
    cur.requests += 1
    wsMap.set(r.workspace, cur)
  }
  const workspaces: WorkspaceRow[] = [...wsMap.entries()]
    .map(([name, v]) => ({
      name,
      tokens: v.tokens,
      requests: v.requests,
      share: pct(v.tokens, total),
    }))
    .sort((a, b) => b.tokens - a.tokens)

  /* ---------------- 1b. 供应商 / 中转分布 ---------------- */
  const pvMap = new Map<string, { tokens: number; requests: number }>()
  for (const r of rows) {
    const name = r.provider || TOKEN_SOURCE_LABEL[r.source] || r.source
    const cur = pvMap.get(name) ?? { tokens: 0, requests: 0 }
    cur.tokens += r.total
    cur.requests += 1
    pvMap.set(name, cur)
  }
  const providers: WorkspaceRow[] = [...pvMap.entries()]
    .map(([name, v]) => ({
      name,
      tokens: v.tokens,
      requests: v.requests,
      share: pct(v.tokens, total),
    }))
    .sort((a, b) => b.tokens - a.tokens)

  /* ---------------- 1c. 数据来源分布 ---------------- */
  const srcMap = new Map<string, { tokens: number; requests: number }>()
  for (const r of rows) {
    const name = TOKEN_SOURCE_LABEL[r.source] || r.source
    const cur = srcMap.get(name) ?? { tokens: 0, requests: 0 }
    cur.tokens += r.total
    cur.requests += 1
    srcMap.set(name, cur)
  }
  const sources: WorkspaceRow[] = [...srcMap.entries()]
    .map(([name, v]) => ({
      name,
      tokens: v.tokens,
      requests: v.requests,
      share: pct(v.tokens, total),
    }))
    .sort((a, b) => b.tokens - a.tokens)

  /* ---------------- 2. 日活分布（日 × 小时） ---------------- */
  const heatMap = new Map<string, number[]>()
  for (const r of rows) {
    const d = dayKey(r.ts)
    let arr = heatMap.get(d)
    if (!arr) {
      arr = new Array(24).fill(0)
      heatMap.set(d, arr)
    }
    arr[hourOf(r.ts)] += r.total
  }
  const dayKeys = [...heatMap.keys()].sort()
  const firstDay = cutoff || (dayKeys.length ? Date.parse(`${dayKeys[0]}T00:00:00`) : today0)
  const dates: string[] = []
  for (let t = startOfDay(firstDay); t <= today0; t += DAY) dates.push(dayKey(t))
  // 数据里若存在晚于今天的日期（时区/异常），一并补上
  for (const d of dayKeys) if (!dates.includes(d)) dates.push(d)
  dates.sort()
  const trimmed = dates.slice(-92)
  const matrix = trimmed.map((d) => heatMap.get(d) ?? new Array(24).fill(0))
  const heat: Heat = {
    dates: trimmed,
    matrix,
    max: Math.max(1, ...matrix.flat()),
    dailyTotal: matrix.map((row) => row.reduce((a, b) => a + b, 0)),
  }

  /* ---------------- 2b. 每日趋势（连续日期，缺的补 0） ---------------- */
  const trendReq = new Map<string, number>()
  const trendCost = new Map<string, number>()
  const trendInput = new Map<string, number>()
  const trendCached = new Map<string, number>()
  for (const r of rows) {
    const d = dayKey(r.ts)
    trendReq.set(d, (trendReq.get(d) ?? 0) + 1)
    trendCost.set(d, (trendCost.get(d) ?? 0) + r.costUsd)
    trendInput.set(d, (trendInput.get(d) ?? 0) + r.input)
    trendCached.set(d, (trendCached.get(d) ?? 0) + r.cachedRead)
  }
  const daily: DailySeries = {
    dates: heat.dates,
    tokens: heat.dailyTotal,
    requests: heat.dates.map((d) => trendReq.get(d) ?? 0),
    cost: heat.dates.map((d) => trendCost.get(d) ?? 0),
    cacheRate: heat.dates.map((d) => {
      const input = trendInput.get(d) ?? 0
      return input > 0 ? (trendCached.get(d) ?? 0) / input : 0
    }),
  }

  // input 字段含缓存读取，展示时把缓存拆出来，避免「输入」被重复计算
  const composition: Composition = {
    input: Math.max(0, inputSum - cachedSum),
    cacheRead: cachedSum,
    cacheWrite: cacheWriteSum,
    output: outputSum,
    reasoning: reasoningSum,
  }

  /* ---------------- 3. 单次请求 token 大小分布 ---------------- */
  const sizesArr = rows.map((r) => r.total).sort((a, b) => a - b)
  const sizes: SizeDist = {
    labels: SIZE_BUCKETS.map((b) => b.label),
    counts: SIZE_BUCKETS.map((b) => sizesArr.filter((n) => b.test(n)).length),
    p50: percentile(sizesArr, 0.5),
    p90: percentile(sizesArr, 0.9),
    p99: percentile(sizesArr, 0.99),
    max: sizesArr.length ? sizesArr[sizesArr.length - 1] : 0,
    mean: sizesArr.length ? sizesArr.reduce((a, b) => a + b, 0) / sizesArr.length : 0,
    n: sizesArr.length,
  }

  /* ---------------- 4. 模型日使用分布（0–24 时） ---------------- */
  const modelReq = new Map<string, number[]>()
  const modelTok = new Map<string, number[]>()
  for (const r of rows) {
    const h = hourOf(r.ts)
    if (!modelReq.has(r.model)) {
      modelReq.set(r.model, new Array(24).fill(0))
      modelTok.set(r.model, new Array(24).fill(0))
    }
    modelReq.get(r.model)![h] += 1
    modelTok.get(r.model)![h] += r.total
  }
  const hourModels = [...modelTok.entries()]
    .sort(
      (a, b) =>
        b[1].reduce((x, y) => x + y, 0) - a[1].reduce((x, y) => x + y, 0),
    )
    .slice(0, 8)
    .map(([m]) => m)
  const hourModel: HourModel = {
    models: hourModels,
    requests: hourModels.map((m) => modelReq.get(m) ?? new Array(24).fill(0)),
    tokens: hourModels.map((m) => modelTok.get(m) ?? new Array(24).fill(0)),
  }

  /* ---------------- 5. 每日模型用量统计汇总 ---------------- */
  const dayModelMap = new Map<string, Map<string, number>>()
  const dayReq = new Map<string, number>()
  const dayCredit = new Map<string, number>()
  const dayCost = new Map<string, number>()
  for (const r of rows) {
    const d = dayKey(r.ts)
    let m = dayModelMap.get(d)
    if (!m) {
      m = new Map()
      dayModelMap.set(d, m)
    }
    m.set(r.model, (m.get(r.model) ?? 0) + r.total)
    dayReq.set(d, (dayReq.get(d) ?? 0) + 1)
    dayCredit.set(d, (dayCredit.get(d) ?? 0) + r.credit)
    dayCost.set(d, (dayCost.get(d) ?? 0) + r.costUsd)
  }
  const tableDates = [...dayModelMap.keys()].sort().reverse()
  const topModels = [...modelTokens.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 6)
    .map(([m]) => m)
  const modelDaily: ModelDaily = {
    dates: tableDates,
    models: topModels,
    tokens: tableDates.map((d) => topModels.map((m) => dayModelMap.get(d)?.get(m) ?? 0)),
    other: tableDates.map((d) => {
      const allModels = dayModelMap.get(d)
      if (!allModels) return 0
      let sum = 0
      for (const [m, v] of allModels) if (!topModels.includes(m)) sum += v
      return sum
    }),
    totals: tableDates.map((d) =>
      [...(dayModelMap.get(d)?.values() ?? [])].reduce((a, b) => a + b, 0),
    ),
    dayRequests: tableDates.map((d) => dayReq.get(d) ?? 0),
    dayCredit: tableDates.map((d) => dayCredit.get(d) ?? 0),
    dayCost: tableDates.map((d) => dayCost.get(d) ?? 0),
    modelTotals: topModels.map((m) => modelTokens.get(m) ?? 0),
    otherTotal: [...modelTokens.entries()]
      .filter(([m]) => !topModels.includes(m))
      .reduce((a, b) => a + b[1], 0),
    grandTotal: total,
    grandRequests: reqCount,
    grandCredit: credit,
    grandCost: costUsd,
  }

  const windowLabel = range === 0 ? '全部时间' : range === 1 ? '今日' : `近 ${range} 天`

  return {
    kpis,
    workspaces,
    providers,
    sources,
    heat,
    sizes,
    hourModel,
    modelDaily,
    daily,
    composition,
    windowLabel,
    recordCount: rows.length,
  }
}
