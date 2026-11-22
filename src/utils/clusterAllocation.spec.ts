import { describe, expect, it } from 'vitest'
import {
  ALLOCATION_MODE_TEXT,
  allocationItems,
  allocationProblem,
  nodeBudgetCap,
  nodeLimitRange,
  planAllocation,
  toAllocationNode,
  type AllocationNode,
} from './clusterAllocation'

function node(
  id: string,
  role: 'master' | 'slave',
  machineLimit?: number | null,
  weight = 1,
): AllocationNode {
  return {
    id,
    role,
    weight,
    capacity: role === 'slave' ? { machineLimit: machineLimit ?? null } : null,
  }
}

describe('clusterAllocation', () => {
  it('节点预算：主节点取会话预算，从节点取能力上报，缺失时兜底', () => {
    expect(nodeBudgetCap(node('m', 'master'), 3000)).toBe(3000)
    expect(nodeBudgetCap(node('m', 'master'), undefined)).toBe(512)
    expect(nodeBudgetCap(node('s', 'slave', 1200), 3000)).toBe(1200)
    const slaveWithoutCapacity: AllocationNode = { id: 's', role: 'slave', weight: 1 }
    expect(nodeBudgetCap(slaveWithoutCapacity, 3000)).toBe(512)
    // 能力已上报但预算为 0 / null 时收敛到 1，避免分配出 0 并发
    expect(nodeBudgetCap(node('s', 'slave', null), 3000)).toBe(1)
    expect(nodeBudgetCap(node('s', 'slave', 0), 3000)).toBe(1)
  })

  it('每节点范围收敛到本机预算，最小值不超过最大值', () => {
    const target = node('s', 'slave', 600)
    expect(nodeLimitRange(target, null, {})).toEqual({ min: 1, max: 600 })
    expect(nodeLimitRange(target, null, { s: { min: 800, max: 900 } })).toEqual({
      min: 600,
      max: 600,
    })
    expect(nodeLimitRange(target, null, { s: { min: 100, max: 200 } })).toEqual({
      min: 100,
      max: 200,
    })
  })

  it('auto 模式按本机预算做权重，强机多拿并发', () => {
    const items = allocationItems(
      [node('m', 'master'), node('s', 'slave', 500)],
      'auto',
      1000,
      {},
      {},
    )
    expect(items.map((item) => item.weight)).toEqual([1000, 500])
    expect(planAllocation(items, 1200)).toEqual({ m: 800, s: 400 })
  })

  it('even 模式权重统一为 1，平均分配并依次补齐余数', () => {
    const items = allocationItems(
      [node('m', 'master'), node('s', 'slave', 500)],
      'even',
      1000,
      {},
      {},
    )
    expect(items.map((item) => item.weight)).toEqual([1, 1])
    expect(planAllocation(items, 500)).toEqual({ m: 250, s: 250 })
    // 奇数总并发：余数按节点顺序补齐，不会丢失
    expect(planAllocation(items, 501)).toEqual({ m: 251, s: 250 })
  })

  it('custom 模式使用自定义权重与 min/max，并受节点预算约束', () => {
    const nodes = [node('m', 'master'), node('s', 'slave', 500)]
    const items = allocationItems(nodes, 'custom', 1000, { m: 3, s: 1 }, {})
    expect(planAllocation(items, 400)).toEqual({ m: 300, s: 100 })

    // 从节点上限 50：超出部分回填给主节点
    const limited = allocationItems(nodes, 'custom', 1000, { m: 3, s: 1 }, { s: { min: 1, max: 50 } })
    expect(planAllocation(limited, 400)).toEqual({ m: 350, s: 50 })

    // 上限超过预算时按预算截断
    const clamped = allocationItems(nodes, 'custom', 1000, { m: 1, s: 1 }, { s: { max: 9999 } })
    expect(clamped.find((item) => item.id === 's')?.max).toBe(500)
  })

  it('最小并发之和超过总并发时按最小并发保守分配并给出原因', () => {
    const items = allocationItems(
      [node('m', 'master'), node('s', 'slave', 500)],
      'custom',
      1000,
      { m: 1, s: 1 },
      { m: { min: 6 }, s: { min: 6 } },
    )
    expect(planAllocation(items, 8)).toEqual({ m: 6, s: 6 })
    expect(allocationProblem(items, 8)).toBe('各节点最小并发之和 12 超过总并发 8')
  })

  it('上限之和小于总并发时提示无法分配', () => {
    const items = allocationItems(
      [node('m', 'master'), node('s', 'slave', 100)],
      'custom',
      1000,
      { m: 1, s: 1 },
      { m: { max: 100 }, s: { max: 100 } },
    )
    expect(allocationProblem(items, 500)).toBe('各节点并发上限之和 200 小于总并发 500')
    expect(allocationProblem(items, 150)).toBe('')
  })

  it('总并发缺失时按节点数兜底，快照节点可直接转换', () => {
    const items = allocationItems([node('m', 'master'), node('s', 'slave', 500)], 'even', null, {}, {})
    expect(planAllocation(items, null)).toEqual({ m: 1, s: 1 })
    expect(
      toAllocationNode({
        id: 's1',
        role: 'slave',
        weight: 2,
        capacity: { cores: 8, memGb: 16, machineLimit: 900 },
      } as never),
    ).toEqual({ id: 's1', role: 'slave', weight: 2, capacity: { cores: 8, memGb: 16, machineLimit: 900 } })
  })

  it('三种分配模式的文案保持一致', () => {
    expect(ALLOCATION_MODE_TEXT.auto).toBe('能力分配（按本机预算）')
    expect(ALLOCATION_MODE_TEXT.even).toBe('平均分配（余数依次补齐）')
    expect(ALLOCATION_MODE_TEXT.custom).toBe('自定义权重与范围')
  })
})
