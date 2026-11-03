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
