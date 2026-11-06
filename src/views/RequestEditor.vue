<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save as saveFile } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined,
  ThunderboltOutlined,
  SendOutlined,
  SaveOutlined,
  DeleteOutlined,
  CloseOutlined,
  CopyOutlined,
  CheckOutlined,
  EditOutlined,
  CodeOutlined,
  PlusOutlined,
  DownOutlined,
  SettingOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import KeyValueEditor from '@/components/KeyValueEditor.vue'
import CodeArea from '@/components/CodeArea.vue'
import EnvironmentDrawer from '@/components/EnvironmentDrawer.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import {
  requestAssertionPrompt,
  requestExplainPrompt,
  requestOptimizePrompt,
  requestPostScriptPrompt,
} from '@/utils/aiScenarios'
import { useProjectStore } from '@/stores/project'
import { backOr, previousEntry } from '@/stores/navHistory'
import { MAX_REQUEST_TABS, useTabsStore } from '@/stores/tabs'
import { useErrorDialogStore } from '@/stores/errorDialog'
import { useWorkspaceStore, type RequestTurnState, type RequestWorkspaceSnapshot } from '@/stores/workspace'
import { setHandoff } from '@/stores/handoff'
import { formatDateTime } from '@/utils/datetime'
import { buildSnippets, type Snippet } from '@/utils/curl'
import { formatJsonAsync } from '@/utils/formatJson'
import { diffLines, diffStats, type DiffLine } from '@/utils/diff'
import { readLastRequest, rememberLastRequest } from '@/utils/lastRequest'
import * as api from '@/api'
import type {
  CookieItem,
  Environment,
  FormField,
  HttpRequestDetail,
  HttpResult,
  KeyValueItem,
  RequestAuth,
  RequestRun,
  RequestRunSummary,
  RequestSearchHit,
  SendRequestPayload,
} from '@/types/models'
import {
  clearRuntimeVars,
  listRuntimeVars,
  renderRuntimeVars,
  setRuntimeVars,
  type ScriptAssertion,
  type ScriptLog,
  type ScriptRequest,
} from '@/utils/scriptRunner'
import { runScriptJob } from '@/utils/scriptHost'
import { POST_SCRIPT_EXAMPLES, PRE_SCRIPT_EXAMPLES } from '@/utils/scriptExamples'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const tabStore = useTabsStore()
const errorDialog = useErrorDialogStore()
const workspace = useWorkspaceStore()

const requestId = computed(() => Number(route.params.id))
const projectId = computed(() => Number(route.query.projectId || 0))

function emptyAuth(): RequestAuth {
  return {
    kind: 'none',
    token: '',
    username: '',
    password: '',
    key: '',
    value: '',
    addTo: 'header',
    tokenUrl: '',
    clientId: '',
    clientSecret: '',
    scope: '',
  }
}

const form = ref<HttpRequestDetail>({
  collectionId: 0,
  name: '',
  method: 'GET',
  url: '',
  headers: [],
  params: [],
  bodyType: 'none',
  body: '',
  bodyMeta: '',
  preScript: '',
  postScript: '',
  assertions: '',
  auth: emptyAuth(),
})

const loading = ref(true)
const saving = ref(false)
/** 本组件里仍在后台执行的请求（按接口 id）：切走再切回仍显示“发送中” */
const sendingRequestIds = reactive(new Set<number>())
const result = ref<HttpResult | null>(null)
/** live = 本次实时响应；run = 从运行记录有损恢复 */
const resultSource = ref<'live' | 'run' | 'none'>('none')
const restoredRunId = ref<number | null>(null)
const sending = computed(() => {
  const id = form.value.id
  if (!id) return false
  return sendingRequestIds.has(id) || workspace.isInflight(id)
})
const bodyPreEl = ref<HTMLElement | null>(null)
const tabsScrollEl = ref<HTMLElement | null>(null)
const activeTab = ref('params')
const responseTab = ref('body')
const insecure = ref(false)
const envId = ref<number | null>(null)
const envDrawerOpen = ref(false)

/* ---------- 发送选项：协议 / 重定向 / 重试（v1.0.8） ----------
   这几个是「发一次怎么发」的开关，不属于接口定义本身，所以存本地偏好而不是写库，
   避免为它们做一次表结构迁移。 */
const SEND_OPT_KEY = 'astroman.send.opts'

interface SendOpts {
  protocol: 'auto' | 'http1' | 'http2'
  followRedirects: boolean
  retryEnabled: boolean
  retryMaxAttempts: number
  retryAllowNonIdempotent: boolean
}

function loadSendOpts(): SendOpts {
  const fallback: SendOpts = {
    protocol: 'auto',
    followRedirects: true,
    retryEnabled: false,
    retryMaxAttempts: 3,
    retryAllowNonIdempotent: false,
  }
  try {
    const raw = localStorage.getItem(SEND_OPT_KEY)
    if (!raw) return fallback
    const parsed = JSON.parse(raw) as Partial<SendOpts>
    return {
      protocol: parsed.protocol === 'http1' || parsed.protocol === 'http2' ? parsed.protocol : 'auto',
      followRedirects: parsed.followRedirects !== false,
      retryEnabled: parsed.retryEnabled === true,
      retryMaxAttempts: Math.min(4, Math.max(1, Number(parsed.retryMaxAttempts) || 3)),
      retryAllowNonIdempotent: parsed.retryAllowNonIdempotent === true,
    }
  } catch {
    return fallback
  }
}

const initialOpts = loadSendOpts()
const protocol = ref<'auto' | 'http1' | 'http2'>(initialOpts.protocol)
const followRedirects = ref(initialOpts.followRedirects)
const retryEnabled = ref(initialOpts.retryEnabled)
const retryMaxAttempts = ref(initialOpts.retryMaxAttempts)
const retryAllowNonIdempotent = ref(initialOpts.retryAllowNonIdempotent)

watch([protocol, followRedirects, retryEnabled, retryMaxAttempts, retryAllowNonIdempotent], () => {
  try {
    localStorage.setItem(
      SEND_OPT_KEY,
      JSON.stringify({
        protocol: protocol.value,
        followRedirects: followRedirects.value,
        retryEnabled: retryEnabled.value,
        retryMaxAttempts: retryMaxAttempts.value,
        retryAllowNonIdempotent: retryAllowNonIdempotent.value,
      }),
    )
  } catch {
    /* 隐私模式下 localStorage 可能不可写，忽略 */
  }
})

/** 在途请求标识：后端按它定位要取消的那一条 */
const requestKey = ref('')
/** 本次是否被用户主动停止（用于把「取消」和「报错」区分开） */
const wasCancelled = ref(false)

const IDEMPOTENT_METHODS = new Set(['GET', 'HEAD', 'OPTIONS', 'PUT', 'DELETE', 'TRACE'])
const retryAvailable = computed(() => IDEMPOTENT_METHODS.has(form.value.method.toUpperCase()))
/** 非幂等方法想重试必须显式勾选，否则后端会拒绝重试 */
const retryNeedsConfirm = computed(() => retryEnabled.value && !retryAvailable.value)

/* ---------- 环境变量抽屉 ---------- */
/** 抽屉内新增 / 修改 / 删除环境后，同步项目环境列表，请求里用到的变量立即生效 */
function onEnvsChanged(list: Environment[]) {
  store.environments = list
  if (envId.value && !list.some((item) => item.id === envId.value)) {
    envId.value = list.find((item) => item.isActive)?.id ?? list[0]?.id ?? null
  }
}

/* ---------- 页面内 AI 分析 ---------- */
const aiAssist = useAiAssistStore()

/** AI 菜单分流：页面内分析抽屉 vs. 带着现场去航仔 */
function onAiMenu(kind: string) {
  if (kind === 'handoff') {
    goHangzaiWithContext()
    return
  }
  runAi(kind)
}

function runAi(kind: string) {
  const detail = form.value
  if (!detail.url?.trim()) {
    message.warning('先填写接口 URL 再做分析')
    return
  }
  const options: Record<string, { title: string; prompt: string }> = {
    explain: { title: '接口响应分析', prompt: requestExplainPrompt(detail, result.value) },
    assert: { title: '生成断言脚本', prompt: requestAssertionPrompt(detail, result.value) },
    post: { title: '生成后置脚本', prompt: requestPostScriptPrompt(detail, result.value) },
    optimize: { title: '接口定义优化', prompt: requestOptimizePrompt(detail) },
  }
  const picked = options[kind]
  if (!picked) return
  void aiAssist.show({
    ...picked,
    source: 'request',
    hint: `${detail.method} ${detail.url}${result.value ? ` · 最近响应 ${result.value.status}` : ' · 未发送过请求'}`,
  })
}

/**
 * 5.5 页面现场交接：把当前接口 / 响应一次性交给航仔。
 * 只做交接，不自动发送；航仔侧展示上下文条，由用户决定问什么。
 */
function goHangzaiWithContext() {
  const detail = form.value
  if (!detail.url?.trim()) {
    message.warning('先填写接口 URL 再去航仔')
    return
  }
  const note =
    result.value && resultSource.value === 'run'
      ? '\n\n【页面现场说明】上面的响应来自最近一次运行记录，不是本次刚发出的请求。'
      : ''
  setHandoff({
    source: 'request',
    title: `${detail.method.toUpperCase()} ${detail.url}${result.value ? ` · ${result.value.status}` : ' · 未发送'}`,
    prompt: requestExplainPrompt(detail, result.value) + note,
    projectId: projectId.value || undefined,
    requestId: detail.id ?? undefined,
  })
  router.push('/ai')
}

/* ---------- 自动保存 ---------- */
const autoState = ref<'saved' | 'dirty' | 'saving' | 'error'>('saved')
let hydrating = true
/** 表单变更版本号：自动保存期间继续编辑时，用版本号判断是否还需再存一次 */
let formVersion = 0
let savedVersion = 0
let saveTimer: number | undefined
let saveQueue: Promise<boolean> = Promise.resolve(true)

function syncTabMeta() {
  if (!form.value.id) return
  const name = form.value.name.trim() || '未命名请求'
  tabStore.touch(form.value.id, name, form.value.method)
  // 「打开其他接口」下拉是进页面时拉的一次快照，改名/改方法后同步一份，免得显示旧名字
  const hit = quickOptions.value.find((item) => item.id === form.value.id)
  if (hit) {
    hit.name = name
    hit.method = form.value.method
  }
}

/** Body 越大，保存往返（含 SQLite 写入）越慢，延长防抖避免输入期间反复排队 */
function autoSaveDelay() {
  const length = (form.value.body ?? '').length
  if (length > 2 * 1024 * 1024) return 3000
  if (length > 256 * 1024) return 2000
  if (length > 64 * 1024) return 1500
  return 900
}

function scheduleAutoSave() {
  if (saveTimer) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined
    void save(false)
  }, autoSaveDelay())
}

watch(
  form,
  () => {
    if (hydrating || !form.value.id) return
    formVersion += 1
    autoState.value = 'dirty'
    syncTabMeta()
    scheduleAutoSave()
  },
  { deep: true },
)

/* ---------- 脚本执行结果 ---------- */
const scriptLogs = ref<ScriptLog[]>([])
const scriptAssertions = ref<ScriptAssertion[]>([])
const scriptError = ref('')
const scriptVars = ref<Record<string, string>>({})
const scriptMs = ref(0)

/* ---------- 脚本示例 ---------- */
const preExample = ref<number>()
const postExample = ref<number>()
const preExampleOptions = PRE_SCRIPT_EXAMPLES.map((item, index) => ({
  label: item.label,
  value: index,
}))
const postExampleOptions = POST_SCRIPT_EXAMPLES.map((item, index) => ({
  label: item.label,
  value: index,
}))

function insertExample(phase: 'pre' | 'post', index?: number) {
  if (typeof index !== 'number') return
  const example = (phase === 'pre' ? PRE_SCRIPT_EXAMPLES : POST_SCRIPT_EXAMPLES)[index]
  if (!example) return
  if (phase === 'pre') {
    const current = form.value.preScript?.trim()
    form.value.preScript = current ? `${current}\n\n${example.code}` : example.code
    preExample.value = undefined
  } else {
    const current = form.value.postScript?.trim()
    form.value.postScript = current ? `${current}\n\n${example.code}` : example.code
    postExample.value = undefined
  }
}

const passedAssertions = computed(() => scriptAssertions.value.filter((item) => item.passed).length)
const failedAssertions = computed(() => scriptAssertions.value.length - passedAssertions.value)
const scriptVarsList = computed(() =>
  Object.entries(scriptVars.value).map(([key, value]) => ({ key, value })),
)

function resetScriptOutput() {
  scriptLogs.value = []
  scriptAssertions.value = []
  scriptError.value = ''
  scriptMs.value = 0
}

/**
 * 切换接口时清空上一条接口的响应与脚本产物。
 * 路由复用同一个 RequestEditor 实例，不清空就会把 A 的响应带到 B 的标签页上。
 */
function resetResponseState() {
  result.value = null
  resultSource.value = 'none'
  restoredRunId.value = null
  wasCancelled.value = false
  // 旧接口的在途请求不再属于当前面板，按钮状态按当前接口的 inflight 重新计算
  cancelling.value = false
  requestKey.value = ''
  resetScriptOutput()
  // 运行时变量是跨接口共享的（例如登录接口写入 token），切标签时保留
  responseTab.value = 'body'
  bodyView.value = 'pretty'
  bodyAbsPath.value = ''
  bodyPreviewError.value = ''
  diffOpen.value = false
  diffBaseRunId.value = null
  diffCustomText.value = ''
  diffUseCustom.value = false
  expandedRunId.value = null
  runDetails.clear()
  runDetailLoading.clear()
}

function resetScriptVars() {
  clearRuntimeVars()
  scriptVars.value = {}
  message.success('已清空运行时变量')
}

/* ---------- 页面现场：内存快照 ---------- */
const RUN_BODY_CAP = 256 * 1024

function captureSnapshot(): Partial<RequestWorkspaceSnapshot> {
  const source = result.value
    ? resultSource.value === 'run'
      ? 'run'
      : 'live'
    : 'none'
  return {
    result: result.value,
    turn: {
      logs: scriptLogs.value.slice(),
      assertions: scriptAssertions.value.slice(),
      error: scriptError.value,
      ms: scriptMs.value,
    },
    responseTab: responseTab.value,
    bodyView: bodyView.value,
    activeTab: activeTab.value,
    bodyAbsPath: bodyAbsPath.value,
    bodyPreviewError: bodyPreviewError.value,
    expandedRunId: expandedRunId.value,
    diffOpen: diffOpen.value,
    diffBaseRunId: diffBaseRunId.value,
    diffCustomText: diffCustomText.value,
    diffUseCustom: diffUseCustom.value,
    resultSource: source,
    restoredRunId: source === 'run' ? restoredRunId.value ?? undefined : undefined,
  }
}

function restoreSnapshot(snapshot: RequestWorkspaceSnapshot) {
  result.value = snapshot.result
  resultSource.value = snapshot.resultSource
  restoredRunId.value = snapshot.restoredRunId ?? null
  scriptLogs.value = snapshot.turn.logs.slice()
  scriptAssertions.value = snapshot.turn.assertions.slice()
  scriptError.value = snapshot.turn.error
  scriptMs.value = snapshot.turn.ms
  responseTab.value = snapshot.responseTab
  bodyView.value = snapshot.bodyView
  activeTab.value = snapshot.activeTab
  bodyAbsPath.value = snapshot.bodyAbsPath
  bodyPreviewError.value = snapshot.bodyPreviewError
  expandedRunId.value = snapshot.expandedRunId
  diffOpen.value = snapshot.diffOpen
  diffBaseRunId.value = snapshot.diffBaseRunId
  diffCustomText.value = snapshot.diffCustomText
  diffUseCustom.value = snapshot.diffUseCustom
  wasCancelled.value = snapshot.result?.cancelled === true
}

function saveSnapshot(requestId = form.value.id) {
  if (!requestId) return
  workspace.save(requestId, captureSnapshot())
}

function headerOf(headers: Record<string, string> | undefined, name: string): string {
  if (!headers) return ''
  const key = Object.keys(headers).find((item) => item.toLowerCase() === name)
  return key ? headers[key] : ''
}

/** 从运行记录做有损恢复：只还原状态、头信息和最多 256KB 正文 */
async function restoreRunAsResult(summary: RequestRunSummary) {
  const run = await ensureRunDetail(summary.id)
  const headers = run?.responseHeaders ?? {}
  const body = run?.responseBody ?? summary.responseBodyPreview ?? ''
  const contentType = headerOf(headers, 'content-type')
  const binary = /^(image|audio|video|application\/octet-stream|application\/pdf)/i.test(
    contentType,
  )
  result.value = {
    status: run?.responseStatus ?? summary.responseStatus,
    statusText: run?.responseStatusText ?? summary.responseStatusText,
    durationMs: run?.durationMs ?? summary.durationMs,
    sizeBytes: run?.sizeBytes ?? summary.sizeBytes,
    headers,
    body: binary ? '' : body,
    truncated: summary.responseBodyTruncated || body.length >= RUN_BODY_CAP,
    error: run?.error ?? summary.error ?? null,
    bodyKind: binary ? 'binary' : 'text',
    bodyFile: null,
    bodyBytes: run?.sizeBytes ?? summary.sizeBytes,
    rawBytes: run?.sizeBytes ?? summary.sizeBytes,
    previewBase64: null,
    timings: null,
    httpVersion: '',
    redirects: [],
    retries: [],
    ignoredHeaders: [],
    cancelled: false,
  }
  resultSource.value = 'run'
  restoredRunId.value = summary.id
  wasCancelled.value = false
}

