export interface Project {
  id: number
  name: string
  description?: string | null
  color?: string | null
  sortOrder: number
  createdAt: number
  updatedAt: number
}

export interface CollectionNode {
  id: number
  projectId: number
  parentId?: number | null
  name: string
  sortOrder: number
  children: CollectionNode[]
}

export interface RequestSummary {
  id: number
  collectionId: number
  name: string
  method: string
  url: string
  sortOrder: number
  updatedAt: number
}

export interface RequestSearchHit {
  id: number
  collectionId: number
  collectionName: string
  name: string
  method: string
  url: string
  updatedAt: number
}

export interface KeyValueItem {
  key: string
  value: string
  enabled: boolean
}

export interface CurlParsed {
  name: string
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  warnings: string[]
}

export interface RequestAuth {
  kind: 'none' | 'bearer' | 'basic' | 'apikey' | 'oauth2'
  token: string
  username: string
  password: string
  key: string
  value: string
  addTo: 'header' | 'query'
  /** OAuth2 Client Credentials */
  tokenUrl?: string
  clientId?: string
  clientSecret?: string
  scope?: string
}

export interface HttpRequestDetail {
  id?: number
  collectionId: number
  name: string
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  preScript: string
  postScript: string
  assertions: string
  auth?: RequestAuth
}

/** multipart/form-data 的表单字段 */
export interface FormField {
  key: string
  value: string
  enabled: boolean
  kind: 'text' | 'file'
}

export interface CookieItem {
  id: number
  domain: string
  path: string
  name: string
  value: string
  secure: boolean
  httpOnly: boolean
  expiresAt?: number | null
  updatedAt: number
}

/** 全局网络与安全设置 */
export interface AppSettings {
  proxyEnabled: boolean
  proxyUrl: string
  certPath: string
  certPassword: string
}

export interface PlanSummary {
  id: number
  projectId: number
  requestId?: number | null
  requestName?: string | null
  name: string
  updatedAt: number
}

export interface TestPlan {
  id: number
  projectId: number
  requestId?: number | null
  name: string
  config: unknown
  updatedAt: number
}

export interface SendRequestPayload {
  requestId?: number | null
  name?: string | null
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body?: string | null
  envId?: number | null
  insecure?: boolean
  timeoutMs?: number
  auth?: RequestAuth | null
  cookieEnabled?: boolean
}

export interface HttpResult {
  status: number
  statusText: string
  durationMs: number
  sizeBytes: number
  headers: Record<string, string>
  body: string
  truncated: boolean
  error?: string | null
}

export interface Environment {
  id: number
  projectId: number
  name: string
  isActive: boolean
  /** 环境脚本：每次发送请求前执行，可用 vars.set() 生成运行时变量 */
  script: string
  variables: EnvVariable[]
}

export interface EnvVariable {
  id?: number
  key: string
  value: string
  isSecret: boolean
  sortOrder: number
}

export interface HistoryItem {
  id: number
  requestId?: number | null
  projectId?: number | null
  requestName?: string | null
  method: string
  url: string
  statusCode?: number | null
  durationMs?: number | null
  createdAt: number
}

export interface HistoryStats {
  total: number
  oldestAt?: number | null
  newestAt?: number | null
  runsTotal: number
}

export interface CleanupResult {
  history: number
  runs: number
}

export interface AppInfo {
  version: string
  dataDir: string
  dbPath: string
}

export interface WorkspaceStats {
  projectCount: number
  collectionCount: number
  requestCount: number
  environmentCount: number
  historyCount: number
  reportCount: number
  reportRequestTotal: number
  reportSuccessTotal: number
  recentHistory: number
  recentReports: number
}

/** 数据大屏：按日请求趋势 */
export interface DailyPoint {
  date: string
  total: number
  success: number
  avgRt: number
}

/** 数据大屏：状态码分组 */
export interface CodeBucket {
  label: string
  count: number
}

