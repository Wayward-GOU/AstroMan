import {
  aiExecute,
  aiToolLogAdd,
  apiImport,
  collectionCreate,
  collectionDelete,
  collectionRename,
  collectionTree,
  cookieList,
  curlParse,
  dashboardMetrics,
  envActivate,
  envCreate,
  envDelete,
  envList,
  envScriptSave,
  envVarSave,
  fileWriteText,
  historyList,
  historyStats,
  openapiExport,
  openapiImport,
  planList,
  planGet,
  planDelete,
  planSave,
  projectCreate,
  projectDelete,
  projectExport,
  projectImport,
  projectList,
  projectUpdate,
  reportExport,
  reportGet,
  reportList,
  reportDelete,
  reportRequests,
  requestDelete,
  requestGet,
  requestList,
  requestOptions,
  requestSave,
  requestSearch,
  runList,
  stressStart,
  stressStop,
  toolHash,
  toolHmac,
  workspaceStats,
} from '@/api'
import type {
  CookieItem,
  DashboardMetrics,
  EnvVariable,
  HistoryStats,
  HttpRequestDetail,
  HttpResult,
  KeyValueItem,
  ReportDetail,
  ReportRequestRecord,
  ReportRequestsPage,
  RequestAuth,
  RequestRun,
  SendRequestPayload,
  SlaTargets,
  StressConfig,
  StressReport,
  StressStage,
} from '@/types/models'
import { diffAsText, diffLines } from '@/tools/analysis'
import { timestampInfo, uuidV4 } from '@/tools/calc'
import {
  csvToJson,
  jsonToCsv,
  jsonToMarkdown,
  jsonToQuery,
  jsonToSql,
  jsonToXml,
  jsonToYaml,
  parseJson,
  queryToJson,
  stringifyJson,
  xmlToJson,
  yamlToJson,
} from '@/tools/converters'
import { decodeJwt, runRegex } from '@/tools/crypto'
import { jsonToEntity, type EntityTarget } from '@/tools/entity'
import { urlInfo } from '@/tools/network'
import {
  base64Decode,
  base64Encode,
  base64UrlDecode,
  base64UrlEncode,
} from '@/tools/transforms'

/** 工具参数：模型输出的 JSON 对象 */
export type ToolArgs = Record<string, unknown>

export interface ToolResult {
  ok: boolean
  /** 回灌给模型 / 展示给用户的文本 */
  output: string
  raw?: unknown
  durationMs: number
  /** 一句话摘要：界面卡片先展示摘要，详情里再看全文 */
  summary?: string
  /** 列表类结果的总条数（模型直接引用，不要逐条数） */
  total?: number
  /** 结果是否被截断（true 时 total 仍然准确） */
  truncated?: boolean
}

interface ToolSpec {
  name: string
  desc: string
  args: string
  handler: (args: ToolArgs) => Promise<unknown>
  format?: (value: unknown) => string
  /** 会修改 / 删除数据或启动压测，必须由用户确认后再执行 */
  danger?: boolean
}

/* ---------------- 参数 JSON Schema 强校验（调度器硬规则） ---------------- */

type ArgKind = 'string' | 'number' | 'boolean' | 'object' | 'array' | 'any'

interface ArgRule {
  kind: ArgKind
  required?: boolean
  /** 枚举范围 */
  enum?: readonly string[]
  /** 正则约束 */
  pattern?: RegExp
  /** 数字大小 / 数组长度下限 */
  min?: number
  /** 数字大小 / 数组长度上限 */
  max?: number
  /** 数组元素类型 */
  item?: 'string' | 'number'
}

const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const
const BODY_TYPES = ['none', 'json', 'raw', 'form', 'multipart', 'binary', 'graphql'] as const
/** 必须是本机绝对路径：C:\ / C:/ / \\server / /root */
const ABS_PATH = /^(?:[A-Za-z]:[\\/]|\\\\|\/)/

/** 每个工具的入参约束；未声明 kind 的字段一律按 any 处理，缺失即非法 */
const SCHEMAS: Record<string, Record<string, ArgRule>> = {
  project_list: {},
  project_request_counts: {},
  project_create: {
    name: { kind: 'string', required: true },
    description: { kind: 'string' },
  },
  project_update: {
    id: { kind: 'number', required: true },
    name: { kind: 'string' },
    description: { kind: 'string' },
  },
  project_delete: { id: { kind: 'number', required: true } },
  collection_tree: { projectId: { kind: 'number', required: true } },
  collection_create: {
    projectId: { kind: 'number', required: true },
    parentId: { kind: 'number' },
    name: { kind: 'string', required: true },
  },
  collection_rename: {
    id: { kind: 'number', required: true },
    name: { kind: 'string', required: true },
  },
  collection_delete: { id: { kind: 'number', required: true } },
  request_list: { collectionId: { kind: 'number', required: true } },
  request_get: { id: { kind: 'number', required: true } },
  request_search: {
    projectId: { kind: 'number', required: true },
    keyword: { kind: 'string', required: true },
  },
  request_options: { projectId: { kind: 'number', required: true } },
  request_create: {
    collectionId: { kind: 'number', required: true },
    name: { kind: 'string', required: true },
    method: { kind: 'string', required: true, enum: METHODS },
    url: { kind: 'string', required: true },
    headers: { kind: 'any' },
    params: { kind: 'any' },
    bodyType: { kind: 'string', enum: BODY_TYPES },
    body: { kind: 'any' },
    preScript: { kind: 'string' },
    postScript: { kind: 'string' },
    assertions: { kind: 'string' },
    auth: { kind: 'any' },
  },
  send_request: {
    method: { kind: 'string', required: true, enum: METHODS },
    url: { kind: 'string', required: true },
    headers: { kind: 'any' },
    params: { kind: 'any' },
    bodyType: { kind: 'string', enum: BODY_TYPES },
    body: { kind: 'any' },
    envId: { kind: 'number' },
    // 硬规则 5：普通 HTTP 调试最长 30 秒
    timeoutMs: { kind: 'number', min: 1000, max: 30000 },
    cookieEnabled: { kind: 'boolean' },
  },
  curl_import: {
    curl: { kind: 'string', required: true },
    collectionId: { kind: 'number' },
    name: { kind: 'string' },
  },
  request_update: {
    id: { kind: 'number', required: true },
    name: { kind: 'string' },
    method: { kind: 'string', enum: METHODS },
    url: { kind: 'string' },
    headers: { kind: 'any' },
    params: { kind: 'any' },
    bodyType: { kind: 'string', enum: BODY_TYPES },
    body: { kind: 'any' },
    preScript: { kind: 'string' },
    postScript: { kind: 'string' },
    assertions: { kind: 'string' },
    auth: { kind: 'any' },
  },
  request_script_save: {
    id: { kind: 'number', required: true },
    preScript: { kind: 'string' },
    postScript: { kind: 'string' },
    assertions: { kind: 'string' },
  },
  request_delete: { id: { kind: 'number', required: true } },
  env_list: { projectId: { kind: 'number', required: true } },
  history_list: {
    limit: { kind: 'number', min: 1, max: 20000 },
    requestId: { kind: 'number' },
    projectId: { kind: 'number' },
  },
  history_export: {
    path: { kind: 'string', required: true, pattern: ABS_PATH },
    requestId: { kind: 'number' },
    projectId: { kind: 'number' },
    limit: { kind: 'number', min: 1, max: 20000 },
    format: { kind: 'string', enum: ['csv', 'json'] },
  },
  report_list: { limit: { kind: 'number', min: 1, max: 200 } },
  report_get: { id: { kind: 'number', required: true } },
  env_create: {
    projectId: { kind: 'number', required: true },
    name: { kind: 'string', required: true },
  },
  env_var_save: {
    projectId: { kind: 'number', required: true },
    envId: { kind: 'number', required: true },
    variables: { kind: 'array' },
    removeKeys: { kind: 'array', item: 'string' },
  },
  env_activate: {
    projectId: { kind: 'number', required: true },
    id: { kind: 'number', required: true },
  },
  plan_list: { projectId: { kind: 'number', required: true } },
  plan_get: { id: { kind: 'number', required: true } },
  plan_delete: { id: { kind: 'number', required: true } },
  plan_save: {
    projectId: { kind: 'number', required: true },
    requestId: { kind: 'number' },
    name: { kind: 'string', required: true },
    mode: { kind: 'string', enum: ['fixed', 'stages'] },
    concurrent: { kind: 'number', min: 1, max: 500 },
    durationSec: { kind: 'number', min: 1, max: 3600 },
    rampUpSec: { kind: 'number', min: 0, max: 3600 },
    thinkTimeMs: { kind: 'number', min: 0, max: 600000 },
    targetRps: { kind: 'number', min: 1, max: 100000 },
    stages: { kind: 'any' },
    slaP95: { kind: 'number', min: 1 },
    slaP99: { kind: 'number', min: 1 },
    slaErrorRate: { kind: 'number', min: 0 },
    slaMinRps: { kind: 'number', min: 0 },
    envId: { kind: 'number' },
  },
  stress_start: {
    requestId: { kind: 'number' },
    url: { kind: 'string' },
    method: { kind: 'string', enum: METHODS },
    name: { kind: 'string' },
    headers: { kind: 'any' },
    params: { kind: 'any' },
    bodyType: { kind: 'string', enum: BODY_TYPES },
    body: { kind: 'any' },
    envId: { kind: 'number' },
    concurrent: { kind: 'number', min: 1, max: 500 },
    durationSec: { kind: 'number', min: 1, max: 3600 },
    rampUpSec: { kind: 'number', min: 0, max: 3600 },
    thinkTimeMs: { kind: 'number', min: 0, max: 600000 },
    targetRps: { kind: 'number', min: 1, max: 100000 },
    stages: { kind: 'any' },
    slaP95: { kind: 'number', min: 1 },
    slaP99: { kind: 'number', min: 1 },
    slaErrorRate: { kind: 'number', min: 0 },
    slaMinRps: { kind: 'number', min: 0 },
  },
  stress_stop: {},
  run_batch: {
    requestIds: { kind: 'array', required: true, item: 'number', min: 1 },
    rounds: { kind: 'number', min: 1, max: 100 },
    envId: { kind: 'number' },
    stopOnError: { kind: 'boolean' },
    // 硬规则 5：批量执行同样按 30 秒单请求超时
    timeoutMs: { kind: 'number', min: 1000, max: 30000 },
  },
  project_import: {
    path: { kind: 'string', required: true, pattern: ABS_PATH },
  },
  project_export: {
    projectId: { kind: 'number', required: true },
    path: { kind: 'string', required: true, pattern: ABS_PATH },
  },
  api_import: {
    path: { kind: 'string', required: true, pattern: ABS_PATH },
    projectId: { kind: 'number' },
  },
  openapi_import: {
    path: { kind: 'string', required: true, pattern: ABS_PATH },
    projectId: { kind: 'number' },
  },
  openapi_export: {
    projectId: { kind: 'number', required: true },
    path: { kind: 'string', required: true, pattern: ABS_PATH },
  },
  report_export: {
    id: { kind: 'number', required: true },
    path: { kind: 'string', required: true, pattern: ABS_PATH },
  },
  workspace_stats: {},
  dashboard_metrics: {},
  /* ---------------- 阶段四：业务工具补齐 ---------------- */
  env_delete: { id: { kind: 'number', required: true } },
  env_script_save: {
    id: { kind: 'number', required: true },
    script: { kind: 'string' },
  },
  report_delete: { id: { kind: 'number', required: true } },
  report_requests: {
    id: { kind: 'number', required: true },
    page: { kind: 'number', min: 1 },
    pageSize: { kind: 'number', min: 1, max: 200 },
    success: { kind: 'boolean' },
  },
  history_stats: {},
  run_list: {
    requestId: { kind: 'number', required: true },
    limit: { kind: 'number', min: 1, max: 500 },
  },
  cookie_list: {},
  /* ---------------- 阶段四：工具箱（复用 src/tools/*） ---------------- */
  toolbox_json_format: {
    text: { kind: 'string', required: true },
    indent: { kind: 'number', min: 0, max: 8 },
  },
  toolbox_json_to_entity: {
    text: { kind: 'string', required: true },
    target: { kind: 'string', required: true, enum: ['java', 'csharp', 'go', 'ts'] },
    rootName: { kind: 'string' },
    packageName: { kind: 'string' },
    lombok: { kind: 'boolean' },
    comments: { kind: 'boolean' },
  },
  toolbox_json_to_sql: {
    text: { kind: 'string', required: true },
    table: { kind: 'string', required: true },
    mode: { kind: 'string', enum: ['single', 'batch'] },
  },
  toolbox_convert: {
    text: { kind: 'string', required: true },
    kind: {
      kind: 'string',
      required: true,
      enum: [
        'json2yaml',
        'yaml2json',
        'json2xml',
        'xml2json',
        'csv2json',
        'json2csv',
        'json2markdown',
        'json2query',
        'query2json',
      ],
    },
    delimiter: { kind: 'string' },
    rootName: { kind: 'string' },
  },
  toolbox_timestamp: { input: { kind: 'string' } },
  toolbox_base64: {
    text: { kind: 'string', required: true },
    action: { kind: 'string', required: true, enum: ['encode', 'decode', 'urlEncode', 'urlDecode'] },
  },
  toolbox_hash: {
    text: { kind: 'string', required: true },
    algorithm: { kind: 'string', required: true, enum: ['MD5', 'SHA1', 'SHA256', 'SHA512'] },
    hmacKey: { kind: 'string' },
  },
  toolbox_url: { text: { kind: 'string', required: true } },
  toolbox_regex: {
    pattern: { kind: 'string', required: true },
    text: { kind: 'string', required: true },
    flags: { kind: 'string' },
  },
  toolbox_jwt: { token: { kind: 'string', required: true } },
  toolbox_diff: {
    left: { kind: 'string', required: true },
    right: { kind: 'string', required: true },
    format: { kind: 'string', enum: ['text', 'json'] },
  },
  toolbox_uuid: { count: { kind: 'number', min: 1, max: 20 } },
  /* ---------------- 阶段四：工具目录检索 ---------------- */
  tool_search: { keyword: { kind: 'string', required: true } },
}

