<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import {
  ApiOutlined,
  CheckCircleOutlined,
  ClearOutlined,
  CodeOutlined,
  CopyOutlined,
  DownOutlined,
  DownloadOutlined,
  DeleteOutlined,
  FileZipOutlined,
  GlobalOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  RightOutlined,
  SafetyCertificateOutlined,
  SearchOutlined,
  SendOutlined,
  SettingOutlined,
  StopOutlined,
} from '@ant-design/icons-vue'
import * as api from '@/api/capture'
import type {
  CaptureBody,
  CaptureCaInfo,
  CaptureFlow,
  CaptureFlowDetail,
  CaptureHeader,
  CaptureRepeatResult,
  CaptureStatus,
} from '@/types/capture'
import { formatDateTime } from '@/utils/datetime'

interface HostGroup {
  host: string
  flows: CaptureFlow[]
  firstId: number
  errors: number
  lastStatus: number | null
}

interface GroupMenu {
  x: number
  y: number
  host: string
  ids: number[]
}

interface RepeatForm {
  method: string
  url: string
  headers: CaptureHeader[]
  body: string
  /** 原始请求体是二进制时不允许改写，重发按原样发送 */
  bodyLocked: boolean
  originalBody: string
}

const status = ref<CaptureStatus>(emptyStatus())
const ca = ref<CaptureCaInfo>(emptyCa())
const flows = ref<CaptureFlow[]>([])
const detail = ref<CaptureFlowDetail | null>(null)
const selectedId = ref<number | null>(null)
const filter = ref('')
const onlyErrors = ref(false)
const autoScroll = ref(true)
const port = ref(8888)
const bindAddr = ref('127.0.0.1')
const starting = ref(false)
const stopping = ref(false)
const loadingCa = ref(false)
const installingCa = ref(false)
const sysproxyBusy = ref(false)
const settingsOpen = ref(false)
const repeatBusy = ref(false)
const repeatOpen = ref(false)
const repeatResultOpen = ref(false)
const repeatResult = ref<CaptureRepeatResult | null>(null)
const repeatResultPretty = ref(true)
const repeatForm = ref<RepeatForm>({
  method: 'GET',
  url: '',
  headers: [],
  body: '',
  bodyLocked: false,
  originalBody: '',
})
const repeatMethods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const repeatSkipHeaders = new Set([
  'host',
  'content-length',
  'connection',
  'proxy-connection',
  'transfer-encoding',
  'upgrade',
  'keep-alive',
  'content-encoding',
])
const exporting = ref(false)
const collapsedHosts = ref<Set<string>>(new Set())
const groupMenu = ref<GroupMenu | null>(null)
const requestPretty = ref(true)
const responsePretty = ref(true)
const flowList = ref<HTMLElement | null>(null)
const workspace = ref<HTMLElement | null>(null)
const splitWidth = ref(380)
const resizing = ref(false)
const tauriReady = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
let timer: number | undefined
let caTimer: number | undefined

function emptyStatus(): CaptureStatus {
  return {
    state: 'idle',
    port: 0,
    bindAddr: '127.0.0.1',
    sessionId: null,
    startedAt: 0,
    flowCount: 0,
    dropped: 0,
    caInstalled: false,
    sysproxyOn: false,
    upstreamProxy: null,
    lastError: null,
  }
}

function emptyCa(): CaptureCaInfo {
  return {
    exists: false,
    fingerprint: '',
    thumbprint: '',
    createdAt: 0,
    expiresAt: 0,
    installed: false,
  }
}

const running = computed(
  () => status.value.state === 'running' || status.value.state === 'starting',
)
const stateText = computed(() => {
  switch (status.value.state) {
    case 'starting':
      return '启动中'
    case 'running':
      return '录制中'
    case 'stopping':
      return '停止中'
    case 'error':
      return '异常'
    default:
      return '未开始'
  }
})
const stateColor = computed(() => {
  switch (status.value.state) {
    case 'running':
      return 'success'
    case 'starting':
    case 'stopping':
      return 'processing'
    case 'error':
      return 'error'
    default:
      return undefined
  }
})
const proxyAddress = computed(() => {
  const activePort = status.value.port > 0 ? status.value.port : Number(port.value) || 8888
  const activeBind = status.value.port > 0 ? status.value.bindAddr : bindAddr.value
  return `${activeBind}:${activePort}`
})
const localProxy = computed(() => {
  const activePort = status.value.port > 0 ? status.value.port : Number(port.value) || 8888
  return `127.0.0.1:${activePort}`
})
const isLan = computed(() => bindAddr.value === '0.0.0.0' || status.value.bindAddr === '0.0.0.0')
const lanIps = ref<string[]>([])
const phoneProxy = computed(() => {
  const ip = lanIps.value[0]
  if (!ip) return ''
  const activePort = status.value.port > 0 ? status.value.port : Number(port.value) || 8888
  return `${ip}:${activePort}`
})
const filterActive = computed(() => !!filter.value.trim() || onlyErrors.value)

const visibleFlows = computed(() => {
  const keyword = filter.value.trim().toLowerCase()
  return flows.value.filter((flow) => {
    if (onlyErrors.value && !flow.error && (flow.status ?? 0) < 400) return false
    if (!keyword) return true
    return [flow.method, flow.host, flow.path, flow.status?.toString() ?? '', flow.contentType]
      .some((value) => value.toLowerCase().includes(keyword))
  })
})

const groups = computed<HostGroup[]>(() => {
  const map = new Map<string, HostGroup>()
  for (const flow of visibleFlows.value) {
    let group = map.get(flow.host)
    if (!group) {
      group = {
        host: flow.host,
        flows: [],
        firstId: flow.id,
        errors: 0,
        lastStatus: null,
      }
      map.set(flow.host, group)
    }
    group.flows.push(flow)
    if (flow.error || (flow.status ?? 0) >= 500) group.errors += 1
    group.lastStatus = flow.status
  }
  return [...map.values()].sort((a, b) => a.firstId - b.firstId)
})

const errorCount = computed(() => flows.value.filter((flow) => flow.error).length)
const queryParams = computed(() => (detail.value ? queryRows(detail.value.path) : []))
const requestCookies = computed(() =>
  detail.value ? cookieRows(detail.value.requestHeaders, 'request') : [],
)
const responseCookies = computed(() =>
  detail.value ? cookieRows(detail.value.responseHeaders, 'response') : [],
)

function isCollapsed(host: string) {
  if (filterActive.value) return false
  return collapsedHosts.value.has(host)
}

function toggleHost(host: string) {
  const next = new Set(collapsedHosts.value)
  if (next.has(host)) next.delete(host)
  else next.add(host)
  collapsedHosts.value = next
}

function openGroupMenu(event: MouseEvent, group: HostGroup) {
  closeGroupMenu()
  // 菜单宽约 230、高约 190，贴边时向内收，避免被窗口裁掉
  const x = Math.max(8, Math.min(event.clientX, window.innerWidth - 238))
  const y = Math.max(8, Math.min(event.clientY, window.innerHeight - 198))
  groupMenu.value = {
    x,
    y,
    host: group.host,
    ids: group.flows.map((flow) => flow.id),
  }
}

function closeGroupMenu() {
  groupMenu.value = null
}

function expandAll() {
  collapsedHosts.value = new Set()
}

function collapseAll() {
  collapsedHosts.value = new Set(groups.value.map((group) => group.host))
}

function methodClass(method: string) {
  return `method-${method.toUpperCase()}`
}

function statusTone(code: number | null) {
  if (code == null) return 'st-error'
  if (code >= 500) return 'st-5xx'
  if (code >= 400) return 'st-4xx'
  if (code >= 300) return 'st-3xx'
  if (code >= 200) return 'st-2xx'
  return 'st-error'
}

function statusClass(flow: CaptureFlow) {
  return flow.error ? 'st-error' : statusTone(flow.status)
}

function formatBytes(value: number) {
  if (!value) return '—'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
}

function formatDuration(value: number) {
  if (value < 1000) return `${value} ms`
  return `${(value / 1000).toFixed(2)} s`
}

function pad(value: number, length = 2) {
  return String(value).padStart(length, '0')
}

function formatClock(timestamp: number) {
  if (!timestamp) return '—'
  const date = new Date(timestamp)
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(
    date.getMilliseconds(),
    3,
  )}`
}

function headerText(headers: CaptureHeader[]) {
  return headers.map((header) => `${header.name}: ${header.value}`).join('\n') || '（空）'
}

function decodeValue(value: string) {
  try {
    return decodeURIComponent(value.replace(/\+/g, ' '))
  } catch {
    return value
  }
}

function queryRows(path: string): CaptureHeader[] {
  const index = path.indexOf('?')
  if (index < 0) return []
  return path
    .slice(index + 1)
    .split('&')
    .filter(Boolean)
    .map((pair) => {
      const [name, ...rest] = pair.split('=')
      return { name: decodeValue(name ?? ''), value: decodeValue(rest.join('=')) }
    })
}

function cookieRows(headers: CaptureHeader[], kind: 'request' | 'response'): CaptureHeader[] {
  if (kind === 'request') {
    return headers
      .filter((header) => header.name.toLowerCase() === 'cookie')
      .flatMap((header) =>
        header.value
          .split(';')
          .map((item) => item.trim())
          .filter(Boolean)
          .map((item) => {
            const [name, ...rest] = item.split('=')
            return { name: name.trim(), value: rest.join('=').trim() }
          }),
      )
  }
  return headers
    .filter((header) => header.name.toLowerCase() === 'set-cookie')
    .map((header) => {
      const [pair, ...attrs] = header.value.split(';')
      const [name, ...rest] = pair.split('=')
      return {
        name: name.trim(),
        value: [rest.join('=').trim(), ...attrs.map((attr) => attr.trim())]
          .filter(Boolean)
          .join('; '),
      }
    })
}

function currentUrl(flow: CaptureFlowDetail) {
  return `${flow.scheme}://${flow.host}${flow.path}`
}

