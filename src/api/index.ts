import { Channel, invoke } from '@tauri-apps/api/core'
import type {
  AiChatPayload,
  AiContextMeta,
  AiProvider,
  AiSession,
  AiStoredMessage,
  AiStreamEvent,
  AiToolLogPage,
  AiToolLogStats,
  AiUsagePage,
  AiUsageStats,
  AppSettings,
  AuditLog,
  BackupItem,
  BackupTarget,
  BackupTargetInput,
  BackupTargetTestReport,
  BackupUploadResult,
  CheckResult,
  CleanupResult,
  CollectionNode,
  CookieItem,
  CurlParsed,
  DashboardMetrics,
  Environment,
  EnvVariable,
  EntityTarget,
  GraphqlIntrospectPayload,
  GraphqlSchemaEntry,
  GraphqlSchemaMeta,
  HistoryItem,
  HistoryStats,
  HttpRequestDetail,
  HttpResult,
  IpLookupInfo,
  KeyPack,
  MonitorProcess,
  MonitorSnapshot,
  MockLog,
  MockRoute,
  MockRouteInput,
  MockStartPayload,
  MockStatus,
  OpenApiSource,
  PlanSummary,
  PerformanceProfile,
  PolicySnapshot,
  PostmanExportReport,
  Project,
  PythonCheckResult,
  RealtimeConnectPayload,
  RealtimeSendPayload,
  RealtimeStatus,
  ReportDetail,
  ReportRequestsPage,
  RemoteBackup,
  RequestRun,
  RequestSearchHit,
  RequestRunSummary,
  RequestSummary,
  RequestEntities,
  SecretStatus,
  SendRequestPayload,
  StorageInfo,
  PreflightReport,
  TestArtifact,
  TestCase,
  TestCasePayload,
  TestDataset,
  TestDatasetPayload,
  TestRun,
  TestRunReport,
  TestSuite,
  TestSuitePayload,
  TestSuiteSummary,
  RunDetail,
  StressConfig,
  StressReport,
  TestPlan,
  UpdateInfo,
  WorkspaceStats,
} from '@/types/models'
import type {
  ContractCheckResult,
  ContractSnapshot,
  RevisionBrief,
  RevisionDetail,
  RevisionDiff,
  TraceRollbackResult,
} from '@/types/contract'
import type {
  DocsExportPayload,
  DocsExportResult,
  DocsPreparePayload,
  DocsPrepareResult,
  DocsRenderPayload,
  DocsRenderResult,
  DocsSiteStartPayload,
  DocsSiteStatus,
} from '@/types/docs'

export { appInfo, projectList, projectCreate, projectUpdate, projectDelete } from './project'

// 工作台总览
export const workspaceStats = () => invoke<WorkspaceStats>('workspace_stats')

// 工作台数据大屏
export const dashboardMetrics = () => invoke<DashboardMetrics>('dashboard_metrics')

// 集合
export const collectionTree = (projectId: number) =>
  invoke<CollectionNode[]>('collection_tree', { projectId })

export const collectionCreate = (projectId: number, parentId: number | null, name: string) =>
  invoke<CollectionNode>('collection_create', { projectId, parentId, name })

export const collectionRename = (id: number, name: string) =>
  invoke<void>('collection_rename', { id, name })

export const collectionDelete = (id: number) => invoke<void>('collection_delete', { id })

/** 拖拽排序：按 ids 的顺序重排同一父集合下的集合 */
export const collectionReorder = (projectId: number, parentId: number | null, ids: number[]) =>
  invoke<void>('collection_reorder', { projectId, parentId, ids })

// 请求
export const requestList = (collectionId: number) =>
  invoke<RequestSummary[]>('request_list', { collectionId })

export const requestGet = (id: number) => invoke<HttpRequestDetail>('request_get', { id })

export const requestSearch = (projectId: number, keyword: string) =>
  invoke<RequestSearchHit[]>('request_search', { projectId, keyword })

export const requestOptions = (projectId: number) =>
  invoke<RequestSearchHit[]>('request_options', { projectId })

export const requestSave = (request: HttpRequestDetail) =>
  invoke<HttpRequestDetail>('request_save', { request })

/** 自动保存：只写库不回读整份详情，避免大 Body 双向往返 */
export const requestSaveQuick = (request: HttpRequestDetail) =>
  invoke<void>('request_save_quick', { request })

export const requestDelete = (id: number) => invoke<void>('request_delete', { id })

/** 拖拽排序：按 ids 的顺序重排某集合下的请求 */
export const requestReorder = (collectionId: number, ids: number[]) =>
  invoke<void>('request_reorder', { collectionId, ids })

