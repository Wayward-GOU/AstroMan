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
  /** 请求类型：http | graphql | sse | websocket（与发送时的 HTTP 版本无关） */
  protocol?: string
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
  /** 请求实体（字段树 JSON，仅本接口使用）；不传表示保存时保持原值 */
  requestEntity?: string | null
  /** 返回实体（字段树 JSON，仅本接口使用）；不传表示保存时保持原值 */
  responseEntity?: string | null
}

/** 实时调试会话状态（G-09） */
export interface RealtimeStatus {
  sessionId: string
  protocol: string
  /** connecting | open | closed | error */
  state: string
  url: string
  messageCount: number
  bytes: number
  lastEventId?: string | null
  error?: string | null
  closeReason?: string | null
}

export interface RealtimeConnectPayload {
  sessionId?: string | null
  protocol: 'sse' | 'websocket'
  url: string
  headers?: KeyValueItem[]
  envId?: number | null
  autoReconnect?: boolean
  pingIntervalMs?: number
}

/** 实时事件流（Tauri 事件 `realtime:event` 的 payload） */
export interface RealtimeEvent {
  sessionId: string
  protocol: string
  /** message | reconnect | notice | connecting | open | closed | error | sent */
  type: string
  event?: string | null
  id?: string | null
  retry?: number | null
  data?: unknown
  messageType?: string
  message?: string
  attempt?: number
  reason?: string
  error?: string | null
  closeReason?: string | null
  ts: number
}

export interface RealtimeSendPayload {
  sessionId: string
  data: string
  kind?: 'text' | 'binary'
}

/** GraphQL Schema 缓存（G-10） */
export interface GraphqlSchemaEntry {
  id: number
  projectId: number
  endpoint: string
  schemaJson: string
  fetchedAt: number
}

export interface GraphqlSchemaMeta {
  id: number
  projectId: number
  endpoint: string
  fetchedAt: number
  bytes: number
}

export interface GraphqlIntrospectPayload {
  projectId: number
  endpoint: string
  headers?: KeyValueItem[]
  envId?: number | null
  timeoutMs?: number
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
  /** 性能模式：auto / low / standard（P-05） */
  performanceMode?: string
  /** AI 权限模式：readonly / confirm_write / trusted（AI-01） */
  aiPermissionMode?: string
  /** 摘要专用 provider id；0 表示沿用主 provider（AI-07） */
  summaryProviderId?: number
  /** 月度预算（美元）；0 表示不限制（AI-06） */
  monthlyBudget?: number
  /** 预算达到 80% 时提醒 */
  budgetAlert?: boolean
  /** 预算用尽后硬停止（默认只提醒） */
  budgetHardStop?: boolean
  /** 失败回退链：逗号分隔的 provider id；空 = 关闭（AI-12） */
  aiFallbackIds?: string
  /** 跨任务并发之和上限（默认 1000，硬上限 2000） */
  stressMaxConcurrent?: number
}

/** 企业策略（policy.json / W4-2） */
export interface EnterprisePolicy {
  disableAi: boolean
  /** 只允许本地 AI 模型，拒绝非回环地址（AI-13） */
  disableCloudAi: boolean
  disableLanServices: boolean
  disableTelemetry: boolean
  forceEncryptBackup: boolean
  /** 允许的导入格式；空数组表示不限制 */
  allowedImportFormats: string[]
  /** 请求历史保留天数；0 表示只按条数上限 */
  retentionDays: number
}

/** policy_get / policy_reload 的返回 */
export interface PolicySnapshot {
  policy: EnterprisePolicy
  path: string
  /** 文件格式错误时的提示；正常为 null */
  error: string | null
}

/** 审计日志（P-07）：只追加，默认保留 90 天 */
export interface AuditLog {
  id: number
  createdAt: number
  /** docs_export / ca_install / ai_write_confirm ... */
  action: string
  target?: string | null
  /** ok / failed / denied */
  result: string
  detail?: unknown
  traceId?: string | null
  actor: string
}

/** 抓包与响应体预算（P-05） */
export interface CaptureLimits {
  maxFlows: number
  maxBodyBytes: number
  maxResponseBytes: number
  /** 低内存档：前端据此关闭图片预览、减少自动格式化 */
  lowMemory: boolean
}