function shellQuote(value: string) {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

function curlText(flow: CaptureFlowDetail) {
  const parts = [`curl -X ${flow.method} ${shellQuote(currentUrl(flow))}`]
  for (const header of flow.requestHeaders) {
    const name = header.name.toLowerCase()
    if (['host', 'content-length', 'connection', 'proxy-connection'].includes(name)) continue
    parts.push(`  -H ${shellQuote(`${header.name}: ${header.value}`)}`)
  }
  if (flow.requestBody.text) {
    parts.push(`  --data-raw ${shellQuote(flow.requestBody.text)}`)
  }
  return parts.join(' \\\n')
}

function isJsonBody(body: CaptureBody) {
  if (!body.text) return false
  if (body.contentType.includes('json')) return true
  return /^\s*[{\[]/.test(body.text)
}

function bodyText(body: CaptureBody, pretty: boolean) {
  if (!body.text) return ''
  if (!pretty || !isJsonBody(body)) return body.text
  try {
    return JSON.stringify(JSON.parse(body.text), null, 2)
  } catch {
    return body.text
  }
}

function bodyDataUrl(body: CaptureBody) {
  if (!body.base64) return ''
  return `data:${body.contentType || 'application/octet-stream'};base64,${body.base64}`
}

function rawRequest() {
  const flow = detail.value
  if (!flow) return ''
  const body = flow.requestBody.text ? `\n\n${flow.requestBody.text}` : ''
  return `${flow.method} ${flow.path} ${flow.httpVersion}\n${headerText(flow.requestHeaders)}${body}`
}

function rawResponse() {
  const flow = detail.value
  if (!flow) return ''
  const body = flow.responseBody.text ? `\n\n${flow.responseBody.text}` : ''
  return `${flow.httpVersion} ${flow.status ?? 'ERR'}\n${headerText(flow.responseHeaders)}${body}`
}

function bodyExtension(body: CaptureBody) {
  const type = body.contentType
  if (type.includes('json')) return 'json'
  if (type.includes('html')) return 'html'
  if (type.includes('xml')) return 'xml'
  if (type.includes('javascript')) return 'js'
  if (type.includes('css')) return 'css'
  if (type.includes('svg')) return 'svg'
  if (type.includes('png')) return 'png'
  if (type.includes('jpeg') || type.includes('jpg')) return 'jpg'
  if (type.includes('gif')) return 'gif'
  if (type.includes('webp')) return 'webp'
  if (type.startsWith('text/')) return 'txt'
  return 'bin'
}

async function refreshStatus() {
  try {
    status.value = await api.captureStatus()
    if (status.value.state !== 'idle') {
      port.value = status.value.port || port.value
      bindAddr.value = status.value.bindAddr || bindAddr.value
    }
  } catch {
    // 页面刚启动或后端重启时忽略，下一轮再取
  }
}

async function refreshFlows() {
  try {
    flows.value = await api.captureFlows(1000)
    if (autoScroll.value && !filterActive.value) {
      await nextTick()
      if (flowList.value) flowList.value.scrollTop = flowList.value.scrollHeight
    }
  } catch {
    // 代理未启动或后端重启时忽略，下一轮再取
  }
}

async function refreshCa(silent = false) {
  if (!silent) loadingCa.value = true
  try {
    ca.value = await api.captureCaInfo()
  } catch (error) {
    if (!silent) message.error(String(error))
  } finally {
    if (!silent) loadingCa.value = false
  }
}

function onListScroll() {
  closeGroupMenu()
  const element = flowList.value
  if (!element) return
  autoScroll.value = element.scrollHeight - element.scrollTop - element.clientHeight < 40
}

async function selectFlow(flow: CaptureFlow) {
  selectedId.value = flow.id
  requestPretty.value = true
  responsePretty.value = true
  try {
    detail.value = await api.captureFlowDetail(flow.id)
  } catch (error) {
    detail.value = null
    message.error(String(error))
  }
}

async function clearFlows() {
  try {
    status.value = await api.captureFlowClear()
    flows.value = []
    selectedId.value = null
    detail.value = null
  } catch (error) {
    message.error(String(error))
  }
}

async function start() {
  if (running.value) return
  const targetPort = Number(port.value)
  if (!Number.isInteger(targetPort) || targetPort < 0 || targetPort > 65535) {
    message.warning('端口需在 0~65535 之间')
    return
  }
  starting.value = true
  try {
    flows.value = []
    selectedId.value = null
    detail.value = null
    status.value = await api.captureStart({
      port: targetPort,
      bindAddr: bindAddr.value,
      sslHosts: [],
      ignorePatterns: [],
      captureStatic: true,
    })
    port.value = status.value.port || targetPort
    autoScroll.value = true
    message.success(`代理已启动：${proxyAddress.value}`)
    if (!ca.value.installed) {
      message.warning('代理已启动，但根证书未被系统信任，HTTPS 仍无法解密')
    }
  } catch (error) {
    message.error(String(error))
    await refreshStatus()
  } finally {
    starting.value = false
  }
}

async function stop() {
  if (!running.value) return
  stopping.value = true
  try {
    status.value = await api.captureStop()
    message.success('已停止录制，端口已释放')
  } catch (error) {
    message.error(String(error))
  } finally {
    stopping.value = false
  }
}

async function exportCa() {
  try {
    const path = await saveDialog({
      defaultPath: 'astroman-ca.crt',
      filters: [{ name: '根证书', extensions: ['crt', 'pem'] }],
    })
    if (!path) return
    await api.captureCaExport(path)
    message.success('证书已导出，双击安装到「受信任的根证书颁发机构」')
  } catch (error) {
    message.error(String(error))
  }
}

function installCa() {
  Modal.confirm({
    title: '安装当前根证书？',
    content:
      '将把当前根证书安装到「当前用户 → 受信任的根证书颁发机构」。安装后 HTTPS 流量才能被解密；如果之前装过旧证书，这一步会用当前证书覆盖安装。',
    okText: '安装',
    cancelText: '取消',
    async onOk() {
      installingCa.value = true
      try {
        ca.value = await api.captureCaInstall()
        if (ca.value.installed) {
          message.success('根证书已安装并被系统信任')
        } else {
          message.warning('安装命令已执行，但暂未检测到信任状态，请点「重新检测」')
        }
      } catch (error) {
        message.error(String(error))
      } finally {
        installingCa.value = false
      }
    },
  })
}

async function toggleSysproxy() {
  sysproxyBusy.value = true
  try {
    status.value = status.value.sysproxyOn
      ? await api.captureSysproxyDisable()
      : await api.captureSysproxyEnable()
    message.success(status.value.sysproxyOn ? '已接管系统代理' : '已恢复系统代理')
  } catch (error) {
    message.error(String(error))
  } finally {
    sysproxyBusy.value = false
  }
}

function regenerateCa() {
  Modal.confirm({
    title: '重新生成根证书？',
    content:
      '旧证书会立即失效。已经安装过旧证书的浏览器和手机会出现证书警告，需要卸载旧证书并重新安装。',
    okText: '重新生成',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      try {
        ca.value = await api.captureCaGenerate(true)
        message.success('已生成新证书，请重新安装')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

async function copyText(text: string, label: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success(`${label}已复制`)
  } catch {
    message.warning('当前环境不支持剪贴板')
  }
}

async function refreshLanIps() {
  try {
    lanIps.value = await api.captureLanIps()
  } catch {
    lanIps.value = []
  }
}

const repeatMethodOptions = computed(() => {
  const options = new Set(repeatMethods)
  if (repeatForm.value.method) options.add(repeatForm.value.method)
  return Array.from(options)
})

function fillRepeatForm(flow: CaptureFlowDetail) {
  const text = flow.requestBody.text
  repeatForm.value = {
    method: flow.method,
    url: currentUrl(flow),
    headers: flow.requestHeaders
      .filter((header) => !repeatSkipHeaders.has(header.name.toLowerCase()))
      .map((header) => ({ name: header.name, value: header.value })),
    body: text ?? '',
    bodyLocked: flow.requestBody.size > 0 && text === null,
    originalBody: text ?? '',
  }
}

function openRepeat() {
  if (!detail.value) return
  fillRepeatForm(detail.value)
  repeatOpen.value = true
}

function resetRepeat() {
  if (detail.value) fillRepeatForm(detail.value)
}

function addRepeatHeader() {
  repeatForm.value.headers.push({ name: '', value: '' })
}

function removeRepeatHeader(index: number) {
  repeatForm.value.headers.splice(index, 1)
}

function closeRepeatResult() {
  repeatResultOpen.value = false
  repeatResult.value = null
}

async function submitRepeat() {
  if (repeatBusy.value) return
  const flow = detail.value
  if (!flow) return
  const form = repeatForm.value
  const url = form.url.trim()
  if (!/^https?:\/\//i.test(url)) {
    message.warning('URL 需要以 http:// 或 https:// 开头')
    return
  }
  const method = form.method.trim().toUpperCase()
  if (!method) {
    message.warning('请求方法不能为空')
    return
  }
  const headers = form.headers
    .map((header) => ({ name: header.name.trim(), value: header.value }))
    .filter((header) => header.name)
  const bodyOverride =
    form.bodyLocked || form.body === form.originalBody ? undefined : form.body

  repeatBusy.value = true
  try {
    const result = await api.captureRepeat({
      id: flow.id,
      method,
      url,
      headers,
      body: bodyOverride,
    })
    repeatOpen.value = false
    if (!result.recorded) {
      repeatResult.value = result
      repeatResultPretty.value = true
      repeatResultOpen.value = true
      return
    }
    message.success('已重发，新流量已记录')
    window.setTimeout(async () => {
      await refreshFlows()
      try {
        const target = new URL(result.url)
        const path = `${target.pathname}${target.search}`
        const candidates = flows.value.filter(
          (item) => item.id > flow.id && item.method === method && item.path === path,
        )
        const latest = candidates[candidates.length - 1]
        if (latest) await selectFlow(latest)
      } catch {
        // URL 解析失败时只保留列表刷新
      }
    }, 450)
  } catch (error) {
    message.error(String(error))
  } finally {
    repeatBusy.value = false
  }
}

type ExportKind = 'har' | 'zip'

/** 导出 HAR 或 ZIP；传 ids 时只导出选中的流量（域名分组） */
async function exportCapture(kind: ExportKind, ids?: number[], host?: string) {
  if (!flows.value.length) return
  exporting.value = true
  try {
    const stamp = new Date()
    const stampText = `${stamp.getFullYear()}${pad(stamp.getMonth() + 1)}${pad(
      stamp.getDate(),
    )}-${pad(stamp.getHours())}${pad(stamp.getMinutes())}`
    const safeHost = host?.replace(/[^a-zA-Z0-9._-]+/g, '_')
    const name = `${safeHost ? `${safeHost}-` : 'astroman-'}${stampText}.${kind}`
    const path = await saveDialog({
      defaultPath: name,
      filters: [
        kind === 'har'
          ? { name: 'HAR 会话', extensions: ['har'] }
          : { name: 'ZIP 压缩包', extensions: ['zip'] },
      ],
    })
    if (!path) return
    if (kind === 'har') {
      await api.captureExportHar(path, ids)
    } else {
      await api.captureExportZip(path, ids)
    }
    if (host && ids?.length) {
      message.success(`已导出 ${host} 的 ${ids.length} 条请求`)
    } else if (kind === 'har') {
      message.success('HAR 已导出')
    } else {
      message.success('ZIP 已导出，解压后打开 index.html')
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    exporting.value = false
  }
}

function exportHar() {
  void exportCapture('har')
}

function exportZip() {
  void exportCapture('zip')
}

/** 右键域名分组：只导出该分组下的请求 */
function exportGroupHar() {
  const menu = groupMenu.value
  closeGroupMenu()
  if (!menu || !menu.ids.length) return
  void exportCapture('har', menu.ids, menu.host)
}

function exportGroupZip() {
  const menu = groupMenu.value
  closeGroupMenu()
  if (!menu || !menu.ids.length) return
  void exportCapture('zip', menu.ids, menu.host)
}

async function saveBody(part: 'request' | 'response') {
  const flow = detail.value
  if (!flow) return
  const body = part === 'request' ? flow.requestBody : flow.responseBody
  if (!body.size) {
    message.warning('没有可保存的 body')
    return
  }
  try {
    const path = await saveDialog({
      defaultPath: `flow-${flow.id}-${part}.${bodyExtension(body)}`,
    })
    if (!path) return
    await api.captureSaveBody(flow.id, part, path)
    message.success('body 已保存')
  } catch (error) {
    message.error(String(error))
  }
}

function beginResize(event: PointerEvent) {
  if (window.innerWidth < 1120) return
  event.preventDefault()
  closeGroupMenu()
  resizing.value = true
  window.addEventListener('pointermove', onResizeMove)
  window.addEventListener('pointerup', endResize, { once: true })
}

function onResizeMove(event: PointerEvent) {
  const element = workspace.value
  if (!element) return
  const rect = element.getBoundingClientRect()
  splitWidth.value = Math.min(Math.max(event.clientX - rect.left, 280), rect.width - 520)
}

function endResize() {
  resizing.value = false
  window.removeEventListener('pointermove', onResizeMove)
  localStorage.setItem('astroman.capture.split.v2', String(splitWidth.value))
}

function onGlobalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeGroupMenu()
}

onMounted(async () => {
  const stored = Number(localStorage.getItem('astroman.capture.split.v2'))
  if (Number.isFinite(stored) && stored >= 280) {
    splitWidth.value = Math.min(stored, Math.max(window.innerWidth - 780, 280))
  }
  window.addEventListener('click', closeGroupMenu)
  window.addEventListener('keydown', onGlobalKeydown)
  window.addEventListener('blur', closeGroupMenu)
  await Promise.all([refreshStatus(), refreshCa(), refreshFlows(), refreshLanIps()])
  timer = window.setInterval(() => {
    refreshStatus()
    if (running.value) refreshFlows()
  }, 1500)
  caTimer = window.setInterval(() => {
    if (document.visibilityState === 'visible') refreshCa(true)
  }, 5000)
})

onBeforeUnmount(() => {
  if (timer) window.clearInterval(timer)
  if (caTimer) window.clearInterval(caTimer)
  window.removeEventListener('pointermove', onResizeMove)
  window.removeEventListener('click', closeGroupMenu)
  window.removeEventListener('keydown', onGlobalKeydown)
  window.removeEventListener('blur', closeGroupMenu)
})

watch(settingsOpen, (open) => {
  if (open) void refreshLanIps()
})
</script>

<template>
  <div class="page capture-page">
    <header class="capture-toolbar">
      <div class="toolbar-group">
        <a-button
          v-if="!running"
          type="primary"
          class="record-btn"
          :loading="starting"
          @click="start"
        >
          <template #icon><span class="rec-dot" /></template>
          录制
        </a-button>
        <a-button v-else danger class="record-btn" :loading="stopping" @click="stop">
          <template #icon><span class="rec-square" /></template>
          停止
        </a-button>
        <a-button size="small" :disabled="!flows.length" @click="clearFlows">
          <ClearOutlined />
          清空
        </a-button>
        <a-divider type="vertical" class="toolbar-divider" />
        <a-tag :color="stateColor" class="state-pill">
          <span class="state-dot" :class="status.state" />
          {{ stateText }}
        </a-tag>
        <span class="toolbar-meta mono">{{ proxyAddress }}</span>
        <span v-if="status.dropped" class="toolbar-warn">丢弃 {{ status.dropped }}</span>
      </div>

      <div class="toolbar-group toolbar-right">
        <a-button
          size="small"
          :disabled="!flows.length"
          :loading="exporting"
          @click="exportHar"
        >
          <DownloadOutlined />
          导出 HAR
        </a-button>
        <a-button
          size="small"
          :disabled="!flows.length"
          :loading="exporting"
          title="导出 ZIP：含 index.html、HAR 与每条流量的可读明细"
          @click="exportZip"
        >
          <FileZipOutlined />
          导出 ZIP
        </a-button>
        <a-button
          size="small"
          class="ca-btn"
          :class="{ trusted: ca.installed }"
          @click="settingsOpen = true"
        >
          <SafetyCertificateOutlined />
          {{ ca.installed ? 'CA 已信任' : 'CA 未安装' }}
        </a-button>
        <a-button
          size="small"
          :disabled="!running"
          :type="status.sysproxyOn ? 'primary' : 'default'"
          :loading="sysproxyBusy"
          @click="toggleSysproxy"
        >
          <GlobalOutlined />
          {{ status.sysproxyOn ? '系统代理已接管' : '接管系统代理' }}
        </a-button>
        <a-button size="small" class="icon-btn" title="抓包设置" @click="settingsOpen = true">
          <SettingOutlined />
        </a-button>
      </div>
    </header>

    <a-alert
      v-if="!tauriReady"
      type="warning"
      show-icon
      message="当前在浏览器预览中运行，代理后端不可用；请用 npm run tauri dev 或桌面安装包启动。"
      class="capture-alert"
    />
    <a-alert
      v-if="status.lastError"
      type="error"
      show-icon
      :message="status.lastError"
      class="capture-alert"
    />

    <div ref="workspace" class="capture-workspace" :class="{ resizing }">
      <section class="fm-card flow-pane" :style="{ width: `${splitWidth}px` }">
        <div class="pane-head">
          <div class="pane-title">
            <span>会话</span>
            <span class="count">{{ visibleFlows.length }}</span>
            <span class="sub">{{ groups.length }} 个域名</span>
            <span v-if="errorCount" class="error-count">{{ errorCount }} 错误</span>
          </div>
          <div class="pane-tools">
            <a-input
              v-model:value="filter"
              allow-clear
              size="small"
              placeholder="过滤域名 / 路径 / 方法 / 状态"
              class="tree-filter"
            >
              <template #prefix><SearchOutlined /></template>
            </a-input>
            <a-checkbox v-model:checked="onlyErrors" class="err-check">仅错误</a-checkbox>
          </div>
        </div>

        <div class="tree-bar">
          <span class="tree-head">域名 / 请求</span>
          <div class="tree-actions">
            <a-checkbox v-model:checked="autoScroll" class="auto-check">自动滚动</a-checkbox>
            <a-button type="text" size="small" @click="expandAll">展开</a-button>
            <a-button type="text" size="small" @click="collapseAll">折叠</a-button>
          </div>
        </div>

        <div v-if="!flows.length" class="empty-state guide">
          <div class="guide-top">
            <div class="empty-icon">
              <ApiOutlined />
            </div>
            <div>
              <h2>{{ running ? '代理已就绪，等待流量' : '开始抓包' }}</h2>
              <p>
                {{
                  running
                    ? isLan && phoneProxy
                      ? `代理正在 ${proxyAddress} 监听；手机 Wi-Fi 代理填 ${phoneProxy}，HTTPS 需在手机安装根证书。`
                      : `代理正在 ${proxyAddress} 监听，先在设置里确认根证书与系统代理。`
                    : '按顺序完成录制、证书、代理三步，然后发起请求。'
                }}
              </p>
            </div>
          </div>
          <div class="guide-actions">
            <a-button
              v-if="!running"
              size="small"
              type="primary"
              :loading="starting"
              @click="start"
            >
              <PlayCircleOutlined />
              开始录制
            </a-button>
            <a-button size="small" @click="settingsOpen = true">
              <SafetyCertificateOutlined />
              证书与代理
            </a-button>
          </div>
          <ol class="mini-steps">
            <li :class="{ done: running }"><b>1</b>开始录制</li>
            <li :class="{ done: ca.installed }"><b>2</b>安装并信任根证书</li>
            <li :class="{ done: status.sysproxyOn }"><b>3</b>接管系统代理</li>
            <li><b>4</b>发起请求</li>
          </ol>
          <div class="code-block mono">curl -x http://{{ localProxy }} http://example.com</div>
          <p class="empty-hint">
            流量只存内存，最多 5000 条 / 128 MB body；重启后清空。HTTPS 需信任根证书，Firefox 要单独设代理。
          </p>
        </div>

        <div v-else-if="!visibleFlows.length" class="empty-state">
          <div class="empty-icon">
            <ApiOutlined />
          </div>
          <h2>没有匹配的流量</h2>
          <p>换个过滤词，或取消「仅错误」后再看。</p>
        </div>

        <div v-else ref="flowList" class="tree" @scroll="onListScroll">
          <div v-for="group in groups" :key="group.host" class="host-group">
            <button
              type="button"
              class="host-row"
              @click="toggleHost(group.host)"
              @contextmenu.prevent="openGroupMenu($event, group)"
            >
              <RightOutlined v-if="isCollapsed(group.host)" class="caret" />
              <DownOutlined v-else class="caret" />
              <GlobalOutlined class="host-icon" />
              <span class="host-name" :title="group.host">{{ group.host }}</span>
              <span class="host-count">{{ group.flows.length }}</span>
              <span v-if="group.errors" class="host-errors">{{ group.errors }} 错误</span>
              <span class="host-last" :class="statusTone(group.lastStatus)">
                {{ group.lastStatus ?? 'ERR' }}
              </span>
            </button>

            <template v-if="!isCollapsed(group.host)">
              <button
                v-for="flow in group.flows"
                :key="flow.id"
                type="button"
                class="flow-row"
                :class="{ active: selectedId === flow.id, failed: !!flow.error }"
                @click="selectFlow(flow)"
              >
                <span class="method-tag flow-method" :class="methodClass(flow.method)">
                  {{ flow.method }}
                </span>
                <span class="flow-path" :title="`${flow.path}\n${flow.status ?? 'ERR'} · ${formatDuration(flow.durationMs)}`">
                  {{ flow.path }}
                </span>
                <span class="flow-status" :class="statusClass(flow)">
                  {{ flow.status ?? 'ERR' }}
                </span>
                <span class="flow-time">{{ formatDuration(flow.durationMs) }}</span>
                <span class="flow-size">{{ formatBytes(flow.responseSize) }}</span>
              </button>
            </template>
          </div>
        </div>

        <div class="pane-foot">
          <span>{{ status.flowCount }} 条</span>
          <span v-if="filterActive">匹配 {{ visibleFlows.length }}</span>
          <span v-if="status.dropped">丢弃 {{ status.dropped }}</span>
          <span class="mono foot-proxy">{{ localProxy }}</span>
        </div>
      </section>

      <div class="splitter" :class="{ active: resizing }" @pointerdown="beginResize" />

      <section class="fm-card detail-pane" :class="{ 'detail-empty': !detail }">
        <template v-if="detail">
          <div class="detail-head">
            <div class="detail-line">
              <span class="method-tag detail-method" :class="methodClass(detail.method)">
                {{ detail.method }}
              </span>
              <span class="flow-status detail-status" :class="statusClass(detail)">
                {{ detail.status ?? 'ERR' }}
              </span>
              <span class="detail-duration">{{ formatDuration(detail.durationMs) }}</span>
              <span class="detail-clock">{{ formatClock(detail.startedAt) }}</span>
              <div class="detail-actions">
                <a-button size="small" :loading="repeatBusy" @click="openRepeat">
                  <SendOutlined />
                  重发
                </a-button>
                <a-button size="small" @click="copyText(currentUrl(detail), 'URL')">
                  <CopyOutlined />
                  URL
                </a-button>
                <a-button size="small" @click="copyText(curlText(detail), 'cURL')">
                  <CodeOutlined />
                  cURL
                </a-button>
              </div>
            </div>
            <p class="detail-url mono">{{ currentUrl(detail) }}</p>
          </div>

          <a-alert
            v-if="detail.error"
            type="error"
            show-icon
            :message="detail.error"
            class="detail-error"
          />

          <a-tabs size="small" class="detail-tabs">
            <a-tab-pane key="overview" tab="概览">
              <div class="meta-grid">
                <div class="meta-item">
                  <span>请求方法</span><b>{{ detail.method }}</b>
                </div>
                <div class="meta-item">
                  <span>响应状态</span>
                  <b :class="statusTone(detail.status)">{{ detail.status ?? 'ERR' }}</b>
                </div>
                <div class="meta-item">
                  <span>HTTP 版本</span><b>{{ detail.httpVersion || '—' }}</b>
                </div>
                <div class="meta-item">
                  <span>总耗时</span><b>{{ formatDuration(detail.durationMs) }}</b>
                </div>
                <div class="meta-item">
                  <span>请求大小</span><b>{{ formatBytes(detail.requestSize) }}</b>
                </div>
                <div class="meta-item">
                  <span>响应大小</span><b>{{ formatBytes(detail.responseSize) }}</b>
                </div>
                <div class="meta-item">
                  <span>请求类型</span><b>{{ detail.requestBody.contentType || '—' }}</b>
                </div>
                <div class="meta-item">
                  <span>响应类型</span><b>{{ detail.contentType || '—' }}</b>
                </div>
                <div class="meta-item">
                  <span>开始时间</span><b>{{ formatDateTime(detail.startedAt) }}</b>
                </div>
                <div class="meta-item">
                  <span>客户端</span><b class="mono">{{ detail.clientAddr || '—' }}</b>
                </div>
                <div class="meta-item wide">
                  <span>完整 URL</span><b class="mono">{{ currentUrl(detail) }}</b>
                </div>
              </div>
              <p class="overview-hint">
                总耗时包含连接、等待与传输；DNS / TCP / TLS / 首字节的拆分随后续版本接入。
              </p>
            </a-tab-pane>

            <a-tab-pane key="request" tab="请求">
              <section class="detail-section">
                <h4>Query 参数 <span class="section-count">{{ queryParams.length }}</span></h4>
                <table v-if="queryParams.length" class="kv-table">
                  <thead><tr><th>名称</th><th>值</th></tr></thead>
                  <tbody>
                    <tr v-for="(item, index) in queryParams" :key="`${item.name}-${index}`">
                      <td class="mono">{{ item.name }}</td>
                      <td class="mono">{{ item.value }}</td>
                    </tr>
                  </tbody>
                </table>
                <p v-else class="section-empty">没有 Query 参数</p>
              </section>

              <section v-if="requestCookies.length" class="detail-section">
                <h4>Cookie <span class="section-count">{{ requestCookies.length }}</span></h4>
                <table class="kv-table">
                  <thead><tr><th>名称</th><th>值</th></tr></thead>
                  <tbody>
                    <tr v-for="(item, index) in requestCookies" :key="`${item.name}-${index}`">
                      <td class="mono">{{ item.name }}</td>
                      <td class="mono">{{ item.value }}</td>
                    </tr>
                  </tbody>
                </table>
              </section>

              <section class="detail-section">
                <h4>请求头 <span class="section-count">{{ detail.requestHeaders.length }}</span></h4>
                <table class="kv-table">
                  <thead><tr><th>名称</th><th>值</th></tr></thead>
                  <tbody>
                    <tr v-for="(header, index) in detail.requestHeaders" :key="index">
                      <td class="mono">{{ header.name }}</td>
                      <td class="mono">{{ header.value }}</td>
                    </tr>
                  </tbody>
                </table>
              </section>

              <section class="detail-section">
                <h4>请求体</h4>
                <div class="body-block">
                  <div class="body-bar">
                    <span>{{ detail.requestBody.contentType || '未知类型' }}</span>
                    <span>· {{ formatBytes(detail.requestBody.size) }}</span>
                    <a-tag v-if="detail.requestBody.truncated" color="warning" class="body-tag">
                      仅保留前 2 MB
                    </a-tag>
                    <span class="spacer" />
                    <a-checkbox
                      v-if="isJsonBody(detail.requestBody)"
                      v-model:checked="requestPretty"
                      class="pretty-check"
                    >
                      格式化
                    </a-checkbox>
                    <a-button
                      size="small"
                      type="text"
                      title="复制请求体"
                      @click="copyText(bodyText(detail.requestBody, requestPretty), '请求体')"
                    >
                      <CopyOutlined />
                    </a-button>
                    <a-button size="small" type="text" title="保存请求体" @click="saveBody('request')">
                      <DownloadOutlined />
                    </a-button>
                  </div>
                  <div v-if="!detail.requestBody.size" class="body-empty">无请求体</div>
                  <pre
                    v-else-if="detail.requestBody.text !== null"
                    class="body-pre"
                  >{{ bodyText(detail.requestBody, requestPretty) }}</pre>
                  <div v-else class="body-empty">
                    二进制内容（{{ formatBytes(detail.requestBody.size) }}），暂不预览
                  </div>
                </div>
              </section>
            </a-tab-pane>

            <a-tab-pane key="response" tab="响应">
              <section v-if="responseCookies.length" class="detail-section">
                <h4>Set-Cookie <span class="section-count">{{ responseCookies.length }}</span></h4>
                <table class="kv-table">
                  <thead><tr><th>名称</th><th>值 / 属性</th></tr></thead>
                  <tbody>
                    <tr v-for="(item, index) in responseCookies" :key="`${item.name}-${index}`">
                      <td class="mono">{{ item.name }}</td>
                      <td class="mono">{{ item.value }}</td>
                    </tr>
                  </tbody>
                </table>
              </section>

              <section class="detail-section">
                <h4>响应头 <span class="section-count">{{ detail.responseHeaders.length }}</span></h4>
                <table v-if="detail.responseHeaders.length" class="kv-table">
                  <thead><tr><th>名称</th><th>值</th></tr></thead>
                  <tbody>
                    <tr v-for="(header, index) in detail.responseHeaders" :key="index">
                      <td class="mono">{{ header.name }}</td>
                      <td class="mono">{{ header.value }}</td>
                    </tr>
                  </tbody>
                </table>
                <p v-else class="section-empty">没有响应头</p>
              </section>

              <section class="detail-section">
                <h4>响应体</h4>
                <div class="body-block">
                  <div class="body-bar">
                    <span>{{ detail.responseBody.contentType || '未知类型' }}</span>
                    <span>· {{ formatBytes(detail.responseBody.size) }}</span>
                    <a-tag v-if="detail.responseBody.truncated" color="warning" class="body-tag">
                      仅保留前 2 MB
                    </a-tag>
                    <span class="spacer" />
                    <a-checkbox
                      v-if="isJsonBody(detail.responseBody)"
                      v-model:checked="responsePretty"
                      class="pretty-check"
                    >
                      格式化
                    </a-checkbox>
                    <a-button
                      size="small"
                      type="text"
                      title="复制响应体"
                      @click="copyText(bodyText(detail.responseBody, responsePretty), '响应体')"
                    >
                      <CopyOutlined />
                    </a-button>
                    <a-button size="small" type="text" title="保存响应体" @click="saveBody('response')">
                      <DownloadOutlined />
                    </a-button>
                  </div>
                  <div v-if="!detail.responseBody.size" class="body-empty">无响应体</div>
                  <img
                    v-else-if="detail.responseBody.base64 && detail.responseBody.contentType.startsWith('image/')"
                    class="body-image"
                    :src="bodyDataUrl(detail.responseBody)"
                    alt="响应图片预览"
                  />
                  <pre
                    v-else-if="detail.responseBody.text !== null"
                    class="body-pre"
                  >{{ bodyText(detail.responseBody, responsePretty) }}</pre>
                  <div v-else class="body-empty">
                    二进制内容（{{ formatBytes(detail.responseBody.size) }}），暂不预览
                  </div>
                </div>
              </section>
            </a-tab-pane>

            <a-tab-pane key="raw" tab="原始">
              <div class="raw-block">
                <div class="raw-head">
                  <span>请求</span>
                  <a-button type="text" size="small" @click="copyText(rawRequest(), '原始请求')">
                    <CopyOutlined />
                  </a-button>
                </div>
                <pre class="raw-pre">{{ rawRequest() }}</pre>
              </div>
              <div class="raw-block">
                <div class="raw-head">
                  <span>响应</span>
                  <a-button type="text" size="small" @click="copyText(rawResponse(), '原始响应')">
                    <CopyOutlined />
                  </a-button>
                </div>
                <pre class="raw-pre">{{ rawResponse() }}</pre>
              </div>
            </a-tab-pane>
          </a-tabs>
        </template>

        <template v-else>
          <div class="empty-icon small">
            <ApiOutlined />
          </div>
          <h3>请求详情</h3>
          <p>在左侧选择一个请求，查看概览、请求/响应头、Query、Cookie、body 与原始报文。</p>
        </template>
      </section>
    </div>

    <footer class="capture-statusbar">
      <span :class="{ 'status-ok': running, 'status-bad': status.state === 'error' }">
        ● {{ stateText }}
      </span>
      <span>端口 {{ status.port || '—' }}</span>
      <span>请求 {{ status.flowCount }}</span>
      <span>域名 {{ groups.length }}</span>
      <span v-if="status.dropped">丢弃 {{ status.dropped }}</span>
      <span>系统代理 {{ status.sysproxyOn ? '已接管' : '未接管' }}</span>
      <span :class="{ 'status-ok': ca.installed, 'status-bad': !ca.installed }">
        CA {{ ca.installed ? '已信任' : '未信任' }}
      </span>
      <span class="mono">{{ localProxy }}</span>
    </footer>

    <a-drawer
      v-model:open="settingsOpen"
      title="抓包设置"
      placement="right"
      width="min(400px, 92vw)"
    >
      <div class="settings-body">
        <section class="settings-section">
          <h4>代理</h4>
          <div class="settings-row">
            <span>监听端口</span>
            <a-input-number
              v-model:value="port"
              :min="0"
              :max="65535"
              :disabled="running"
              size="small"
              class="settings-port"
            />
          </div>
          <div class="settings-row">
            <span>监听地址</span>
            <a-select v-model:value="bindAddr" :disabled="running" size="small" class="settings-select">
              <a-select-option value="127.0.0.1">仅本机 127.0.0.1</a-select-option>
              <a-select-option value="0.0.0.0">同网段 0.0.0.0（手机）</a-select-option>
            </a-select>
          </div>
          <div class="settings-line">
            <span class="mono">{{ proxyAddress }}</span>
            <a-button type="text" size="small" @click="copyText(localProxy, '代理地址')">
              <CopyOutlined />
              复制
            </a-button>
          </div>
          <div v-if="isLan" class="settings-line phone-line">
            <span class="phone-label">手机代理填</span>
            <span class="mono">{{ phoneProxy || '未检测到局域网 IPv4' }}</span>
            <a-button
              v-if="phoneProxy"
              type="text"
              size="small"
              @click="copyText(phoneProxy, '手机代理地址')"
            >
              <CopyOutlined />
              复制
            </a-button>
          </div>
          <p v-if="isLan" class="settings-hint phone-warn">
            开代理后手机如果像“断网”，通常是还没安装或信任根证书：所有 HTTPS 请求都会被拒绝。先装证书，
            或暂时关闭手机代理恢复上网。
          </p>
          <div class="settings-actions">
            <a-button
              v-if="!running"
              type="primary"
              size="small"
              :loading="starting"
              @click="start"
            >
              <PlayCircleOutlined />
              开始录制
            </a-button>
            <a-button v-else danger size="small" :loading="stopping" @click="stop">
              <StopOutlined />
              停止录制
            </a-button>
            <a-button
              size="small"
              :disabled="!running"
              :loading="sysproxyBusy"
              @click="toggleSysproxy"
            >
              <GlobalOutlined />
              {{ status.sysproxyOn ? '恢复系统代理' : '一键接管系统代理' }}
            </a-button>
          </div>
          <p class="settings-hint">
            接管后会修改 Windows 系统代理，停止录制或退出应用时自动恢复原值。手机抓包把监听地址切到
            0.0.0.0，手机和电脑连同一 Wi-Fi，Wi-Fi 代理填上面的「手机代理」地址；HTTPS 还需在手机上安装根证书。
          </p>
        </section>

        <section class="settings-section">
          <h4>
            根证书
            <a-tag :color="ca.installed ? 'success' : 'error'" class="settings-tag">
              {{ ca.installed ? '已安装并信任' : '未安装' }}
            </a-tag>
          </h4>
          <a-spin :spinning="loadingCa">
            <dl class="cert-meta">
              <dt>SHA-1（证书管理器里的「指纹」）</dt>
              <dd class="mono fingerprint">
                {{ ca.thumbprint || '生成后显示' }}
                <a-button
                  v-if="ca.thumbprint"
                  type="text"
                  size="small"
                  class="copy-btn"
                  @click="copyText(ca.thumbprint, 'SHA-1 指纹')"
                >
                  <CopyOutlined />
                </a-button>
              </dd>
              <dt>SHA-256</dt>
              <dd class="mono fingerprint">
                {{ ca.fingerprint || '生成后显示' }}
                <a-button
                  v-if="ca.fingerprint"
                  type="text"
                  size="small"
                  class="copy-btn"
                  @click="copyText(ca.fingerprint, 'SHA-256 指纹')"
                >
                  <CopyOutlined />
                </a-button>
              </dd>
              <dt>有效期至</dt>
              <dd>{{ ca.expiresAt ? formatDateTime(ca.expiresAt) : '—' }}</dd>
            </dl>
          </a-spin>
          <div class="cert-actions">
            <a-button size="small" @click="exportCa">
              <DownloadOutlined />
              导出
            </a-button>
            <a-button
              size="small"
              type="primary"
              ghost
              :loading="installingCa"
              @click="installCa"
            >
              <SafetyCertificateOutlined />
              {{ ca.installed ? '重新安装' : '一键安装' }}
            </a-button>
            <a-button size="small" :loading="loadingCa" @click="refreshCa()">
              <ReloadOutlined />
              检测
            </a-button>
          </div>
          <p class="card-hint" :class="{ 'hint-ok': ca.installed }">
            <template v-if="ca.installed">
              <CheckCircleOutlined /> 当前证书已被系统信任，HTTPS 可以解密。
            </template>
            <template v-else>
              检测不到当前证书。若确定装过，多半装成了旧证书（重新生成后旧证书会失效），点「一键安装」覆盖即可。
            </template>
          </p>
          <a-button type="text" size="small" danger class="regen-btn" @click="regenerateCa">
            <ReloadOutlined />
            重新生成根证书
          </a-button>
        </section>

        <section class="settings-section">
          <h4>使用顺序</h4>
          <ol class="settings-steps">
            <li :class="{ done: running }">
              <b>1</b>
              <div><strong>开始录制</strong><span>启动本机代理 {{ proxyAddress }}</span></div>
            </li>
            <li :class="{ done: ca.installed }">
              <b>2</b>
              <div><strong>安装并信任根证书</strong><span>HTTPS 解密的前提</span></div>
            </li>
            <li :class="{ done: status.sysproxyOn }">
              <b>3</b>
              <div><strong>接管系统代理</strong><span>也可以手动填 {{ localProxy }}</span></div>
            </li>
            <li>
              <b>4</b>
              <div><strong>发起请求</strong><span>浏览器、curl 或手机 App 均可</span></div>
            </li>
          </ol>
          <div class="code-block mono">curl -x http://{{ localProxy }} http://example.com</div>
        </section>

        <section class="settings-section">
          <h4>提示</h4>
          <ul class="tip-list">
            <li>Firefox 使用独立证书库和代理设置，需要单独导入证书、填代理地址。</li>
            <li>HTTP/3（QUIC）不经过代理，必要时关闭浏览器 QUIC。</li>
            <li>部分 App 做了证书固定（pinning），这类流量无法解密。</li>
            <li v-if="isLan">当前监听 0.0.0.0，手机代理填电脑局域网 IP + 端口，不是 127.0.0.1。</li>
            <li>流量仅存内存（5000 条 / 128 MB body），重启应用后清空。</li>
          </ul>
        </section>
      </div>
    </a-drawer>

    <Teleport to="body">
      <div
        v-if="groupMenu"
        class="capture-context-menu"
        :style="{ left: `${groupMenu.x}px`, top: `${groupMenu.y}px` }"
        @click.stop
        @contextmenu.prevent
      >
        <div class="context-head" :title="groupMenu.host">
          <span class="context-host">{{ groupMenu.host }}</span>
          <span class="context-count">{{ groupMenu.ids.length }} 条</span>
        </div>
        <button type="button" :disabled="exporting" @click="exportGroupHar">
          <DownloadOutlined />
          导出该分组 HAR
        </button>
        <button type="button" :disabled="exporting" @click="exportGroupZip">
          <FileZipOutlined />
          导出该分组 ZIP
        </button>
        <button type="button" @click="copyText(groupMenu.host, '域名')">
          <CopyOutlined />
          复制域名
        </button>
        <div class="context-sep" />
        <button type="button" @click="toggleHost(groupMenu.host)">
          <RightOutlined v-if="isCollapsed(groupMenu.host)" />
          <DownOutlined v-else />
          {{ isCollapsed(groupMenu.host) ? '展开该分组' : '折叠该分组' }}
        </button>
      </div>
    </Teleport>

    <a-modal
      v-model:open="repeatOpen"
      title="重发请求"
      :width="780"
      :body-style="{ maxHeight: '66vh', overflow: 'auto' }"
      :mask-closable="false"
      ok-text="发送"
      cancel-text="取消"
      :confirm-loading="repeatBusy"
      @ok="submitRepeat"
    >
      <div class="repeat-form">
        <div class="repeat-line">
          <a-select v-model:value="repeatForm.method" size="small" class="repeat-method">
            <a-select-option v-for="item in repeatMethodOptions" :key="item" :value="item">
              {{ item }}
            </a-select-option>
          </a-select>
          <a-input
            v-model:value="repeatForm.url"
            size="small"
            class="mono"
            placeholder="https://host/path?query"
            @press-enter="submitRepeat"
          />
        </div>

        <div class="repeat-sect-head">
          <span>请求头 <em>{{ repeatForm.headers.length }}</em></span>
          <a-button size="small" type="link" @click="addRepeatHeader">
            <PlusOutlined />
            添加
          </a-button>
        </div>
        <div class="repeat-headers">
          <div
            v-for="(header, index) in repeatForm.headers"
            :key="index"
            class="repeat-header-row"
          >
            <a-input v-model:value="header.name" size="small" class="mono" placeholder="Header" />
            <a-input v-model:value="header.value" size="small" class="mono" placeholder="Value" />
            <a-button
              size="small"
              type="text"
              danger
              title="删除请求头"
              @click="removeRepeatHeader(index)"
            >
              <DeleteOutlined />
            </a-button>
          </div>
          <p v-if="!repeatForm.headers.length" class="repeat-empty">没有请求头</p>
        </div>

        <div class="repeat-sect-head">
          <span>请求体</span>
          <em v-if="!repeatForm.bodyLocked">{{ formatBytes(repeatForm.body.length) }}</em>
        </div>
        <a-textarea
          v-model:value="repeatForm.body"
          :rows="8"
          class="mono repeat-body"
          :disabled="repeatForm.bodyLocked"
          placeholder="无请求体"
        />
        <p v-if="repeatForm.bodyLocked" class="repeat-locked">
          原始请求体为二进制（{{ detail ? formatBytes(detail.requestBody.size) : '' }}），重发时按原样发送。
        </p>
        <p v-else class="repeat-hint">
          {{ running
            ? '当前录制中：重发会经过本机代理并生成一条新记录。'
            : '当前未录制：重发直接发送，响应只在弹窗里展示，不会写入列表。' }}
        </p>

        <div class="repeat-actions">
          <a-button size="small" @click="resetRepeat">恢复原请求</a-button>
        </div>
      </div>
    </a-modal>

    <a-modal
      v-model:open="repeatResultOpen"
      :title="repeatResult ? `重发结果 · ${repeatResult.status} ${repeatResult.statusText}` : '重发结果'"
      :width="860"
      :footer="null"
      @cancel="closeRepeatResult"
    >
      <div v-if="repeatResult" class="repeat-result">
        <div class="repeat-result-meta">
          <span class="method-tag" :class="methodClass(repeatForm.method)">
            {{ repeatForm.method }}
          </span>
          <b :class="statusTone(repeatResult.status)">
            {{ repeatResult.status }} {{ repeatResult.statusText }}
          </b>
          <span>{{ formatDuration(repeatResult.durationMs) }}</span>
          <span>{{ formatBytes(repeatResult.body.size) }}</span>
          <a-tag>未录制，仅本次展示</a-tag>
        </div>
        <p class="repeat-result-url mono">{{ repeatResult.url }}</p>
        <a-tabs size="small">
          <a-tab-pane key="body" tab="响应体">
            <div class="body-block">
              <div class="body-bar">
                <span>{{ repeatResult.body.contentType || '未知类型' }}</span>
                <span>· {{ formatBytes(repeatResult.body.size) }}</span>
                <a-tag v-if="repeatResult.body.truncated" color="warning" class="body-tag">
                  仅保留前 2 MB
                </a-tag>
                <span class="spacer" />
                <a-checkbox
                  v-if="isJsonBody(repeatResult.body)"
                  v-model:checked="repeatResultPretty"
                  class="pretty-check"
                >
                  格式化
                </a-checkbox>
                <a-button
                  size="small"
                  type="text"
                  title="复制响应体"
                  @click="copyText(bodyText(repeatResult.body, repeatResultPretty), '响应体')"
                >
                  <CopyOutlined />
                </a-button>
              </div>
              <div v-if="!repeatResult.body.size" class="body-empty">无响应体</div>
              <img
                v-else-if="
                  repeatResult.body.base64 && repeatResult.body.contentType.startsWith('image/')
                "
                class="body-image"
                :src="bodyDataUrl(repeatResult.body)"
                alt="响应图片预览"
              />
              <pre v-else-if="repeatResult.body.text !== null" class="body-pre">{{
                bodyText(repeatResult.body, repeatResultPretty)
              }}</pre>
              <div v-else class="body-empty">
                二进制内容（{{ formatBytes(repeatResult.body.size) }}），暂不预览
              </div>
            </div>
          </a-tab-pane>
          <a-tab-pane key="headers" tab="响应头">
            <table v-if="repeatResult.headers.length" class="kv-table">
              <thead><tr><th>名称</th><th>值</th></tr></thead>
              <tbody>
                <tr v-for="(header, index) in repeatResult.headers" :key="index">
                  <td class="mono">{{ header.name }}</td>
                  <td class="mono">{{ header.value }}</td>
                </tr>
              </tbody>
            </table>
            <p v-else class="section-empty">没有响应头</p>
          </a-tab-pane>
        </a-tabs>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.capture-page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
}

.capture-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  flex: none;
  padding: 7px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow-sm);
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  min-width: 0;
}

.toolbar-right {
  justify-content: flex-end;
}

.toolbar-divider {
  height: 18px;
  margin: 0 2px;
  border-color: var(--fm-border);
}

.record-btn {
  min-width: 76px;
}

.rec-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #fff;
}

.rec-square {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 2px;
  background: currentColor;
}

.state-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: 0;
}

.state-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--fm-muted);
}

