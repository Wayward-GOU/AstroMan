import { describe, expect, it } from 'vitest'
import { aggregateTokenUsage, DAY, dayKey, startOfDay } from '@/utils/tokenUsageAgg'
import type { TokenSourceId, UsageRecord } from '@/types/tokenUsage'

/** 固定一个「现在」，让窗口计算可复现 */
const NOW = new Date('2026-11-27T10:00:00+08:00').getTime()
const TODAY0 = startOfDay(NOW)

function rec(
  ts: number,
  total: number,
  overrides: Partial<UsageRecord> = {},
): UsageRecord {
  return {
    id: `r-${ts}-${total}`,
    ts,
    source: 'workbuddy',
    model: 'deepseek-v4',
    workspace: 'AstroMan',
    workspacePath: '/tmp/AstroMan',
    sessionId: 's1',
    input: Math.round(total * 0.8),
    output: Math.round(total * 0.2),
    cachedRead: Math.round(total * 0.4),
    cacheWrite: 0,
    reasoning: 0,
    total,
    credit: 1,
    costUsd: 0,
    costKind: 'none',
    provider: '',
    ...overrides,
  }
}

describe('aggregateTokenUsage', () => {
  it('近 7 天包含今天但不含更早的记录', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 100),
      rec(TODAY0 - 6 * DAY + 3_600_000, 200),
      rec(TODAY0 - 7 * DAY, 400),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.kpis.total).toBe(300)
    expect(agg.kpis.requests).toBe(2)
    expect(agg.recordCount).toBe(2)
  })

  it('全量窗口不做时间过滤', () => {
    const all = [rec(TODAY0 - 400 * DAY, 10), rec(TODAY0, 20)]
    const agg = aggregateTokenUsage(all, 0, null, NOW)
    expect(agg.kpis.total).toBe(30)
    expect(agg.windowLabel).toBe('全部时间')
  })

  it('环比对齐「上一个等长窗口」，没有上一窗口时为 null', () => {
    const all = [
      rec(TODAY0, 150),
      // 前 7 天窗口内
      rec(TODAY0 - 7 * DAY, 100),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.kpis.totalDelta).toBeCloseTo(50, 5)

    const onlyToday = [rec(TODAY0, 150)]
    expect(aggregateTokenUsage(onlyToday, 7, null, NOW).kpis.totalDelta).toBeNull()
  })

  it('按来源过滤后只统计该来源', () => {
    const all = [
      rec(TODAY0, 100, { source: 'workbuddy' }),
      rec(TODAY0, 300, { source: 'codex' as TokenSourceId, sessionId: 's2' }),
    ]
    const agg = aggregateTokenUsage(all, 7, 'codex', NOW)
    expect(agg.kpis.total).toBe(300)
    expect(agg.kpis.sessions).toBe(1)
  })

  it('热力图按天×小时聚合，同小时累加', () => {
    const hour = new Date(NOW).getHours()
    const all = [rec(NOW, 10), rec(NOW, 5)]
    const agg = aggregateTokenUsage(all, 1, null, NOW)
    expect(agg.heat.dates).toEqual([dayKey(NOW)])
    expect(agg.heat.matrix[0][hour]).toBe(15)
    expect(agg.heat.max).toBe(15)
    expect(agg.heat.dailyTotal[0]).toBe(15)
  })

  it('大小分布按桶计数并给出分位', () => {
    const all = [
      rec(TODAY0, 1000),
      rec(TODAY0, 3000),
      rec(TODAY0, 12000),
      rec(TODAY0, 250000),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.sizes.labels[0]).toBe('<2k')
    expect(agg.sizes.counts[0]).toBe(1) // 1k
    expect(agg.sizes.counts[1]).toBe(1) // 3k
    expect(agg.sizes.counts[3]).toBe(1) // 12k
    expect(agg.sizes.counts[7]).toBe(1) // 250k
    expect(agg.sizes.max).toBe(250000)
    expect(agg.sizes.n).toBe(4)
    // p50 取上中位数（第 2 个）
    expect(agg.sizes.p50).toBe(3000)
  })

  it('KPI 的主力模型与占比按 token 排序', () => {
    const all = [
      rec(TODAY0, 100, { model: 'a' }),
      rec(TODAY0, 300, { model: 'b' }),
      rec(TODAY0, 100, { model: 'b' }),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.kpis.topModel).toBe('b')
    expect(agg.kpis.topShare).toBeCloseTo(80, 5)
  })

  it('工作空间按 token 倒序并给占比', () => {
    const all = [
      rec(TODAY0, 100, { workspace: 'A' }),
      rec(TODAY0, 300, { workspace: 'B' }),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.workspaces.map((w) => w.name)).toEqual(['B', 'A'])
    expect(agg.workspaces[0].share).toBeCloseTo(75, 5)
  })

  it('每日明细覆盖 top 模型与合计', () => {
    const all = [rec(TODAY0, 100), rec(TODAY0, 200)]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.modelDaily.dates).toEqual([dayKey(TODAY0)])
    expect(agg.modelDaily.totals[0]).toBe(300)
    expect(agg.modelDaily.dayRequests[0]).toBe(2)
    expect(agg.modelDaily.grandTotal).toBe(300)
  })

  it('缓存命中率 = 缓存读取 / 输入', () => {
    const all = [rec(TODAY0, 100, { input: 100, cachedRead: 25 })]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.kpis.cacheRate).toBeCloseTo(0.25, 5)
  })

  it('空数据不报错且各项为 0', () => {
    const agg = aggregateTokenUsage([], 30, null, NOW)
    expect(agg.kpis.total).toBe(0)
    expect(agg.kpis.topModel).toBe('—')
    expect(agg.heat.max).toBe(1)
    expect(agg.sizes.n).toBe(0)
    expect(agg.workspaces).toEqual([])
  })

  it('费用按记账 / 估算累加，并统计无价目条数', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 1000, { costUsd: 0.5, costKind: 'billing' }),
      rec(TODAY0 + 7_200_000, 2000, { costUsd: 0.25, costKind: 'estimate' }),
      rec(TODAY0 + 9_000_000, 3000, { costUsd: 0, costKind: 'none' }),
    ]
    const agg = aggregateTokenUsage(all, 1, null, NOW)
    expect(agg.kpis.costUsd).toBeCloseTo(0.75, 6)
    expect(agg.kpis.unpriced).toBe(1)
    expect(agg.modelDaily.grandCost).toBeCloseTo(0.75, 6)
    expect(agg.modelDaily.dayCost[0]).toBeCloseTo(0.75, 6)
  })

  it('供应商维度：有 provider 用它，没有的回落来源名', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 100, { provider: 'DeepSeek' }),
      rec(TODAY0 + 3_600_000, 50, { provider: 'DeepSeek' }),
      rec(TODAY0 + 3_600_000, 25, { source: 'codex' }),
    ]
    const agg = aggregateTokenUsage(all, 1, null, NOW)
    const byName = Object.fromEntries(agg.providers.map((p) => [p.name, p.tokens]))
    expect(byName.DeepSeek).toBe(150)
    expect(byName.Codex).toBe(25)
    expect(agg.providers[0].name).toBe('DeepSeek')
  })

  it('每日趋势补齐日历天，没有调用的那天是 0', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 100),
      // 第 4 天前有记录，中间几天留空
      rec(TODAY0 - 4 * DAY + 3_600_000, 300),
    ]
    const agg = aggregateTokenUsage(all, 7, null, NOW)
    expect(agg.daily.dates).toHaveLength(7)
    expect(agg.daily.tokens).toHaveLength(7)
    // 末位是今天
    expect(agg.daily.dates[6]).toBe(dayKey(TODAY0))
    expect(agg.daily.tokens[6]).toBe(100)
    expect(agg.daily.tokens[2]).toBe(300)
    expect(agg.daily.tokens[3]).toBe(0)
    expect(agg.daily.requests[6]).toBe(1)
  })

  it('缓存命中率按天算，没有输入的天记 0 而不是 NaN', () => {
    const all = [rec(TODAY0 + 3_600_000, 1000, { input: 1000, cachedRead: 600 })]
    const agg = aggregateTokenUsage(all, 3, null, NOW)
    expect(agg.daily.cacheRate[2]).toBeCloseTo(0.6, 6)
    expect(agg.daily.cacheRate[0]).toBe(0)
    expect(Number.isNaN(agg.daily.cacheRate[0])).toBe(false)
  })

  it('构成把缓存从 input 里拆出去，四项相加等于总量', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 1000, {
        input: 800,
        cachedRead: 300,
        cacheWrite: 120,
        output: 200,
        reasoning: 40,
      }),
      rec(TODAY0 + 7_200_000, 500, {
        input: 400,
        cachedRead: 100,
        cacheWrite: 0,
        output: 100,
        reasoning: 0,
      }),
    ]
    const agg = aggregateTokenUsage(all, 1, null, NOW)
    expect(agg.composition.cacheRead).toBe(400)
    expect(agg.composition.cacheWrite).toBe(120)
    expect(agg.composition.output).toBe(300)
    // input 含缓存读取：1200 - 400
    expect(agg.composition.input).toBe(800)
    expect(agg.composition.reasoning).toBe(40)
    const sum =
      agg.composition.input +
      agg.composition.cacheRead +
      agg.composition.cacheWrite +
      agg.composition.output
    expect(sum).toBe(1620)
  })

  it('来源维度按客户端聚合，标签用中文名', () => {
    const all = [
      rec(TODAY0 + 3_600_000, 100, { source: 'workbuddy' }),
      rec(TODAY0 + 3_600_000, 60, { source: 'claude' }),
      rec(TODAY0 + 3_600_000, 40, { source: 'capture' }),
    ]
    const agg = aggregateTokenUsage(all, 1, null, NOW)
    const byName = Object.fromEntries(agg.sources.map((s) => [s.name, s.tokens]))
    expect(byName.WorkBuddy).toBe(100)
    expect(byName.Claude).toBe(60)
    expect(byName['抓包观测']).toBe(40)
    expect(agg.sources[0].name).toBe('WorkBuddy')
    expect(agg.sources[0].share).toBeCloseTo(50, 6)
  })
})