/** 只有“本次会话最后打开的接口”才做启动后有损恢复，避免打开旧接口时冒出陈年响应 */
function shouldRestoreFromRun(id: number): boolean {
  const last = readLastRequest(projectId.value)
  if (last?.id !== id) return false
  const newest = runs.value[0]
  if (!newest) return false
  return Date.now() - newest.createdAt < 7 * 24 * 3600_000
}

function activeEnvVars(): Record<string, string> {
  const env = store.environments.find((item) => item.id === envId.value)
  const map: Record<string, string> = {}
  for (const item of env?.variables ?? []) {
    if (item.key) map[item.key] = item.value
  }
  return map
}

function kvToMap(items: KeyValueItem[]): Record<string, string> {
  const map: Record<string, string> = {}
  for (const item of items) {
    const key = item.key.trim()
    if (!item.enabled || !key) continue
    map[key] = item.value
  }
  return map
}

function mapToItems(map: Record<string, string>): KeyValueItem[] {
  return Object.entries(map).map(([key, value]) => ({ key, value, enabled: true }))
}

const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const bodyTypes = [
  { label: 'JSON', value: 'json' },
  { label: 'Raw', value: 'raw' },
  { label: 'Form', value: 'form' },
  { label: 'Form-Data', value: 'multipart' },
  { label: 'Binary', value: 'binary' },
  { label: 'GraphQL', value: 'graphql' },
  { label: '无', value: 'none' },
]

const enabledParams = computed(() => form.value.params.filter((p) => p.enabled && p.key).length)
const enabledHeaders = computed(() => form.value.headers.filter((h) => h.enabled && h.key).length)

/* ---------- 认证 ---------- */
const authKinds = [
  { label: '无认证', value: 'none' },
  { label: 'Bearer Token', value: 'bearer' },
  { label: 'Basic Auth', value: 'basic' },
  { label: 'API Key', value: 'apikey' },
  { label: 'OAuth2', value: 'oauth2' },
]

const authKind = computed({
  get: () => form.value.auth?.kind ?? 'none',
  set: (value: RequestAuth['kind']) => {
    if (!form.value.auth) form.value.auth = emptyAuth()
    form.value.auth.kind = value
  },
})

const authActive = computed(() => authKind.value !== 'none')

const oauthLoading = ref(false)

async function fetchOauthToken() {
  const auth = form.value.auth
  if (!auth) return
  if (!auth.tokenUrl?.trim()) {
    message.warning('请填写 Token URL')
    return
  }
  oauthLoading.value = true
  try {
    const token = await api.oauth2Token(
      {
        tokenUrl: renderRuntimeVars(auth.tokenUrl.trim()),
        clientId: renderRuntimeVars(auth.clientId ?? ''),
        clientSecret: renderRuntimeVars(auth.clientSecret ?? ''),
        scope: renderRuntimeVars(auth.scope ?? ''),
      },
      insecure.value,
    )
    auth.token = token
    message.success('已获取访问令牌，发送请求时将自动使用')
  } catch (e) {
    message.error(String(e))
  } finally {
    oauthLoading.value = false
  }
}

/* ---------- Body 各类型草稿：切换类型时保留各自内容 ---------- */
const bodyDrafts = ref<Record<string, string>>({})

function parseBodyDrafts(raw?: string): Record<string, string> {
  if (!raw) return {}
  try {
    const parsed = JSON.parse(raw)
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return Object.fromEntries(
        Object.entries(parsed).filter(([, value]) => typeof value === 'string'),
      ) as Record<string, string>
    }
  } catch {
    // 旧数据可能是空字符串或非法 JSON，按没有草稿处理
  }
  return {}
}

function rememberBodyDraft(type: string, body: string) {
  if (!type || type === 'none' || hydrating) return
  if (bodyDrafts.value[type] === body) return
  bodyDrafts.value = { ...bodyDrafts.value, [type]: body }
}

/**
 * 保存时序列化「其它类型」的草稿。
 * 当前类型的正文走 body 字段，不必重复存一份；这样大 Body 只在保存时序列化一次小对象，
 * 而不是每次按键都 JSON.stringify 整份草稿。
 */
function bodyMetaFor(type: string): string {
  const drafts: Record<string, string> = {}
  for (const [key, value] of Object.entries(bodyDrafts.value)) {
    if (key !== type) drafts[key] = value
  }
  return Object.keys(drafts).length ? JSON.stringify(drafts) : ''
}

/* ---------- multipart / binary / GraphQL ---------- */
const multipartFields = computed<FormField[]>({
  get() {
    if (form.value.bodyType !== 'multipart') return []
    try {
      const parsed = JSON.parse(form.value.body || '[]')
      return Array.isArray(parsed) ? (parsed as FormField[]) : []
    } catch {
      return []
    }
  },
  set(fields) {
    form.value.body = JSON.stringify(fields)
  },
})

function addMultipartField(kind: 'text' | 'file') {
  multipartFields.value = [
    ...multipartFields.value,
    { key: '', value: '', enabled: true, kind },
  ]
}

function removeMultipartField(index: number) {
  const fields = [...multipartFields.value]
  fields.splice(index, 1)
  multipartFields.value = fields
}

async function pickFileForField(index: number) {
  const picked = await open({ multiple: false, directory: false })
  if (typeof picked !== 'string') return
  const fields = [...multipartFields.value]
  const field = fields[index]
  if (!field) return
  fields[index] = { ...field, value: picked, kind: 'file' }
  multipartFields.value = fields
}

const binaryPath = computed({
  get: () => (form.value.bodyType === 'binary' ? form.value.body || '' : ''),
  set: (value: string) => {
    form.value.body = value
  },
})

async function pickBinaryFile() {
  const picked = await open({ multiple: false, directory: false })
  if (typeof picked === 'string') binaryPath.value = picked
}

/* ---------- urlencoded 表单（表格编辑，存储仍为 a=b&c=d） ---------- */
function decodeFormPart(part: string): string {
  try {
    return decodeURIComponent(part.replace(/\+/g, ' '))
  } catch {
    return part
  }
}

function encodeFormPart(part: string): string {
  // {{变量}} 保持原样，避免被编码后无法在发送时替换
  return part
    .split(/(\{\{[^{}]*\}\})/)
    .map((segment) => (/^\{\{[^{}]*\}\}$/.test(segment) ? segment : encodeURIComponent(segment)))
    .join('')
}

const formFields = computed<KeyValueItem[]>({
  get() {
    if (form.value.bodyType !== 'form') return []
    return (form.value.body || '')
      .split('&')
      .filter((chunk) => chunk.trim())
      .map((chunk) => {
        const index = chunk.indexOf('=')
        const rawKey = index >= 0 ? chunk.slice(0, index) : chunk
        const rawValue = index >= 0 ? chunk.slice(index + 1) : ''
        return { key: decodeFormPart(rawKey), value: decodeFormPart(rawValue), enabled: true }
      })
  },
  set(fields) {
    form.value.body = fields
      .filter((field) => field.key || field.value)
      .map((field) => `${encodeFormPart(field.key)}=${encodeFormPart(field.value)}`)
      .join('&')
  },
})

const gqlQuery = ref('')
const gqlVarsText = ref('')
const gqlVarsError = ref('')
let gqlSyncing = false

function loadGraphqlBody() {
  if (form.value.bodyType !== 'graphql') return
  gqlSyncing = true
  try {
    const parsed = JSON.parse(form.value.body || '{}')
    gqlQuery.value = typeof parsed.query === 'string' ? parsed.query : ''
    gqlVarsText.value =
      parsed.variables && Object.keys(parsed.variables).length
        ? JSON.stringify(parsed.variables, null, 2)
        : ''
    gqlVarsError.value = ''
  } catch {
    gqlQuery.value = form.value.body || ''
    gqlVarsText.value = ''
  } finally {
    gqlSyncing = false
  }
}

function applyGraphqlBody() {
  if (gqlSyncing || form.value.bodyType !== 'graphql') return
  let variables: Record<string, unknown> = {}
  const text = gqlVarsText.value.trim()
  if (text) {
    try {
      const parsed = JSON.parse(text)
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        variables = parsed as Record<string, unknown>
        gqlVarsError.value = ''
      } else {
        gqlVarsError.value = '变量需为 JSON 对象，已按空对象发送'
      }
    } catch {
      gqlVarsError.value = '变量不是合法 JSON，已按空对象发送'
    }
  } else {
    gqlVarsError.value = ''
  }
  form.value.body = JSON.stringify({ query: gqlQuery.value, variables })
}

watch([gqlQuery, gqlVarsText], applyGraphqlBody)

watch(
  () => form.value.bodyType,
  (type, previous) => {
    if (type === previous) return
    // 切走前先记住旧类型的 Body，再恢复目标类型上次的内容
    if (previous) rememberBodyDraft(previous, form.value.body ?? '')
    const draft = bodyDrafts.value[type]
    form.value.body = draft !== undefined ? draft : type === 'multipart' ? '[]' : ''
    if (type === 'graphql') loadGraphqlBody()
  },
)

/* ---------- Cookie 管理 ---------- */
const cookieOpen = ref(false)
const cookieLoading = ref(false)
const cookies = ref<CookieItem[]>([])

async function openCookies() {
  cookieOpen.value = true
  await loadCookies()
}

async function loadCookies() {
  cookieLoading.value = true
  try {
    cookies.value = await api.cookieList()
  } catch (e) {
    message.error(String(e))
  } finally {
    cookieLoading.value = false
  }
}

async function removeCookie(id: number) {
  await api.cookieDelete(id)
  await loadCookies()
  message.success('已删除 Cookie')
}

function clearCookies() {
  Modal.confirm({
    title: '清空本机全部 Cookie？',
    content: '清空后需要重新登录才能恢复会话态，项目与接口配置不受影响。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.cookieClear()
      await loadCookies()
      message.success('已清空 Cookie')
    },
  })
}

interface HeaderPreset {
  id: number
  group: string
  key: string
  value: string
}

const headerPresets: HeaderPreset[] = [
  { id: 1, group: '常用', key: 'Accept', value: 'application/json' },
  { id: 2, group: '常用', key: 'Content-Type', value: 'application/json' },
  { id: 3, group: '常用', key: 'User-Agent', value: 'AstroMan/1.0' },
  { id: 4, group: '常用', key: 'Cache-Control', value: 'no-cache' },
  { id: 5, group: '常用', key: 'Connection', value: 'keep-alive' },
  { id: 6, group: '认证', key: 'Authorization', value: 'Bearer {{token}}' },
  { id: 7, group: '认证', key: 'Authorization', value: 'Basic {{credentials}}' },
  { id: 8, group: '认证', key: 'X-Api-Key', value: '{{api_key}}' },
  { id: 9, group: '认证', key: 'X-Auth-Token', value: '{{token}}' },
  { id: 10, group: '内容协商', key: 'Accept', value: 'text/html,application/xhtml+xml' },
  { id: 11, group: '内容协商', key: 'Accept-Encoding', value: 'gzip, deflate, br' },
  { id: 12, group: '内容协商', key: 'Accept-Language', value: 'zh-CN,zh;q=0.9' },
  { id: 13, group: '内容协商', key: 'Content-Type', value: 'application/x-www-form-urlencoded' },
  { id: 14, group: '内容协商', key: 'Content-Type', value: 'multipart/form-data' },
  { id: 15, group: '客户端信息', key: 'Origin', value: '{{base_url}}' },
  { id: 16, group: '客户端信息', key: 'Referer', value: '{{base_url}}/' },
  { id: 17, group: '客户端信息', key: 'Cookie', value: 'session={{session_id}}' },
  { id: 18, group: '客户端信息', key: 'X-Requested-With', value: 'XMLHttpRequest' },
  { id: 19, group: '客户端信息', key: 'X-Request-Id', value: '{{uuid}}' },
  { id: 20, group: '条件请求', key: 'If-None-Match', value: '{{etag}}' },
  { id: 21, group: '条件请求', key: 'If-Modified-Since', value: '{{last_modified}}' },
]

const headerGroups = computed(() => {
  const groups: { label: string; items: HeaderPreset[] }[] = []
  for (const preset of headerPresets) {
    let group = groups.find((item) => item.label === preset.group)
    if (!group) {
      group = { label: preset.group, items: [] }
      groups.push(group)
    }
    group.items.push(preset)
  }
  return groups
})

function applyHeaderPreset(id: number) {
  const preset = headerPresets.find((item) => item.id === id)
  if (!preset) return
  const existing = form.value.headers.find(
    (item) => item.key.trim().toLowerCase() === preset.key.toLowerCase(),
  )
  if (existing) {
    existing.value = preset.value
    existing.enabled = true
  } else {
    form.value.headers.push({ key: preset.key, value: preset.value, enabled: true })
  }
  if (preset.key.toLowerCase() === 'content-type' && form.value.bodyType === 'none') {
    if (preset.value.includes('json')) form.value.bodyType = 'json'
    else if (preset.value.includes('urlencoded')) form.value.bodyType = 'form'
  }
  message.success(`已加入 ${preset.key}`)
}

// 连续快速切标签会并发触发 loadRequest，只让最后一次加载落地，
// 否则先发出的旧请求返回时会把上一个接口的详情写回面板
let loadToken = 0
/** 组件实例是否已卸载：后台请求结束时不再碰已销毁的视图 */
let disposed = false

async function loadRequest(id: number) {
  const token = ++loadToken
  if (!id) {
    loading.value = false
    return
  }
  // 切走之前，把当前接口的现场存下来
  const previousId = form.value.id
  if (previousId && previousId !== id) saveSnapshot(previousId)

  loading.value = true
  hydrating = true
  // 先清响应再拉详情：切标签立即可见空白，也不会把上一条的响应带过来
  resetResponseState()
  try {
    const detail = await api.requestGet(id)
    if (token !== loadToken) return
    if (!detail.auth) detail.auth = emptyAuth()
    // 先恢复各类型草稿，再赋值，避免 bodyType watcher 用空草稿覆盖当前 Body
    bodyDrafts.value = parseBodyDrafts(detail.bodyMeta)
    if (detail.bodyType && detail.bodyType !== 'none') {
      bodyDrafts.value = { ...bodyDrafts.value, [detail.bodyType]: detail.body ?? '' }
    }
    form.value = detail
    if (detail.bodyType === 'graphql') loadGraphqlBody()
    formVersion = 0
    savedVersion = 0
    autoState.value = 'saved'
    tabStore.ensureProject(projectId.value)
    const opened = tabStore.open({
      id,
      name: detail.name || '未命名请求',
      method: detail.method,
    })
    if (!opened) {
      message.warning(`最多同时打开 ${MAX_REQUEST_TABS} 个接口，请先关闭部分标签`)
    }
    revealActiveTab()
    // 记住本接口，返回项目页时自动定位
    rememberLastRequest(projectId.value, {
      id,
      collectionId: detail.collectionId,
      name: detail.name,
      method: detail.method,
    })
    await loadRuns()
    if (token !== loadToken) return

    // 优先恢复内存现场；没有现场时从最近一条运行记录有损恢复
    const snapshot = workspace.get(id)
    if (snapshot) {
      restoreSnapshot(snapshot)
      if (snapshot.expandedRunId) void ensureRunDetail(snapshot.expandedRunId)
      if (snapshot.diffOpen && snapshot.diffBaseRunId && !snapshot.diffUseCustom) {
        void ensureRunDetail(snapshot.diffBaseRunId)
      }
    } else if (runs.value[0] && shouldRestoreFromRun(id)) {
      await restoreRunAsResult(runs.value[0])
      if (token !== loadToken) return
    }

    // 在途请求属于当前接口：恢复“执行中”状态；真正的 Promise 在发起它的闭包里
    const inflight = workspace.inflightOf(id)
    if (inflight) requestKey.value = inflight.requestKey
  } catch (e) {
    if (token !== loadToken) return
    tabStore.close(id)
    message.error(String(e))
  } finally {
    if (token === loadToken) {
      loading.value = false
      hydrating = false
    }
  }
}

/* ---------- 快速切换到本项目其他接口 ---------- */
const quickOptions = ref<RequestSearchHit[]>([])
const quickValue = ref<number | undefined>(undefined)

async function loadQuickOptions() {
  if (!projectId.value) return
  try {
    quickOptions.value = await api.requestOptions(projectId.value)
  } catch {
    quickOptions.value = []
  }
}

function quickOpen(value: number) {
  quickValue.value = undefined
  if (value === requestId.value) return
  if (!tabStore.canOpen(value)) {
    message.warning(`最多同时打开 ${MAX_REQUEST_TABS} 个接口，请先关闭部分标签`)
    return
  }
  switchTab(value)
}

onMounted(async () => {
  try {
    if (projectId.value && !store.environments.length) {
      store.environments = await api.envList(projectId.value)
    }
    const active = store.environments.find((e) => e.isActive) ?? store.environments[0]
    envId.value = active?.id ?? null
  } catch (e) {
    message.error(String(e))
  }
  await loadRequest(requestId.value)
  await loadQuickOptions()
})

watch(requestId, async (id, previous) => {
  if (!id || id === previous) return
  // 路由已被其他入口（关闭标签、返回）带走了，作废还在排队的 switchTab
  switchToken++
  await flushSave()
  // 连续点击多个标签时，路由最终停在最后一个；前面排队的旧切换直接放弃
  if (requestId.value !== id) return
  await loadRequest(id)
})

