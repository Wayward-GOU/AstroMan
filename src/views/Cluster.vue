<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { ThunderboltOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import { copyToClipboard } from '@/utils/clipboard'
import type {
  ClusterDevice,
  ClusterMasterSnapshot,
  ClusterNodeInfo,
  ClusterProtocolInfo,
  ClusterSampleEvent,
  ClusterSlaveSnapshot,
  DiscoveredMaster,
  Orchestration,
  OrchestrationPhase,
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
const plansLoading = ref(false)
const planId = ref<number | null>(null)
const planSettings = ref<PlanSettings>({})
const planStages = ref<StressStage[]>([])
const planEnvId = ref<number | null>(null)
const environments = ref<Environment[]>([])
const totalConcurrent = ref(20)
const port = ref(17801)
const pairTtlMin = ref(5)
const masterName = ref('')
const listenAddr = ref('0.0.0.0')
const creating = ref(false)

/** 并发编排：固定并发 / 复用计划的阶梯 / 自定义 ramp、hold、step */
type OrchestrationMode = 'off' | 'plan' | 'custom'
const orchestrationMode = ref<OrchestrationMode>('off')
const MAX_ORCHESTRATION_PHASES = 32
const customPhases = ref<OrchestrationPhase[]>([
  { kind: 'ramp', durationSec: 30, fromConcurrent: 1, toConcurrent: 20 },
  { kind: 'hold', durationSec: 60, concurrent: 20 },
])

function newPhase(kind: OrchestrationPhase['kind']): OrchestrationPhase {
  const peak = Math.max(1, numberOr(totalConcurrent.value, 20))
  return kind === 'ramp'
    ? { kind, durationSec: 30, fromConcurrent: 1, toConcurrent: peak }
    : { kind, durationSec: 60, concurrent: peak }
}

function addPhase() {
  if (customPhases.value.length >= MAX_ORCHESTRATION_PHASES) return
  const last = customPhases.value[customPhases.value.length - 1]
  const kind = last?.kind === 'ramp' ? 'hold' : 'step'
  customPhases.value.push(newPhase(kind))
}

function removePhase(index: number) {
  customPhases.value.splice(index, 1)
}

const PHASE_OPTIONS = [
  { value: 'ramp', label: '爬坡 ramp' },
  { value: 'hold', label: '保持 hold' },
  { value: 'step', label: '阶梯 step' },
]

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

/** 创建任务前的地址展示跟随用户填写的端口，避免与任务实际端口不一致 */
const displayPort = computed(() => numberOr(port.value, protocol.value?.defaultPort ?? 17801))

const pairTtlMinLimit = computed(() =>
  Math.max(1, Math.round((protocol.value?.minPairCodeTtlSec ?? 60) / 60)),
)

const pairTtlMaxLimit = computed(() =>
  Math.max(pairTtlMinLimit.value, Math.round((protocol.value?.maxPairCodeTtlSec ?? 3600) / 60)),
)

const pairTtlSeconds = computed(
  () =>
    Math.min(
      pairTtlMaxLimit.value,
      Math.max(pairTtlMinLimit.value, Math.round(numberOr(pairTtlMin.value, 5))),
    ) * 60,
)

const pairTtlText = computed(() => {
  const sec = session.value?.pairCodeTtlSec ?? pairTtlSeconds.value
  return `${Math.max(1, Math.round(sec / 60))} 分钟`
})

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

const planStageAlert = computed(() =>
  planStagesMode.value
    ? `该阶梯计划将转换为 ${planStageList.value.length} 个 step 阶段编排下发，各节点按阶段同步切换并发`
    : '',
)

function normalizedPhase(phase: OrchestrationPhase): OrchestrationPhase | null {
  const durationSec = Math.floor(numberOr(phase.durationSec, 0))
  if (durationSec <= 0) return null
  if (phase.kind === 'ramp') {
    const from = Math.floor(numberOr(phase.fromConcurrent, 0))
    const to = Math.floor(numberOr(phase.toConcurrent, 0))
    if (from <= 0 || to <= 0) return null
    return { kind: 'ramp', durationSec, fromConcurrent: from, toConcurrent: to, concurrent: null }
  }
  const concurrent = Math.floor(numberOr(phase.concurrent, 0))
  if (concurrent <= 0) return null
  return { kind: phase.kind, durationSec, fromConcurrent: null, toConcurrent: null, concurrent }
}

const orchestrationPhases = computed<OrchestrationPhase[]>(() => {
  if (orchestrationMode.value === 'off') return []
  if (orchestrationMode.value === 'plan') {
    return planStageList.value.map((stage) => ({
      kind: 'step' as const,
      durationSec: stage.durationSec,
      concurrent: stage.concurrent,
    }))
  }
  return customPhases.value
})

const orchestrationPlan = computed<Orchestration | null>(() => {
  const phases = orchestrationPhases.value
  if (!phases.length) return null
  const normalized = phases.map(normalizedPhase)
  if (normalized.some((phase) => phase == null)) return null
  return { version: 1, phases: normalized as OrchestrationPhase[], assignment: 'weighted' }
})

const orchestrationErrors = computed<string[]>(() => {
  if (orchestrationMode.value === 'off') return []
  const phases = orchestrationPhases.value
  const errors: string[] = []
  if (!phases.length) errors.push('至少需要 1 个编排阶段')
  if (phases.length > MAX_ORCHESTRATION_PHASES) {
    errors.push(`最多 ${MAX_ORCHESTRATION_PHASES} 个阶段，当前 ${phases.length} 个`)
  }
  phases.forEach((phase, index) => {
    if (!normalizedPhase(phase)) {
      errors.push(
        phase.kind === 'ramp'
          ? `第 ${index + 1} 个 ramp 阶段需填写大于 0 的起止并发与时长`
          : `第 ${index + 1} 个 ${phase.kind} 阶段需填写大于 0 的并发与时长`,
      )
    }
  })
  return errors
})

const orchestrationPeak = computed(() =>
  orchestrationPlan.value ? orchestrationPeakFor(orchestrationPlan.value.phases) : 0,
)

const orchestrationDuration = computed(() =>
  orchestrationPlan.value?.phases.reduce((sum, phase) => sum + phase.durationSec, 0) ?? 0,
)

const effectiveDuration = computed(() =>
  orchestrationPlan.value ? orchestrationDuration.value : planDuration.value,
)

watch(orchestrationPeak, (peak) => {
  if (peak > 0) totalConcurrent.value = Math.min(MAX_CONCURRENT, peak)
})

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

type Tone = 'idle' | 'info' | 'ok' | 'live' | 'warn' | 'bad'

const masterTone = computed<Tone>(() => {
  const status = session.value?.status
  if (!status) return 'idle'
  if (status === 'failed') return 'bad'
  if (status === 'finished') return session.value?.incomplete ? 'warn' : 'ok'
  if (status === 'running') return 'live'
  return 'info'
})

const STAGE_STEPS = ['配对', '下发', '就绪', '施压', '完成']
const STAGE_INDEX: Record<string, number> = {
  pairing: 0,
  preparing: 1,
  ready: 2,
  running: 3,
  finished: 4,
}
const stageReached = ref(0)

const stageCurrent = computed(() => {
  const status = session.value?.status ?? ''
  if (status === 'failed') return stageReached.value
  return STAGE_INDEX[status] ?? 0
})

const nodeList = computed<ClusterNodeInfo[]>(() => session.value?.nodes ?? [])

const hasSlave = computed(() => nodeList.value.some((node) => node.role === 'slave'))

/**
 * 配对阶段的并发预览：与后端 `allocate_concurrency` 同口径，
 * 每个节点先保底 1 并发，剩余按权重分配，余数从大到小补齐。
 */
const plannedConcurrent = computed<Record<string, number>>(() => {
  const nodes = nodeList.value
  if (!nodes.length) return {}
  const total = Math.max(1, numberOr(session.value?.totalConcurrent, nodes.length))
  const items = nodes.map((node) => ({
    id: node.id,
    weight: Math.max(0.1, numberOr(weights.value[node.id] ?? node.weight, 1)),
  }))
  const sum = items.reduce((acc, item) => acc + item.weight, 0)
  const remaining = Math.max(0, total - items.length)
  const exact = items.map((item) => {
    const share = remaining * item.weight / sum
    const base = Math.floor(share)
    return { id: item.id, count: 1 + base, fraction: share - base }
  })
  let leftover = Math.max(0, total - exact.reduce((acc, item) => acc + item.count, 0))
  const byFraction = [...exact].sort((a, b) => b.fraction - a.fraction)
  let index = 0
  while (leftover > 0 && byFraction.length) {
    byFraction[index % byFraction.length].count += 1
    leftover -= 1
    index += 1
  }
  return Object.fromEntries(exact.map((item) => [item.id, item.count]))
})

const tooFewConcurrent = computed(
  () => nodeList.value.length > 0 && (session.value?.totalConcurrent ?? 0) < nodeList.value.length,
)

const startCountdownSec = computed(() => {
  const at = session.value?.startAtMs ?? 0
  return at > nowTick.value ? Math.ceil((at - nowTick.value) / 1000) : 0
})

const totalQps = computed(() => nodeList.value.reduce((sum, node) => sum + node.qps, 0))

/* ---------- 实时曲线：每秒采样按秒归集，避免各节点上报时刻不同导致错位 ---------- */

const LIVE_WINDOW = 120
const liveSamplesBySec = ref<Record<number, Record<string, number>>>({})

function resetLiveSamples() {
  liveSamplesBySec.value = {}
}

const liveElapsed = computed(() =>
  Object.keys(liveSamplesBySec.value)
    .map(Number)
    .sort((a, b) => a - b)
    .slice(-LIVE_WINDOW),
)

const liveLabels = computed(() => liveElapsed.value.map((sec) => `${sec}s`))

const NODE_COLORS = [
  'var(--fm-primary)',
  'var(--fm-method-put)',
  'var(--fm-warning)',
  'var(--fm-purple)',
  'var(--fm-danger)',
]

const liveSeries = computed(() => {
  if (!liveElapsed.value.length) return []
  return nodeList.value.map((node, index) => ({
    name: node.name || node.id,
    color: NODE_COLORS[index % NODE_COLORS.length],
    values: liveElapsed.value.map((sec) => liveSamplesBySec.value[sec]?.[node.id] ?? 0),
  }))
})

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

/* ---------- 设备列表与 mDNS 发现（R9） ---------- */

const devices = ref<ClusterDevice[]>([])
const devicesLoading = ref(false)
const discoveredMasters = ref<DiscoveredMaster[]>([])
const browsingMasters = ref(false)

const deviceColumns = [
  { title: '设备', key: 'name', width: 190 },
  { title: '最近地址', key: 'addr', width: 170 },
  { title: '能力', key: 'capacity', width: 180 },
  { title: '最近在线', key: 'seen', width: 118 },
]

async function loadDevices() {
  if (devicesLoading.value) return
  devicesLoading.value = true
  try {
    devices.value = await api.clusterDevicesList(50)
  } catch {
    /* 设备缓存只做展示，读取失败不阻塞主流程 */
  } finally {
    devicesLoading.value = false
  }
}

function deviceCapacityText(device: ClusterDevice): string {
  if (!device.capacityConcurrent) return '未检测'
  const qps = device.capacityQps > 0 ? ` · ${fmt(device.capacityQps, 0)} QPS` : ''
  return `${device.capacityConcurrent} 并发${qps}`
}

async function browseMasters() {
  if (browsingMasters.value) return
  browsingMasters.value = true
  try {
    discoveredMasters.value = await api.clusterMdnsBrowse(3000)
    if (!discoveredMasters.value.length) {
      message.info('未发现局域网主节点，可直接填写地址与配对码')
    }
  } catch (error) {
    message.warning(`mDNS 浏览失败，可手动填写地址：${String(error)}`)
  } finally {
    browsingMasters.value = false
  }
}

function useDiscoveredMaster(item: DiscoveredMaster) {
  const host = item.addresses[0] ?? item.host
  slaveForm.masterAddr = `${host}:${item.port}`
  message.success(`已填入 ${slaveForm.masterAddr}`)
}

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

const slaveTone = computed<Tone>(() => {
  switch (slaveStatus.value) {
    case 'connecting':
    case 'preparing':
      return 'info'
    case 'ready':
    case 'done':
      return 'ok'
    case 'running':
      return 'live'
    case 'rejected':
    case 'lost':
      return 'bad'
    default:
      return 'idle'
  }
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
  try {
    const [info, items] = await Promise.all([
      api.clusterProtocolInfo(),
      api.clusterLocalAddresses(),
    ])
    protocol.value = info
    addresses.value = items
    port.value = info.defaultPort
    pairTtlMin.value = Math.max(1, Math.round(info.pairCodeTtlSec / 60))
  } catch (error) {
    message.error(`读取集群参数失败：${String(error)}`)
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
  plansLoading.value = true
  try {
    const [list, envs] = await Promise.all([api.planList(id), api.envList(id)])
    plans.value = list
    environments.value = envs
    if (!planId.value && list.length) planId.value = list[0].id
  } catch (error) {
    message.error(String(error))
  } finally {
    plansLoading.value = false
  }
})

watch(planId, async (id) => {
  planSettings.value = {}
  planStages.value = []
  planEnvId.value = null
  orchestrationMode.value = 'off'
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
    // 阶梯计划默认按计划编排下发；固定并发的计划保持关闭
    orchestrationMode.value = planStagesMode.value ? 'plan' : 'off'
  } catch (error) {
    message.error(String(error))
  }
})

/* ---------- 主节点操作 ---------- */

function applySession(snapshot: ClusterMasterSnapshot) {
  const prevJob = session.value?.jobId
  const prevStatus = session.value?.status
  session.value = snapshot
  if (prevJob !== snapshot.jobId) stageReached.value = 0
  const reached = STAGE_INDEX[snapshot.status]
  if (reached != null && snapshot.status !== 'failed' && reached > stageReached.value) {
    stageReached.value = reached
  }
  // 新任务或开始施压时重置曲线，避免上一轮采样混进本轮
  if (prevJob !== snapshot.jobId || (snapshot.status === 'running' && prevStatus !== 'running')) {
    resetLiveSamples()
  }
  const next: Record<string, number> = { ...weights.value }
  for (const node of snapshot.nodes) {
    if (!weightDirty.value[node.id]) next[node.id] = node.suggestedWeight ?? node.weight
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
  const orchestrated = orchestrationPlan.value
  const total = Math.max(
    1,
    Math.min(
      MAX_CONCURRENT,
      orchestrated ? orchestrationPeak.value : numberOr(totalConcurrent.value, 20),
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
    durationSec: Math.max(1, effectiveDuration.value),
    rampUpSec: stagesMode || orchestrated ? 0 : Math.max(0, numberOr(settings.rampUpSec, 0)),
    thinkTimeMs: Math.max(0, numberOr(settings.thinkTimeMs, 0)),
    // 分布式编排由主节点展开成每节点的阶段表，配置本身保持单场景
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
  if (orchestrationMode.value !== 'off' && (!orchestrationPlan.value || orchestrationErrors.value.length)) {
    message.warning(orchestrationErrors.value[0] ?? '请完整填写编排阶段')
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
      pairCodeTtlSec: pairTtlSeconds.value,
      masterName: masterName.value.trim() || null,
      listenAddr: listenAddr.value === '0.0.0.0' ? null : listenAddr.value,
      orchestration: orchestrationPlan.value,
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
  const phases = session.value.orchestration?.phases.length ?? 0
  Modal.confirm({
    title: '开始分布式压测？',
    content: phases
      ? `全部节点将在约 2 秒后同步起跑，按 ${phases} 个编排阶段切换并发，峰值 ${session.value.totalConcurrent}。`
      : `全部节点将在约 2 秒后同步起跑，总并发 ${session.value.totalConcurrent}。`,
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
        resetLiveSamples()
        message.success('任务已关闭')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

function openReport() {
  if (session.value?.reportId) void router.push(`/reports/${session.value.reportId}`)
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
  disconnected: '断线 · 可重连',
  lost: '失联',
  left: '已离开',
  rejected: '拒绝',
}

const NODE_STATUS_TONE: Record<string, Tone> = {
  pairing: 'idle',
  pairReady: 'info',
  preparing: 'info',
  preflight: 'info',
  ready: 'ok',
  running: 'live',
  done: 'ok',
  disconnected: 'warn',
  lost: 'bad',
  left: 'idle',
  rejected: 'bad',
}

function nodeStatusText(status: string): string {
  return NODE_STATUS_TEXT[status] ?? status
}

function nodeStatusTone(status: string): Tone {
  return NODE_STATUS_TONE[status] ?? 'idle'
}

const PHASE_KIND_TEXT: Record<string, string> = {
  ramp: '爬坡',
  hold: '保持',
  step: '阶梯',
}

function phaseKindText(kind: string): string {
  return PHASE_KIND_TEXT[kind] ?? kind
}

function phaseRangeText(phase: OrchestrationPhase): string {
  return phase.kind === 'ramp'
    ? `${phase.fromConcurrent ?? 0} → ${phase.toConcurrent ?? 0}`
    : `${phase.concurrent ?? 0}`
}

function orchestrationPeakFor(phases: OrchestrationPhase[]): number {
  return phases.reduce(
    (peak, phase) =>
      Math.max(
        peak,
        phase.kind === 'ramp'
          ? Math.max(phase.fromConcurrent ?? 0, phase.toConcurrent ?? 0)
          : phase.concurrent ?? 0,
      ),
    0,
  )
}

/** 主节点当前编排阶段（0 基序号来自后端） */
const currentPhaseInfo = computed(() => {
  const phases = session.value?.orchestration?.phases ?? []
  const index = session.value?.currentPhase
  if (index == null || !phases[index]) return null
  const phase = phases[index]
  return {
    index,
    total: phases.length,
    kind: phaseKindText(phase.kind),
    durationSec: phase.durationSec,
    range: phaseRangeText(phase),
  }
})

const slavePhaseText = computed(() => {
  if (slave.value?.phaseIndex == null) return '—'
  const concurrent = slave.value.phaseConcurrent
  return `第 ${slave.value.phaseIndex + 1} 阶段 · ${concurrent || '—'} 并发`
})

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
  { title: '分配并发', key: 'concurrent', width: 92, align: 'right' as const },
  { title: 'QPS', key: 'qps', width: 86, align: 'right' as const },
  { title: '时钟偏移', key: 'clock', width: 128, align: 'right' as const },
  { title: '能力', key: 'capacity', width: 130 },
  { title: '断线缺口', key: 'gap', width: 96, align: 'right' as const },
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

/** 一次性复制地址 + 配对码，方便直接发给另一台机器 */
async function copyPairInfo() {
  const current = session.value
  if (!current?.pairCode) return
  const address = pairAddresses.value[0]
  const text = [
    `主节点：${address ? `${address}:${current.port}` : `端口 ${current.port}`}`,
    `配对码：${current.pairCode}（${pairTtlText.value}内有效，单次使用）`,
  ].join('\n')
  try {
    await copyToClipboard(text)
    message.success('配对信息已复制')
  } catch {
    message.warning('复制失败，请手动选择地址与配对码')
  }
}

/* ---------- 生命周期 ---------- */

let deviceTicks = 0

async function tick() {
  nowTick.value = Date.now()
  if (activeTab.value === 'master' && ++deviceTicks % 5 === 0) void loadDevices()
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

watch(
  () => session.value?.nodes.length ?? 0,
  () => void loadDevices(),
)

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
    unlisteners.push(
      await listen<ClusterSampleEvent>('cluster:sample', (event) => {
        const current = session.value
        if (!current || event.payload.jobId !== current.jobId) return
        const sec = event.payload.sample.elapsedSec
        liveSamplesBySec.value = {
          ...liveSamplesBySec.value,
          [sec]: {
            ...(liveSamplesBySec.value[sec] ?? {}),
            [event.payload.nodeId]: event.payload.sample.qps,
          },
        }
        // 长时间压测只保留最近窗口的两倍，超出即裁剪
        const keys = Object.keys(liveSamplesBySec.value).map(Number)
        if (keys.length > LIVE_WINDOW * 2) {
          const keep = keys.sort((a, b) => a - b).slice(-LIVE_WINDOW)
          liveSamplesBySec.value = Object.fromEntries(
            keep.map((key) => [String(key), liveSamplesBySec.value[key]]),
          )
        }
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
  void loadDevices()
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
      subtitle="多机同压 · 时钟对齐 · 同步起跑 · 采样合并为同一份报告"
    />

    <div class="flow-band">
      <div class="flow-node">
        <span class="flow-cap">主节点</span>
        <span class="flow-desc">调度 · 时钟 · 合并</span>
      </div>
      <span class="flow-arrow" />
      <div class="flow-node">
        <span class="flow-cap">配对码</span>
        <span class="flow-desc">6 位 · 有效期内单次</span>
      </div>
      <span class="flow-arrow" />
      <div class="flow-node">
        <span class="flow-cap">从节点</span>
        <span class="flow-desc">反向连接 · 本地环境</span>
      </div>
      <span class="flow-arrow" />
      <div class="flow-node">
        <span class="flow-cap">同步起跑</span>
        <span class="flow-desc">每秒采样 · 同一报告</span>
      </div>
    </div>

    <a-tabs v-model:active-key="activeTab" size="small">
      <a-tab-pane key="master" tab="主节点">
        <a-row v-if="!session" :gutter="16">
          <a-col :xs="24" :lg="15">
            <section class="fm-card panel">
              <header class="panel-head">
                <div>
                  <h3>新建分布式任务</h3>
                  <p class="panel-sub">选择已绑定接口的压测计划，生成一次性配对码</p>
                </div>
              </header>
              <div class="panel-body">
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
                        :loading="plansLoading"
                        show-search
                        option-filter-prop="label"
                        placeholder="选择已绑定接口的计划"
                        :disabled="!projectId"
                      />
                    </a-form-item>
                  </a-col>
                </a-row>

                <div v-if="projectId && !plansLoading && !plans.length" class="plans-empty">
                  <span>该项目还没有压测计划，先创建计划并绑定接口。</span>
                  <a-button type="link" size="small" @click="router.push('/plans')">
                    去创建压测计划
                  </a-button>
                </div>

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
                <div v-if="planStageAlert" class="notice info">
                  <span class="nt">阶梯计划</span>
                  <span>{{ planStageAlert }}</span>
                </div>

                <a-row :gutter="12">
                  <a-col :xs="12" :md="6">
                    <a-form-item :label="orchestrationMode === 'off' ? '总并发' : `峰值并发（编排 ${orchestrationPeak}）`">
                      <a-input-number
                        v-model:value="totalConcurrent"
                        :min="1"
                        :max="MAX_CONCURRENT"
                        :step="10"
                        :disabled="orchestrationMode !== 'off'"
                        class="w-full"
                      />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="12" :md="6">
                    <a-form-item label="集群端口">
                      <a-input-number v-model:value="port" :min="1024" :max="65535" class="w-full" />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="12" :md="6">
                    <a-form-item :label="`配对码有效期（${pairTtlMinLimit}–${pairTtlMaxLimit} 分钟）`">
                      <a-input-number
                        v-model:value="pairTtlMin"
                        :min="pairTtlMinLimit"
                        :max="pairTtlMaxLimit"
                        :step="1"
                        class="w-full"
                      />
                    </a-form-item>
                  </a-col>
                  <a-col :xs="12" :md="6">
                    <a-form-item label="主节点名称（可选）">
                      <a-input v-model:value="masterName" placeholder="默认取本机计算机名" allow-clear />
                    </a-form-item>
                  </a-col>
                </a-row>

                <a-row :gutter="12">
                  <a-col :xs="24" :md="14">
                    <a-form-item label="并发编排">
                      <a-radio-group v-model:value="orchestrationMode" size="small" button-style="solid">
                        <a-radio-button value="off">固定并发</a-radio-button>
                        <a-radio-button value="plan" :disabled="!planStagesMode">按计划阶梯</a-radio-button>
                        <a-radio-button value="custom">自定义编排</a-radio-button>
                      </a-radio-group>
                    </a-form-item>
                  </a-col>
                  <a-col :xs="24" :md="10">
                    <a-form-item label="监听网卡">
                      <a-select v-model:value="listenAddr" size="small">
                        <a-select-option value="0.0.0.0">0.0.0.0（全部网卡）</a-select-option>
                        <a-select-option v-for="item in addresses" :key="item" :value="item">
                          {{ item }}
                        </a-select-option>
                      </a-select>
                    </a-form-item>
                  </a-col>
                </a-row>

                <div v-if="orchestrationMode !== 'off'" class="orch-editor">
                  <div class="orch-head">
                    <span class="preview-label">阶段</span>
                    <span class="orch-summary mono">
                      {{ orchestrationPhases.length }} 段 · {{ orchestrationDuration }}s · 峰值
                      {{ orchestrationPeak }}
                    </span>
                  </div>
                  <div v-if="orchestrationMode === 'plan'" class="notice info">
                    <span class="nt">计划阶梯</span>
                    <span>按计划中的 {{ planStageList.length }} 段并发下发 step 阶段，各节点按时间点同步切换。</span>
                  </div>
                  <div v-else class="orch-rows">
                    <div v-for="(phase, index) in customPhases" :key="index" class="orch-row">
                      <span class="orch-no mono">{{ String(index + 1).padStart(2, '0') }}</span>
                      <a-select
                        v-model:value="phase.kind"
                        :options="PHASE_OPTIONS"
                        size="small"
                        class="orch-kind"
                      />
                      <a-input-number
                        v-model:value="phase.durationSec"
                        :min="1"
                        :max="86400"
                        size="small"
                        class="orch-num"
                      />
                      <span class="orch-unit">s</span>
                      <template v-if="phase.kind === 'ramp'">
                        <a-input-number
                          v-model:value="phase.fromConcurrent"
                          :min="1"
                          :max="MAX_CONCURRENT"
                          size="small"
                          class="orch-num"
                        />
                        <span class="orch-unit">→</span>
                        <a-input-number
                          v-model:value="phase.toConcurrent"
                          :min="1"
                          :max="MAX_CONCURRENT"
                          size="small"
                          class="orch-num"
                        />
                      </template>
                      <a-input-number
                        v-else
                        v-model:value="phase.concurrent"
                        :min="1"
                        :max="MAX_CONCURRENT"
                        size="small"
                        class="orch-num"
                      />
                      <a-button
                        type="text"
                        size="small"
                        danger
                        :disabled="customPhases.length <= 1"
                        @click="removePhase(index)"
                      >
                        删除
                      </a-button>
                    </div>
                    <a-button
                      size="small"
                      :disabled="customPhases.length >= MAX_ORCHESTRATION_PHASES"
                      @click="addPhase"
                    >
                      添加阶段
                    </a-button>
                  </div>
                  <div v-if="orchestrationErrors.length" class="notice warn">
                    <span class="nt">编排</span>
                    <span>{{ orchestrationErrors[0] }}</span>
                  </div>
                </div>

                <a-button type="primary" :loading="creating" :disabled="!canCreate" @click="createSession">
                  <ThunderboltOutlined />
                  创建任务并生成配对码
                </a-button>
                <p class="hint">
                  总并发按节点权重拆分，主节点同样作为施压节点参与；编排阶段由主节点展开成各节点的逐阶段并发并同步切换。
                </p>
              </a-form>
              </div>
            </section>
          </a-col>
          <a-col :xs="24" :lg="9">
            <section class="fm-card panel">
              <header class="panel-head">
                <h3>本机配对地址</h3>
              </header>
              <div class="panel-body">
                <div class="addr-list">
                  <button
                    v-for="item in addresses"
                    :key="item"
                    class="addr-chip mono"
                    @click="copy(`${item}:${displayPort}`)"
                  >
                    {{ item }}:{{ displayPort }}
                  </button>
                </div>
                <p class="hint">
                  端口取自上方「集群端口」；点击地址复制。从节点在另一台机器的「从节点」页填写该地址与配对码。
                </p>
              </div>
            </section>

            <section class="fm-card panel how-panel">
              <header class="panel-head">
                <h3>工作方式</h3>
              </header>
              <ol class="how-list">
                <li>
                  <span class="how-no mono">01</span>
                  <span>主节点创建任务，生成 6 位配对码</span>
                </li>
                <li>
                  <span class="how-no mono">02</span>
                  <span>从节点填地址与配对码，反向接入</span>
                </li>
                <li>
                  <span class="how-no mono">03</span>
                  <span>下发配置、目标预检、统一起跑</span>
                </li>
                <li>
                  <span class="how-no mono">04</span>
                  <span>各节点按秒采样，合并为一份报告</span>
                </li>
              </ol>
            </section>
          </a-col>
        </a-row>

        <template v-else>
          <section class="fm-card panel session-panel">
            <header class="panel-head session-head">
              <div class="session-id">
                <span class="status-dot" :class="masterTone" />
                <h3>{{ session.name }}</h3>
                <span class="st-pill" :class="masterTone">{{ masterStatusText }}</span>
                <span v-if="session.incomplete" class="st-pill warn">结果不完整</span>
              </div>
              <a-space size="small" wrap>
                <a-button
                  v-if="session.status === 'pairing'"
                  type="primary"
                  size="small"
                  :loading="busy"
                  :disabled="!hasSlave"
                  :title="hasSlave ? '' : '至少加入 1 个从节点后才能下发配置'"
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
            </header>

            <div class="panel-body">
              <div class="stages">
                <div
                  v-for="(step, index) in STAGE_STEPS"
                  :key="step"
                  class="stage"
                  :class="{
                    done: index < stageCurrent,
                    now: index === stageCurrent,
                    bad: index === stageCurrent && session.status === 'failed',
                  }"
                >
                  <span class="s-dot" />
                  <span class="s-label">{{ step }}</span>
                </div>
              </div>

              <div class="stats-grid">
                <div class="stat">
                  <span class="stat-label">总并发</span>
                  <span class="stat-value">{{ session.totalConcurrent }}</span>
                </div>
                <div class="stat">
                  <span class="stat-label">已用时</span>
                  <span class="stat-value">{{ session.elapsedSec }} <i>s</i></span>
                </div>
                <div class="stat">
                  <span class="stat-label">合计 QPS</span>
                  <span class="stat-value">{{ fmt(totalQps) }}</span>
                </div>
                <div class="stat">
                  <span class="stat-label">参与节点</span>
                  <span class="stat-value">{{ participatingNodes }} <i>/ {{ nodeList.length }}</i></span>
                </div>
                <div class="stat">
                  <span class="stat-label">报告</span>
                  <span class="stat-value">{{ session.reportId ? `#${session.reportId}` : '—' }}</span>
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
                  <template v-if="startCountdownSec > 0"> · {{ startCountdownSec }}s 后同步起跑</template>
                  · 任务 #{{ session.jobId }} · 端口 {{ session.port }}
                </span>
              </div>
              <div v-if="currentPhaseInfo" class="target-line">
                <span class="preview-label">阶段</span>
                <span class="mono">
                  第 {{ currentPhaseInfo.index + 1 }}/{{ currentPhaseInfo.total }} 阶段 · {{ currentPhaseInfo.kind }}
                  {{ currentPhaseInfo.range }} 并发 · {{ currentPhaseInfo.durationSec }}s
                </span>
              </div>
              <div v-else-if="session.orchestration?.phases?.length" class="target-line">
                <span class="preview-label">编排</span>
                <span class="mono">
                  {{ session.orchestration.phases.length }} 阶段 · 峰值 {{ orchestrationPeakFor(session.orchestration.phases) }} 并发 · 启动后按时间切换
                </span>
              </div>

              <div v-if="session.message" class="notice" :class="session.incomplete ? 'warn' : 'info'">
                <span class="nt">{{ session.incomplete ? '注意' : '状态' }}</span>
                <span>{{ session.message }}</span>
              </div>
              <div v-else-if="session.status === 'pairing'" class="notice info">
                <span class="nt">等待</span>
                <span>等待从节点加入。至少加入 1 个从节点后，点「下发配置」按权重拆分并发并做目标预检。</span>
              </div>
              <div v-else-if="session.status === 'ready'" class="notice ok">
                <span class="nt">就绪</span>
                <span>全部节点已就绪。点「同步启动」后各节点按统一时间点起跑，主节点本地引擎同时开始。</span>
              </div>
            </div>
          </section>

          <section v-if="session.pairCode" class="pair-panel">
            <div class="pair-left">
              <span class="pair-label">配对码 · {{ pairTtlText }}内有效</span>
              <div class="pair-code mono">{{ session.pairCode }}</div>
              <div class="pair-meta">
                单次使用 · 剩余 <b class="mono">{{ pairRemainingText }}</b> · 端口 {{ session.port }}
              </div>
            </div>
            <div class="pair-right">
              <div class="pair-right-head">
                <span class="pair-label">本机地址</span>
                <button class="link-btn" @click="copyPairInfo">复制配对信息</button>
              </div>
              <div class="addr-list">
                <button
                  v-for="item in pairAddresses"
                  :key="item"
                  class="addr-chip mono"
                  @click="copy(`${item}:${session.port}`)"
                >
                  {{ item }}:{{ session.port }}
                </button>
              </div>
              <p class="hint">把地址与配对码发给从节点；点击地址可复制。</p>
            </div>
          </section>
          <div v-else-if="session.status === 'pairing'" class="notice info">
            <span class="nt">配对码</span>
            <span>配对码已过期。新从节点需要新配对码：关闭任务后重新创建即可；已配对节点仍可直接下发配置。</span>
          </div>

          <section v-if="liveElapsed.length" class="fm-card panel chart-panel">
            <header class="panel-head">
              <div class="panel-title">
                <h3>各节点 QPS</h3>
                <span class="panel-sub">最近 {{ liveElapsed.length }} 秒 · 每秒刷新</span>
              </div>
            </header>
            <div class="panel-body chart-body">
              <LineChart :series="liveSeries" :labels="liveLabels" :area="false" :height="200" />
            </div>
          </section>

          <section class="fm-card panel node-panel">
            <header class="panel-head">
              <div class="panel-title">
                <h3>节点</h3>
                <span class="panel-sub">准备时按权重拆分总并发</span>
              </div>
            </header>
            <a-table
              :columns="nodeColumns"
              :data-source="nodeList"
              row-key="id"
              size="small"
              :pagination="false"
              :scroll="{ x: 1260 }"
            >
              <template #bodyCell="{ column, record }">
                <template v-if="column.key === 'name'">
                  <div class="node-name">{{ record.name }}</div>
                  <div class="node-addr mono">
                    {{ record.addr || '—' }}
                    <span class="node-proto">v{{ record.protocolVersion }}</span>
                    <span v-if="record.os" class="node-proto">{{ record.os }}</span>
                  </div>
                </template>
                <template v-else-if="column.key === 'role'">
                  <span class="role-pill" :class="record.role">
                    {{ record.role === 'master' ? '主节点' : '从节点' }}
                  </span>
                </template>
                <template v-else-if="column.key === 'status'">
                  <span class="st-pill" :class="nodeStatusTone(record.status)" :title="record.error || undefined">
                    <i class="pill-dot" />
                    {{ nodeStatusText(record.status) }}
                  </span>
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
                  <button
                    v-if="session.status === 'pairing' && record.suggestedWeight != null"
                    class="weight-suggest mono"
                    title="按本机能力检测归一化，点击采用"
                    @click="setWeight(record, record.suggestedWeight)"
                  >
                    建议 {{ fmt(record.suggestedWeight, 2) }}
                  </button>
                </template>
                <template v-else-if="column.key === 'qps'">
                  <span class="mono">{{ fmt(record.qps) }}</span>
                </template>
                <template v-else-if="column.key === 'concurrent'">
                  <span v-if="session.status === 'pairing'" class="mono planned-value">
                    ≈ {{ fmtInt(plannedConcurrent[record.id]) }}
                  </span>
                  <span v-else class="mono">{{ fmtInt(record.concurrent) }}</span>
                </template>
                <template v-else-if="column.key === 'clock'">
                  <span class="mono">{{ clockText(record) }}</span>
                </template>
                <template v-else-if="column.key === 'capacity'">
                  <template v-if="record.capacityConcurrent">
                    <span class="mono">{{ record.capacityConcurrent }} 并发</span>
                    <div v-if="record.cores" class="node-addr mono">
                      {{ record.cores }} 核<template v-if="record.capacityQps"> · {{ fmt(record.capacityQps, 0) }} QPS</template>
                    </div>
                  </template>
                  <span v-else class="planned-value">未检测</span>
                </template>
                <template v-else-if="column.key === 'gap'">
                  <span class="mono" :class="{ 'node-dropped': record.gapSeconds > 0 }">
                    {{ record.gapSeconds > 0 ? `${record.gapSeconds}s` : '—' }}
                  </span>
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
            <div class="node-foot">
              <div v-if="tooFewConcurrent" class="notice warn">
                <span class="nt">并发</span>
                <span>总并发少于节点数，无法保证每个节点至少 1 并发。请提高总并发，或关闭多余节点的会话后重新创建任务。</span>
              </div>
              <div v-if="!nodeList.length" class="inline-empty">还没有节点</div>
              <p v-else-if="session.status === 'pairing'" class="hint">
                从节点加入后会出现在这里；权重可调，主节点与从节点一视同仁。「分配并发」为按权重预估，下发配置后以实际分配为准。
              </p>
            </div>
          </section>
        </template>

        <section class="fm-card panel device-panel">
          <header class="panel-head">
            <div class="panel-title">
              <h3>历史设备</h3>
              <span class="panel-sub">按稳定 deviceId 缓存能力检测，跨任务复用</span>
            </div>
            <a-button size="small" :loading="devicesLoading" @click="loadDevices">刷新</a-button>
          </header>
          <a-table
            :columns="deviceColumns"
            :data-source="devices"
            row-key="nodeId"
            size="small"
            :pagination="false"
            :scroll="{ x: 660 }"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'name'">
                <div class="device-name">
                  <span class="online-dot" :class="{ on: record.online }" />
                  {{ record.nodeName || record.nodeId }}
                  <span v-if="record.online" class="st-pill ok">在线</span>
                </div>
                <div class="node-addr mono">{{ record.nodeId }}</div>
              </template>
              <template v-else-if="column.key === 'addr'">
                <span class="mono">{{ record.lastAddr || '—' }}</span>
              </template>
              <template v-else-if="column.key === 'capacity'">
                <span class="mono">{{ deviceCapacityText(record) }}</span>
                <div v-if="record.cores || record.os" class="node-addr mono">
                  <template v-if="record.cores">{{ record.cores }} 核</template>
                  <template v-if="record.cores && record.os"> · </template>
                  <template v-if="record.os">{{ record.os }}</template>
                </div>
              </template>
              <template v-else-if="column.key === 'seen'">
                <span class="mono">{{ fmtTime(record.lastSeenAt) }}</span>
              </template>
            </template>
          </a-table>
          <div v-if="!devices.length" class="inline-empty">
            还没有设备记录；从节点首次配对后会自动上报本机能力。
          </div>
        </section>
      </a-tab-pane>

      <a-tab-pane key="slave" tab="从节点">
        <a-row :gutter="16">
          <a-col :xs="24" :lg="11">
            <section class="fm-card panel">
              <header class="panel-head">
                <div>
                  <h3>加入主节点</h3>
                  <p class="panel-sub">填写主节点地址与一次性配对码</p>
                </div>
              </header>
              <div class="panel-body">
                <a-form layout="vertical" class="cluster-form">
                  <a-form-item label="主节点地址">
                    <a-input
                      v-model:value="slaveForm.masterAddr"
                      placeholder="192.168.1.10:17801"
                      :disabled="slaveConnected"
                    />
                    <div class="mdns-row">
                      <a-button
                        size="small"
                        :loading="browsingMasters"
                        :disabled="slaveConnected"
                        @click="browseMasters"
                      >
                        搜索局域网主节点
                      </a-button>
                      <span class="hint inline">mDNS 失败时仍可手动填写地址</span>
                    </div>
                    <div v-if="discoveredMasters.length" class="mdns-list">
                      <button
                        v-for="item in discoveredMasters"
                        :key="item.fullname"
                        class="mdns-item"
                        @click="useDiscoveredMaster(item)"
                      >
                        <span class="mdns-name">{{ item.nodeName || item.name }}</span>
                        <span class="mono">{{ item.addresses[0] || item.host }}:{{ item.port }}</span>
                        <span class="st-pill">v{{ item.version }}</span>
                      </button>
                    </div>
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
                    配对码由主节点生成，在有效期内单次使用。本机按「环境名」在本地查找同名环境，
                    变量值不会从主节点下发。
                  </p>
                </a-form>
              </div>
            </section>
          </a-col>
          <a-col :xs="24" :lg="13">
            <section class="fm-card panel">
              <header class="panel-head">
                <div class="session-id">
                  <span class="status-dot" :class="slaveTone" />
                  <h3>本机会话</h3>
                  <span v-if="slave" class="st-pill" :class="slaveTone">{{ slaveStatusText }}</span>
                </div>
                <span v-if="slave" class="mono slave-addr">{{ slave.masterAddr }}</span>
              </header>
              <div class="panel-body">
                <template v-if="slave">
                  <div class="kv-grid">
                    <div class="kv">
                      <span>节点名称</span>
                      <b>{{ slave.nodeName || '—' }}</b>
                    </div>
                    <div class="kv">
                      <span>节点 ID</span>
                      <b class="mono">{{ slave.nodeId || '—' }}</b>
                    </div>
                    <div class="kv">
                      <span>设备 ID</span>
                      <b class="mono">{{ slave.deviceId || '—' }}</b>
                    </div>
                    <div class="kv">
                      <span>协议版本</span>
                      <b class="mono">v{{ slave.protocolVersion }}</b>
                    </div>
                    <div class="kv">
                      <span>计划</span>
                      <b>{{ slave.planName || '等待下发' }}</b>
                    </div>
                    <div class="kv">
                      <span>分配并发</span>
                      <b class="mono">{{ slave.concurrent || '—' }}</b>
                    </div>
                    <div class="kv">
                      <span>当前阶段</span>
                      <b class="mono">{{ slavePhaseText }}</b>
                    </div>
                    <div class="kv">
                      <span>当前 QPS</span>
                      <b class="mono">{{ fmt(slave.qps) }}</b>
                    </div>
                    <div class="kv">
                      <span>已用时</span>
                      <b class="mono">{{ slave.elapsedSec }}s</b>
                    </div>
                    <div class="kv">
                      <span>加入时间</span>
                      <b class="mono">{{ fmtTime(slave.joinedAt) }}</b>
                    </div>
                    <div class="kv">
                      <span>起跑时间</span>
                      <b class="mono">{{ slave.startAtMs ? fmtTime(slave.startAtMs) : '等待主节点指令' }}</b>
                    </div>
                    <div class="kv wide">
                      <span>目标</span>
                      <b class="mono">{{ slave.method || '—' }} {{ slave.target || '' }}</b>
                    </div>
                  </div>
                  <div v-if="slave.reconnecting" class="notice warn">
                    <span class="nt">重连</span>
                    <span>
                      主节点连接中断，正在进行第 {{ slave.reconnectAttempts }} 次重连；本机引擎继续运行，
                      恢复后按令牌回到原任务并补报采样。
                    </span>
                  </div>
                  <div v-if="slave.missingVars.length" class="notice warn">
                    <span class="nt">变量</span>
                    <span>本机环境缺少变量：{{ slave.missingVars.join('、') }}</span>
                  </div>
                  <div v-if="slave.error" class="notice bad">
                    <span class="nt">错误</span>
                    <span>{{ slave.error }}</span>
                  </div>
                  <a-button
                    v-if="slaveConnected"
                    danger
                    size="small"
                    :loading="slaveBusy"
                    @click="disconnectSlave"
                  >
                    断开连接
                  </a-button>
                </template>
                <div v-else class="inline-empty">尚未加入主节点</div>
              </div>
            </section>
          </a-col>
        </a-row>
      </a-tab-pane>

      <a-tab-pane key="protocol" tab="协议参数">
        <section class="fm-card panel">
          <header class="panel-head">
            <div>
              <h3>协议参数</h3>
              <p class="panel-sub">由主节点下发，从节点只读</p>
            </div>
          </header>
          <div class="panel-body">
            <a-descriptions bordered size="small" :column="2">
              <a-descriptions-item label="协议版本">{{ protocol?.version ?? '—' }}</a-descriptions-item>
              <a-descriptions-item label="默认端口">{{ protocol?.defaultPort ?? '—' }}</a-descriptions-item>
              <a-descriptions-item label="配对码默认有效期">
                {{ protocol ? `${Math.round(protocol.pairCodeTtlSec / 60)} 分钟` : '—' }}
              </a-descriptions-item>
              <a-descriptions-item label="配对码可调范围">
                {{
                  protocol
                    ? `${Math.round(protocol.minPairCodeTtlSec / 60)}–${Math.round(protocol.maxPairCodeTtlSec / 60)} 分钟`
                    : '—'
                }}
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
              <a-descriptions-item label="最低兼容版本">v{{ protocol?.minVersion ?? '—' }}</a-descriptions-item>
              <a-descriptions-item label="重连宽限">
                {{ protocol ? `${Math.round(protocol.reconnectGraceSec / 60)} 分钟` : '—' }}
              </a-descriptions-item>
              <a-descriptions-item label="重连退避上限">
                {{ protocol ? `${protocol.reconnectBackoffMaxSec} 秒` : '—' }}
              </a-descriptions-item>
              <a-descriptions-item label="mDNS 服务">
                <span class="mono">{{ protocol?.mdnsService ?? '—' }}</span>
              </a-descriptions-item>
            </a-descriptions>
            <p class="hint">改动需升级主从双方版本，避免协议不兼容。</p>
          </div>
        </section>
      </a-tab-pane>
    </a-tabs>
  </div>
</template>

<style scoped>
/* ---------- 工作模型条 ---------- */
.flow-band {
  display: flex;
  align-items: stretch;
  margin-bottom: 16px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}

.flow-node {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 9px 14px;
}

.flow-cap {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
}

.flow-desc {
  font-size: 11.5px;
  color: var(--fm-text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.flow-arrow {
  position: relative;
  flex: none;
  width: 24px;
}

.flow-arrow::before {
  content: '';
  position: absolute;
  left: 50%;
  top: 50%;
  width: 6px;
  height: 6px;
  border-top: 1px solid var(--fm-border-strong);
  border-right: 1px solid var(--fm-border-strong);
  transform: translate(-50%, -50%) rotate(45deg);
}

/* ---------- 面板 ---------- */
.panel {
  margin-bottom: 12px;
  overflow: hidden;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px 16px;
  padding: 11px 16px;
  border-bottom: 1px solid var(--fm-border);
}

.panel-head h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--fm-text);
}

.panel-sub {
  margin: 3px 0 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--fm-muted);
}

.panel-title {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 4px 10px;
}

.panel-title .panel-sub {
  margin: 0;
}

.panel-body {
  padding: 14px 16px 16px;
}

.session-head {
  align-items: center;
}

.session-id {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  min-width: 0;
}

/* ---------- 状态点与胶囊 ---------- */
.status-dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.status-dot.info {
  background: var(--fm-primary);
}

.status-dot.ok,
.status-dot.live {
  background: var(--fm-success);
}

.status-dot.live {
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--fm-success) 14%, transparent);
  animation: cluster-pulse 1.6s infinite;
}

.status-dot.warn {
  background: var(--fm-warning);
}

.status-dot.bad {
  background: var(--fm-danger);
}

@keyframes cluster-pulse {
  0% {
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--fm-success) 32%, transparent);
  }
  70% {
    box-shadow: 0 0 0 7px color-mix(in srgb, var(--fm-success) 0%, transparent);
  }
  100% {
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--fm-success) 0%, transparent);
  }
}

.st-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 20px;
  padding: 0 8px;
  border: 1px solid var(--fm-border);
  border-radius: 999px;
  background: var(--fm-surface);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.04em;
  color: var(--fm-muted);
  white-space: nowrap;
}

.st-pill .pill-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
}

.st-pill.info {
  color: var(--fm-primary);
  border-color: color-mix(in srgb, var(--fm-primary) 32%, transparent);
  background: color-mix(in srgb, var(--fm-primary) 8%, transparent);
}

.st-pill.ok,
.st-pill.live {
  color: var(--fm-success);
  border-color: color-mix(in srgb, var(--fm-success) 32%, transparent);
  background: color-mix(in srgb, var(--fm-success) 8%, transparent);
}

.st-pill.warn {
  color: var(--fm-warning);
  border-color: color-mix(in srgb, var(--fm-warning) 36%, transparent);
  background: color-mix(in srgb, var(--fm-warning) 8%, transparent);
}

.st-pill.bad {
  color: var(--fm-danger);
  border-color: color-mix(in srgb, var(--fm-danger) 32%, transparent);
  background: color-mix(in srgb, var(--fm-danger) 8%, transparent);
}

.role-pill {
  display: inline-flex;
  align-items: center;
  height: 19px;
  padding: 0 7px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-text-2);
  white-space: nowrap;
}

.role-pill.master {
  color: var(--fm-brass-text);
  border-color: color-mix(in srgb, var(--fm-brass) 40%, transparent);
  background: color-mix(in srgb, var(--fm-brass) 8%, transparent);
}

/* ---------- 阶段轨 ---------- */
.stages {
  position: relative;
  display: flex;
  margin: 2px 0 16px;
}

.stages::before {
  content: '';
  position: absolute;
  left: 10%;
  right: 10%;
  top: 5px;
  height: 1px;
  background: var(--fm-border);
}

.stage {
  position: relative;
  z-index: 1;
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}

.s-dot {
  width: 11px;
  height: 11px;
  border-radius: 50%;
  border: 1px solid var(--fm-border-strong);
  background: var(--fm-surface);
}

.stage.done .s-dot {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
}

.stage.now .s-dot {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 3px var(--fm-primary-soft);
}

.stage.bad .s-dot {
  background: var(--fm-danger);
  border-color: var(--fm-danger);
  box-shadow: 0 0 0 3px var(--fm-danger-soft);
}

.s-label {
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.1em;
  color: var(--fm-muted);
}

.stage.done .s-label {
  color: var(--fm-text-2);
}

.stage.now .s-label {
  color: var(--fm-primary);
  font-weight: 600;
}

.stage.bad .s-label {
  color: var(--fm-danger);
}

/* ---------- 指标 ---------- */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(112px, 1fr));
  gap: 8px;
  margin-bottom: 14px;
}

