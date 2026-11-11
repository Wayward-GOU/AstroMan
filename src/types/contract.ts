/** 请求版本历史与契约快照（DB v10） */
import type { KeyValueItem } from './models'

/* ---------- 请求版本 ---------- */

/** 版本列表项，与后端 commands::revision::RevisionBrief 对应 */
export interface RevisionBrief {
  id: number
  requestId: number
  revision: number
  /** save = 保存 / 自动保存；restore = 回滚；import = 导入 */
  source: string
  /** 超限快照已截断，不能回滚 */
  truncated: boolean
  createdAt: number
  /** AI 写操作所属提问轮次；普通保存为 null（AI-09） */
  traceId?: string | null
}

/** 按 trace_id 聚合回滚的结果 */
export interface TraceRollbackResult {
  traceId: string
  restored: { requestId: number; revision: number; name: string }[]
  skipped: { requestId: number; revision: number; reason: string }[]
}

/** 版本快照内容：只回认证类型，不回凭据 */
export interface RevisionView {
  name: string
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  bodyMeta: string
  preScript: string
  postScript: string
  assertions: string
  authKind: string
  authConfigured: boolean
  requestEntity?: string | null
  responseEntity?: string | null
}

export interface RevisionDetail {
  brief: RevisionBrief
  snapshot: RevisionView
}

export interface RevisionChange {
  field: string
  /** added / removed / changed / type_changed / required_changed / description_changed */
  kind: string
  /** changed = 实质变化；warning = 仅说明变化 */
  level: string
  before: string
  after: string
}

export interface RevisionDiff {
  from: RevisionBrief
  to: RevisionBrief
  changes: RevisionChange[]
}

/* ---------- 契约快照 ---------- */

export interface ContractSnapshot {
  id: number
  projectId: number
  label: string
  requestCount: number
  source: string
  digest: string
  createdAt: number
}

export interface ContractChange {
  path: string
  method: string
  field: string
  kind: string
  message: string
  before?: unknown
  after?: unknown
}

export interface ContractSummary {
  breaking: number
  compatible: number
  warnings: number
  paths: number
  operations: number
}

export interface ContractDiff {
  breaking: ContractChange[]
  compatible: ContractChange[]
  warnings: ContractChange[]
  /** 无返回实体、响应侧不参与判定的接口（如 "GET /health"） */
  schemaMissing: string[]
  summary: ContractSummary
}

export interface ContractCheckResult {
  baseline: ContractSnapshot
  diff: ContractDiff
  generatedAt: number
}
