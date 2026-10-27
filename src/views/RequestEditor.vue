<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save as saveFile } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeftOutlined,
  ThunderboltOutlined,
  SendOutlined,
  SaveOutlined,
  DeleteOutlined,
  CloseOutlined,
  CopyOutlined,
  CodeOutlined,
  PlusOutlined,
} from '@ant-design/icons-vue'
import KeyValueEditor from '@/components/KeyValueEditor.vue'
import CodeArea from '@/components/CodeArea.vue'
import { useProjectStore } from '@/stores/project'
import { MAX_REQUEST_TABS, useTabsStore } from '@/stores/tabs'
import { formatDateTime } from '@/utils/datetime'
import { buildSnippets, type Snippet } from '@/utils/curl'
import { diffLines, diffStats, type DiffLine } from '@/utils/diff'
import * as api from '@/api'
import type {
  CookieItem,
  FormField,
  HttpRequestDetail,
  HttpResult,
  KeyValueItem,
  RequestAuth,
  RequestRun,
  RequestSearchHit,
  SendRequestPayload,
} from '@/types/models'
import {
  clearRuntimeVars,
  listRuntimeVars,
  renderRuntimeVars,
  runEnvScript,
  runPostScript,
  runPreScript,
  type ScriptAssertion,
  type ScriptLog,
  type ScriptRequest,
} from '@/utils/scriptRunner'
import { POST_SCRIPT_EXAMPLES, PRE_SCRIPT_EXAMPLES } from '@/utils/scriptExamples'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const tabStore = useTabsStore()

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
  preScript: '',
  postScript: '',
  assertions: '',
  auth: emptyAuth(),
})

const loading = ref(true)
const saving = ref(false)
const sending = ref(false)
const result = ref<HttpResult | null>(null)
const activeTab = ref('params')
const responseTab = ref('body')
const insecure = ref(false)
const envId = ref<number | null>(null)

/* ---------- 自动保存 ---------- */
const autoState = ref<'saved' | 'dirty' | 'saving' | 'error'>('saved')
let hydrating = true
let lastSaved = ''
let saveTimer: number | undefined
let saveQueue: Promise<boolean> = Promise.resolve(true)

function syncTabMeta() {
  if (!form.value.id) return
  tabStore.touch(form.value.id, form.value.name.trim() || '未命名请求', form.value.method)
}

function scheduleAutoSave() {
  if (saveTimer) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined
    void save(false)
  }, 900)
}