// 单接口运行记录
export const runList = (requestId: number, limit = 200) =>
  invoke<RequestRunSummary[]>('run_list', { requestId, limit })

/** 展开某条运行记录时再取完整请求 / 响应内容 */
export const runGet = (id: number) => invoke<RequestRun>('run_get', { id })

export const runClear = (requestId: number) => invoke<void>('run_clear', { requestId })

// 接口实体（请求 / 返回字段树）
export const requestEntitiesGet = (requestId: number) =>
  invoke<RequestEntities>('request_entities_get', { requestId })

/** 重新推断实体；target=request 时优先用编辑器里的当前请求体 */
export const requestEntityGenerate = (params: {
  requestId: number
  target: EntityTarget
  body?: string | null
  bodyType?: string | null
}) => invoke<RequestEntities>('request_entity_generate', params)

// 请求版本历史
export const requestRevisionList = (requestId: number, limit?: number) =>
  invoke<RevisionBrief[]>('request_revision_list', { requestId, limit: limit ?? null })

export const requestRevisionGet = (requestId: number, revision: number) =>
  invoke<RevisionDetail>('request_revision_get', { requestId, revision })

export const requestRevisionDiff = (requestId: number, from: number, to: number) =>
  invoke<RevisionDiff>('request_revision_diff', { requestId, from, to })

/** 回滚到某个版本：后端按实体合并语义恢复，并生成一条 source=restore 的新版本 */
export const requestRevisionRestore = (requestId: number, revision: number) =>
  invoke<HttpRequestDetail>('request_revision_restore', { requestId, revision })

/** AI 写操作执行前记录前置快照（source=ai，按 traceId 聚合回滚） */
export const requestRevisionSnapshot = (requestId: number, traceId: string) =>
  invoke<void>('request_revision_snapshot', { requestId, traceId })

/** 某轮提问关联的 AI 前置快照 */
export const requestRevisionTrace = (traceId: string, limit?: number) =>
  invoke<RevisionBrief[]>('request_revision_trace', { traceId, limit: limit ?? null })

/** 按 traceId 一键回滚该轮全部 AI 写操作 */
export const requestRevisionRollbackTrace = (traceId: string) =>
  invoke<TraceRollbackResult>('request_revision_rollback_trace', { traceId })

// 契约快照与变更检测
export const contractSnapshotCreate = (projectId: number, label?: string) =>
  invoke<ContractSnapshot>('contract_snapshot_create', { projectId, label: label ?? null })

export const contractSnapshotList = (projectId: number) =>
  invoke<ContractSnapshot[]>('contract_snapshot_list', { projectId })

export const contractCheck = (projectId: number, baselineId: number) =>
  invoke<ContractCheckResult>('contract_check', { projectId, baselineId })

/** 生成契约报告；传 path 时同时写入文件 */
export const contractReport = (
  projectId: number,
  baselineId: number,
  format: 'markdown' | 'html',
  path?: string | null,
) => invoke<string>('contract_report', { projectId, baselineId, format, path: path ?? null })

export const sendRequest = (payload: SendRequestPayload) =>
  invoke<HttpResult>('send_http_request', { payload })

/** OAuth2 Client Credentials：换取 access_token */
export const oauth2Token = (
  config: { tokenUrl: string; clientId: string; clientSecret: string; scope: string },
  insecure?: boolean,
) => invoke<string>('oauth2_token', { config, insecure: insecure ?? false })

// 环境
export const envList = (projectId: number) => invoke<Environment[]>('env_list', { projectId })

export const envCreate = (projectId: number, name: string) =>
  invoke<Environment>('env_create', { projectId, name })

export const envActivate = (projectId: number, id: number) =>
  invoke<void>('env_activate', { projectId, id })

export const envDelete = (id: number) => invoke<void>('env_delete', { id })

/** 读取环境的非敏感变量值（用于界面变量渲染预览；敏感值不返回） */
export const envPlainVars = (envId: number) =>
  invoke<Record<string, string>>('env_plain_vars', { envId })

export const envVarSave = (envId: number, variables: EnvVariable[]) =>
  invoke<void>('env_var_save', { envId, variables })

export const envScriptSave = (id: number, script: string) =>
  invoke<void>('env_script_save', { id, script })

/** 标记环境是否为生产（AI-01 强制确认） */
export const envSetProd = (id: number, isProd: boolean) =>
  invoke<void>('env_set_prod', { id, isProd })