.stat {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
}

.stat-label {
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.stat-value {
  font-family: var(--fm-font-mono);
  font-size: 17px;
  font-weight: 600;
  line-height: 1.25;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
}

.stat-value i {
  font-style: normal;
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
}

.target-line {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text);
}

.target-line .mono {
  color: var(--fm-text-2);
  word-break: break-all;
}

/* ---------- 提示 ---------- */
.notice {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  margin-top: 10px;
  padding: 8px 12px;
  border: 1px solid var(--fm-border);
  border-left: 2px solid var(--fm-border-strong);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  font-size: 12px;
  line-height: 1.65;
  color: var(--fm-text-2);
}

.notice .nt {
  flex: none;
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  line-height: 1.9;
  color: var(--fm-muted);
}

.notice.info {
  border-left-color: var(--fm-primary);
}
.notice.info .nt {
  color: var(--fm-primary);
}
.notice.ok {
  border-left-color: var(--fm-success);
}
.notice.ok .nt {
  color: var(--fm-success);
}
.notice.warn {
  border-left-color: var(--fm-warning);
}
.notice.warn .nt {
  color: var(--fm-warning);
}
.notice.bad {
  border-left-color: var(--fm-danger);
}
.notice.bad .nt {
  color: var(--fm-danger);
}

/* ---------- 表单 ---------- */
.cluster-form {
  margin-bottom: 0;
}

