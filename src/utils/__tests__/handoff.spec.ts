import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  clearHandoff,
  peekHandoff,
  setHandoff,
  takeHandoff,
  type HangzaiHandoff,
} from '@/stores/handoff'

const payload: Omit<HangzaiHandoff, 'createdAt'> = {
  source: 'request',
  title: 'POST /api/login',
  prompt: '请分析这次接口调用',
  projectId: 1,
  requestId: 2,
}

describe('hangzai handoff', () => {
  beforeEach(() => {
    clearHandoff()
    vi.useFakeTimers()
    vi.setSystemTime(1_000_000)
  })

  afterEach(() => {
    clearHandoff()
    vi.useRealTimers()
  })

  it('写入后可以读取，take 之后消费掉', () => {
    setHandoff(payload)
    expect(peekHandoff()?.title).toBe('POST /api/login')
    expect(takeHandoff()?.requestId).toBe(2)
    expect(peekHandoff()).toBeNull()
  })

  it('超过 5 分钟自动失效', () => {
    setHandoff(payload)
    vi.setSystemTime(1_000_000 + 5 * 60_000 + 1)
    expect(peekHandoff()).toBeNull()
  })

  it('5 分钟内仍有效', () => {
    setHandoff(payload)
    vi.setSystemTime(1_000_000 + 4 * 60_000)
    expect(takeHandoff()?.source).toBe('request')
  })

  it('clear 可以主动丢弃页面现场', () => {
    setHandoff(payload)
    clearHandoff()
    expect(peekHandoff()).toBeNull()
  })
})