function checkRule(key: string, value: unknown, rule: ArgRule): string | null {
  if (value === undefined || value === null || value === '') {
    return rule.required ? `缺少必填参数 ${key}` : null
  }
  switch (rule.kind) {
    case 'any':
      return null
    case 'string': {
      const text =
        typeof value === 'string' ? value : typeof value === 'number' ? String(value) : ''
      if (!text) return `参数 ${key} 必须是字符串`
      if (rule.enum && !rule.enum.includes(text)) {
        return `参数 ${key} 必须是 ${rule.enum.join(' / ')} 之一`
      }
      if (rule.pattern && !rule.pattern.test(text)) {
        return `参数 ${key} 需要本机绝对路径（如 D:\\data\\api.json）`
      }
      return null
    }
    case 'number': {
      const num =
        typeof value === 'number'
          ? value
          : typeof value === 'string' && value.trim() !== ''
            ? Number(value)
            : Number.NaN
      if (!Number.isFinite(num)) return `参数 ${key} 必须是数字`
      if (rule.min !== undefined && num < rule.min) return `参数 ${key} 不能小于 ${rule.min}`
      if (rule.max !== undefined && num > rule.max) return `参数 ${key} 不能大于 ${rule.max}`
      return null
    }
    case 'boolean':
      return typeof value === 'boolean' ? null : `参数 ${key} 必须是布尔值`
    case 'object':
      return typeof value === 'object' && !Array.isArray(value)
        ? null
        : `参数 ${key} 必须是对象`
    case 'array': {
      if (!Array.isArray(value)) return `参数 ${key} 必须是数组`
      if (rule.min !== undefined && value.length < rule.min) {
        return `参数 ${key} 至少需要 ${rule.min} 项`
      }
      if (rule.max !== undefined && value.length > rule.max) {
        return `参数 ${key} 最多 ${rule.max} 项`
      }
      if (rule.item === 'number' && value.some((entry) => !Number.isFinite(Number(entry)))) {
        return `参数 ${key} 必须是数字数组`
      }
      if (rule.item === 'string' && value.some((entry) => typeof entry !== 'string')) {
        return `参数 ${key} 必须是字符串数组`
      }
      return null
    }
    default:
      return null
  }
}

/** 校验工具入参；返回中文错误（作为 Observation 回灌给模型），合法返回 null */
export function validateToolArgs(tool: string, args: ToolArgs): string | null {
  if (typeof args !== 'object' || args === null || Array.isArray(args)) {
    return '参数必须是 JSON 对象'
  }
  const schema = SCHEMAS[tool] ?? {}
  for (const [key, rule] of Object.entries(schema)) {
    const error = checkRule(key, args[key], rule)
    if (error) return error
  }
  return null
}

/* ---------------- 数据脱敏（禁止明文密钥进入模型上下文） ---------------- */

function maskValue(value: string): string {
  if (!value) return value
  if (value.length <= 6) return '******'
  return `${value.slice(0, 4)}******${value.slice(-2)}`
}

/** 对 token / secret / password / Authorization 等字段做脱敏 */
export function maskSensitive(text: string): string {
  if (!text) return text
  let out = text.replace(
    /((?:Bearer|Basic)\s+)([A-Za-z0-9._~+/=-]{6,})/gi,
    (_match, head: string, value: string) => `${head}${maskValue(value)}`,
  )
  out = out.replace(
    /("[^"]*(?:api[_-]?key|token|secret|password|authorization|cookie|private[_-]?key)[^"]*"\s*:\s*)"([^"]*)"/gi,
    (_match, key: string, value: string) => `${key}"${maskValue(value)}"`,
  )
  out = out.replace(
    /((?:api[_-]?key|token|secret|password|appsecret|clientsecret|authorization|cookie)\s*[:=]\s*)(?!\*{2})([^\s,;}\]"'&]+)/gi,
    (_match, head: string, value: string) => `${head}${maskValue(value)}`,
  )
  return out
}

/* ---------------- 参数归一化 ---------------- */

function clip(text: string, limit: number): string {
  if (text.length <= limit) return text
  return `${text.slice(0, limit)}\n…（已截断，共 ${text.length} 字符）`
}

/**
 * 列表类工具统一输出结构：明确给出 total，最多附 50 条明细。
 * 模型可以直接引用 total，不需要逐条数，也不会把「截断」误当成真实数量。
 */
function summarizeList(list: unknown[]): Record<string, unknown> {
  // 明细限 50 条：total 放在最前，即使输出被二次截断，模型也能拿到准确总数
  const items = list.slice(0, 50)
  return { total: list.length, shown: items.length, truncated: list.length > items.length, items }
}