onBeforeRouteLeave(async () => {
  saveSnapshot()
  await flushSave()
})

onBeforeUnmount(() => {
  disposed = true
  saveSnapshot()
  if (bodySizeTimer) window.clearTimeout(bodySizeTimer)
})

// 页面挂载期间，后台请求完成时自动回填；本组件自己发起的请求直接改视图，不重复回填
watch(
  () => workspace.snapshots[requestId.value]?.stamp ?? 0,
  (stamp, previous) => {
    if (!stamp || stamp === previous) return
    if (form.value.id !== requestId.value) return
    if (sendingRequestIds.has(requestId.value)) return
    const snapshot = workspace.get(requestId.value)
    if (!snapshot) return
    restoreSnapshot(snapshot)
    void loadRuns().then(() => {
      if (disposed || form.value.id !== requestId.value) return
      if (snapshot.diffOpen && snapshot.diffBaseRunId && !snapshot.diffUseCustom) {
        void ensureRunDetail(snapshot.diffBaseRunId)
      }
      if (!snapshot.expandedRunId) return
      expandedRunId.value = snapshot.expandedRunId
      void ensureRunDetail(snapshot.expandedRunId)
    })
  },
)

async function flushSave(): Promise<boolean> {
  if (saveTimer) {
    window.clearTimeout(saveTimer)
    saveTimer = undefined
  }
  return save(false)
}

/** 浅拷贝出参：大 Body 字符串按引用传递，不做 JSON 深拷贝 */
function buildSavePayload(): HttpRequestDetail {
  const current = form.value
  return {
    ...current,
    headers: current.headers.map((item) => ({ ...item })),
    params: current.params.map((item) => ({ ...item })),
    auth: { ...(current.auth ?? emptyAuth()) },
    bodyMeta: bodyMetaFor(current.bodyType),
  }
}

async function persist(showToast: boolean): Promise<boolean> {
  if (!form.value.id) return true
  if (!showToast && formVersion === savedVersion) {
    autoState.value = 'saved'
    return true
  }
  const version = formVersion
  saving.value = true
  autoState.value = 'saving'
  try {
    // 自动保存只需要「写进去」，不需要把整份详情再传回来（大 Body 往返很贵）
    await api.requestSaveQuick(buildSavePayload())
    savedVersion = version
    if (formVersion === version) {
      autoState.value = 'saved'
    } else {
      autoState.value = 'dirty'
      scheduleAutoSave()
    }
    syncTabMeta()
    if (showToast) message.success('已保存')
    return true
  } catch (e) {
    autoState.value = 'error'
    message.error(String(e))
    return false
  } finally {
    saving.value = false
  }
}

function save(showToast = true): Promise<boolean> {
  if (!form.value.id) return Promise.resolve(true)
  saveQueue = saveQueue.then(
    () => persist(showToast),
    () => persist(showToast),
  )
  return saveQueue
}

async function goBack() {
  await flushSave()
  // 优先回到真正来源页（例如从历史记录点进来就回历史记录）；
  // 若上一页只是另一个请求标签页，则按原逻辑回项目
  const prev = previousEntry.value
  const fromHistory = prev && !prev.path.startsWith('/request/')
  router.push(fromHistory ? backOr(`/project/${projectId.value}`) : `/project/${projectId.value}`)
}

/** 标签过多出现横向滚动时，把当前激活标签滚进视野 */
function revealActiveTab() {
  nextTick(() => {
    const active = tabsScrollEl.value?.querySelector<HTMLElement>('.req-tab.active')
    active?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  })
}

// 连续点击不同标签时只认最后一次点击，避免慢保存把路由推回上一个标签
let switchToken = 0

async function switchTab(id: number) {
  if (id === requestId.value) return
  const token = ++switchToken
  await flushSave()
  if (token !== switchToken) return
  router.push({
    name: 'request',
    params: { id },
    query: projectId.value ? { projectId: projectId.value } : {},
  })
}

async function closeTab(id: number) {
  const isActive = id === requestId.value
  const next = tabStore.close(id)
  if (!isActive) return
  await flushSave()
  if (next) {
    router.push({
      name: 'request',
      params: { id: next },
      query: projectId.value ? { projectId: projectId.value } : {},
    })
  } else {
    router.push(`/project/${projectId.value}`)
  }
}

async function closeAllTabs() {
  await flushSave()
  tabStore.closeAll()
  router.push(`/project/${projectId.value}`)
}

function deleteRequest() {
  const id = form.value.id
  if (!id) return
  Modal.confirm({
    title: `删除请求「${form.value.name || '未命名请求'}」？`,
    content: '该请求及其参数、请求头、Body 将被永久删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.requestDelete(id)
      workspace.forget(id)
      form.value.id = undefined
      tabStore.close(id)
      message.success('已删除请求')
      const next = tabStore.activeId
      if (next) {
        router.push({
          name: 'request',
          params: { id: next },
          query: projectId.value ? { projectId: projectId.value } : {},
        })
      } else {
        router.push(projectId.value ? `/project/${projectId.value}` : '/')
      }
    },
  })
}

/** 执行出错时附上上下文，方便定位是哪一次发送、哪个环境 */
function execContext(stage: string) {
  const env = store.environments.find((item) => item.id === envId.value)
  return [
    { label: '阶段', value: stage },
    { label: '接口', value: form.value.name || '未命名请求' },
    { label: '请求', value: `${form.value.method} ${form.value.url || '（未填写 URL）'}` },
    { label: '环境', value: env?.name ?? '未选择' },
  ]
}

async function send() {
  if (!form.value.url.trim()) {
    message.warning('请填写 URL')
    return
  }
  // 记住发起时的接口与关键配置：中途切标签后，后台请求仍按发起时的口径完成
  const ownerId = form.value.id
  const ownerName = form.value.name
  const ownerAuth = form.value.auth ?? null
  const preCode = form.value.preScript?.trim()
  const postCode = form.value.postScript?.trim()
  const ownerEnvId = envId.value
  const ownerRequestKey = `req-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
  const ownerRetry = {
    enabled: retryEnabled.value,
    maxAttempts: retryMaxAttempts.value,
    allowNonIdempotent: retryAllowNonIdempotent.value,
  }

  if (ownerId) sendingRequestIds.add(ownerId)
  result.value = null
  resultSource.value = 'none'
  restoredRunId.value = null
  wasCancelled.value = false
  // 每次发送一个新标识：重试/重定向都在同一标识下，取消时能一次打中
  requestKey.value = ownerRequestKey
  if (ownerId) workspace.markInflight(ownerId, ownerRequestKey)
  const turn: RequestTurnState = { logs: [], assertions: [], error: '', ms: 0 }
  resetScriptOutput()
  // 先清掉内存现场的旧响应，避免发送过程中切回来看到上一次的结果
  if (ownerId) {
    workspace.save(ownerId, {
      result: null,
      turn: { ...turn },
      resultSource: 'none',
      restoredRunId: null,
      responseTab: responseTab.value,
      bodyView: bodyView.value,
      activeTab: activeTab.value,
      bodyAbsPath: bodyAbsPath.value,
      bodyPreviewError: bodyPreviewError.value,
    })
  }

  const commitTurn = () => {
    if (disposed || form.value.id !== ownerId) return
    scriptLogs.value = turn.logs.slice()
    scriptAssertions.value = turn.assertions.slice()
    scriptError.value = turn.error
    scriptMs.value = turn.ms
  }

  try {
    const envVars = activeEnvVars()
    const request: ScriptRequest = {
      method: form.value.method,
      url: form.value.url,
      headers: kvToMap(form.value.headers),
      params: kvToMap(form.value.params),
      bodyType: form.value.bodyType,
      body: form.value.body ?? '',
    }
    let outgoing = request
    const envScript = store.environments.find((item) => item.id === ownerEnvId)?.script?.trim()
    if (envScript) {
      const envResult = await runScriptJob({
        phase: 'env',
        code: envScript,
        request,
        env: envVars,
        vars: listRuntimeVars(),
      })
      turn.logs.push(...envResult.logs)
      turn.ms += envResult.durationMs
      turn.error = envResult.error ?? ''
      commitTurn()
      if (envResult.error) {
        if (ownerId) {
          workspace.save(ownerId, { turn: { ...turn }, resultSource: 'none' })
        }
        if (!disposed && form.value.id === ownerId) {
          errorDialog.show({
            title: '环境脚本执行失败',
            error: envResult.error,
            context: execContext('环境脚本'),
          })
        }
        return
      }
      setRuntimeVars(envResult.vars)
      outgoing = envResult.request
    }

    if (preCode) {
      const preResult = await runScriptJob({
        phase: 'pre',
        code: preCode,
        request: outgoing,
        env: envVars,
        vars: listRuntimeVars(),
      })
      turn.logs.push(...preResult.logs)
      turn.ms += preResult.durationMs
      turn.error = preResult.error ?? ''
      commitTurn()
      if (preResult.error) {
        if (ownerId) {
          workspace.save(ownerId, { turn: { ...turn }, resultSource: 'none' })
        }
        if (!disposed && form.value.id === ownerId) {
          errorDialog.show({
            title: '前置脚本执行失败',
            error: preResult.error,
            context: execContext('前置脚本'),
          })
        }
        return
      }
      setRuntimeVars(preResult.vars)
      outgoing = preResult.request
    }

    const payload: SendRequestPayload = {
      requestId: ownerId ?? null,
      name: ownerName,
      method: outgoing.method,
      url: renderRuntimeVars(outgoing.url),
      headers: mapToItems(outgoing.headers).map((item) => ({
        ...item,
        value: renderRuntimeVars(item.value),
      })),
      params: mapToItems(outgoing.params).map((item) => ({
        ...item,
        value: renderRuntimeVars(item.value),
      })),
      bodyType: outgoing.bodyType,
      body: renderRuntimeVars(outgoing.body),
      envId: ownerEnvId,
      insecure: insecure.value,
      timeoutMs: 30000,
      auth: ownerAuth,
      cookieEnabled: true,
      // v1.0.8：让后端能定位到这条在途请求（「停止」按钮依赖它）
      requestKey: ownerRequestKey,
      protocol: protocol.value === 'auto' ? null : protocol.value,
      followRedirects: followRedirects.value,
      retry: ownerRetry,
    }

    const incoming = await api.sendRequest(payload)
    const stillOwner = !disposed && form.value.id === ownerId

    // 无论用户是否还在当前接口，都先把实时响应写进内存现场
    if (ownerId) {
      workspace.save(ownerId, {
        result: incoming,
        turn: { ...turn },
        resultSource: 'live',
        restoredRunId: null,
        ...(stillOwner
          ? {
              responseTab: responseTab.value,
              bodyView: bodyView.value,
              activeTab: activeTab.value,
              bodyAbsPath: bodyAbsPath.value,
              bodyPreviewError: bodyPreviewError.value,
            }
          : {}),
      })
      workspace.clearInflight(ownerId)
    }

    if (stillOwner) {
      result.value = incoming
      resultSource.value = 'live'
      restoredRunId.value = null
      wasCancelled.value = incoming.cancelled === true
      commitTurn()

      // 用户主动停止：不是错误，用 info 提示即可，不弹错误框
      if (wasCancelled.value) {
        message.info('已停止本次请求')
      } else if (incoming.error) {
        // 超时、DNS、TLS、连接被拒等执行期错误会放在 result.error，直接弹窗展示完整原因
        errorDialog.show({
          title: '请求发送失败',
          error: incoming.error,
          context: execContext('发送请求'),
        })
      }
    }

    if (postCode) {
      const postResult = await runScriptJob({
        phase: 'post',
        code: postCode,
        request: outgoing,
        response: incoming,
        env: envVars,
        vars: listRuntimeVars(),
      })
      if (!postResult.error) setRuntimeVars(postResult.vars)
      turn.logs.push(...postResult.logs)
      turn.assertions = postResult.assertions
      turn.ms += postResult.durationMs
      turn.error = postResult.error ?? ''
      if (!disposed && form.value.id === ownerId) {
        commitTurn()
        if (postResult.error) {
          errorDialog.show({
            title: '后置脚本执行失败',
            error: postResult.error,
            context: execContext('后置脚本'),
          })
        }
        if (postResult.assertions.length || postResult.error) responseTab.value = 'tests'
      }
      // 后置脚本的日志与断言也写进现场
      if (ownerId) {
        workspace.save(ownerId, {
          turn: { ...turn },
          ...(!disposed && form.value.id === ownerId
            ? { responseTab: responseTab.value, bodyView: bodyView.value }
            : {}),
        })
      }
    }
  } catch (e) {
    // 已经切到别的接口：旧请求的错误不再打扰当前接口
    if (!disposed && form.value.id === ownerId) {
      errorDialog.show({
        title: '请求执行失败',
        error: e,
        context: execContext('请求执行'),
      })
    }
  } finally {
    if (ownerId) {
      sendingRequestIds.delete(ownerId)
      workspace.clearInflight(ownerId)
    }
    // 切标签后可能已经在新接口上发起请求，别把新请求的「发送中」状态提前关掉
    if (form.value.id === ownerId && !disposed) {
      scriptVars.value = listRuntimeVars()
    }
  }
  if (!disposed && form.value.id === ownerId) await loadRuns()
}

/* ---------- v1.0.8：停止在途请求 ---------- */
const cancelling = ref(false)

async function cancelSend() {
  // 以当前接口的在途标识为准：切走再切回后，requestKey 可能已被别的接口覆盖
  const id = form.value.id
  const key = (id ? workspace.inflightOf(id)?.requestKey : '') || requestKey.value
  if (!key || cancelling.value) return
  cancelling.value = true
  try {
    // 返回 false 说明请求刚好结束了，静默忽略即可
    await api.httpCancel(key)
  } catch (e) {
    errorDialog.show({ title: '停止请求失败', error: e, context: { 操作: 'http_cancel' } })
  } finally {
    cancelling.value = false
  }
}

/* ---------- v1.0.8：耗时分解（瀑布图） ----------
   口径：dns / connect 只在建连时有值；复用连接时 connectMs = 0 且 reusedConnection = true，
   这不代表「连接极快」。代理场景下 connect 不含代理握手，必须标注。 */
interface TimingRow {
  key: string
  label: string
  ms: number
  /** 在总时长里的起始位置百分比 */
  offset: number
  width: number
  color: string
  hint: string
}

const timings = computed(() => result.value?.timings ?? null)

const timingRows = computed<TimingRow[]>(() => {
  const t = timings.value
  if (!t) return []
  const total = Math.max(t.totalMs, 1)
  const rows: TimingRow[] = []
  let cursor = 0
  const push = (key: string, label: string, ms: number, color: string, hint: string) => {
    if (!(ms > 0)) return
    rows.push({
      key,
      label,
      ms,
      offset: (cursor / total) * 100,
      width: Math.max((ms / total) * 100, 0.5),
      color,
      hint,
    })
    cursor += ms
  }
  if (t.dnsMs && t.dnsMs > 0) push('dns', 'DNS 解析', t.dnsMs, '#8b5cf6', '域名解析耗时')
  push(
    'connect',
    t.reusedConnection ? '连接（复用）' : 'TCP + TLS',
    t.connectMs,
    '#0ea5e9',
    t.reusedConnection ? '复用了已有连接，未新建握手' : '含 TCP 与 TLS 握手',
  )
  push('ttfb', '等待首字节', t.ttfbMs, '#f59e0b', '从请求发出到收到响应首字节')
  push('download', '正文下载', t.downloadMs, '#10b981', '读取响应体的耗时')
  return rows
})

/** 计时口径说明：把「容易被误读」的地方显式写出来 */
const timingNotes = computed<string[]>(() => {
  const t = timings.value
  if (!t) return []
  const notes: string[] = []
  if (t.reusedConnection) {
    notes.push('本次复用了已有连接，连接耗时记为 0 —— 这表示「没有新建连接」，不是「连接很快」。')
  }
  if (t.proxy) {
    notes.push('经过代理：连接耗时不含代理握手时间，实际网络时间会比这里显示的更长。')
  }
  if (!t.dnsMs) {
    notes.push('未记录到 DNS 阶段：域名已被解析缓存，或请求直连 IP。')
  }
  return notes
})

/* ---------- v1.0.8：二进制响应 ---------- */
const bodyKind = computed(() => result.value?.bodyKind ?? 'text')
const isBinaryBody = computed(
  () => bodyKind.value === 'binary' || bodyKind.value === 'stream',
)
const isImageBody = computed(() => {
  const type = (result.value?.headers?.['content-type'] ?? '').toLowerCase()
  return type.startsWith('image/')
})
const isPdfBody = computed(() => {
  const type = (result.value?.headers?.['content-type'] ?? '').toLowerCase()
  return type.includes('application/pdf')
})

/** 落盘响应的绝对路径：asset: 预览与「打开所在目录」都用它 */
const bodyAbsPath = ref('')
const bodyPreviewError = ref('')

const bodyPreviewSrc = computed(() => {
  const preview = result.value?.previewBase64
  if (preview) {
    const type = (result.value?.headers?.['content-type'] ?? 'image/png').split(';')[0]
    return `data:${type};base64,${preview}`
  }
  if (bodyAbsPath.value) {
    try {
      return convertFileSrc(bodyAbsPath.value)
    } catch {
      return ''
    }
  }
  return ''
})

watch(
  () => result.value?.bodyFile,
  async (relPath) => {
    bodyAbsPath.value = ''
    bodyPreviewError.value = ''
    if (!relPath) return
    try {
      bodyAbsPath.value = await api.responseFilePath(relPath)
    } catch (e) {
      bodyPreviewError.value = String(e)
    }
  },
  { immediate: true },
)

/** 二进制响应「另存为」：有落盘文件就走流式拷贝，没有就用 Base64 写盘 */
async function saveBinaryBody() {
  const current = result.value
  if (!current) return
  if (
    resultSource.value === 'run' &&
    !current.bodyFile &&
    !current.previewBase64 &&
    !current.body
  ) {
    message.warning('运行记录未保存二进制内容，请重新发送后再保存')
    return
  }
  const suggested = guessFileName(current)
  const target = await saveFile({ defaultPath: suggested })
  if (!target) return
  try {
    if (current.bodyFile) {
      const absolute = bodyAbsPath.value || (await api.responseFilePath(current.bodyFile))
      await api.fileCopy(absolute, target)
    } else if (current.previewBase64) {
      await api.fileWriteBinary(target, current.previewBase64)
    } else {
      await api.fileWriteText(target, current.body)
    }
    message.success(`已保存到 ${target}`)
  } catch (e) {
    errorDialog.show({ title: '保存响应失败', error: e, context: { 操作: '另存响应体' } })
  }
}

/** 从 Content-Disposition / URL 猜一个像样的文件名 */function guessFileName(current: HttpResult): string {
  const disposition = current.headers?.['content-disposition'] ?? ''
  const quoted = /filename\*?=(?:UTF-8''|")?([^";]+)/i.exec(disposition)
  if (quoted?.[1]) {
    try {
      return decodeURIComponent(quoted[1].trim())
    } catch {
      return quoted[1].trim()
    }
  }
  const type = (current.headers?.['content-type'] ?? '').toLowerCase()
  const ext = type.includes('pdf')
    ? 'pdf'
    : type.includes('png')
      ? 'png'
      : type.includes('jpeg')
        ? 'jpg'
        : type.includes('gif')
          ? 'gif'
          : type.includes('webp')
            ? 'webp'
            : type.includes('zip')
              ? 'zip'
              : 'bin'
  return `response-${current.status}-${Date.now()}.${ext}`
}

/** 打开响应文件所在目录（复用 storage_open，避免再引一个 opener 插件） */
async function openBodyFolder() {
  if (!bodyAbsPath.value) return
  const sep = bodyAbsPath.value.includes('\\') ? '\\' : '/'
  const dir = bodyAbsPath.value.slice(0, bodyAbsPath.value.lastIndexOf(sep))
  try {
    await api.storageOpen(dir)
  } catch (e) {
    errorDialog.show({ title: '打开目录失败', error: e, context: { 操作: 'storage_open' } })
  }
}

async function copyBodyPath() {
  if (!bodyAbsPath.value) return
  try {
    await navigator.clipboard.writeText(bodyAbsPath.value)
    message.success('路径已复制')
  } catch {
    message.warning('复制失败，请手动选择路径')
  }
}

const runs = ref<RequestRunSummary[]>([])
/** 运行详情按需加载：列表只带 2KB 预览，展开某条时再取完整参数与响应 */
const runDetails = reactive(new Map<number, RequestRun>())
const runDetailLoading = reactive(new Set<number>())
const runsOpen = ref(true)
const runsLoading = ref(false)
const expandedRunId = ref<number | null>(null)

/* ---------- 运行历史：分页 20 条（整页共用一个滚动，不再单独限高） ---------- */
const RUNS_PAGE_SIZE = 20
const runsPage = ref(1)
const runsPanelEl = ref<HTMLElement | null>(null)

const runsTotalPages = computed(() => Math.max(1, Math.ceil(runs.value.length / RUNS_PAGE_SIZE)))

const pagedRuns = computed(() => {
  const start = (runsPage.value - 1) * RUNS_PAGE_SIZE
  return runs.value.slice(start, start + RUNS_PAGE_SIZE)
})

function setRunsPage(page: number) {
  const next = Math.min(Math.max(1, page), runsTotalPages.value)
  if (next === runsPage.value) return
  runsPage.value = next
  expandedRunId.value = null
  // 翻页后把列表顶部带回视野，避免停在上一页的滚动位置
  nextTick(() => {
    runsPanelEl.value?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  })
}

async function loadRuns() {
  const id = form.value.id
  if (!id) return
  runsLoading.value = true
  try {
    const list = await api.runList(id, 200)
    // 等结果期间已切到别的接口：不要把旧接口的运行记录塞进当前面板
    if (form.value.id !== id) return
    runs.value = list
    runsPage.value = 1
    runDetails.clear()
    runDetailLoading.clear()
    expandedRunId.value = null
  } catch (e) {
    if (form.value.id === id) message.error(String(e))
  } finally {
    if (form.value.id === id) runsLoading.value = false
  }
}

async function ensureRunDetail(id: number): Promise<RequestRun | null> {
  const cached = runDetails.get(id)
  if (cached) return cached
  if (runDetailLoading.has(id)) return null
  runDetailLoading.add(id)
  try {
    const detail = await api.runGet(id)
    runDetails.set(id, detail)
    return detail
  } catch (e) {
    message.error(String(e))
    return null
  } finally {
    runDetailLoading.delete(id)
  }
}

async function toggleRun(run: RequestRunSummary) {
  if (expandedRunId.value === run.id) {
    expandedRunId.value = null
    return
  }
  expandedRunId.value = run.id
  await ensureRunDetail(run.id)
}

function runStatusColor(status: number) {
  if (!status) return 'var(--fm-danger)'
  if (status < 300) return 'var(--fm-success)'
  if (status < 400) return 'var(--fm-warning)'
  return 'var(--fm-danger)'
}

function fmtRunTime(ts: number) {
  return formatDateTime(ts)
}

/** 当前响应由哪条运行记录恢复而来 */
const restoredRun = computed(
  () => runs.value.find((run) => run.id === restoredRunId.value) ?? null,
)

function fmtAgo(ts: number) {
  const diff = Math.max(0, Date.now() - ts)
  if (diff < 60_000) return '刚刚'
  if (diff < 3600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86400_000) return `${Math.floor(diff / 3600_000)} 小时前`
  return `${Math.floor(diff / 86400_000)} 天前`
}

const runSourceHint = computed(() => {
  const run = restoredRun.value
  const when = run ? ` · ${fmtAgo(run.createdAt)}` : ''
  return run?.responseBodyTruncated
    ? `来自运行记录${when}，记录只保存前 256KB；完整响应请查看该次运行或重新发送`
    : `来自运行记录${when}，耗时分解、重定向等实时信息不包含在历史记录中`
})

function fmtTimeOf(ts: number) {
  return formatDateTime(ts)
}

function runHeaders(id: number) {
  return Object.entries(runDetails.get(id)?.responseHeaders ?? {})
}

function runParams(run: RequestRunSummary) {
  return (runDetails.get(run.id)?.params ?? []).filter((item) => item.enabled)
}

function runRequestHeaders(run: RequestRunSummary) {
  return runDetails.get(run.id)?.headers ?? []
}

function runBodyText(run: RequestRunSummary) {
  return runDetails.get(run.id)?.body || run.requestBodyPreview
}

function runResponseText(run: RequestRunSummary) {
  return runDetails.get(run.id)?.responseBody || run.responseBodyPreview
}

function runHasExtraParams(run: RequestRunSummary) {
  return Boolean(runDetails.get(run.id)?.params?.length || runDetails.get(run.id)?.headers?.length)
}

function prettyRunBody(text: string) {
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    return text
  }
}

function clearRuns() {
  const id = form.value.id
  if (!id) return
  Modal.confirm({
    title: '清空该接口的运行历史？',
    content: '只删除这个接口的运行记录，不影响接口本身与压测报告。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.runClear(id)
      runs.value = []
      runDetails.clear()
      runDetailLoading.clear()
      expandedRunId.value = null
      runsPage.value = 1
      // 清空运行记录后，页面上已有的响应仍是实时结果，只去掉“来自运行记录”的来源标识
      if (resultSource.value === 'run') {
        resultSource.value = result.value ? 'live' : 'none'
        restoredRunId.value = null
        saveSnapshot(id)
      }
      message.success('已清空')
    },
  })
}

/* ---------- 响应体另存为 / 响应对比 ---------- */
const diffOpen = ref(false)
const diffBaseRunId = ref<number | null>(null)
const diffCustomText = ref('')
const diffUseCustom = ref(false)

const diffRunOptions = computed(() =>
  runs.value.map((run) => ({
    value: run.id,
    label: `#${run.id} · ${run.responseStatus || 'ERR'} · ${fmtRunTime(run.createdAt)}`,
  })),
)