/** 数据大屏：调用最频繁的接口 */
export interface TopRequest {
  name: string
  method: string
  url: string
  count: number
  avgRt: number
  successRate: number
}

/** 数据大屏：最近压测报告走势 */
export interface ReportPoint {
  id: number
  name: string
  p95Rt: number
  errorRate: number
  totalRequests: number
  createdAt: number
}

/** 数据大屏：按小时请求量 */
export interface HourPoint {
  hour: number
  count: number
}

/** 数据大屏：项目维度调用量 */
export interface ProjectPoint {
  name: string
  count: number
  avgRt: number
  successRate: number
}

/** 数据大屏聚合结果 */
export interface DashboardMetrics {
  days: DailyPoint[]
  codeBuckets: CodeBucket[]
  topRequests: TopRequest[]
  reportPoints: ReportPoint[]
  methodBuckets: CodeBucket[]
  hourBuckets: HourPoint[]
  latencyBuckets: CodeBucket[]
  topProjects: ProjectPoint[]
}

/** AI 服务商：kind 决定协议（openai / anthropic / gemini） */
export interface AiProvider {
  id?: number | null
  name: string
  kind: string
  baseUrl: string
  model: string
  /** 编辑时留空表示沿用已保存的 Key（列表接口不回传明文） */
  apiKey: string
  /** 列表回显掩码，形如 sk-****abcd */
  apiKeyMasked?: string
  hasKey?: boolean
  temperature?: number | null
  /** 单次生成最大 token，0 / 空 = 按协议默认 */
  maxTokens?: number | null
  /** 单次请求超时秒数，0 / 空 = 默认 300 */
  timeoutSecs?: number | null
  isDefault?: boolean
}

export interface AiMessage {
  role: 'system' | 'user' | 'assistant'
  content: string
}

export interface AiChatPayload {
  providerId: number
  messages: AiMessage[]
  temperature?: number | null
  /** 在途请求标识，配合 ai_chat_cancel 停止生成 */
  requestId?: string | null
}

/** 流式事件：delta 增量文本 / reasoning 思考 / retry 重试提示 / done 结束 / cancelled 已停止 / error 错误 */
export interface AiStreamEvent {
  kind: 'delta' | 'reasoning' | 'retry' | 'done' | 'cancelled' | 'error' | string
  text: string
}

/** AI 历史会话 */
export interface AiSession {
  id: number
  title: string
  providerId?: number | null
  messageCount: number
  pinned?: boolean
  summary?: string | null
  summaryUntil?: number
  createdAt: number
  updatedAt: number
}

export interface AiStoredMessage {
  id: number
  sessionId: number
  role: 'user' | 'assistant' | string
  content: string
  reasoning?: string | null
  createdAt: number
}

/** AI 工具调用流水 */
export interface AiToolLog {
  id: number
  sessionId?: number | null
  tool: string
  args: string
  ok: boolean
  durationMs: number
  resultPreview: string
  /** 第几轮工具调用（1 起） */
  round?: number
  /** 同一轮任务的链路标识 */
  traceId?: string | null
  /** 前端等待超时（后台仍在执行） */
  timeout?: boolean
  createdAt: number
}

export interface AiToolLogPage {
  items: AiToolLog[]
  total: number
}

export interface AiToolDayStat {
  date: string
  total: number
  failed: number
}

export interface AiToolUsageStat {
  tool: string
  total: number
  failed: number
  avgDurationMs: number
}

export interface AiToolLogStats {
  total: number
  ok: number
  failed: number
  avgDurationMs: number
  last24h: number
  days: AiToolDayStat[]
  tools: AiToolUsageStat[]
}

/** 单个模型（服务商）的用量与准确率 */
export interface AiUsageStat {
  providerId: number | null
  providerName: string
  model: string
  calls: number
  okCalls: number
  /** 成功调用占比 0~1 */
  successRate: number
  promptTokens: number
  completionTokens: number
  totalTokens: number
  avgLatencyMs: number
  toolCalls: number
  toolOk: number
  toolSuccessRate: number
  lastUsedAt: number
}