/** 工具结果摘要：界面卡片先展示这一行，全文收进「详情」 */
function envelopeOf(
  tool: string,
  raw: unknown,
): {
  summary: string
  total?: number
  truncated?: boolean
} {
  if (Array.isArray(raw)) {
    return { summary: `共 ${raw.length} 条记录`, total: raw.length, truncated: false }
  }
  if (typeof raw === 'string') {
    const line = raw.split('\n').find((item) => item.trim()) ?? ''
    return { summary: line.length > 120 ? `${line.slice(0, 120)}…` : line || '已完成' }
  }
  if (raw && typeof raw === 'object') {
    const record = raw as Record<string, unknown>
    const total = Number(record.total)
    if (Number.isFinite(total) && Array.isArray(record.items)) {
      const shown = record.items.length
      const truncated = record.truncated === true || total > shown
      return {
        summary: `共 ${total} 条${truncated ? `，仅展示前 ${shown} 条` : ''}`,
        total,
        truncated,
      }
    }
    if (Number.isFinite(total) && 'total' in record) {
      return { summary: `共 ${total} 条`, total }
    }
    if ('deleted' in record) {
      return { summary: `已删除 #${str(record.deleted)}` }
    }
    if (typeof record.name === 'string' && record.name) {
      return { summary: `完成：${record.name}${record.id ? `（#${record.id}）` : ''}` }
    }
    if (record.摘要) {
      return { summary: `${str(record.算法, toolLabel(tool))}：${str(record.摘要)}` }
    }
    if (record.说明) return { summary: str(record.说明) }
  }
  return { summary: `${toolLabel(tool)}已完成` }
}

function str(value: unknown, fallback = ''): string {
  if (value === null || value === undefined) return fallback
  return typeof value === 'string' ? value : String(value)
}

/* ---------------- 中文标签：英文枚举 / 数字 ID 一律先翻译再交给模型 ---------------- */

const REPORT_STATUS_LABELS: Record<string, string> = {
  completed: '已完成',
  running: '运行中',
  stopped: '已停止',
  aborted: '已中止',
  failed: '失败',
  pending: '排队中',
  queued: '排队中',
  interrupted: '已中断',
}

const STRESS_MODE_LABELS: Record<string, string> = {
  fixed: '固定并发',
  ramp: '渐进加压',
  rampup: '渐进加压',
  stage: '分阶段加压',
  stages: '分阶段加压',
  duration: '定时长',
  count: '定次数',
}

const BODY_TYPE_LABELS: Record<string, string> = {
  none: '无请求体',
  json: 'JSON',
  form: '表单',
  formdata: '表单',
  'form-data': '表单文件上传',
  multipart: '表单文件上传',
  urlencoded: 'URL 编码表单',
  'x-www-form-urlencoded': 'URL 编码表单',
  raw: '原始文本',
  text: '纯文本',
  xml: 'XML',
  binary: '二进制',
  graphql: 'GraphQL',
}

const ERROR_TYPE_LABELS: Record<string, string> = {
  http: 'HTTP 错误状态码',
  timeout: '请求超时',
  tls: '证书 / TLS 握手失败',
  connect: '连接失败',
  dns: 'DNS 解析失败',
  network: '网络异常',
  assertion: '断言失败',
  unknown: '未知错误',
}

/** 命中映射就返回中文，否则原样返回（含空值兜底） */
function cnLabel(map: Record<string, string>, value: unknown, fallback = ''): string {
  const raw = str(value).trim()
  if (!raw) return fallback
  return map[raw.toLowerCase()] ?? raw
}

function fmtTime(ms?: number | null): string {
  if (!ms) return '—'
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function pct(part: number, total: number): string {
  if (!total) return '0%'
  return `${((part / total) * 100).toFixed(1)}%`
}

function fmtBytes(bytes: number): string {
  if (!bytes) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
}

function flattenCollections(nodes: unknown[]): { id: number; name: string }[] {
  const out: { id: number; name: string }[] = []
  const walk = (list: unknown[]) => {
    for (const entry of list) {
      if (!entry || typeof entry !== 'object') continue
      const node = entry as { id?: number; name?: string; children?: unknown[] }
      if (typeof node.id === 'number') out.push({ id: node.id, name: str(node.name) })
      if (Array.isArray(node.children)) walk(node.children)
    }
  }
  walk(nodes)
  return out
}

/** 报告里只有 requestId / envId，这里换成项目名 / 请求名 / 环境名，避免回答里出现裸数字 */
async function describeReportTarget(requestId?: number | null, envId?: number | null) {
  const target = { project: '', request: '', environment: '' }
  let projectId: number | undefined

  if (requestId) {
    try {
      const detail = await requestGet(requestId)
      target.request = `${detail.method} ${detail.name}`
      const projects = await projectList().catch(() => [])
      for (const project of projects) {
        const tree = await collectionTree(project.id).catch(() => [])
        const hit = flattenCollections(tree).some((node) => node.id === detail.collectionId)
        if (hit) {
          projectId = project.id
          target.project = project.name
          break
        }
      }
    } catch {
      target.request = '请求已删除'
    }
  }

  if (envId) {
    const projects = projectId ? [{ id: projectId, name: target.project }] : await projectList().catch(() => [])
    for (const project of projects) {
      const envs = await envList(project.id).catch(() => [])
      const hit = envs.find((item) => item.id === envId)
      if (hit) {
        target.environment = hit.name
        if (!target.project) target.project = project.name
        break
      }
    }
    if (!target.environment) target.environment = '环境已删除'
  }

  return target
}

/** 执行记录：项目名替代 projectId，时间格式化，避免回答里出现裸数字 */
function historyRows(
  rows: {
    requestName?: string | null
    projectId?: number | null
    method: string
    url: string
    statusCode?: number | null
    durationMs?: number | null
    createdAt: number
  }[],
  projectNames: Map<number, string>,
) {
  return rows.map((row) => ({
    接口: row.requestName || '未命名接口',
    方法: row.method,
    URL: row.url,
    状态码: row.statusCode ?? '无响应',
    耗时:
      row.durationMs === null || row.durationMs === undefined
        ? '—'
        : `${row.durationMs} 毫秒`,
    时间: fmtTime(row.createdAt),
    所属项目: row.projectId
      ? projectNames.get(row.projectId) ?? '项目已删除'
      : '未关联项目',
  }))
}

/** 接口类列表：保留 id 供后续调用，时间换成可读格式 */
function requestRows<
  T extends {
    id: number
    name: string
    method: string
    url: string
    updatedAt: number
    collectionName?: string
  },
>(rows: T[]) {
  return rows.map((row) => ({
    id: row.id,
    name: row.name,
    method: row.method,
    url: row.url,
    ...(row.collectionName ? { 所属集合: row.collectionName } : {}),
    更新时间: fmtTime(row.updatedAt),
  }))
}

/** 工作台总览：字段名中文化，避免回答里出现 projectCount 这类 key */
function workspaceDigest(stats: {
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
}) {
  return {
    项目数: stats.projectCount,
    集合数: stats.collectionCount,
    接口数: stats.requestCount,
    环境数: stats.environmentCount,
    执行记录数: stats.historyCount,
    压测报告数: stats.reportCount,
    压测请求总数: stats.reportRequestTotal,
    压测成功总数: stats.reportSuccessTotal,
    压测整体成功率: pct(stats.reportSuccessTotal, stats.reportRequestTotal),
    近7天请求数: stats.recentHistory,
    近7天新增报告: stats.recentReports,
  }
}

/** 数据大屏指标：分组标签、耗时、成功率全部转中文与可读单位 */
function dashboardDigest(metrics: DashboardMetrics) {
  return {
    最近14天趋势: metrics.days.map((item) => ({
      日期: item.date,
      请求数: item.total,
      成功数: item.success,
      成功率: pct(item.success, item.total),
      平均响应: `${Math.round(item.avgRt)} 毫秒`,
    })),
    状态码分布: metrics.codeBuckets.map((item) => ({ 区间: item.label, 次数: item.count })),
    请求方法分布: metrics.methodBuckets.map((item) => ({ 方法: item.label, 次数: item.count })),
    时段分布: metrics.hourBuckets.map((item) => ({ 时段: `${item.hour} 点`, 请求数: item.count })),
    响应耗时分布: metrics.latencyBuckets.map((item) => ({ 区间: item.label, 次数: item.count })),
    热门接口: metrics.topRequests.map((item) => ({
      接口: item.name,
      方法: item.method,
      URL: item.url,
      调用次数: item.count,
      平均响应: `${Math.round(item.avgRt)} 毫秒`,
      成功率: `${item.successRate.toFixed(1)}%`,
    })),
    最活跃项目: metrics.topProjects.map((item) => ({
      项目: item.name,
      调用次数: item.count,
      平均响应: `${Math.round(item.avgRt)} 毫秒`,
      成功率: `${item.successRate.toFixed(1)}%`,
    })),
    压测报告趋势: metrics.reportPoints.map((item) => ({
      报告名称: item.name,
      请求总数: item.totalRequests,
      P95响应: `${Math.round(item.p95Rt)} 毫秒`,
      错误率: `${item.errorRate.toFixed(1)}%`,
      生成时间: fmtTime(item.createdAt),
    })),
  }
}

/** 压测报告列表：状态、时间、成功率全部转成人能直接读的中文 */
function reportRows(list: StressReport[]): Record<string, unknown>[] {
  return list.map((item) => ({
    报告编号: item.id,
    名称: item.planName,
    状态: cnLabel(REPORT_STATUS_LABELS, item.status, '未知'),
    请求总数: item.totalRequests,
    成功数: item.successCount,
    失败数: item.errorCount,
    成功率: pct(item.successCount, item.totalRequests),
    平均响应: `${Math.round(item.avgRt)} 毫秒`,
    P95响应: `${Math.round(item.p95Rt)} 毫秒`,
    平均QPS: Number((item.avgQps ?? 0).toFixed(1)),
    并发数: item.concurrency,
    开始时间: fmtTime(item.startTime),
    创建时间: fmtTime(item.createdAt),
  }))
}

/** 压测报告详情：字段名、枚举值全部中文，且不再暴露 requestId / envId 这类裸数字 */
async function reportDigest(detail: ReportDetail) {
  const report = detail.report
  const config = detail.config
  const target = await describeReportTarget(report.requestId, config?.envId ?? null)

  const statusCodes = detail.statusCodes.map((item) => ({
    状态码: item.code,
    次数: item.count,
    占比: pct(item.count, report.totalRequests),
  }))

  const errorTypes = detail.errorTypes.map((item) => ({
    错误类型: cnLabel(ERROR_TYPE_LABELS, item.errorType, '未知错误'),
    次数: item.count,
    占比: pct(item.count, report.totalRequests),
  }))

  const histogram = detail.histogram.map((item) => ({
    区间: item.label,
    请求数: item.count,
    占比: pct(item.count, report.totalRequests),
  }))

  const stages = detail.stages.map((item) => ({
    阶段: item.index + 1,
    并发数: item.concurrent,
    时长: `${item.durationSec} 秒`,
    请求数: item.requests,
    失败数: item.errors,
    平均QPS: item.avgQps,
    平均响应: `${Math.round(item.avgRt)} 毫秒`,
    P95响应: `${Math.round(item.p95Rt)} 毫秒`,
  }))

  const recentErrors = detail.errors.slice(0, 10).map((item) => ({
    发生时间: `第 ${item.elapsedSec} 秒`,
    错误类型: cnLabel(ERROR_TYPE_LABELS, item.errorType, '未知错误'),
    错误信息: clip(item.errorMessage || '', 300),
    请求地址: item.requestUrl,
    响应状态码: item.responseStatus ?? '无响应',
    响应内容: clip(item.responseBody || '', 300),
  }))

  const samples = detail.samples.slice(-30).map((item) => ({
    第几秒: item.elapsedSec,
    并发: item.concurrent,
    QPS: item.qps,
    平均响应: item.avgRt,
    P95响应: item.p95Rt,
    成功数: item.successCount,
    失败数: item.errorCount,
  }))

  return {
    报告编号: report.id,
    报告名称: report.planName || target.request || '未命名报告',
    状态: cnLabel(REPORT_STATUS_LABELS, report.status, '未知'),
    压测目标: {
      项目: target.project || '未关联项目',
      请求: target.request || '未关联请求',
      环境: target.environment || '未指定环境',
    },
    时间: {
      开始: fmtTime(report.startTime),
      结束: fmtTime(report.endTime),
      生成: fmtTime(report.createdAt),
    },
    配置: config
      ? {
          加压模式: cnLabel(STRESS_MODE_LABELS, config.mode, '未记录'),
          并发数: report.concurrency,
          目标QPS: config.targetRps || '不限',
          爬坡时间: config.rampUpSec ? `${config.rampUpSec} 秒` : '无',
          请求体类型: cnLabel(BODY_TYPE_LABELS, config.bodyType, '未记录'),
          连接超时: config.timeoutMs ? `${config.timeoutMs} 毫秒` : '默认',
          建连超时: config.connectTimeoutMs ? `${config.connectTimeoutMs} 毫秒` : '默认',
          跟随重定向: config.followRedirects === undefined ? '未记录' : config.followRedirects ? '开启' : '关闭',
          请求间隔: config.thinkTimeMs ? `${config.thinkTimeMs} 毫秒` : '无',
          自动停止条件: config.stopErrorRate
            ? `错误率达到 ${config.stopErrorRate}%（且请求数 ≥ ${config.stopMinRequests ?? 0}）`
            : '未设置',
          请求头数量: config.headers ?? 0,
          查询参数数量: config.params ?? 0,
        }
      : '无配置快照',
    总体指标: {
      请求总数: report.totalRequests,
      成功数: report.successCount,
      失败数: report.errorCount,
      错误率: pct(report.errorCount, report.totalRequests),
      平均响应: `${Math.round(report.avgRt)} 毫秒`,
      最快响应: `${Math.round(report.minRt)} 毫秒`,
      最慢响应: `${Math.round(report.maxRt)} 毫秒`,
      P50: `${Math.round(report.p50Rt)} 毫秒`,
      P95: `${Math.round(report.p95Rt)} 毫秒`,
      P99: `${Math.round(report.p99Rt)} 毫秒`,
      平均QPS: report.avgQps,
      最高QPS: report.maxQps,
      Apdex: report.apdex,
      传输数据量: fmtBytes(report.bytesTotal),
    },
    状态码分布: statusCodes,
    错误类型分布: errorTypes,
    延迟直方图: histogram,
    阶段明细: stages.length ? stages : '单阶段压测',
    错误样本: recentErrors.length ? recentErrors : '无错误',
    采样点总数: detail.samples.length,
    最近采样: samples,
  }
}


function optionalNum(value: unknown): number | undefined {
  if (value === null || value === undefined || value === '') return undefined
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isFinite(parsed) ? parsed : undefined
}

/** 压测阶段：最多 20 段，并发 1-500，时长 1-3600 秒 */
function toStages(value: unknown): StressStage[] {
  if (!Array.isArray(value)) return []
  return value.slice(0, 20).map((item) => {
    const stage = (item ?? {}) as Record<string, unknown>
    return {
      concurrent: Math.min(500, Math.max(1, Math.round(optionalNum(stage.concurrent) ?? 1))),
      durationSec: Math.min(3600, Math.max(1, Math.round(optionalNum(stage.durationSec) ?? 30))),
    }
  })
}

/** SLA 目标：一个都没给时返回 null，避免空对象被当成启用了 SLA */
function slaOf(args: ToolArgs): SlaTargets | null {
  const p95 = optionalNum(args.slaP95)
  const p99 = optionalNum(args.slaP99)
  const errorRate = optionalNum(args.slaErrorRate)
  const minRps = optionalNum(args.slaMinRps)
  if (p95 === undefined && p99 === undefined && errorRate === undefined && minRps === undefined) {
    return null
  }
  return { p95Ms: p95 ?? null, p99Ms: p99 ?? null, errorRate: errorRate ?? null, minRps: minRps ?? null }
}

function needNum(value: unknown, name: string): number {
  const parsed = optionalNum(value)
  if (parsed === undefined) throw new Error(`参数 ${name} 必须是数字`)
  return parsed
}

/** 把模型给的 headers / params 归一化成 KeyValueItem[] */
function toItems(value: unknown): KeyValueItem[] {
  if (value === null || value === undefined || value === '') return []
  if (Array.isArray(value)) {
    const items: KeyValueItem[] = []
    for (const entry of value) {
      if (entry && typeof entry === 'object') {
        const obj = entry as ToolArgs
        const key = str(obj.key ?? obj.name).trim()
        if (!key) continue
        items.push({ key, value: str(obj.value), enabled: obj.enabled !== false })
      } else {
        const text = str(entry)
        const pos = text.indexOf(':')
        if (pos > 0) {
          items.push({ key: text.slice(0, pos).trim(), value: text.slice(pos + 1).trim(), enabled: true })
        } else if (text.trim()) {
          items.push({ key: text.trim(), value: '', enabled: true })
        }
      }
    }
    return items
  }
  if (typeof value === 'object') {
    return Object.entries(value as ToolArgs).map(([key, val]) => ({
      key,
      value: str(val),
      enabled: true,
    }))
  }
  return []
}

function bodyText(body: unknown): string {
  if (body === null || body === undefined) return ''
  if (typeof body === 'string') return body
  try {
    return JSON.stringify(body, null, 2)
  } catch {
    return String(body)
  }
}

function bodyTypeOf(args: ToolArgs): string {
  const given = str(args.bodyType).trim()
  if (given) return given
  const body = args.body
  if (body === null || body === undefined || body === '') return 'none'
  return 'json'
}

function pretty(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

/** 常见 HTTP 状态码 → 中文含义，避免回答里只剩 200 / 404 这样的数字 */
const HTTP_STATUS_CN: Record<number, string> = {
  200: '成功',
  201: '已创建',
  202: '已接受',
  204: '无内容',
  301: '永久重定向',
  302: '临时重定向',
  304: '未修改',
  400: '请求参数错误',
  401: '未授权 / 认证失败',
  403: '禁止访问',
  404: '资源不存在',
  405: '请求方法不允许',
  408: '请求超时',
  409: '资源冲突',
  413: '请求体过大',
  415: '媒体类型不支持',
  422: '参数校验失败',
  429: '请求过于频繁',
  500: '服务端内部错误',
  502: '网关错误',
  503: '服务不可用',
  504: '网关超时',
}

function statusLabel(status: number): string {
  if (!status) return '无响应'
  const text = HTTP_STATUS_CN[status]
  return text ? `${status} ${text}` : `${status}`
}

function formatHttpResult(value: unknown): string {
  const result = value as HttpResult
  const lines = [
    `状态：${statusLabel(result.status)}`,
    `耗时：${result.durationMs} 毫秒　大小：${fmtBytes(result.sizeBytes)}${result.truncated ? '（响应体已截断）' : ''}`,
  ]
  if (result.error) lines.push(`错误：${result.error}`)
  const headers = result.headers ?? {}
  const contentType = headers['content-type'] ?? headers['Content-Type']
  if (contentType) lines.push(`Content-Type：${contentType}`)
  const body = (result.body ?? '').slice(0, 4000)
  lines.push('响应体：', body || '（空）')
  return lines.join('\n')
}

/* ---------------- 工具实现 ---------------- */

function requestDetail(args: ToolArgs): HttpRequestDetail {
  const url = str(args.url).trim()
  if (!url) throw new Error('参数 url 不能为空')
  const detail: HttpRequestDetail = {
    collectionId: needNum(args.collectionId, 'collectionId'),
    name: str(args.name, 'AI 创建的请求').trim() || 'AI 创建的请求',
    method: str(args.method, 'GET').toUpperCase(),
    url,
    headers: toItems(args.headers),
    params: toItems(args.params ?? args.query),
    bodyType: bodyTypeOf(args),
    body: bodyText(args.body),
    preScript: str(args.preScript),
    postScript: str(args.postScript),
    assertions: str(args.assertions),
  }
  if (args.auth && typeof args.auth === 'object') {
    detail.auth = args.auth as RequestAuth
  }
  return detail
}

function sendPayload(args: ToolArgs): SendRequestPayload {
  const url = str(args.url).trim()
  if (!url) throw new Error('参数 url 不能为空')
  return {
    method: str(args.method, 'GET').toUpperCase(),
    url,
    headers: toItems(args.headers),
    params: toItems(args.params ?? args.query),
    bodyType: bodyTypeOf(args),
    body: bodyText(args.body),
    envId: optionalNum(args.envId) ?? null,
    timeoutMs: optionalNum(args.timeoutMs) ?? 30000,
    cookieEnabled: args.cookieEnabled !== false,
  }
}

function needPath(value: unknown): string {
  const path = str(value).trim()
  if (!path) throw new Error('参数 path 不能为空，需要用户提供本机绝对路径')
  return path
}

/** CSV 单元格转义 */
function csvCell(value: string): string {
  return /[",\n\r]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value
}

/** 已保存请求 → 可直接发送的载荷 */
function detailPayload(
  detail: HttpRequestDetail,
  extra: { envId?: number | null; timeoutMs?: number } = {},
): SendRequestPayload {
  return {
    requestId: detail.id ?? null,
    name: detail.name,
    method: detail.method,
    url: detail.url,
    headers: detail.headers,
    params: detail.params,
    bodyType: detail.bodyType,
    body: detail.body,
    envId: extra.envId ?? null,
    timeoutMs: extra.timeoutMs ?? 30000,
    auth: detail.auth ?? null,
    cookieEnabled: true,
  }
}

const SPECS: ToolSpec[] = [
  {
    name: 'project_list',
    desc: '列出本机全部项目（id / 名称 / 描述 / 时间）；回答里必须原样使用 name 字段',
    args: '无',
    handler: async () => {
      const projects = await projectList()
      return projects.map((item) => ({
        id: item.id,
        name: item.name,
        description: item.description ?? '',
        创建时间: fmtTime(item.createdAt),
        更新时间: fmtTime(item.updatedAt),
      }))
    },
  },
  {
    name: 'project_request_counts',
    desc: '按项目统计请求数量，返回真实项目名称 + 请求数（数值降序）；问「各项目请求数」时必须用它',
    args: '无',
    handler: async () => {
      const projects = await projectList()
      const rows = await Promise.all(
        projects.map(async (project) => {
          const tree = await collectionTree(project.id).catch(() => [])
          const ids: number[] = []
          const walk = (nodes: { id: number; children?: unknown[] }[]) => {
            for (const node of nodes) {
              ids.push(node.id)
              walk((node.children ?? []) as { id: number; children?: unknown[] }[])
            }
          }
          walk(tree as { id: number; children?: unknown[] }[])
          const lists = await Promise.all(ids.map((id) => requestList(id).catch(() => [])))
          return {
            id: project.id,
            name: project.name,
            请求数: lists.reduce((sum, list) => sum + list.length, 0),
          }
        }),
      )
      return rows.sort((a, b) => b.请求数 - a.请求数)
    },
  },
  {
    name: 'project_create',
    desc: '新建项目',
    args: '{ name, description? }',
    handler: (args) => projectCreate(str(args.name, '新项目').trim() || '新项目', str(args.description)),
  },
  {
    name: 'project_update',
    desc: '修改项目名称或描述（只传需要改的字段）',
    args: '{ id, name?, description? }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const current = (await projectList()).find((item) => item.id === id)
      if (!current) throw new Error('所选项目不存在，请先查询项目列表后再试')
      const name = str(args.name, current.name).trim() || current.name
      const description =
        args.description === undefined ? current.description : str(args.description)
      await projectUpdate(id, name, description ?? undefined)
      return { id, name, description }
    },
  },
  {
    name: 'project_delete',
    desc: '删除项目及其中全部集合与请求（危险操作，需用户确认）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const current = (await projectList()).find((item) => item.id === id)
      await projectDelete(id)
      return { deleted: id, name: current?.name ?? `#${id}` }
    },
  },
  {
    name: 'collection_tree',
    desc: '读取某项目下的集合树',
    args: '{ projectId }',
    handler: (args) => collectionTree(needNum(args.projectId, 'projectId')),
  },
  {
    name: 'collection_create',
    desc: '在项目下新建集合（parentId 为空表示顶级）',
    args: '{ projectId, parentId?, name }',
    handler: (args) =>
      collectionCreate(
        needNum(args.projectId, 'projectId'),
        optionalNum(args.parentId) ?? null,
        str(args.name, '新集合').trim() || '新集合',
      ),
  },
  {
    name: 'collection_rename',
    desc: '重命名集合',
    args: '{ id, name }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const name = str(args.name).trim()
      if (!name) throw new Error('参数 name 不能为空')
      await collectionRename(id, name)
      return { id, name }
    },
  },
  {
    name: 'collection_delete',
    desc: '删除集合及其下全部请求（危险操作，需用户确认）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      await collectionDelete(id)
      return { deleted: id }
    },
  },
  {
    name: 'request_list',
    desc: '列出某集合下的请求（返回 total 总条数，items 最多前 50 条；时间为本地格式）',
    args: '{ collectionId }',
    handler: async (args) =>
      summarizeList(requestRows(await requestList(needNum(args.collectionId, 'collectionId')))),
  },
  {
    name: 'request_get',
    desc: '读取单个请求的完整定义',
    args: '{ id }',
    handler: (args) => requestGet(needNum(args.id, 'id')),
  },
  {
    name: 'request_search',
    desc: '在项目内按关键字搜索请求（返回 total 命中数，items 最多前 50 条，含所属集合与更新时间）',
    args: '{ projectId, keyword }',
    handler: async (args) => {
      const hits = await requestSearch(
        needNum(args.projectId, 'projectId'),
        str(args.keyword),
      )
      const result: Record<string, unknown> = summarizeList(requestRows(hits))
      if (hits.length >= 100) {
        result.说明 = '搜索结果达到 100 条查询上限，实际命中可能更多，请补充关键字'
      }
      return result
    },
  },
  {
    name: 'request_options',
    desc: '列出项目内请求的 id / 名称 / 方法 / URL，便于挑选（返回 total 总条数，items 最多前 50 条）',
    args: '{ projectId }',
    handler: async (args) =>
      summarizeList(requestRows(await requestOptions(needNum(args.projectId, 'projectId')))),
  },
  {
    name: 'request_create',
    desc: '在集合下新建请求（自动保存到数据库）',
    args: '{ collectionId, name, method, url, headers?, params?, bodyType?, body?, preScript?, postScript?, assertions?, auth? }',
    handler: (args) => requestSave(requestDetail(args)),
  },
  {
    name: 'send_request',
    desc: '发送真实 HTTP 请求并返回状态码、耗时与响应体（与手动发送同一条链路）',
    args: '{ method, url, headers?, params?, bodyType?, body?, envId?, timeoutMs?, cookieEnabled? }',
    handler: (args) => aiExecute(sendPayload(args)),
    format: formatHttpResult,
  },
  {
    name: 'curl_import',
    desc: '解析 cURL 命令；传入 collectionId 时直接保存为新请求，否则只返回解析结果',
    args: '{ curl, collectionId?, name? }',
    handler: async (args) => {
      const curl = str(args.curl).trim()
      if (!curl) throw new Error('参数 curl 不能为空')
      const parsed = await curlParse(curl)
      const collectionId = optionalNum(args.collectionId)
      if (collectionId === undefined) {
        return { parsed }
      }
      const saved = await requestSave({
        collectionId,
        name: str(args.name, parsed.name).trim() || parsed.name || '导入的请求',
        method: parsed.method,
        url: parsed.url,
        headers: parsed.headers,
        params: parsed.params,
        bodyType: parsed.bodyType,
        body: parsed.body,
        preScript: '',
        postScript: '',
        assertions: '',
      })
      return {
        id: saved.id,
        name: saved.name,
        method: saved.method,
        url: saved.url,
        collectionId,
        warnings: parsed.warnings,
      }
    },
  },
  {
    name: 'request_update',
    desc: '修改已保存请求的字段（只传要改的字段，其余保留）',
    args: '{ id, name?, method?, url?, headers?, params?, bodyType?, body?, preScript?, postScript?, assertions?, auth? }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const current = await requestGet(id)
      const next: HttpRequestDetail = { ...current }

      if (args.name !== undefined) next.name = str(args.name, current.name).trim() || current.name
      if (args.method !== undefined) next.method = str(args.method, current.method).toUpperCase()
      if (args.url !== undefined) {
        const url = str(args.url, current.url).trim()
        if (!url) throw new Error('参数 url 不能为空')
        next.url = url
      }
      if (args.headers !== undefined) next.headers = toItems(args.headers)
      if (args.params !== undefined || args.query !== undefined) {
        next.params = toItems(args.params ?? args.query)
      }
      if (args.bodyType !== undefined || args.body !== undefined) {
        const fallback = current.bodyType === 'none' ? 'json' : current.bodyType
        next.bodyType = args.bodyType !== undefined ? bodyTypeOf(args) : fallback
        next.body = args.body !== undefined ? bodyText(args.body) : current.body
      }
      if (args.preScript !== undefined) next.preScript = str(args.preScript)
      if (args.postScript !== undefined) next.postScript = str(args.postScript)
      if (args.assertions !== undefined) next.assertions = str(args.assertions)
      if (args.auth && typeof args.auth === 'object') {
        next.auth = args.auth as RequestAuth
      }

      await requestSave(next)
      return { id: next.id ?? id, name: next.name, method: next.method, url: next.url }
    },
  },
  {
    name: 'request_script_save',
    desc: '给已保存的请求写入前置 / 后置脚本（只改脚本字段，其余保持不变），保存前会做语法检查',
    args: '{ id, preScript?, postScript?, assertions? }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const current = await requestGet(id)
      const next: HttpRequestDetail = { ...current }

      if (args.preScript !== undefined) next.preScript = str(args.preScript)
      if (args.postScript !== undefined) next.postScript = str(args.postScript)
      if (args.assertions !== undefined) next.assertions = str(args.assertions)

      for (const [field, code] of [
        ['preScript', next.preScript],
        ['postScript', next.postScript],
      ] as const) {
        if (!code.trim()) continue
        try {
          // 只构造不执行：脚本有语法错误时在这里抛出
          new Function(
            'request', 'response', 'vars', 'env', 'console', 'rsa', 'cipher', 'test', 'assert',
            code,
          )
        } catch (error) {
          throw new Error(`${field} 语法错误：${String(error)}`)
        }
      }

      await requestSave(next)
      return {
        id,
        name: next.name,
        preScriptLines: next.preScript.split('\n').length,
        postScriptLines: next.postScript.split('\n').length,
        note: '已保存，可在请求编辑器的「前置/后置脚本」标签页查看并试运行',
      }
    },
  },
  {
    name: 'request_delete',
    desc: '删除已保存的请求（危险操作，需用户确认）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const current = await requestGet(id)
      await requestDelete(id)
      return { deleted: id, name: current.name }
    },
  },
  {
    name: 'env_list',
    desc: '读取项目的环境与变量（密钥类变量的值会打码，不进入模型上下文）',
    args: '{ projectId }',
    handler: async (args) => {
      const envs = await envList(needNum(args.projectId, 'projectId'))
      return envs.map((env) => ({
        id: env.id,
        环境名称: env.name,
        是否当前环境: env.isActive ? '是' : '否',
        变量数: env.variables.length,
        变量: env.variables.map((item) => ({
          变量名: item.key,
          值: item.isSecret ? '******（密钥，已隐藏）' : item.value,
          是否密钥: item.isSecret ? '是' : '否',
        })),
        环境脚本行数: env.script ? env.script.split('\n').length : 0,
      }))
    },
  },
  {
    name: 'history_list',
    desc: '最近的请求历史，可按接口（requestId）或项目（projectId）过滤；返回接口名、项目名与本地时间',
    args: '{ limit?, requestId?, projectId? }',
    handler: async (args) => {
      const limit = Math.min(2000, Math.round(optionalNum(args.limit) ?? 20))
      const [rows, projects] = await Promise.all([
        historyList(limit, optionalNum(args.requestId) ?? null, optionalNum(args.projectId) ?? null),
        projectList().catch(() => []),
      ])
      const names = new Map(projects.map((item) => [item.id, item.name]))
      const items = historyRows(rows.slice(0, 50), names)
      return { total: rows.length, shown: items.length, items }
    },
  },
  {
    name: 'history_export',
    desc: '把执行记录导出成 CSV / JSON 文件：可按接口或项目过滤，例如「某个接口最近 50 条」「某个项目最近 100 条」',
    args: "{ path, requestId?, projectId?, limit?, format?: 'csv' | 'json' }",
    danger: true,
    handler: async (args) => {
      const path = needPath(args.path)
      const limit = Math.min(20000, Math.max(1, Math.round(optionalNum(args.limit) ?? 100)))
      const requestId = optionalNum(args.requestId) ?? null
      const projectId = optionalNum(args.projectId) ?? null
      const format = str(args.format, 'csv').toLowerCase() === 'json' ? 'json' : 'csv'

      const rows = await historyList(limit, requestId, projectId)
      if (!rows.length) throw new Error('没有符合条件的执行记录，未生成文件')

      if (format === 'json') {
        await fileWriteText(path, JSON.stringify(rows, null, 2))
      } else {
        const projects = await projectList().catch(() => [])
        const names = new Map(projects.map((item) => [item.id, item.name]))
        const header = ['接口名称', '所属项目', '方法', 'URL', '状态码', '耗时(ms)', '时间']
        const lines = rows.map((row) =>
          [
            row.requestName ?? '未命名接口',
            row.projectId ? names.get(row.projectId) ?? '项目已删除' : '未关联项目',
            row.method ?? '',
            row.url ?? '',
            row.statusCode ?? '',
            row.durationMs ?? '',
            fmtTime(row.createdAt),
          ]
            .map((cell) => csvCell(String(cell)))
            .join(','),
        )
        // BOM 让 Excel 正确识别 UTF-8
        await fileWriteText(path, `\uFEFF${header.join(',')}\n${lines.join('\n')}\n`)
      }
      return `已导出 ${rows.length} 条记录到 ${path}（${format.toUpperCase()}）`
    },
  },
  {
    name: 'report_list',
    desc: '压测报告列表（状态、时间、成功率已转中文）',
    args: '{ limit? }',
    handler: async (args) => reportRows(await reportList(optionalNum(args.limit) ?? 20)),
  },
  {
    name: 'report_get',
    desc: '压测报告详情（指标、状态码、错误、阶段均已转中文标签，项目/请求/环境显示名称而非 ID）',
    args: '{ id }',
    handler: async (args) => reportDigest(await reportGet(needNum(args.id, 'id'))),
  },
  {
    name: 'env_create',
    desc: '在项目下新建环境（如 dev / test / prod）',
    args: '{ projectId, name }',
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const name = str(args.name, '新环境').trim() || '新环境'
      const env = await envCreate(projectId, name)
      return { id: env.id, projectId, name: env.name }
    },
  },
  {
    name: 'env_var_save',
    desc: '新增或覆盖环境变量：同 key 覆盖、其他保留，removeKeys 用于删除变量',
    args: '{ projectId, envId, variables?: [{ key, value, isSecret? }], removeKeys?: string[] }',
    danger: true,
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const envId = needNum(args.envId, 'envId')
      const env = (await envList(projectId)).find((item) => item.id === envId)
      if (!env) throw new Error('所选环境不属于该项目，请先查询项目环境后再试')

      const removeKeys = Array.isArray(args.removeKeys)
        ? args.removeKeys.map((key) => str(key)).filter(Boolean)
        : []
      const merged: EnvVariable[] = env.variables
        .filter((item) => !removeKeys.includes(item.key))
        .map((item) => ({ ...item }))

      const incoming = Array.isArray(args.variables) ? args.variables : []
      for (const entry of incoming) {
        if (!entry || typeof entry !== 'object') continue
        const obj = entry as ToolArgs
        const key = str(obj.key ?? obj.name).trim()
        if (!key) continue
        const value = str(obj.value)
        const isSecret = obj.isSecret === true
        const existing = merged.find((item) => item.key === key)
        if (existing) {
          existing.value = value
          existing.isSecret = isSecret
        } else {
          merged.push({ key, value, isSecret, sortOrder: merged.length })
        }
      }
      merged.forEach((item, index) => {
        item.sortOrder = index
      })

      await envVarSave(envId, merged)
      return {
        环境: env.name,
        变量数: merged.length,
        变量: merged.map((item) => ({
          变量名: item.key,
          值: item.isSecret ? '******（密钥，已隐藏）' : item.value,
          是否密钥: item.isSecret ? '是' : '否',
        })),
      }
    },
  },
  {
    name: 'env_activate',
    desc: '切换项目当前生效的环境',
    args: '{ projectId, id }',
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const id = needNum(args.id, 'id')
      await envActivate(projectId, id)
      const env = (await envList(projectId)).find((item) => item.id === id)
      const project = (await projectList()).find((item) => item.id === projectId)
      return {
        项目: project?.name ?? '未知项目',
        当前环境: env?.name ?? '未知环境',
      }
    },
  },
  {
    name: 'plan_list',
    desc: '项目的压测计划列表（含关联请求名与本地时间）',
    args: '{ projectId }',
    handler: async (args) => {
      const plans = await planList(needNum(args.projectId, 'projectId'))
      return plans.map((item) => ({
        计划名称: item.name,
        关联请求: item.requestName || '未关联请求',
        更新时间: fmtTime(item.updatedAt),
      }))
    },
  },
  {
    name: 'plan_get',
    desc: '读取某个压测计划的完整配置（参数、阶段、SLA、关联请求）',
    args: '{ id }',
    handler: async (args) => {
      const plan = await planGet(needNum(args.id, 'id'))
      return {
        计划名称: plan.name,
        关联请求: plan.requestId ?? '未关联',
        更新时间: fmtTime(plan.updatedAt),
        配置: plan.config,
      }
    },
  },
  {
    name: 'plan_delete',
    desc: '删除压测计划（危险操作，需用户确认）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      await planDelete(id)
      return `压测计划 #${id} 已删除`
    },
  },
  {
    name: 'plan_save',
    desc: '保存压测计划到项目（固定并发或梯度阶段，可带 SLA），保存后可在「压测」页直接载入执行',
    args: '{ projectId, name, requestId?, mode?: fixed|stages, concurrent?, durationSec?, rampUpSec?, thinkTimeMs?, targetRps?, stages?: [{ concurrent, durationSec }], slaP95?, slaP99?, slaErrorRate?, slaMinRps?, envId? }',
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const name = str(args.name).trim()
      if (!name) throw new Error('参数 name 不能为空')
      const requestId = args.requestId === null || args.requestId === undefined ? null : needNum(args.requestId, 'requestId')
      const mode = args.mode === 'stages' ? 'stages' : 'fixed'
      const stages = toStages(args.stages)
      if (mode === 'stages' && !stages.length) throw new Error('梯度模式需要提供 stages 数组')

      const concurrent = Math.min(500, Math.max(1, Math.round(optionalNum(args.concurrent) ?? 10)))
      const durationSec = Math.min(3600, Math.max(1, Math.round(optionalNum(args.durationSec) ?? 30)))
      const sla = slaOf(args)
      const config = {
        settings: {
          mode,
          concurrent,
          durationSec,
          rampUpSec: Math.max(0, Math.round(optionalNum(args.rampUpSec) ?? 0)),
          thinkTimeMs: Math.max(0, Math.round(optionalNum(args.thinkTimeMs) ?? 0)),
          timeoutMs: 30000,
          connectTimeoutMs: 10000,
          followRedirects: true,
          stopOnErrorRate: false,
          stopErrorRate: 5,
          stopMinRequests: 100,
          insecure: false,
          targetRps: Math.max(0, Math.round(optionalNum(args.targetRps) ?? 0)),
          slaEnabled: sla !== null,
          slaP95: sla?.p95Ms ?? 500,
          slaP99: sla?.p99Ms ?? 1000,
          slaErrorRate: sla?.errorRate ?? 1,
          slaMinRps: sla?.minRps ?? 0,
        },
        stages: mode === 'stages' ? stages : [],
        envId: optionalNum(args.envId) ?? null,
      }

      const saved = await planSave({ projectId, requestId, name, config })
      const summary =
        mode === 'stages'
          ? `${stages.length} 个阶段：${stages.map((item) => `${item.concurrent} 并发 × ${item.durationSec}s`).join(' → ')}`
          : `${concurrent} 并发 × ${durationSec}s`
      const slaText = sla
        ? `，SLA：P95 ≤ ${sla.p95Ms ?? '-'}ms、P99 ≤ ${sla.p99Ms ?? '-'}ms、错误率 ≤ ${sla.errorRate ?? '-'}%`
        : ''
      return `压测计划「${saved.name}」已保存（${summary}${slaText}）。到「压测」页选择该项目和接口后，在计划下拉里载入即可执行`
    },
  },
  {
    name: 'stress_start',
    desc: '启动压测：按已保存请求（requestId）或直接给 URL；默认 10 并发 × 30 秒，支持梯度阶段与 SLA',
    args: '{ requestId?, url?, method?, name?, headers?, params?, bodyType?, body?, envId?, concurrent?, durationSec?, rampUpSec?, thinkTimeMs?, targetRps?, stages?, slaP95?, slaP99?, slaErrorRate?, slaMinRps? }',
    danger: true,
    handler: async (args) => {
      const requestId = optionalNum(args.requestId)
      type StressBase = Omit<StressConfig, 'concurrent' | 'durationSec' | 'rampUpSec' | 'thinkTimeMs'>
      let base: StressBase
      if (requestId) {
        const detail = await requestGet(requestId)
        base = {
          requestId,
          name: str(args.name, detail.name),
          method: str(args.method, detail.method).toUpperCase(),
          url: detail.url,
          headers: detail.headers,
          params: detail.params,
          bodyType: detail.bodyType,
          body: detail.body,
          envId: optionalNum(args.envId) ?? null,
        }
      } else {
        const url = str(args.url).trim()
        if (!url) throw new Error('参数 requestId 和 url 至少提供一个')
        base = {
          requestId: null,
          name: str(args.name, url),
          method: str(args.method, 'GET').toUpperCase(),
          url,
          headers: toItems(args.headers),
          params: toItems(args.params ?? args.query),
          bodyType: bodyTypeOf(args),
          body: bodyText(args.body),
          envId: optionalNum(args.envId) ?? null,
        }
      }

      const stages = toStages(args.stages)
      const concurrent = stages.length
        ? Math.max(...stages.map((item) => item.concurrent))
        : Math.min(500, Math.max(1, Math.round(optionalNum(args.concurrent) ?? 10)))
      const durationSec = stages.length
        ? stages.reduce((sum, item) => sum + item.durationSec, 0)
        : Math.min(3600, Math.max(1, Math.round(optionalNum(args.durationSec) ?? 30)))
      const rampUpSec = Math.max(0, Math.round(optionalNum(args.rampUpSec) ?? 0))
      const thinkTimeMs = Math.max(0, Math.round(optionalNum(args.thinkTimeMs) ?? 0))
      const sla = slaOf(args)

      await stressStart({
        ...base,
        insecure: false,
        timeoutMs: 30000,
        connectTimeoutMs: 10000,
        followRedirects: true,
        stopErrorRate: null,
        stopMinRequests: null,
        concurrent,
        durationSec,
        rampUpSec,
        thinkTimeMs,
        stages,
        targetRps: optionalNum(args.targetRps) ?? null,
        sla,
      })
      const shape = stages.length
        ? `${stages.length} 个阶段（峰值 ${concurrent} 并发，累计 ${durationSec}s）`
        : `${concurrent} 并发 × ${durationSec}s`
      return `已启动压测：${base.name}（${base.method} ${base.url}）${shape}${
        rampUpSec ? `，爬坡 ${rampUpSec}s` : ''
      }${sla ? '，已启用 SLA 校验' : ''}；结束后可用 report_list 取最新报告，再用 report_get 查看详情`
    },
  },
  {
    name: 'stress_stop',
    desc: '停止正在运行的压测（危险操作，需用户确认）',
    args: '无',
    danger: true,
    handler: async () => {
      await stressStop()
      return '已发送停止指令，稍后用 report_list 查看已生成的报告'
    },
  },
  {
    name: 'run_batch',
    desc: '按顺序批量执行已保存请求（最多 50 个 × 20 轮），返回每次的状态码与耗时',
    args: '{ requestIds: number[], rounds?, envId?, stopOnError?, timeoutMs? }',
    danger: true,
    handler: async (args) => {
      const rawIds = Array.isArray(args.requestIds) ? args.requestIds : []
      const requestIds = rawIds.map((item) => needNum(item, 'requestIds')).slice(0, 50)
      if (!requestIds.length) throw new Error('参数 requestIds 不能为空')
      const rounds = Math.min(20, Math.max(1, Math.round(optionalNum(args.rounds) ?? 1)))
      const envId = optionalNum(args.envId) ?? null
      const stopOnError = args.stopOnError === true
      const timeoutMs = Math.round(optionalNum(args.timeoutMs) ?? 30000)

      const details: HttpRequestDetail[] = []
      for (const id of requestIds) details.push(await requestGet(id))

      let total = 0
      let okCount = 0
      let failCount = 0
      const lines: string[] = []
      let aborted = false

      for (let round = 1; round <= rounds && !aborted; round += 1) {
        for (const detail of details) {
          total += 1
          const label = `${detail.name} ${detail.method} ${detail.url}`
          try {
            const result = await aiExecute(detailPayload(detail, { envId, timeoutMs }))
            const good = result.status >= 200 && result.status < 400
            if (good) okCount += 1
            else failCount += 1
            const extra = result.error ? ` ${result.error}` : ''
            lines.push(`第 ${round} 轮 · ${label} → ${result.status} (${result.durationMs}ms)${extra}`)
            if (!good && stopOnError) {
              aborted = true
              lines.push('已按 stopOnError 中止后续请求')
              break
            }
          } catch (error) {
            failCount += 1
            lines.push(`第 ${round} 轮 · ${label} → 失败：${String(error)}`)
            if (stopOnError) {
              aborted = true
              lines.push('已按 stopOnError 中止后续请求')
              break
            }
          }
        }
      }

      return [
        `批量执行完成：${requestIds.length} 个请求 × ${rounds} 轮 = ${total} 次，成功 ${okCount}，失败 ${failCount}`,
        ...lines,
      ].join('\n')
    },
  },
  {
    name: 'project_import',
    desc: '从 AstroMan 项目导出文件（.astroman.json）导入项目（可能覆盖同名项目，需用户确认）',
    args: '{ path }',
    danger: true,
    handler: async (args) => {
      const project = await projectImport(needPath(args.path))
      return { id: project.id, name: project.name, description: project.description }
    },
  },
  {
    name: 'project_export',
    desc: '把项目导出为 .astroman.json 文件（会写入用户给出的路径）',
    args: '{ projectId, path }',
    danger: true,
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const path = needPath(args.path)
      const project = (await projectList()).find((item) => item.id === projectId)
      await projectExport(projectId, path)
      return `已导出项目「${project?.name ?? '未知项目'}」到 ${path}`
    },
  },
  {
    name: 'api_import',
    desc: '导入 Postman Collection v2 / Apifox 文件（projectId 为空则新建项目）',
    args: '{ path, projectId? }',
    handler: async (args) => {
      const project = await apiImport(needPath(args.path), optionalNum(args.projectId) ?? null)
      return { id: project.id, name: project.name }
    },
  },
  {
    name: 'openapi_import',
    desc: '导入 OpenAPI 3 / Swagger 文件（projectId 为空则新建项目）',
    args: '{ path, projectId? }',
    handler: async (args) => {
      const project = await openapiImport(needPath(args.path), optionalNum(args.projectId) ?? null)
      return { id: project.id, name: project.name }
    },
  },
  {
    name: 'openapi_export',
    desc: '把项目导出为 OpenAPI 3 JSON（会写入用户给出的路径）',
    args: '{ projectId, path }',
    danger: true,
    handler: async (args) => {
      const projectId = needNum(args.projectId, 'projectId')
      const path = needPath(args.path)
      const project = (await projectList()).find((item) => item.id === projectId)
      await openapiExport(projectId, path)
      return `已导出项目「${project?.name ?? '未知项目'}」的 OpenAPI 文档到 ${path}`
    },
  },
  {
    name: 'report_export',
    desc: '把压测报告导出为 HTML（会写入用户给出的路径）',
    args: '{ id, path }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const path = needPath(args.path)
      const report = (await reportList(200)).find((item) => item.id === id)
      await reportExport(id, path)
      return `已导出报告「${report?.planName ?? '未知报告'}」到 ${path}`
    },
  },
  {
    name: 'workspace_stats',
    desc: '工作台总览：项目 / 接口 / 环境 / 记录 / 报告数量与压测成功率（已转中文标签）',
    args: '无',
    handler: async () => workspaceDigest(await workspaceStats()),
  },
  {
    name: 'dashboard_metrics',
    desc: '数据大屏指标：趋势、状态码、方法、耗时、热门接口、活跃项目（已转中文标签与可读单位）',
    args: '无',
    handler: async () => dashboardDigest(await dashboardMetrics()),
  },
  /* ---------------- 阶段四：业务工具补齐 ---------------- */
  {
    name: 'env_delete',
    desc: '删除某个环境及其变量（危险操作，需用户确认）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      await envDelete(id)
      return { deleted: id }
    },
  },
  {
    name: 'env_script_save',
    desc: '写入 / 清空某个环境的环境脚本（每次发请求前执行，可用 vars.set 生成运行时变量）',
    args: '{ id, script }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      if (args.script === undefined) throw new Error('参数 script 不能为空；清空脚本请传空字符串')
      const script = str(args.script)
      await envScriptSave(id, script)
      return { id, 脚本长度: script.length }
    },
  },
  {
    name: 'report_delete',
    desc: '删除某份压测报告（危险操作，需用户确认，无法撤销）',
    args: '{ id }',
    danger: true,
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      await reportDelete(id)
      return { deleted: id }
    },
  },
  {
    name: 'report_requests',
    desc: '分页读取压测报告里的单请求记录（状态码、耗时、是否成功、错误原因）；total 为真实总数',
    args: '{ id, page?, pageSize?, success? }',
    handler: async (args) => {
      const id = needNum(args.id, 'id')
      const page = Math.max(1, Math.round(optionalNum(args.page) ?? 1))
      const pageSize = Math.min(200, Math.max(1, Math.round(optionalNum(args.pageSize) ?? 50)))
      const success =
        typeof args.success === 'boolean' ? (args.success as boolean) : null
      const data: ReportRequestsPage = await reportRequests(id, page, pageSize, success)
      const rows = (data.items ?? []).map((item: ReportRequestRecord) => ({
        状态码: item.status,
        是否成功: item.success ? '成功' : '失败',
        耗时毫秒: item.latencyMs,
        首字节毫秒: item.ttfbMs,
        响应字节: item.bytes,
        请求方法: item.method,
        地址: item.url,
        错误类型: item.errorType ? cnLabel(ERROR_TYPE_LABELS, item.errorType) : '',
        错误信息: item.errorMessage ?? '',
        时间: fmtTime(item.timeMs),
      }))
      return {
        total: data.total,
        shown: rows.length,
        truncated: data.total > rows.length,
        页码: page,
        每页条数: pageSize,
        items: rows,
      }
    },
  },
  {
    name: 'history_stats',
    desc: '请求历史总览：历史条数、单接口运行记录数、最早 / 最近记录时间',
    args: '无',
    handler: async () => {
      const stats: HistoryStats = await historyStats()
      return {
        历史记录数: stats.total,
        单接口运行记录数: stats.runsTotal,
        最早记录: fmtTime(stats.oldestAt),
        最近记录: fmtTime(stats.newestAt),
      }
    },
  },
  {
    name: 'run_list',
    desc: '读取某个已保存请求最近 N 次运行记录（状态码、耗时、响应片段）；需先拿到 requestId',
    args: '{ requestId, limit? }',
    handler: async (args) => {
      const requestId = needNum(args.requestId, 'requestId')
      const limit = Math.min(500, Math.max(1, Math.round(optionalNum(args.limit) ?? 50)))
      const runs: RequestRun[] = await runList(requestId, limit)
      const rows = runs.map((run) => ({
        id: run.id,
        状态码: run.responseStatus,
        耗时毫秒: run.durationMs,
        响应大小: run.sizeBytes,
        错误: run.error ?? '',
        时间: fmtTime(run.createdAt),
        请求方法: run.method,
        地址: run.url,
        响应片段: clip(str(run.responseBody), 500),
      }))
      return {
        total: runs.length,
        shown: rows.length,
        truncated: false,
        说明: `最多读取最近 ${limit} 次运行记录，total 为本次返回条数`,
        items: rows,
      }
    },
  },
  {
    name: 'cookie_list',
    desc: '列出本机 Cookie 管理器中的 Cookie（值已打码，只用于确认域名与是否过期）',
    args: '无',
    handler: async () => {
      const items: CookieItem[] = await cookieList()
      const rows = items.map((item) => ({
        id: item.id,
        域名: item.domain,
        路径: item.path,
        名称: item.name,
        值: maskValue(item.value),
        Secure: item.secure ? '是' : '否',
        HttpOnly: item.httpOnly ? '是' : '否',
        过期时间: item.expiresAt ? fmtTime(item.expiresAt) : '会话级',
        更新时间: fmtTime(item.updatedAt),
      }))
      return summarizeList(rows)
    },
  },
  /* ---------------- 阶段四：工具箱（复用 src/tools/*） ---------------- */
  {
    name: 'toolbox_json_format',
    desc: '格式化 / 压缩 JSON，返回缩进后的文本；解析失败会给出错误原因',
    args: '{ text, indent? }',
    handler: async (args) => {
      const indent = Math.min(8, Math.max(0, Math.round(optionalNum(args.indent) ?? 2)))
      return stringifyJson(parseJson(str(args.text)), indent)
    },
  },
  {
    name: 'toolbox_json_to_entity',
    desc: '把 JSON 转成 Java / C# / Go / TypeScript 实体类代码',
    args: '{ text, target: java|csharp|go|ts, rootName?, packageName?, lombok?, comments? }',
    handler: async (args) => {
      const target = str(args.target, 'java') as EntityTarget
      return jsonToEntity(target, str(args.text), {
        rootName: str(args.rootName).trim() || undefined,
        packageName: str(args.packageName).trim() || undefined,
        lombok: args.lombok === undefined ? true : args.lombok === true,
        comments: args.comments === undefined ? true : args.comments === true,
      })
    },
  },
  {
    name: 'toolbox_json_to_sql',
    desc: '把 JSON（对象或数组）转成 INSERT SQL 语句，需指定表名',
    args: '{ text, table, mode?: single|batch }',
    handler: async (args) => {
      const mode = str(args.mode, 'single') === 'batch' ? 'batch' : 'single'
      return jsonToSql(str(args.text), str(args.table).trim(), mode)
    },
  },
  {
    name: 'toolbox_convert',
    desc: '结构化数据互转：JSON↔YAML / JSON↔XML / CSV↔JSON / JSON→Markdown / JSON↔Query String',
    args: '{ text, kind: json2yaml|yaml2json|json2xml|xml2json|csv2json|json2csv|json2markdown|json2query|query2json, delimiter?, rootName? }',
    handler: async (args) => {
      const text = str(args.text)
      const delimiter = str(args.delimiter, ',') || ','
      switch (str(args.kind)) {
        case 'json2yaml':
          return jsonToYaml(text)
        case 'yaml2json':
          return yamlToJson(text)
        case 'json2xml':
          return jsonToXml(text, str(args.rootName, 'root') || 'root')
        case 'xml2json':
          return xmlToJson(text)
        case 'csv2json':
          return csvToJson(text, delimiter)
        case 'json2csv':
          return jsonToCsv(text, delimiter)
        case 'json2markdown':
          return jsonToMarkdown(text)
        case 'json2query':
          return jsonToQuery(text)
        case 'query2json':
          return queryToJson(text)
        default:
          throw new Error('参数 kind 不在支持范围内')
      }
    },
  },
  {
    name: 'toolbox_timestamp',
    desc: '时间戳 / 日期互转：秒、毫秒、纳秒与本地 / UTC / ISO 时间',
    args: '{ input? }',
    handler: async (args) => timestampInfo(str(args.input)),
  },
  {
    name: 'toolbox_base64',
    desc: 'Base64 编码 / 解码（含 URL-safe 变体）',
    args: '{ text, action: encode|decode|urlEncode|urlDecode }',
    handler: async (args) => {
      const text = str(args.text)
      switch (str(args.action)) {
        case 'encode':
          return base64Encode(text)
        case 'decode':
          return base64Decode(text)
        case 'urlEncode':
          return base64UrlEncode(text)
        case 'urlDecode':
          return base64UrlDecode(text)
        default:
          throw new Error('参数 action 不在支持范围内')
      }
    },
  },
  {
    name: 'toolbox_hash',
    desc: '计算文本哈希（MD5 / SHA1 / SHA256 / SHA512）；传 hmacKey 时改为 HMAC 签名',
    args: '{ text, algorithm: MD5|SHA1|SHA256|SHA512, hmacKey? }',
    handler: async (args) => {
      const text = str(args.text)
      const algorithm = str(args.algorithm, 'SHA256') || 'SHA256'
      const key = str(args.hmacKey)
      const digest = key ? await toolHmac(text, key, algorithm) : await toolHash(text, algorithm)
      return { 算法: key ? `HMAC-${algorithm}` : algorithm, 摘要: digest }
    },
  },
  {
    name: 'toolbox_url',
    desc: '解析 URL：协议、主机、端口、路径、查询串与逐项查询参数',
    args: '{ text }',
    handler: async (args) => urlInfo(str(args.text)),
  },
  {
    name: 'toolbox_regex',
    desc: '正则匹配：返回命中数量与每组匹配内容；最多返回前 50 条',
    args: '{ pattern, text, flags? }',
    handler: async (args) => {
      const result = runRegex(str(args.pattern), str(args.flags, 'g') || 'g', str(args.text))
      const rows = result.matches.slice(0, 50).map((item) => ({
        位置: item.index,
        内容: item.text,
        分组: item.groups,
        命名分组: item.named,
      }))
      return {
        total: result.matches.length,
        shown: rows.length,
        truncated: result.matches.length > rows.length,
        说明: result.text,
        items: rows,
      }
    },
  },
  {
    name: 'toolbox_jwt',
    desc: '解析 JWT 的 Header / Payload（不校验签名，签名段只显示前 12 位）',
    args: '{ token }',
    handler: async (args) => {
      const parts = decodeJwt(str(args.token))
      return {
        Header: parts.header,
        Payload: parts.payload,
        签名: parts.signature ? `${parts.signature.slice(0, 12)}…` : '（无）',
      }
    },
  },
  {
    name: 'toolbox_diff',
    desc: '对比两段文本的行级差异，返回新增 / 删除行数；format=json 时附带明细',
    args: '{ left, right, format?: text|json }',
    handler: async (args) => {
      const result = diffLines(str(args.left), str(args.right))
      if (str(args.format, 'text') !== 'json') return diffAsText(result)
      const changed = result.lines.filter((line) => line.kind !== 'same')
      const lines = changed.slice(0, 200).map((line) => ({
        类型: line.kind === 'add' ? '新增' : '删除',
        行号: line.kind === 'add' ? line.rightNo : line.leftNo,
        内容: line.text,
      }))
      return {
        新增行数: result.added,
        删除行数: result.removed,
        相同行数: result.same,
        truncated: changed.length > lines.length,
        items: lines,
      }
    },
  },
  {
    name: 'toolbox_uuid',
    desc: '生成 UUID v4（默认 1 个，最多 20 个）',
    args: '{ count? }',
    handler: async (args) => {
      const count = Math.min(20, Math.max(1, Math.round(optionalNum(args.count) ?? 1)))
      return { total: count, items: Array.from({ length: count }, () => uuidV4()) }
    },
  },
  {
    name: 'tool_search',
    desc: '按关键词检索工具清单（长尾工具先用它查用法），返回工具名、用途与参数',
    args: '{ keyword }',
    handler: async (args) => {
      const keyword = str(args.keyword).trim().toLowerCase()
      if (!keyword) throw new Error('参数 keyword 不能为空')
      const aliases = TOOL_KEYWORDS.filter((item) => item.words.some((word) => keyword.includes(word)))
      const terms = [keyword, ...aliases.flatMap((item) => item.tools)].map((item) => item.toLowerCase())
      const hits = SPECS.filter((spec) => {
        if (spec.name === 'tool_search') return false
        const haystack = `${spec.name} ${spec.desc} ${spec.args}`.toLowerCase()
        return terms.some((term) => term && haystack.includes(term))
      }).slice(0, 12)
      if (!hits.length) {
        return {
          total: 0,
          hint: '没有匹配的工具，换更短的关键词，例如：环境 / 报告 / 历史 / 转换 / 编码 / 压测 / 导出',
        }
      }
      return {
        total: hits.length,
        items: hits.map((spec) => ({ tool: spec.name, desc: spec.desc, args: spec.args })),
      }
    },
  },
]