.plans-empty {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  margin: -4px 0 12px;
  padding: 8px 12px;
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border);
}

.plan-preview {
  margin: 2px 0 12px;
  padding: 10px 12px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
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
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  color: var(--fm-muted);
}

.preview-value {
  word-break: break-all;
}

.w-full {
  width: 100%;
}

/* ---------- 地址 ---------- */
.addr-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.addr-chip {
  display: inline-flex;
  align-items: center;
  height: 24px;
  padding: 0 9px;
  border: 1px solid var(--fm-border-control);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface);
  font-size: 11px;
  color: var(--fm-text-2);
  cursor: pointer;
  transition:
    border-color var(--fm-dur-fast) var(--fm-ease),
    color var(--fm-dur-fast) var(--fm-ease),
    background-color var(--fm-dur-fast) var(--fm-ease);
}

.addr-chip:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.hint {
  margin: 9px 0 0;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

/* ---------- 工作方式 ---------- */
.how-panel {
  margin-bottom: 0;
}

.how-list {
  list-style: none;
  margin: 0;
  padding: 6px 16px 10px;
}

.how-list li {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 7px 0;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-2);
}

.how-list li:last-child {
  border-bottom: 0;
}

.how-no {
  flex: none;
  font-size: 9.5px;
  letter-spacing: 0.12em;
  color: var(--fm-brass-text);
}

