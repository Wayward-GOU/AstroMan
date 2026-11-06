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
  /** 各 Body 类型的草稿（JSON 字符串），切换类型时保留内容 */
  bodyMeta?: string
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
  /** Python 解释器路径，留空自动探测 */
  pythonPath?: string
  /** 是否允许执行本地 Python 脚本（测试场景的钩子与 Python 用例） */
  pythonEnabled?: boolean
}

/** Python 解释器探测结果（python_check） */
export interface PythonCheckResult {
  enabled: boolean
  available: boolean
  info?: {
    program: string
    launcherArgs: string[]
    display: string
    version: string
    major: number
    minor: number
    patch: number
  } | null
  message: string
  /** 自动探测时试过的候选命令 */
  tried: string[]
  /** 随包 astroman_runtime.py 的路径 */
  runtimePath?: string | null
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

/** 重试策略：仅幂等方法默认可重试 */
export interface RetryConfig {
  enabled: boolean
  /** 最多尝试次数（含首次），后端上限 4 */
  maxAttempts: number
  /** 允许对 POST/PATCH 等非幂等方法重试，默认关闭 */
  allowNonIdempotent: boolean
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
  /** 执行来源：manual（默认）/ batch / suite / cli */
  origin?: string | null
  /** 前端生成的请求标识，用于「停止」按钮定位在途请求 */
  requestKey?: string | null
  /** 协议偏好：auto（默认）/ http1 / http2 */
  protocol?: 'auto' | 'http1' | 'http2' | null
  /** 是否跟随重定向，默认跟随（最多 10 跳） */
  followRedirects?: boolean | null
  /** 按请求覆盖连接超时 */
  connectTimeoutMs?: number | null
  retry?: RetryConfig | null
}

/**
 * 耗时分解（口径见技术方案 4.5）
 *
 * `connectMs` 只统计「新建 TCP（含 TLS）」；复用连接时为 0 且 `reusedConnection = true`，
 * 不代表「连接极快」。代理场景下不含代理握手，UI 必须标注。
 */
export interface RequestTimings {
  dnsMs?: number | null
  connectMs: number
  ttfbMs: number
  downloadMs: number
  totalMs: number
  reusedConnection: boolean
  proxy: boolean
}

/** 重定向链上的一跳 */
export interface RedirectHop {
  status: number
  location: string
  host: string
  durationMs: number
}

/** 一次重试尝试 */
export interface RetryAttempt {
  attempt: number
  status: number
  delayMs: number
  error?: string | null
}

/** 响应正文形态：文本 / JSON / 二进制落盘 / 超大流式落盘 */
export type BodyKind = 'text' | 'json' | 'binary' | 'stream'

export interface HttpResult {
  status: number
  statusText: string
  durationMs: number
  sizeBytes: number
  headers: Record<string, string>
  body: string
  truncated: boolean
  /** 现场缓存因正文过大未保留内容（仅内存快照使用） */
  bodyOmitted?: boolean
  error?: string | null
  bodyKind: BodyKind
  /** 落盘后的相对路径（相对数据目录），用 responseFilePath 换绝对路径 */
  bodyFile?: string | null
  /** 正文实际字节数（解压后） */
  bodyBytes: number
  /** 压缩态字节数（来自 Content-Length；gzip/br 时与 bodyBytes 不同） */
  rawBytes: number
  /** 小片段 Base64（图片 / PDF 首页），asset: 不可用时兜底 */
  previewBase64?: string | null
  timings?: RequestTimings | null
  /** 实际协商到的协议：HTTP/1.1 / HTTP/2 */
  httpVersion: string
  redirects: RedirectHop[]
  retries: RetryAttempt[]
  /** 因 HTTP/2 被剔除的请求头 */
  ignoredHeaders: string[]
  /** 用户主动取消 */
  cancelled: boolean
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
  /** 敏感变量只返回掩码（`••••••`），需要保留原值时原样回写即可 */
  value: string
  isSecret: boolean
  sortOrder: number
  /** 库里确实存在值（敏感变量被掩码时用它提示「已设置」） */
  hasValue?: boolean
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
  /** 执行来源：manual / batch / suite / cli */
  origin?: string
  createdAt: number
  /** 耗时分解（JSON 字符串，前端解析后画瀑布图） */
  timingsJson?: string | null
  /** text | json | binary | stream */
  bodyKind?: string | null
  /** 响应文件相对路径 */
  bodyFile?: string | null
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
  /** 是否支持图片输入（记事本截图识别） */
  supportsVision?: boolean
  isDefault?: boolean
  /** 上下文窗口覆盖（token）；0 / 空 = 按模型名自动识别 */
  contextWindow?: number | null
}

/** 后端注入的航仔提示词元信息：前端预算据此扣减，避免猜错 */
export interface AiContextMeta {
  systemPromptChars: number
  defaultMaxTokens: number
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
  /**
   * 调用场景：agent（主对话，后端注入全局提示词）/ analysis（页面分析）/
   * summary（会话摘要）；不传等同 agent
   */
  scene?: 'agent' | 'analysis' | 'summary' | (string & {})
  /** 本次请求的上下文窗口（token），后端据此收紧输出上限 */
  contextWindow?: number | null
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

/** 运行记录列表摘要：只带 2KB 预览，大 Body 在展开时调用 runGet 再取 */
export interface RequestRunSummary {
  id: number
  requestId: number
  method: string
  url: string
  bodyType: string
  responseStatus: number
  responseStatusText: string
  durationMs: number
  sizeBytes: number
  error?: string | null
  origin?: string
  createdAt: number
  requestBodySize: number
  responseBodySize: number
  requestBodyTruncated: boolean
  responseBodyTruncated: boolean
  requestBodyPreview: string
  responseBodyPreview: string
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
  /** 原始请求 / 响应体大小（不是 256KB 入库截断后的长度） */
  requestBodySize: number
  responseBodySize: number
  requestBodyTruncated: boolean
  responseBodyTruncated: boolean
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
  /** 逐请求明细采集策略，默认 auto */
  recordDetail?: RecordDetailMode
}

/** full：每请求记录；sample：1% 采样；off：不记录；auto：引擎按 QPS 定档 */
export type RecordDetailMode = 'full' | 'sample' | 'off' | 'auto'

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
  /** 实际生效的明细采集模式 */
  recordDetail?: string
  /** 用户请求的采集模式（auto 可能已降档） */
  recordDetailRequested?: string
  /** 施压端环境信息 */
  agent?: AgentInfo
  /** 运行期指标（发送字节、丢弃计数、资源峰值等） */
  metrics?: RuntimeMetrics
}

export interface AgentInfo {
  tool?: string
  version?: string
  os?: string
  osVersion?: string
  arch?: string
  host?: string
  cpuCores?: number
  memTotalMb?: number
}

export interface RuntimeMetrics {
  bytesSent?: number
  errorsDropped?: number
  recordsDropped?: number
  recordMode?: string
  cpuPeakCores?: number
  memPeakMb?: number
  sampleIntervalSec?: number
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
  /** 可编辑的测试场景描述 */
  scenarioText?: string | null
  /** 可编辑的报告总结 */
  summaryText?: string | null
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
  /** 本次新增的采样点（增量，前端需自行累积） */
  samples: ReportSample[]
  /** 当前生效的明细采集模式 */
  recordMode: string
  /** 一次性提示（如自动降档），前端 toast 后丢弃 */
  notice?: string | null
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
  /** 占物理内存的百分比（0–100） */
  memoryPercent: number
  /** 可执行文件路径，权限不足时为空 */
  exe: string
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

/* ---------- v1.0.8 自动化测试 ---------- */

export interface TestSuiteSummary {
  id: number
  projectId: number
  name: string
  description?: string
  envId?: number
  envName?: string
  tags?: string
  pythonPath?: string
  configJson?: string
  caseCount: number
  createdAt: number
  updatedAt: number
}

export interface TestSuite {
  id: number
  projectId: number
  name: string
  description?: string
  envId?: number
  tags?: string
  pythonPath?: string
  configJson?: string
  setupScript?: string
  teardownScript?: string
  createdAt: number
  updatedAt: number
}

export interface TestSuitePayload {
  id?: number
  projectId: number
  name: string
  description?: string
  envId?: number
  tags?: string
  pythonPath?: string
  configJson?: string
  setupScript?: string
  teardownScript?: string
}

export interface TestCase {
  id: number
  suiteId: number
  name: string
  kind: string
  requestId?: number
  planId?: number
  datasetId?: number
  dependsOn?: string
  sortOrder: number
  enabled: boolean
  timeoutMs: number
  retries: number
  retryOnFailure: boolean
  continueOnFailure: boolean
  tags?: string
  variablesJson?: string
  assertionsJson?: string
  preScript?: string
  postScript?: string
  mainScript?: string
  stressJson?: string
  createdAt: number
  updatedAt: number
}

export interface TestCasePayload {
  id?: number
  suiteId: number
  name: string
  kind: string
  requestId?: number
  planId?: number
  datasetId?: number
  dependsOn?: string
  sortOrder?: number
  enabled?: boolean
  timeoutMs?: number
  retries?: number
  retryOnFailure?: boolean
  continueOnFailure?: boolean
  tags?: string
  variablesJson?: string
  assertionsJson?: string
  preScript?: string
  postScript?: string
  mainScript?: string
  stressJson?: string
}

export interface TestDataset {
  id: number
  suiteId: number
  name: string
  format: string
  path?: string
  content?: string
  rowCount: number
  sha256?: string
  createdAt: number
}

export interface TestDatasetPayload {
  id?: number
  suiteId: number
  name: string
  format: string
  path?: string
  content?: string
}

export interface SuiteDetail {
  suite: TestSuite
  cases: TestCase[]
  datasets: TestDataset[]
}

export interface TestRun {
  id: number
  suiteId: number
  envId?: number
  envName?: string
  trigger: string
  status: string
  total: number
  passed: number
  failed: number
  skipped: number
  errored: number
  durationMs: number
  appVersion?: string
  pythonVersion?: string
  reportDir?: string
  startedAt?: number
  finishedAt?: number
  createdAt: number
}

export interface RunCase {
  id: number
  runId: number
  caseId?: number
  name: string
  kind: string
  status: string
  attempts: number
  durationMs: number
  httpDurationMs: number
  pythonDurationMs: number
  requestSnapshot?: string
  responseSnapshot?: string
  assertionsTotal: number
  assertionsFailed: number
  error?: string
  logs?: string
  pythonVersion?: string
  scriptHash?: string
  reportId?: number
}

export interface RunAssertion {
  id: number
  runCaseId: number
  name: string
  passed: boolean
  message?: string
  durationMs: number
}

export interface RunDetail {
  run: TestRun
  cases: RunCase[]
  assertions: RunAssertion[]
}

export interface PreflightReport {
  suiteId: number
  suiteName: string
  envId?: number
  envName?: string
  cookieMode: string
  strictScripts: boolean
  totalCases: number
  enabledCases: number
  errors: number
  warnings: number
  runnable: boolean
  items: PreflightItem[]
}

export interface PreflightItem {
  level: string
  code: string
  message: string
  caseId?: number
  caseName?: string
  hint?: string
}

export interface TestArtifact {
  id: number
  runCaseId?: number
  kind: string
  name: string
  path: string
  relativePath?: string
  sizeBytes: number
  exists: boolean
}

export interface TestRunReport {
  runId: number
  dir: string
  relativeDir?: string
  files: string[]
}

/* ---------- 备份 / 自检 / 诊断（v1.0.8） ---------- */

/** 备份类型：daily / weekly / manual / pre-restore / pre-migrate */
export type BackupKind = 'daily' | 'weekly' | 'manual' | 'pre-restore' | 'pre-migrate'

export interface BackupItem {
  name: string
  path: string
  /** 字节数 */
  size: number
  createdAt: number
  kind: BackupKind | string
  kindLabel: string
  /** 备份文件内的 schema 版本（读取失败时为 0） */
  schemaVersion: number
  tables: number
}

export interface CheckResult {
  ok: boolean
  detail: string
  durationMs: number
  tables: number
  schemaVersion: number
}

/* ---------- 密钥与主密钥（v1.0.8） ---------- */

/** 主密钥来源：系统钥匙串 / 降级文件 / 口令 */
export type KeySource = 'keyring' | 'file' | 'passphrase'

export interface SecretStatus {
  source: KeySource | string
  sourceLabel: string
  /** 口令模式下未解锁时为 true，此时敏感值全部不可用 */
  locked: boolean
  lockedReason?: string | null
  keyringAvailable: boolean
  /** 降级到文件模式：等同于没有加密 */
  degraded: boolean
  encryptedEnvCount: number
  encryptedAiCount: number
  keyCreatedAt?: number | null
  /** 主密钥能读到但与库内密文不匹配：已保存的 Key 解不出来 */
  keyMismatch?: boolean
  keyMismatchReason?: string | null
  /** 不匹配时解不开的条数 */
  brokenSecretCount?: number
}

/** 密钥包文件内容（导出 / 导入） */
export interface KeyPack {
  app: string
  version: number
  salt: string
  nonce: string
  data: string
  createdAt: number
  source: string
}
