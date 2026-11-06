import { defineStore } from 'pinia'
import type { HttpResult } from '@/types/models'
import type { ScriptAssertion, ScriptLog } from '@/utils/scriptRunner'

export interface RequestTurnState {
  logs: ScriptLog[]
  assertions: ScriptAssertion[]
  error: string
  ms: number
}

export interface RequestWorkspaceSnapshot {
  requestId: number
  result: HttpResult | null
  turn: RequestTurnState
  responseTab: string
  bodyView: 'pretty' | 'raw'
  activeTab: string
  bodyAbsPath: string
  bodyPreviewError: string
  expandedRunId: number | null
  diffOpen: boolean
  diffBaseRunId: number | null
  diffCustomText: string
  diffUseCustom: boolean
  /** live = 本次内存中的真实响应；run = 从运行记录有损恢复 */
  resultSource: 'live' | 'run' | 'none'
  restoredRunId?: number | null
  updatedAt: number
  /** 每次写入自增，页面 watch 它做增量回填 */
  stamp: number
}

interface Inflight {
  requestKey: string
  startedAt: number
}

const MAX_CACHED_REQUESTS = 5
const MAX_CACHED_BYTES = 32 * 1024 * 1024

function emptyTurn(): RequestTurnState {
  return { logs: [], assertions: [], error: '', ms: 0 }
}

/** 响应过大时不缓存正文，避免一次切换就把整份大 Body 留在内存里 */
function cacheableResult(result: HttpResult | null): HttpResult | null {
  if (!result) return result
  const bodyLength = result.body?.length ?? 0
  if (bodyLength <= MAX_CACHED_BYTES) return result
  return { ...result, body: '', truncated: true, bodyOmitted: true }
}

export const useWorkspaceStore = defineStore('workspace', {
  state: () => ({
    snapshots: {} as Record<number, RequestWorkspaceSnapshot>,
    inflight: {} as Record<number, Inflight>,
    seq: 0,
  }),
  actions: {
    save(requestId: number, patch: Partial<RequestWorkspaceSnapshot>) {
      if (!requestId) return
      const previous = this.snapshots[requestId]
      this.snapshots[requestId] = {
        requestId,
        result: patch.result === undefined ? previous?.result ?? null : cacheableResult(patch.result),
        turn: patch.turn ?? previous?.turn ?? emptyTurn(),
        responseTab: patch.responseTab ?? previous?.responseTab ?? 'body',
        bodyView: patch.bodyView ?? previous?.bodyView ?? 'pretty',
        activeTab: patch.activeTab ?? previous?.activeTab ?? 'params',
        bodyAbsPath: patch.bodyAbsPath ?? previous?.bodyAbsPath ?? '',
        bodyPreviewError: patch.bodyPreviewError ?? previous?.bodyPreviewError ?? '',
        expandedRunId:
          patch.expandedRunId === undefined
            ? previous?.expandedRunId ?? null
            : patch.expandedRunId,
        diffOpen: patch.diffOpen ?? previous?.diffOpen ?? false,
        diffBaseRunId:
          patch.diffBaseRunId === undefined
            ? previous?.diffBaseRunId ?? null
            : patch.diffBaseRunId,
        diffCustomText: patch.diffCustomText ?? previous?.diffCustomText ?? '',
        diffUseCustom: patch.diffUseCustom ?? previous?.diffUseCustom ?? false,
        resultSource: patch.resultSource ?? previous?.resultSource ?? 'none',
        restoredRunId:
          patch.restoredRunId === undefined ? previous?.restoredRunId : patch.restoredRunId,
        updatedAt: Date.now(),
        stamp: ++this.seq,
      }
      this.evict()
    },
    get(requestId: number): RequestWorkspaceSnapshot | null {
      return this.snapshots[requestId] ?? null
    },
    markInflight(requestId: number, requestKey: string) {
      if (!requestId) return
      this.inflight[requestId] = { requestKey, startedAt: Date.now() }
    },
    clearInflight(requestId: number) {
      delete this.inflight[requestId]
    },
    isInflight(requestId: number) {
      return Boolean(this.inflight[requestId])
    },
    inflightOf(requestId: number): Inflight | null {
      return this.inflight[requestId] ?? null
    },
    forget(requestId: number) {
      delete this.snapshots[requestId]
      delete this.inflight[requestId]
    },
    evict() {
      const entries = Object.values(this.snapshots).sort((a, b) => b.updatedAt - a.updatedAt)
      let total = 0
      const keep = new Set<number>()
      for (const item of entries) {
        const size = (item.result?.body?.length ?? 0) + (item.result?.headers ? 4096 : 0)
        if (keep.size >= MAX_CACHED_REQUESTS) break
        if (total + size > MAX_CACHED_BYTES && keep.size > 0) break
        total += size
        keep.add(item.requestId)
      }
      for (const key of Object.keys(this.snapshots)) {
        if (!keep.has(Number(key))) delete this.snapshots[Number(key)]
      }
    },
  },
})