/** 最近审计（P-07） */
export const auditLogList = (options?: { action?: string; limit?: number; offset?: number }) =>
  invoke<AuditLog[]>('audit_log_list', {
    action: options?.action ?? null,
    limit: options?.limit ?? null,
    offset: options?.offset ?? null,
  })

export const auditLogCleanup = () => invoke<number>('audit_log_cleanup')

// 历史
export const historyList = (
  limit = 100,
  requestId?: number | null,
  projectId?: number | null,
) =>
  invoke<HistoryItem[]>('history_list', {
    limit,
    requestId: requestId ?? null,
    projectId: projectId ?? null,
  })

export const historyStats = () => invoke<HistoryStats>('history_stats')

/** 清理历史数据：from / to 为空表示不限边界（即全部清除），dryRun 只统计不删除 */
export const dataCleanup = (options: {
  from?: number | null
  to?: number | null
  history?: boolean
  runs?: boolean
  vacuum?: boolean
  dryRun?: boolean
}) =>
  invoke<CleanupResult>('data_cleanup', {
    from: options.from ?? null,
    to: options.to ?? null,
    history: options.history ?? true,
    runs: options.runs ?? false,
    vacuum: options.vacuum ?? false,
    dryRun: options.dryRun ?? false,
  })

// Cookie
export const cookieList = () => invoke<CookieItem[]>('cookie_list')

export const cookieSave = (cookie: {
  domain: string
  path?: string | null
  name: string
  value: string
  expiresAt?: number | null
}) =>
  invoke<void>('cookie_save', {
    domain: cookie.domain,
    path: cookie.path ?? '/',
    name: cookie.name,
    value: cookie.value,
    expiresAt: cookie.expiresAt ?? null,
  })

export const cookieDelete = (id: number) => invoke<void>('cookie_delete', { id })

export const cookieClear = (domain?: string) =>
  invoke<number>('cookie_clear', { domain: domain ?? null })

// 全局网络与安全设置（代理 / 客户端证书 / Python 授权）
export const settingsGet = () => invoke<AppSettings>('settings_get')

export const settingsSet = (settings: AppSettings) =>
  invoke<AppSettings>('settings_set', { settings })

/** 当前运行档位与抓包 / 响应体预算（P-05） */
export const performanceProfile = () => invoke<PerformanceProfile>('performance_profile')

/** 企业策略：当前生效值、文件位置与解析错误（W4-2） */
export const policyGet = () => invoke<PolicySnapshot>('policy_get')

/** 管理员改完 policy.json 后热加载 */
export const policyReload = () => invoke<PolicySnapshot>('policy_reload')

/** 前端异常转后端日志（走统一脱敏与滚动落盘） */
export const frontendLog = (level: string, message: string, context?: string) =>
  invoke<void>('frontend_log', { payload: { level, message, context: context ?? null } })

/** 探测 Python 解释器；path 为空时按设置项与默认候选自动探测 */
export const pythonCheck = (path?: string) =>
  invoke<PythonCheckResult>('python_check', { path: path?.trim() || null })

// 压测计划
export const planList = (projectId: number) => invoke<PlanSummary[]>('plan_list', { projectId })

export const planGet = (id: number) => invoke<TestPlan>('plan_get', { id })

export const planSave = (plan: {
  id?: number | null
  projectId: number
  requestId?: number | null
  name: string
  config: unknown
}) =>
  invoke<TestPlan>('plan_save', {
    plan: {
      id: plan.id ?? null,
      projectId: plan.projectId,
      requestId: plan.requestId ?? null,
      name: plan.name,
      config: plan.config,
    },
  })

export const planDelete = (id: number) => invoke<void>('plan_delete', { id })

// 本地文件读写（Runner 数据文件、导出结果、保存响应体）
export const fileReadText = (path: string) => invoke<string>('file_read_text', { path })

/** 导出前判断目标文件是否已存在，用于覆盖二次确认 */
export const fileExists = (path: string) => invoke<boolean>('file_exists', { path })

export const fileWriteText = (path: string, content: string) =>
  invoke<void>('file_write_text', { path, content })

// 工具箱
export const ipLookup = (ip?: string) =>
  invoke<IpLookupInfo>('ip_lookup', { ip: ip?.trim() || null })

// 压测
export const stressStart = (config: StressConfig) => invoke<void>('stress_start', { config })

export const stressStop = () => invoke<void>('stress_stop')

// 压测报告
export const reportList = (limit = 100) => invoke<StressReport[]>('report_list', { limit })

export const reportGet = (id: number) => invoke<ReportDetail>('report_get', { id })

