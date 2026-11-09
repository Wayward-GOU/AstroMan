import { describe, expect, it } from 'vitest'
import {
  USD_TO_CNY,
  estimateCost,
  fmtUnit,
  priceOf,
} from '@/utils/aiPricing'

describe('priceOf', () => {
  it('未填自定义单价时走内置价格表', () => {
    const rule = priceOf('gpt-4o-mini', { inputPrice: 0, outputPrice: 0, currency: 'USD' })
    expect(rule.custom).toBe(false)
    expect(rule.input).toBeCloseTo(0.15)
    expect(rule.output).toBeCloseTo(0.6)
    expect(rule.currency).toBe('USD')
  })

  it('美元自定义单价优先于内置价格表', () => {
    const rule = priceOf('gpt-4o', { inputPrice: 1.2, outputPrice: 4.8, currency: 'USD' })
    expect(rule.custom).toBe(true)
    expect(rule.input).toBeCloseTo(1.2)
    expect(rule.output).toBeCloseTo(4.8)
    expect(rule.listInput).toBeCloseTo(1.2)
    expect(rule.fallback).toBe(false)
  })

  it('人民币单价会换算成美元并按人民币展示原价', () => {
    const rule = priceOf('deepseek-chat', { inputPrice: 7.2, outputPrice: 21.6, currency: 'CNY' })
    expect(rule.currency).toBe('CNY')
    expect(rule.input).toBeCloseTo(1)
    expect(rule.output).toBeCloseTo(3)
    expect(rule.listInput).toBeCloseTo(7.2)
    expect(rule.listOutput).toBeCloseTo(21.6)
  })

  it('只填一边时另一边沿用内置价（人民币报价按汇率补齐）', () => {
    const rule = priceOf('deepseek-chat', { inputPrice: 7.2, outputPrice: 0, currency: 'CNY' })
    expect(rule.input).toBeCloseTo(1)
    // 内置 deepseek-chat 输出 1.1 美元，折人民币后仍应还原成 1.1 美元
    expect(rule.output).toBeCloseTo(1.1)
    expect(rule.listOutput).toBeCloseTo(1.1 * USD_TO_CNY)
  })

  it('无法识别的模型在未填单价时标记为兜底估算', () => {
    expect(priceOf('my-private-model').fallback).toBe(true)
    expect(priceOf('my-private-model', { inputPrice: 2, outputPrice: 4 }).fallback).toBe(false)
  })
})

describe('estimateCost', () => {
  it('使用自定义单价计算成本', () => {
    const cost = estimateCost('gpt-4o', 1_000_000, 500_000, {
      inputPrice: 1,
      outputPrice: 2,
      currency: 'USD',
    })
    expect(cost.inputCost).toBeCloseTo(1)
    expect(cost.outputCost).toBeCloseTo(1)
    expect(cost.total).toBeCloseTo(2)
  })

  it('人民币单价换算后成本与美元一致', () => {
    const cost = estimateCost('gpt-4o', 1_000_000, 0, {
      inputPrice: USD_TO_CNY,
      outputPrice: 0,
      currency: 'CNY',
    })
    expect(cost.total).toBeCloseTo(1)
  })
})

describe('fmtUnit', () => {
  it('按币种展示单价', () => {
    expect(fmtUnit(2)).toBe('$2.00/M')
    expect(fmtUnit(14.4, 'CNY')).toBe('¥14.40/M')
  })
})