function responseFileName() {
  const base = (form.value.name || 'response').replace(/[\\/:*?"<>|]/g, '_')
  const ext = prettyBody.value.trim().startsWith('{') || prettyBody.value.trim().startsWith('[') ? 'json' : 'txt'
  return `${base}.${ext}`
}

async function saveResponseBody() {
  if (!result.value) return
  try {
    const path = await saveFile({
      title: '保存响应体',
      defaultPath: responseFileName(),
      filters: [
        { name: 'JSON', extensions: ['json'] },
        { name: '文本', extensions: ['txt'] },
      ],
    })
    if (!path) return
    // 超过 2MB 的文本响应后端已经流式落盘，直接拷文件才能拿到完整内容
    // （编辑器里的 body 只保留前 5MB，截断后另存会丢数据）
    if (result.value.bodyFile) {
      const absolute = bodyAbsPath.value || (await api.responseFilePath(result.value.bodyFile))
      await api.fileCopy(absolute, path)
    } else {
      await api.fileWriteText(path, result.value.body)
    }
    message.success('响应体已保存')
  } catch (e) {
    message.error(String(e))
  }
}

async function openDiff() {
  diffUseCustom.value = runs.value.length === 0
  const first = runs.value[0]?.id ?? null
  diffBaseRunId.value = first
  diffCustomText.value = ''
  diffOpen.value = true
  if (first) await ensureRunDetail(first)
}

watch(diffBaseRunId, (id) => {
  if (!diffUseCustom.value && id) void ensureRunDetail(id)
})

const diffBaseText = computed(() => {
  if (diffUseCustom.value) return diffCustomText.value
  const run = diffBaseRunId.value ? runDetails.get(diffBaseRunId.value) : undefined
  return run ? prettyRunBody(run.responseBody) : ''
})

const diffCurrentText = computed(() => (bodyView.value === 'pretty' ? prettyBody.value : result.value?.body ?? ''))

const diffResult = computed<DiffLine[]>(() => {
  if (!diffOpen.value || !result.value) return []
  return diffLines(diffBaseText.value, diffCurrentText.value)
})

const diffSummary = computed(() => diffStats(diffResult.value))

async function openStress() {
  if (!form.value.url.trim()) {
    message.warning('请先填写 URL')
    return
  }
  const ok = await save(false)
  if (!ok) return
  router.push({
    name: 'stress',
    query: {
      requestId: String(form.value.id ?? requestId.value),
      projectId: projectId.value ? String(projectId.value) : undefined,
    },
  })
}

/** 超过该体量不再做 Pretty：一次 5MB 的 stringify 会明显卡住响应面板 */
const PRETTY_MAX = 512 * 1024

const prettyBody = computed(() => {
  const body = result.value?.body ?? ''
  if (!body || body.length > PRETTY_MAX) return body
  try {
    return JSON.stringify(JSON.parse(body), null, 2)
  } catch {
    return body
  }
})

/** 行数按需缓存，避免模板在每次渲染时 split 整段响应 */
const prettyLineCount = computed(() => {
  const text = prettyBody.value
  if (!text) return 0
  let count = 1
  let index = text.indexOf('\n')
  while (index !== -1) {
    count += 1
    index = text.indexOf('\n', index + 1)
  }
  return count
})

const bodyView = ref<'pretty' | 'raw'>('pretty')

// 新响应或切换 Pretty/Raw 后正文滚回顶部，避免停在上一次的滚动位置
watch([result, bodyView], () => {
  nextTick(() => {
    if (bodyPreEl.value) bodyPreEl.value.scrollTop = 0
  })
})

/* 字节数防抖：大 Body 每次按键都 new Blob 也是一笔可见开销 */
const bodySize = ref(0)
let bodySizeTimer: number | undefined

watch(
  () => form.value.body,
  (body) => {
    if (bodySizeTimer) window.clearTimeout(bodySizeTimer)
    bodySizeTimer = window.setTimeout(() => {
      bodySizeTimer = undefined
      bodySize.value = new Blob([body ?? '']).size
    }, 300)
  },
  { immediate: true },
)

const projectName = computed(
  () => store.projects.find((p) => p.id === projectId.value)?.name ?? '',
)

/** 超过该字符数时格式化前先确认：大文本解析和回填会占用几百毫秒 */
const FORMAT_CONFIRM_MAX = 2 * 1024 * 1024

async function formatBody() {
  const text = form.value.body
  if (!text.trim()) return
  if (text.length > FORMAT_CONFIRM_MAX) {
    const confirmed = await new Promise<boolean>((resolve) => {
      Modal.confirm({
        title: 'Body 较大，格式化可能停顿片刻',
        content: `当前 ${fmtSize(new Blob([text]).size)}，格式化会重新排版整段内容，是否继续？`,
        okText: '开始格式化',
        cancelText: '取消',
        onOk: () => resolve(true),
        onCancel: () => resolve(false),
      })
    })
    if (!confirmed) return
  }
  const hide = message.loading('正在格式化…', 0)
  try {
    form.value.body = await formatJsonAsync(text)
    message.success('已格式化')
  } catch {
    message.error('JSON 解析失败，请检查格式')
  } finally {
    hide()
  }
}

async function copyBody() {
  const text = result.value?.body ?? ''
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制响应体')
  } catch {
    message.error('复制失败')
  }
}

/* ---------- 代码片段（Postman 风格） ---------- */
const codeOpen = ref(false)
const snippetKey = ref('curl')
const snippets = computed<Snippet[]>(() => {
  const env = store.environments.find((item) => item.id === envId.value)
  return buildSnippets(form.value, env?.variables)
})
const activeSnippet = computed(
  () => snippets.value.find((item) => item.key === snippetKey.value) ?? snippets.value[0],
)

/* 生成内容默认只读；进入编辑后改动按代码类型分别留存，复制取改动后的文本 */
const codeEditing = ref(false)
const codeEditText = ref('')
const codeAreaRef = ref<HTMLTextAreaElement | null>(null)
const editedCodes = reactive<Record<string, string>>({})

const activeCode = computed(
  () => editedCodes[snippetKey.value] ?? activeSnippet.value.code,
)
const isCodeEdited = computed(() => editedCodes[snippetKey.value] !== undefined)

function startEditCode() {
  codeEditText.value = activeCode.value
  codeEditing.value = true
  nextTick(() => {
    fitCodeArea()
    codeAreaRef.value?.focus()
  })
}

function fitCodeArea() {
  const el = codeAreaRef.value
  if (!el) return
  const max = Math.max(260, window.innerHeight - 300)
  el.style.height = 'auto'
  el.style.height = `${Math.min(Math.max(el.scrollHeight + 2, 240), max)}px`
}

function finishEditCode() {
  // 与生成内容一致时不留残余草稿，避免误报「已修改」
  if (codeEditText.value === activeSnippet.value.code) {
    delete editedCodes[snippetKey.value]
  } else {
    editedCodes[snippetKey.value] = codeEditText.value
  }
  codeEditing.value = false
}

function resetCode() {
  delete editedCodes[snippetKey.value]
  codeEditText.value = activeSnippet.value.code
}

watch(codeEditText, (text) => {
  if (!codeEditing.value) return
  if (text === activeSnippet.value.code) delete editedCodes[snippetKey.value]
  else editedCodes[snippetKey.value] = text
  nextTick(fitCodeArea)
})

/* 编辑中切换语言：先落盘当前草稿，再载入目标语言的文本 */
watch(snippetKey, () => {
  codeEditText.value = activeCode.value
  if (codeEditing.value) nextTick(fitCodeArea)
})

/* 关闭抽屉时退出编辑态，下次打开仍是默认的只读视图（草稿保留） */
watch(codeOpen, (open) => {
  if (!open) codeEditing.value = false
})

function openCode() {
  if (!form.value.url?.trim()) {
    message.warning('请先填写请求 URL')
    return
  }
  codeOpen.value = true
}

async function copySnippet() {
  try {
    await navigator.clipboard.writeText(activeCode.value)
    message.success(
      isCodeEdited.value
        ? `已复制修改后的 ${activeSnippet.value.label} 代码`
        : `已复制 ${activeSnippet.value.label} 代码`,
    )
  } catch {
    message.error('复制失败')
  }
}

function fmtSize(n: number) {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}

const responseHeaders = computed(() => {
  if (!result.value) return []
  return Object.entries(result.value.headers).map(([key, value]) => ({ key, value }))
})
</script>

<template>
  <div class="editor-root">
    <a-spin :spinning="loading">
    <div class="editor-page">
      <div class="topbar">
        <div class="left">
          <a-button type="text" size="small" class="back-btn" @click="goBack">
            <template #icon><ArrowLeftOutlined /></template>
            返回
          </a-button>
          <span class="divider" />
          <span class="crumb">
            <span class="crumb-project">{{ projectName || '项目' }}</span>
            <span class="crumb-sep">/</span>
            <a-input
              v-model:value="form.name"
              class="name-input"
              placeholder="请求名称"
              :title="form.name"
            />
          </span>
          <span class="saved-hint" :class="`is-${autoState}`">
            <template v-if="autoState === 'saving'">保存中…</template>
            <template v-else-if="autoState === 'dirty'">待自动保存</template>
            <template v-else-if="autoState === 'error'">保存失败</template>
            <template v-else>已自动保存</template>
            <template v-if="form.id"> · #{{ form.id }}</template>
          </span>
        </div>
        <div class="right">
          <a-checkbox v-model:checked="insecure" class="insecure">忽略证书校验</a-checkbox>
          <a-popover trigger="click" placement="bottomRight" :overlay-style="{ width: '340px' }">
            <template #content>
              <div class="send-opt-panel">
                <div class="send-opt-title">发送选项</div>
                <div class="send-opt-row">
                  <span class="send-opt-label">协议</span>
                  <a-radio-group v-model:value="protocol" size="small">
                    <a-radio-button value="auto">自动</a-radio-button>
                    <a-radio-button value="http1">HTTP/1.1</a-radio-button>
                    <a-radio-button value="http2">HTTP/2</a-radio-button>
                  </a-radio-group>
                </div>
                <div class="send-opt-hint">
                  自动按 ALPN 协商；选 HTTP/2 时明文 <code>http://</code> 会被拒绝（h2c 需要预置知识，暂不支持）。
                </div>
                <div class="send-opt-row">
                  <span class="send-opt-label">重定向</span>
                  <a-checkbox v-model:checked="followRedirects">
                    跟随（最多 10 跳）
                  </a-checkbox>
                </div>
                <div class="send-opt-row">
                  <span class="send-opt-label">失败重试</span>
                  <a-checkbox v-model:checked="retryEnabled">启用</a-checkbox>
                </div>
                <template v-if="retryEnabled">
                  <div class="send-opt-row">
                    <span class="send-opt-label">最多尝试</span>
                    <a-input-number
                      v-model:value="retryMaxAttempts"
                      :min="1"
                      :max="4"
                      size="small"
                      style="width: 90px"
                    />
                    <span class="send-opt-hint inline">次（含首次）</span>
                  </div>
                  <div class="send-opt-row">
                    <a-checkbox v-model:checked="retryAllowNonIdempotent">
                      允许对非幂等方法重试
                    </a-checkbox>
                  </div>
                  <div v-if="retryNeedsConfirm && !retryAllowNonIdempotent" class="send-opt-warn">
                    {{ form.method.toUpperCase() }} 不是幂等方法，未勾选时不会重试（避免重复下单/重复扣款）。
                  </div>
                  <div class="send-opt-hint">
                    仅对 408 / 429 / 5xx 重试，退避 200 → 400 → 800 ms，服务端给 <code>Retry-After</code> 时按它等待（上限 10s）。
                  </div>
                </template>
              </div>
            </template>
            <a-button size="small" class="send-opt-btn">
              <template #icon><SettingOutlined /></template>
              发送选项
              <span v-if="protocol !== 'auto' || !followRedirects || retryEnabled" class="send-opt-dot" />
            </a-button>
          </a-popover>
          <a-button v-if="form.id" type="text" size="small" danger class="del-btn" @click="deleteRequest">
            <template #icon><DeleteOutlined /></template>
            删除
          </a-button>
          <a-dropdown :trigger="['click']" placement="bottomRight">
            <a-button>
              <template #icon><AiSparkIcon /></template>
              AI 分析
              <DownOutlined class="ai-caret" />
            </a-button>
            <template #overlay>
              <a-menu @click="(info: any) => onAiMenu(String(info.key))">
                <a-menu-item key="explain">解释这次请求 / 响应</a-menu-item>
                <a-menu-item key="assert">生成断言脚本</a-menu-item>
                <a-menu-item key="post">生成后置提取脚本</a-menu-item>
                <a-menu-item key="optimize">优化这个接口定义</a-menu-item>
                <a-menu-divider />
                <a-menu-item key="handoff">带这次请求去航仔</a-menu-item>
              </a-menu>
            </template>
          </a-dropdown>
          <a-button @click="openCookies">Cookie</a-button>
          <a-button @click="openCode">
            <template #icon><CodeOutlined /></template>
            代码
          </a-button>
          <a-button :loading="saving" @click="save()">
            <template #icon><SaveOutlined /></template>
            保存
          </a-button>
          <a-button class="stress-btn" @click="openStress">
            <template #icon><ThunderboltOutlined /></template>
            压测
          </a-button>
        </div>
      </div>

      <div class="tabs-bar">
        <div ref="tabsScrollEl" class="tabs-scroll">
          <div
            v-for="tab in tabStore.tabs"
            :key="tab.id"
            class="req-tab"
            :class="{ active: tab.id === requestId }"
            :title="tab.name"
            @click="switchTab(tab.id)"
            @auxclick.middle.prevent="closeTab(tab.id)"
          >
            <span class="tab-method" :class="`m-${tab.method}`">{{ tab.method }}</span>
            <span class="tab-name">{{ tab.name || '未命名请求' }}</span>
            <span class="tab-close" @click.stop="closeTab(tab.id)">
              <CloseOutlined />
            </span>
          </div>
        </div>
        <div class="tabs-tools">
          <a-select
            v-if="quickOptions.length"
            v-model:value="quickValue"
            class="quick-open"
            size="small"
            show-search
            placeholder="打开其他接口"
            :options="
              quickOptions
                .filter((item) => item.id !== requestId)
                .map((item) => ({ value: item.id, label: `${item.method} ${item.name}` }))
            "
            :filter-option="
              (input: string, option: any) =>
                String(option?.label ?? '')
                  .toLowerCase()
                  .includes(input.toLowerCase())
            "
            @change="quickOpen"
          />
          <span class="tabs-count">{{ tabStore.tabs.length }} / {{ MAX_REQUEST_TABS }}</span>
          <a-button v-if="tabStore.tabs.length > 1" type="text" size="small" @click="closeAllTabs">
            关闭全部
          </a-button>
        </div>
      </div>

      <div class="url-bar">
        <div class="request-bar">
          <a-select v-model:value="form.method" class="method-select" :class="`m-${form.method}`" :bordered="false">
            <a-select-option v-for="m in methods" :key="m" :value="m">{{ m }}</a-select-option>
          </a-select>
          <span class="bar-divider" />
          <a-input
            v-model:value="form.url"
            class="url-input mono"
            :bordered="false"
            placeholder="https://api.example.com/users 或 {{base_url}}/users"
            @press-enter="send"
          />
          <span class="bar-divider" />
          <a-select
            v-model:value="envId"
            class="env-select"
            :bordered="false"
            placeholder="不使用环境"
            allow-clear
          >
            <a-select-option v-for="env in store.environments" :key="env.id" :value="env.id">
              {{ env.name }}
            </a-select-option>
          </a-select>
          <a-tooltip :title="projectId ? '管理环境变量（增删变量、环境脚本）' : '当前接口未关联项目，无法管理环境'">
            <a-button
              type="text"
              size="small"
              class="env-gear"
              :disabled="!projectId"
              @click="envDrawerOpen = true"
            >
              <template #icon><SettingOutlined /></template>
            </a-button>
          </a-tooltip>
          <a-button type="primary" class="send-btn" :loading="sending" @click="send">
            <template #icon><SendOutlined /></template>
            发送
          </a-button>
          <a-tooltip v-if="sending" title="停止本次请求（连接、等待或下载阶段都可以中断）">
            <a-button danger class="stop-btn" :loading="cancelling" @click="cancelSend">
              停止
            </a-button>
          </a-tooltip>
        </div>
        <span class="url-tip">Enter 发送 · 支持环境变量</span>
      </div>

      <div class="panes">
        <div class="req-pane fm-card">
          <div class="pane-head">
            <span class="pane-title">
              <span class="pane-dot req" />
              Request
            </span>
            <span class="pane-meta mono">
              {{ form.method }} · {{ enabledParams }} Params · {{ enabledHeaders }} Headers
            </span>
          </div>
          <a-tabs v-model:active-key="activeTab" size="small" class="tabs">
            <a-tab-pane key="params">
              <template #tab>
                Params <span class="tab-count">{{ enabledParams }}</span>
              </template>
              <div class="tab-body">
                <KeyValueEditor v-model="form.params" key-placeholder="参数名" value-placeholder="参数值" />
              </div>
            </a-tab-pane>
            <a-tab-pane key="headers">
              <template #tab>
                Headers <span class="tab-count">{{ enabledHeaders }}</span>
              </template>
              <div class="tab-body">
                <div class="header-tools">
                  <a-select
                    class="preset-select"
                    size="small"
                    placeholder="插入常用请求头"
                    :value="undefined"
                    @change="(v: number) => applyHeaderPreset(v)"
                  >
                    <a-select-opt-group
                      v-for="group in headerGroups"
                      :key="group.label"
                      :label="group.label"
                    >
                      <a-select-option
                        v-for="preset in group.items"
                        :key="preset.id"
                        :value="preset.id"
                      >
                        {{ preset.key }}: {{ preset.value }}
                      </a-select-option>
                    </a-select-opt-group>
                  </a-select>
                  <span class="header-hint">选择后自动追加，已存在的同名头会覆盖值</span>
                </div>
                <KeyValueEditor v-model="form.headers" key-placeholder="Header 名" value-placeholder="Header 值" />
              </div>
            </a-tab-pane>
            <a-tab-pane key="auth">
              <template #tab>
                Auth <span class="tab-count" :class="{ off: !authActive }">{{ authActive ? authKind : '无' }}</span>
              </template>
              <div class="tab-body auth-tab">
                <a-radio-group v-model:value="authKind" size="small" button-style="solid">
                  <a-radio-button v-for="item in authKinds" :key="item.value" :value="item.value">
                    {{ item.label }}
                  </a-radio-button>
                </a-radio-group>

                <div v-if="authKind === 'bearer'" class="auth-form">
                  <a-input-password
                    v-model:value="form.auth!.token"
                    placeholder="Token，支持环境变量引用"
                    allow-clear
                  />
                  <span class="header-hint">发送时自动添加请求头 Authorization: Bearer &lt;token&gt;</span>
                </div>

                <div v-else-if="authKind === 'basic'" class="auth-form">
                  <a-input v-model:value="form.auth!.username" placeholder="用户名" allow-clear />
                  <a-input-password v-model:value="form.auth!.password" placeholder="密码" allow-clear />
                  <span class="header-hint">发送时自动生成 Basic 认证头（用户名:密码 的 Base64）</span>
                </div>

                <div v-else-if="authKind === 'apikey'" class="auth-form">
                  <a-input v-model:value="form.auth!.key" placeholder="Key（如 X-API-Key）" allow-clear />
                  <a-input-password v-model:value="form.auth!.value" placeholder="Value" allow-clear />
                  <a-radio-group v-model:value="form.auth!.addTo" size="small">
                    <a-radio value="header">添加到 Header</a-radio>
                    <a-radio value="query">添加到 Query 参数</a-radio>
                  </a-radio-group>
                </div>

                <div v-else-if="authKind === 'oauth2'" class="auth-form">
                  <a-input
                    v-model:value="form.auth!.tokenUrl"
                    placeholder="Token URL，如 https://auth.example.com/oauth/token"
                    allow-clear
                  />
                  <a-input v-model:value="form.auth!.clientId" placeholder="Client ID" allow-clear />
                  <a-input-password
                    v-model:value="form.auth!.clientSecret"
                    placeholder="Client Secret"
                    allow-clear
                  />
                  <a-input v-model:value="form.auth!.scope" placeholder="Scope（可选，空格分隔）" allow-clear />
                  <div class="oauth-row">
                    <a-button size="small" :loading="oauthLoading" @click="fetchOauthToken">
                      获取令牌
                    </a-button>
                    <span class="header-hint">
                      {{ form.auth!.token ? '已获取令牌，发送时自动带 Authorization: Bearer' : 'Client Credentials 模式，令牌仅保存在本机' }}
                    </span>
                  </div>
                </div>

                <div v-else class="empty-hint">该请求未使用认证，可直接在 Headers 中手动添加 Authorization</div>

                <div class="auth-tip">若 Headers 中已手动填写 Authorization，则以手动填写的内容为准。</div>
              </div>
            </a-tab-pane>
            <a-tab-pane key="body">
              <template #tab>
                Body <span class="tab-count" :class="{ off: form.bodyType === 'none' }">{{ form.bodyType }}</span>
              </template>
              <div class="tab-body body-tab">
                <div class="body-head">
                  <a-radio-group v-model:value="form.bodyType" size="small" button-style="solid">
                    <a-radio-button v-for="t in bodyTypes" :key="t.value" :value="t.value">
                      {{ t.label }}
                    </a-radio-button>
                  </a-radio-group>
                  <a-button
                    v-if="form.bodyType === 'json'"
                    size="small"
                    type="link"
                    :disabled="!form.body.trim()"
                    @click="formatBody"
                  >
                    格式化 JSON
                  </a-button>
                  <span v-if="form.body" class="body-size mono">{{ fmtSize(bodySize) }}</span>
                </div>

                <div v-if="form.bodyType === 'multipart'" class="multipart">
                  <div class="multipart-head">
                    <span>字段</span>
                    <span>类型</span>
                    <span>值 / 文件</span>
                    <span />
                    <a-button size="small" @click="addMultipartField('text')">
                      <template #icon><PlusOutlined /></template>
                      文本字段
                    </a-button>
                    <a-button size="small" @click="addMultipartField('file')">
                      <template #icon><PlusOutlined /></template>
                      文件字段
                    </a-button>
                  </div>
                  <div v-for="(field, index) in multipartFields" :key="index" class="multipart-row">
                    <a-checkbox v-model:checked="field.enabled" />
                    <a-input v-model:value="field.key" size="small" placeholder="字段名" />
                    <a-select v-model:value="field.kind" size="small" class="multipart-kind">
                      <a-select-option value="text">文本</a-select-option>
                      <a-select-option value="file">文件</a-select-option>
                    </a-select>
                    <template v-if="field.kind === 'file'">
                      <a-input v-model:value="field.value" size="small" placeholder="文件路径" readonly />
                      <a-button size="small" @click="pickFileForField(index)">选择</a-button>
                    </template>
                    <a-input v-else v-model:value="field.value" size="small" placeholder="字段值" />
                    <a-button size="small" danger type="text" @click="removeMultipartField(index)">
                      <template #icon><DeleteOutlined /></template>
                    </a-button>
                  </div>
                  <div v-if="!multipartFields.length" class="empty-hint">还没有字段，点击右上角添加</div>
                  <div class="header-hint">Content-Type 会自动设为 multipart/form-data（含 boundary），无需手动填写</div>
                </div>

                <div v-else-if="form.bodyType === 'binary'" class="binary">
                  <a-input v-model:value="binaryPath" placeholder="选择要上传的文件" readonly />
                  <a-button @click="pickBinaryFile">选择文件</a-button>
                  <span class="header-hint">默认以 application/octet-stream 发送，可在 Headers 中覆盖 Content-Type</span>
                </div>

                <div v-else-if="form.bodyType === 'graphql'" class="graphql">
                  <div class="script-label">Query</div>
                  <CodeArea v-model="gqlQuery" fill :rows="9" placeholder="query { user(id: 1) { id name } }" />
                  <div class="script-label">Variables（JSON 对象，可留空）</div>
                  <CodeArea v-model="gqlVarsText" fill :rows="5" placeholder='{"id": 1}' />
                  <div v-if="gqlVarsError" class="gql-error">{{ gqlVarsError }}</div>
                </div>

                <div v-else-if="form.bodyType === 'form'" class="form-fields">
                  <KeyValueEditor
                    v-model="formFields"
                    :show-check="false"
                    key-placeholder="字段名"
                    value-placeholder="字段值"
                  />
                  <div class="header-hint" v-pre>
                    发送时自动按 application/x-www-form-urlencoded 编码，{{变量}} 保持原样
                  </div>
                </div>

                <CodeArea
                  v-else-if="form.bodyType !== 'none'"
                  v-model="form.body"
                  fill
                  :rows="14"
                  :commentable="form.bodyType === 'json'"
                  :language="form.bodyType === 'json' ? 'json' : 'plain'"
                  placeholder='{"name":"fink"}'
                />
                <div v-else class="empty-hint">该请求没有 Body</div>
              </div>
            </a-tab-pane>
            <a-tab-pane key="script" tab="脚本">
              <div class="tab-body">
                <div class="script-label script-label-row">
                  <span>前置脚本（发送前执行，可生成/修改请求参数）</span>
                  <a-select
                    v-model:value="preExample"
                    :options="preExampleOptions"
                    size="small"
                    class="script-example"
                    placeholder="插入示例"
                    @change="insertExample('pre', $event)"
                  />
                </div>
                <a-textarea
                  v-model:value="form.preScript"
                  class="mono script-input"
                  :rows="7"
                  placeholder="// 例如&#10;request.body = JSON.stringify({ id: Date.now(), name: 'fink' })&#10;vars.set('traceId', 'req-' + Date.now())"
                />
                <div class="script-label script-label-row">
                  <span>后置脚本 / 测试断言（收到响应后执行）</span>
                  <a-select
                    v-model:value="postExample"
                    :options="postExampleOptions"
                    size="small"
                    class="script-example"
                    placeholder="插入示例"
                    @change="insertExample('post', $event)"
                  />
                </div>
                <a-textarea
                  v-model:value="form.postScript"
                  class="mono script-input"
                  :rows="7"
                  placeholder="// 例如&#10;test('状态码为 200', () => assert(response.status === 200))"
                />
                <div class="script-hint">
                  可用：<code>request</code>（method / url / headers / params / body）、
                  <code>response</code>（status / headers / body / json()）、
                  <code>vars</code>（get / set / unset）、<code>console</code>、
                  <code>rsa.publicEncrypt(明文, 公钥)</code>（PKCS#1 v1.5，返回 Base64，等价 Java 的 RSAUtils.publicEncrypt）、
                  <code>cipher</code>（aes / des / tripleDes 加解密，md5 / sha1 / sha256、hmacSha1 / hmacSha256、base64 / hex 编解码）；后置脚本另有
                  <code>test(name, fn)</code>、<code>assert(条件, 提示)</code>。变量可用 <code v-pre>{{name}}</code> 写入 URL、Header 或 Body。
                </div>
              </div>
            </a-tab-pane>
          </a-tabs>
        </div>

        <div class="res-pane fm-card">
          <div class="res-head">
            <span class="pane-title">
              <span class="pane-dot res" />
              Response
            </span>
            <a-tooltip v-if="result && resultSource === 'run'" :title="runSourceHint">
              <span class="proto-pill run-source mono">
                来自运行记录<template v-if="restoredRun"> · {{ fmtAgo(restoredRun.createdAt) }}</template>
                <template v-if="restoredRun?.responseBodyTruncated"> · 仅前 256KB</template>
              </span>
            </a-tooltip>
            <template v-if="result">
              <span class="status-pill mono" :class="{ ok: result.status > 0 && result.status < 400, bad: result.status >= 400 || !result.status }">
                {{ result.status || 'ERR' }}{{ result.statusText ? ` ${result.statusText}` : '' }}
              </span>
              <span class="meta-divider" />
              <span class="meta mono">{{ result.durationMs }} ms</span>
              <span class="meta-divider" />
              <span class="meta mono">{{ fmtSize(result.sizeBytes) }}</span>
              <a-tooltip v-if="result.httpVersion" :title="`实际协商协议：${result.httpVersion}`">
                <span class="proto-pill mono">{{ result.httpVersion }}</span>
              </a-tooltip>
              <a-tooltip
                v-if="result.cancelled"
                title="本次请求被手动停止，数据可能不完整"
              >
                <span class="proto-pill warn mono">已停止</span>
              </a-tooltip>
              <a-tooltip
                v-else-if="result.retries.length"
                :title="`共发生 ${result.retries.length} 次重试`"
              >
                <span class="proto-pill retry mono">重试 ×{{ result.retries.length }}</span>
              </a-tooltip>
              <a-tooltip
                v-if="result.redirects.length"
                :title="`跟随了 ${result.redirects.length} 跳重定向`"
              >
                <span class="proto-pill mono">跳转 ×{{ result.redirects.length }}</span>
              </a-tooltip>
            </template>
            <span
              v-if="scriptAssertions.length"
              class="assert-pill mono"
              :class="{ bad: failedAssertions > 0 }"
            >
              {{ failedAssertions > 0 ? '✗' : '✓' }}
              断言 {{ passedAssertions }}/{{ scriptAssertions.length }}
            </span>
            <span class="res-spacer" />
            <template v-if="result && !result.error">
              <a-radio-group
                v-if="!isBinaryBody"
                v-model:value="bodyView"
                size="small"
                button-style="solid"
                class="view-toggle"
              >
                <a-radio-button value="pretty">Pretty</a-radio-button>
                <a-radio-button value="raw">Raw</a-radio-button>
              </a-radio-group>
              <a-button v-if="!isBinaryBody" size="small" type="text" @click="copyBody">复制</a-button>
              <a-button size="small" type="text" @click="isBinaryBody ? saveBinaryBody() : saveResponseBody()">
                另存为
              </a-button>
              <a-button size="small" type="text" :disabled="!runs.length && !result.body" @click="openDiff">对比</a-button>
            </template>
          </div>

          <div v-if="!result" class="res-empty">
            <div class="res-empty-icon">⇥</div>
            <div class="res-empty-title">准备就绪</div>
            <div class="res-empty-desc">填写 URL 后点「发送」或按 Enter 查看响应结果</div>
          </div>
          <template v-else>
            <a-alert v-if="result.error" type="error" :message="result.error" show-icon style="margin: 12px 14px" />
            <a-tabs v-model:active-key="responseTab" size="small" class="tabs res-tabs">
              <a-tab-pane key="body">
                <template #tab>
                  Body
                  <span class="tab-count">{{
                    isBinaryBody
                      ? fmtSize(result.bodyBytes || result.sizeBytes)
                      : prettyLineCount
                        ? `${prettyLineCount} 行`
                        : '0'
                  }}</span>
                </template>

                <!-- 二进制 / 超大响应：已落盘，不塞进文本框 -->
                <div v-if="isBinaryBody" class="binary-body">
                  <div v-if="isImageBody && bodyPreviewSrc" class="binary-preview">
                    <img :src="bodyPreviewSrc" :alt="`响应图片 ${result.status}`" />
                  </div>
                  <div v-else class="binary-icon">{{ isPdfBody ? 'PDF' : 'BIN' }}</div>
                  <div class="binary-meta">
                    <div class="binary-title">
                      {{ isPdfBody ? 'PDF 文档' : isImageBody ? '图片' : '二进制响应' }}
                      <span class="mono binary-size">{{ fmtSize(result.bodyBytes || result.sizeBytes) }}</span>
                    </div>
                    <div class="binary-desc">
                      <template v-if="resultSource === 'run'">
                        运行记录只保存文本正文，二进制内容无法从历史恢复；请重新发送后再查看或另存。
                      </template>
                      <template v-else>
                        该响应不是文本内容（{{ bodyKind === 'binary' ? '二进制' : bodyKind }}），
                        已完整保存到本机，不载入编辑器以避免卡死。点「另存为」导出到你需要的位置。
                      </template>
                    </div>
                    <div v-if="result.rawBytes && result.rawBytes !== result.bodyBytes" class="binary-desc">
                      压缩传输：{{ fmtSize(result.rawBytes) }} → 解压后 {{ fmtSize(result.bodyBytes) }}
                    </div>
                    <div v-if="bodyAbsPath" class="binary-path mono" :title="bodyAbsPath">
                      {{ bodyAbsPath }}
                    </div>
                    <div v-else-if="bodyPreviewError" class="binary-desc bad">
                      {{ bodyPreviewError }}
                    </div>
                    <div class="binary-actions">
                      <a-button size="small" type="primary" @click="saveBinaryBody">另存为</a-button>
                      <a-button size="small" :disabled="!bodyAbsPath" @click="openBodyFolder">
                        打开所在目录
                      </a-button>
                      <a-button size="small" @click="copyBodyPath">复制路径</a-button>
                    </div>
                  </div>
                </div>

                <template v-else>
                  <div v-if="result.bodyOmitted" class="truncate-hint">
                    响应较大（{{ fmtSize(result.sizeBytes) }}），切换页面时未在内存保留正文；请重新发送或查看运行记录
                  </div>
                  <div v-else-if="result.bodyFile" class="truncate-hint">
                    完整响应（{{ fmtSize(result.bodyBytes || result.sizeBytes) }}）已落盘到本机，编辑器只加载前 5MB；
                    点「另存为」导出的是完整文件。
                  </div>
                  <div v-else-if="result.truncated" class="truncate-hint">响应体超过 5MB，已截断显示</div>
                  <div class="res-body-wrap">
                    <pre ref="bodyPreEl" class="mono body-pre">{{ bodyView === 'pretty' ? prettyBody : result.body }}</pre>
                  </div>
                </template>
              </a-tab-pane>
              <a-tab-pane key="headers">
                <template #tab>
                  Headers <span class="tab-count">{{ responseHeaders.length }}</span>
                </template>
                <div class="res-header-list">
                  <div v-for="h in responseHeaders" :key="h.key" class="res-header-row">
                    <span class="mono hk">{{ h.key }}</span>
                    <span class="mono hv">{{ h.value }}</span>
                  </div>
                  <div v-if="result.ignoredHeaders.length" class="ignored-headers">
                    <div class="ignored-title">因 HTTP/2 被剔除的请求头</div>
                    <div class="ignored-list mono">{{ result.ignoredHeaders.join('、') }}</div>
                    <div class="ignored-hint">
                      HTTP/2 禁止这些逐跳头（connection / keep-alive / transfer-encoding 等），
                      发送时已自动忽略；如果你需要它们生效，把协议切到 HTTP/1.1。
                    </div>
                  </div>
                </div>
              </a-tab-pane>

              <a-tab-pane key="timing">
                <template #tab>
                  计时
                  <span class="tab-count">{{ Math.round(result.durationMs) }}ms</span>
                </template>
                <div class="timing-panel">
                  <div v-if="!timings" class="empty-hint">
                    本次请求没有采集到耗时分解（可能在使用直连或旧版本记录）。
                  </div>
                  <template v-else>
                    <div class="timing-total">
                      <span class="timing-total-label">总耗时</span>
                      <span class="timing-total-value mono">{{ Math.round(timings.totalMs) }} ms</span>
                    </div>
                    <div class="waterfall">
                      <div v-for="row in timingRows" :key="row.key" class="wf-row">
                        <span class="wf-label">{{ row.label }}</span>
                        <div class="wf-track">
                          <div
                            class="wf-bar"
                            :style="{
                              left: `${row.offset}%`,
                              width: `${row.width}%`,
                              background: row.color,
                            }"
                            :title="`${row.label}：${row.ms.toFixed(1)} ms（${row.hint}）`"
                          />
                        </div>
                        <span class="wf-value mono">{{ row.ms.toFixed(1) }} ms</span>
                      </div>
                    </div>
                    <div v-if="timingNotes.length" class="timing-notes">
                      <div v-for="(note, index) in timingNotes" :key="index" class="timing-note">
                        · {{ note }}
                      </div>
                    </div>

                    <div v-if="result.redirects.length" class="timing-block">
                      <div class="timing-block-title">重定向链（{{ result.redirects.length }} 跳）</div>
                      <div v-for="(hop, index) in result.redirects" :key="index" class="hop-row">
                        <span class="hop-index mono">{{ index + 1 }}</span>
                        <span class="hop-status mono" :class="{ bad: hop.status >= 400 }">{{ hop.status }}</span>
                        <span class="hop-host mono">{{ hop.host }}</span>
                        <span class="hop-loc mono" :title="hop.location">{{ hop.location }}</span>
                        <span class="hop-ms mono">{{ hop.durationMs.toFixed(1) }} ms</span>
                      </div>
                    </div>

                    <div v-if="result.retries.length" class="timing-block">
                      <div class="timing-block-title">重试记录（{{ result.retries.length }} 次）</div>
                      <div v-for="(item, index) in result.retries" :key="index" class="hop-row">
                        <span class="hop-index mono">#{{ item.attempt }}</span>
                        <span class="hop-status mono" :class="{ bad: item.status >= 400 || !item.status }">
                          {{ item.status || 'ERR' }}
                        </span>
                        <span class="hop-loc mono" :title="item.error || ''">{{ item.error || '按状态码重试' }}</span>
                        <span class="hop-ms mono">等待 {{ item.delayMs }} ms</span>
                      </div>
                    </div>
                  </template>
                </div>
              </a-tab-pane>
              <a-tab-pane key="tests">
                <template #tab>
                  测试
                  <span
                    class="tab-count"
                    :class="{ bad: failedAssertions > 0, ok: scriptAssertions.length > 0 && failedAssertions === 0 }"
                  >
                    {{ scriptAssertions.length ? `${passedAssertions}/${scriptAssertions.length}` : '0' }}
                  </span>
                </template>
                <div class="test-panel">
                  <a-alert
                    v-if="scriptError"
                    type="error"
                    show-icon
                    class="test-alert"
                  >
                    <template #message>脚本执行出错</template>
                    <template #description>
                      <pre class="script-error-pre">{{ scriptError }}</pre>
                    </template>
                  </a-alert>
                  <div v-if="scriptAssertions.length" class="test-list">
                    <div
                      v-for="(item, index) in scriptAssertions"
                      :key="`${index}-${item.name}`"
                      class="test-row"
                      :class="{ failed: !item.passed }"
                    >
                      <span class="test-mark">{{ item.passed ? '✓' : '✗' }}</span>
                      <span class="test-name">{{ item.name }}</span>
                      <span v-if="item.message" class="test-msg mono">{{ item.message }}</span>
                    </div>
                  </div>
                  <div v-else-if="!scriptError" class="empty-hint">
                    后置脚本没有产生断言。可在「脚本」页的 后置脚本 中用 test() 添加。
                  </div>
                  <div v-if="scriptLogs.length" class="test-block">
                    <div class="test-section">控制台输出</div>
                    <div v-for="(log, index) in scriptLogs" :key="`log-${index}`" class="log-row" :class="log.level">
                      <span class="log-phase">{{
                        log.phase === 'env' ? '环境' : log.phase === 'pre' ? '前置' : '后置'
                      }}</span>
                      <span class="log-text mono">{{ log.text }}</span>
                    </div>
                  </div>
                  <div v-if="scriptVarsList.length" class="test-block">
                    <div class="test-section">
                      运行时变量
                      <a class="vars-clear" @click="resetScriptVars">清空</a>
                    </div>
                    <div v-for="item in scriptVarsList" :key="item.key" class="var-row">
                      <span class="var-key mono">{{ item.key }}</span>
                      <span class="var-value mono">{{ item.value }}</span>
                    </div>
                  </div>
                  <div v-if="scriptMs" class="test-time">脚本耗时 {{ scriptMs }} ms</div>
                </div>
              </a-tab-pane>
            </a-tabs>
          </template>
        </div>
      </div>

      <section ref="runsPanelEl" class="runs-panel" :class="{ collapsed: !runsOpen }">
        <div class="runs-head">
          <button class="runs-toggle" @click="runsOpen = !runsOpen">
            <span class="run-caret" :class="{ open: runsOpen }">›</span>
            运行历史
            <span class="runs-count">{{ runs.length }}</span>
          </button>
          <span class="runs-hint">每次发送自动记录请求参数、响应与耗时，该接口保留最近 200 条</span>
          <div class="runs-actions">
            <a-spin v-if="runsLoading" size="small" />
            <a v-if="runs.length" class="runs-clear" @click="clearRuns">清空</a>
          </div>
        </div>

        <div v-if="runsOpen" class="runs-body">
          <div v-if="!runs.length" class="runs-empty">
            还没有运行记录，发送一次请求后会自动记录在这里
          </div>
          <template v-else>
            <div class="run-row run-row-head">
              <span>时间</span>
              <span>方法</span>
              <span>状态</span>
              <span>耗时</span>
              <span>大小</span>
              <span>URL</span>
              <span />
            </div>
            <template v-for="run in pagedRuns" :key="run.id">
              <div
                class="run-row"
                :class="{ active: expandedRunId === run.id }"
                @click="toggleRun(run)"
              >
                <span class="run-time mono">{{ fmtRunTime(run.createdAt) }}</span>
                <span class="run-method mono">{{ run.method }}</span>
                <span class="run-status mono" :style="{ color: runStatusColor(run.responseStatus) }">
                  {{ run.responseStatus || 'ERR' }}
                </span>
                <span class="run-dur mono">{{ run.durationMs }} ms</span>
                <span class="run-size mono">{{ fmtSize(run.sizeBytes) }}</span>
                <span class="run-url mono" :title="run.url">{{ run.url }}</span>
                <span class="run-caret" :class="{ open: expandedRunId === run.id }">›</span>
              </div>

              <div v-if="expandedRunId === run.id" class="run-detail">
                <div v-if="runDetailLoading.has(run.id)" class="runs-empty run-detail-loading">
                  正在加载运行详情…
                </div>
                <template v-else>
                  <div class="run-col">
                    <div class="run-col-title">请求参数</div>
                    <div class="run-line">
                      <span class="rk">URL</span>
                      <span class="rv mono">{{ run.url }}</span>
                    </div>
                    <div v-for="p in runParams(run)" :key="`p-${p.key}-${p.value}`" class="run-line">
                      <span class="rk">{{ p.key }}</span>
                      <span class="rv mono">{{ p.value }}</span>
                    </div>
                    <div v-for="h in runRequestHeaders(run)" :key="`h-${h.key}-${h.value}`" class="run-line">
                      <span class="rk">{{ h.key }}</span>
                      <span class="rv mono">{{ h.value }}</span>
                    </div>
                    <div v-if="runBodyText(run)" class="run-block">
                      <div class="run-col-title">
                        请求 Body · {{ run.bodyType }}
                        <span v-if="run.requestBodyTruncated" class="run-col-meta mono">
                          已截断 · 原 {{ fmtSize(run.requestBodySize) }}
                        </span>
                      </div>
                      <pre class="mono run-pre">{{ prettyRunBody(runBodyText(run)) }}</pre>
                    </div>
                    <div
                      v-else-if="!runHasExtraParams(run)"
                      class="runs-empty"
                    >
                      该次请求没有额外参数
                    </div>
                  </div>

                  <div class="run-col">
                    <div class="run-col-title">
                      响应
                      <span class="mono run-col-status" :style="{ color: runStatusColor(run.responseStatus) }">
                        {{ run.responseStatus || 'ERR' }} {{ run.responseStatusText }}
                      </span>
                      <span class="run-col-meta mono">
                        {{ run.durationMs }} ms · 响应体 {{ fmtSize(run.responseBodySize) }}
                      </span>
                    </div>
                    <div v-if="run.error" class="run-error">{{ run.error }}</div>
                    <template v-else>
                      <div v-for="[k, v] in runHeaders(run.id)" :key="k" class="run-line">
                        <span class="rk">{{ k }}</span>
                        <span class="rv mono">{{ v }}</span>
                      </div>
                      <pre v-if="runResponseText(run)" class="mono run-pre">{{ prettyRunBody(runResponseText(run)) }}</pre>
                      <div v-else class="runs-empty">响应体为空</div>
                      <div v-if="run.responseBodyTruncated" class="runs-empty">
                        响应体已截断存储，完整大小 {{ fmtSize(run.responseBodySize) }}
                      </div>
                    </template>
                  </div>
                </template>
              </div>
            </template>

            <div v-if="runs.length > RUNS_PAGE_SIZE" class="runs-pager">
              <span class="runs-pager-info">
                共 {{ runs.length }} 条 · 每页 {{ RUNS_PAGE_SIZE }} 条
              </span>
              <a-pagination
                size="small"
                :current="runsPage"
                :page-size="RUNS_PAGE_SIZE"
                :total="runs.length"
                :show-size-changer="false"
                @change="setRunsPage"
              />
            </div>
          </template>
        </div>
      </section>
    </div>

    <a-drawer
      v-model:open="codeOpen"
      placement="right"
      :width="580"
      title="代码片段"
      class="code-drawer"
    >
      <div class="code-hint">
        与 Postman 的 Code snippet 一致，可直接复制到终端、脚本或文档中；环境变量已按当前环境替换，敏感变量保持占位符。
      </div>
      <a-radio-group v-model:value="snippetKey" size="small" button-style="solid" class="code-tabs">
        <a-radio-button v-for="item in snippets" :key="item.key" :value="item.key">
          {{ item.label }}
        </a-radio-button>
      </a-radio-group>
      <div class="code-actions">
        <a-button size="small" :type="codeEditing ? 'default' : 'primary'" @click="copySnippet">
          <template #icon><CopyOutlined /></template>
          复制
        </a-button>
        <a-button v-if="!codeEditing" size="small" @click="startEditCode">
          <template #icon><EditOutlined /></template>
          编辑
        </a-button>
        <a-button v-else size="small" type="primary" @click="finishEditCode">
          <template #icon><CheckOutlined /></template>
          完成
        </a-button>
        <a-button v-if="isCodeEdited && !codeEditing" size="small" type="text" @click="resetCode">
          恢复生成内容
        </a-button>
        <span
          v-if="isCodeEdited"
          class="code-edited-badge"
          title="已手动修改，不再随当前请求自动更新；复制取当前文本"
        >
          已修改
        </span>
        <span class="code-file mono">{{ form.name || 'request' }} · {{ activeSnippet.language }}</span>
      </div>
      <textarea
        v-if="codeEditing"
        ref="codeAreaRef"
        v-model="codeEditText"
        class="code-block code-textarea mono"
        spellcheck="false"
      ></textarea>
      <pre v-else class="code-block mono">{{ activeCode }}</pre>
    </a-drawer>

    <a-modal
      v-model:open="cookieOpen"
      title="本机 Cookie"
      :width="760"
      :footer="null"
    >
      <div class="cookie-hint">
        响应中的 Set-Cookie 会自动保存到本机，发送请求时按域名与路径自动携带；也可手动添加用于登录态调试。
      </div>
      <div class="cookie-actions">
        <a-button size="small" :loading="cookieLoading" @click="loadCookies">刷新</a-button>
        <a-button size="small" danger @click="clearCookies">清空全部</a-button>
      </div>
      <a-table
        :data-source="cookies"
        :pagination="false"
        size="small"
        row-key="id"
        :scroll="{ y: 360 }"
        :locale="{ emptyText: '暂无 Cookie' }"
      >
        <a-table-column title="域名" data-index="domain" key="domain" ellipsis />
        <a-table-column title="路径" data-index="path" key="path" width="100" />
        <a-table-column title="名称" data-index="name" key="name" ellipsis />
        <a-table-column title="值" data-index="value" key="value" ellipsis />
        <a-table-column title="过期时间" key="expiresAt" width="170">
          <template #default="{ record }">
            <span class="mono">{{ record.expiresAt ? fmtTimeOf(record.expiresAt) : '会话' }}</span>
          </template>
        </a-table-column>
        <a-table-column title="操作" key="action" width="80">
          <template #default="{ record }">
            <a-button type="link" size="small" danger @click="removeCookie(record.id)">删除</a-button>
          </template>
        </a-table-column>
      </a-table>
    </a-modal>

    <a-modal
      v-model:open="diffOpen"
      title="响应对比"
      :width="980"
      :footer="null"
      class="diff-modal"
    >
      <div class="diff-toolbar">
        <a-radio-group v-model:value="diffUseCustom" size="small" button-style="solid">
          <a-radio-button :value="false" :disabled="!runs.length">对比历史运行</a-radio-button>
          <a-radio-button :value="true">对比自定义文本</a-radio-button>
        </a-radio-group>
        <a-select
          v-if="!diffUseCustom"
          v-model:value="diffBaseRunId"
          :options="diffRunOptions"
          size="small"
          class="diff-select"
          placeholder="选择基准运行"
        />
        <span class="diff-stat">
          <span class="add">+{{ diffSummary.added }}</span>
          <span class="del">-{{ diffSummary.removed }}</span>
        </span>
      </div>
      <a-textarea
        v-if="diffUseCustom"
        v-model:value="diffCustomText"
        class="mono diff-input"
        :rows="5"
        placeholder="粘贴基准响应体，与当前响应逐行对比"
      />
      <div class="diff-view">
        <pre v-if="diffResult.length" class="mono diff-pre"><span
          v-for="(line, index) in diffResult"
          :key="index"
          class="diff-line"
          :class="line.type"
        >{{ line.type === 'add' ? '+' : line.type === 'del' ? '-' : ' ' }} {{ line.text }}
