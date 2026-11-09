import { describe, expect, it } from 'vitest'
import { budgetState } from '@/utils/aiBudget'

describe('budgetState', () => {
  it('未设置预算时不限制', () => {
    const state = budgetState(12, { monthlyBudget: 0 })
    expect(state.level).toBe('off')
    expect(state.remaining).toBe(Infinity)
  })

  it('未达到 80% 时正常', () => {
    const state = budgetState(3, { monthlyBudget: 10, budgetAlert: true })
    expect(state.level).toBe('ok')
    expect(state.ratio).toBeCloseTo(0.3)
    expect(state.remaining).toBeCloseTo(7)
  })

  it('达到 80% 触发提醒', () => {
    expect(budgetState(8, { monthlyBudget: 10, budgetAlert: true }).level).toBe('warn')
    expect(budgetState(8, { monthlyBudget: 10, budgetAlert: false }).level).toBe('ok')
  })

  it('用尽但未开启硬停时只提醒', () => {
    expect(budgetState(11, { monthlyBudget: 10, budgetHardStop: false }).level).toBe('warn')
  })

  it('用尽且开启硬停时阻断', () => {
    expect(budgetState(11, { monthlyBudget: 10, budgetHardStop: true }).level).toBe('blocked')
  })

  it('非法用量按 0 处理', () => {
    expect(budgetState(Number.NaN, { monthlyBudget: 10 }).ratio).toBe(0)
  })
})
