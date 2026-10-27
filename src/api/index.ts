import { invoke } from '@tauri-apps/api/core'
import type {
  AppSettings,
  CleanupResult,
  CollectionNode,
  CookieItem,
  CurlParsed,
  DashboardMetrics,
  Environment,
  EnvVariable,
  HistoryItem,
  HistoryStats,
  HttpRequestDetail,
  HttpResult,
  IpLookupInfo,
  MonitorProcess,
  MonitorSnapshot,
  PlanSummary,
  Project,
  ReportDetail,
  ReportRequestsPage,
  RequestRun,
  RequestSearchHit,
  RequestSummary,
  SendRequestPayload,
  StorageInfo,
  StressConfig,
  StressReport,
  TestPlan,
  UpdateInfo,
  WorkspaceStats,
} from '@/types/models'

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

export const requestDelete = (id: number) => invoke<void>('request_delete', { id })

/** 拖拽排序：按 ids 的顺序重排某集合下的请求 */
export const requestReorder = (collectionId: number, ids: number[]) =>
  invoke<void>('request_reorder', { collectionId, ids })

// 单接口运行记录
export const runList = (requestId: number, limit = 200) =>
  invoke<RequestRun[]>('run_list', { requestId, limit })

export const runClear = (requestId: number) => invoke<void>('run_clear', { requestId })

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

export const envVarSave = (envId: number, variables: EnvVariable[]) =>
  invoke<void>('env_var_save', { envId, variables })

export const envScriptSave = (id: number, script: string) =>
  invoke<void>('env_script_save', { id, script })

// 历史
export const historyList = (limit = 100) => invoke<HistoryItem[]>('history_list', { limit })

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

// 全局网络与安全设置（代理 / 客户端证书）
export const settingsGet = () => invoke<AppSettings>('settings_get')

export const settingsSet = (settings: AppSettings) =>
  invoke<AppSettings>('settings_set', { settings })

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

// cURL 导入
export const curlParse = (text: string) => invoke<CurlParsed>('curl_parse', { text })

// 项目导入导出
export const projectExport = (projectId: number, path: string) =>
  invoke<void>('project_export', { projectId, path })

export const projectImport = (path: string) => invoke<Project>('project_import', { path })

// OpenAPI 3.0 导出
export const openapiExport = (projectId: number, path: string) =>
  invoke<void>('openapi_export', { projectId, path })

// OpenAPI / Swagger 导入（projectId 为空时新建项目）
export const openapiImport = (path: string, projectId?: number | null) =>
  invoke<Project>('openapi_import', { path, projectId: projectId ?? null })

// 自动更新
export const updateCheck = () => invoke<UpdateInfo>('update_check')

export const updateDownload = (urls: string[], fileName: string) =>
  invoke<string>('update_download', { urls, fileName })

export const openExternal = (target: string) => invoke<void>('open_external', { target })

// Postman（Collection v2.0/v2.1）与 Apifox（OpenAPI）导入
export const apiImport = (path: string, projectId?: number | null) =>
  invoke<Project>('api_import', { path, projectId: projectId ?? null })

// 工具箱
export const toolHash = (text: string, algorithm: string) =>
  invoke<string>('tool_hash', { text, algorithm })

export const toolHmac = (text: string, key: string, algorithm: string) =>
  invoke<string>('tool_hmac', { text, key, algorithm })

// 系统监视
export const winstatsSnapshot = () => invoke<MonitorSnapshot>('winstats_snapshot')

export const winstatsProcesses = (limit = 12, byMemory = false) =>
  invoke<MonitorProcess[]>('winstats_processes', { limit, byMemory })

// 数据存储位置
export const storageInfo = () => invoke<StorageInfo>('storage_info')

export const storageSet = (path: string, migrate: boolean) =>
  invoke<StorageInfo>('storage_set', { path, migrate })

export const storageReset = () => invoke<StorageInfo>('storage_reset')

export const storageOpen = (path: string) => invoke<void>('storage_open', { path })

export const appRestart = () => invoke<void>('app_restart')