export const reportDelete = (id: number) => invoke<void>('report_delete', { id })

export const reportExport = (id: number, path: string) =>
  invoke<void>('report_export', { id, path })

export const reportWriteFile = (path: string, data: number[]) =>
  invoke<void>('report_write_file', { path, data })

export const reportRequests = (id: number, page = 1, pageSize = 50, success?: boolean | null) =>
  invoke<ReportRequestsPage>('report_requests', { id, page, pageSize, success: success ?? null })

/** 保存报告的场景描述与总结 */
export const reportUpdateTexts = (id: number, scenarioText: string, summaryText: string) =>
  invoke<void>('report_update_texts', { id, scenarioText, summaryText })

/** 清理某份报告的逐请求明细（保留汇总与采样） */
export const reportClearDetails = (id: number) => invoke<number>('report_clear_details', { id })

/** 明细 / 错误 CSV 流式导出，返回写入行数 */
export const reportExportDetailCsv = (id: number, kind: 'requests' | 'errors', path: string) =>
  invoke<number>('report_export_detail_csv', { id, kind, path })

// cURL 导入
export const curlParse = (text: string) => invoke<CurlParsed>('curl_parse', { text })

// 项目导入导出
export const projectExport = (projectId: number, path: string) =>
  invoke<void>('project_export', { projectId, path })

export const projectImport = (path: string) => invoke<Project>('project_import', { path })

// OpenAPI 3.0 导出
export const openapiExport = (projectId: number, path: string) =>
  invoke<void>('openapi_export', { projectId, path })

// 接口文档导出（阶段一：Markdown / HTML）
export const docsPrepare = (payload: DocsPreparePayload) =>
  invoke<DocsPrepareResult>('docs_prepare', { payload })

export const docsRender = (payload: DocsRenderPayload) =>
  invoke<DocsRenderResult>('docs_render', { payload })

export const docsExport = (payload: DocsExportPayload) =>
  invoke<DocsExportResult>('docs_export', { payload })

// 只读文档站：启动 / 停止 / 状态 / 刷新
export const docsSiteStart = (payload: DocsSiteStartPayload) =>
  invoke<DocsSiteStatus>('docs_site_start', { payload })
export const docsSiteStop = () => invoke<DocsSiteStatus>('docs_site_stop')
export const docsSiteStatus = () => invoke<DocsSiteStatus>('docs_site_status')
export const docsSiteRefresh = () => invoke<DocsSiteStatus>('docs_site_refresh')

// OpenAPI / Swagger 导入（projectId 为空时新建项目）
export const openapiImport = (path: string, projectId?: number | null) =>
  invoke<Project>('openapi_import', { path, projectId: projectId ?? null })

// 自动更新
export const updateCheck = () => invoke<UpdateInfo>('update_check')

export const updateDownload = (urls: string[], fileName: string, sha256?: string | null) =>
  invoke<string>('update_download', { urls, fileName, sha256: sha256 ?? null })

export const openExternal = (target: string) => invoke<void>('open_external', { target })

// Postman（Collection v2.0/v2.1）与 Apifox（OpenAPI）导入
export const apiImport = (path: string, projectId?: number | null) =>
  invoke<Project>('api_import', { path, projectId: projectId ?? null })

// 工具箱
export const toolHash = (text: string, algorithm: string) =>
  invoke<string>('tool_hash', { text, algorithm })

export const toolHmac = (text: string, key: string, algorithm: string) =>
  invoke<string>('tool_hmac', { text, key, algorithm })

// AI 助手
export const aiContextMeta = () => invoke<AiContextMeta>('ai_context_meta')

export const aiProviderList = () => invoke<AiProvider[]>('ai_provider_list')

export const aiProviderSave = (provider: AiProvider) =>
  invoke<AiProvider>('ai_provider_save', { provider })

export const aiProviderDelete = (id: number) => invoke<void>('ai_provider_delete', { id })

export const aiProviderTest = (id: number) => invoke<string>('ai_provider_test', { id })

/** 流式对话：边收边回调，结束后 invoke 才 resolve */
export const aiChat = async (
  payload: AiChatPayload,
  onEvent: (event: AiStreamEvent) => void,
) => {
  const channel = new Channel<AiStreamEvent>()
  channel.onmessage = onEvent
  await invoke<void>('ai_chat', { payload, onEvent: channel })
}

/** 停止在途生成：命中返回 true */
export const aiChatCancel = (requestId: string) =>
  invoke<boolean>('ai_chat_cancel', { requestId })