export interface AiUsageDay {
  day: string
  calls: number
  promptTokens: number
  completionTokens: number
  totalTokens: number
}

/** 单次模型调用记录 */
export interface AiUsageLog {
  id: number
  providerName: string
  model: string
  promptTokens: number
  completionTokens: number
  totalTokens: number
  ok: boolean
  latencyMs: number
  error: string
  /** 这轮实际发给模型的内容（本轮提问；工具续跑时附带首问） */
  requestPreview: string
  createdAt: number
}

/** 用量明细分页 */
export interface AiUsagePage {
  items: AiUsageLog[]
  total: number
}

export interface AiUsageStats {
  days: number
  totalCalls: number
  okCalls: number
  successRate: number
  promptTokens: number
  completionTokens: number
  totalTokens: number
  avgLatencyMs: number
  toolCalls: number
  toolOk: number
  toolSuccessRate: number
  items: AiUsageStat[]
  daily: AiUsageDay[]
}

export interface RequestRun {
  id: number
  requestId: number
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  responseStatus: number
  responseStatusText: string
  responseHeaders: Record<string, string>
  responseBody: string
  durationMs: number
  sizeBytes: number
  error?: string | null
  createdAt: number
}

export interface StressStage {
  concurrent: number
  durationSec: number
}

export interface SlaTargets {
  p95Ms?: number | null
  p99Ms?: number | null
  errorRate?: number | null
  minRps?: number | null
}

export interface StressConfig {
  requestId?: number | null
  name?: string | null
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body?: string | null
  envId?: number | null
  insecure?: boolean
  timeoutMs?: number
  connectTimeoutMs?: number
  followRedirects?: boolean
  stopErrorRate?: number | null
  stopMinRequests?: number | null
  concurrent: number
  durationSec: number
  rampUpSec: number
  thinkTimeMs: number
  stages?: StressStage[]
  targetRps?: number | null
  sla?: SlaTargets | null
}

export interface ReportSample {
  elapsedSec: number
  concurrent: number
  qps: number
  avgRt: number
  p50Rt: number
  p95Rt: number
  p99Rt: number
  ttfbAvg: number
  bytes: number
  successCount: number
  errorCount: number
}

export interface ReportError {
  elapsedSec: number
  errorType: string
  errorMessage: string
  requestUrl: string
  responseStatus?: number | null
  responseBody: string
}

export interface ReportRequestRecord {
  elapsedMs: number
  timeMs: number
  method: string
  url: string
  status: number
  latencyMs: number
  ttfbMs: number
  bytes: number
  success: boolean
  body?: string | null
  params?: string | null
  headers?: string | null
  responseHeaders?: string | null
  responseBody?: string | null
  errorType?: string | null
  errorMessage?: string | null
}

export interface ReportRequestsPage {
  total: number
  items: ReportRequestRecord[]
}

export interface StressReport {
  id: number
  planName: string
  requestId?: number | null
  totalRequests: number
  successCount: number
  errorCount: number
  avgRt: number
  minRt: number
  maxRt: number
  p50Rt: number
  p75Rt: number
  p90Rt: number
  p95Rt: number
  p99Rt: number
  p999Rt: number
  stddevRt: number
  ttfbAvg: number
  ttfbP95: number
  apdex: number
  bytesTotal: number
  maxQps: number
  avgQps: number
  targetRps: number
  errorRate: number
  concurrency: number
  status: string
  startTime?: number | null
  endTime?: number | null
  createdAt: number
}

export interface StatusCount {
  code: number
  count: number
}

export interface ErrorTypeCount {
  errorType: string
  count: number
}

export interface HistogramBucket {
  label: string
  minMs: number
  maxMs: number
  count: number
}

export interface StageSummary {
  index: number
  concurrent: number
  durationSec: number
  requests: number
  errors: number
  avgQps: number
  avgRt: number
  p95Rt: number
}