/* ---------- 配对码 ---------- */
.pair-panel {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 22px 28px;
  margin-bottom: 12px;
  padding: 16px 18px;
  border: 1px solid color-mix(in srgb, var(--fm-primary) 28%, var(--fm-border));
  border-radius: var(--fm-radius);
  background: linear-gradient(120deg, var(--fm-primary-soft), var(--fm-surface) 58%);
}

.pair-left {
  flex: none;
}

.pair-label {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
}

.pair-code {
  margin: 5px 0 4px;
  font-size: 34px;
  font-weight: 600;
  letter-spacing: 0.28em;
  line-height: 1.1;
  color: var(--fm-primary);
}

.pair-meta {
  font-size: 11.5px;
  color: var(--fm-text-2);
}

.pair-meta b {
  color: var(--fm-text);
  font-weight: 600;
}

.pair-right {
  flex: 1 1 260px;
  min-width: 230px;
}

.pair-right-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.link-btn {
  padding: 0;
  border: 0;
  background: transparent;
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  color: var(--fm-primary);
  cursor: pointer;
}

.link-btn:hover {
  text-decoration: underline;
}

/* ---------- 图表 ---------- */
.chart-panel {
  margin-bottom: 12px;
}

.chart-body {
  padding: 8px 16px 10px;
}