/** 拉取服务商实际可用模型（前端缓存 24 小时，失败回退内置清单） */
export const aiModelsList = (providerId: number) =>
  invoke<string[]>('ai_models_list', { providerId })

/** AI 生成的请求直接执行，与手动发送共用同一链路；confirmed 用于权限确认 */
export const aiExecute = (payload: SendRequestPayload, confirmed = false, traceId?: string | null) =>
  invoke<HttpResult>('ai_execute', { payload, confirmed, traceId: traceId ?? null })

/** 写工具 / 发送确认的审计落库（AI-01） */
export interface AiConfirmAuditPayload {
  tool: string
  target?: string | null
  allowed: boolean
  mode?: string | null
  traceId?: string | null
}

export const aiConfirmAudit = (payload: AiConfirmAuditPayload) =>
  invoke<void>('ai_confirm_audit', { payload })

/** 按 traceId 取消航仔正在执行的工具（AI-02）：中断在途 HTTP 请求 */
export const toolCancel = (traceId: string) =>
  invoke<boolean>('tool_cancel', { traceId })

// AI 会话记录
export const aiSessionList = (keyword?: string) =>
  invoke<AiSession[]>('ai_session_list', { keyword: keyword ?? null })

export const aiSessionPin = (id: number, pinned: boolean) =>
  invoke<void>('ai_session_pin', { id, pinned })

export const aiSessionSummarySave = (id: number, summary: string, summaryUntil: number) =>
  invoke<void>('ai_session_summary_save', { id, summary, summaryUntil })

export const aiSessionCreate = (title?: string, providerId?: number | null) =>
  invoke<AiSession>('ai_session_create', { title: title ?? null, providerId: providerId ?? null })

export const aiSessionRename = (id: number, title: string) =>
  invoke<void>('ai_session_rename', { id, title })

export const aiSessionDelete = (id: number) => invoke<void>('ai_session_delete', { id })

export const aiMessageList = (sessionId: number) =>
  invoke<AiStoredMessage[]>('ai_message_list', { sessionId })

export const aiMessageAdd = (
  sessionId: number,
  role: string,
  content: string,
  reasoning?: string,
) => invoke<AiStoredMessage>('ai_message_add', { sessionId, role, content, reasoning: reasoning ?? null })

export const aiMessageClear = (sessionId: number) =>
  invoke<void>('ai_message_clear', { sessionId })

/** 删除单条消息（按数据库 id）：消息级删除只移除这一条 */
export const aiMessageDelete = (id: number) => invoke<void>('ai_message_delete', { id })

/** 编辑重发：删除该消息及其后的所有消息，避免旧分支残留 */
export const aiMessageDeleteFrom = (
  sessionId: number,
  createdAt: number,
  inclusive = true,
) =>
  invoke<void>('ai_message_delete_from', { sessionId, createdAt, inclusive })

// AI 工具调用流水
export const aiToolLogAdd = (log: {
  sessionId?: number | null
  tool: string
  args: string
  ok: boolean
  durationMs: number
  resultPreview: string
  round?: number
  traceId?: string | null
  timeout?: boolean
}) =>
  invoke<void>('ai_tool_log_add', {
    sessionId: log.sessionId ?? null,
    tool: log.tool,
    args: log.args,
    ok: log.ok,
    durationMs: log.durationMs,
    resultPreview: log.resultPreview,
    round: log.round ?? 0,
    traceId: log.traceId ?? null,
    timeout: log.timeout ?? false,
  })

export const aiToolLogList = (query: {
  sessionId?: number | null
  tool?: string | null
  ok?: boolean | null
  days?: number
  page?: number
  pageSize?: number
}) =>
  invoke<AiToolLogPage>('ai_tool_log_list', {
    sessionId: query.sessionId ?? null,
    tool: query.tool || null,
    ok: query.ok ?? null,
    days: query.days ?? 365,
    page: query.page ?? 1,
    pageSize: query.pageSize ?? 20,
  })

export const aiToolLogStats = (days = 30) =>
  invoke<AiToolLogStats>('ai_tool_log_stats', { days })

export const aiToolLogClear = (sessionId?: number | null, days?: number | null) =>
  invoke<number>('ai_tool_log_clear', { sessionId: sessionId ?? null, days: days ?? null })

// 模型用量统计；sinceMs 用于自然月等自定义窗口（优先于 days）
export const aiUsageStats = (days = 30, sinceMs: number | null = null) =>
  invoke<AiUsageStats>('ai_usage_stats', { days, sinceMs })