/** 中文关键词 → 工具名片段，供 tool_search 检索长尾工具 */
const TOOL_KEYWORDS: Array<{ words: string[]; tools: string[] }> = [
  { words: ['环境', '变量', 'env'], tools: ['env_', 'environment'] },
  { words: ['报告', '压测结果', 'report'], tools: ['report_'] },
  { words: ['历史', '记录', 'history'], tools: ['history_', 'run_'] },
  { words: ['转换', '格式化', 'json', 'yaml', 'xml', 'csv'], tools: ['toolbox_'] },
  { words: ['编码', '解码', 'base64', '哈希', '加密', '签名', 'hash'], tools: ['toolbox_base64', 'toolbox_hash'] },
  { words: ['时间', '时间戳', 'timestamp'], tools: ['toolbox_timestamp'] },
  { words: ['正则', 'regex'], tools: ['toolbox_regex'] },
  { words: ['差异', 'diff', '对比'], tools: ['toolbox_diff'] },
  { words: ['实体', 'entity', 'java', 'go', 'typescript'], tools: ['toolbox_json_to_entity'] },
  { words: ['sql', '建表'], tools: ['toolbox_json_to_sql'] },
  { words: ['jwt', 'token 解析'], tools: ['toolbox_jwt'] },
  { words: ['uuid'], tools: ['toolbox_uuid'] },
  { words: ['url', '链接解析'], tools: ['toolbox_url'] },
  { words: ['cookie'], tools: ['cookie_list'] },
  { words: ['脚本', 'script'], tools: ['request_script_save', 'env_script_save'] },
  { words: ['导入', 'import'], tools: ['_import'] },
  { words: ['导出', 'export'], tools: ['_export'] },
  { words: ['计划', '压测'], tools: ['plan_', 'stress_'] },
]

