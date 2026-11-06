import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useHangzaiTaskStore } from '@/stores/hangzaiTask'

describe('hangzai task store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('记录任务阶段并能用阶段文案描述', () => {
    const task = useHangzaiTaskStore()
    task.update({ active: true, phase: 'tool', tool: 'request_get', round: 2 })
    expect(task.active).toBe(true)
    expect(task.phaseText).toBe('正在执行 request_get')
    task.update({ phase: 'confirm', awaitingConfirm: true })
    expect(task.phaseText).toBe('等待你确认一个写操作')
  })

  it('clear 会把任务镜像恢复初始状态', () => {
    const task = useHangzaiTaskStore()
    task.update({ active: true, runId: 'run-1', phase: 'streaming', canStop: true })
    task.clear()
    expect(task.active).toBe(false)
    expect(task.runId).toBeNull()
    expect(task.canStop).toBe(false)
    expect(task.awaitingConfirm).toBe(false)
  })

  it('停止入口绑定后可以从任务坞请求停止', async () => {
    const task = useHangzaiTaskStore()
    const stop = vi.fn(async () => undefined)
    task.bindStop(stop)
    await task.requestStop()
    expect(stop).toHaveBeenCalledTimes(1)
  })
})