// 用量明细：每次模型调用一条，分页返回，ok 为空表示不筛状态
export const aiUsageLogs = (days = 30, page = 1, pageSize = 10, ok: boolean | null = null) =>
  invoke<AiUsagePage>('ai_usage_logs', { days, page, pageSize, ok })

export const aiUsageClear = (all = false) => invoke<number>('ai_usage_clear', { all })

// 系统监视
export const winstatsSnapshot = () => invoke<MonitorSnapshot>('winstats_snapshot')

/** limit 为 0 时返回全部进程 */
export const winstatsProcesses = (limit = 0, byMemory = false) =>
  invoke<MonitorProcess[]>('winstats_processes', { limit, byMemory })

export const winstatsProcessKill = (pid: number, name?: string) =>
  invoke<void>('winstats_process_kill', { pid, name })

// 数据存储位置
export const storageInfo = () => invoke<StorageInfo>('storage_info')

export const storageSet = (path: string, migrate: boolean) =>
  invoke<StorageInfo>('storage_set', { path, migrate })

export const storageReset = () => invoke<StorageInfo>('storage_reset')

export const storageOpen = (path: string) => invoke<void>('storage_open', { path })

export const appRestart = () => invoke<void>('app_restart')

/* ---------- v1.0.8 自动化测试 ---------- */

export const testSuiteList = (projectId?: number, keyword?: string, tag?: string) =>
  invoke<TestSuiteSummary[]>('test_suite_list', {
    projectId: projectId ?? null,
    keyword: keyword ?? null,
    tag: tag ?? null,
  })

export const testSuiteGet = (id: number) =>
  invoke<{ suite: TestSuite; cases: TestCase[]; datasets: TestDataset[] }>('test_suite_get', {
    id,
  })

export const testSuiteSave = (payload: TestSuitePayload) =>
  invoke<number>('test_suite_save', { payload })

export const testSuiteDelete = (id: number) => invoke<void>('test_suite_delete', { id })

/** 导出套件包（*.astromantest.json） */
export const testSuiteExport = (suiteId: number, path: string) =>
  invoke<string>('test_suite_export', { suiteId, path })

/** 导入套件包；onConflict：skip / overwrite / rename */
export const testSuiteImport = (path: string, projectId?: number, onConflict = 'rename') =>
  invoke<number>('test_suite_import', {
    path,
    projectId: projectId ?? null,
    onConflict,
  })

export const testCaseList = (suiteId: number) =>
  invoke<TestCase[]>('test_case_list', { suiteId })

export const testCaseGet = (id: number) => invoke<TestCase>('test_case_get', { id })

export const testCaseSave = (payload: TestCasePayload) =>
  invoke<number>('test_case_save', { payload })

export const testCaseDelete = (id: number) => invoke<void>('test_case_delete', { id })

export const testCaseReorder = (suiteId: number, ids: number[]) =>
  invoke<void>('test_case_reorder', { suiteId, ids })

export const testDatasetList = (suiteId: number) =>
  invoke<TestDataset[]>('test_dataset_list', { suiteId })

export const testDatasetGet = (id: number) => invoke<TestDataset>('test_dataset_get', { id })

export const testDatasetSave = (payload: TestDatasetPayload) =>
  invoke<number>('test_dataset_save', { payload })

export const testDatasetDelete = (id: number) => invoke<void>('test_dataset_delete', { id })

export const testPreflight = (suiteId: number, envId?: number) =>
  invoke<PreflightReport>('test_preflight', { suiteId, envId: envId ?? null })

export const testRunStart = (
  suiteId: number,
  envId?: number,
  overrides?: { failFast?: boolean; vars?: Record<string, string>; secrets?: Record<string, string> },
  trigger: 'ui' | 'ai' | 'cli' = 'ui',
) => invoke<number>('test_run_start', { suiteId, envId: envId ?? null, overrides, trigger })

export const testRunStop = (runId: number) => invoke<void>('test_run_stop', { runId })

export const testRunGet = (runId: number) => invoke<RunDetail>('test_run_get', { runId })

export const testRunList = (suiteId?: number, page = 1, size = 20) =>
  invoke<TestRun[]>('test_run_list', { suiteId: suiteId ?? null, page, size })

export const testRunCurrent = (suiteId?: number) =>
  invoke<RunDetail | null>('test_run_current', { suiteId: suiteId ?? null })

export const testReportGenerate = (
  runId: number,
  formats?: string[],
  outDir?: string,
) =>
  invoke<TestRunReport>('test_report_generate', {
    runId,
    formats: formats ?? null,
    outDir: outDir ?? null,
  })

