import { invoke } from '@tauri-apps/api/core'
import type {
  CollectionNode,
  CurlParsed,
  Environment,
  EnvVariable,
  HistoryItem,
  HttpRequestDetail,
  HttpResult,
  IpLookupInfo,
  MonitorProcess,
  MonitorSnapshot,
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
  UpdateInfo,
  WorkspaceStats,
} from '@/types/models'

export { appInfo, projectList, projectCreate, projectUpdate, projectDelete } from './project'

// 工作台总览
export const workspaceStats = () => invoke<WorkspaceStats>('workspace_stats')

// 集合
export const collectionTree = (projectId: number) =>
  invoke<CollectionNode[]>('collection_tree', { projectId })

export const collectionCreate = (projectId: number, parentId: number | null, name: string) =>
  invoke<CollectionNode>('collection_create', { projectId, parentId, name })

export const collectionRename = (id: number, name: string) =>
  invoke<void>('collection_rename', { id, name })

export const collectionDelete = (id: number) => invoke<void>('collection_delete', { id })

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

// 单接口运行记录
export const runList = (requestId: number, limit = 50) =>
  invoke<RequestRun[]>('run_list', { requestId, limit })

export const runClear = (requestId: number) => invoke<void>('run_clear', { requestId })

export const sendRequest = (payload: SendRequestPayload) =>
  invoke<HttpResult>('send_http_request', { payload })

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

export const historyClear = () => invoke<void>('history_clear')

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

// 自动更新
export const updateCheck = () => invoke<UpdateInfo>('update_check')

export const updateDownload = (url: string, fileName: string) =>
  invoke<string>('update_download', { url, fileName })

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
