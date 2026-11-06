import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useWorkspaceStore } from '@/stores/workspace'
import type { HttpResult } from '@/types/models'

function httpResult(overrides: Partial<HttpResult> = {}): HttpResult {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 12,
    sizeBytes: 2,
    headers: { 'content-type': 'text/plain' },
    body: 'ok',
    truncated: false,
    error: null,
    bodyKind: 'text',
    bodyFile: null,
    bodyBytes: 2,
    rawBytes: 2,
    previewBase64: null,
    timings: null,
    httpVersion: 'HTTP/1.1',
    redirects: [],
    retries: [],
    ignoredHeaders: [],
    cancelled: false,
    ...overrides,
  }
}

describe('workspace store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('保存后能按接口 id 取回，并带上默认视图状态', () => {
    const workspace = useWorkspaceStore()
    workspace.save(1, { result: httpResult() })
    const snapshot = workspace.get(1)
    expect(snapshot).not.toBeNull()
    expect(snapshot?.result?.status).toBe(200)
    expect(snapshot?.responseTab).toBe('body')
    expect(snapshot?.bodyView).toBe('pretty')
    expect(snapshot?.activeTab).toBe('params')
    expect(snapshot?.resultSource).toBe('none')
    expect(workspace.get(2)).toBeNull()
  })

  it('增量保存只覆盖传入字段，未传字段沿用上一次', () => {
    const workspace = useWorkspaceStore()
    workspace.save(1, { responseTab: 'tests', bodyView: 'raw', result: httpResult() })
    workspace.save(1, { turn: { logs: [], assertions: [], error: 'boom', ms: 9 } })
    const snapshot = workspace.get(1)!
    expect(snapshot.responseTab).toBe('tests')
    expect(snapshot.bodyView).toBe('raw')
    expect(snapshot.result?.status).toBe(200)
    expect(snapshot.turn.error).toBe('boom')
    expect(snapshot.turn.ms).toBe(9)
  })

  it('expandedRunId 显式传 null 时可以被清空', () => {
    const workspace = useWorkspaceStore()
    workspace.save(1, { expandedRunId: 7, diffBaseRunId: 8 })
    workspace.save(1, { expandedRunId: null, diffBaseRunId: null })
    const snapshot = workspace.get(1)!
    expect(snapshot.expandedRunId).toBeNull()
    expect(snapshot.diffBaseRunId).toBeNull()
  })

  it('每次保存都会推进 stamp', () => {
    const workspace = useWorkspaceStore()
    workspace.save(1, {})
    const first = workspace.get(1)!.stamp
    workspace.save(1, {})
    const second = workspace.get(1)!.stamp
    expect(second).toBeGreaterThan(first)
  })

  it('只保留最近 5 个接口的现场', () => {
    vi.useFakeTimers()
    try {
      const workspace = useWorkspaceStore()
      for (let id = 1; id <= 6; id++) {
        vi.setSystemTime(1000 + id)
        workspace.save(id, { result: httpResult({ body: `body-${id}` }) })
      }
      expect(workspace.get(1)).toBeNull()
      expect(workspace.get(2)).not.toBeNull()
      expect(workspace.get(6)).not.toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('总字节超限时优先保留最新的现场', () => {
    vi.useFakeTimers()
    try {
      const workspace = useWorkspaceStore()
      const big = 'x'.repeat(20 * 1024 * 1024)
      vi.setSystemTime(1000)
      workspace.save(1, { result: httpResult({ body: big }) })
      vi.setSystemTime(2000)
      workspace.save(2, { result: httpResult({ body: big }) })
      expect(workspace.get(1)).toBeNull()
      expect(workspace.get(2)).not.toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('超过 32MB 的响应不缓存正文，只保留状态与头信息', () => {
    const workspace = useWorkspaceStore()
    const huge = 'x'.repeat(33 * 1024 * 1024)
    workspace.save(1, { result: httpResult({ body: huge, sizeBytes: huge.length }) })
    const snapshot = workspace.get(1)!
    expect(snapshot.result?.body).toBe('')
    expect(snapshot.result?.truncated).toBe(true)
    expect(snapshot.result?.headers['content-type']).toBe('text/plain')
    expect(snapshot.result?.bodyOmitted).toBe(true)
  })

  it('在途状态按接口记录，可查询和清除', () => {
    const workspace = useWorkspaceStore()
    expect(workspace.isInflight(1)).toBe(false)
    workspace.markInflight(1, 'req-1')
    expect(workspace.isInflight(1)).toBe(true)
    expect(workspace.inflightOf(1)?.requestKey).toBe('req-1')
    workspace.clearInflight(1)
    expect(workspace.isInflight(1)).toBe(false)
  })

  it('删除接口时同步清掉现场与在途状态', () => {
    const workspace = useWorkspaceStore()
    workspace.save(3, { result: httpResult() })
    workspace.markInflight(3, 'req-3')
    workspace.forget(3)
    expect(workspace.get(3)).toBeNull()
    expect(workspace.isInflight(3)).toBe(false)
  })
})