export const testArtifactList = (runId: number) =>
  invoke<TestArtifact[]>('test_artifact_list', { runId })

export const testArtifactExport = (artifactId: number, path: string) =>
  invoke<void>('test_artifact_export', { artifactId, path })

/* ---------- v1.0.8：请求取消 / 二进制响应 ---------- */

/** 取消在途请求；返回 false 表示请求已结束（取消失败无所谓） */
export const httpCancel = (requestId: string) =>
  invoke<boolean>('http_cancel', { requestId })

/** 把 Base64 内容写成本地文件（另存为响应体） */
export const fileWriteBinary = (path: string, base64: string) =>
  invoke<void>('file_write_binary', { path, base64 })

/** 拷贝文件（响应体另存为，走流式拷贝不占内存）；返回字节数 */
export const fileCopy = (src: string, dst: string) =>
  invoke<number>('file_copy', { src, dst })

/** 把响应落盘的相对路径换成绝对路径（只接受 responses/ 前缀） */
export const responseFilePath = (relPath: string) =>
  invoke<string>('response_file_path', { relPath })

/* ---------- v1.0.8：备份 / 自检 / 诊断 ---------- */

export const backupList = () => invoke<BackupItem[]>('backup_list')

export const backupCreate = () => invoke<BackupItem>('backup_create')

/** 恢复备份：写入 restore.pending 标记，需重启应用生效 */
export const backupRestore = (name: string) => invoke<BackupItem>('backup_restore', { name })

export const backupCancelRestore = () => invoke<void>('backup_cancel_restore')

export const backupDelete = (name: string) => invoke<void>('backup_delete', { name })

export const backupDirPath = () => invoke<string>('backup_dir_path')

/** 数据库自检：deep=true 走 integrity_check（慢 1~2 个数量级） */
export const dbCheck = (deep = false) => invoke<CheckResult>('db_check', { deep })

/** 导出诊断包（日志尾巴 + 环境信息 + 表统计），返回写入路径 */
export const diagnosticsExport = (path: string) =>
  invoke<string>('diagnostics_export', { path })

/* ---------- W2：异地加密备份（G-06） ---------- */

export const backupTargetList = () => invoke<BackupTarget[]>('backup_target_list')

/** 新建 / 修改备份目标；password 留空表示保留原密码 */
export const backupTargetSave = (input: BackupTargetInput) =>
  invoke<BackupTarget>('backup_target_save', { input })

export const backupTargetDelete = (id: number) => invoke<void>('backup_target_delete', { id })

export const backupTargetTest = (id: number) =>
  invoke<BackupTargetTestReport>('backup_target_test', { id })

export const backupTargetRemoteList = (id: number) =>
  invoke<RemoteBackup[]>('backup_target_remote_list', { id })

/** 上传加密备份（口令不落库，仅本次使用） */
export const backupUpload = (id: number, passphrase: string) =>
  invoke<BackupUploadResult>('backup_upload', { id, passphrase })

/** 从异地备份恢复：写入待恢复标记，需重启应用生效 */
export const backupRestoreFromTarget = (id: number, name: string, passphrase: string) =>
  invoke<BackupItem>('backup_restore_from_target', { id, name, passphrase })

/* ---------- W2：Mock Server（G-04） ---------- */

export const mockRoutesList = (projectId: number) =>
  invoke<MockRoute[]>('mock_routes_list', { projectId })

export const mockRouteSave = (route: MockRouteInput) =>
  invoke<MockRoute>('mock_route_save', { route })

export const mockRouteDelete = (id: number) => invoke<void>('mock_route_delete', { id })

/** 按项目请求批量补路由（不覆盖手工编辑） */
export const mockRoutesGenerate = (projectId: number) =>
  invoke<MockRoute[]>('mock_routes_generate', { projectId })

export const mockRoutesClear = (projectId: number) =>
  invoke<number>('mock_routes_clear', { projectId })

export const mockStart = (payload: MockStartPayload) =>
  invoke<MockStatus>('mock_start', { payload })

export const mockStop = (projectId: number) => invoke<MockStatus>('mock_stop', { projectId })

export const mockStatus = (projectId: number) => invoke<MockStatus>('mock_status', { projectId })

export const mockStatuses = () => invoke<MockStatus[]>('mock_statuses')

export const mockLogs = (projectId: number) => invoke<MockLog[]>('mock_logs', { projectId })

export const mockLogsClear = (projectId: number) =>
  invoke<void>('mock_logs_clear', { projectId })

/* ---------- W2：导入导出补齐（G-07） ---------- */