const TOOL_MAP = new Map(SPECS.map((spec) => [spec.name, spec]))

export const toolNames = () => SPECS.map((spec) => spec.name)

/** 工具 id → 中文名，用于界面展示 */
export const TOOL_LABELS: Record<string, string> = {
  project_list: '读取项目列表',
  project_request_counts: '统计各项目请求数',
  project_create: '新建项目',
  project_update: '修改项目',
  project_delete: '删除项目',
  collection_tree: '读取集合树',
  collection_create: '新建集合',
  collection_rename: '重命名集合',
  collection_delete: '删除集合',
  request_list: '读取请求列表',
  request_get: '读取请求详情',
  request_search: '搜索请求',
  request_options: '待选请求清单',
  request_create: '创建接口请求',
  request_update: '修改接口请求',
  request_script_save: '写入前后置脚本',
  request_delete: '删除接口请求',
  curl_import: '导入 cURL',
  send_request: '发送 HTTP 请求',
  run_batch: '批量执行请求',
  env_list: '读取环境变量',
  env_create: '新建环境',
  env_var_save: '写入环境变量',
  env_activate: '切换环境',
  history_list: '读取请求历史',
  history_export: '导出执行记录',
  report_list: '读取压测报告',
  report_get: '读取报告详情',
  report_export: '导出压测报告',
  project_import: '导入项目',
  project_export: '导出项目',
  api_import: '导入 Postman / Apifox',
  openapi_import: '导入 OpenAPI',
  openapi_export: '导出 OpenAPI',
  plan_list: '读取压测计划',
  plan_get: '读取压测计划详情',
  plan_delete: '删除压测计划',
  plan_save: '保存压测计划',
  stress_start: '启动压测',
  stress_stop: '停止压测',
  workspace_stats: '工作台统计',
  dashboard_metrics: '数据大屏指标',
  env_delete: '删除环境',
  env_script_save: '写入环境脚本',
  report_delete: '删除压测报告',
  report_requests: '读取报告请求记录',
  history_stats: '请求历史统计',
  run_list: '读取运行记录',
  cookie_list: '读取 Cookie 列表',
  toolbox_json_format: '格式化 JSON',
  toolbox_json_to_entity: 'JSON 转实体类',
  toolbox_json_to_sql: 'JSON 转 SQL',
  toolbox_convert: '结构化数据转换',
  toolbox_timestamp: '时间戳转换',
  toolbox_base64: 'Base64 编解码',
  toolbox_hash: '哈希 / HMAC',
  toolbox_url: '解析 URL',
  toolbox_regex: '正则匹配',
  toolbox_jwt: '解析 JWT',
  toolbox_diff: '文本差异对比',
  toolbox_uuid: '生成 UUID',
  tool_search: '检索工具清单',
}