.state-dot.running {
  background: var(--fm-success);
  box-shadow: 0 0 0 3px var(--fm-success-soft);
  animation: capture-pulse 1.6s ease-in-out infinite;
}

.state-dot.starting,
.state-dot.stopping {
  background: var(--fm-warning);
}

.state-dot.error {
  background: var(--fm-danger);
}

@keyframes capture-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.4;
  }
}

.toolbar-meta {
  font-size: 11px;
  color: var(--fm-muted);
}

.toolbar-warn {
  font-size: 11px;
  color: var(--fm-warning);
}

.ca-btn.trusted {
  color: var(--fm-success);
  border-color: color-mix(in srgb, var(--fm-success) 35%, var(--fm-border));
}

.icon-btn {
  width: 28px;
  padding: 0;
}

.capture-alert {
  margin: 0;
  flex: none;
}

.capture-workspace {
  display: flex;
  flex: 1;
  min-height: 0;
  align-items: stretch;
}

.capture-workspace.resizing {
  user-select: none;
  cursor: col-resize;
}

.flow-pane {
  display: flex;
  flex-direction: column;
  flex: none;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px 10px;
  flex: none;
  min-height: 42px;
  padding: 5px 10px;
  border-bottom: 1px solid var(--fm-border);
}

.pane-title {
  display: flex;
  align-items: baseline;
  gap: 7px;
  min-width: 0;
  font-size: 13px;
  font-weight: 650;
  white-space: nowrap;
}

