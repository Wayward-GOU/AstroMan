<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { ThunderboltOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import { copyToClipboard } from '@/utils/clipboard'
import type {
  ClusterMasterSnapshot,
  ClusterNodeInfo,
  ClusterProtocolInfo,
  ClusterSlaveSnapshot,
} from '@/types/cluster'
import type {
  CapturePreset,
  Environment,
  HttpRequestDetail,
  PlanSummary,
  RecordDetailMode,
  SlaTargets,
  StressConfig,
  StressStage,
} from '@/types/models'

const router = useRouter()
const projectStore = useProjectStore()

const activeTab = ref('master')
const protocol = ref<ClusterProtocolInfo | null>(null)
const addresses = ref<string[]>([])
const loading = ref(false)
/** 1 秒心跳：配对码倒计时与快照轮询共用 */
const nowTick = ref(Date.now())
const unlisteners: UnlistenFn[] = []
let timer: number | undefined
let polling = false

/** 并发上限，与后端 stress.rs 的 MAX_CONCURRENT 保持一致 */
const MAX_CONCURRENT = 5000

/* ---------- 主节点：任务创建 ---------- */

const projectId = ref<number | null>(null)
const plans = ref<PlanSummary[]>([])
const planId = ref<number | null>(null)
const planSettings = ref<PlanSettings>({})
const planStages = ref<StressStage[]>([])
const planEnvId = ref<number | null>(null)
const environments = ref<Environment[]>([])
const totalConcurrent = ref(20)
const port = ref(17801)
const masterName = ref('')
const creating = ref(false)

/** 计划保存的 settings 快照，字段与 StressTest.vue 的 settings 一致 */
interface PlanSettings {
  mode?: 'fixed' | 'stages'
  concurrent?: number
  durationSec?: number
  rampUpSec?: number
  thinkTimeMs?: number
  timeoutMs?: number
  connectTimeoutMs?: number
  followRedirects?: boolean
  stopOnErrorRate?: boolean
  stopErrorRate?: number
  stopMinRequests?: number
  insecure?: boolean
  targetRps?: number
  recordDetail?: string
  captureMode?: string
  topN?: number
  slowThresholdMs?: number
  baselineReportId?: number | null
  longRunAck?: boolean
  slaEnabled?: boolean
  slaP95?: number
  slaP99?: number
  slaErrorRate?: number
  slaMinRps?: number
}

const projectOptions = computed(() =>
  projectStore.projects.map((item) => ({ value: item.id, label: item.name })),
)

const planOptions = computed(() =>
  plans.value.map((item) => ({
    value: item.id,
    label: `#${item.id} ${item.name}${item.requestMissing ? '（接口缺失）' : ''}`,
    disabled: item.requestMissing || !item.requestId,
  })),
)

const selectedPlan = computed(
  () => plans.value.find((item) => item.id === planId.value) ?? null,
)

const planEnvName = computed(
  () => environments.value.find((item) => item.id === planEnvId.value)?.name ?? null,
)

const planStageList = computed(() =>
  planStages.value.map((item) => ({
    concurrent: Math.max(1, Math.min(MAX_CONCURRENT, numberOr(item.concurrent, 1))),
    durationSec: Math.max(1, numberOr(item.durationSec, 1)),
  })),
)

const planPeakConcurrent = computed(() =>
  planStageList.value.reduce((peak, item) => Math.max(peak, item.concurrent), 0),
)

const planStagesMode = computed(
  () => planSettings.value.mode === 'stages' && planStageList.value.length > 0,
)

const planDuration = computed(() =>
  planStagesMode.value
    ? planStageList.value.reduce((sum, item) => sum + item.durationSec, 0)
    : Math.max(1, numberOr(planSettings.value.durationSec, 30)),
)

const planModeText = computed(() =>
  planStagesMode.value ? `阶梯 ${planStageList.value.length} 段` : '固定并发',
)

const planSummaryText = computed(() =>
  planStagesMode.value
    ? `峰值 ${planPeakConcurrent.value} 并发，共 ${planDuration.value}s`
    : `${numberOr(planSettings.value.concurrent, 20)} 并发 × ${planDuration.value}s`,
)

/** 一期共享配置不接受阶梯，前端提前说明按峰值折算 */
const planStageAlert = computed(() =>
  planStagesMode.value
    ? `分布式一期只支持固定并发：该阶梯计划将按峰值 ${planPeakConcurrent.value} 并发、总时长 ${planDuration.value}s 执行`
    : '',
)

const canCreate = computed(
  () =>
    !!projectId.value &&
    !!selectedPlan.value &&
    !!selectedPlan.value.requestId &&
    !selectedPlan.value.requestMissing,
)

/* ---------- 主节点：会话 ---------- */

const session = ref<ClusterMasterSnapshot | null>(null)
const busy = ref(false)
const weights = ref<Record<string, number>>({})
const weightDirty = ref<Record<string, boolean>>({})

const ACTIVE_MASTER_STATUS = ['pairing', 'preparing', 'ready', 'running']

const sessionActive = computed(
  () => !!session.value && ACTIVE_MASTER_STATUS.includes(session.value.status),
)

const masterStatusText = computed(() => {
  const status = session.value?.status ?? ''
  return (
    {
      pairing: '等待配对',
      preparing: '下发配置',
      ready: '全部就绪',
      running: '施压中',
      finished: '已完成',
      failed: '已失败',
    }[status] ?? status
  )
})

const masterStatusColor = computed(() => {
  if (!session.value) return 'default'
  if (session.value.status === 'failed') return 'error'
  if (session.value.status === 'finished') return session.value.incomplete ? 'warning' : 'success'
  if (session.value.status === 'running') return 'processing'
  return 'blue'
})

const nodeList = computed<ClusterNodeInfo[]>(() => session.value?.nodes ?? [])

const totalQps = computed(() => nodeList.value.reduce((sum, node) => sum + node.qps, 0))

const participatingNodes = computed(
  () =>
    nodeList.value.filter((node) => node.status !== 'rejected' && node.status !== 'left').length,
)

const pairRemainingMs = computed(() => {
  const expires = session.value?.pairCodeExpiresAt ?? 0
  return Math.max(0, expires - nowTick.value)
})

const pairRemainingText = computed(() => {
  const sec = Math.ceil(pairRemainingMs.value / 1000)
  const mm = String(Math.floor(sec / 60)).padStart(2, '0')
  const ss = String(sec % 60).padStart(2, '0')
  return `${mm}:${ss}`
})

const pairAddresses = computed(() =>
  session.value?.localAddresses?.length ? session.value.localAddresses : addresses.value,
)

const readyToLaunch = computed(
  () =>
    session.value?.status === 'ready' &&
    nodeList.value.length > 0 &&
    nodeList.value.every((node) => ['ready', 'done', 'left'].includes(node.status)),
)

/* ---------- 从节点 ---------- */

const slave = ref<ClusterSlaveSnapshot | null>(null)
const slaveBusy = ref(false)
const slaveForm = reactive({ masterAddr: '', pairCode: '', nodeName: '' })

const SLAVE_ACTIVE_STATUS = ['connecting', 'preparing', 'ready', 'running']

const slaveStatus = computed(() => slave.value?.status ?? 'idle')

const slaveConnected = computed(() => SLAVE_ACTIVE_STATUS.includes(slaveStatus.value))

const slaveStatusText = computed(
  () =>
    (
      {
        idle: '未连接',
        connecting: '连接中',
        preparing: '接收配置',
        ready: '已就绪',
        running: '施压中',
        done: '已完成',
        lost: '已失联',
        left: '已断开',
        rejected: '被拒绝',
      } as Record<string, string>
    )[slaveStatus.value] ?? slaveStatus.value,
)

const slaveStatusColor = computed(() => {
  if (slaveStatus.value === 'rejected' || slaveStatus.value === 'lost') return 'error'
  if (slaveStatus.value === 'done') return 'success'
  if (SLAVE_ACTIVE_STATUS.includes(slaveStatus.value)) return 'processing'
  return 'default'
})

/* ---------- 数据加载 ---------- */

function numberOr(value: unknown, fallback: number): number {
  const num = Number(value)
  return Number.isFinite(num) ? num : fallback
}

function positiveOrNull(value: unknown): number | null {
  const num = Number(value)
  return Number.isFinite(num) && num > 0 ? num : null
}

function recordMode(value: unknown): RecordDetailMode {
  return value === 'full' || value === 'sample' || value === 'off' ? value : 'auto'
}

function capturePreset(value: unknown): CapturePreset {
  return value === 'capacity' || value === 'locate' ? value : 'auto'
}

function loadSlaveCache() {
  try {
    const saved = localStorage.getItem('astroman:cluster-master')
    if (saved) slaveForm.masterAddr = saved
  } catch {
    /* 忽略存储失败 */
  }
}

async function loadProtocol() {
  loading.value = true
  try {
    const [info, items] = await Promise.all([
      api.clusterProtocolInfo(),
      api.clusterLocalAddresses(),
    ])
    protocol.value = info
    addresses.value = items
    port.value = info.defaultPort
  } catch (error) {
    message.error(`读取集群参数失败：${String(error)}`)
  } finally {
    loading.value = false
  }
}

async function loadSessions() {
  try {
    const list = await api.clusterMasterSessions()
    const active = [...list]
      .sort((a, b) => b.createdAt - a.createdAt)
      .find((item) => ACTIVE_MASTER_STATUS.includes(item.status))
    if (active) applySession(active)
  } catch {
    /* 读取历史会话失败不阻塞新建 */
  }
}

async function loadSlaveSnapshot() {
  try {
    const snapshot = await api.clusterSlaveSnapshot()
    if (snapshot.status !== 'idle') applySlave(snapshot)
  } catch {
    /* 没有从节点会话时保持表单 */
  }
}

watch(projectId, async (id) => {
  plans.value = []
  planId.value = null
  planSettings.value = {}
  planStages.value = []
  planEnvId.value = null
  environments.value = []
  if (!id) return
  try {
    const [list, envs] = await Promise.all([api.planList(id), api.envList(id)])
    plans.value = list
    environments.value = envs
    if (!planId.value && list.length) planId.value = list[0].id
  } catch (error) {
    message.error(String(error))
  }
})

watch(planId, async (id) => {
  planSettings.value = {}
  planStages.value = []
  planEnvId.value = null
  if (!id) return
  try {
    const plan = await api.planGet(id)
    const config = plan.config as {
      settings?: PlanSettings
      stages?: StressStage[]
      envId?: number | null
    } | null
    planSettings.value = config?.settings ?? {}
    planStages.value = Array.isArray(config?.stages)
      ? config.stages.map((item) => ({
          concurrent: Math.max(1, numberOr(item.concurrent, 1)),
          durationSec: Math.max(1, numberOr(item.durationSec, 1)),
        }))
      : []
    planEnvId.value = config?.envId ?? null
    totalConcurrent.value = planStagesMode.value
      ? Math.max(1, planPeakConcurrent.value)
      : Math.max(1, numberOr(planSettings.value.concurrent, 20))
  } catch (error) {
    message.error(String(error))
  }
})

/* ---------- 主节点操作 ---------- */

function applySession(snapshot: ClusterMasterSnapshot) {
  session.value = snapshot
  const next: Record<string, number> = { ...weights.value }
  for (const node of snapshot.nodes) {
    if (!weightDirty.value[node.id]) next[node.id] = node.weight
  }
  weights.value = next
}

function setWeight(node: ClusterNodeInfo, value: unknown) {
  const num = Number(value)
  if (!Number.isFinite(num) || num <= 0) return
  weights.value = { ...weights.value, [node.id]: Math.min(10, Math.max(0.1, num)) }
  weightDirty.value = { ...weightDirty.value, [node.id]: true }
}

function buildConfig(detail: HttpRequestDetail): StressConfig {
  const settings = planSettings.value
  const stagesMode = planStagesMode.value
  const total = Math.max(
    1,
    Math.min(
      MAX_CONCURRENT,
      numberOr(totalConcurrent.value, stagesMode ? planPeakConcurrent.value : 20),
    ),
  )
  const sla: SlaTargets | null = settings.slaEnabled
    ? {
        p95Ms: positiveOrNull(settings.slaP95),
        p99Ms: positiveOrNull(settings.slaP99),
        errorRate: positiveOrNull(settings.slaErrorRate),
        minRps: positiveOrNull(settings.slaMinRps),
      }
    : null
  return {
    requestId: detail.id,
    projectId: selectedPlan.value?.projectId ?? projectId.value ?? undefined,
    planId: selectedPlan.value?.id ?? undefined,
    name: selectedPlan.value?.name ?? detail.name,
    method: detail.method,
    url: detail.url,
    headers: detail.headers,
    params: detail.params,
    bodyType: detail.bodyType,
    body: detail.body,
    envId: planEnvId.value,
    insecure: settings.insecure === true,
    timeoutMs: Math.max(1, numberOr(settings.timeoutMs, 30000)),
    connectTimeoutMs: Math.max(1, numberOr(settings.connectTimeoutMs, 10000)),
    followRedirects: settings.followRedirects !== false,
    stopErrorRate: settings.stopOnErrorRate ? numberOr(settings.stopErrorRate, 5) : null,
    stopMinRequests: settings.stopOnErrorRate ? numberOr(settings.stopMinRequests, 100) : null,
    concurrent: total,
    durationSec: Math.max(1, planDuration.value),
    rampUpSec: stagesMode ? 0 : Math.max(0, numberOr(settings.rampUpSec, 0)),
    thinkTimeMs: Math.max(0, numberOr(settings.thinkTimeMs, 0)),
    // 一期主从同步启动只支持单场景，阶梯计划在页面上已折算为峰值并发 + 总时长
    stages: [],
    targetRps: positiveOrNull(settings.targetRps),
    recordDetail: recordMode(settings.recordDetail),
    captureMode: capturePreset(settings.captureMode),
    topN: settings.captureMode === 'locate' ? Math.max(0, numberOr(settings.topN, 200)) : null,
    slowThresholdMs:
      settings.captureMode === 'locate' ? Math.max(0, numberOr(settings.slowThresholdMs, 0)) : null,
    ignoreOverlap: false,
    baselineReportId: settings.baselineReportId ?? null,
    longRunAck: settings.longRunAck === true,
    sla,
  }
}

async function createSession() {
  const plan = selectedPlan.value
  const requestId = plan?.requestId
  if (!plan || !requestId) {
    message.warning('请选择已绑定接口的计划')
    return
  }
  if (session.value) {
    message.warning('请先关闭当前分布式任务')
    return
  }
  creating.value = true
  try {
    const detail = await api.requestGet(requestId)
    if (!detail.url.trim()) {
      message.warning('该计划的接口还没有填写 URL，请先到请求编辑器补全')
      return
    }
    weightDirty.value = {}
    weights.value = {}
    const snapshot = await api.clusterMasterCreate({
      planId: plan.id,
      planName: plan.name,
      config: buildConfig(detail),
      envName: planEnvName.value,
      port: numberOr(port.value, protocol.value?.defaultPort ?? 17801),
      masterName: masterName.value.trim() || null,
    })
    applySession(snapshot)
    message.success('任务已创建，把配对码发给从节点')
  } catch (error) {
    message.error(String(error))
  } finally {
    creating.value = false
  }
}

async function refreshSession() {
  if (!session.value) return
  try {
    applySession(await api.clusterMasterSnapshot(session.value.jobId))
  } catch {
    /* 会话被关闭后停止轮询 */
  }
}

async function prepareSession() {
  if (!session.value) return
  if (!nodeList.value.some((node) => node.role === 'slave')) {
    message.warning('至少需要一个从节点完成配对后再下发配置')
    return
  }
  busy.value = true
  try {
    const payload = nodeList.value.map((node) => ({
      nodeId: node.id,
      weight: weights.value[node.id] ?? node.weight,
    }))
    applySession(await api.clusterMasterPrepare(session.value.jobId, payload))
    message.success('配置已下发，等待从节点预检')
  } catch (error) {
    message.error(String(error))
    await refreshSession()
  } finally {
    busy.value = false
  }
}

function launchSession() {
  if (!session.value) return
  Modal.confirm({
    title: '开始分布式压测？',
    content: `全部节点将在约 2 秒后同步起跑，总并发 ${session.value.totalConcurrent}。`,
    okText: '启动',
    cancelText: '取消',
    async onOk() {
      if (!session.value) return
      busy.value = true
      try {
        applySession(await api.clusterMasterLaunch(session.value.jobId))
        message.success('已下发启动指令')
      } catch (error) {
        message.error(String(error))
      } finally {
        busy.value = false
      }
    },
  })
}

function abortSession() {
  if (!session.value) return
  const jobId = session.value.jobId
  Modal.confirm({
    title: '中止分布式压测？',
    content: '已收到的数据会保留并标注为不完整，报告仍会生成。',
    okText: '中止',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      busy.value = true
      try {
        applySession(await api.clusterMasterAbort(jobId, '主节点手动中止'))
        message.success('已中止，正在合并已收到的数据')
      } catch (error) {
        message.error(String(error))
      } finally {
        busy.value = false
      }
    },
  })
}