</span></pre>
        <div v-else class="diff-empty">暂无可对比内容</div>
      </div>
    </a-modal>

    <EnvironmentDrawer
      v-model:open="envDrawerOpen"
      :project-id="projectId"
      :active-id="envId"
      @select="envId = $event"
      @changed="onEnvsChanged"
    />
  </a-spin>
  </div>
</template>

<style scoped>
/* 整页共用一个滚动：外层 .main 已经是 overflow:auto，
   这里让页面至少一屏高、内容多则自然撑高，请求/响应区不再被压扁 */
.editor-root {
  height: 100%;
}

.editor-root :deep(.ant-spin-nested-loading),
.editor-root :deep(.ant-spin-container) {
  height: 100%;
}

.editor-page {
  min-height: 100%;
  display: flex;
  flex-direction: column;
}

.ai-caret {
  font-size: 10px;
  margin-left: 4px;
}

.topbar {
  height: 46px;
  padding: 0 14px;
  background: var(--fm-surface);
  border-bottom: 1px solid var(--fm-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.left,
.right {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.left {
  flex: 1;
  overflow: hidden;
}

.right {
  flex-shrink: 0;
}

.tabs-bar {
  display: flex;
  align-items: stretch;
  gap: 8px;
  height: 36px;
  padding: 0 10px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.tabs-scroll {
  display: flex;
  align-items: stretch;
  gap: 4px;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  padding-top: 5px;
}

.tabs-scroll::-webkit-scrollbar {
  height: 4px;
}

.tabs-scroll::-webkit-scrollbar-thumb {
  border: none;
  border-radius: 4px;
  background: var(--fm-border-strong);
}

.req-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  min-width: 118px;
  max-width: 220px;
  padding: 0 8px 0 10px;
  border: 1px solid transparent;
  border-bottom: none;
  border-radius: 7px 7px 0 0;
  color: var(--fm-muted);
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
  transition: background 0.15s ease, color 0.15s ease;
}

.req-tab:hover {
  background: rgba(100, 116, 139, 0.08);
  color: var(--fm-text-2);
}

.req-tab.active {
  background: var(--fm-surface);
  border-color: var(--fm-border);
  color: var(--fm-text);
  box-shadow: inset 0 1px 0 var(--fm-primary);
}

.tab-method {
  font-size: 9.5px;
  font-weight: 700;
  letter-spacing: 0.2px;
  flex-shrink: 0;
}

.tab-method.m-GET {
  color: var(--fm-success);
}

.tab-method.m-POST {
  color: var(--fm-warning);
}

.tab-method.m-PUT {
  color: var(--fm-primary);
}

.tab-method.m-PATCH {
  color: var(--fm-purple);
}

.tab-method.m-DELETE {
  color: var(--fm-danger);
}

.tab-method.m-HEAD,
.tab-method.m-OPTIONS {
  color: var(--fm-muted);
}

.tab-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tab-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  border-radius: 4px;
  font-size: 10px;
  color: var(--fm-muted);
}

.tab-close:hover {
  background: rgba(100, 116, 139, 0.18);
  color: var(--fm-danger);
}

.tabs-tools {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.tabs-count {
  font-size: 10px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.quick-open {
  width: 168px;
}

.back-btn {
  color: var(--fm-text-2);
}

.divider {
  width: 1px;
  height: 16px;
  background: var(--fm-border-strong);
}

.name-input {
  width: clamp(160px, 24vw, 380px);
  min-width: 120px;
  font-weight: 600;
  border-color: transparent;
  background: transparent;
  font-size: 13px;
  text-overflow: ellipsis;
}

.name-input:hover {
  border-color: var(--fm-border);
}

.crumb {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.crumb-project {
  font-size: 11.5px;
  color: var(--fm-muted);
  max-width: 150px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.crumb-sep {
  font-size: 11px;
  color: var(--fm-border-strong);
}

.saved-hint {
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.saved-hint.is-dirty {
  color: var(--fm-warning);
}

.saved-hint.is-saving {
  color: var(--fm-primary);
}

.saved-hint.is-error {
  color: var(--fm-danger);
}

.insecure {
  font-size: 12px;
  color: var(--fm-muted);
}

.stress-btn {
  color: var(--fm-warning);
  border-color: var(--fm-border-strong);
  background: var(--fm-warning-soft);
}

.stress-btn:hover {
  color: var(--fm-warning);
  border-color: var(--fm-warning);
  background: var(--fm-warning-soft);
}

.url-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: var(--fm-surface);
  border-bottom: 1px solid var(--fm-border);
}

.request-bar {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  height: 42px;
  padding: 3px 3px 3px 4px;
  border: 1px solid var(--fm-border-strong);
  border-radius: 9px;
  background: var(--fm-surface);
  transition: border-color 0.16s, box-shadow 0.16s;
}

.request-bar:focus-within {
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 3px var(--fm-primary-soft);
}

.method-select {
  width: 96px;
  flex-shrink: 0;
  font-weight: 700;
}

.method-select :deep(.ant-select-selector) {
  box-shadow: none !important;
  border-radius: 6px !important;
  transition: background-color 0.14s ease;
}

/* 方法色底：让 URL 输入条左侧的方法一眼可辨（Postman / Apifox 的做法） */
.method-select.m-GET :deep(.ant-select-selector) {
  background: var(--fm-method-get-soft, var(--fm-success-soft));
}
.method-select.m-POST :deep(.ant-select-selector) {
  background: var(--fm-method-post-soft, var(--fm-warning-soft));
}
.method-select.m-PUT :deep(.ant-select-selector) {
  background: var(--fm-method-put-soft, var(--fm-primary-soft));
}
.method-select.m-PATCH :deep(.ant-select-selector) {
  background: var(--fm-method-patch-soft, var(--fm-purple-soft));
}
.method-select.m-DELETE :deep(.ant-select-selector) {
  background: var(--fm-method-delete-soft, var(--fm-danger-soft));
}

.method-select.m-GET :deep(.ant-select-selection-item) {
  color: var(--fm-method-get, var(--fm-success));
}
.method-select.m-POST :deep(.ant-select-selection-item) {
  color: var(--fm-method-post, var(--fm-warning));
}
.method-select.m-PUT :deep(.ant-select-selection-item) {
  color: var(--fm-method-put, var(--fm-primary));
}
.method-select.m-PATCH :deep(.ant-select-selection-item) {
  color: var(--fm-method-patch, var(--fm-purple));
}
.method-select.m-DELETE :deep(.ant-select-selection-item) {
  color: var(--fm-method-delete, var(--fm-danger));
}

.bar-divider {
  width: 1px;
  height: 20px;
  flex-shrink: 0;
  background: var(--fm-border);
}

.url-input {
  flex: 1;
  min-width: 0;
  font-size: 13px;
}

.url-input :deep(.ant-input) {
  height: 36px;
}

.env-select {
  width: 150px;
  flex-shrink: 0;
  font-size: 12px;
}

.env-select :deep(.ant-select-selector) {
  box-shadow: none !important;
}

.env-gear {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  color: var(--fm-text-2);
}

.env-gear:hover {
  color: var(--fm-primary);
}

.send-btn {
  height: 34px;
  flex-shrink: 0;
  border-radius: 8px;
  font-weight: 600;
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--fm-primary) 80%, #ffffff) 0%,
    var(--fm-primary) 100%
  );
  border: none;
  box-shadow: 0 3px 10px color-mix(in srgb, var(--fm-primary) 32%, transparent);
  transition: filter 0.16s ease, box-shadow 0.16s ease, transform 0.1s ease;
}

.send-btn:not(:disabled):hover {
  filter: brightness(1.07);
  box-shadow: 0 5px 16px color-mix(in srgb, var(--fm-primary) 42%, transparent);
  transform: translateY(-1px);
}

.send-btn:not(:disabled):active {
  transform: translateY(0);
  filter: brightness(0.97);
}

.url-tip {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--fm-muted);
}

.panes {
  flex: 1 1 auto;
  /* 固定高度：请求 / 响应各自内部滚动，长 Body 不再把整页撑长 */
  height: clamp(380px, 58vh, 660px);
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
  padding: 12px 14px 14px;
  overflow: hidden;
}

.req-pane,
.res-pane {
  min-width: 0;
  min-height: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface);
  box-shadow: var(--fm-shadow-sm);
  transition: border-color 0.16s ease, box-shadow 0.16s ease;
}

.req-pane:focus-within,
.res-pane:hover {
  border-color: var(--fm-border-strong);
}

.pane-head {
  height: 40px;
  flex-shrink: 0;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.pane-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--fm-text-2);
}

.pane-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}