/** 当前运行档位（performance_profile） */
export interface PerformanceProfile {
  requested: string
  tier: 'low' | 'standard' | 'high'
  limits: CaptureLimits
  totalMemoryBytes: number | null
  description: string
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
  projectName?: string | null
  requestId?: number | null
  requestName?: string | null
  /** 接口已删除或计划未绑定接口 */
  requestMissing: boolean
  name: string
  method?: string | null
  /** URL 模板（可能含 {{变量}}） */
  urlTemplate?: string | null
  /** 解析后地址 */
  resolvedUrl?: string | null
  envId?: number | null
  envName?: string | null
  mode: 'fixed' | 'stages' | string
  stagesSummary: string
  lastRunAt?: number | null
  lastStatus?: string | null
  reportCount: number
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

/** 计划目标快照（config_json.target） */
export interface PlanTargetSnapshot {
  requestId?: number | null
  method?: string | null
  urlTemplate?: string | null
  resolvedUrl?: string | null
  capturedAt?: number | null
}

export interface PlanImportMatch {
  planName: string
  requestId?: number | null
  requestMethod: string
  requestUrl: string
  envId?: number | null
  envName?: string | null
  missingVars: string[]
  hasSensitive: boolean
  warnings: string[]
}

export interface PlanImportPreview {
  formatOk: boolean
  includesSensitive: boolean
  plans: PlanImportMatch[]
}

export interface PlanTemplate {
  id: string
  name: string
  description: string
  mode: 'fixed' | 'stages'
  concurrent?: number
  durationSec?: number
  stages?: { concurrent: number; durationSec: number }[]
  captureMode?: 'capacity' | 'locate' | 'auto'
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
  /** 生产环境：AI 无论哪种权限模式都强制确认（AI-01） */
  isProd?: boolean
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
  /** 压缩后实际释放的字节（主库 + WAL） */
  freedBytes: number
  /** 压缩失败原因；数据删除已提交，不影响删除结果 */
  compactError?: string | null
}

/** 删除报告 / 明细后的空间回收结果 */
export interface ReclaimResult {
  removed: number
  freedBytes: number
  compactError?: string | null
}

export interface AppInfo {
  version: string
  dataDir: string
  dbPath: string
  /** WebView2 / WebKit 版本 */
  webviewVersion?: string | null
  /** 数据目录风险提示 */
  dataDirRisks?: string[]
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
  /** 原生 function calling：-1 自动 / 0 关闭 / 1 开启（AI-04） */
  nativeTools?: number | null
  /** 输入单价覆盖（每百万 token）；0 = 使用内置价格表（AI-06） */
  inputPrice?: number | null
  outputPrice?: number | null
  /** 计价币种：USD / CNY */
  currency?: string | null
}

/** 后端注入的航仔提示词元信息：前端预算据此扣减，避免猜错 */
export interface AiContextMeta {
  systemPromptChars: number
  defaultMaxTokens: number
}

export interface AiToolCall {
  id: string
  name: string
  /** JSON 文本，与 OpenAI arguments 一致 */
  arguments: string
}

export interface AiToolDef {
  name: string
  description: string
  parameters: Record<string, unknown>
}

export interface AiMessage {
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: string
  /** 原生 function calling：assistant 消息携带的工具调用（AI-04） */
  toolCalls?: AiToolCall[]
  /** 工具结果消息对应的调用 id */
  toolCallId?: string
  /** 工具结果消息对应的工具名（Anthropic / Gemini 需要） */
  name?: string
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
  /** 当前轮图片：最多 3 张，只随本次请求发送（AI-03） */
  images?: Array<{ mime: string; data: string }>
  /** 原生 function calling 工具定义；为空时走文本 fbin-tool 协议（AI-04） */
  tools?: AiToolDef[]
}

/**
 * 流式事件：delta 增量文本 / reasoning 思考 / retry 重试提示 / fallback 已切换备用模型 /
 * tool_call 原生工具调用 / done 结束 / cancelled 已停止 / error 错误
 */
export interface AiStreamEvent {
  kind:
    | 'delta'
    | 'reasoning'
    | 'retry'
    | 'fallback'
    | 'tool_call'
    | 'done'
    | 'cancelled'
    | 'error'
    | string
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
  /** 所属项目：环境变量回退与计划保存用 */
  projectId?: number | null
  /** 来源计划：写入报告，便于按计划聚合 */
  planId?: number | null
  name?: string | null
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body?: string | null
  envId?: number | null
  /** 运行期变量：优先级高于环境变量，不落库 */
  vars?: Record<string, string>
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
  /** 采集模式预设：容量 / 定位 / 自动 */
  captureMode?: CapturePreset
  /** 定位模式保留的慢请求条数（默认 200，0 = 不保留） */
  topN?: number | null
  /** 只记录慢于该阈值的请求（毫秒），0 或空 = 不设阈值 */
  slowThresholdMs?: number | null
  /** 同目标冲突时忽略并继续（报告会留痕） */
  ignoreOverlap?: boolean
  /** 手动计划选择的历史基线报告（3.4.4） */
  baselineReportId?: number | null
  /** 超过 2 小时的长稳启动确认 */
  longRunAck?: boolean
}

/** capacity：容量模式；locate：定位模式；auto：沿用 recordDetail 并自动降档 */
export type CapturePreset = 'capacity' | 'locate' | 'auto'

/** 运行中的压测任务摘要（stress_jobs） */
export interface StressJobSummary {
  id: number
  name: string
  target: string
  planId?: number | null
  requestId?: number | null
  concurrent: number
  startedAt: number
  elapsedSec: number
  status: string
  /** 最近一秒实时 QPS，运行中由注册表每秒刷新 */
  qps?: number
  windowLabel?: string | null
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
  /** 响应体耗时均值（body = latency - ttfb，v15） */
  bodyAvg: number
  /** 响应体耗时 P95（v15） */
  bodyP95: number
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
  /** 来源计划（v15 由任务注册表写入） */
  planId?: number | null
  /** 同批多计划报告分组号 */
  runGroupId?: number | null
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
  /** 解析后的完整地址 */
  url?: string
  /** URL 模板原文（可能含 {{变量}}） */
  urlTemplate?: string
  /** 目标快照（3.5.1 双存地址） */
  target?: PlanTargetSnapshot | null
  headers?: number
  params?: number
  bodyType?: string
  envId?: number | null
  /** 解析后的环境名 */
  envName?: string | null
  projectId?: number | null
  planId?: number | null
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
  /** 采集预设 / TopN / 慢请求阈值 */
  captureMode?: CapturePreset | string
  topN?: number
  slowThresholdMs?: number
  /** 本机施压能力快照（1.7.5） */
  machineCapacity?: MachineCapacitySnapshot | null
  /** 开始压测时同主机已有其他任务在施压的警告（1.3.2，只留痕不拦截） */
  sameHostWarning?: string | null
  /** 手动计划选择的历史基线报告（3.4.4） */
  baselineReportId?: number | null
  /** 施压端环境信息 */
  agent?: AgentInfo
  /** 运行期指标（发送字节、丢弃计数、资源峰值等） */
  metrics?: RuntimeMetrics
}

/** 压测配置带入的机器能力快照 */
export interface MachineCapacitySnapshot {
  probedAt?: number
  maxQps?: number
  cores?: number
  cpuPeak?: number
  saturationConcurrent?: number
  reachedLimit?: boolean
}

/** 本机能力检测逐档采样 */
export interface CapacitySample {
  concurrent: number
  qps: number
  avgRt: number
  p95Rt: number
  errorRate: number
  /** AstroMan 进程占用核数 */
  cpu: number
  memMb: number
}

/** 本机能力检测结果 */
export interface CapacityResult {
  probedAt: number
  maxQps: number
  saturationConcurrent: number
  cpuPeak: number
  memPeak: number
  cores: number
  os: string
  /** false 表示 1024 并发仍未触顶，只能当 "≥ X QPS" 看 */
  reachedLimit: boolean
  samples: CapacitySample[]
}

/** 检测进度事件 */
export interface CapacityProgress {
  stage: number
  concurrent: number
  qps: number
  cpu: number
  mem: number
  done: boolean
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
  /** 自动降档轨迹（1.6.3） */
  downgradeTrail?: Array<{ atSec: number; from: string; to: string; qps: number }>
  slowRequestCount?: number
}

export interface ReportDetail {
  report: StressReport
  samples: ReportSample[]
  errors: ReportError[]
  statusCodes: StatusCount[]
  errorTypes: ErrorTypeCount[]
  histogram: HistogramBucket[]
  /** 响应体读取耗时直方图（v15） */
  bodyHistogram?: HistogramBucket[]
  stages: StageSummary[]
  config?: ReportConfigSnapshot | null
  /** TopN 慢请求快照（v15） */
  slowRequests?: SlowRequest[]
  /** 可编辑的测试场景描述 */
  scenarioText?: string | null
  /** 可编辑的报告总结 */
  summaryText?: string | null
}

/** 慢请求快照：定位模式与长稳报告使用 */
export interface SlowRequest {
  elapsedMs: number
  method: string
  url: string
  status: number
  latencyMs: number
  ttfbMs: number
  /** 响应体耗时 = latency - ttfb */
  bodyMs: number
}

export interface StressProgress {
  /** 任务 ID：多窗口并行时前端按此过滤 */
  jobId: number
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
  jobId: number
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
  /** 安装包 SHA-256，下载后强制校验 */
  assetSha256?: string | null
  /** stable / beta / 企业清单自定义 */
  channel?: string
  /** 灰度比例与当前设备命中的桶（0-99） */
  rolloutPercent?: number
  rolloutBucket?: number
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

/* ---------- W2：异地备份目标（G-06） ---------- */

export type BackupTargetKind = 'local' | 'webdav'

export interface BackupTarget {
  id: number
  name: string
  kind: BackupTargetKind | string
  kindLabel: string
  url: string
  username: string
  dir: string
  enabled: boolean
  /** 只表示是否存过密码，密文 / 明文都不会下发 */
  hasPassword: boolean
  lastRunAt?: number | null
  lastStatus?: string | null
  createdAt: number
}

export interface BackupTargetInput {
  id?: number
  name: string
  kind: BackupTargetKind | string
  url?: string
  username?: string
  /** 留空表示保留原密码 */
  password?: string
  dir?: string
  enabled?: boolean
}

export interface BackupTargetTestReport {
  ok: boolean
  detail: string
  checkedAt: number
}

export interface RemoteBackup {
  name: string
  size: number
  modifiedAt?: number | null
  kind: string
}

export interface BackupUploadResult {
  targetId: number
  targetName: string
  kind: string
  remoteName: string
  encryptedPath: string
  encryptedSize: number
  uploadedAt: number
  detail: string
  snapshot: BackupItem
}

/* ---------- W2：本地 Mock Server（G-04） ---------- */

export interface MockRoute {
  id: number
  projectId: number
  requestId?: number | null
  name: string
  method: string
  path: string
  statusCode: number
  headers: KeyValueItem[]
  /** auto = 回放最近一次运行 / 实体骨架 */
  bodyType: string
  body: string
  delayMs: number
  enabled: boolean
  sortOrder: number
  updatedAt: number
  /** 只读：与同 method + path 的其它启用路由冲突 */
  conflict?: boolean
}

export type MockRouteInput = Partial<MockRoute> & {
  projectId: number
  method: string
  path: string
}

export interface MockStartPayload {
  projectId: number
  /** 省略或 0 时自动分配 */
  port?: number
  host?: string
}

export interface MockStatus {
  running: boolean
  projectId?: number | null
  url: string
  host: string
  port: number
  lan: boolean
  token?: string | null
  routeCount: number
  startedAt: number
  requests: number
}

export interface MockLog {
  id: number
  at: number
  method: string
  path: string
  status: number
  durationMs: number
  source: string
  routeId?: number | null
  generated: boolean
  error?: string | null
}

/* ---------- W2：导入导出补齐（G-07） ---------- */

export interface PostmanExportReport {
  requests: number
  collections: number
  skipped: string[]
}

export interface OpenApiSource {
  id: number
  projectId: number
  url: string
  headers: KeyValueItem[]
  updatedAt: number
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

/* ---------- 接口实体（v1.1.2） ---------- */

/** 实体字段树节点，与后端 entity::model::EntityNode 对应 */
export interface EntityNode {
  name: string
  type: string
  itemType: string
  required: boolean
  description: string
  /** 用户手动添加的字段：合并时不被自动刷新删除 */
  manual: boolean
  /** 最近一次发送未出现（保留字段，提示用户确认） */
  stale: boolean
  children: EntityNode[]
}

/** 一份实体文档（某个方向的请求 / 返回），与后端 entity::model::EntityDoc 对应 */
export interface EntityDoc {
  version: number
  updatedAt: number
  hiddenPaths: string[]
  root: EntityNode
}

/** request_entities_get / request_entity_generate 的返回 */
export interface RequestEntities {
  requestEntity?: EntityDoc | null
  responseEntity?: EntityDoc | null
  warnings: string[]
}

/** 实体方向 */
export type EntityTarget = 'request' | 'response'