/** 导出 Postman Collection v2.1（响应示例已脱敏） */
export const postmanExport = (projectId: number, path: string) =>
  invoke<PostmanExportReport>('postman_export', { projectId, path })

/** 导入 Bruno 工作目录（递归扫描 .bru） */
export const brunoImport = (path: string, projectId?: number) =>
  invoke<Project>('bruno_import', { path, projectId: projectId ?? null })

/** 从 URL 拉取 OpenAPI / Swagger JSON 并导入 */
export const openapiImportUrl = (payload: {
  url: string
  headers?: { key: string; value: string; enabled?: boolean }[]
  projectId?: number | null
  save?: boolean
}) => invoke<Project>('openapi_import_url', { payload })

export const openapiSourceList = (projectId: number) =>
  invoke<OpenApiSource[]>('openapi_source_list', { projectId })

export const openapiSourceDelete = (id: number) =>
  invoke<void>('openapi_source_delete', { id })

export const openapiSourceRefresh = (id: number) =>
  invoke<Project>('openapi_source_refresh', { id })

/* ---------- v1.0.8：密钥与主密钥 ---------- */

export const secretsStatus = () => invoke<SecretStatus>('secrets_status')

/** 口令模式解锁；未解锁时所有敏感值不可用 */
export const secretsUnlock = (password: string) =>
  invoke<SecretStatus>('secrets_unlock', { password })

/** 切换到口令模式：会用新密钥整体重加密已有密文 */
export const secretsSetPassphrase = (password: string, confirm: string) =>
  invoke<SecretStatus>('secrets_set_passphrase', { password, confirm })

/** 导出密钥包（换机 / 重装前必须做）；返回写入路径 */
export const secretsExport = (password: string, path: string) =>
  invoke<string>('secrets_export', { password, path })

export const secretsImport = (password: string, path: string) =>
  invoke<SecretStatus>('secrets_import', { password, path })

/** 清空全部密钥值，需显式 confirmed */
export const secretsReset = (confirmed: boolean) =>
  invoke<number>('secrets_reset', { confirmed })

/** 只读预检密钥包文件，避免导入时选错文件 */
export const secretsInspectPack = (path: string) =>
  invoke<KeyPack>('secrets_inspect_pack', { path })

/* ---------- W3：实时调试（SSE / WebSocket） ---------- */

export const realtimeConnect = (payload: RealtimeConnectPayload) =>
  invoke<RealtimeStatus>('realtime_connect', { payload })

export const realtimeSend = (payload: RealtimeSendPayload) =>
  invoke<void>('realtime_send', { payload })

export const realtimeClose = (sessionId: string) =>
  invoke<boolean>('realtime_close', { sessionId })

export const realtimeStatus = (sessionId: string) =>
  invoke<RealtimeStatus | null>('realtime_status', { sessionId })

/* ---------- W3：GraphQL Schema（G-10） ---------- */

export const graphqlIntrospect = (payload: GraphqlIntrospectPayload) =>
  invoke<GraphqlSchemaEntry>('graphql_introspect', { payload })

export const graphqlSchemaGet = (projectId: number, endpoint: string) =>
  invoke<GraphqlSchemaEntry | null>('graphql_schema_get', { projectId, endpoint })

export const graphqlSchemaList = (projectId: number) =>
  invoke<GraphqlSchemaMeta[]>('graphql_schema_list', { projectId })

export const graphqlSchemaDelete = (projectId: number, endpoint: string) =>
  invoke<boolean>('graphql_schema_delete', { projectId, endpoint })

/* ---------- W3：Git 友好工作区（G-11） ---------- */

export interface WorkspaceCounts {
  projects: number
  collections: number
  requests: number
  environments: number
}

export interface WorkspaceExportResult {
  path: string
  counts: WorkspaceCounts
  warnings: string[]
}

export interface WorkspaceConflict {
  kind: string
  id: number
  name: string
  reason: string
}

export interface WorkspaceImportResult {
  created: WorkspaceCounts
  updated: WorkspaceCounts
  skipped: number
  conflicts: WorkspaceConflict[]
  warnings: string[]
}

export const workspaceExportDir = (payload: { path: string; projectIds?: number[] }) =>
  invoke<WorkspaceExportResult>('workspace_export_dir', { payload })

export const workspaceImportDir = (payload: { path: string; overwrite?: boolean }) =>
  invoke<WorkspaceImportResult>('workspace_import_dir', { payload })


/** 清掉日志脱敏用的敏感值缓存（切换环境后调用） */
export const secretsClearCache = () => invoke<void>('secrets_clear_cache')
