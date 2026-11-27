/**
 * Token 用量看板类型：与 `src-tauri/src/commands/token_usage.rs` 的
 * `#[serde(rename_all = "camelCase")]` 结构一一对应。
 *
 * 口径：只来自各客户端写在盘上的真实 usage 字段，不做估算。
 */

export type TokenSourceId =
  | 'workbuddy'
  | 'codex'
  | 'claude'
  | 'chatgpt'
  | 'ccswitch'
  | 'capture'

export const TOKEN_SOURCE_LABEL: Record<TokenSourceId, string> = {
  workbuddy: 'WorkBuddy',
  codex: 'Codex',
  claude: 'Claude',
  chatgpt: 'ChatGPT',
  ccswitch: 'CC Switch 代理',
  capture: '抓包观测',
}

/** 单次模型调用的标准化记录 */
export interface UsageRecord {
  /** 去重键：同一 messageId 在多文件中出现只计一次 */
  id: string
  /** epoch 毫秒 */
  ts: number
  source: TokenSourceId
  model: string
  /** 工作空间名（cwd 末段） */
  workspace: string
  workspacePath: string
  sessionId: string
  /** 输入 token（含缓存读取，与官方口径一致） */
  input: number
  output: number
  cachedRead: number
  cacheWrite: number
  /** 推理 token（已含在 output 内，仅用于展示） */
  reasoning: number
  total: number
  /** 官方逐请求实际积分，缺失为 0 */
  credit: number
  /** 该次调用费用（美元）：代理流水是账面值，本地日志按价目表估算 */
  costUsd: number
  /** 费用来源：billing 账面 / estimate 估算 / none 无价目 */
  costKind: 'billing' | 'estimate' | 'none'
  /** 供应商或中转名（只有 CC Switch 代理行有） */
  provider: string
}

export interface SourceStat {
  found: boolean
  path: string
  files: number
  requests: number
  tokens: number
}

export interface ScanStats {
  scannedAt: number
  durationMs: number
  files: number
  parseErrors: number
  deduped: number
  /** 全量费用（美元），与所选时间窗口无关 */
  costUsd: number
  /** 没有价目、未计入费用的记录数 */
  unpriced: number
  sources: Record<TokenSourceId, SourceStat>
}

export interface ScanResult {
  records: UsageRecord[]
  stats: ScanStats
}

export interface RootInfo {
  source: TokenSourceId
  dir: string
  exists: boolean
}

/* ---------------- 数据源体检 ---------------- */

export type ClientKind = 'cli' | 'desktop' | 'ide' | 'proxy'

/**
 * covered  已接入：本机有日志且解析到真实 usage
 * idle     已接入但无调用：日志目录存在、记录为 0
 * blind    读不到：客户端装着、有本地数据，但没有 token 用量可解析
 * absent   未安装：所有探测路径都不存在
 */
export type ClientStatus = 'covered' | 'idle' | 'blind' | 'absent'

export interface ClientProbe {
  id: string
  name: string
  kind: ClientKind
  status: ClientStatus
  /** 可解析时对应的日志来源，读不到为 null */
  source: TokenSourceId | null
  paths: string[]
  requests: number
  tokens: number
  /** 本地数据体积 MB；未知为 null */
  dataMB: number | null
  detail: string
}

/** 始终无法被本机日志覆盖的调用方式（不可探测，固定文案） */
export const UNOBSERVABLE: { name: string; detail: string }[] = [
  { name: '浏览器内的网页版', detail: '在浏览器里用 chatgpt.com / claude.ai 等，不产生本地日志' },
  { name: '第三方套壳与 API 中转', detail: '非官方客户端，不写 token 用量' },
  { name: '手机、平板等其它设备', detail: '不在本机，天然无法统计' },
]