.pane-title .count {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-muted);
}

.pane-title .sub {
  font-size: 11px;
  font-weight: 400;
  color: var(--fm-muted);
}

.pane-title .error-count {
  font-size: 11px;
  font-weight: 500;
  color: var(--fm-danger);
}

.pane-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.tree-filter {
  width: 180px;
  max-width: 46vw;
}

.err-check,
.auto-check,
.pretty-check {
  font-size: 11px;
  white-space: nowrap;
}

.err-check :deep(.ant-checkbox-wrapper),
.auto-check :deep(.ant-checkbox-wrapper) {
  font-size: 11px;
}

.tree-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex: none;
  height: 27px;
  padding: 0 10px;
  font-size: 10.5px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.tree-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.tree-actions :deep(.ant-btn) {
  height: 22px;
  padding: 0 5px;
  font-size: 10.5px;
}

.tree {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.host-group {
  border-bottom: 1px solid color-mix(in srgb, var(--fm-border) 70%, transparent);
}

.host-row {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: 28px;
  padding: 0 9px;
  border: 0;
  background: var(--fm-sidebar);
  text-align: left;
  font-family: inherit;
  font-size: 11.5px;
  font-weight: 650;
  color: var(--fm-text-2);
  cursor: pointer;
}

.host-row:hover {
  background: var(--fm-sidebar-hover);
}

.capture-context-menu {
  position: fixed;
  z-index: 4000;
  width: 230px;
  padding: 5px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow);
  color: var(--fm-text);
  animation: fm-fade-in 0.12s ease;
}