export interface ReportConfigSnapshot {
  mode?: string
  method?: string
  url?: string
  headers?: number
  params?: number
  bodyType?: string
  envId?: number | null
  timeoutMs?: number
  connectTimeoutMs?: number
  followRedirects?: boolean
  stopErrorRate?: number | null
  stopMinRequests?: number | null
  thinkTimeMs?: number
  rampUpSec?: number
  targetRps?: number
  stages?: StressStage[]
  sla?: SlaTargets | null
}

export interface ReportDetail {
  report: StressReport
  samples: ReportSample[]
  errors: ReportError[]
  statusCodes: StatusCount[]
  errorTypes: ErrorTypeCount[]
  histogram: HistogramBucket[]
  stages: StageSummary[]
  config?: ReportConfigSnapshot | null
}

export interface StressProgress {
  name: string
  running: boolean
  elapsedSec: number
  total: number
  success: number
  errors: number
  rps: number
  avgRt: number
  p95Rt: number
  p99Rt: number
  concurrent: number
  stageIndex: number
  activeConcurrent: number
  statusCodes: StatusCount[]
  samples: ReportSample[]
}

export interface StressDone {
  reportId?: number | null
  name: string
  status: string
  total: number
  success: number
  errors: number
  elapsedSec: number
  message?: string | null
}

/* ---------- 系统监视（winstats 风格） ---------- */

export interface MonitorSystemInfo {
  name: string
  osVersion: string
  kernelVersion: string
  hostName: string
  uptimeSec: number
  physicalCores: number
  logicalCores: number
}

export interface MonitorCpu {
  total: number
  brand: string
  vendor: string
  frequencyMhz: number
  cores: number[]
}

export interface MonitorMemory {
  total: number
  used: number
  available: number
  percent: number
}

export interface MonitorSwap {
  total: number
  used: number
  percent: number
}

export interface MonitorDisk {
  name: string
  mount: string
  fileSystem: string
  total: number
  available: number
  percent: number
  readPerSec: number
  writePerSec: number
  removable: boolean
}

export interface MonitorNetwork {
  name: string
  receivedPerSec: number
  transmittedPerSec: number
  totalReceived: number
  totalTransmitted: number
  mac: string
  ipv4: string[]
  ipv6: string[]
  gateway: string[]
}

export interface MonitorLocalIp {
  adapter: string
  address: string
  family: string
  gateway: string
  primary: boolean
}

export interface StorageInfo {
  dataDir: string
  dbPath: string
  defaultDir: string
  isCustom: boolean
  dbSize: number
  walSize: number
  pendingDir?: string | null
}

export interface UpdateInfo {
  currentVersion: string
  latestVersion: string
  hasUpdate: boolean
  releaseUrl: string
  notes: string
  publishedAt: string
  assetName?: string | null
  assetUrl?: string | null
  /** API 资源地址（api.github.com），github.com 直连受限时优先使用 */
  assetApiUrl?: string | null
  assetSize?: number | null
}

export interface MonitorBattery {
  present: boolean
  onAc: boolean
  percent: number
  minutesLeft: number
}

export interface MonitorGpu {
  name: string
  usage: number
}

export interface MonitorSnapshot {
  timestamp: number
  system: MonitorSystemInfo
  cpu: MonitorCpu
  memory: MonitorMemory
  swap?: MonitorSwap | null
  disks: MonitorDisk[]
  networks: MonitorNetwork[]
  ips: MonitorLocalIp[]
  battery?: MonitorBattery | null
  gpus: MonitorGpu[]
}

export interface MonitorProcess {
  pid: number
  name: string
  cpu: number
  memory: number
}

/* ---------- 工具箱：IP 查询 ---------- */

export interface IpLookupInfo {
  ip: string
  country?: string
  country_code?: string
  prov?: string
  city?: string
  city_code?: string
  city_short_code?: string
  area?: string
  post_code?: string
  area_code?: string
  isp?: string
  lng?: string
  lat?: string
  long_ip?: number
  big_area?: string
}