.pane-dot.req {
  background: var(--fm-primary);
  box-shadow: 0 0 0 3px var(--fm-primary-soft);
}

.pane-dot.res {
  background: var(--fm-success);
  box-shadow: 0 0 0 3px var(--fm-success-soft);
}

.pane-meta {
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tabs {
  padding: 0 14px 10px;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tabs :deep(.ant-tabs-nav) {
  margin-bottom: 8px;
}

.tabs :deep(.ant-tabs-content-holder) {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.tabs :deep(.ant-tabs-content),
.tabs :deep(.ant-tabs-tabpane) {
  height: 100%;
}

.tab-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 17px;
  height: 16px;
  padding: 0 5px;
  margin-left: 5px;
  border-radius: 5px;
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 9.5px;
  font-weight: 600;
  line-height: 1;
}

.tab-count.off {
  opacity: 0.6;
}

.header-tools {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.preset-select {
  width: 280px;
}

.header-hint {
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 认证 ---------- */
.auth-tab {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.auth-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 520px;
}

.oauth-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.auth-tip {
  font-size: 11px;
  color: var(--fm-muted);
}

/* ---------- multipart / binary / GraphQL ---------- */
.multipart {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 10px;
}

.multipart-head {
  display: grid;
  grid-template-columns: 20px 1fr 96px 1fr auto;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: var(--fm-muted);
}

.multipart-head > span:nth-child(4) {
  display: none;
}

.multipart-row {
  display: grid;
  grid-template-columns: 20px 1fr 96px 1fr auto;
  align-items: center;
  gap: 8px;
}

.multipart-row .ant-input {
  width: 100%;
}

.multipart-kind {
  width: 96px;
}

.binary {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 10px;
}

.graphql {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 10px;
}

.gql-error {
  font-size: 11px;
  color: var(--fm-danger);
}

.cookie-hint {
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.7;
  margin-bottom: 10px;
}

.cookie-actions {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}

/* ---------- 响应对比 ---------- */
.diff-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.diff-select {
  flex: 1;
  min-width: 200px;
}

.diff-stat {
  display: flex;
  gap: 8px;
  font-size: 11.5px;
  font-family: var(--fm-mono, ui-monospace, monospace);
}

.diff-stat .add {
  color: var(--fm-success);
}

.diff-stat .del {
  color: var(--fm-danger);
}

.diff-input {
  margin-bottom: 10px;
  font-size: 12px;
}

.diff-view {
  max-height: 460px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
}

.diff-pre {
  margin: 0;
  padding: 8px 0;
  font-size: 11.5px;
  line-height: 1.55;
}

.diff-line {
  display: block;
  padding: 0 10px;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

.diff-line.add {
  background: color-mix(in srgb, var(--fm-success) 14%, transparent);
  color: var(--fm-success);
}

.diff-line.del {
  background: color-mix(in srgb, var(--fm-danger) 14%, transparent);
  color: var(--fm-danger);
}

.diff-empty {
  padding: 22px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.tab-body {
  padding: 4px 0 8px;
}

.body-tab {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding-bottom: 0;
}

.body-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 6px 0 10px;
}

.body-size {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
}

.body-input {
  font-size: 12px;
  line-height: 1.7;
}

.script-label {
  font-size: 11px;
  color: var(--fm-muted);
  margin: 10px 0 6px;
}

.script-label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.script-example {
  width: 118px;
  flex: none;
}

.script-input {
  font-size: 11.5px;
  line-height: 1.65;
}

.script-hint {
  margin-top: 10px;
  padding: 9px 11px;
  border-radius: 7px;
  background: var(--fm-surface-alt);
  font-size: 10.5px;
  line-height: 1.8;
  color: var(--fm-muted);
}

.script-hint code {
  padding: 1px 4px;
  margin: 0 1px;
  border-radius: 4px;
  background: var(--fm-surface);
  color: var(--fm-text-2);
  font-size: 10px;
}

.assert-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-left: 10px;
  padding: 2px 8px;
  border-radius: 20px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
  font-size: 10px;
  font-weight: 600;
}

.assert-pill.bad {
  background: var(--fm-danger-soft, #fdeaea);
  color: var(--fm-danger);
}

.tab-count.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.tab-count.bad {
  background: var(--fm-danger-soft, #fdeaea);
  color: var(--fm-danger);
}

/* ---------- 脚本测试结果 ---------- */
.test-panel {
  height: 100%;
  box-sizing: border-box;
  padding: 6px 14px 14px;
  overflow: auto;
  font-size: 11.5px;
}

.test-alert {
  margin-bottom: 10px;
}

.script-error-pre {
  margin: 2px 0 0;
  max-height: 190px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: 'JetBrains Mono', 'Cascadia Code', 'SFMono-Regular', Consolas, monospace;
  font-size: 11px;
  line-height: 1.6;
}

.test-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.test-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 6px 9px;
  border-radius: 6px;
  background: var(--fm-success-soft);
  color: var(--fm-text-2);
}

.test-row.failed {
  background: var(--fm-danger-soft, #fdeaea);
}

.test-mark {
  width: 12px;
  flex-shrink: 0;
  color: var(--fm-success);
  font-weight: 700;
}

.test-row.failed .test-mark {
  color: var(--fm-danger);
}

.test-name {
  flex-shrink: 0;
}

.test-msg {
  margin-left: auto;
  max-width: 60%;
  color: var(--fm-danger);
  font-size: 10.5px;
  word-break: break-all;
  white-space: pre-wrap;
  text-align: right;
}

.test-block {
  margin-top: 14px;
}

.test-section {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 10.5px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.vars-clear {
  font-weight: 400;
  color: var(--fm-primary);
  font-size: 10px;
}

.log-row {
  display: flex;
  gap: 8px;
  padding: 4px 8px;
  border-radius: 5px;
  border-bottom: 1px dashed var(--fm-border);
}

.log-row.warn .log-text {
  color: var(--fm-warning);
}

.log-row.error .log-text {
  color: var(--fm-danger);
}

.log-phase {
  flex-shrink: 0;
  padding: 0 5px;
  height: 15px;
  line-height: 15px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 9.5px;
}

.log-text {
  flex: 1;
  min-width: 0;
  font-size: 10.5px;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

.var-row {
  display: flex;
  gap: 10px;
  padding: 5px 8px;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 10.5px;
}

.var-key {
  width: 150px;
  flex-shrink: 0;
  color: var(--fm-primary);
}

.var-value {
  flex: 1;
  min-width: 0;
  color: var(--fm-text-2);
  word-break: break-all;
}

.test-time {
  margin-top: 12px;
  font-size: 10px;
  color: var(--fm-muted);
}

.res-head {
  height: 40px;
  flex-shrink: 0;
  padding: 0 14px;
  display: flex;
  align-items: center;
  gap: 10px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
  overflow: hidden;
}

/* 指标与操作按钮必须保持单行；空间不够时只压缩来源标签 */
.res-head > * {
  flex-shrink: 0;
}

.res-head > .proto-pill.run-source {
  flex-shrink: 1;
  min-width: 0;
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.res-head .status-pill,
.res-head .meta,
.res-head .assert-pill,
.res-head .proto-pill {
  white-space: nowrap;
}

.status-pill {
  padding: 1px 8px;
  border-radius: 5px;
  font-size: 11.5px;
  font-weight: 700;
}

.status-pill.ok {
  color: var(--fm-success);
  background: var(--fm-success-soft);
}

.status-pill.bad {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
}

.res-spacer {
  flex: 1;
}

.view-toggle :deep(.ant-radio-button-wrapper) {
  font-size: 10.5px;
  padding: 0 9px;
}

.meta-divider {
  width: 1px;
  height: 12px;
  background: var(--fm-border-strong);
}

.meta {
  font-size: 11px;
  color: var(--fm-muted);
}

.res-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 20px;
}

.res-empty-icon {
  width: 48px;
  height: 48px;
  border-radius: 14px;
  background: var(--fm-surface-alt);
  color: #c2ccdb;
  font-size: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 12px;
}

.res-empty-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.res-empty-desc {
  margin-top: 4px;
  font-size: 11px;
  color: var(--fm-muted);
}

.res-tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.res-body-wrap {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.body-pre {
  flex: 1;
  min-height: 0;
  box-sizing: border-box;
  margin: 0;
  padding: 14px 16px;
  font-size: 12.5px;
  line-height: 1.75;
  white-space: pre-wrap;
  word-break: break-all;
  overflow: auto;
  color: var(--fm-text-2);
}

.truncate-hint {
  padding: 6px 14px;
  font-size: 11px;
  color: var(--fm-warning);
}

.res-header-list {
  height: 100%;
  box-sizing: border-box;
  padding: 4px 0 10px;
  overflow: auto;
}

.res-header-row {
  display: flex;
  gap: 14px;
  padding: 7px 14px;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 11px;
}

.hk {
  width: 200px;
  flex-shrink: 0;
  color: var(--fm-text-2);
}

.hv {
  flex: 1;
  word-break: break-all;
  color: var(--fm-muted);
}

/* ---------- 运行历史 ---------- */
.runs-panel {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
}

.runs-head {
  position: sticky;
  top: 0;
  z-index: 3;
  height: 34px;
  flex-shrink: 0;
  padding: 0 14px;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
  border-top: 1px solid var(--fm-border);
}

.runs-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: none;
  background: transparent;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  cursor: pointer;
}

.runs-count {
  min-width: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: var(--fm-border);
  color: var(--fm-text-2);
  font-size: 10px;
  line-height: 16px;
  text-align: center;
}

.runs-hint {
  flex: 1;
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.runs-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.runs-clear {
  font-size: 11px;
  color: var(--fm-danger);
}

/* 列表随页面滚动，不再有独立滚动条 */
.runs-body {
  flex: 1;
  padding-bottom: 6px;
}

.runs-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
  border-top: 1px solid var(--fm-border);
  background: var(--fm-surface);
}

.runs-pager-info {
  font-size: 11px;
  color: var(--fm-muted);
}

.runs-empty {
  padding: 14px;
  font-size: 11px;
  color: var(--fm-muted);
}

.run-row {
  display: grid;
  grid-template-columns: 132px 58px 52px 66px 64px minmax(0, 1fr) 18px;
  align-items: center;
  gap: 8px;
  padding: 5px 14px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11px;
  color: var(--fm-text-2);
  cursor: pointer;
}

.run-row:hover {
  background: var(--fm-surface-alt);
}

.run-row.active {
  background: var(--fm-primary-soft);
}

.run-row-head {
  position: sticky;
  top: 34px;
  z-index: 2;
  background: var(--fm-surface);
  color: var(--fm-muted);
  font-size: 10px;
  cursor: default;
  border-bottom: 1px solid var(--fm-border);
}

.run-row-head:hover {
  background: var(--fm-surface);
}

.run-time,
.run-dur,
.run-size {
  color: var(--fm-muted);
}

.run-method {
  font-weight: 700;
  color: var(--fm-primary);
}

.run-status {
  font-weight: 700;
}

.run-url {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-2);
}

.run-caret {
  color: var(--fm-muted);
  transition: transform 0.15s ease;
  display: inline-block;
}

.run-caret.open {
  transform: rotate(90deg);
}

.run-detail {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 14px;
  padding: 10px 14px 14px;
  background: var(--fm-code-bg);
  border-bottom: 1px solid var(--fm-border);
}

.run-col {
  min-width: 0;
}

.run-col-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.run-col-status {
  font-weight: 700;
}

.run-col-meta {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  font-weight: 400;
}

.run-line {
  display: flex;
  gap: 8px;
  padding: 3px 0;
  font-size: 11px;
}

.rk {
  width: 132px;
  flex-shrink: 0;
  color: var(--fm-muted);
  word-break: break-all;
}

.rv {
  flex: 1;
  color: var(--fm-text-2);
  word-break: break-all;
}

.run-block {
  margin-top: 6px;
}

.run-pre {
  margin: 0;
  padding: 8px 10px;
  max-height: 200px;
  overflow: auto;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

.run-error {
  padding: 6px 8px;
  background: var(--fm-danger-soft);
  border-radius: 4px;
  font-size: 11px;
  color: var(--fm-danger);
  word-break: break-all;
}

@media (max-width: 1100px) {
  .panes {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
    /* 单列时请求与响应上下叠放，各自固定一半高度、内部滚动 */
    height: clamp(600px, 82vh, 940px);
    min-height: 0;
    overflow: hidden;
  }

  .req-pane,
  .res-pane {
    min-height: 240px;
  }

  .run-row {
    grid-template-columns: 120px 54px 46px 58px minmax(0, 1fr) 16px;
  }

  .run-row > :nth-child(5) {
    display: none;
  }
}

@media (max-width: 860px) {
  .topbar {
    height: auto;
    min-height: 48px;
    padding: 6px 12px;
    flex-wrap: wrap;
    gap: 8px;
  }

  .left {
    flex: 1 1 100%;
    min-width: 0;
  }

  .name-input {
    width: 100%;
    flex: 1;
  }

  .right {
    flex-wrap: wrap;
  }

  .url-bar {
    flex-wrap: wrap;
    padding: 8px 12px;
    gap: 8px;
  }

  .method-select {
    width: 92px;
  }

  .url-input {
    flex: 1 1 100%;
    order: -1;
  }

  .env-select {
    width: 148px;
  }

  .panes {
    padding: 8px 10px;
  }

  .tabs {
    padding: 0 10px;
  }

  .preset-select {
    width: 100%;
  }

  .header-tools {
    flex-wrap: wrap;
  }

  .runs-hint {
    display: none;
  }

  .run-detail {
    grid-template-columns: minmax(0, 1fr);
  }

  .run-row {
    grid-template-columns: 108px 50px 42px minmax(0, 1fr) 16px;
    gap: 6px;
  }

  .run-row > :nth-child(4),
  .run-row > :nth-child(5) {
    display: none;
  }
}

/* ---------- 代码片段抽屉 ---------- */
.code-hint {
  font-size: 11.5px;
  line-height: 1.75;
  color: var(--fm-muted);
  padding: 10px 12px;
  border-radius: 9px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  margin-bottom: 14px;
}

.code-tabs {
  margin-bottom: 12px;
}

.code-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 10px;
}

.code-file {
  margin-left: auto;
  font-size: 11px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.code-edited-badge {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 10.5px;
  line-height: 16px;
  color: var(--fm-warning, #d97706);
  background: color-mix(in srgb, var(--fm-warning, #d97706) 12%, transparent);
  border: 1px solid color-mix(in srgb, var(--fm-warning, #d97706) 30%, transparent);
  cursor: help;
}

.code-block {
  margin: 0;
  padding: 14px 16px;
  border-radius: 10px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  line-height: 1.75;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
  max-height: calc(100vh - 280px);
  overflow: auto;
}

.code-textarea {
  display: block;
  width: 100%;
  height: auto;
  min-height: 240px;
  max-height: calc(100vh - 300px);
  resize: none;
  outline: none;
  tab-size: 2;
  transition: border-color 0.16s ease, box-shadow 0.16s ease, background 0.16s ease;
  background: var(--fm-surface);
  border-style: dashed;
}

.code-textarea:focus {
  border-color: var(--fm-primary);
  border-style: solid;
  background: var(--fm-surface);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fm-primary) 16%, transparent);
}

/* ---------- v1.0.8：发送选项 / 停止按钮 / 协议徽标 ---------- */
.send-opt-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  position: relative;
}

.send-opt-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--fm-primary);
  margin-left: 2px;
}

.send-opt-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px;
}

.send-opt-title {
  font-weight: 600;
  font-size: 13px;
}

.send-opt-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.send-opt-label {
  font-size: 12px;
  color: var(--fm-text-2);
  min-width: 56px;
}

.send-opt-hint {
  font-size: 11px;
  line-height: 1.5;
  color: var(--fm-text-3);
}

.send-opt-hint.inline {
  margin-left: 0;
}

.send-opt-hint code {
  font-size: 11px;
  padding: 0 3px;
  border-radius: 3px;
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.05));
}

.send-opt-warn {
  font-size: 11px;
  line-height: 1.5;
  color: #d97706;
}

.stop-btn {
  margin-left: 6px;
}

.proto-pill {
  margin-left: 6px;
  padding: 0 6px;
  height: 18px;
  line-height: 18px;
  font-size: 11px;
  border-radius: 4px;
  color: var(--fm-text-2);
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.05));
  border: 1px solid var(--fm-border, rgba(0, 0, 0, 0.08));
}

.proto-pill.warn {
  color: #d97706;
  border-color: rgba(217, 119, 6, 0.4);
  background: rgba(217, 119, 6, 0.1);
}

.proto-pill.retry {
  color: #2563eb;
  border-color: rgba(37, 99, 235, 0.35);
  background: rgba(37, 99, 235, 0.08);
}

.proto-pill.run-source {
  color: #7c3aed;
  border-color: rgba(124, 58, 237, 0.35);
  background: rgba(124, 58, 237, 0.08);
}

/* ---------- v1.0.8：二进制响应 ---------- */
.binary-body {
  display: flex;
  gap: 18px;
  align-items: flex-start;
  padding: 18px 16px;
}

.binary-preview {
  flex: 0 0 auto;
  max-width: 46%;
  border: 1px solid var(--fm-border, rgba(0, 0, 0, 0.08));
  border-radius: 8px;
  overflow: hidden;
  background: repeating-conic-gradient(rgba(128, 128, 128, 0.12) 0% 25%, transparent 0% 50%) 50% / 16px 16px;
}

.binary-preview img {
  display: block;
  max-width: 100%;
  max-height: 320px;
  object-fit: contain;
}

.binary-icon {
  flex: 0 0 auto;
  width: 96px;
  height: 96px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  letter-spacing: 1px;
  color: var(--fm-text-3);
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.05));
  border: 1px dashed var(--fm-border, rgba(0, 0, 0, 0.15));
}

.binary-meta {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.binary-title {
  font-size: 14px;
  font-weight: 600;
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.binary-size {
  font-size: 12px;
  font-weight: 400;
  color: var(--fm-text-3);
}

.binary-desc {
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-2);
}

.binary-desc.bad {
  color: #dc2626;
}

.binary-path {
  font-size: 11px;
  color: var(--fm-text-3);
  word-break: break-all;
  padding: 6px 8px;
  border-radius: 5px;
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.04));
}