.context-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
  padding: 5px 8px 7px;
  border-bottom: 1px solid var(--fm-border);
}

.context-host {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  font-weight: 650;
}

.context-count {
  flex: none;
  font-size: 10px;
  color: var(--fm-muted);
}

.capture-context-menu button {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 30px;
  padding: 0 8px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  font-family: inherit;
  font-size: 12px;
  color: var(--fm-text-2);
  text-align: left;
  cursor: pointer;
}

.capture-context-menu button:hover {
  background: var(--fm-sidebar-hover);
  color: var(--fm-text);
}

.capture-context-menu button:disabled {
  opacity: 0.6;
  cursor: default;
}

.context-sep {
  height: 1px;
  margin: 4px 6px;
  background: var(--fm-border);
}

.caret {
  width: 10px;
  font-size: 9px;
  color: var(--fm-muted);
  flex: none;
}

.host-icon {
  font-size: 11px;
  color: var(--fm-primary);
  flex: none;
}

.host-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.host-count {
  flex: none;
  min-width: 18px;
  padding: 0 5px;
  font-size: 9.5px;
  font-weight: 600;
  line-height: 14px;
  text-align: center;
  color: var(--fm-muted);
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: 8px;
}

.host-errors {
  flex: none;
  font-size: 9.5px;
  font-weight: 500;
  color: var(--fm-danger);
}