/** 该工具是否会改动数据，需要用户点击确认 */
export function isDangerousTool(tool: string): boolean {
  return TOOL_MAP.get(tool)?.danger === true
}

/**
 * 会写入 / 触发执行的工具：新建、保存、启动这类操作即使没标 danger，
 * 也必须先把「将要做什么」摆给用户确认，不能由模型直接落库或开跑。
 */
const WRITE_TOOLS = new Set([
  'project_create',
  'project_update',
  'project_delete',
  'collection_create',
  'collection_rename',
  'collection_delete',
  'request_create',
  'request_update',
  'request_delete',
  'request_script_save',
  'curl_import',
  'env_create',
  'env_var_save',
  'env_activate',
  'env_delete',
  'env_script_save',
  'plan_save',
  'plan_delete',
  'stress_start',
  'stress_stop',
  'run_batch',
  'history_export',
  'report_export',
  'report_delete',
  'project_import',
  'project_export',
  'api_import',
  'openapi_import',
  'openapi_export',
])

/** 需要用户先确认再执行的工具，判断统一走这里 */
export function isWriteTool(tool: string): boolean {
  return isDangerousTool(tool) || WRITE_TOOLS.has(tool)
}

export function toolLabel(tool: string): string {
  return TOOL_LABELS[tool] ?? tool
}