/* ---------- 节点表 ---------- */
.node-panel {
  margin-bottom: 12px;
}

.node-foot {
  padding: 10px 16px 14px;
}

.node-name {
  line-height: 1.5;
  color: var(--fm-text);
}

.node-addr {
  font-size: 11px;
  color: var(--fm-muted);
  word-break: break-all;
}

.node-error {
  margin-top: 2px;
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--fm-danger);
}

.node-dropped {
  color: var(--fm-warning);
}

.weight-input {
  width: 78px;
}

.planned-value {
  color: var(--fm-muted);
}

.inline-empty {
  padding: 18px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

/* ---------- 从节点会话 ---------- */
.slave-addr {
  font-size: 11px;
  color: var(--fm-muted);
}

.kv-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(148px, 1fr));
  gap: 10px 18px;
  margin-bottom: 12px;
}

.kv {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.kv span {
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.kv b {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
  word-break: break-all;
}

.kv.wide {
  grid-column: 1 / -1;
}

/* ---------- 编排编辑 ---------- */
.orch-editor {
  margin: -2px 0 12px;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
}

.orch-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.orch-summary {
  font-size: 11px;
  color: var(--fm-muted);
}

.orch-rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.orch-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.orch-no {
  width: 18px;
  flex: none;
  font-size: 9.5px;
  color: var(--fm-brass-text);
}

.orch-kind {
  width: 112px;
}

.orch-num {
  width: 86px;
}

.orch-unit {
  font-size: 11px;
  color: var(--fm-muted);
}

/* ---------- 设备列表 ---------- */
.device-panel {
  margin: 12px 0 0;
}

.device-name {
  display: flex;
  align-items: center;
  gap: 6px;
  line-height: 1.5;
  color: var(--fm-text);
}

.online-dot {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.online-dot.on {
  background: var(--fm-success);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fm-success) 14%, transparent);
}

.node-proto {
  margin-left: 6px;
  padding: 0 4px;
  border: 1px solid var(--fm-border);
  border-radius: 3px;
  font-size: 9.5px;
  color: var(--fm-muted);
}

.weight-suggest {
  display: block;
  margin-top: 2px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 10px;
  color: var(--fm-primary);
  cursor: pointer;
}

.weight-suggest:hover {
  text-decoration: underline;
}

/* ---------- mDNS ---------- */
.mdns-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 6px;
}

.hint.inline {
  margin: 0;
}

.mdns-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 8px;
}

.mdns-item {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  padding: 6px 9px;
  border: 1px solid var(--fm-border-control);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface);
  color: var(--fm-text-2);
  text-align: left;
  cursor: pointer;
}

.mdns-item:hover {
  border-color: var(--fm-primary);
}

.mdns-name {
  font-size: 12px;
  color: var(--fm-text);
}

@media (max-width: 760px) {
  .flow-band {
    flex-direction: column;
  }

  .flow-arrow {
    display: none;
  }

  .flow-node {
    padding: 8px 12px;
  }

  .flow-desc {
    white-space: normal;
  }
}
</style>
