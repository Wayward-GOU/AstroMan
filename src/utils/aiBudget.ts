/**
 * 月度预算判定（AI-06）：只做纯计算，金额统一按美元。
 * 默认只提醒；只有用户显式开启 `budgetHardStop` 才阻断发送。
 */

export interface BudgetSettings {
  /** 月度预算（美元）；0 / 空表示不限制 */
  monthlyBudget?: number | null
  /** 达到 80% 时提醒；未配置按开启处理 */
  budgetAlert?: boolean | null
  /** 预算用尽后硬停止；默认只提醒 */
  budgetHardStop?: boolean | null
}

export type BudgetLevel = 'off' | 'ok' | 'warn' | 'blocked'

export interface BudgetState {
  level: BudgetLevel
  budget: number
  used: number
  ratio: number
  remaining: number
}

const EMPTY: BudgetState = { level: 'off', budget: 0, used: 0, ratio: 0, remaining: Infinity }

export function budgetState(usedUsd: number, settings?: BudgetSettings | null): BudgetState {
  const budget = Number(settings?.monthlyBudget)
  if (!Number.isFinite(budget) || budget <= 0) return { ...EMPTY }
  const used = Number.isFinite(usedUsd) && usedUsd > 0 ? usedUsd : 0
  const ratio = used / budget
  const remaining = Math.max(0, budget - used)
  if (ratio >= 1) {
    return {
      level: settings?.budgetHardStop ? 'blocked' : 'warn',
      budget,
      used,
      ratio,
      remaining,
    }
  }
  if (ratio >= 0.8 && settings?.budgetAlert !== false) {
    return { level: 'warn', budget, used, ratio, remaining }
  }
  return { level: 'ok', budget, used, ratio, remaining }
}

/** 当前自然月的起点（本地时区零点） */
export function monthStartMs(now = Date.now()): number {
  const date = new Date(now)
  date.setDate(1)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}