function closeSession() {
  if (!session.value) return
  const jobId = session.value.jobId
  const active = sessionActive.value
  Modal.confirm({
    title: active ? '关闭并中止该任务？' : '关闭该任务？',
    content: active
      ? '关闭会先中止正在运行的压测，已收到的数据会并入报告。'
      : '关闭后任务信息与配对码都会失效，已生成的报告不受影响。',
    okText: '关闭',
    okType: active ? 'danger' : 'primary',
    cancelText: '取消',
    async onOk() {
      try {
        await api.clusterMasterClose(jobId)
        session.value = null
        weights.value = {}
        weightDirty.value = {}
        message.success('任务已关闭')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

function openReport() {
  if (session.value?.reportId) router.push(`/reports/${session.value.reportId}`)
}

/* ---------- 从节点操作 ---------- */

function applySlave(snapshot: ClusterSlaveSnapshot) {
  if (snapshot.status === 'idle') {
    slave.value = null
    return
  }
  slave.value = snapshot
}

async function connectSlave() {
  const masterAddr = slaveForm.masterAddr.trim()
  const pairCode = slaveForm.pairCode.trim()
  if (!masterAddr) {
    message.warning('请填写主节点地址')
    return
  }
  if (!/^\d{6}$/.test(pairCode)) {
    message.warning('配对码为 6 位数字')
    return
  }
  slaveBusy.value = true
  try {
    const snapshot = await api.clusterSlaveConnect({
      masterAddr,
      pairCode,
      nodeName: slaveForm.nodeName.trim() || null,
    })
    applySlave(snapshot)
    slaveForm.pairCode = ''
    try {
      localStorage.setItem('astroman:cluster-master', masterAddr)
    } catch {
      /* 忽略存储失败 */
    }
    message.success('已连接主节点，等待下发配置')
  } catch (error) {
    message.error(String(error))
    try {
      applySlave(await api.clusterSlaveSnapshot())
    } catch {
      /* 忽略状态回读失败 */
    }
  } finally {
    slaveBusy.value = false
  }
}

function disconnectSlave() {
  Modal.confirm({
    title: '断开从节点连接？',
    content: '施压中会同时停止本机引擎，主节点会把本节点标记为离开。',
    okText: '断开',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      slaveBusy.value = true
      try {
        await api.clusterSlaveDisconnect()
        slaveForm.pairCode = ''
        await loadSlaveSnapshot()
        message.success('已断开')
      } catch (error) {
        message.error(String(error))
      } finally {
        slaveBusy.value = false
      }
    },
  })
}

/* ---------- 展示辅助 ---------- */

function fmt(value: number | null | undefined, digits = 1): string {
  if (value == null || !Number.isFinite(Number(value))) return '—'
  return Number(value).toFixed(digits)
}

function fmtInt(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(Number(value))) return '—'
  return Math.round(Number(value)).toLocaleString()
}

function clockText(node: ClusterNodeInfo): string {
  if (node.clockOffsetMs == null) return '—'
  const offset = node.clockOffsetMs >= 0 ? `+${fmt(node.clockOffsetMs)}` : fmt(node.clockOffsetMs)
  const uncertainty = node.clockUncertaintyMs == null ? '' : ` ±${fmt(node.clockUncertaintyMs)}`
  return `${offset}${uncertainty} ms`
}

function resourceText(node: ClusterNodeInfo): string {
  if (node.cpu == null && node.memMb == null) return '—'
  const cpu = node.cpu == null ? '—' : `${fmt(node.cpu, 2)} 核`
  const mem = node.memMb == null ? '—' : `${fmtInt(node.memMb)} MB`
  return `${cpu} · ${mem}`
}

const NODE_STATUS_TEXT: Record<string, string> = {
  pairing: '等待配对',
  pairReady: '已配对',
  preparing: '下发配置',
  preflight: '目标预检',
  ready: '就绪',
  running: '施压中',
  done: '完成',
  lost: '失联',
  left: '已离开',
  rejected: '拒绝',
}

const NODE_STATUS_COLOR: Record<string, string> = {
  pairing: 'default',
  pairReady: 'cyan',
  preparing: 'processing',
  preflight: 'processing',
  ready: 'cyan',
  running: 'processing',
  done: 'success',
  lost: 'error',
  left: 'default',
  rejected: 'error',
}

function nodeStatusText(status: string): string {
  return NODE_STATUS_TEXT[status] ?? status
}

function nodeStatusColor(status: string): string {
  return NODE_STATUS_COLOR[status] ?? 'default'
}

function fmtTime(ms: number | null | undefined): string {
  if (!ms) return '—'
  try {
    return new Date(ms).toLocaleTimeString('zh-CN', { hour12: false })
  } catch {
    return '—'
  }
}

const nodeColumns = [
  { title: '节点', key: 'name', width: 220 },
  { title: '角色', key: 'role', width: 76 },
  { title: '状态', key: 'status', width: 110 },
  { title: '权重', key: 'weight', width: 110 },
  { title: '并发', dataIndex: 'concurrent', key: 'concurrent', width: 76, align: 'right' as const },
  { title: 'QPS', key: 'qps', width: 86, align: 'right' as const },
  { title: '时钟偏移', key: 'clock', width: 128, align: 'right' as const },
  {
    title: '丢弃采样',
    dataIndex: 'droppedSamples',
    key: 'dropped',
    width: 92,
    align: 'right' as const,
  },
  { title: '资源', key: 'resource', width: 150 },
]

async function copy(text: string) {
  try {
    await copyToClipboard(text)
    message.success(`已复制 ${text}`)
  } catch {
    message.warning('复制失败，请手动选择地址')
  }
}

/* ---------- 生命周期 ---------- */

async function tick() {
  nowTick.value = Date.now()
  if (polling) return
  polling = true
  try {
    if (session.value && sessionActive.value) await refreshSession()
    if (slaveConnected.value) {
      try {
        applySlave(await api.clusterSlaveSnapshot())
      } catch {
        /* 忽略瞬时失败 */
      }
    }
  } finally {
    polling = false
  }
}

onMounted(async () => {
  loadSlaveCache()
  try {
    unlisteners.push(
      await listen<ClusterMasterSnapshot>('cluster:master', (event) => {
        if (session.value && event.payload.jobId === session.value.jobId) applySession(event.payload)
      }),
    )
    unlisteners.push(
      await listen<ClusterSlaveSnapshot>('cluster:slave', (event) => {
        if (event.payload.status === 'idle') return
        applySlave(event.payload)
      }),
    )
  } catch (error) {
    console.error('集群事件监听失败', error)
  }
  timer = window.setInterval(() => void tick(), 1000)
  void loadProtocol()
  if (!projectStore.projects.length) {
    try {
      await projectStore.loadProjects()
    } catch (error) {
      message.error(String(error))
    }
  }
  if (!projectId.value && projectStore.projects.length) {
    projectId.value = projectStore.projects[0].id
  }
  void loadSessions()
  void loadSlaveSnapshot()
})

onBeforeUnmount(() => {
  for (const off of unlisteners) off()
  if (timer) window.clearInterval(timer)
  // 关闭页面不中断任务：主从会话都归后端进程，页面只是视图
})
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Cluster"
      title="分布式压测"
      subtitle="多机施压一期：手动 IP + 配对码 · 时钟对齐 · 采样合并 · 复用现有报告"
    >
      <a-tag>V1.2 一期</a-tag>
    </PageHeader>

    <a-alert type="info" show-icon class="boundary">
      <template #message>一期边界</template>
      <template #description>
        主节点负责调度、时钟对齐、采样合并与出报告，同时作为施压节点参与压测；从节点反向连接主节点。
        本期不做 mDNS 自动发现、断线重连、跨网段与故障后并发重分配。
      </template>
    </a-alert>

    <a-tabs v-model:active-key="activeTab" size="small">
      <a-tab-pane key="master" tab="主节点">
        <a-row v-if="!session" :gutter="16">
          <a-col :xs="24" :lg="15">
            <a-card size="small" title="新建分布式任务">
              <a-form layout="vertical" class="cluster-form">
                <a-row :gutter="12">
                  <a-col :xs="24" :md="10">
                    <a-form-item label="项目">
                      <a-select
                        v-model:value="projectId"
                        :options="projectOptions"
                        show-search
                        option-filter-prop="label"
                        placeholder="选择项目"
                      />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="24" :md="14">
                    <a-form-item label="压测计划">
                      <a-select
                        v-model:value="planId"
                        :options="planOptions"
                        show-search
                        option-filter-prop="label"
                        placeholder="选择已绑定接口的计划"
                        :disabled="!projectId"
                      />
                    </a-form-item>
                  </a-col>
                </a-row>

                <div v-if="selectedPlan" class="plan-preview">
                  <div class="preview-row">
                    <span class="preview-label">目标</span>
                    <span class="mono preview-value">
                      {{ selectedPlan.method || 'GET' }}
                      {{ selectedPlan.resolvedUrl || selectedPlan.urlTemplate || '—' }}
                    </span>
                  </div>
                  <div class="preview-row">
                    <span class="preview-label">加压</span>
                    <span>{{ planModeText }} · {{ planSummaryText }}</span>
                  </div>
                  <div class="preview-row">
                    <span class="preview-label">环境</span>
                    <span>{{ planEnvName || '未绑定环境（变量需在各节点本地同名环境中存在）' }}</span>
                  </div>
                </div>
                <a-alert
                  v-if="planStageAlert"
                  type="warning"
                  show-icon
                  class="inline-alert"
                  :message="planStageAlert"
                />

                <a-row :gutter="12">
                  <a-col :xs="12" :md="6">
                    <a-form-item label="总并发">
                      <a-input-number
                        v-model:value="totalConcurrent"
                        :min="1"
                        :max="MAX_CONCURRENT"
                        :step="10"
                        class="w-full"
                      />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="12" :md="6">
                    <a-form-item label="集群端口">
                      <a-input-number v-model:value="port" :min="1024" :max="65535" class="w-full" />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="24" :md="12">
                    <a-form-item label="主节点名称（可选）">
                      <a-input v-model:value="masterName" placeholder="默认取本机计算机名" allow-clear />
                    </a-form-item>
                  </a-col>
                </a-row>

                <a-button type="primary" :loading="creating" :disabled="!canCreate" @click="createSession">
                  <ThunderboltOutlined />
                  创建任务并生成配对码
                </a-button>
                <p class="hint">
                  总并发会按节点权重拆分，主节点也作为一个施压节点参与；一期只支持固定并发单场景。
                </p>
              </a-form>
            </a-card>
          </a-col>
          <a-col :xs="24" :lg="9">
            <a-card size="small" title="本机配对地址" :loading="loading">
              <div class="addr-list">
                <a-tag
                  v-for="item in addresses"
                  :key="item"
                  class="addr-tag"
                  @click="copy(`${item}:${protocol?.defaultPort ?? 17801}`)"
                >
                  {{ item }}:{{ protocol?.defaultPort ?? 17801 }}
                </a-tag>
              </div>
              <p class="hint">
                创建任务后以任务实际端口为准；从节点在另一台机器的「从节点」页填写该地址与配对码，点击地址可复制。
              </p>
            </a-card>
          </a-col>
        </a-row>

        <template v-else>
          <a-card size="small" class="session-card">
            <template #title>
              <div class="session-title">
                <span>{{ session.name }}</span>
                <a-tag :color="masterStatusColor">{{ masterStatusText }}</a-tag>
                <a-tag v-if="session.incomplete" color="orange">结果不完整</a-tag>
              </div>
            </template>
            <template #extra>
              <a-space size="small">
                <a-button
                  v-if="session.status === 'pairing'"
                  type="primary"
                  size="small"
                  :loading="busy"
                  @click="prepareSession"
                >
                  下发配置
                </a-button>
                <a-button
                  v-if="session.status === 'ready'"
                  type="primary"
                  size="small"
                  :loading="busy"
                  :disabled="!readyToLaunch"
                  @click="launchSession"
                >
                  同步启动
                </a-button>
                <a-button v-if="sessionActive" danger size="small" :loading="busy" @click="abortSession">
                  中止
                </a-button>
                <a-button v-if="session.reportId" size="small" @click="openReport">
                  查看报告 #{{ session.reportId }}
                </a-button>
                <a-button size="small" @click="closeSession">关闭任务</a-button>
              </a-space>
            </template>

            <div class="metric-row">
              <div class="metric">
                <span class="metric-label">总并发</span>
                <span class="metric-value mono">{{ session.totalConcurrent }}</span>
              </div>
              <div class="metric">
                <span class="metric-label">已用时</span>
                <span class="metric-value mono">{{ session.elapsedSec }}s</span>
              </div>
              <div class="metric">
                <span class="metric-label">合计 QPS</span>
                <span class="metric-value mono">{{ fmt(totalQps) }}</span>
              </div>
              <div class="metric">
                <span class="metric-label">参与节点</span>
                <span class="metric-value mono">{{ participatingNodes }} / {{ nodeList.length }}</span>
              </div>
              <div class="metric">
                <span class="metric-label">报告</span>
                <span class="metric-value mono">{{ session.reportId ? `#${session.reportId}` : '—' }}</span>
              </div>
            </div>

            <div class="target-line">
              <span class="preview-label">目标</span>
              <span class="mono">{{ session.method }} {{ session.target }}</span>
            </div>
            <div class="target-line">
              <span class="preview-label">起跑</span>
              <span class="mono">
                {{ session.startAtMs ? fmtTime(session.startAtMs) : '等待全部节点就绪' }}
                · 任务 #{{ session.jobId }} · 端口 {{ session.port }}
              </span>
            </div>

            <a-alert
              v-if="session.message"
              class="inline-alert"
              :type="session.incomplete ? 'warning' : 'info'"
              show-icon
              :message="session.message"
            />
            <a-alert
              v-else-if="session.status === 'pairing'"
              class="inline-alert"
              type="info"
              show-icon
              message="等待从节点加入"
              description="从节点完成配对与时钟对齐后，点「下发配置」按权重拆分并发并做目标预检。"
            />
            <a-alert
              v-else-if="session.status === 'ready'"
              class="inline-alert"
              type="success"
              show-icon
              message="全部节点已就绪"
              description="点「同步启动」后各节点按统一时间点起跑；主节点本地引擎也会同时开始。"
            />
          </a-card>

          <a-card v-if="session.pairCode" size="small" title="配对码" class="pair-card">
            <div class="pair-body">
              <div class="pair-left">
                <div class="pair-code mono">{{ session.pairCode }}</div>
                <div class="pair-hint">
                  单次使用 · 剩余
                  <b class="mono">{{ pairRemainingText }}</b>
                  · 端口 {{ session.port }}
                </div>
              </div>
              <div class="pair-right">
                <div class="addr-list">
                  <a-tag
                    v-for="item in pairAddresses"
                    :key="item"
                    class="addr-tag"
                    @click="copy(`${item}:${session.port}`)"
                  >
                    {{ item }}:{{ session.port }}
                  </a-tag>
                </div>
                <p class="hint">把地址与配对码发给从节点；点击地址可复制。</p>
              </div>
            </div>
          </a-card>
          <a-alert
            v-else-if="session.status === 'pairing'"
            class="inline-alert"
            type="info"
            show-icon
            message="配对码已过期"
            description="新从节点需要新配对码：关闭任务后重新创建即可；已配对节点仍可直接下发配置。"
          />

          <a-card size="small" class="node-card">
            <template #title>
              <div class="session-title">
                <span>节点</span>
                <span class="card-sub">准备时按权重拆分总并发</span>
              </div>
            </template>
            <a-table
              :columns="nodeColumns"
              :data-source="nodeList"
              row-key="id"
              size="small"
              :pagination="false"
            >
              <template #bodyCell="{ column, record }">
                <template v-if="column.key === 'name'">
                  <div class="node-name">{{ record.name }}</div>
                  <div class="node-addr mono">{{ record.addr || '—' }}</div>
                </template>
                <template v-else-if="column.key === 'role'">
                  <a-tag :color="record.role === 'master' ? 'gold' : 'blue'">
                    {{ record.role === 'master' ? '主节点' : '从节点' }}
                  </a-tag>
                </template>
                <template v-else-if="column.key === 'status'">
                  <a-tag :color="nodeStatusColor(record.status)" :title="record.error || undefined">
                    {{ nodeStatusText(record.status) }}
                  </a-tag>
                  <div v-if="record.error" class="node-error">{{ record.error }}</div>
                </template>
                <template v-else-if="column.key === 'weight'">
                  <a-input-number
                    v-if="session.status === 'pairing'"
                    :value="weights[record.id] ?? record.weight"
                    :min="0.1"
                    :max="10"
                    :step="0.1"
                    size="small"
                    class="weight-input"
                    @update:value="setWeight(record, $event)"
                  />
                  <span v-else class="mono">{{ fmt(record.weight, 2) }}</span>
                </template>
                <template v-else-if="column.key === 'qps'">
                  <span class="mono">{{ fmt(record.qps) }}</span>
                </template>
                <template v-else-if="column.key === 'clock'">
                  <span class="mono">{{ clockText(record) }}</span>
                </template>
                <template v-else-if="column.key === 'dropped'">
                  <span class="mono" :class="{ 'node-dropped': record.droppedSamples > 0 }">
                    {{ fmtInt(record.droppedSamples) }}
                  </span>
                </template>
                <template v-else-if="column.key === 'resource'">
                  <span class="mono">{{ resourceText(record) }}</span>
                </template>
              </template>
            </a-table>
            <div v-if="!nodeList.length" class="inline-empty">还没有节点</div>
            <p v-else-if="session.status === 'pairing'" class="hint">
              从节点加入后会出现在这里；权重可调，主节点与从节点一视同仁。
            </p>
          </a-card>
        </template>
      </a-tab-pane>

      <a-tab-pane key="slave" tab="从节点">
        <a-row :gutter="16">
          <a-col :xs="24" :lg="11">
            <a-card size="small" title="加入主节点">
              <a-form layout="vertical" class="cluster-form">
                <a-form-item label="主节点地址">
                  <a-input
                    v-model:value="slaveForm.masterAddr"
                    placeholder="192.168.1.10:17801"
                    :disabled="slaveConnected"
                  />
                </a-form-item>
                <a-form-item label="配对码">
                  <a-input
                    v-model:value="slaveForm.pairCode"
                    placeholder="6 位数字"
                    :maxlength="6"
                    :disabled="slaveConnected"
                    class="mono"
                  />
                </a-form-item>
                <a-form-item label="节点名称（可选）">
                  <a-input
                    v-model:value="slaveForm.nodeName"
                    placeholder="默认取本机计算机名"
                    :disabled="slaveConnected"
                    allow-clear
                  />
                </a-form-item>
                <a-space>
                  <a-button
                    type="primary"
                    :loading="slaveBusy"
                    :disabled="slaveConnected"
                    @click="connectSlave"
                  >
                    加入集群
                  </a-button>
                  <a-button v-if="slaveConnected" danger :loading="slaveBusy" @click="disconnectSlave">
                    断开连接
                  </a-button>
                </a-space>
                <p class="hint">
                  配对码由主节点生成，5 分钟内有效且只能使用一次。本机按「环境名」在本地查找同名环境，
                  变量值不会从主节点下发。
                </p>
              </a-form>
            </a-card>
          </a-col>
          <a-col :xs="24" :lg="13">
            <a-card size="small" title="本机会话">
              <template v-if="slave">
                <div class="slave-status">
                  <a-tag :color="slaveStatusColor">{{ slaveStatusText }}</a-tag>
                  <span class="mono slave-addr">{{ slave.masterAddr }}</span>
                  <span v-if="slave.nodeId" class="hint mono">节点 {{ slave.nodeId }}</span>
                </div>
                <a-descriptions bordered size="small" :column="2" class="slave-desc">
                  <a-descriptions-item label="节点名称">{{ slave.nodeName || '—' }}</a-descriptions-item>
                  <a-descriptions-item label="加入时间">{{ fmtTime(slave.joinedAt) }}</a-descriptions-item>
                  <a-descriptions-item label="计划">{{ slave.planName || '等待下发' }}</a-descriptions-item>
                  <a-descriptions-item label="分配并发">{{ slave.concurrent || '—' }}</a-descriptions-item>
                  <a-descriptions-item label="目标" :span="2">
                    <span class="mono">{{ slave.method || '—' }} {{ slave.target || '' }}</span>
                  </a-descriptions-item>
                  <a-descriptions-item label="当前 QPS">
                    <span class="mono">{{ fmt(slave.qps) }}</span>
                  </a-descriptions-item>
                  <a-descriptions-item label="已用时">
                    <span class="mono">{{ slave.elapsedSec }}s</span>
                  </a-descriptions-item>
                  <a-descriptions-item label="起跑时间" :span="2">
                    {{ slave.startAtMs ? fmtTime(slave.startAtMs) : '等待主节点指令' }}
                  </a-descriptions-item>
                </a-descriptions>
                <a-alert
                  v-if="slave.missingVars.length"
                  class="inline-alert"
                  type="warning"
                  show-icon
                  message="本机环境缺少变量"
                  :description="slave.missingVars.join('、')"
                />
                <a-alert
                  v-if="slave.error"
                  class="inline-alert"
                  type="error"
                  show-icon
                  :message="slave.error"
                />
                <a-button v-if="slaveConnected" danger size="small" :loading="slaveBusy" @click="disconnectSlave">
                  断开连接
                </a-button>
              </template>
              <a-empty v-else description="尚未加入主节点" />
            </a-card>
          </a-col>
        </a-row>
      </a-tab-pane>

      <a-tab-pane key="protocol" tab="协议参数">
        <a-descriptions bordered size="small" :column="2">
          <a-descriptions-item label="协议版本">{{ protocol?.version ?? '—' }}</a-descriptions-item>
          <a-descriptions-item label="默认端口">{{ protocol?.defaultPort ?? '—' }}</a-descriptions-item>
          <a-descriptions-item label="配对码有效期">
            {{ protocol ? `${Math.round(protocol.pairCodeTtlSec / 60)} 分钟` : '—' }}
          </a-descriptions-item>
          <a-descriptions-item label="配对码位数">{{ protocol?.pairCodeDigits ?? '—' }}</a-descriptions-item>
          <a-descriptions-item label="心跳间隔">
            {{ protocol ? `${protocol.heartbeatSec} 秒` : '—' }}
          </a-descriptions-item>
          <a-descriptions-item label="失联判定">
            {{ protocol ? `连续丢 ${protocol.heartbeatMisses} 次（约 ${protocol.heartbeatSec * protocol.heartbeatMisses} 秒）` : '—' }}
          </a-descriptions-item>
          <a-descriptions-item label="时钟对齐">{{ protocol ? `${protocol.clockRounds} 轮往返` : '—' }}</a-descriptions-item>
          <a-descriptions-item label="采样丢弃阈值">
            {{ protocol ? `误差 > ${protocol.clockDropUncertaintyMs} ms` : '—' }}
          </a-descriptions-item>
          <a-descriptions-item label="单任务节点上限">{{ protocol?.maxNodes ?? '—' }}</a-descriptions-item>
        </a-descriptions>
        <p class="hint">
          所有参数由主节点下发，从节点只读取；改动需升级主从双方版本，避免协议不兼容。
        </p>
      </a-tab-pane>
    </a-tabs>
  </div>
</template>

<style scoped>
.boundary {
  margin-bottom: 14px;
}

.cluster-form {
  margin-bottom: 4px;
}

.w-full {
  width: 100%;
}

.plan-preview {
  margin: 2px 0 12px;
  padding: 10px 12px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.preview-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 12px;
  line-height: 1.6;
}

.preview-label {
  flex: 0 0 40px;
  color: var(--fm-muted);
}

.preview-value {
  word-break: break-all;
}

.inline-alert {
  margin: 10px 0;
}

.addr-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.addr-tag {
  cursor: pointer;
  font-family: var(--fm-font-mono);
  margin: 0;
}

.hint {
  margin: 10px 0 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.session-card {
  margin-bottom: 12px;
}

.session-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}

.card-sub {
  font-size: 12px;
  font-weight: 400;
  color: var(--fm-muted);
}

.metric-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 28px;
  padding-bottom: 10px;
  border-bottom: 1px dashed var(--fm-border);
}

.metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 64px;
}

.metric-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.metric-value {
  font-size: 18px;
  font-weight: 600;
  line-height: 1.2;
}

.target-line {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-top: 8px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text);
}

.target-line .mono {
  word-break: break-all;
}

.pair-card {
  margin-bottom: 12px;
}

.pair-body {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 20px;
}

.pair-left {
  flex: 0 0 auto;
}

.pair-code {
  font-size: 30px;
  font-weight: 700;
  letter-spacing: 8px;
  line-height: 1.2;
  color: var(--fm-primary);
}

.pair-hint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--fm-muted);
}

.pair-right {
  flex: 1 1 260px;
  min-width: 220px;
}

.node-card {
  margin-bottom: 12px;
}

.node-name {
  line-height: 1.5;
}

.node-addr {
  font-size: 12px;
  color: var(--fm-muted);
  word-break: break-all;
}

.node-error {
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--fm-danger);
}

.node-dropped {
  color: var(--fm-warning);
}

.weight-input {
  width: 82px;
}

.inline-empty {
  padding: 18px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.slave-status {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 12px;
}

.slave-addr {
  color: var(--fm-muted);
}

.slave-desc {
  margin-bottom: 12px;
}
</style>
