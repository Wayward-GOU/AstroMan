<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { ThunderboltOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import { copyToClipboard } from '@/utils/clipboard'
import { ensurePoolPreview } from '@/utils/poolGate'
import {
  ALLOCATION_MODE_TEXT,
  allocationItems,
  allocationProblem as allocationProblemOf,
  nodeBudgetCap,
  nodeLimitRange,
  planAllocation,
  toAllocationNode,
} from '@/utils/clusterAllocation'
import type {
  ClusterJobDetail,
  ClusterJobSummary,
  ClusterMasterSnapshot,
  ClusterNodeInfo,
  ClusterProtocolInfo,
  ClusterSampleFrame,
  ClusterSlaveHistoryItem,
  ClusterSlaveSnapshot,
  DiscoveredMaster,
  VarAuditReport,
} from '@/types/cluster'
import type {
  CapturePreset,
  Environment,
  HttpRequestDetail,
  PlanSummary,
  PoolBinding,
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

/** 并发上限，与后端 stress.rs 的 MAX_CONCURRENT 保持一致（绝对安全上限） */
const MAX_CONCURRENT = 100000

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
  /** 计划里保存的参数 / Token 池绑定（v1.1.2） */
  poolBindings?: PoolBinding[]
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

/** 分布式任务按固定并发执行，前端在创建前说明按峰值折算 */
const planStageAlert = computed(() =>
  planStagesMode.value
    ? `分布式任务按固定并发执行：该阶梯计划将按峰值 ${planPeakConcurrent.value} 并发、总时长 ${planDuration.value}s 运行`
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
/** 变量体检：只读，不含变量值 */
const auditOpen = ref(false)
const auditLoading = ref(false)
const audit = ref<VarAuditReport | null>(null)
const weights = ref<Record<string, number>>({})
const weightDirty = ref<Record<string, boolean>>({})
/** 分配模式：auto 能力分配 / even 平均分配 / custom 自定义权重与范围 */
const allocationMode = ref<'auto' | 'even' | 'custom'>('auto')
/** custom 模式下每节点的最小/最大并发 */
const nodeLimits = ref<Record<string, { min: number | null; max: number | null }>>({})
/** 集群变量策略：master_wins 主节点优先 / local_wins 本机优先 */
const varPolicy = ref<'master_wins' | 'local_wins'>('master_wins')

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

/** 计划是否使用了自定义列 / 自动注入：旧从节点不支持这类绑定 */
const hasCustomColumnBinding = computed(() =>
  (planSettings.value.poolBindings ?? []).some((binding) => (binding.column ?? '').trim() !== ''),
)

/** 只统计仍在会话里的节点：失联/离开/完成的历史节点不再参与兼容性提示 */
function isActiveNode(node: ClusterNodeInfo): boolean {
  return !['lost', 'left', 'rejected', 'done'].includes(node.status)
}

/** 版本较低、不支持池自定义列的从节点 */
const legacySlaves = computed(() =>
  nodeList.value.filter(
    (node) => node.role === 'slave' && isActiveNode(node) && !node.poolColumns,
  ),
)

/** 未提供加密公钥、只能读本机同名环境的从节点 */
const envFallbackSlaves = computed(() =>
  nodeList.value.filter(
    (node) => node.role === 'slave' && isActiveNode(node) && !node.envBundle,
  ),
)

/** 当前计划会被旧节点阻断时，下发按钮直接禁用并说明原因 */
const compatBlocked = computed(
  () => hasCustomColumnBinding.value && legacySlaves.value.length > 0,
)

const poolSliceLimit = computed(() => session.value?.poolSliceLimitBytes || 64 * 1024 * 1024)

const poolEstimate = computed(() => session.value?.poolEstimate ?? null)

/** 池切片已用 80% 以上的节点：提前提示，避免下发时才失败 */
const nearLimitNodes = computed(() =>
  nodeList.value.filter(
    (node) =>
      isActiveNode(node) &&
      node.poolSliceBytes != null &&
      node.poolSliceBytes >= poolSliceLimit.value * 0.8,
  ),
)

const prepareDisabledReason = computed(() => {
  if (!hasSlave.value) return '至少加入 1 个从节点后才能下发配置'
  if (compatBlocked.value) {
    return `节点 ${legacySlaves.value.map((node) => node.name).join('、')} 版本较低，请升级后重新加入`
  }
  return ''
})

/** 当前分配模式下的每节点权重与范围（auto / even / custom） */
const allocationRows = computed(() =>
  allocationItems(
    nodeList.value.map(toAllocationNode),
    allocationMode.value,
    session.value?.budget,
    weights.value,
    nodeLimits.value,
  ),
)

/**
 * 配对阶段的并发预览：与后端 `allocate_concurrency_capped` 同口径。
 * 先保留每节点最小并发，剩余按权重分配；超出上限的部分按余数依次补齐。
 */
const plannedConcurrent = computed<Record<string, number>>(() =>
  planAllocation(allocationRows.value, session.value?.totalConcurrent),
)

/** 分配结果提示：下限/上限与总并发冲突时直接给出原因 */
const allocationProblem = computed(() =>
  allocationProblemOf(allocationRows.value, session.value?.totalConcurrent),
)

const allocationModeText = computed(() => ALLOCATION_MODE_TEXT[allocationMode.value])

const tooFewConcurrent = computed(() => !!allocationProblem.value)

const startCountdownSec = computed(() => {
  const at = session.value?.startAtMs ?? 0
  return at > nowTick.value ? Math.ceil((at - nowTick.value) / 1000) : 0
})

const totalQps = computed(() => nodeList.value.reduce((sum, node) => sum + node.qps, 0))

/* ---------- 实时曲线：采样先进非响应式缓冲，按节拍提交 ---------- */

const LIVE_WINDOW = 120
/** 单条曲线最多保留的点数：超出后按步长抽稀，减少 SVG path 重算成本 */
const LIVE_MAX_POINTS = 60
const LIVE_REFRESH_KEY = 'astroman:cluster-live-refresh'

/** 提交节拍（秒）：主节点自己也在施压，图表不必每秒重绘 */
const liveRefreshSec = ref(loadLiveRefreshSec())
const liveSamplesBySec = ref<Record<number, Record<string, number>>>({})
/** 采样缓冲：普通 Map，写入不触发任何响应式更新 */
const liveBuffer = new Map<number, Record<string, number>>()

function loadLiveRefreshSec(): number {
  const saved = Number(localStorage.getItem(LIVE_REFRESH_KEY))
  return [1, 2, 5].includes(saved) ? saved : 2
}

function pushLiveSample(sec: number, nodeId: string, qps: number) {
  const bucket = liveBuffer.get(sec)
  if (bucket) bucket[nodeId] = qps
  else liveBuffer.set(sec, { [nodeId]: qps })
}

/** 把缓冲提交到响应式状态；页面隐藏时跳过，恢复可见后补一次 */
function commitLiveSamples() {
  if (typeof document !== 'undefined' && document.hidden) return
  const keys = [...liveBuffer.keys()].sort((a, b) => a - b).slice(-LIVE_WINDOW)
  if (!keys.length) return
  const keep = new Set(keys)
  for (const key of liveBuffer.keys()) {
    if (!keep.has(key)) liveBuffer.delete(key)
  }
  const next: Record<number, Record<string, number>> = {}
  for (const key of keys) next[key] = liveBuffer.get(key) ?? {}
  liveSamplesBySec.value = next
}

function resetLiveSamples() {
  liveBuffer.clear()
  liveSamplesBySec.value = {}
}

/** 图表里的节点集合：只在节点增删或改名时更新，避免每秒快照触发曲线重算 */
const chartNodes = shallowRef<{ id: string; name: string }[]>([])

function syncChartNodes(list: ClusterNodeInfo[]) {
  const next = list.map((node) => ({ id: node.id, name: node.name || node.id }))
  const same =
    next.length === chartNodes.value.length &&
    next.every(
      (node, index) =>
        node.id === chartNodes.value[index].id && node.name === chartNodes.value[index].name,
    )
  if (!same) chartNodes.value = next
}

const liveElapsed = computed(() => {
  const all = Object.keys(liveSamplesBySec.value)
    .map(Number)
    .sort((a, b) => a - b)
    .slice(-LIVE_WINDOW)
  if (all.length <= LIVE_MAX_POINTS) return all
  const step = Math.ceil(all.length / LIVE_MAX_POINTS)
  const picked = all.filter((_, index) => index % step === 0)
  const last = all[all.length - 1]
  if (picked[picked.length - 1] !== last) picked.push(last)
  return picked
})

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
  return chartNodes.value.map((node, index) => ({
    name: node.name,
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
  const prevJob = session.value?.jobId
  const prevStatus = session.value?.status
  session.value = snapshot
  if (prevJob !== snapshot.jobId) {
    stageReached.value = 0
    // 刷新页面后恢复上次选择，避免默认 auto 覆盖已下发的 even/custom
    if (snapshot.allocationMode === 'even' || snapshot.allocationMode === 'custom') {
      allocationMode.value = snapshot.allocationMode
    }
  }
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
    if (!weightDirty.value[node.id]) next[node.id] = node.weight
  }
  weights.value = next
  syncChartNodes(snapshot.nodes)
}

function setWeight(node: ClusterNodeInfo, value: unknown) {
  const num = Number(value)
  if (!Number.isFinite(num) || num <= 0) return
  weights.value = { ...weights.value, [node.id]: Math.min(10, Math.max(0.1, num)) }
  weightDirty.value = { ...weightDirty.value, [node.id]: true }
}

function setLimit(node: ClusterNodeInfo, field: 'min' | 'max', value: unknown) {
  const num = Number(value)
  const current = nodeLimits.value[node.id] ?? { min: 1, max: null }
  const next = Number.isFinite(num) && num > 0 ? Math.floor(num) : null
  nodeLimits.value = { ...nodeLimits.value, [node.id]: { ...current, [field]: next } }
}

function limitText(node: ClusterNodeInfo) {
  const masterBudget = session.value?.budget
  const { min, max } = nodeLimitRange(node, masterBudget, nodeLimits.value)
  return max >= nodeBudgetCap(node, masterBudget) ? `${min} ~ 预算` : `${min} ~ ${max}`
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
    varPolicy: varPolicy.value,
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
    // 主从同步启动只支持单场景，阶梯计划在页面上已折算为峰值并发 + 总时长
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
    poolBindings: Array.isArray(settings.poolBindings) ? settings.poolBindings : [],
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
    nodeLimits.value = {}
    allocationMode.value = 'auto'
    const snapshot = await api.clusterMasterCreate({
      planId: plan.id,
      planName: plan.name,
      config: buildConfig(detail),
      envName: planEnvName.value,
      port: numberOr(port.value, protocol.value?.defaultPort ?? 17801),
      pairCodeTtlSec: pairTtlSeconds.value,
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
  if (compatBlocked.value) {
    message.error(prepareDisabledReason.value)
    return
  }
  // 下发前强制执行池预览：失败或取消都不允许下发（无池绑定直接通过）
  if (
    !(await ensurePoolPreview(
      planSettings.value.poolBindings,
      totalConcurrent.value,
      '下发配置前确认参数 / Token 池',
    ))
  ) {
    return
  }
  busy.value = true
  try {
    const payload = nodeList.value.map((node) => {
      const limit = nodeLimits.value[node.id]
      return {
        nodeId: node.id,
        weight: weights.value[node.id] ?? node.weight,
        minConcurrent: allocationMode.value === 'custom' ? (limit?.min ?? 1) : null,
        maxConcurrent: allocationMode.value === 'custom' ? (limit?.max ?? null) : null,
      }
    })
    applySession(await api.clusterMasterPrepare(session.value.jobId, allocationMode.value, payload))
    message.success(`配置已按「${allocationModeText.value}」下发，等待从节点预检`)
  } catch (error) {
    message.error(String(error))
    await refreshSession()
  } finally {
    busy.value = false
  }
}

/* ---------- 变量体检 ---------- */

const AUDIT_KIND_TEXT: Record<string, string> = {
  user: '普通',
  secret: '密钥',
  os: '系统',
  pool: '数据池',
}

const AUDIT_SOURCE_TEXT: Record<string, string> = {
  env: '环境变量',
  os: '本机环境',
  pool: '数据池',
  none: '缺失',
}

const AUDIT_STATE_TEXT: Record<string, string> = {
  ok: '通过',
  missing: '缺失',
  pending: '待确认',
  unknown: '未知',
}

async function openVarAudit() {
  if (!session.value) return
  auditOpen.value = true
  auditLoading.value = true
  try {
    audit.value = await api.clusterVarAudit(session.value.jobId)
  } catch (error) {
    message.error(String(error))
  } finally {
    auditLoading.value = false
  }
}

function auditNodeState(row: VarAuditReport['items'][number], nodeId: string) {
  return row.nodeStates[nodeId]
}

function auditPolicyText(policy: string) {
  return policy === 'local_wins' ? '从节点优先' : '主节点优先'
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
        resetLiveSamples()
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

/** 卡片里的紧凑写法：± 不确定度太长会把四列挤到省略号，完整值放 title */
function clockShort(node: ClusterNodeInfo): string {
  if (node.clockOffsetMs == null) return '—'
  const offset = `${node.clockOffsetMs >= 0 ? '+' : ''}${fmt(node.clockOffsetMs)}`
  return `${offset} ms`
}

/**
 * 峰值行（灰度小字）：本次配对以来的 QPS / 并发 / 整机 CPU / 内存 / 网卡最高点。
 * 卡片里所有「峰值」都收在这一行，实时值留在上面的仪表条与指标格里，口径统一。
 */
function peakLineText(node: ClusterNodeInfo): string {
  const parts: string[] = []
  if (node.peakQps > 0) parts.push(`QPS ${fmtInt(node.peakQps)}`)
  if (node.peakConcurrent > 0) parts.push(`并发 ${fmtInt(node.peakConcurrent)}`)
  if (node.peakHostCpuPercent != null) parts.push(`CPU ${Math.round(node.peakHostCpuPercent)}%`)
  if (node.peakHostMemPercent != null) parts.push(`内存 ${Math.round(node.peakHostMemPercent)}%`)
  if (node.peakNetKbps != null) parts.push(`网卡 ${fmt(node.peakNetKbps / 1024, 1)} MB/s`)
  return parts.length ? `峰值 ${parts.join(' · ')}` : ''
}

/** hover 里补上「峰值 vs 当前」，避免只看到峰值却不知道距离多远 */
function peakLineTitle(node: ClusterNodeInfo): string {
  const rows: string[] = []
  if (node.peakQps > 0) rows.push(`QPS 峰值 ${fmtInt(node.peakQps)}（当前 ${fmtInt(node.qps)}）`)
  if (node.peakConcurrent > 0) {
    rows.push(`并发峰值 ${fmtInt(node.peakConcurrent)}（当前 ${fmtInt(node.concurrent)}）`)
  }
  const pair = (label: string, peak: number | null | undefined, now: string) => {
    if (peak == null) return
    rows.push(`${label}峰值 ${Math.round(peak)}%（当前 ${now}）`)
  }
  pair('整机 CPU ', node.peakHostCpuPercent, `${Math.round(node.hostCpuPercent ?? 0)}%`)
  pair('整机内存 ', node.peakHostMemPercent, `${Math.round(node.hostMemPercent ?? 0)}%`)
  if (node.peakNetKbps != null) {
    const now = (node.netRxKbps ?? 0) + (node.netTxKbps ?? 0)
    rows.push(`网卡峰值 ${fmt(node.peakNetKbps / 1024, 1)} MB/s（当前 ${fmt(now / 1024, 1)} MB/s）`)
  }
  if (!rows.length) return ''
  return `本次配对以来最高值\n${rows.join('\n')}`
}

/** 运行期漂移：每秒复测一次偏移，超过 50ms 说明两台机器的时钟在跑偏 */
function driftText(node: ClusterNodeInfo): string {
  if (!node.clockRecheckAt || node.clockDriftMs == null) return ''
  const drift = node.clockDriftMs
  if (Math.abs(drift) < 1) return '漂移 <1 ms'
  return `漂移 ${drift > 0 ? '+' : ''}${fmt(drift)} ms`
}

function driftTone(node: ClusterNodeInfo): string {
  if (node.clockDriftMs == null || !node.clockRecheckAt) return ''
  return Math.abs(node.clockDriftMs) >= 50 ? 'bad' : ''
}

function resourceText(node: ClusterNodeInfo): string {
  if (node.cpu == null && node.memMb == null && node.hostCpuPercent == null) return '—'
  const parts: string[] = []
  if (node.cpu != null) parts.push(`引擎 ${fmt(node.cpu, 2)} 核`)
  if (node.hostCpuPercent != null) parts.push(`整机 ${Math.round(node.hostCpuPercent)}%`)
  if (node.memMb != null) parts.push(`${fmtInt(node.memMb)} MB`)
  if (node.netRxKbps != null || node.netTxKbps != null) {
    const rx = node.netRxKbps == null ? '—' : fmt(node.netRxKbps / 1024, 1)
    const tx = node.netTxKbps == null ? '—' : fmt(node.netTxKbps / 1024, 1)
    parts.push(`↓${rx} ↑${tx} MB/s`)
  }
  return parts.length ? parts.join(' · ') : '—'
}

const NODE_STATUS_TEXT: Record<string, string> = {
  pending: '待接受',
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

const NODE_STATUS_TONE: Record<string, Tone> = {
  pending: 'warn' as Tone,
  pairing: 'idle',
  pairReady: 'info',
  preparing: 'info',
  preflight: 'info',
  ready: 'ok',
  running: 'live',
  done: 'ok',
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

const HEALTH_TEXT: Record<string, string> = {
  ok: '正常',
  waiting: '等待首样',
  gap: '采样缺口',
  lost: '无数据',
  done: '正常结束',
}

const HEALTH_TONE: Record<string, Tone> = {
  ok: 'ok',
  waiting: 'warn',
  gap: 'warn',
  lost: 'bad',
  done: 'ok',
}

function healthText(node: ClusterNodeInfo): string {
  return HEALTH_TEXT[node.health?.state ?? ''] ?? node.health?.state ?? '—'
}

function healthTone(node: ClusterNodeInfo): Tone {
  return HEALTH_TONE[node.health?.state ?? ''] ?? 'idle'
}

function healthTitle(node: ClusterNodeInfo): string {
  return node.health?.message || HEALTH_TEXT[node.health?.state ?? ''] || '—'
}

function capacityText(node: ClusterNodeInfo): string {
  const capacity = node.capacity
  if (!capacity) return '未上报'
  return `${capacity.cores} 核 · ${capacity.memGb} GB · 预算 ${capacity.machineLimit}`
}

function fmtMb(bytes: number | null | undefined): string {
  if (bytes == null) return '—'
  const mb = Number(bytes) / 1024 / 1024
  return mb >= 10 ? `${mb.toFixed(0)} MB` : `${mb.toFixed(1)} MB`
}

/** 从节点历史里的资源峰值：进程口径 + 整机口径 + 网卡 */
function slaveResourceText(item: ClusterSlaveHistoryItem): string {
  if (!item.cpuPeak && !item.memPeakMb && !item.hostCpuPeak) return '—'
  const parts: string[] = []
  if (item.cpuPeak) parts.push(`${fmt(item.cpuPeak, 2)} 核`)
  if (item.hostCpuPeak) parts.push(`整机 ${fmt(item.hostCpuPeak, 0)}%`)
  if (item.memPeakMb) parts.push(`${fmtInt(item.memPeakMb)} MB`)
  if (item.netPeakKbps) parts.push(`↓↑ ${fmt(item.netPeakKbps / 1024, 1)} MB/s`)
  return parts.join(' · ')
}

/** 节点兼容性说明：缺哪些能力，鼠标悬停可见 */
function compatTitle(node: ClusterNodeInfo): string {
  const lacks: string[] = []
  if (!node.poolColumns) lacks.push('数据池自定义列 / 自动注入')
  if (!node.envBundle) lacks.push('加密环境变量下发（将只读本机同名环境）')
  return lacks.length ? `旧版节点：不支持${lacks.join('、')}` : '支持数据池自定义列与加密环境变量'
}

function nodeCompatText(node: ClusterNodeInfo): string {
  return node.poolColumns && node.envBundle ? '新版' : '旧版'
}

function peakText(node: ClusterNodeInfo): string {
  return `${fmt(node.peakQps)} / ${fmtInt(node.peakConcurrent)}`
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
  { title: '兼容', key: 'compat', width: 88 },
  { title: '状态', key: 'status', width: 110 },
  { title: '权重', key: 'weight', width: 110 },
  { title: '并发范围', key: 'limits', width: 150 },
  { title: '分配并发', key: 'concurrent', width: 92, align: 'right' as const },
  { title: '能力', key: 'capacity', width: 180 },
  { title: '采集健康', key: 'health', width: 110 },
  { title: '峰值 QPS/并发', key: 'peak', width: 128, align: 'right' as const },
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

/** 节点展示形态：卡片看配置与资源，表格看 13 列密集对照；选择记住 */
const NODE_VIEW_KEY = 'cluster.nodeView'
const nodeView = ref<'card' | 'table'>(
  (() => {
    try {
      return localStorage.getItem(NODE_VIEW_KEY) === 'table' ? 'table' : 'card'
    } catch {
      return 'card'
    }
  })(),
)
watch(nodeView, (value) => {
  try {
    localStorage.setItem(NODE_VIEW_KEY, value)
  } catch {
    /* 隐私模式下写不进去，忽略 */
  }
})
const nodeViewOptions = [
  { label: '卡片', value: 'card' },
  { label: '表格', value: 'table' },
]

/** 卡片里的 CPU / 内存仪表：超过 70% 转警示色，超过 85% 转危险色 */
function meterTone(value: number | null | undefined): string {
  if (value == null) return ''
  if (value >= 85) return 'bad'
  if (value >= 70) return 'warn'
  return ''
}

function meterWidth(value: number | null | undefined): string {
  if (value == null) return '0%'
  return `${Math.max(0, Math.min(100, Math.round(value)))}%`
}

/** 集群审批：接受 / 拒绝 / 自动接受 */
const joinBusy = ref<string | null>(null)
const autoBusy = ref(false)

async function approveJoin(nodeId: string) {
  const current = session.value
  if (!current) return
  joinBusy.value = nodeId
  try {
    const next = await api.clusterMasterApprove(current.jobId, nodeId)
    session.value = next
    message.success('已接受节点，正在下发配置')
  } catch (error) {
    message.error(String(error))
  } finally {
    joinBusy.value = null
  }
}

async function rejectJoin(nodeId: string) {
  const current = session.value
  if (!current) return
  joinBusy.value = nodeId
  try {
    const next = await api.clusterMasterReject(current.jobId, nodeId, '主机拒绝了加入申请')
    session.value = next
    message.info('已拒绝该节点')
  } catch (error) {
    message.error(String(error))
  } finally {
    joinBusy.value = null
  }
}

async function toggleAutoAccept(value: boolean) {
  const current = session.value
  if (!current) return
  autoBusy.value = true
  try {
    session.value = await api.clusterMasterAutoAccept(current.jobId, value)
  } catch (error) {
    message.error(String(error))
  } finally {
    autoBusy.value = false
  }
}

const pendingJoins = computed(() => session.value?.pending ?? [])

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

/* ---------- 历史记录 ---------- */

const historyLoading = ref(false)
const historyStatus = ref('')
const historyKeyword = ref('')
const historyPage = ref(1)
const historyPageSize = 20
const historyTotal = ref(0)
const historyItems = ref<ClusterJobSummary[]>([])
const historyDetailOpen = ref(false)
const historyDetailLoading = ref(false)
const historyDetail = ref<ClusterJobDetail | null>(null)

const HISTORY_STATUS_OPTIONS = [
  { value: '', label: '全部状态' },
  { value: 'finished', label: '已完成' },
  { value: 'failed', label: '失败' },
  { value: 'running', label: '运行中' },
]

const HISTORY_STATUS_TEXT: Record<string, string> = {
  pairing: '配对中',
  preparing: '准备中',
  ready: '就绪',
  running: '运行中',
  finished: '已完成',
  failed: '失败',
  idle: '空闲',
}

const historyColumns = [
  { title: '创建时间', key: 'createdAt', width: 170 },
  { title: '计划', dataIndex: 'planName', key: 'planName', ellipsis: true },
  { title: '节点', dataIndex: 'nodeCount', key: 'nodeCount', width: 70 },
  { title: '总并发', dataIndex: 'totalConcurrent', key: 'totalConcurrent', width: 90 },
  { title: '平均 QPS', key: 'avgQps', width: 110 },
  { title: '状态', key: 'status', width: 100 },
  { title: '操作', key: 'action', width: 140 },
]

const historyNodeColumns = [
  { title: '节点', dataIndex: 'nodeName', key: 'nodeName', ellipsis: true },
  { title: '角色', key: 'role', width: 70 },
  { title: '地址', dataIndex: 'addr', key: 'addr', width: 150, ellipsis: true },
  { title: '并发', dataIndex: 'concurrent', key: 'concurrent', width: 70 },
  { title: 'QPS', key: 'qps', width: 90 },
  { title: 'CPU', key: 'cpu', width: 90 },
  { title: '状态', key: 'status', width: 90 },
]

function historyStatusText(status: string) {
  return HISTORY_STATUS_TEXT[status] ?? status
}

function historyStatusTone(status: string): Tone {
  if (status === 'finished') return 'ok'
  if (status === 'failed') return 'bad'
  if (status === 'running') return 'live'
  if (status === 'preparing' || status === 'pairing' || status === 'ready') return 'info'
  return 'idle'
}

async function loadHistory() {
  historyLoading.value = true
  try {
    const page = await api.clusterJobsList(
      historyPage.value,
      historyPageSize,
      historyStatus.value,
      historyKeyword.value.trim(),
    )
    historyItems.value = page.items
    historyTotal.value = page.total
  } catch (error) {
    message.error(`读取历史失败：${String(error)}`)
  } finally {
    historyLoading.value = false
  }
}

function applyHistoryFilter() {
  historyPage.value = 1
  void loadHistory()
}

function onHistoryPageChange(pagination: { current?: number }) {
  historyPage.value = pagination.current ?? 1
  void loadHistory()
}

async function openHistoryDetail(record: ClusterJobSummary) {
  historyDetailOpen.value = true
  historyDetailLoading.value = true
  historyDetail.value = null
  try {
    historyDetail.value = await api.clusterJobDetail(record.id)
  } catch (error) {
    message.error(String(error))
  } finally {
    historyDetailLoading.value = false
  }
}

function historyRowProps(record: ClusterJobSummary) {
  return { onClick: () => void openHistoryDetail(record) }
}

function clearHistory() {
  Modal.confirm({    title: '清理历史任务',
    content: '只删除已结束的任务记录，运行中的任务不受影响。',
    okText: '清理',
    cancelText: '取消',
    onOk: async () => {
      try {
        const removed = await api.clusterJobsClear(0)
        message.success(`已清理 ${removed} 条历史`)
        historyPage.value = 1
        await loadHistory()
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

/* ---------- 从节点历史 ---------- */

const slaveHistory = ref<ClusterSlaveHistoryItem[]>([])
const slaveHistoryLoading = ref(false)

const slaveHistoryColumns = [
  { title: '时间', key: 'createdAt', width: 160 },
  { title: '主节点', dataIndex: 'masterAddr', key: 'masterAddr', width: 160, ellipsis: true },
  { title: '计划', dataIndex: 'planName', key: 'planName', ellipsis: true },
  { title: '并发', dataIndex: 'concurrent', key: 'concurrent', width: 70 },
  { title: '峰值 QPS', key: 'qpsAvg', width: 100 },
  { title: '本机资源峰值', key: 'resources', width: 210 },
  { title: '状态', key: 'status', width: 90 },
]

async function loadSlaveHistory() {
  slaveHistoryLoading.value = true
  try {
    slaveHistory.value = await api.clusterSlaveHistoryList(50, 0)
  } catch {
    slaveHistory.value = []
  } finally {
    slaveHistoryLoading.value = false
  }
}

function clearSlaveHistory() {
  Modal.confirm({
    title: '清空本机参与记录',
    content: '只删除本机的配对/执行历史，不影响主节点上的报告。',
    okText: '清空',
    cancelText: '取消',
    onOk: async () => {
      try {
        const removed = await api.clusterSlaveHistoryClear()
        message.success(`已清空 ${removed} 条记录`)
        await loadSlaveHistory()
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

watch(activeTab, (tab) => {
  if (tab === 'history') void loadHistory()
  if (tab === 'slave') void loadSlaveHistory()
})

/* ---------- 生命周期 ---------- */

let liveTimer: number | undefined

function startLiveTimer() {
  if (liveTimer) window.clearInterval(liveTimer)
  liveTimer = window.setInterval(() => commitLiveSamples(), liveRefreshSec.value * 1000)
}

function onVisibilityChange() {
  if (!document.hidden) commitLiveSamples()
}

watch(liveRefreshSec, (sec) => {
  try {
    localStorage.setItem(LIVE_REFRESH_KEY, String(sec))
  } catch {
    /* 忽略存储失败 */
  }
  startLiveTimer()
})

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

/* ---------- 局域网发现（从节点） ---------- */
const discovered = ref<DiscoveredMaster[]>([])
const discoveryOn = ref(false)
const discoveryBusy = ref(false)
let discoveryTimer: number | undefined

async function refreshDiscovered() {
  try {
    discovered.value = await api.clusterDiscoveryList()
  } catch {
    discovered.value = []
  }
}

async function toggleDiscovery() {
  discoveryBusy.value = true
  try {
    if (discoveryOn.value) {
      await api.clusterDiscoveryStop()
      discoveryOn.value = false
      discovered.value = []
      if (discoveryTimer) {
        window.clearInterval(discoveryTimer)
        discoveryTimer = undefined
      }
    } else {
      await api.clusterDiscoveryStart()
      discoveryOn.value = true
      await refreshDiscovered()
      discoveryTimer = window.setInterval(() => void refreshDiscovered(), 2000)
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    discoveryBusy.value = false
  }
}

async function joinDiscovered(item: DiscoveredMaster) {
  try {
    const snapshot = await api.clusterJoinDiscovered(
      item.addr,
      slaveForm.nodeName.trim() || null,
    )
    applySlave(snapshot)
    slaveForm.masterAddr = item.addr
    if (snapshot.status === 'pending') {
      message.info('已提交加入申请，请在主节点「加入申请」中接受')
    } else {
      message.success('已连接主节点')
    }
  } catch (error) {
    message.error(String(error))
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
    unlisteners.push(
      await listen<ClusterSampleFrame>('cluster:sample-frame', (event) => {
        const current = session.value
        if (!current || event.payload.jobId !== current.jobId) return
        // 一帧包含该秒所有节点：只写缓冲，图表按 liveRefreshSec 的节拍统一提交
        for (const node of event.payload.nodes) {
          pushLiveSample(event.payload.sec, node.nodeId, node.qps)
        }
      }),
    )
  } catch (error) {
    console.error('集群事件监听失败', error)
  }
  timer = window.setInterval(() => void tick(), 1000)
  startLiveTimer()
  document.addEventListener('visibilitychange', onVisibilityChange)
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
  if (liveTimer) window.clearInterval(liveTimer)
  if (discoveryTimer) window.clearInterval(discoveryTimer)
  document.removeEventListener('visibilitychange', onVisibilityChange)
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
                <div v-if="planStageAlert" class="notice warn">
                  <span class="nt">阶梯计划</span>
                  <span>{{ planStageAlert }}</span>
                </div>

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

                <a-button type="primary" :loading="creating" :disabled="!canCreate" @click="createSession">
                  <ThunderboltOutlined />
                  创建任务并生成配对码
                </a-button>
                <p class="hint">
                  总并发按节点权重拆分，主节点同样作为施压节点参与；分布式任务按固定并发执行。
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
                <a-button size="small" :loading="auditLoading" @click="openVarAudit">
                  变量体检
                </a-button>
                <a-button
                  v-if="session.status === 'pairing'"
                  type="primary"
                  size="small"
                  :loading="busy"
                  :disabled="!hasSlave || compatBlocked"
                  :title="prepareDisabledReason"
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

              <div v-if="session.message" class="notice" :class="session.incomplete ? 'warn' : 'info'">
                <span class="nt">{{ session.incomplete ? '注意' : '状态' }}</span>
                <span>{{ session.message }}</span>
              </div>
              <div v-else-if="session.status === 'pairing'" class="notice info">
                <span class="nt">等待</span>
                <span>等待从节点加入。至少加入 1 个从节点后，点「下发配置」按「{{ allocationModeText }}」拆分并发并做目标预检。</span>
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

          <section v-if="session.status === 'pairing'" class="fm-card panel">
            <header class="panel-head">
              <div class="panel-title">
                <h3>加入申请</h3>
                <span class="panel-sub">
                  待接受 {{ pendingJoins.length }} 个 · 局域网发现
                  {{ session.discovery ? '已开启' : '已关闭' }}
                </span>
              </div>
              <div class="panel-actions">
                <span class="hint">自动接受</span>
                <a-switch
                  :checked="session.autoAccept"
                  :loading="autoBusy"
                  size="small"
                  @change="(value: unknown) => toggleAutoAccept(value === true)"
                />
              </div>
            </header>
            <div class="panel-body">
              <div v-if="!pendingJoins.length" class="inline-empty">
                暂无申请。从节点可通过局域网发现申请加入，或输入配对码连接。
              </div>
              <div v-for="item in pendingJoins" :key="item.requestId" class="join-row">
                <div class="join-main">
                  <div class="join-name">{{ item.name }}</div>
                  <div class="join-meta mono">
                    {{ item.addr }} · {{ item.deviceId || '未上报设备' }} · 申请于
                    {{ fmtTime(item.requestedAt) }}
                  </div>
                  <div v-if="item.capacity" class="join-meta mono">
                    {{ item.capacity.cores }} 核 · {{ item.capacity.memGb }} GB · 预算
                    {{ item.capacity.machineLimit }}
                  </div>
                </div>
                <div class="join-actions">
                  <a-button
                    size="small"
                    type="primary"
                    :loading="joinBusy === item.nodeId"
                    @click="approveJoin(item.nodeId)"
                  >
                    接受
                  </a-button>
                  <a-button
                    size="small"
                    danger
                    :disabled="joinBusy === item.nodeId"
                    @click="rejectJoin(item.nodeId)"
                  >
                    拒绝
                  </a-button>
                </div>
              </div>
            </div>
          </section>

          <section v-if="liveElapsed.length" class="fm-card panel chart-panel">
            <header class="panel-head">
              <div class="panel-title">
                <h3>各节点 QPS</h3>
                <span class="panel-sub">
                  {{ liveElapsed.length }} 个采样点 · 每 {{ liveRefreshSec }} 秒刷新一次
                </span>
              </div>
              <div class="panel-actions">
                <a-select v-model:value="liveRefreshSec" size="small" style="width: 120px">
                  <a-select-option :value="1">实时 · 1 秒</a-select-option>
                  <a-select-option :value="2">平衡 · 2 秒</a-select-option>
                  <a-select-option :value="5">省资源 · 5 秒</a-select-option>
                </a-select>
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
                <span class="panel-sub">准备时按「{{ allocationModeText }}」拆分总并发</span>
              </div>
              <div class="panel-actions">
                <a-segmented
                  v-model:value="nodeView"
                  size="small"
                  :options="nodeViewOptions"
                />
                <template v-if="session.status === 'pairing'">
                  <a-select v-model:value="allocationMode" size="small" style="width: 150px">
                    <a-select-option value="auto">能力分配</a-select-option>
                    <a-select-option value="even">平均分配</a-select-option>
                    <a-select-option value="custom">自定义权重</a-select-option>
                  </a-select>
                  <a-select v-model:value="varPolicy" size="small" style="width: 140px">
                    <a-select-option value="master_wins">变量主节点优先</a-select-option>
                    <a-select-option value="local_wins">变量本机优先</a-select-option>
                  </a-select>
                </template>
              </div>
            </header>

            <div v-if="nodeView === 'card' && nodeList.length" class="node-cards">
              <article
                v-for="record in nodeList"
                :key="record.id"
                class="node-card"
                :class="[record.role, `tone-${nodeStatusTone(record.status)}`]"
              >
                <header class="nc-head">
                  <span class="nc-avatar" aria-hidden="true">
                    <svg viewBox="0 0 16 16" width="15" height="15" fill="none">
                      <rect
                        x="2.5"
                        y="2.5"
                        width="11"
                        height="11"
                        rx="2"
                        stroke="currentColor"
                        stroke-width="1.2"
                      />
                      <rect x="5.5" y="5.5" width="5" height="5" rx="1" fill="currentColor" />
                      <path
                        d="M5 1v1.5M8 1v1.5M11 1v1.5M5 13.5V15M8 13.5V15M11 13.5V15M1 5h1.5M1 8h1.5M1 11h1.5M13.5 5H15M13.5 8H15M13.5 11H15"
                        stroke="currentColor"
                        stroke-width="1.1"
                        stroke-linecap="round"
                      />
                    </svg>
                  </span>
                  <div class="nc-title">
                    <div class="node-name" :title="record.name">{{ record.name }}</div>
                    <div class="node-addr mono" :title="record.addr || ''">
                      {{ record.addr || '—' }}
                    </div>
                  </div>
                  <div class="nc-pills">
                    <span class="role-pill" :class="record.role">
                      {{ record.role === 'master' ? '主节点' : '从节点' }}
                    </span>
                    <span
                      class="st-pill"
                      :class="nodeStatusTone(record.status)"
                      :title="record.error || undefined"
                    >
                      <i class="pill-dot" />
                      {{ nodeStatusText(record.status) }}
                    </span>
                  </div>
                </header>

                <div class="nc-specs">
                  <div class="nc-spec">
                    <span class="k">CPU</span>
                    <span class="v">{{ record.capacity ? `${record.capacity.cores} 核` : '未上报' }}</span>
                  </div>
                  <div class="nc-spec">
                    <span class="k">内存</span>
                    <span class="v">{{ record.capacity ? `${record.capacity.memGb} GB` : '未上报' }}</span>
                  </div>
                  <div class="nc-spec">
                    <span class="k">预算并发</span>
                    <span class="v">{{ record.capacity ? fmtInt(record.capacity.machineLimit) : '未上报' }}</span>
                  </div>
                </div>

                <div
                  v-if="record.hostCpuPercent != null || record.hostMemPercent != null"
                  class="nc-meters"
                >
                  <div v-if="record.hostCpuPercent != null" class="nc-meter">
                    <div class="nc-meter-top">
                      <span>整机 CPU</span>
                      <span class="mono">{{ Math.round(record.hostCpuPercent) }}%</span>
                    </div>
                    <div class="nc-track">
                      <i
                        class="nc-fill"
                        :class="meterTone(record.hostCpuPercent)"
                        :style="{ width: meterWidth(record.hostCpuPercent) }"
                      />
                    </div>
                  </div>
                  <div v-if="record.hostMemPercent != null" class="nc-meter">
                    <div class="nc-meter-top">
                      <span>整机内存</span>
                      <span class="mono">{{ Math.round(record.hostMemPercent) }}%</span>
                    </div>
                    <div class="nc-track">
                      <i
                        class="nc-fill"
                        :class="meterTone(record.hostMemPercent)"
                        :style="{ width: meterWidth(record.hostMemPercent) }"
                      />
                    </div>
                  </div>
                </div>

                <div class="nc-metrics">
                  <div class="nc-metric">
                    <span class="k">分配并发</span>
                    <span v-if="session.status === 'pairing'" class="v mono planned-value">
                      ≈ {{ fmtInt(plannedConcurrent[record.id]) }}
                    </span>
                    <span v-else class="v mono">{{ fmtInt(record.concurrent) }}</span>
                  </div>
                  <div class="nc-metric">
                    <span class="k">QPS</span>
                    <span class="v mono">{{ fmt(record.qps) }}</span>
                  </div>
                  <div class="nc-metric">
                    <span class="k">时钟偏移</span>
                    <span class="v mono" :title="clockText(record)">{{ clockShort(record) }}</span>
                  </div>
                </div>

                <div v-if="peakLineText(record)" class="nc-peak" :title="peakLineTitle(record)">
                  {{ peakLineText(record) }}
                </div>

                <div class="nc-foot">
                  <!-- custom 模式下权重与范围就在卡片底部可编辑，这里不再重复展示 -->
                  <span
                    v-if="!(session.status === 'pairing' && allocationMode === 'custom')"
                    class="nc-chip"
                  >
                    并发范围 {{ limitText(record) }}
                  </span>
                  <span v-if="record.netRxKbps != null || record.netTxKbps != null" class="nc-chip">
                    ↓{{ fmt((record.netRxKbps ?? 0) / 1024, 1) }} ↑{{
                      fmt((record.netTxKbps ?? 0) / 1024, 1)
                    }}
                    MB/s
                  </span>
                  <span v-if="record.threads != null" class="nc-chip">线程 {{ fmtInt(record.threads) }}</span>
                  <span v-if="record.load1 != null" class="nc-chip">负载 {{ fmt(record.load1, 2) }}</span>
                  <span
                    class="st-pill"
                    :class="healthTone(record)"
                    :title="healthTitle(record)"
                  >
                    <i class="pill-dot" />
                    {{ healthText(record) }}
                  </span>
                  <span
                    v-if="record.role === 'slave'"
                    class="st-pill"
                    :class="record.poolColumns && record.envBundle ? 'ok' : 'warn'"
                    :title="compatTitle(record)"
                  >
                    {{ nodeCompatText(record) }}
                  </span>
                </div>

                <div
                  v-if="session.status === 'pairing' && allocationMode === 'custom'"
                  class="nc-edit"
                >
                  <span class="nc-edit-label">权重</span>
                  <a-input-number
                    :value="weights[record.id] ?? record.weight"
                    :min="0.1"
                    :max="10"
                    :step="0.1"
                    size="small"
                    class="weight-input"
                    @update:value="setWeight(record, $event)"
                  />
                  <span class="nc-edit-label">并发</span>
                  <div class="limit-inputs">
                    <a-input-number
                      :value="nodeLimits[record.id]?.min ?? 1"
                      :min="1"
                      size="small"
                      class="limit-input"
                      @update:value="setLimit(record, 'min', $event)"
                    />
                    <span class="limit-sep">~</span>
                    <a-input-number
                      :value="nodeLimits[record.id]?.max ?? null"
                      :min="1"
                      placeholder="预算"
                      size="small"
                      class="limit-input"
                      @update:value="setLimit(record, 'max', $event)"
                    />
                  </div>
                </div>

                <div v-if="record.poolSliceBytes != null" class="nc-foot">
                  <span
                    class="nc-chip"
                    :class="{ over: record.poolSliceBytes >= poolSliceLimit * 0.8 }"
                  >
                    池切片 {{ fmtMb(record.poolSliceBytes) }} / {{ fmtMb(poolSliceLimit) }}
                  </span>
                </div>

                <div v-if="record.error" class="node-error">{{ record.error }}</div>
                <div v-if="driftText(record)" class="mono clock-drift" :class="driftTone(record)">
                  {{ driftText(record) }}
                </div>
              </article>
            </div>

            <a-table
              v-if="nodeView === 'table'"
              :columns="nodeColumns"
              :data-source="nodeList"
              row-key="id"
              size="small"
              :pagination="false"
              :scroll="{ x: 1250 }"
            >
              <template #bodyCell="{ column, record }">
                <template v-if="column.key === 'name'">
                  <div class="node-name">{{ record.name }}</div>
                  <div class="node-addr mono">{{ record.addr || '—' }}</div>
                </template>
                <template v-else-if="column.key === 'role'">
                  <span class="role-pill" :class="record.role">
                    {{ record.role === 'master' ? '主节点' : '从节点' }}
                  </span>
                </template>
                <template v-else-if="column.key === 'compat'">
                  <span v-if="record.role === 'master'" class="mono">—</span>
                  <span
                    v-else
                    class="st-pill"
                    :class="record.poolColumns && record.envBundle ? 'ok' : 'warn'"
                    :title="compatTitle(record)"
                  >
                    {{ nodeCompatText(record) }}
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
                    v-if="session.status === 'pairing' && allocationMode === 'custom'"
                    :value="weights[record.id] ?? record.weight"
                    :min="0.1"
                    :max="10"
                    :step="0.1"
                    size="small"
                    class="weight-input"
                    @update:value="setWeight(record, $event)"
                  />
                  <span v-else class="mono">
                    {{
                      session.status === 'pairing'
                        ? allocationMode === 'even'
                          ? '1'
                          : '按能力'
                        : fmt(record.weight, 2)
                    }}
                  </span>
                </template>
                <template v-else-if="column.key === 'limits'">
                  <div
                    v-if="session.status === 'pairing' && allocationMode === 'custom'"
                    class="limit-inputs"
                  >
                    <a-input-number
                      :value="nodeLimits[record.id]?.min ?? 1"
                      :min="1"
                      size="small"
                      class="limit-input"
                      @update:value="setLimit(record, 'min', $event)"
                    />
                    <span class="limit-sep">~</span>
                    <a-input-number
                      :value="nodeLimits[record.id]?.max ?? null"
                      :min="1"
                      placeholder="预算"
                      size="small"
                      class="limit-input"
                      @update:value="setLimit(record, 'max', $event)"
                    />
                  </div>
                  <span v-else class="mono">{{ limitText(record) }}</span>
                </template>
                <template v-else-if="column.key === 'qps'">
                  <span class="mono">{{ fmt(record.qps) }}</span>
                </template>
                <template v-else-if="column.key === 'capacity'">
                  <div class="mono">{{ capacityText(record) }}</div>
                  <div
                    v-if="record.poolSliceBytes != null"
                    class="mono pool-slice"
                    :class="{ over: record.poolSliceBytes >= poolSliceLimit * 0.8 }"
                  >
                    池切片 {{ fmtMb(record.poolSliceBytes) }} / {{ fmtMb(poolSliceLimit) }}
                  </div>
                </template>
                <template v-else-if="column.key === 'health'">
                  <span class="st-pill" :class="healthTone(record)" :title="healthTitle(record)">
                    <i class="pill-dot" />
                    {{ healthText(record) }}
                  </span>
                </template>
                <template v-else-if="column.key === 'peak'">
                  <span class="mono">{{ peakText(record) }}</span>
                </template>
                <template v-else-if="column.key === 'concurrent'">
                  <span v-if="session.status === 'pairing'" class="mono planned-value">
                    ≈ {{ fmtInt(plannedConcurrent[record.id]) }}
                  </span>
                  <span v-else class="mono">{{ fmtInt(record.concurrent) }}</span>
                </template>
                <template v-else-if="column.key === 'clock'">
                  <span class="mono">{{ clockText(record) }}</span>
                  <div
                    v-if="driftText(record)"
                    class="mono clock-drift"
                    :class="driftTone(record)"
                    :title="`运行期每 60 秒复测一次；漂移超过 50 ms 说明两台机器时钟在跑偏`"
                  >
                    {{ driftText(record) }}
                  </div>
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
              <div v-if="allocationProblem" class="notice warn">
                <span class="nt">分配</span>
                <span>{{ allocationProblem }}，请调整总并发或节点范围后再下发。</span>
              </div>
              <div v-if="compatBlocked" class="notice bad">
                <span class="nt">版本</span>
                <span>
                  节点 {{ legacySlaves.map((node) => node.name).join('、') }}
                  版本较低，不支持数据池自定义列或自动注入；请升级后重新加入，或把池绑定改为「取 value 列」。
                </span>
              </div>
              <div v-else-if="envFallbackSlaves.length" class="notice warn">
                <span class="nt">环境</span>
                <span>
                  节点 {{ envFallbackSlaves.map((node) => node.name).join('、') }}
                  未提供加密公钥，只能读取本机同名环境；变量值可能与主节点不一致。
                </span>
              </div>
              <div
                v-if="poolEstimate"
                class="notice"
                :class="poolEstimate.bytes > poolSliceLimit ? 'warn' : 'info'"
              >
                <span class="nt">数据池</span>
                <span>
                  合计约 {{ fmtMb(poolEstimate.bytes) }}（{{ fmtInt(poolEstimate.items) }} 条 /
                  {{ poolEstimate.pools }} 个池），单节点切片上限 {{ fmtMb(poolSliceLimit) }}。
                  <template v-if="poolEstimate.bytes > poolSliceLimit">
                    当前体量已超过单节点上限，建议增加从节点分摊或精简池条目。
                  </template>
                </span>
              </div>
              <div v-if="nearLimitNodes.length" class="notice warn">
                <span class="nt">切片</span>
                <span>
                  节点 {{ nearLimitNodes.map((node) => node.name).join('、') }}
                  的池切片已接近上限；可减少池条目或增加节点分摊。
                </span>
              </div>
              <div v-if="!nodeList.length" class="inline-empty">还没有节点</div>
              <p v-else-if="session.status === 'pairing'" class="hint">
                从节点加入后会出现在这里。能力分配按各机预算、平均分配无差别拆分、自定义权重可单独设置每节点最小/最大并发；「分配并发」为预估，下发配置后以实际分配为准。
              </p>
            </div>
          </section>
        </template>
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
                <div class="discover-head">
                  <div>
                    <div class="discover-title">附近的主节点</div>
                    <div class="hint">监听 UDP 17802；发现后申请加入，需主机接受</div>
                  </div>
                  <a-button size="small" :loading="discoveryBusy" @click="toggleDiscovery">
                    {{ discoveryOn ? '停止发现' : '开始发现' }}
                  </a-button>
                </div>
                <div v-if="discoveryOn" class="discover-list">
                  <div v-if="!discovered.length" class="inline-empty">
                    正在搜索局域网内的主节点…
                  </div>
                  <div v-for="item in discovered" :key="item.instanceId" class="join-row">
                    <div class="join-main">
                      <div class="join-name">{{ item.name }}</div>
                      <div class="join-meta mono">
                        {{ item.addr }} · {{ item.status }} · {{ item.nodeCount }}/{{ item.maxNodes }}
                        <template v-if="item.capacity">
                          · {{ item.capacity.cores }} 核 · {{ Math.round(item.capacity.memMb / 1024) }} GB
                        </template>
                      </div>
                    </div>
                    <a-button size="small" type="primary" @click="joinDiscovered(item)">
                      申请加入
                    </a-button>
                  </div>
                </div>
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
                      <span>计划</span>
                      <b>{{ slave.planName || '等待下发' }}</b>
                    </div>
                    <div class="kv">
                      <span>分配并发</span>
                      <b class="mono">{{ slave.concurrent || '—' }}</b>
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

        <section class="fm-card panel history-panel">
          <header class="panel-head">
            <div class="panel-title">
              <h3>本机参与过的任务</h3>
              <span class="panel-sub">配对与执行记录，保留最近 50 条</span>
            </div>
            <div class="panel-actions">
              <a-button size="small" @click="loadSlaveHistory">刷新</a-button>
              <a-button size="small" danger @click="clearSlaveHistory">清空</a-button>
            </div>
          </header>
          <a-table
            size="small"
            :columns="slaveHistoryColumns"
            :data-source="slaveHistory"
            :loading="slaveHistoryLoading"
            :pagination="false"
            row-key="id"
            :scroll="{ y: 320 }"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'createdAt'">
                {{ fmtTime(record.createdAt) }}
              </template>
              <template v-else-if="column.key === 'qpsAvg'">
                {{ fmt(record.qpsAvg) }}
              </template>
              <template v-else-if="column.key === 'resources'">
                <span class="mono">{{ slaveResourceText(record) }}</span>
              </template>
              <template v-else-if="column.key === 'status'">
                <span class="st-pill" :class="historyStatusTone(record.status)">
                  <i class="pill-dot" />
                  {{ historyStatusText(record.status) }}
                </span>
              </template>
            </template>
          </a-table>
        </section>
      </a-tab-pane>

      <a-tab-pane key="history" tab="历史">
        <section class="fm-card panel">
          <header class="panel-head">
            <div class="panel-title">
              <h3>主节点任务历史</h3>
              <span class="panel-sub">配对与执行记录，点击行查看节点明细</span>
            </div>
            <div class="panel-actions history-actions">
              <a-select
                v-model:value="historyStatus"
                size="small"
                style="width: 120px"
                :options="HISTORY_STATUS_OPTIONS"
                @change="applyHistoryFilter"
              />
              <a-input-search
                v-model:value="historyKeyword"
                size="small"
                placeholder="计划或主节点名"
                style="width: 200px"
                @search="applyHistoryFilter"
              />
              <a-button size="small" @click="loadHistory">刷新</a-button>
              <a-button size="small" danger @click="clearHistory">清理</a-button>
            </div>
          </header>
          <a-table
            size="small"
            :columns="historyColumns"
            :data-source="historyItems"
            :loading="historyLoading"
            :pagination="{
              current: historyPage,
              pageSize: historyPageSize,
              total: historyTotal,
              showSizeChanger: false,
            }"
            row-key="id"
            :custom-row="historyRowProps"
            @change="onHistoryPageChange"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'createdAt'">
                {{ fmtTime(record.createdAt) }}
              </template>
              <template v-else-if="column.key === 'avgQps'">
                {{ record.avgQps ? fmt(record.avgQps) : '—' }}
              </template>
              <template v-else-if="column.key === 'status'">
                <span class="st-pill" :class="historyStatusTone(record.status)">
                  <i class="pill-dot" />
                  {{ historyStatusText(record.status) }}
                </span>
              </template>
              <template v-else-if="column.key === 'action'">
                <a-space>
                  <a @click.stop="openHistoryDetail(record)">详情</a>
                  <a v-if="record.reportId" @click.stop="router.push(`/reports/${record.reportId}`)">
                    报告
                  </a>
                </a-space>
              </template>
            </template>
          </a-table>
        </section>
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
            </a-descriptions>
            <p class="hint">改动需升级主从双方版本，避免协议不兼容。</p>
          </div>
        </section>
      </a-tab-pane>
    </a-tabs>

    <a-drawer
      v-model:open="historyDetailOpen"
      :title="historyDetail ? `任务 #${historyDetail.job.id}` : '任务详情'"
      placement="right"
      :width="760"
    >
      <a-spin :spinning="historyDetailLoading">
        <template v-if="historyDetail">
          <a-descriptions bordered size="small" :column="2" class="history-desc">
            <a-descriptions-item label="计划">
              {{ historyDetail.job.planName || '—' }}
            </a-descriptions-item>
            <a-descriptions-item label="主节点">
              {{ historyDetail.job.masterNode || '—' }}
            </a-descriptions-item>
            <a-descriptions-item label="创建时间">
              {{ fmtTime(historyDetail.job.createdAt) }}
            </a-descriptions-item>
            <a-descriptions-item label="结束时间">
              {{ fmtTime(historyDetail.job.finishedAt) }}
            </a-descriptions-item>
            <a-descriptions-item label="总并发">
              {{ historyDetail.job.totalConcurrent }}
            </a-descriptions-item>
            <a-descriptions-item label="状态">
              <span class="st-pill" :class="historyStatusTone(historyDetail.job.status)">
                <i class="pill-dot" />
                {{ historyStatusText(historyDetail.job.status) }}
              </span>
            </a-descriptions-item>
            <a-descriptions-item label="平均 QPS">
              {{ historyDetail.job.avgQps ? fmt(historyDetail.job.avgQps) : '—' }}
            </a-descriptions-item>
            <a-descriptions-item label="报告">
              <a
                v-if="historyDetail.job.reportId"
                @click="router.push(`/reports/${historyDetail.job.reportId}`)"
              >
                #{{ historyDetail.job.reportId }}
              </a>
              <span v-else>—</span>
            </a-descriptions-item>
          </a-descriptions>
          <a-alert
            v-if="historyDetail.job.incomplete"
            type="warning"
            show-icon
            class="history-alert"
            :message="historyDetail.job.message || '有节点失联或提前退出，结果不完整'"
          />
          <a-table
            size="small"
            :columns="historyNodeColumns"
            :data-source="historyDetail.nodes"
            :pagination="false"
            row-key="nodeName"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'role'">
                {{ record.role === 'master' ? '主' : '从' }}
              </template>
              <template v-else-if="column.key === 'qps'">{{ fmt(record.qps) }}</template>
              <template v-else-if="column.key === 'cpu'">
                {{ record.cpu == null ? '—' : `${fmt(record.cpu, 2)} 核` }}
              </template>
              <template v-else-if="column.key === 'status'">
                <span class="st-pill" :class="historyStatusTone(record.status)">
                  <i class="pill-dot" />
                  {{ historyStatusText(record.status) }}
                </span>
              </template>
            </template>
          </a-table>
        </template>
      </a-spin>
    </a-drawer>

    <a-modal
      v-model:open="auditOpen"
      title="变量体检"
      width="900px"
      :footer="null"
      :get-container="false"
    >
      <a-spin :spinning="auditLoading">
        <div v-if="audit" class="audit-body">
          <div class="audit-summary">
            <span>
              环境
              <b class="mono">{{ audit.envName || '未指定' }}</b>
            </span>
            <span>变量 <b>{{ audit.envVarCount }}</b></span>
            <span>密钥 <b>{{ audit.secretCount }}</b></span>
            <span>冲突策略 <b>{{ auditPolicyText(audit.varPolicy) }}</b></span>
            <span>从节点 <b>{{ audit.nodes.length }}</b></span>
          </div>

          <div v-if="!audit.items.length" class="audit-empty">
            当前配置没有引用环境变量、系统变量或数据池。
          </div>
          <div v-else class="audit-table-wrap">
            <table class="audit-table">
              <thead>
                <tr>
                  <th>变量</th>
                  <th>类型</th>
                  <th>来源</th>
                  <th>主节点</th>
                  <th v-for="node in audit.nodes" :key="node.id">{{ node.name }}</th>
                  <th>说明</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in audit.items" :key="`${row.kind}:${row.name}`">
                  <td class="mono audit-name">{{ row.name }}</td>
                  <td>{{ AUDIT_KIND_TEXT[row.kind] ?? row.kind }}</td>
                  <td>{{ AUDIT_SOURCE_TEXT[row.source] ?? row.source }}</td>
                  <td>
                    <span class="audit-chip" :class="row.masterHasValue ? 'ok' : 'bad'">
                      {{ row.masterHasValue ? '有值' : '无值' }}
                    </span>
                  </td>
                  <td v-for="node in audit.nodes" :key="node.id">
                    <span
                      class="audit-chip"
                      :class="auditNodeState(row, node.id)?.state ?? 'pending'"
                      :title="auditNodeState(row, node.id)?.detail ?? ''"
                    >
                      {{ AUDIT_STATE_TEXT[auditNodeState(row, node.id)?.state ?? 'pending'] ?? '待确认' }}
                    </span>
                  </td>
                  <td class="audit-message">{{ row.message || '—' }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <p class="audit-hint">
            体检只读：不会下发配置、不会读取变量值。主节点「无值」的普通变量 / 密钥会在下发时被从节点拦截；
            系统变量需要在「设置 → 压测 → OS 变量」加入白名单；数据池变量需要先在计划里绑定池。
          </p>
        </div>
      </a-spin>
    </a-modal>
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

.join-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--fm-border);
}

.join-row:last-child {
  border-bottom: none;
}

.join-name {
  color: var(--fm-text);
  font-weight: 600;
}

.join-meta {
  margin-top: 2px;
  font-size: 11.5px;
  color: var(--fm-muted);
  word-break: break-all;
}

.join-actions {
  display: flex;
  gap: 8px;
  flex: none;
}

.discover-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 10px;
  margin-bottom: 12px;
  border-bottom: 1px solid var(--fm-border);
}

.discover-title {
  color: var(--fm-text);
  font-weight: 600;
}

.discover-list {
  margin-bottom: 14px;
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

/* ---------- 节点卡片（配置 + 资源一览） ---------- */
.node-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(318px, 1fr));
  gap: 12px;
  padding: 14px 16px 4px;
}

.node-card {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 11px;
  padding: 13px 14px 12px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
  background: var(--fm-surface);
  transition:
    border-color 0.15s ease,
    background 0.15s ease;
}

/* 顶部 2px 状态条：角色 + 状态双重编码，不靠颜色也能分辨（左上是文字徽标） */
.node-card::before {
  content: '';
  position: absolute;
  inset: 0 0 auto;
  height: 2px;
  background: var(--fm-border-strong);
}

.node-card.master::before {
  background: var(--fm-brass);
}

.node-card.tone-ok::before,
.node-card.tone-live::before {
  background: var(--fm-success);
}

.node-card.tone-warn::before {
  background: var(--fm-warning);
}

.node-card.tone-bad::before {
  background: var(--fm-danger);
}

.node-card:hover {
  border-color: var(--fm-border-strong);
}

.nc-head {
  display: flex;
  align-items: flex-start;
  gap: 9px;
}

.nc-avatar {
  flex: none;
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
}

.node-card.master .nc-avatar {
  color: var(--fm-brass-text);
  border-color: color-mix(in srgb, var(--fm-brass) 40%, transparent);
  background: color-mix(in srgb, var(--fm-brass) 8%, transparent);
}

.nc-title {
  flex: 1;
  min-width: 0;
}

.nc-title .node-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nc-title .node-addr {
  margin-top: 1px;
  font-size: 11px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nc-pills {
  flex: none;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}

/* 1px 间隙 + 底色露出：三格配置用发丝线分隔，比加边框干净 */
.nc-specs {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 1px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-border);
  overflow: hidden;
}

.nc-spec {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 9px;
  background: var(--fm-surface-alt);
}

.nc-spec .k {
  font-size: 10px;
  letter-spacing: 0.04em;
  color: var(--fm-muted);
}

.nc-spec .v {
  font-family: var(--fm-font-mono);
  font-size: 12px;
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nc-meters {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.nc-meter-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  font-size: 11px;
  color: var(--fm-muted);
}

.nc-meter-top .mono {
  color: var(--fm-text-2);
}

.nc-track {
  height: 3px;
  margin-top: 4px;
  border-radius: 999px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.nc-fill {
  display: block;
  height: 100%;
  border-radius: 999px;
  background: var(--fm-primary);
  transition: width 0.3s ease;
}

.nc-fill.warn {
  background: var(--fm-warning);
}

.nc-fill.bad {
  background: var(--fm-danger);
}

/* 峰值行：收在实时指标下方当脚注读；负 margin 抵消卡片 11px 的块间距 */
.nc-peak {
  margin-top: -4px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--fm-text-3);
  font-variant-numeric: tabular-nums;
}

.nc-metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
}

.nc-metric {
  min-width: 0;
}

.nc-metric .k {
  display: block;
  margin-bottom: 2px;
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nc-metric .v {
  display: block;
  font-size: 12px;
  color: var(--fm-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nc-foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.nc-chip {
  display: inline-flex;
  align-items: center;
  height: 20px;
  padding: 0 7px;
  border: 1px solid var(--fm-border);
  border-radius: 999px;
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-text-2);
  white-space: nowrap;
}

.nc-chip.over {
  color: var(--fm-warning);
  border-color: color-mix(in srgb, var(--fm-warning) 40%, transparent);
  background: color-mix(in srgb, var(--fm-warning) 8%, transparent);
}

.nc-edit {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding-top: 9px;
  border-top: 1px dashed var(--fm-border);
}

.nc-edit-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.node-card .clock-drift {
  margin-top: 0;
}

.pool-slice {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-muted);
}

.pool-slice.over {
  color: var(--fm-warning);
  font-weight: 600;
}

.node-dropped {
  color: var(--fm-warning);
}

.clock-drift {
  font-size: 11px;
  color: var(--fm-muted);
  margin-top: 2px;
}

.clock-drift.bad {
  color: var(--fm-warning);
}

.weight-input {
  width: 78px;
}

.limit-inputs {
  display: flex;
  align-items: center;
  gap: 4px;
}

.limit-input {
  width: 58px;
}

.limit-sep {
  color: var(--fm-muted);
}

.panel-actions {
  display: flex;
  align-items: center;
  gap: 8px;
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

/* ---------- 变量体检 ---------- */
.audit-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.audit-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
  padding: 9px 12px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-sunken);
  font-size: 12px;
  color: var(--fm-text-2);
}

.audit-summary b {
  color: var(--fm-text);
  font-weight: 600;
}

.audit-table-wrap {
  max-height: 52vh;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.audit-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.audit-table th,
.audit-table td {
  padding: 8px 10px;
  border-bottom: 1px solid var(--fm-border);
  text-align: left;
  white-space: nowrap;
}

.audit-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--fm-surface-sunken);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.audit-table tbody tr:last-child td {
  border-bottom: none;
}

.audit-name {
  font-weight: 600;
  color: var(--fm-text);
}

.audit-message {
  max-width: 260px;
  white-space: normal;
  line-height: 1.55;
  color: var(--fm-text-2);
}

.audit-chip {
  display: inline-flex;
  align-items: center;
  height: 20px;
  padding: 0 7px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
}

.audit-chip.ok {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.audit-chip.bad,
.audit-chip.missing {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.audit-chip.pending {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.audit-chip.unknown {
  background: var(--fm-surface-sunken);
  color: var(--fm-muted);
}

.audit-empty {
  padding: 28px 12px;
  text-align: center;
  font-size: 12.5px;
  color: var(--fm-muted);
}

.audit-hint {
  margin: 0;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.history-panel {
  margin-top: 16px;
}

.history-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.history-desc {
  margin-bottom: 12px;
}

.history-alert {
  margin-bottom: 12px;
}
</style>