.binary-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

/* ---------- v1.0.8：计时瀑布 ---------- */
.timing-panel {
  padding: 14px 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.timing-total {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.timing-total-label {
  font-size: 12px;
  color: var(--fm-text-3);
}

.timing-total-value {
  font-size: 18px;
  font-weight: 600;
}

.waterfall {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.wf-row {
  display: grid;
  grid-template-columns: 96px 1fr 88px;
  align-items: center;
  gap: 10px;
}

.wf-label {
  font-size: 12px;
  color: var(--fm-text-2);
  text-align: right;
}

.wf-track {
  position: relative;
  height: 16px;
  border-radius: 4px;
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.05));
  overflow: hidden;
}

.wf-bar {
  position: absolute;
  top: 0;
  height: 100%;
  border-radius: 4px;
  min-width: 2px;
  transition: width 0.2s ease;
}

.wf-value {
  font-size: 11px;
  color: var(--fm-text-2);
  text-align: right;
}

.timing-notes {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.04));
}

.timing-note {
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-3);
}

.timing-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-top: 1px solid var(--fm-border, rgba(0, 0, 0, 0.07));
  padding-top: 10px;
}

.timing-block-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.hop-row {
  display: grid;
  grid-template-columns: 28px 52px 140px 1fr 76px;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  padding: 3px 0;
}

.hop-index {
  color: var(--fm-text-3);
}

.hop-status {
  font-weight: 600;
  color: #059669;
}

.hop-status.bad {
  color: #dc2626;
}

.hop-host {
  color: var(--fm-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hop-loc {
  color: var(--fm-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hop-ms {
  text-align: right;
  color: var(--fm-text-2);
}

.ignored-headers {
  margin-top: 10px;
  padding: 10px 12px;
  border-radius: 6px;
  background: rgba(217, 119, 6, 0.08);
  border: 1px solid rgba(217, 119, 6, 0.25);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ignored-title {
  font-size: 12px;
  font-weight: 600;
  color: #b45309;
}

.ignored-list {
  font-size: 11px;
  color: #b45309;
  word-break: break-all;
}

.ignored-hint {
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-3);
}
</style>