/**
 * 常用工具直接注入提示词；其余长尾工具只列名字，
 * 由 tool_search 按需检索，避免工具越多提示词越臃肿（T-12）。
 */
const CORE_TOOL_NAMES = new Set([
  'project_list',
  'project_request_counts',
  'collection_tree',
  'request_list',
  'request_get',
  'request_search',
  'request_options',
  'request_create',
  'request_update',
  'request_script_save',
  'request_delete',
  'curl_import',
  'send_request',
  'run_batch',
  'env_list',
  'history_list',
  'history_export',
  'report_list',
  'report_get',
  'plan_save',
  'stress_start',
  'stress_stop',
  'workspace_stats',
  'tool_search',
])

const catalogLine = (spec: ToolSpec) => `- ${spec.name}：${spec.desc}；参数 ${spec.args}`

/** 供系统提示词使用的工具清单：常用工具给全量说明，长尾只给名字 */
export const TOOL_CATALOG = [
  SPECS.filter((spec) => CORE_TOOL_NAMES.has(spec.name)).map(catalogLine).join('\n'),
  '',
  `其余工具（未列说明，先调用 tool_search({ keyword }) 检索用法，再按返回的参数调用）：${SPECS.filter(
    (spec) => !CORE_TOOL_NAMES.has(spec.name),
  )
    .map((spec) => spec.name)
    .join('、')}`,
].join('\n')