watch(
  form,
  () => {
    if (hydrating || !form.value.id) return
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

function resetScriptVars() {
  clearRuntimeVars()
  scriptVars.value = {}
  message.success('已清空运行时变量')
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
    if (type === 'multipart') {
      try {
        const parsed = JSON.parse(form.value.body || '[]')
        if (!Array.isArray(parsed)) form.value.body = '[]'
      } catch {
        form.value.body = '[]'
      }
    } else if (type === 'binary') {
      form.value.body = ''
    } else if (type === 'graphql') {
      loadGraphqlBody()
    }
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
  { id: 3, group: '常用', key: 'User-Agent', value: 'FbinHttp/1.0' },
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

async function loadRequest(id: number) {
  if (!id) {
    loading.value = false
    return
  }
  loading.value = true
  hydrating = true
  try {
    const detail = await api.requestGet(id)
    if (!detail.auth) detail.auth = emptyAuth()
    form.value = detail
    if (detail.bodyType === 'graphql') loadGraphqlBody()
    lastSaved = JSON.stringify(detail)
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
    await loadRuns()
  } catch (e) {
    tabStore.close(id)
    message.error(String(e))
  } finally {
    loading.value = false
    hydrating = false
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
  await flushSave()
  await loadRequest(id)
})

onBeforeRouteLeave(async () => {
  await flushSave()
})

async function flushSave(): Promise<boolean> {
  if (saveTimer) {
    window.clearTimeout(saveTimer)
    saveTimer = undefined
  }
  return save(false)
}

async function persist(showToast: boolean): Promise<boolean> {
  if (!form.value.id) return true
  const payload = JSON.parse(JSON.stringify(form.value)) as HttpRequestDetail
  const snapshot = JSON.stringify(payload)
  if (!showToast && snapshot === lastSaved) {
    autoState.value = 'saved'
    return true
  }
  saving.value = true
  autoState.value = 'saving'
  try {
    await api.requestSave(payload)
    lastSaved = snapshot
    if (JSON.stringify(form.value) === snapshot) {
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
  router.push(`/project/${projectId.value}`)
}

async function switchTab(id: number) {
  if (id === requestId.value) return
  await flushSave()
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

async function send() {
  if (!form.value.url.trim()) {
    message.warning('请填写 URL')
    return
  }
  sending.value = true
  result.value = null
  resetScriptOutput()
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
    const envScript = store.environments.find((item) => item.id === envId.value)?.script?.trim()
    if (envScript) {
      const envResult = runEnvScript(envScript, request, envVars)
      scriptLogs.value.push(...envResult.logs)
      scriptMs.value += envResult.durationMs
      scriptError.value = envResult.error ?? ''
      if (envResult.error) {
        message.error(`环境脚本执行失败：${envResult.error}`)
        return
      }
      outgoing = envResult.request
    }

    const preCode = form.value.preScript?.trim()
    if (preCode) {
      const preResult = runPreScript(preCode, outgoing, envVars)
      scriptLogs.value.push(...preResult.logs)
      scriptMs.value += preResult.durationMs
      scriptError.value = preResult.error ?? ''
      if (preResult.error) {
        message.error(`前置脚本执行失败：${preResult.error}`)
        return
      }
      outgoing = preResult.request
    }

    const payload: SendRequestPayload = {
      requestId: form.value.id ?? null,
      name: form.value.name,
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
      envId: envId.value,
      insecure: insecure.value,
      timeoutMs: 30000,
      auth: form.value.auth ?? null,
      cookieEnabled: true,
    }

    result.value = await api.sendRequest(payload)

    const postCode = form.value.postScript?.trim()
    if (postCode) {
      const postResult = runPostScript(postCode, outgoing, result.value, envVars)
      scriptLogs.value.push(...postResult.logs)
      scriptAssertions.value = postResult.assertions
      scriptMs.value += postResult.durationMs
      scriptError.value = postResult.error ?? ''
      if (postResult.error) message.error(`后置脚本执行失败：${postResult.error}`)
      if (postResult.assertions.length || postResult.error) responseTab.value = 'tests'
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    sending.value = false
    scriptVars.value = listRuntimeVars()
  }
  await loadRuns()
}

const runs = ref<RequestRun[]>([])
const runsOpen = ref(true)
const runsLoading = ref(false)
const expandedRunId = ref<number | null>(null)

async function loadRuns() {
  if (!form.value.id) return
  runsLoading.value = true
  try {
    runs.value = await api.runList(form.value.id, 200)
  } catch (e) {
    message.error(String(e))
  } finally {
    runsLoading.value = false
  }
}

function toggleRun(run: RequestRun) {
  expandedRunId.value = expandedRunId.value === run.id ? null : run.id
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

function fmtTimeOf(ts: number) {
  return formatDateTime(ts)
}

function runHeaders(run: RequestRun) {
  return Object.entries(run.responseHeaders ?? {})
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
      expandedRunId.value = null
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
    await api.fileWriteText(path, result.value.body)
    message.success('响应体已保存')
  } catch (e) {
    message.error(String(e))
  }
}

function openDiff() {
  diffUseCustom.value = runs.value.length === 0
  diffBaseRunId.value = runs.value[0]?.id ?? null
  diffCustomText.value = ''
  diffOpen.value = true
}

const diffBaseText = computed(() => {
  if (diffUseCustom.value) return diffCustomText.value
  const run = runs.value.find((item) => item.id === diffBaseRunId.value)
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

const prettyBody = computed(() => {
  const body = result.value?.body ?? ''
  try {
    return JSON.stringify(JSON.parse(body), null, 2)
  } catch {
    return body
  }
})

const bodyView = ref<'pretty' | 'raw'>('pretty')

const bodySize = computed(() => new Blob([form.value.body ?? '']).size)

const projectName = computed(
  () => store.projects.find((p) => p.id === projectId.value)?.name ?? '',
)

function formatBody() {
  try {
    form.value.body = JSON.stringify(JSON.parse(form.value.body), null, 2)
    message.success('已格式化')
  } catch {
    message.error('JSON 解析失败，请检查格式')
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

function openCode() {
  if (!form.value.url?.trim()) {
    message.warning('请先填写请求 URL')
    return
  }
  codeOpen.value = true
}

async function copySnippet() {
  try {
    await navigator.clipboard.writeText(activeSnippet.value.code)
    message.success(`已复制 ${activeSnippet.value.label} 代码`)
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
            <a-input v-model:value="form.name" class="name-input" placeholder="请求名称" />
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
          <a-button v-if="form.id" type="text" size="small" danger class="del-btn" @click="deleteRequest">
            <template #icon><DeleteOutlined /></template>
            删除
          </a-button>
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
        <div class="tabs-scroll">
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
          <a-button type="primary" class="send-btn" :loading="sending" @click="send">
            <template #icon><SendOutlined /></template>
            发送
          </a-button>
        </div>
        <span class="url-tip">Enter 发送 · 支持 {{ }} 变量</span>
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

                <CodeArea
                  v-else-if="form.bodyType !== 'none'"
                  v-model="form.body"
                  fill
                  :rows="14"
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
            <template v-if="result">
              <span class="status-pill mono" :class="{ ok: result.status > 0 && result.status < 400, bad: result.status >= 400 || !result.status }">
                {{ result.status || 'ERR' }}{{ result.statusText ? ` ${result.statusText}` : '' }}
              </span>
              <span class="meta-divider" />
              <span class="meta mono">{{ result.durationMs }} ms</span>
              <span class="meta-divider" />
              <span class="meta mono">{{ fmtSize(result.sizeBytes) }}</span>
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
              <a-radio-group v-model:value="bodyView" size="small" button-style="solid" class="view-toggle">
                <a-radio-button value="pretty">Pretty</a-radio-button>
                <a-radio-button value="raw">Raw</a-radio-button>
              </a-radio-group>
              <a-button size="small" type="text" @click="copyBody">复制</a-button>
              <a-button size="small" type="text" @click="saveResponseBody">另存为</a-button>
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
                  Body <span class="tab-count">{{ prettyBody ? `${prettyBody.split('\n').length} 行` : '0' }}</span>
                </template>
                <div v-if="result.truncated" class="truncate-hint">响应体超过 5MB，已截断显示</div>
                <div class="res-body-wrap">
                  <pre class="mono body-pre">{{ bodyView === 'pretty' ? prettyBody : result.body }}</pre>
                </div>
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
                    :message="`脚本执行出错：${scriptError}`"
                    show-icon
                    class="test-alert"
                  />
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

      <section class="runs-panel" :class="{ collapsed: !runsOpen }">
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
            <template v-for="run in runs" :key="run.id">
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
                <div class="run-col">
                  <div class="run-col-title">请求参数</div>
                  <div class="run-line">
                    <span class="rk">URL</span>
                    <span class="rv mono">{{ run.url }}</span>
                  </div>
                  <div v-for="p in run.params.filter((i) => i.enabled)" :key="`p-${p.key}-${p.value}`" class="run-line">
                    <span class="rk">{{ p.key }}</span>
                    <span class="rv mono">{{ p.value }}</span>
                  </div>
                  <div v-for="h in run.headers" :key="`h-${h.key}-${h.value}`" class="run-line">
                    <span class="rk">{{ h.key }}</span>
                    <span class="rv mono">{{ h.value }}</span>
                  </div>
                  <div v-if="run.body" class="run-block">
                    <div class="run-col-title">请求 Body · {{ run.bodyType }}</div>
                    <pre class="mono run-pre">{{ prettyRunBody(run.body) }}</pre>
                  </div>
                  <div v-if="!run.params.length && !run.headers.length && !run.body" class="runs-empty">
                    该次请求没有额外参数
                  </div>
                </div>

                <div class="run-col">
                  <div class="run-col-title">
                    响应
                    <span class="mono run-col-status" :style="{ color: runStatusColor(run.responseStatus) }">
                      {{ run.responseStatus || 'ERR' }} {{ run.responseStatusText }}
                    </span>
                    <span class="run-col-meta mono">{{ run.durationMs }} ms · {{ fmtSize(run.sizeBytes) }}</span>
                  </div>
                  <div v-if="run.error" class="run-error">{{ run.error }}</div>
                  <template v-else>
                    <div v-for="[k, v] in runHeaders(run)" :key="k" class="run-line">
                      <span class="rk">{{ k }}</span>
                      <span class="rv mono">{{ v }}</span>
                    </div>
                    <pre v-if="run.responseBody" class="mono run-pre">{{ prettyRunBody(run.responseBody) }}</pre>
                    <div v-else class="runs-empty">响应体为空</div>
                  </template>
                </div>
              </div>
            </template>
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
        <a-button size="small" type="primary" @click="copySnippet">
          <template #icon><CopyOutlined /></template>
          复制
        </a-button>
        <span class="code-file mono">{{ form.name || 'request' }} · {{ activeSnippet.language }}</span>
      </div>
      <pre class="code-block mono">{{ activeSnippet.code }}</pre>
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
  </a-spin>
</template>

<style scoped>
.editor-page {
  height: 100%;
  display: flex;
  flex-direction: column;
}

:deep(.ant-spin-nested-loading),
:deep(.ant-spin-container) {
  height: 100%;
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
  overflow: hidden;
  text-overflow: ellipsis;
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
  width: 230px;
  font-weight: 600;
  border-color: transparent;
  background: transparent;
  font-size: 13px;
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
  width: 104px;
  flex-shrink: 0;
  font-weight: 700;
}

.method-select :deep(.ant-select-selector) {
  box-shadow: none !important;
}

.method-select.m-GET :deep(.ant-select-selection-item) {
  color: var(--fm-success);
}
.method-select.m-POST :deep(.ant-select-selection-item) {
  color: var(--fm-warning);
}
.method-select.m-PUT :deep(.ant-select-selection-item) {
  color: var(--fm-primary);
}
.method-select.m-PATCH :deep(.ant-select-selection-item) {
  color: var(--fm-purple);
}
.method-select.m-DELETE :deep(.ant-select-selection-item) {
  color: var(--fm-danger);
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

.send-btn {
  height: 34px;
  flex-shrink: 0;
  border-radius: 7px;
  font-weight: 600;
}

.url-tip {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--fm-muted);
}

.panes {
  flex: 1;
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
  max-height: 32%;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
  border-top: 1px solid var(--fm-border);
}

.runs-panel.collapsed {
  max-height: none;
}

.runs-head {
  height: 34px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
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

.runs-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
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
  top: 0;
  z-index: 1;
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
    overflow: auto;
  }

  .req-pane,
  .res-pane {
    min-height: 320px;
  }

  .runs-panel {
    max-height: 50%;
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
  gap: 10px;
  margin-bottom: 10px;
}

.code-file {
  font-size: 11px;
  color: var(--fm-muted);
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
</style>
