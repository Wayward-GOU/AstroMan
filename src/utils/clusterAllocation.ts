import type { ClusterNodeInfo, ClusterNodeRole } from '@/types/cluster'

/**
 * 集群并发分配的前端预览算法（v1.1.2）
 *
 * 与后端 `allocate_concurrency_capped` 同口径：先保留每节点最小并发，
 * 剩余按权重分配，超出上限的部分按小数余数依次补齐。这里保持纯计算，
 * 不依赖 Vue / DOM，便于单测与在集群页复用。
 */

export type AllocationMode = 'auto' | 'even' | 'custom'

export interface AllocationNode {
  id: string
  role: ClusterNodeRole
  /** 快照里记录的权重，custom 模式未改动时作为初值 */
  weight: number
  /** 从节点能力上报的本机预算；主节点忽略 */
  capacity?: { machineLimit?: number | null } | null
}

export interface NodeLimitInput {
  min?: number | null
  max?: number | null
}

export interface AllocationItem {
  id: string
  weight: number
  min: number
  max: number
}

function numberOr(value: unknown, fallback: number): number {
  const num = Number(value)
  return Number.isFinite(num) ? num : fallback
}

/** 节点本机并发预算：主节点取会话预算，从节点取能力上报 */
export function nodeBudgetCap(
  node: Pick<AllocationNode, 'role' | 'capacity'>,
  masterBudget?: number | null,
  fallback = 512,
): number {
  if (node.role === 'master') return Math.max(1, numberOr(masterBudget, fallback))
  return Math.max(1, numberOr(node.capacity?.machineLimit, fallback))
}

/** custom 模式下的每节点最小/最大并发，自动收敛到本机预算 */
export function nodeLimitRange(
  node: Pick<AllocationNode, 'id' | 'role' | 'capacity'>,
  masterBudget: number | null | undefined,
  limits: Record<string, NodeLimitInput | undefined>,
  fallback = 512,
): { min: number; max: number } {
  const cap = nodeBudgetCap(node, masterBudget, fallback)
  const raw = limits[node.id]
  const min = Math.min(cap, Math.max(1, numberOr(raw?.min, 1)))
  const max = Math.max(min, Math.min(cap, numberOr(raw?.max, cap)))
  return { min, max }
}

/**
 * 按分配模式生成每节点权重与范围
 *
 * - auto：权重取本机预算，强机多拿并发；
 * - even：权重统一为 1，按节点数平均，余数依次补齐；
 * - custom：使用用户填写的权重（0.1~10）与 min/max。
 */
export function allocationItems(
  nodes: AllocationNode[],
  mode: AllocationMode,
  masterBudget: number | null | undefined,
  weights: Record<string, number>,
  limits: Record<string, NodeLimitInput | undefined>,
  fallback = 512,
): AllocationItem[] {
  return nodes.map((node) => {
    const cap = nodeBudgetCap(node, masterBudget, fallback)
    const range =
      mode === 'custom'
        ? nodeLimitRange(node, masterBudget, limits, fallback)
        : { min: 1, max: cap }
    const weight =
      mode === 'auto'
        ? cap
        : mode === 'even'
          ? 1
          : Math.max(0.1, numberOr(weights[node.id] ?? node.weight, 1))
    return { id: node.id, weight, ...range }
  })
}

/** 配对阶段的并发预览：先满足下限，剩余按权重分配，余数按小数部分补齐 */
export function planAllocation(
  items: AllocationItem[],
  totalConcurrent: number | null | undefined,
): Record<string, number> {
  if (!items.length) return {}
  const total = Math.max(1, numberOr(totalConcurrent, items.length))
  const minSum = items.reduce((acc, item) => acc + item.min, 0)
  if (minSum > total) {
    return Object.fromEntries(items.map((item) => [item.id, item.min]))
  }
  const counts = items.map((item) => item.min)
  let remaining = total - minSum
  if (remaining > 0) {
    const sum = items.reduce((acc, item) => acc + item.weight, 0) || 1
    const pool = remaining
    const fractions = items.map((item, index) => {
      const room = item.max - item.min
      const share = (pool * item.weight) / sum
      const add = Math.min(Math.floor(share), room)
      counts[index] += add
      remaining -= add
      return { index, fraction: share - Math.floor(share), room: room - add }
    })
    fractions.sort((a, b) => b.fraction - a.fraction)
    while (remaining > 0) {
      let progressed = false
      for (const item of fractions) {
        if (remaining === 0) break
        if (item.room > 0) {
          counts[item.index] += 1
          item.room -= 1
          remaining -= 1
          progressed = true
        }
      }
      if (!progressed) break
    }
  }
  return Object.fromEntries(items.map((item, index) => [item.id, counts[index]]))
}

/** 分配结果提示：下限/上限与总并发冲突时直接给出原因 */
export function allocationProblem(
  items: AllocationItem[],
  totalConcurrent: number | null | undefined,
): string {
  if (!items.length) return ''
  const total = Math.max(1, numberOr(totalConcurrent, items.length))
  const minSum = items.reduce((acc, item) => acc + item.min, 0)
  if (minSum > total) return `各节点最小并发之和 ${minSum} 超过总并发 ${total}`
  const maxSum = items.reduce((acc, item) => acc + item.max, 0)
  if (maxSum < total) return `各节点并发上限之和 ${maxSum} 小于总并发 ${total}`
  return ''
}

/** 分配模式文案：页面标题与下发成功提示共用 */
export const ALLOCATION_MODE_TEXT: Record<AllocationMode, string> = {
  auto: '能力分配（按本机预算）',
  even: '平均分配（余数依次补齐）',
  custom: '自定义权重与范围',
}

/** 供组件把快照节点转成分配输入，避免各处重复取字段 */
export function toAllocationNode(node: ClusterNodeInfo): AllocationNode {
  return {
    id: node.id,
    role: node.role,
    weight: node.weight,
    capacity: node.capacity ?? null,
  }
}
