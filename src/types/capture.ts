export type CaptureState = 'idle' | 'starting' | 'running' | 'stopping' | 'error'

export interface CaptureStatus {
  state: CaptureState
  port: number
  bindAddr: string
  sessionId: number | null
  startedAt: number
  flowCount: number
  dropped: number
  caInstalled: boolean
  sysproxyOn: boolean
  upstreamProxy: string | null
  lastError: string | null
}

export interface CaptureCaInfo {
  exists: boolean
  /** SHA-256，冒号分隔 */
  fingerprint: string
  /** SHA-1，Windows 证书管理器里的「指纹」 */
  thumbprint: string
  createdAt: number
  expiresAt: number
  installed: boolean
}

export interface CaptureHeader {
  name: string
  value: string
}

export interface CaptureBody {
  /** 原始 body 字节数 */
  size: number
  /** 超过 2 MB 时只保留了前缀 */
  truncated: boolean
  binary: boolean
  /** 文本 body；二进制或压缩内容为 null */
  text: string | null
  /** 图片等可预览二进制；其余为 null */
  base64: string | null
  contentType: string
}

export interface CaptureFlow {
  id: number
  startedAt: number
  method: string
  scheme: string
  host: string
  path: string
  httpVersion: string
  status: number | null
  durationMs: number
  requestSize: number
  responseSize: number
  contentType: string
  clientAddr: string
  error: string | null
  /** 响应由本地映射直接返回 */
  mapped: boolean
  /** 断点改过请求或响应 */
  modified: boolean
  /** 远程映射真正发往的上游地址；未命中时为 null */
  remote: string | null
}

export interface CaptureFlowDetail extends CaptureFlow {
  requestHeaders: CaptureHeader[]
  responseHeaders: CaptureHeader[]
  requestBody: CaptureBody
  responseBody: CaptureBody
}

export interface CaptureStartOptions {
  port?: number
  bindAddr?: string
  sslHosts?: string[]
  ignorePatterns?: string[]
  captureStatic?: boolean
  upstreamProxy?: string | null
}

/** 重发时可覆盖的参数；不传的字段沿用原请求 */
export interface CaptureRepeatPayload {
  id: number
  method?: string
  url?: string
  headers?: CaptureHeader[]
  body?: string
}

/** 重发结果：录制中会生成新流量，未录制时只在弹窗展示响应 */
export interface CaptureRepeatResult {
  recorded: boolean
  status: number
  statusText: string
  durationMs: number
  url: string
  headers: CaptureHeader[]
  body: CaptureBody
}

/** 断点阶段 */
export type BreakpointPhase = 'request' | 'response' | 'both'

export interface BreakpointRule {
  id: number
  enabled: boolean
  name: string
  /** URL 匹配：含 `*` 走通配，否则按包含；只写 host/path 也能命中 */
  pattern: string
  /** 空串或 any 表示不限方法 */
  method: string
  phase: BreakpointPhase
}

export type MapSource = 'file' | 'inline'

export interface MapRule {
  id: number
  enabled: boolean
  name: string
  pattern: string
  method: string
  source: MapSource
  /** source=file 时的本地路径，每次命中实时读取 */
  filePath: string
  status: number
  /** 留空按扩展名或内容猜 */
  contentType: string
  /** source=inline 时的响应正文 */
  body: string
  headers: CaptureHeader[]
}

/** 远程映射：命中后把请求改发到 target，列表里仍显示原地址 */
export interface MapRemoteRule {
  id: number
  enabled: boolean
  name: string
  pattern: string
  method: string
  /** 目标地址，可只写 host:port，也可写完整 URL */
  target: string
  /** 是否保留原请求的路径与查询串（默认 true） */
  keepPath: boolean
}

export interface WeakNetwork {
  enabled: boolean
  /** 只记录 UI 选中的预设名，参数仍可单独改 */
  preset: string
  latencyMs: number
  jitterMs: number
  /** 下行带宽（kbps），0 表示不限速 */
  bandwidthKbps: number
  /** 0~100，命中的请求直接返回 504 */
  lossPercent: number
  /** 作用范围，空表示全部流量 */
  patterns: string[]
}

/** 一条被断点挂起的流量 */
export interface PendingBreakpoint {
  pauseId: number
  phase: 'request' | 'response'
  rule: string
  method: string
  url: string
  httpVersion: string
  status: number | null
  requestHeaders: CaptureHeader[]
  requestBody: CaptureBody
  /** false 表示请求体是二进制或过大，只能原样放行 */
  requestEditable: boolean
  responseHeaders: CaptureHeader[]
  responseBody: CaptureBody
  /** false 表示响应体未缓冲（流式或过大），只能改状态码与响应头 */
  responseEditable: boolean
  createdAt: number
}

export interface InterceptView {
  breakpointsEnabled: boolean
  breakpointsActive: boolean
  mapEnabled: boolean
  mapActive: boolean
  remoteEnabled: boolean
  remoteActive: boolean
  weak: WeakNetwork
  breakpointRules: BreakpointRule[]
  mapRules: MapRule[]
  remoteRules: MapRemoteRule[]
  pausedTotal: number
  pendingCount: number
  maxPending: number
  timeoutSeconds: number
  pending: PendingBreakpoint[]
}

/** 断点里改写入参 */
export interface RequestEdit {
  method?: string
  url?: string
  headers?: CaptureHeader[]
  body?: string
}

/** 断点里改写返参 */
export interface ResponseEdit {
  status?: number
  headers?: CaptureHeader[]
  body?: string
}