.host-last {
  margin-left: auto;
  flex: none;
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
}

.flow-row {
  display: grid;
  grid-template-columns: 46px minmax(0, 1fr) 36px 54px 58px;
  gap: 6px;
  align-items: center;
  width: 100%;
  height: 27px;
  padding: 0 8px 0 24px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  text-align: left;
  font-family: inherit;
  font-size: 11px;
  color: var(--fm-text-2);
  cursor: pointer;
}

.flow-row:hover {
  background: var(--fm-sidebar-hover);
}

.flow-row.active {
  background: var(--fm-primary-soft);
  border-left-color: var(--fm-primary);
}

.flow-row.failed {
  background: color-mix(in srgb, var(--fm-danger) 5%, transparent);
}

.flow-method {
  justify-self: start;
  min-width: 44px;
  height: 18px;
  padding: 0 5px;
  font-size: 9.5px;
}

.flow-path {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.flow-status {
  font-weight: 650;
  font-variant-numeric: tabular-nums;
}

.flow-time,
.flow-size {
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.st-2xx {
  color: var(--fm-success);
}

.st-3xx {
  color: var(--fm-method-put);
}

.st-4xx {
  color: var(--fm-warning);
}

.st-5xx,
.st-error {
  color: var(--fm-danger);
}

.splitter {
  position: relative;
  width: 10px;
  flex: none;
  cursor: col-resize;
}

.splitter::after {
  content: '';
  position: absolute;
  left: 4px;
  top: 0;
  bottom: 0;
  width: 2px;
  border-radius: 2px;
  background: transparent;
}

.splitter:hover::after,
.splitter.active::after {
  background: var(--fm-primary);
}

.pane-foot {
  display: flex;
  align-items: center;
  gap: 14px;
  flex: none;
  height: 28px;
  padding: 0 10px;
  font-size: 10.5px;
  color: var(--fm-muted);
  border-top: 1px solid var(--fm-border);
}

.foot-proxy {
  margin-left: auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty-state {
  flex: 1;
  min-height: 220px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 22px;
  text-align: center;
}

.empty-icon {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 10px;
  font-size: 21px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: var(--fm-radius-lg);
}

.empty-icon.small {
  width: 38px;
  height: 38px;
  font-size: 18px;
}

.empty-state h2 {
  margin: 0 0 5px;
  font-size: 14px;
  font-weight: 650;
}

.empty-state p {
  margin: 0;
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--fm-muted);
}

.capture-guide {
  align-items: stretch;
}

.guide {
  align-items: stretch;
  justify-content: flex-start;
  text-align: left;
  padding: 18px;
  overflow: auto;
}

.guide-top {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.guide-top .empty-icon {
  flex: none;
  margin-bottom: 0;
}

.guide-top h2 {
  margin: 2px 0 4px;
}

.guide-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
  flex-wrap: wrap;
}

.mini-steps {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
}

.mini-steps li {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 6px 9px;
  font-size: 11px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.mini-steps li b {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 17px;
  height: 17px;
  flex: none;
  font-size: 9.5px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 50%;
}

.mini-steps li.done {
  color: var(--fm-success);
  border-color: color-mix(in srgb, var(--fm-success) 30%, var(--fm-border));
}

.mini-steps li.done b {
  color: var(--fm-success);
  background: var(--fm-success-soft);
}

.code-block {
  margin-top: 10px;
  padding: 7px 9px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--fm-text-2);
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  word-break: break-all;
}

.empty-hint {
  margin-top: 10px;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
  opacity: 0.85;
}

.detail-pane {
  flex: 1;
  min-width: 320px;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 11px 13px 13px;
  overflow: hidden;
}

.detail-pane.detail-empty {
  align-items: center;
  justify-content: center;
  text-align: center;
}

.detail-empty h3 {
  margin: 0 0 5px;
  font-size: 13px;
  font-weight: 650;
}

.detail-empty p {
  max-width: 320px;
  margin: 0;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.detail-head {
  flex: none;
}

.detail-line {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.detail-method {
  min-width: 48px;
  height: 20px;
  font-size: 10px;
}

.detail-status {
  font-size: 12px;
}

.detail-duration {
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.detail-clock {
  font-size: 10.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.detail-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
}

.detail-actions :deep(.ant-btn) {
  height: 24px;
  padding: 0 7px;
  font-size: 11px;
}

.detail-url {
  margin: 7px 0 6px;
  padding: 5px 7px;
  font-size: 10.5px;
  line-height: 1.5;
  color: var(--fm-text-2);
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  word-break: break-all;
}

.detail-error {
  margin: 4px 0 6px;
  flex: none;
}

.detail-tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.detail-tabs :deep(.ant-tabs-nav) {
  margin-bottom: 6px;
  flex: none;
}

.detail-tabs :deep(.ant-tabs-nav .ant-tabs-tab) {
  padding: 5px 0;
  font-size: 11.5px;
}

.detail-tabs :deep(.ant-tabs-content-holder) {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.detail-tabs :deep(.ant-tabs-tabpane) {
  padding-right: 2px;
}

.meta-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1px;
  overflow: hidden;
  background: var(--fm-border);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.meta-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding: 6px 9px;
  background: var(--fm-surface);
}

.meta-item span {
  font-size: 10px;
  color: var(--fm-muted);
}

.meta-item b {
  font-size: 11px;
  font-weight: 600;
  word-break: break-all;
}

.meta-item.wide {
  grid-column: 1 / -1;
}

.overview-hint {
  margin: 8px 0 0;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.detail-section {
  margin-bottom: 13px;
}

.detail-section h4 {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 0 5px;
  font-size: 11.5px;
  font-weight: 650;
}

.section-count {
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
}

.section-empty {
  margin: 0;
  padding: 7px 9px;
  font-size: 10.5px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.kv-table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
  font-size: 10.5px;
}

.kv-table th {
  padding: 4px 7px;
  text-align: left;
  font-size: 10px;
  font-weight: 600;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.kv-table td {
  padding: 4px 7px;
  vertical-align: top;
  color: var(--fm-text-2);
  border: 1px solid var(--fm-border);
  word-break: break-all;
}

.kv-table td:first-child {
  width: 33%;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.body-block {
  overflow: hidden;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.body-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  padding: 4px 7px;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.body-bar .spacer {
  margin-left: auto;
}

.body-bar :deep(.ant-btn) {
  height: 20px;
  padding: 0 4px;
}

.body-tag {
  margin: 0;
  font-size: 9.5px;
  line-height: 15px;
}

.pretty-check :deep(.ant-checkbox-wrapper) {
  font-size: 10px;
}

.body-pre,
.raw-pre {
  margin: 0;
  padding: 8px 9px;
  max-height: 430px;
  overflow: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 10.5px;
  line-height: 1.55;
  color: var(--fm-text-2);
  background: var(--fm-code-bg);
  white-space: pre-wrap;
  word-break: break-word;
}

.body-empty {
  padding: 14px;
  font-size: 10.5px;
  text-align: center;
  color: var(--fm-muted);
}

.body-image {
  display: block;
  max-width: 100%;
  max-height: 430px;
  margin: 8px auto;
  background: var(--fm-surface-alt);
}

.raw-block {
  margin-bottom: 10px;
  overflow: hidden;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.raw-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 2px 4px 2px 9px;
  font-size: 10.5px;
  font-weight: 600;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.raw-head :deep(.ant-btn) {
  height: 20px;
  padding: 0 4px;
}

.raw-pre {
  max-height: 300px;
}

.capture-statusbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
  flex: none;
  padding: 0 4px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.status-ok {
  color: var(--fm-success);
}

.status-bad {
  color: var(--fm-warning);
}

.settings-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.settings-section {
  padding-bottom: 14px;
  border-bottom: 1px solid var(--fm-border);
}

.settings-section:last-child {
  border-bottom: 0;
  padding-bottom: 0;
}

.settings-section h4 {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin: 0 0 8px;
  font-size: 12px;
  font-weight: 650;
}

.settings-tag {
  margin: 0;
  font-size: 10px;
}

.settings-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 7px;
  font-size: 11.5px;
  color: var(--fm-text-2);
}

.settings-port {
  width: 130px;
}

.settings-select {
  width: 190px;
}

.settings-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  margin: 6px 0;
  font-size: 11px;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.settings-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
  margin-top: 8px;
}

.settings-hint {
  margin: 8px 0 0;
  font-size: 10.5px;
  line-height: 1.65;
  color: var(--fm-muted);
}

.phone-line {
  gap: 6px;
}

.phone-line .mono {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
}

.phone-label {
  flex: none;
  color: var(--fm-muted);
}

.phone-warn {
  color: var(--fm-warning);
}

.cert-meta {
  margin: 0;
}

.cert-meta dt {
  margin-top: 5px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.cert-meta dd {
  display: flex;
  align-items: flex-start;
  gap: 3px;
  margin: 2px 0 0;
  font-size: 11px;
  color: var(--fm-text-2);
}

.fingerprint {
  font-size: 10.5px;
  line-height: 1.5;
  word-break: break-all;
}

.copy-btn {
  flex: none;
  height: 18px;
  padding: 0 3px;
  margin-top: -2px;
}

.cert-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 9px;
}

.card-hint {
  margin: 7px 0 0;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.card-hint.hint-ok {
  color: var(--fm-success);
}

.card-hint .anticon {
  margin-right: 3px;
}

.regen-btn {
  height: 22px;
  padding: 0 3px;
  margin-top: 3px;
  font-size: 10.5px;
}

.settings-steps {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.settings-steps li {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 9px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.settings-steps li.done {
  border-color: color-mix(in srgb, var(--fm-success) 30%, var(--fm-border));
}

.settings-steps li > b {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  flex: none;
  font-size: 9.5px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 50%;
}

.settings-steps li.done > b {
  color: var(--fm-success);
  background: var(--fm-success-soft);
}

.settings-steps div {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.settings-steps strong {
  font-size: 11px;
  font-weight: 600;
}

.settings-steps span {
  font-size: 10px;
  color: var(--fm-muted);
}

.tip-list {
  margin: 0;
  padding-left: 15px;
  font-size: 10.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

@media (max-width: 1280px) {
  .toolbar-meta {
    display: none;
  }

  .pane-title .sub {
    display: none;
  }
}

@media (max-width: 1120px) {
  .capture-workspace {
    flex-direction: column;
    gap: 8px;
  }

  .flow-pane {
    width: 100% !important;
    height: 44vh;
    flex: none;
  }

  .splitter {
    display: none;
  }

  .detail-pane {
    flex: 1;
    min-width: 0;
  }
}

@media (max-width: 820px) {
  .capture-toolbar {
    align-items: flex-start;
  }

  .toolbar-right {
    width: 100%;
    justify-content: flex-start;
  }

  .tree-filter {
    width: 150px;
    max-width: 44vw;
  }

  .meta-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .flow-row {
    grid-template-columns: 46px minmax(0, 1fr) 38px 52px;
  }

  .flow-size {
    display: none;
  }

  .detail-actions {
    width: 100%;
    margin-left: 0;
  }
}

.repeat-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.repeat-line {
  display: flex;
  gap: 8px;
}

.repeat-method {
  width: 104px;
  flex: none;
}

.repeat-sect-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.repeat-sect-head em {
  font-style: normal;
  font-weight: 400;
  color: var(--fm-muted);
}

.repeat-headers {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 200px;
  overflow: auto;
  padding-right: 2px;
}

.repeat-header-row {
  display: grid;
  grid-template-columns: 180px minmax(0, 1fr) 30px;
  gap: 6px;
  align-items: center;
}

.repeat-empty {
  margin: 0;
  padding: 8px 0;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.repeat-body {
  font-size: 11px;
}

.repeat-hint,
.repeat-locked {
  margin: 0;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.repeat-locked {
  color: var(--fm-warning);
}

.repeat-actions {
  display: flex;
}

.repeat-result {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.repeat-result-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 11px;
}

.repeat-result-url {
  margin: 0;
  font-size: 10.5px;
  color: var(--fm-muted);
  word-break: break-all;
}

.repeat-result .body-pre {
  max-height: 52vh;
}
</style>