/** 目录分层效果：core 为实际注入提示词的字符数，full 为全量注入时的字符数 */
export function toolCatalogStats(): { core: number; full: number; ratio: number } {
  const full = SPECS.map(catalogLine).join('\n').length
  const core = TOOL_CATALOG.length
  return { core, full, ratio: full ? core / full : 1 }
}

/* ---------------- 工具硬超时 ---------------- */

/** 普通工具等待上限 */
const DEFAULT_TOOL_TIMEOUT = 60_000
/** 导出入 / 导出类要读写整份文件，给更宽的上限 */
const FILE_TOOL_TIMEOUT = 120_000
const FILE_TOOLS = new Set([
  'history_export',
  'report_export',
  'project_export',
  'openapi_export',
  'project_import',
  'api_import',
  'openapi_import',
])
/** 压测启停本身立即返回任务句柄，包超时反而会误判 */
const NO_TIMEOUT_TOOLS = new Set(['stress_start', 'stress_stop'])

class ToolTimeoutError extends Error {}

/**
 * 每个工具的等待上限：
 * - send_request / run_batch 沿用入参 timeoutMs（逐次 × 轮数再加余量）
 * - 导入导出 120 秒，其余 60 秒；压测启停不包
 */
export function toolTimeoutOf(tool: string, args: ToolArgs): number | null {
  if (NO_TIMEOUT_TOOLS.has(tool)) return null
  const argTimeout = Number(args.timeoutMs)
  if (tool === 'send_request') {
    const per = Number.isFinite(argTimeout) && argTimeout > 0 ? Math.min(argTimeout, 120_000) : 30_000
    return per + 5_000
  }
  if (tool === 'run_batch') {
    const rounds = Math.max(1, Number(args.rounds) || (Array.isArray(args.requestIds) ? args.requestIds.length : 1))
    const per = Number.isFinite(argTimeout) && argTimeout > 0 ? Math.min(argTimeout, 120_000) : 30_000
    return Math.min(rounds * per + 10_000, 600_000)
  }
  return FILE_TOOLS.has(tool) ? FILE_TOOL_TIMEOUT : DEFAULT_TOOL_TIMEOUT
}

/** 只限制前端等待时间：Tauri invoke 没有取消原语，超时后迟到的结果直接丢弃 */
function withTimeout<T>(task: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      reject(new ToolTimeoutError(`工具执行超时（${Math.round(ms / 1000)} 秒），任务未完成`))
    }, ms)
    task.then(
      (value) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        resolve(value)
      },
      (error) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        reject(error)
      },
    )
  })
}

/**
 * 执行一个工具：统一捕获错误、截断输出，并把本次调用写入独立流水表。
 * 无论成功失败都会记录，便于用户回看每次工具请求。
 */
export async function executeTool(
  tool: string,
  args: ToolArgs,
  sessionId: number | null,
  meta?: { round?: number; traceId?: string | null },
): Promise<ToolResult> {
  const spec = TOOL_MAP.get(tool)
  const started = typeof performance !== 'undefined' ? performance.now() : Date.now()
  let ok = true
  let output = ''
  let raw: unknown
  let timedOut = false

  if (!spec) {
    ok = false
    output = `未知工具「${tool}」。可用工具：${toolNames().join('、')}`
  } else {
    // 硬规则 3：入参先过 JSON Schema，非法直接拒绝底层调用并把原因回灌给模型
    const invalid = validateToolArgs(tool, args)
    if (invalid) {
      ok = false
      output = `参数校验失败：${invalid}`
    } else {
      try {
        const limit = toolTimeoutOf(tool, args)
        let task = spec.handler(args)
        if (limit) task = withTimeout(task, limit)
        raw = await task
        output = spec.format ? spec.format(raw) : clip(pretty(raw), 8000)
      } catch (error) {
        ok = false
        if (error instanceof ToolTimeoutError) {
          timedOut = true
          output = error.message
        } else {
          output = `工具执行失败：${error instanceof Error ? error.message : String(error)}`
        }
      }
    }
    // 硬规则 6：结果进入模型上下文前做脱敏
    output = maskSensitive(output)
  }

  const durationMs = Math.round(
    (typeof performance !== 'undefined' ? performance.now() : Date.now()) - started,
  )
  // 统一信封：界面卡片先展示 summary，total / truncated 供计数与提示
  const envelope = ok ? envelopeOf(tool, raw) : { summary: output.split('\n')[0] || '执行失败' }
  try {
    await aiToolLogAdd({
      sessionId,
      tool,
      args: clip(pretty(args ?? {}), 2000),
      ok,
      durationMs,
      resultPreview: clip(output, 2000),
      round: meta?.round ?? 0,
      traceId: meta?.traceId ?? null,
      timeout: timedOut,
    })
  } catch {
    // 流水写入失败不影响主流程
  }

  return { ok, output, raw, durationMs, ...envelope }
}
