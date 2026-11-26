<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  ThunderboltOutlined,
  DatabaseOutlined,
  ArrowLeftOutlined,
  StopOutlined,
  PlayCircleOutlined,
  FileTextOutlined,
  InfoCircleOutlined,
  PlusOutlined,
  FolderOpenOutlined,
  SaveOutlined,
  DeleteOutlined,
  DesktopOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import StressReportList from '@/components/StressReportList.vue'
import { useProjectStore } from '@/stores/project'
import { useAiAssistStore } from '@/stores/aiAssist'
import { useErrorDialogStore } from '@/stores/errorDialog'
import { stressPlanPrompt } from '@/utils/aiScenarios'
import { ensurePoolPreview } from '@/utils/poolGate'
import * as api from '@/api'
import type {
  CapturePreset,
  CapacityResult,
  Environment,
  HttpRequestDetail,
  PlanSummary,
  PlanTemplate,
  PoolBinding,
  PoolItem,
  PoolPreview,
  PoolSummary,
  RecordDetailMode,
  ReportSample,
  ReportConfigSnapshot,
  RequestSearchHit,
  StressDone,
  StressJobSummary,
  StressProgress,
  StressReport,
  StressStage,
  VarSourceItem,
} from '@/types/models'

const route = useRoute()
const router = useRouter()
const projectStore = useProjectStore()

const requestId = computed(() => Number(route.query.requestId || 0))
const projectId = computed(() => Number(route.query.projectId || 0))
/** 单实例/多窗口入口参数：计划、任务、环境（1.2.1） */
const routePlanId = computed(() => Number(route.query.planId || 0))
const routeJobId = computed(() => Number(route.query.jobId || 0))
/** 报告页「再次运行」：无计划时按报告快照回填 */
const routeReportId = computed(() => Number(route.query.reportId || 0))
const routeEnvId = computed(() => Number(route.query.envId || 0))
const newWindow = computed(() => route.query.newWindow === '1' || route.query.newWindow === 'true')

/** 本窗口绑定的压测任务；事件与停止都按它过滤（1.1.2） */
const jobId = ref<number | null>(null)

const request = ref<HttpRequestDetail | null>(null)
const environments = ref<Environment[]>([])
const envId = ref<number | null>(null)
const loading = ref(true)

/* ---------- 新建压测弹窗 ---------- */
const configOpen = ref(false)
const targetProjectId = ref<number | null>(null)
/** 多接口压测：选中的接口 id 列表，第一个作为报告主目标 */
const targetRequestIds = ref<number[]>([])
/** 每个接口的流量权重；留空或 0 由后端按平均分配归一 */
const requestWeights = reactive<Record<number, number>>({})
/** 开始压测时按 id 取到的完整接口定义（Headers/Body/Auth 随接口各自保留） */
const selectedDetails = ref<HttpRequestDetail[]>([])
const choices = ref<RequestSearchHit[]>([])
const choicesLoading = ref(false)
const preparing = ref(false)
const activeProjectId = ref<number | null>(null)
const modalEnvironments = ref<Environment[]>([])
const modalEnvId = ref<number | null>(null)

/** 兼容旧逻辑：主目标取选中列表的第一个 */
const targetRequestId = computed(() => targetRequestIds.value[0] ?? null)

/** 当前选中环境的非敏感变量值，用于把 {{base_url}}/xxx 渲染成完整路径 */
const plainVars = ref<Record<string, string>>({})

const projectOptions = computed(() =>
  projectStore.projects.map((item) => ({ value: item.id, label: item.name })),
)

/** 名称里已经带了方法前缀（如「GET 查询参数」）就不再拼一遍 */
function displayName(method: string, name: string): string {
  return name.toUpperCase().startsWith(`${method} `.toUpperCase())
    ? name
    : `${method} ${name}`
}

/** 用当前选中环境的非敏感变量渲染 URL，得到完整路径；缺变量时保留模板原文 */
function renderPreviewUrl(text: string): string {
  if (!text || !text.includes('{{')) return text
  return text.replace(/\{\{\s*([\w.-]+)\s*\}\}/g, (raw, key: string) =>
    plainVars.value[key] ?? raw,
  )
}

/** 预览里仍未解析的变量名：环境未确定或有缺失时不允许开始（4.6） */
const unresolvedVars = computed(() => {
  const sources = selectedChoices.value.length
    ? selectedChoices.value.map((item) => item.url)
    : [request.value?.url ?? '']
  const names = new Set<string>()
  for (const source of sources) {
    for (const match of renderPreviewUrl(source).matchAll(/\{\{\s*([\w.-]+)\s*\}\}/g)) {
      names.add(match[1])
    }
  }
  return [...names]
})

/** 解析后的完整地址（4.6）：环境切换后立即更新，缺变量时保留模板原文 */
const previewUrl = computed(() => {
  const source = selectedChoice.value?.url ?? request.value?.url ?? ''
  return renderPreviewUrl(source)
})

/** 最近一次本机能力检测结果（1.7.4）：压测页默认展示，不阻塞启动 */
const capacity = ref<CapacityResult | null>(null)
async function loadCapacity() {
  try {
    capacity.value = await api.capacityGet()
  } catch {
    capacity.value = null
  }
}

const requestSelectOptions = computed(() =>
  choices.value.map((item) => ({
    value: item.id,
    /** 选中后选择框里显示「方法 + 名称」 */
    label: displayName(item.method, item.name),
    method: item.method,
    name: item.name,
    /** 下拉与目标卡片展示渲染后的完整路径 */
    url: renderPreviewUrl(item.url),
  })),
)

interface SelectedChoice extends RequestSearchHit {
  /** 渲染后的完整路径 */
  url: string
}

const selectedChoices = computed<SelectedChoice[]>(() =>
  targetRequestIds.value
    .map((id) => {
      const raw = choices.value.find((item) => item.id === id)
      return raw ? { ...raw, url: renderPreviewUrl(raw.url) } : null
    })
    .filter((item): item is SelectedChoice => item !== null),
)

const selectedChoice = computed(() => selectedChoices.value[0] ?? null)

/** 权重输入：0 / 空 = 平均分配（后端统一归一） */
function requestWeight(id: number): number {
  return requestWeights[id] || 0
}

function setRequestWeight(id: number, value: number | string | null) {
  const next = Number(value)
  requestWeights[id] = Number.isFinite(next) && next > 0 ? next : 0
}

/** 清空多接口选择相关的临时状态 */
function clearRequestWeights() {
  for (const key of Object.keys(requestWeights)) delete requestWeights[Number(key)]
  selectedDetails.value = []
}

/** 多接口时提示权重口径 */
const weightHint = computed(() => {
  if (selectedChoices.value.length < 2) return ''
  const total = targetRequestIds.value.reduce((sum, id) => sum + (requestWeights[id] || 1), 0)
  return `合计权重 ${total}，按占比把请求分发给各接口；留空表示平均分配`
})

const currentProject = computed(
  () =>
    projectStore.projects.find(
      (item) => item.id === (targetProjectId.value ?? activeProjectId.value),
    ) ?? null,
)

const aiAssist = useAiAssistStore()
const errorDialog = useErrorDialogStore()

/** 压测出错时的上下文，方便看出是哪套配置 */
function stressContext(stage: string) {
  const env = environments.value.find((item) => item.id === envId.value)
  return [
    { label: '阶段', value: stage },
    { label: '接口', value: request.value?.name ?? '未选择接口' },
    {
      label: '请求',
      value: request.value ? `${request.value.method} ${request.value.url}` : '—',
    },
    { label: '环境', value: env?.name ?? '未选择' },
    {
      label: '配置',
      value:
        settings.mode === 'fixed'
          ? `${settings.concurrent} 并发 × ${settings.durationSec}s`
          : `${stages.value.length} 段阶梯 ${totalSeconds.value}s`,
    },
  ]
}

function aiPlan() {
  if (!request.value) {
    message.warning('先选择一个接口再做计划')
    return
  }
  void aiAssist.show({
    title: '压测计划建议',
    prompt: stressPlanPrompt(request.value, settings, stages.value),
    source: 'request',
    hint: `${request.value.method} ${request.value.url} · 当前 ${settings.concurrent} 并发 × ${settings.durationSec}s`,
  })
}

/** 压测页头部 Tab：压测 / 计划 / 报告（2.1.3） */
function switchStressTab(key: string | number) {
  if (key === 'plans') router.push('/plans')
  else if (key === 'reports') router.push('/reports')
}

/** 把当前任务放到独立窗口查看；未绑定时按当前计划开新窗口（1.2.1） */
async function openJobWindow() {
  try {
    await api.stressOpenWindow(activePlanId.value, jobId.value, envId.value)
  } catch (e) {
    message.error(String(e))
  }
}

async function openConfig() {
  if (running.value) {
    message.warning('压测进行中，请先停止当前压测')
    return
  }
  if (!projectStore.projects.length) {
    try {
      await projectStore.loadProjects()
    } catch (e) {
      message.error(String(e))
      return
    }
  }
  targetProjectId.value = projectStore.projects[0]?.id ?? null
  targetRequestIds.value = []
  clearRequestWeights()
  configOpen.value = true
  // 打开抽屉即刷新变量来源，避免沿用上次打开时的旧状态
  void loadVarSources()
}

watch(targetProjectId, async (id) => {
  targetRequestIds.value = []
  clearRequestWeights()
  choices.value = []
  modalEnvironments.value = []
  modalEnvId.value = null
  plans.value = []
  activePlanId.value = null
  if (!id) return
  choicesLoading.value = true
  try {
    const [list, envs, planList] = await Promise.all([
      api.requestOptions(id),
      api.envList(id),
      api.planList(id),
    ])
    choices.value = list
    modalEnvironments.value = envs
    plans.value = planList
    const active = envs.find((env) => env.isActive) ?? envs[0]
    modalEnvId.value = active?.id ?? null
  } catch (e) {
    message.error(String(e))
  } finally {
    choicesLoading.value = false
  }
})

// 环境切换后拉取非敏感变量，供接口 URL 预览渲染（失败不影响选择流程）
watch(modalEnvId, async (envId) => {
  plainVars.value = {}
  if (!envId) return
  try {
    plainVars.value = await api.envPlainVars(envId)
  } catch {
    /* 预览拿不到变量就保留模板原文，不值得弹错 */
  }
})

/* ---------- 压测计划 ---------- */
const plans = ref<PlanSummary[]>([])
const activePlanId = ref<number | null>(null)
const planName = ref('')
const planSaving = ref(false)

function currentPlanConfig() {
  return {
    settings: { ...settings },
    stages: stages.value.map((item) => ({ ...item })),
    envId: modalEnvId.value,
    // 多接口：只存请求 id 与权重，运行时再从请求库读取最新定义
    requests: targetRequestIds.value.map((id) => ({
      requestId: id,
      weight: requestWeights[id] || 0,
    })),
  }
}

async function applyPlan(id: number | null) {
  if (!id) {
    activePlanId.value = null
    planName.value = ''
    return
  }
  try {
    const plan = await api.planGet(id)
    const config = plan.config as {
      settings?: Record<string, unknown>
      stages?: StressStage[]
      envId?: number | null
      requests?: { requestId?: number | null; weight?: number | null }[]
    } | null
    if (config?.settings) Object.assign(settings, config.settings)
    if (Array.isArray(config?.stages) && config.stages.length) {
      stages.value = config.stages.map((item) => ({
        concurrent: Number(item.concurrent) || 1,
        durationSec: Number(item.durationSec) || 1,
      }))
    }
    if (config?.envId !== undefined) modalEnvId.value = config.envId ?? null
    if (Array.isArray(config?.requests) && config.requests.length) {
      clearRequestWeights()
      targetRequestIds.value = config.requests
        .map((item) => Number(item.requestId) || 0)
        .filter((value) => value > 0)
      for (const item of config.requests) {
        const requestId = Number(item.requestId) || 0
        const weight = Number(item.weight) || 0
        if (requestId > 0 && weight > 0) requestWeights[requestId] = weight
      }
    } else if (plan.requestId) {
      targetRequestIds.value = [plan.requestId]
    }
    activePlanId.value = plan.id
    planName.value = plan.name
    message.success(`已载入计划「${plan.name}」`)
  } catch (e) {
    message.error(String(e))
  }
}

async function savePlan() {
  if (!targetProjectId.value) return
  const name = planName.value.trim() || `${selectedChoice.value?.name ?? '压测'} 计划`
  planSaving.value = true
  try {
    const saved = await api.planSave({
      id: activePlanId.value,
      projectId: targetProjectId.value,
      requestId: targetRequestId.value,
      name,
      config: currentPlanConfig(),
    })
    activePlanId.value = saved.id
    planName.value = saved.name
    plans.value = await api.planList(targetProjectId.value)
    message.success('计划已保存，可在下次压测时直接载入')
  } catch (e) {
    message.error(String(e))
  } finally {
    planSaving.value = false
  }
}

function removePlan() {
  if (!activePlanId.value) return
  const id = activePlanId.value
  Modal.confirm({
    title: '删除该压测计划？',
    content: '只删除保存的压测配置，已生成的压测报告不受影响。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.planDelete(id)
      plans.value = plans.value.filter((item) => item.id !== id)
      activePlanId.value = null
      planName.value = ''
      message.success('计划已删除')
    },
  })
}

async function startFromConfig() {
  if (!targetProjectId.value) {
    message.warning('请先选择项目')
    return
  }
  if (!targetRequestIds.value.length) {
    message.warning('请选择要压测的接口')
    return
  }
  preparing.value = true
  try {
    // 多接口：逐个取完整定义，各自保留 method / headers / body，池在计划级共享
    const details = await Promise.all(
      targetRequestIds.value.map((id) => api.requestGet(id)),
    )
    const empty = details.find((item) => !item.url.trim())
    if (empty) {
      message.warning(`接口「${empty.name}」还没有填写 URL，请先到请求编辑器补全`)
      return
    }
    selectedDetails.value = details
    request.value = details[0]
    activeProjectId.value = targetProjectId.value
    environments.value = modalEnvironments.value
    envId.value = modalEnvId.value
    configOpen.value = false
    done.value = null
    progress.value = null
    await start()
  } catch (e) {
    message.error(String(e))
  } finally {
    preparing.value = false
  }
}


/** 并发上限，与后端 stress.rs 的 MAX_CONCURRENT 保持一致（绝对安全上限） */
const MAX_CONCURRENT = 100000
/** 超过该并发给出资源提示 */
const HIGH_CONCURRENT_HINT = 1000

const settings = reactive({
  mode: 'fixed' as 'fixed' | 'stages',
  recordDetail: 'auto' as RecordDetailMode,
  /** 采集预设：容量 / 定位 / 自动（1.6） */
  captureMode: 'auto' as CapturePreset,
  /** 定位模式保留的慢请求条数 */
  topN: 200,
  /** 只记录慢于该阈值的请求（毫秒），0 = 不设阈值 */
  slowThresholdMs: 0,
  concurrent: 20,
  durationSec: 30,
  rampUpSec: 5,
  thinkTimeMs: 0,
  /** 思考时间分布（8.7）：fixed / uniform / normal / lognormal */
  thinkTimeMode: 'fixed',
  thinkTimeMinMs: 100,
  thinkTimeMaxMs: 500,
  thinkTimeMeanMs: 300,
  thinkTimeStdMs: 100,
  /** 预热曲线：linear / step / spike */
  rampUpMode: 'linear',
  /** 每个 VU 起始相位随机抖动上限（毫秒） */
  vuStartJitterMs: 0,
  timeoutMs: 30000,
  connectTimeoutMs: 10000,
  followRedirects: true,
  stopOnErrorRate: false,
  stopErrorRate: 5,
  stopMinRequests: 100,
  insecure: false,
  targetRps: 0,
  slaEnabled: false,
  slaP95: 500,
  slaP99: 1000,
  slaErrorRate: 1,
  slaMinRps: 0,
  /** 基线对比：选择历史低并发报告（3.4.4） */
  baselineReportId: null as number | null,
  /** 超过 2 小时必须确认（1.4.1） */
  longRunAck: false,
  /** 参数池 / Token 池绑定（v1.1.2；v20 支持 column） */
  poolBindings: [] as PoolBinding[],
})

/** 本次运行忽略同目标检查（1.3.2），仅本次有效 */
const ignoreOverlap = ref(false)

/* ---------- 参数池 / Token 池（v1.1.2） ---------- */
const pools = ref<PoolSummary[]>([])
const poolPreview = ref<PoolPreview | null>(null)
const poolPreviewLoading = ref(false)

async function loadPools() {
  try {
    pools.value = await api.poolList()
  } catch {
    pools.value = []
  }
}

/** 当前池的自定义列；旧池 / 旧接口没有 columns 时返回空数组 */
function poolColumns(name: string): string[] {
  return pools.value.find((item) => item.name === name)?.columns ?? []
}

/** 绑定行的列选项：全部列自动注入 + 当前池的每个列名 */
function poolColumnOptions(name: string) {
  return [
    { label: '全部列（同名注入）', value: '*' },
    ...poolColumns(name).map((column) => ({ label: column, value: column })),
  ]
}

function addPoolBinding() {
  settings.poolBindings.push({
    pool: pools.value[0]?.name ?? '',
    target: 'token',
    mode: 'per_vu',
    refreshOn401: true,
    column: '',
  })
}

/* ---------- 本次变量：开跑前自查普通 / 密钥 / 系统变量 ---------- */
const VAR_KIND_TEXT: Record<string, string> = {
  user: '普通',
  secret: '密钥',
  os: '系统',
}

const varSources = ref<VarSourceItem[]>([])
const varOpen = ref(false)
const varLoading = ref(false)
const varProblemCount = computed(() => varSources.value.filter((item) => !item.ok).length)

/** 按用户实际写法展示占位符：系统变量与密钥带前缀，一眼和普通变量区分开 */
function varPlaceholder(item: VarSourceItem) {
  if (item.kind === 'os') return `{{os:${item.name}}}`
  if (item.kind === 'secret') return `{{secret:${item.name}}}`
  return `{{${item.name}}}`
}

let varTimer: ReturnType<typeof setTimeout> | undefined

/** 只读检查：不写库、不返回变量值，仅告诉用户每个占位符是什么、从哪来、能不能用 */
async function loadVarSources() {
  if (!request.value?.url) {
    varSources.value = []
    return
  }
  varLoading.value = true
  try {
    // 与启动时下发的字段保持一致，避免「清单说没问题、跑起来报缺失」
    varSources.value = await api.stressVarSources({
      url: request.value.url,
      method: request.value.method,
      headers: request.value.headers,
      params: request.value.params,
      bodyType: request.value.bodyType,
      body: request.value.body,
      envId: modalEnvId.value ?? undefined,
      poolBindings: settings.poolBindings.map((item) => ({ ...item })),
    })
    // 有问题时自动展开，正常时保持收起，不打扰
    if (varProblemCount.value > 0) varOpen.value = true
  } catch {
    varSources.value = []
  } finally {
    varLoading.value = false
  }
}

/** 池绑定改动后延迟重算，避免目标变量逐字符输入时连续请求 */
function scheduleVarSources() {
  if (varTimer) clearTimeout(varTimer)
  varTimer = setTimeout(() => void loadVarSources(), 400)
}

watch(request, () => void loadVarSources())
watch(modalEnvId, () => scheduleVarSources())
watch(
  () =>
    settings.poolBindings
      .map((item) => `${item.pool}|${item.column}|${item.target}|${item.mode}`)
      .join('~'),
  () => scheduleVarSources(),
)

/** 切换池：列选择回到旧行为（'' 取首列），并清空旧预览 */
function onPoolChange(binding: PoolBinding) {
  binding.column = ''
  poolPreview.value = null
}

/** 切换列：指定具体列时把占位符默认填成列名；'*' 不需要占位符 */
function setBindingColumn(binding: PoolBinding, value: unknown) {
  const next = typeof value === 'string' ? value : ''
  const previous = binding.column ?? ''
  binding.column = next
  if (next && next !== '*') {
    const target = binding.target.trim()
    if (!target || target === previous) binding.target = next
  }
  poolPreview.value = null
}

/** 页头快捷入口：定位到参数池绑定区，未选接口时先弹出接口选择 */
function jumpToPoolBinding() {
  if (!request.value) {
    message.info('先选择一个要压测的接口，再绑定数据池')
    openConfig()
    return
  }
  if (!settings.poolBindings.length) addPoolBinding()
  nextTick(() => {
    document.getElementById('pool-binding')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  })
}

/** 目标占位符候选：当前请求里已写的 {{变量}} + 常用名字 */
const POOL_TARGET_PRESETS = ['token', 'phone', 'uid', 'account', 'auth.token', 'auth.username', 'auth.password']
const poolTargetOptions = computed(() => {
  const found = new Set<string>(POOL_TARGET_PRESETS)
  const collect = (text?: string | null) => {
    if (!text) return
    for (const match of text.matchAll(/\{\{\s*([^{}]+?)\s*\}\}/g)) {
      const name = match[1].trim()
      if (name) found.add(name)
    }
  }
  const detail = request.value
  if (detail) {
    collect(detail.url)
    collect(detail.body)
    for (const item of detail.headers ?? []) {
      collect(item.key)
      collect(item.value)
    }
    for (const item of detail.params ?? []) {
      collect(item.key)
      collect(item.value)
    }
    if (detail.auth) {
      collect(detail.auth.token)
      collect(detail.auth.username)
      collect(detail.auth.password)
      collect(detail.auth.key)
      collect(detail.auth.value)
    }
  }
  for (const binding of settings.poolBindings) {
    if (binding.target.trim()) found.add(binding.target.trim())
  }
  return [...found].map((value) => ({ value }))
})

function removePoolBinding(index: number) {
  settings.poolBindings.splice(index, 1)
  poolPreview.value = null
}

async function previewPools() {
  if (!settings.poolBindings.length) {
    message.info('请先添加池绑定')
    return
  }
  const missing = settings.poolBindings.find(
    (item) => !item.pool || (item.column !== '*' && !item.target.trim()),
  )
  if (missing) {
    message.warning('每个绑定都需要选择池；非「全部列」模式还需填写目标变量')
    return
  }
  poolPreviewLoading.value = true
  try {
    poolPreview.value = await api.poolPreview(
      settings.poolBindings,
      settings.mode === 'fixed'
        ? settings.concurrent
        : Math.max(...stages.value.map((item) => item.concurrent), 0),
      false,
    )
  } catch (error) {
    message.error(String(error))
  } finally {
    poolPreviewLoading.value = false
  }
}

/** 预览样例：自动注入时拼接整行数据，指定列时只显示该列 */
function previewSampleText(binding: PoolPreview['bindings'][number]): string {
  const texts = binding.sample.slice(0, 3).map((item) => {
    if (binding.column === '*') {
      const data = item.data ?? {}
      return Object.values(data).join('/')
    }
    if (binding.column) return item.data?.[binding.column] ?? item.value
    return item.value
  })
  return texts.filter(Boolean).join('、') || '无数据'
}

/* ---------- 池管理弹窗 ---------- */
const poolManagerOpen = ref(false)
const poolManagerBusy = ref(false)
const newPoolName = ref('')
const newPoolKind = ref<'parameter' | 'token'>('token')
const newPoolSensitive = ref(true)
const activePoolId = ref<number | null>(null)
const activePoolItems = ref<PoolItem[]>([])
const poolImportText = ref('')

async function openPoolManager() {
  poolManagerOpen.value = true
  await loadPools()
  // 池列表加载完成后才能确定默认选中的池，避免打开弹窗时列表为空
  const target = activePoolId.value ?? pools.value[0]?.id ?? null
  if (target) await openPoolItems(target)
  else {
    activePoolId.value = null
    activePoolItems.value = []
  }
}

async function createPool() {
  const name = newPoolName.value.trim()
  if (!name) {
    message.warning('请输入池名称')
    return
  }
  poolManagerBusy.value = true
  try {
    const created = await api.poolCreate({
      name,
      kind: newPoolKind.value,
      sensitive: newPoolSensitive.value,
      description: '',
    })
    newPoolName.value = ''
    await loadPools()
    await openPoolItems(created.id)
    message.success('池已创建')
  } catch (error) {
    message.error(String(error))
  } finally {
    poolManagerBusy.value = false
  }
}

async function openPoolItems(poolId: number) {
  activePoolId.value = poolId
  try {
    activePoolItems.value = await api.poolItems(poolId, false)
  } catch (error) {
    message.error(String(error))
    activePoolItems.value = []
  }
}

async function importPoolItems() {
  if (!activePoolId.value) return
  if (!poolImportText.value.trim()) {
    message.warning('请输入要导入的数据')
    return
  }
  poolManagerBusy.value = true
  try {
    const count = await api.poolImport(activePoolId.value, poolImportText.value)
    poolImportText.value = ''
    await openPoolItems(activePoolId.value)
    await loadPools()
    message.success(`已导入 ${count} 条`)
  } catch (error) {
    message.error(String(error))
  } finally {
    poolManagerBusy.value = false
  }
}

async function removePoolItem(itemId: number) {
  if (!activePoolId.value) return
  try {
    await api.poolDeleteItem(itemId)
    await openPoolItems(activePoolId.value)
    await loadPools()
  } catch (error) {
    message.error(String(error))
  }
}

async function deletePool(poolId: number) {
  try {
    await api.poolDelete(poolId)
    if (activePoolId.value === poolId) {
      activePoolId.value = null
      activePoolItems.value = []
    }
    await loadPools()
    message.success('池已删除')
  } catch (error) {
    message.error(String(error))
  }
}



/** 长稳压测阈值：超过必须勾选风险确认 */
const LONG_RUN_SECONDS = 7200
const totalSecondsForAck = computed(() => totalSeconds.value)
const longRunNeeded = computed(() => totalSecondsForAck.value > LONG_RUN_SECONDS)
/** 固定并发时长上限：绑定计划后放到 24h，快速压测维持 2h（1.4） */
const fixedDurationMax = computed(() => (activePlanId.value ? 86400 : 7200))

/* ---------- 计划模板 ---------- */
const planTemplates = ref<PlanTemplate[]>([])

/** 基线对比候选：历史报告（3.4.4），优先展示当前接口/计划的历史 */
const baselineReports = ref<StressReport[]>([])

async function loadBaselineReports() {
  try {
    baselineReports.value = await api.reportList(200)
  } catch {
    baselineReports.value = []
  }
}

const baselineOptions = computed(() => {
  const list = baselineReports.value.filter((item) => {
    const sameRequest = targetRequestId.value ? item.requestId === targetRequestId.value : true
    const samePlan = activePlanId.value ? item.planId === activePlanId.value : true
    return sameRequest || samePlan
  })
  const source = list.length ? list : baselineReports.value
  return source.slice(0, 50).map((item) => ({
    value: item.id,
    label: `#${item.id} ${item.planName} · ${item.concurrency} 并发 · P95 ${fmt(item.p95Rt)}ms · QPS ${fmt(item.avgQps)}`,
  }))
})

function setBaseline(id: number | null) {
  settings.baselineReportId = id ?? null
}

async function loadTemplates() {
  try {
    planTemplates.value = await api.planTemplates()
  } catch {
    planTemplates.value = []
  }
}

/** 应用模板到当前配置（2.5） */
function applyTemplate(template: PlanTemplate) {
  settings.mode = template.mode
  if (template.mode === 'stages' && template.stages?.length) {
    stages.value = template.stages.map((item) => ({ ...item }))
  }
  if (template.mode === 'fixed') {
    if (template.concurrent) settings.concurrent = template.concurrent
    if (template.durationSec) settings.durationSec = template.durationSec
  }
  if (template.captureMode) settings.captureMode = template.captureMode
  if (longRunNeeded.value) settings.longRunAck = false
  if (longRunNeeded.value && !activePlanId.value) {
    message.info('该模板超过 2 小时，请先保存为计划再运行')
  }
  message.success(`已应用模板「${template.name}」`)
}

function applyTemplateById(id: string) {
  const template = planTemplates.value.find((item) => item.id === id)
  if (template) applyTemplate(template)
}

const stages = ref<StressStage[]>([
  { concurrent: 10, durationSec: 30 },
  { concurrent: 30, durationSec: 30 },
  { concurrent: 60, durationSec: 30 },
])

function addStage() {
  const last = stages.value[stages.value.length - 1]
  stages.value.push({
    concurrent: Math.min(MAX_CONCURRENT, (last?.concurrent ?? 0) + 20),
    durationSec: last?.durationSec ?? 30,
  })
}

function removeStage(index: number) {
  if (stages.value.length <= 1) return
  stages.value.splice(index, 1)
}

const running = ref(false)
const stopping = ref(false)
const progress = ref<StressProgress | null>(null)
const done = ref<StressDone | null>(null)
const resultCard = ref<HTMLElement | null>(null)
const unlisteners: UnlistenFn[] = []
const reportList = ref<{ reload: () => Promise<void> } | null>(null)

onMounted(async () => {
  try {
    unlisteners.push(
      await listen<StressProgress>('stress:progress', (event) => {
        // 多窗口并行：只接收本窗口绑定任务的进度（1.1.2）
        if (jobId.value && event.payload.jobId !== jobId.value) return
        progress.value = event.payload
        // 后端只下发增量采样点，这里累积成完整序列供图表使用
        if (event.payload.samples?.length) {
          liveSamples.value.push(...event.payload.samples)
        }
        if (event.payload.notice) message.info(event.payload.notice)
      }),
    )
    unlisteners.push(
      await listen<StressDone>('stress:done', (event) => {
        if (jobId.value && event.payload.jobId !== jobId.value) return
        done.value = event.payload
        running.value = false
        stopping.value = false
        if (event.payload.reportId) {
          message.success('压测已结束，报告已生成')
        } else if (event.payload.message) {
          errorDialog.show({
            title: '压测异常结束',
            error: event.payload.message,
            context: stressContext('压测执行'),
          })
        }
        void reportList.value?.reload()
        nextTick(() => resultCard.value?.scrollIntoView({ behavior: 'smooth', block: 'center' }))
      }),
    )
  } catch (e) {
    console.error('压测事件监听失败', e)
  }

  try {
    await initFromQuery()
    void loadTemplates()
    void loadBaselineReports()
    void loadCapacity()
    void loadPools()
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

/** 解析入口参数：计划 / 任务 / 环境（1.2.1、4.2） */
async function initFromQuery() {
  // 恢复任务视图：任务归注册表，窗口只是视图，不重跑（1.2.5）
  if (routeJobId.value) {
    jobId.value = routeJobId.value
    await restoreJob(routeJobId.value)
    return
  }
  if (routePlanId.value) {
    try {
      await openPlan(routePlanId.value)
      return
    } catch (e) {
      // 计划可能已被删除：有报告快照时回退，避免「再次运行」直接失败
      if (!routeReportId.value) throw e
      message.warning('关联计划已不存在，已按报告配置快照回填')
    }
  }
  if (routeReportId.value) {
    await openReport(routeReportId.value)
    return
  }
  if (!requestId.value) return
  request.value = await api.requestGet(requestId.value)
  // 请求详情不一定带 projectId，路由参数优先
  const pid = projectId.value || (request.value as { projectId?: number } | null)?.projectId || 0
  if (pid) {
    activeProjectId.value = pid
    environments.value = await api.envList(pid)
    envId.value = pickEnvId(environments.value, routeEnvId.value)
    await loadPlainVars(envId.value)
  }
}

/** 拉取当前环境的非敏感变量，供 URL 预览与未解析检测使用 */
async function loadPlainVars(envIdValue: number | null) {
  plainVars.value = {}
  if (!envIdValue) return
  try {
    plainVars.value = await api.envPlainVars(envIdValue)
  } catch {
    /* 预览拿不到变量就保留模板原文 */
  }
}

/** 环境优先级：路由 envId → 显式计划 env → 项目激活环境（4.2） */
function pickEnvId(list: Environment[], preferred?: number | null): number | null {
  if (preferred && list.some((env) => env.id === preferred)) return preferred
  const active = list.find((env) => env.isActive) ?? list[0]
  return active?.id ?? null
}

/** 重新打开同一任务：恢复任务信息并继续接收进度 */
async function restoreJob(id: number) {
  const jobs = await api.stressJobs()
  const job = jobs.find((item) => item.id === id)
  if (!job) {
    message.warning('该压测任务已结束，可在报告页查看结果')
    return
  }
  running.value = true
  progress.value = {
    jobId: id,
    name: job.name,
    running: true,
    elapsedSec: job.elapsedSec,
    total: 0,
    success: 0,
    errors: 0,
    rps: 0,
    avgRt: 0,
    p95Rt: 0,
    p99Rt: 0,
    concurrent: job.concurrent,
    stageIndex: 0,
    activeConcurrent: job.concurrent,
    statusCodes: [],
    samples: [],
    recordMode: 'auto',
  }
  if (job.requestId) {
    request.value = await api.requestGet(job.requestId)
  }
  if (job.planId) {
    activePlanId.value = job.planId
    planName.value = job.name
  }
}

/** 从计划载入压测页（2.3.1） */
async function openPlan(planIdValue: number) {
  const plan = await api.planGet(planIdValue)
  targetProjectId.value = plan.projectId
  activeProjectId.value = plan.projectId
  environments.value = await api.envList(plan.projectId)
  modalEnvironments.value = environments.value
  const config = plan.config as {
    settings?: Record<string, unknown>
    stages?: StressStage[]
    envId?: number | null
    captureMode?: CapturePreset
    topN?: number
    slowThresholdMs?: number
    requests?: { requestId?: number | null; weight?: number | null }[]
  } | null
  if (config?.settings) Object.assign(settings, config.settings)
  if (config?.captureMode) settings.captureMode = config.captureMode
  if (config?.topN != null) settings.topN = config.topN
  if (config?.slowThresholdMs != null) settings.slowThresholdMs = config.slowThresholdMs
  if (Array.isArray(config?.stages) && config.stages.length) {
    settings.mode = 'stages'
    stages.value = config.stages.map((item) => ({
      concurrent: Number(item.concurrent) || 1,
      durationSec: Number(item.durationSec) || 1,
    }))
  }
  modalEnvId.value = pickEnvId(environments.value, routeEnvId.value || config?.envId || null)
  envId.value = modalEnvId.value
  await loadPlainVars(envId.value)
  activePlanId.value = plan.id
  planName.value = plan.name
  if (Array.isArray(config?.requests) && config.requests.length) {
    clearRequestWeights()
    targetRequestIds.value = config.requests
      .map((item) => Number(item.requestId) || 0)
      .filter((value) => value > 0)
    for (const item of config.requests) {
      const requestId = Number(item.requestId) || 0
      const weight = Number(item.weight) || 0
      if (requestId > 0 && weight > 0) requestWeights[requestId] = weight
    }
    if (targetRequestId.value) {
      // 多接口计划：一次取齐全部接口定义，直接点「开始压测」也能按多接口运行
      selectedDetails.value = await Promise.all(
        targetRequestIds.value.map((id) => api.requestGet(id)),
      )
      request.value = selectedDetails.value[0] ?? null
    }
  } else if (plan.requestId) {
    targetRequestIds.value = [plan.requestId]
    request.value = await api.requestGet(plan.requestId)
  }
}

/** 从报告配置快照回填压测页（报告页「再次运行」） */
async function openReport(reportIdValue: number) {
  const detail = await api.reportGet(reportIdValue)
  const config: ReportConfigSnapshot | null = detail.config ?? null
  activePlanId.value = detail.report.planId ?? null
  planName.value = detail.report.title?.trim() || detail.report.planName

  const pid = config?.projectId ?? 0
  if (pid) {
    targetProjectId.value = pid
    activeProjectId.value = pid
    environments.value = await api.envList(pid)
    modalEnvironments.value = environments.value
    envId.value = pickEnvId(environments.value, config?.envId ?? routeEnvId.value)
    await loadPlainVars(envId.value)
  }

  if (config) {
    if (config.timeoutMs != null) settings.timeoutMs = config.timeoutMs
    if (config.connectTimeoutMs != null) settings.connectTimeoutMs = config.connectTimeoutMs
    if (config.followRedirects != null) settings.followRedirects = config.followRedirects
    if (config.thinkTimeMs != null) settings.thinkTimeMs = config.thinkTimeMs
    if (config.rampUpSec != null) settings.rampUpSec = config.rampUpSec
    if (config.targetRps != null) settings.targetRps = config.targetRps
    if (config.stopErrorRate != null && config.stopErrorRate > 0) {
      settings.stopOnErrorRate = true
      settings.stopErrorRate = config.stopErrorRate
    }
    if (config.stopMinRequests != null) settings.stopMinRequests = config.stopMinRequests
    if (config.recordDetailRequested) {
      settings.recordDetail = config.recordDetailRequested as RecordDetailMode
    }
    if (config.captureMode) settings.captureMode = config.captureMode as CapturePreset
    if (config.topN != null) settings.topN = config.topN
    if (config.slowThresholdMs != null) settings.slowThresholdMs = config.slowThresholdMs
    if (config.baselineReportId != null) settings.baselineReportId = config.baselineReportId
    if (config.sla) {
      settings.slaEnabled = true
      if (config.sla.p95Ms != null) settings.slaP95 = config.sla.p95Ms
      if (config.sla.p99Ms != null) settings.slaP99 = config.sla.p99Ms
      if (config.sla.errorRate != null) settings.slaErrorRate = config.sla.errorRate
      if (config.sla.minRps != null) settings.slaMinRps = config.sla.minRps
    }
    if (config.stages?.length) {
      settings.mode = 'stages'
      stages.value = config.stages.map((item) => ({
        concurrent: Number(item.concurrent) || 1,
        durationSec: Number(item.durationSec) || 1,
      }))
    } else {
      settings.mode = 'fixed'
      settings.concurrent = Math.max(1, detail.report.concurrency || settings.concurrent)
      const span = (detail.report.endTime ?? 0) - (detail.report.startTime ?? 0)
      if (span > 0) settings.durationSec = Math.max(1, Math.round(span / 1000))
    }
  }

  if (detail.report.requestId) {
    // 复用请求定义（请求头 / 体 / 参数），再用报告快照覆盖目标地址与压测参数
    const snapshotRequests = Array.isArray(config?.requests) ? config.requests : []
    if (snapshotRequests.length > 1) {
      // 多接口报告：恢复接口清单与权重，运行时再取各接口最新定义
      clearRequestWeights()
      targetRequestIds.value = snapshotRequests
        .map((item) => Number(item.requestId) || 0)
        .filter((value) => value > 0)
      for (const item of snapshotRequests) {
        const requestId = Number(item.requestId) || 0
        const weight = Number(item.weight) || 0
        if (requestId > 0 && weight > 0) requestWeights[requestId] = weight
      }
    } else {
      targetRequestIds.value = [detail.report.requestId]
    }
    try {
      if (targetRequestIds.value.length > 1) {
        selectedDetails.value = await Promise.all(
          targetRequestIds.value.map((id) => api.requestGet(id)),
        )
        request.value = selectedDetails.value[0] ?? null
      } else {
        request.value = await api.requestGet(detail.report.requestId)
      }
    } catch {
      // 请求已被删除：回退到配置快照，不阻断「再次运行」
      request.value = null
      targetRequestIds.value = []
      selectedDetails.value = []
      message.warning('关联请求已不存在，已按报告配置快照回填')
    }
    if (request.value) {
      if (config?.method) request.value.method = config.method
      if (config?.urlTemplate || config?.url) {
        request.value.url = config.urlTemplate || config.url || request.value.url
      }
      return
    }
  }

  if (config?.url || config?.urlTemplate) {
    // 报告未关联请求：只能按快照回填地址与参数，未落库的请求头/体需要用户补齐
    request.value = {
      id: 0,
      collectionId: 0,
      name: planName.value,
      method: config?.method || 'GET',
      url: config?.urlTemplate || config?.url || '',
      headers: [],
      params: [],
      bodyType: config?.bodyType ?? 'none',
      body: '',
      preScript: '',
      postScript: '',
      assertions: '',
    }
    message.warning(
      '该报告未关联请求或计划：已按配置快照回填地址与压测参数，请求头、请求体与查询参数未存入报告，请补齐后再运行',
    )
    return
  }
  message.warning('该报告未保存可复用的请求配置，请手动选择请求后再运行')
}

onBeforeUnmount(() => {
  for (const off of unlisteners) off()
  // 关闭窗口不中断任务：任务归注册表，窗口只是视图（1.2.5）
})

const liveSamples = ref<ReportSample[]>([])
const samples = computed(() => liveSamples.value)

const qpsSeries = computed(() => [
  {
    name: 'QPS',
    color: 'var(--fm-primary)',
    values: samples.value.map((s) => s.qps),
  },
])

const latencySeries = computed(() => [
  { name: '平均', color: 'var(--fm-primary)', values: samples.value.map((s) => s.avgRt) },
  { name: 'P95', color: 'var(--fm-warning)', values: samples.value.map((s) => s.p95Rt) },
  { name: 'P99', color: 'var(--fm-danger)', values: samples.value.map((s) => s.p99Rt) },
])

const chartLabels = computed(() => samples.value.map((s) => `${s.elapsedSec}s`))

const elapsed = computed(() => progress.value?.elapsedSec ?? done.value?.elapsedSec ?? 0)

const totalSeconds = computed(() => {
  if (settings.mode === 'stages') {
    return stages.value.reduce((sum, item) => sum + Math.max(1, item.durationSec || 0), 0)
  }
  return Math.max(1, settings.durationSec)
})

const peakConcurrent = computed(() => {
  if (settings.mode === 'stages') {
    return stages.value.reduce((peak, item) => Math.max(peak, item.concurrent || 0), 0)
  }
  return settings.concurrent
})

const percent = computed(() =>
  Math.min(100, Math.round((elapsed.value / Math.max(1, totalSeconds.value)) * 100)),
)

const stageLabel = computed(() => {
  if (settings.mode !== 'stages') return ''
  const index = progress.value?.stageIndex ?? 0
  return `阶段 ${index + 1}/${stages.value.length}`
})

const statusCodes = computed(() => progress.value?.statusCodes ?? [])

const RECORD_MODE_TEXT: Record<string, string> = {
  full: '明细：完整',
  sample: '明细：采样 1%',
  off: '明细：已关闭',
}
const recordModeText = computed(() => {
  const mode = progress.value?.recordMode
  if (!mode) return ''
  return RECORD_MODE_TEXT[mode] ?? `明细：${mode}`
})
const statusCodeTotal = computed(() =>
  statusCodes.value.reduce((sum, item) => sum + item.count, 0),
)

function codeClass(code: number) {
  if (code >= 500) return 'bad'
  if (code >= 400) return 'warn'
  if (code >= 300) return 'info'
  return 'ok'
}

const errorRate = computed(() => {
  const total = progress.value?.total ?? done.value?.total ?? 0
  const errors = progress.value?.errors ?? done.value?.errors ?? 0
  return total > 0 ? (errors / total) * 100 : 0
})

const currentRps = computed(() => {
  if (progress.value) return progress.value.rps
  if (done.value && done.value.elapsedSec > 0) return done.value.total / done.value.elapsedSec
  return 0
})

/** 至今平均 QPS = 已完成请求数 ÷ 已运行秒数，避免首秒抖动放大瞬时值 */
const avgQps = computed(() => {
  const seconds = progress.value?.elapsedSec ?? done.value?.elapsedSec ?? 0
  const total = progress.value?.total ?? done.value?.total ?? 0
  return seconds > 0 ? total / seconds : 0
})

/** 本次压测的采样峰值 QPS */
const peakQps = computed(() => samples.value.reduce((max, item) => Math.max(max, item.qps), 0))

function fmt(v: number | undefined | null, digits = 1) {
  if (v === undefined || v === null || Number.isNaN(v)) return '-'
  return v >= 1000 ? v.toFixed(0) : v.toFixed(digits)
}

/** 同主机不同路径只警告不拦截（1.3.2）；后端会再判一次并写进报告快照 */
async function warnSameHost() {
  const raw = request.value?.url?.trim()
  if (!raw) return
  let target: URL
  try {
    target = new URL(raw)
  } catch {
    // URL 含未解析变量时前端无法比较主机，交给后端判定
    return
  }
  try {
    const jobs = await api.stressJobs()
    const list = jobs
      .filter((job) => {
        try {
          const running = new URL(job.target)
          return running.protocol === target.protocol && running.host === target.host
        } catch {
          return false
        }
      })
      .map((job) => `#${job.id} ${job.name}`)
    if (list.length) {
      message.warning(
        `同一主机已有任务 ${list.join('、')} 在施压，同机并发会互相争抢连接与带宽；本次结果不能当作单实例容量结论`,
      )
    }
  } catch {
    /* 拿不到运行中任务时不做提示，后端仍会校验 */
  }
}

async function start() {
  if (!request.value) return
  if (!request.value.url.trim()) {
    message.warning('请先回到请求编辑器填写 URL')
    return
  }
  if (!envId.value) {
    message.warning('请先选择环境：环境未确定时不允许开始压测')
    return
  }
  if (unresolvedVars.value.length) {
    message.warning(
      `当前环境缺少变量：${unresolvedVars.value.join('、')}，请补充变量或切换环境`,
    )
    return
  }
  if (totalSeconds.value > LONG_RUN_SECONDS && !activePlanId.value) {
    message.warning('超过 2 小时的长稳压测必须保存为计划后运行，请先点「保存计划」')
    return
  }
  if (totalSeconds.value > LONG_RUN_SECONDS && !settings.longRunAck) {
    message.warning('本次压测超过 2 小时，请先勾选「已知悉同机施压风险」')
    return
  }

  if (settings.mode === 'stages') {
    if (stages.value.length < 1) {
      message.warning('至少需要一个加压阶段')
      return
    }
    if (stages.value.some((item) => item.concurrent < 1 || item.concurrent > MAX_CONCURRENT)) {
      message.warning(`阶段并发数需在 1 到 ${MAX_CONCURRENT} 之间`)
      return
    }
    if (stages.value.some((item) => item.durationSec < 1)) {
      message.warning('每个阶段至少持续 1 秒')
      return
    }
  } else {
    if (settings.concurrent < 1 || settings.concurrent > MAX_CONCURRENT) {
      message.warning(`并发数需在 1 到 ${MAX_CONCURRENT} 之间`)
      return
    }
    if (settings.rampUpSec >= settings.durationSec) {
      message.warning('预热时长需要小于压测时长')
      return
    }
  }

  // 开跑前强制执行池预览：预览失败或用户取消都不允许启动（无池绑定直接通过）
  const vuCount = settings.mode === 'fixed' ? settings.concurrent : peakConcurrent.value
  if (!(await ensurePoolPreview(settings.poolBindings, vuCount, '开始压测前确认参数 / Token 池'))) {
    return
  }

  await warnSameHost()

  progress.value = null
  done.value = null
  stopping.value = false
  running.value = true
  liveSamples.value = []

  try {
    const id = await api.stressStart({
      requestId: request.value.id,
      projectId: targetProjectId.value ?? activeProjectId.value ?? undefined,
      planId: activePlanId.value ?? undefined,
      name: request.value.name,
      method: request.value.method,
      url: request.value.url,
      headers: request.value.headers,
      params: request.value.params,
      bodyType: request.value.bodyType,
      body: request.value.body,
      // 多接口：各自保留请求定义，按权重共享同一 Token / 参数池，汇总成一份报告
      requests:
        selectedDetails.value.length > 1
          ? selectedDetails.value.map((item) => ({
              requestId: item.id ?? null,
              name: item.name,
              method: item.method,
              url: item.url,
              headers: item.headers,
              params: item.params,
              bodyType: item.bodyType,
              body: item.body,
              auth: item.auth ?? null,
              weight: requestWeights[item.id ?? 0] || 0,
            }))
          : undefined,
      envId: envId.value,
      insecure: settings.insecure,
      timeoutMs: settings.timeoutMs,
      connectTimeoutMs: settings.connectTimeoutMs,
      followRedirects: settings.followRedirects,
      stopErrorRate: settings.stopOnErrorRate ? settings.stopErrorRate : null,
      stopMinRequests: settings.stopOnErrorRate ? settings.stopMinRequests : null,
      concurrent: settings.mode === 'fixed' ? settings.concurrent : peakConcurrent.value,
      durationSec: totalSeconds.value,
      rampUpSec: settings.mode === 'fixed' ? settings.rampUpSec : 0,
      thinkTimeMs: settings.thinkTimeMs,
      thinkTimeMode: settings.thinkTimeMode,
      thinkTimeMinMs: settings.thinkTimeMinMs,
      thinkTimeMaxMs: settings.thinkTimeMaxMs,
      thinkTimeMeanMs: settings.thinkTimeMeanMs,
      thinkTimeStdMs: settings.thinkTimeStdMs,
      rampUpMode: settings.rampUpMode,
      vuStartJitterMs: settings.vuStartJitterMs,
      stages: settings.mode === 'stages' ? stages.value.map((item) => ({ ...item })) : [],
      targetRps: settings.targetRps > 0 ? settings.targetRps : null,
      recordDetail: settings.recordDetail,
      captureMode: settings.captureMode,
      topN: settings.captureMode === 'locate' ? settings.topN : null,
      slowThresholdMs:
        settings.captureMode === 'locate' && settings.slowThresholdMs > 0
          ? settings.slowThresholdMs
          : null,
      ignoreOverlap: ignoreOverlap.value,
      baselineReportId: settings.baselineReportId ?? null,
      longRunAck: settings.longRunAck,
      poolBindings: settings.poolBindings.map((item) => ({ ...item })),
      sla: settings.slaEnabled
        ? {
            p95Ms: settings.slaP95 > 0 ? settings.slaP95 : null,
            p99Ms: settings.slaP99 > 0 ? settings.slaP99 : null,
            errorRate: settings.slaErrorRate > 0 ? settings.slaErrorRate : null,
            minRps: settings.slaMinRps > 0 ? settings.slaMinRps : null,
          }
        : null,
    })
    // 先绑定任务号，避免第一帧进度事件被过滤掉
    jobId.value = id
    if (!routeJobId.value) {
      void router.replace({ query: { ...route.query, jobId: String(id) } })
    }
  } catch (e) {
    running.value = false
    const text = String(e)
    // 同目标冲突：给出「忽略并继续」入口（1.3.2）
    if (text.includes('目标接口正在被任务')) {
      Modal.confirm({
        title: '同目标已有压测任务',
        content: `${text}。同时施压会互相干扰，报告也不能作为单实例容量结论。`,
        okText: '忽略并继续',
        okType: 'danger',
        cancelText: '取消',
        onOk: async () => {
          ignoreOverlap.value = true
          try {
            await start()
          } finally {
            ignoreOverlap.value = false
          }
        },
      })
      return
    }
    errorDialog.show({
      title: '压测启动失败',
      error: e,
      context: stressContext('启动压测'),
    })
  }
}

async function stop() {
  stopping.value = true
  try {
    // 只停本窗口绑定任务；没有绑定时由后端按全部停止兜底
    await api.stressStop(jobId.value)
  } catch (e) {
    message.error(String(e))
  }
}

function backToRequest() {
  if (!request.value?.id) {
    router.push('/')
    return
  }
  router.push({
    name: 'request',
    params: { id: request.value.id },
    query: { projectId: activeProjectId.value || undefined },
  })
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Stress Test"
      title="接口压测"
      subtitle="选择项目与接口、配置压测参数，在本地发起真实并发请求"
    >
      <a-button @click="jumpToPoolBinding">
        <template #icon><DatabaseOutlined /></template>
        数据池绑定
      </a-button>
      <a-button :disabled="!request" @click="aiPlan">
        <template #icon><AiSparkIcon /></template>
        AI 生成计划
      </a-button>
      <a-button v-if="running && jobId" @click="openJobWindow">
        <template #icon><DesktopOutlined /></template>
        新窗口查看
      </a-button>
      <a-button type="primary" :disabled="running" @click="openConfig">
        <template #icon><PlusOutlined /></template>
        新建压测
      </a-button>
      <a-button @click="backToRequest">
        <template #icon><ArrowLeftOutlined /></template>
        返回请求
      </a-button>
    </PageHeader>

    <a-tabs
      :active-key="'stress'"
      size="small"
      class="stress-top-tabs"
      @change="switchStressTab"
    >
      <a-tab-pane key="stress" tab="压测" />
      <a-tab-pane key="plans" tab="计划" />
      <a-tab-pane key="reports" tab="报告" />
    </a-tabs>

    <a-spin :spinning="loading">
      <div v-if="!request && !requestId" class="fm-card guide-empty">
        <div class="empty-icon-wrap"><ThunderboltOutlined /></div>
        <div class="empty-title">选择一个接口开始压测</div>
        <div class="empty-desc">
          点击「新建压测」，先选择项目与接口，再填写并发、时长、限速与 SLA 阈值，
          确认后立即在本地发起真实压测并自动生成报告。
        </div>
        <a-button type="primary" size="large" @click="openConfig">
          <template #icon><PlusOutlined /></template>
          新建压测
        </a-button>
      </div>

      <div v-else-if="request" class="layout">
        <section class="fm-card config">
          <div class="pane-head">
            <span>压测配置</span>
            <span class="pane-tag">单接口</span>
          </div>

          <div class="target">
            <div class="target-top">
              <span class="method-tag" :class="`method-${request.method}`">{{ request.method }}</span>
              <span class="target-name">{{ request.name }}</span>
            </div>
            <div class="target-url mono">{{ request.url }}</div>
            <div class="target-meta">
              Headers {{ request.headers.filter((h) => h.enabled && h.key).length }} 项 ·
              Params {{ request.params.filter((p) => p.enabled && p.key).length }} 项 ·
              Body {{ request.bodyType === 'none' ? '无' : request.bodyType }}
            </div>
          </div>

          <div class="form">
            <div v-if="planTemplates.length" class="field">
              <div class="label">计划模板</div>
              <a-select
                :value="null"
                placeholder="选择模板快速填充并发与采集预设"
                style="width: 100%"
                :disabled="running"
                @change="applyTemplateById"
              >
                <a-select-option
                  v-for="template in planTemplates"
                  :key="template.id"
                  :value="template.id"
                  :title="template.description"
                >
                  {{ template.name }}
                </a-select-option>
              </a-select>
              <div class="hint">低并发基线 / 细化阶梯 / 长稳场景，应用后可继续微调</div>
            </div>

            <div class="field">
              <div class="label">加压模式</div>
              <a-segmented
                v-model:value="settings.mode"
                :options="[
                  { label: '固定并发', value: 'fixed' },
                  { label: '阶梯加压', value: 'stages' },
                ]"
                block
                :disabled="running"
              />
              <div class="hint">
                {{ settings.mode === 'fixed'
                  ? '固定并发持续压测，适合验证稳定吞吐'
                  : '按阶段逐步提升并发，用于寻找性能拐点' }}
              </div>
            </div>

            <div class="field">
              <div class="label">采集预设</div>
              <a-segmented
                v-model:value="settings.captureMode"
                :options="[
                  { label: '自动', value: 'auto' },
                  { label: '容量', value: 'capacity' },
                  { label: '定位', value: 'locate' },
                ]"
                block
                :disabled="running"
              />
              <div class="hint">
                {{
                  settings.captureMode === 'capacity'
                    ? '容量模式按实测 QPS 自动降档，优先保证吞吐与稳定性'
                    : settings.captureMode === 'locate'
                      ? '定位模式保留最慢的 N 条请求，适合排查慢接口（建议低并发）'
                      : '自动模式按下方「明细记录」与 QPS 阈值决定采集档位'
                }}
              </div>
              <template v-if="settings.captureMode === 'locate'">
                <div class="field-row capture-row">
                  <div class="field">
                    <div class="label">慢请求 TopN</div>
                    <a-input-number
                      v-model:value="settings.topN"
                      :min="1"
                      :max="2000"
                      style="width: 100%"
                      :disabled="running"
                    />
                  </div>
                  <div class="field">
                    <div class="label">只记录慢于（ms，0 = 不限）</div>
                    <a-input-number
                      v-model:value="settings.slowThresholdMs"
                      :min="0"
                      :max="120000"
                      :step="100"
                      style="width: 100%"
                      :disabled="running"
                    />
                  </div>
                </div>
              </template>
            </div>

            <div v-if="settings.captureMode === 'auto'" class="field">
              <div class="label">明细记录</div>
              <a-select v-model:value="settings.recordDetail" :disabled="running" style="width: 100%">
                <a-select-option value="auto">自动（按 QPS 降档，推荐）</a-select-option>
                <a-select-option value="full">完整记录（排查问题）</a-select-option>
                <a-select-option value="sample">1% 采样</a-select-option>
                <a-select-option value="off">关闭（冲量跑分）</a-select-option>
              </a-select>
              <div class="hint">
                自动模式下预估 QPS ≥ 2000 降为采样、≥ 5000 关闭明细；聚合指标始终完整
              </div>
            </div>

            <div id="pool-binding" class="field">
              <div class="label pool-label">
                <span>参数池 / Token 池</span>
                <span>
                  <a-button size="small" type="link" @click="openPoolManager">管理池</a-button>
                  <a-button size="small" type="link" :disabled="running" @click="addPoolBinding">
                    + 添加绑定
                  </a-button>
                </span>
              </div>
              <div v-if="!settings.poolBindings.length" class="hint">
                未绑定池时按普通变量取值；绑定后每个 VU 固定取一条或逐请求轮换，适合多账号 / 多 Token / 多手机号场景
              </div>
              <div v-if="settings.poolBindings.length" class="hint">
                目标只是占位符名：在请求的 URL / Query / Header / Body / 认证字段里写
                &#123;&#123;目标名&#125;&#125; 都能取到池值，不限于 token；下拉会列出当前请求里已有的占位符。
                选「全部列」时列名即变量名，会自动注入该条目的所有列。
              </div>
              <div v-for="(binding, index) in settings.poolBindings" :key="index" class="pool-row">
                <a-select
                  v-model:value="binding.pool"
                  size="small"
                  style="width: 30%"
                  :disabled="running"
                  placeholder="选择池"
                  @change="onPoolChange(binding)"
                >
                  <a-select-option v-for="pool in pools" :key="pool.id" :value="pool.name">
                    {{ pool.name }}（{{ pool.itemCount }}）
                  </a-select-option>
                </a-select>
                <a-select
                  v-if="poolColumns(binding.pool).length"
                  :value="binding.column"
                  size="small"
                  style="width: 18%"
                  :disabled="running"
                  placeholder="默认（首列）"
                  :options="poolColumnOptions(binding.pool)"
                  @change="setBindingColumn(binding, $event)"
                />
                <a-auto-complete
                  v-if="binding.column !== '*'"
                  v-model:value="binding.target"
                  size="small"
                  :style="{ width: poolColumns(binding.pool).length ? '22%' : '30%' }"
                  :disabled="running"
                  :options="poolTargetOptions"
                  placeholder="占位符名，如 phone / token"
                />
                <span v-else class="hint column-hint">列名即变量名</span>
                <a-select
                  v-model:value="binding.mode"
                  size="small"
                  style="width: 20%"
                  :disabled="running"
                >
                  <a-select-option value="per_vu">每 VU 固定</a-select-option>
                  <a-select-option value="per_request">逐请求轮换</a-select-option>
                  <a-select-option value="random">随机</a-select-option>
                </a-select>
                <a-button size="small" danger type="text" :disabled="running" @click="removePoolBinding(index)">
                  删除
                </a-button>
              </div>
              <div v-if="settings.poolBindings.length" class="pool-actions">
                <a-checkbox
                  v-for="(binding, index) in settings.poolBindings"
                  :key="`r-${index}`"
                  v-model:checked="binding.refreshOn401"
                  :disabled="running"
                >
                  {{ binding.column === '*' ? binding.pool : binding.target || '绑定' }} 401 自动换下一条
                </a-checkbox>
                <a-button size="small" :loading="poolPreviewLoading" @click="previewPools">
                  预览分配
                </a-button>
              </div>
              <div v-if="poolPreview" class="pool-preview">
                <div v-if="poolPreview.problems.length" class="pool-problems">
                  <div v-for="(problem, index) in poolPreview.problems" :key="index">⚠ {{ problem }}</div>
                </div>
                <div v-else class="hint">未发现池配置问题</div>
                <div class="pool-sample">
                  <div v-for="binding in poolPreview.bindings" :key="binding.pool" class="pool-sample-item">
                    <span class="mono">
                      {{ binding.pool }} →
                      {{ binding.column === '*' ? '自动注入' : binding.column || binding.target }}
                    </span>
                    <span v-if="binding.column === '*'" class="hint">
                      自动注入：{{ (binding.targets ?? []).join('、') || '无列定义' }}
                    </span>
                    <span v-else class="hint">列：{{ binding.column || '首列（默认）' }}</span>
                    <span class="hint">覆盖 {{ binding.vusCovered }} 个 VU · 共 {{ binding.itemCount }} 条</span>
                    <span class="mono">{{ previewSampleText(binding) }}</span>
                  </div>
                </div>
              </div>
            </div>

            <div v-if="request" class="var-panel">
              <button type="button" class="var-head" @click="varOpen = !varOpen">
                <span class="var-title">本次变量</span>
                <span class="hint">{{ varLoading ? '检查中…' : `${varSources.length} 个` }}</span>
                <span v-if="varProblemCount" class="var-flag bad">{{ varProblemCount }} 个待处理</span>
                <span v-else-if="varSources.length" class="var-flag ok">全部可用</span>
                <span class="var-caret">{{ varOpen ? '收起' : '展开' }}</span>
              </button>
              <div v-if="varOpen" class="var-list">
                <div v-if="!varSources.length" class="hint" style="padding: 6px 0">
                  本次配置没有引用占位符。
                </div>
                <div
                  v-for="item in varSources"
                  :key="`${item.kind}:${item.name}`"
                  class="var-row"
                >
                  <code class="var-name">{{ varPlaceholder(item) }}</code>
                  <span class="var-kind">{{ VAR_KIND_TEXT[item.kind] ?? item.kind }}</span>
                  <span class="var-detail">{{ item.detail }}</span>
                  <i class="var-dot" :class="item.ok ? 'ok' : 'bad'" />
                </div>
              </div>
            </div>

            <template v-if="settings.mode === 'fixed'">
              <div class="field">
                <div class="label">并发数</div>
                <a-input-number v-model:value="settings.concurrent" :min="1" :max="MAX_CONCURRENT" style="width: 100%" />
                <div class="hint">同一时刻同时执行的请求数量，建议从 10～30 起步</div>
                <div v-if="settings.concurrent > HIGH_CONCURRENT_HINT" class="hint warn">
                  高并发将占用较多 CPU/内存，建议 8 核以上机器，并确认本机端口与文件句柄限制
                </div>
              </div>

              <div class="field-row">
                <div class="field">
                  <div class="label">压测时长（秒）</div>
                  <a-input-number v-model:value="settings.durationSec" :min="1" :max="fixedDurationMax" style="width: 100%" />
                  <div class="hint">快速压测最长 2 小时；更长时长请保存为压测计划（最长 24 小时）</div>
                </div>
                <div class="field">
                  <div class="label">预热时长（秒）</div>
                  <a-input-number v-model:value="settings.rampUpSec" :min="0" :max="600" style="width: 100%" />
                  <a-select v-model:value="settings.rampUpMode" size="small" style="width: 100%; margin-top: 6px">
                    <a-select-option value="linear">线性进场</a-select-option>
                    <a-select-option value="step">阶梯进场</a-select-option>
                    <a-select-option value="spike">后段冲刺</a-select-option>
                  </a-select>
                  <div class="hint">
                    VU 起始抖动
                    <a-input-number v-model:value="settings.vuStartJitterMs" :min="0" :max="60000" :step="100" size="small" style="width: 90px; margin-left: 6px" />
                    毫秒
                  </div>
                </div>
              </div>
            </template>

            <template v-else>
              <div class="field">
                <div class="label stage-label">
                  <span>加压阶段</span>
                  <a-button size="small" type="link" :disabled="running" @click="addStage">+ 添加阶段</a-button>
                </div>
                <div class="stage-list">
                  <div class="stage-row stage-header">
                    <span />
                    <span>并发数</span>
                    <span>时长（秒）</span>
                    <span />
                  </div>
                  <div v-for="(stage, index) in stages" :key="index" class="stage-row">
                    <span class="stage-index">{{ index + 1 }}</span>
                    <a-input-number
                      v-model:value="stage.concurrent"
                      :min="1"
                      :max="MAX_CONCURRENT"
                      size="small"
                      :disabled="running"
                    />
                    <a-input-number
                      v-model:value="stage.durationSec"
                      :min="1"
                      :max="3600"
                      size="small"
                      :disabled="running"
                    />
                    <a-button
                      size="small"
                      type="text"
                      danger
                      :disabled="running || stages.length <= 1"
                      @click="removeStage(index)"
                    >
                      移除
                    </a-button>
                  </div>
                </div>
                <div class="hint">
                  共 {{ totalSeconds }} 秒，峰值并发 {{ peakConcurrent }}；阶段切换时新并发立即生效
                </div>
              </div>
            </template>

            <div class="field-row">
              <div class="field">
                <div class="label">思考时间</div>
                <a-select v-model:value="settings.thinkTimeMode" size="small" style="width: 100%">
                  <a-select-option value="fixed">固定间隔</a-select-option>
                  <a-select-option value="uniform">均匀分布</a-select-option>
                  <a-select-option value="normal">正态分布</a-select-option>
                  <a-select-option value="lognormal">对数正态</a-select-option>
                </a-select>
                <a-input-number
                  v-if="settings.thinkTimeMode === 'fixed'"
                  v-model:value="settings.thinkTimeMs"
                  :min="0"
                  :max="60000"
                  :step="100"
                  style="width: 100%; margin-top: 6px"
                  addon-after="ms"
                />
                <div v-else-if="settings.thinkTimeMode === 'uniform'" class="field-row" style="margin-top: 6px">
                  <a-input-number v-model:value="settings.thinkTimeMinMs" :min="0" :max="60000" :step="100" addon-after="ms" />
                  <a-input-number v-model:value="settings.thinkTimeMaxMs" :min="0" :max="60000" :step="100" addon-after="ms" />
                </div>
                <div v-else class="field-row" style="margin-top: 6px">
                  <a-input-number v-model:value="settings.thinkTimeMeanMs" :min="0" :max="60000" :step="100" addon-before="均值" addon-after="ms" />
                  <a-input-number v-model:value="settings.thinkTimeStdMs" :min="0" :max="60000" :step="50" addon-before="标准差" addon-after="ms" />
                </div>
                <div class="hint">模拟真实用户停顿时长；固定间隔沿用毫秒值</div>
              </div>
              <div class="field">
                <div class="label">超时（毫秒）</div>
                <a-input-number v-model:value="settings.timeoutMs" :min="500" :max="120000" :step="1000" style="width: 100%" />
              </div>
            </div>

            <div class="field">
              <div class="label">目标 QPS 限速</div>
              <a-input-number v-model:value="settings.targetRps" :min="0" :max="100000" :step="10" style="width: 100%" />
              <div class="hint">0 表示不限速；设置后每秒最多发出该数量的请求</div>
            </div>

            <div class="field">
              <div class="label">
                环境
                <span class="label-required">必选</span>
              </div>
              <a-select v-model:value="envId" allow-clear placeholder="不使用环境变量" style="width: 100%">
                <a-select-option v-for="env in environments" :key="env.id" :value="env.id">
                  {{ env.name }}
                </a-select-option>
              </a-select>
              <div class="hint" v-pre>选择环境后会替换 URL 与 Headers 中的 {{变量}}</div>
              <div v-if="unresolvedVars.length" class="hint warn">
                当前环境缺少变量：{{ unresolvedVars.join('、') }}，补充变量或切换环境后才能开始
              </div>
              <div class="hint capacity-hint">
                <template v-if="capacity">
                  本机能力：{{ capacity.reachedLimit ? '≈' : '≥' }}
                  {{ fmt(capacity.maxQps, 0) }} QPS · 饱和并发
                  {{ capacity.saturationConcurrent }} · 检测于
                  {{ new Date(capacity.probedAt).toLocaleString('zh-CN') }}
                </template>
                <template v-else>未检测本机施压能力，可在「设置 → 本机施压能力」完成检测</template>
              </div>
            </div>

            <a-checkbox v-model:checked="settings.insecure">忽略证书校验（自签证书）</a-checkbox>
            <a-checkbox v-model:checked="ignoreOverlap">
              忽略同目标冲突检查（仅本次有效，报告将标注不可作为单实例容量结论）
            </a-checkbox>
            <div v-if="longRunNeeded" class="long-run-ack">
              <a-checkbox v-model:checked="settings.longRunAck">
                已知悉同机施压风险（本次超过 2 小时，建议使用独立施压机）
              </a-checkbox>
            </div>

            <div class="sla-block">
              <div class="sla-head">
                <a-checkbox v-model:checked="settings.slaEnabled">设置 SLA 验收阈值</a-checkbox>
              </div>
              <div v-if="settings.slaEnabled" class="sla-grid">
                <div class="field">
                  <div class="label">P95 ≤（ms）</div>
                  <a-input-number v-model:value="settings.slaP95" :min="0" :max="120000" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">P99 ≤（ms）</div>
                  <a-input-number v-model:value="settings.slaP99" :min="0" :max="120000" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">错误率 ≤（%）</div>
                  <a-input-number v-model:value="settings.slaErrorRate" :min="0" :max="100" :step="0.5" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">平均 QPS ≥</div>
                  <a-input-number v-model:value="settings.slaMinRps" :min="0" :max="100000" size="small" style="width: 100%" />
                </div>
              </div>
              <div v-if="settings.slaEnabled" class="hint">报告生成时会逐项判定并给出是否达标</div>
            </div>

            <div class="sla-block">
              <div class="sla-head">
                <span class="label">基线对比（可选）</span>
              </div>
              <a-select
                :value="settings.baselineReportId ?? undefined"
                :options="baselineOptions"
                allow-clear
                show-search
                option-filter-prop="label"
                placeholder="选择历史低并发报告作为基线"
                style="width: 100%"
                @change="setBaseline"
              />
              <div class="hint">
                报告详情将对比 P95 / P99 / 平均 QPS / 错误率；建议选择同接口的低并发基线，对比才有意义
              </div>
            </div>
          </div>

          <div class="config-foot">
            <a-button
              v-if="!running"
              type="primary"
              size="large"
              block
              :disabled="!request"
              @click="start"
            >
              <template #icon><PlayCircleOutlined /></template>
              开始压测
            </a-button>
            <a-button v-else type="primary" danger size="large" block :loading="stopping" @click="stop">
              <template #icon><StopOutlined /></template>
              {{ stopping ? '正在停止…' : '停止压测' }}
            </a-button>
            <div class="leave-hint">
              离开或关闭本页不会中断压测；任务在后台继续，可在报告页查看结果，需要中止请点「停止压测」
            </div>
          </div>
        </section>

        <section class="live">
          <div class="fm-card status-card">
            <div class="status-head">
              <div class="status-left">
                <span class="status-dot" :class="{ live: running, done: !running && done }" />
                <span class="status-text">
                  {{ running ? '压测进行中' : done ? '压测已结束' : '等待开始' }}
                </span>
                <span v-if="running" class="status-elapsed mono">{{ elapsed }}s / {{ totalSeconds }}s</span>
                <span v-if="running && stageLabel" class="status-stage">{{ stageLabel }}</span>
                <span v-if="running && recordModeText" class="status-stage">{{ recordModeText }}</span>
              </div>
              <span class="status-name">{{ request.name }}</span>
            </div>
            <a-progress
              :percent="running || done ? percent : 0"
              :show-info="false"
              :stroke-color="running ? 'var(--fm-primary)' : done ? 'var(--fm-success)' : 'var(--fm-border-strong)'"
              size="small"
            />
            <div class="stats">
              <div class="stat">
                <span class="stat-label">总请求</span>
                <span class="stat-value mono">{{ progress?.total ?? done?.total ?? 0 }}</span>
              </div>
              <div class="stat">
                <span class="stat-label">当前 QPS</span>
                <span class="stat-value mono">{{ fmt(currentRps) }}</span>
              </div>
              <div class="stat">
                <span class="stat-label">平均 QPS</span>
                <span class="stat-value mono">{{ fmt(avgQps) }}</span>
              </div>
              <div class="stat">
                <span class="stat-label">平均延迟</span>
                <span class="stat-value mono">{{ fmt(progress?.avgRt) }} <i>ms</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">P95</span>
                <span class="stat-value mono warn">{{ fmt(progress?.p95Rt) }} <i>ms</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">错误率</span>
                <span class="stat-value mono" :class="{ bad: errorRate > 0 }">{{ errorRate.toFixed(1) }} <i>%</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">并发</span>
                <span class="stat-value mono">{{ progress?.concurrent ?? 0 }} <i>/ {{ peakConcurrent }}</i></span>
              </div>
            </div>

            <div v-if="statusCodes.length > 0" class="status-codes">
              <span class="codes-title">状态码分布</span>
              <div class="status-code-item" v-for="item in statusCodes" :key="item.code">
                <span class="code" :class="`code-${codeClass(item.code)}`">{{ item.code }}</span>
                <span class="count">{{ item.count }}</span>
                <span class="ratio">{{ statusCodeTotal > 0 ? ((item.count / statusCodeTotal) * 100).toFixed(1) : '0.0' }}%</span>
              </div>
            </div>
          </div>

          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">吞吐量 QPS</span>
              <span class="chart-sub">按秒统计 · 峰值 {{ fmt(peakQps) }} QPS</span>
            </div>
            <LineChart :series="qpsSeries" :labels="chartLabels" unit="" />
          </div>

          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">响应延迟</span>
              <span class="chart-sub">单位 ms</span>
            </div>
            <LineChart :series="latencySeries" :labels="chartLabels" unit="ms" :area="false" />
          </div>

          <div v-if="done" ref="resultCard" class="fm-card result-card">
            <div class="result-icon"><FileTextOutlined /></div>
            <div class="result-main">
              <div class="result-title">
                {{ done.status === 'stopped' ? '压测已手动停止' : '压测完成' }}
              </div>
              <div class="result-desc">
                共 {{ done.total }} 次请求，成功 {{ done.success }} 次，失败 {{ done.errors }} 次，
                耗时 {{ done.elapsedSec }} 秒{{ done.message ? ` · ${done.message}` : '' }}
              </div>
            </div>
            <a-button v-if="done.reportId" type="primary" @click="router.push(`/reports/${done.reportId}`)">
              查看报告
            </a-button>
          </div>

          <div v-if="!progress && !done" class="fm-card tips-card">
            <div class="tips-title"><InfoCircleOutlined /> 压测前请确认</div>
            <ul>
              <li>只对你有权限的测试环境压测，避免影响线上流量。</li>
              <li>并发数不要一次拉满，先小并发观察错误率与 P95 延迟。</li>
              <li>配置了限流或防刷策略的接口，压测结果会包含大量 429/403。</li>
            </ul>
          </div>
        </section>
      </div>

      <div v-else-if="!loading" class="fm-card guide-empty">
        <div class="empty-title">没有找到这个请求</div>
        <div class="empty-desc">请求可能已经被删除，请回到项目重新选择。</div>
        <a-button type="primary" @click="openConfig">重新选择接口</a-button>
      </div>
    </a-spin>

    <div class="reports-section">
      <div class="reports-head">
        <div>
          <div class="reports-title">压测报告记录</div>
          <div class="reports-sub">最近 200 份，点击「查看详情」查看逐请求明细</div>
        </div>
        <a-button size="small" @click="reportList?.reload()">刷新</a-button>
      </div>
      <StressReportList ref="reportList" />
    </div>

    <a-modal
      v-model:open="configOpen"
      title="新建压测"
      :width="1040"
      :mask-closable="false"
      class="stress-config-modal"
      ok-text="开始压测"
      cancel-text="取消"
      :confirm-loading="preparing"
      @ok="startFromConfig"
    >
      <div class="modal-body">
        <div class="modal-section plan-section">
          <div class="section-title">
            <span class="step">0</span>压测计划
            <span class="section-sub">Plan</span>
          </div>
          <div class="plan-row">
            <a-select
              :value="null"
              placeholder="计划模板"
              class="plan-template"
              :options="planTemplates.map((item) => ({ value: item.id, label: item.name }))"
              @change="applyTemplateById"
            />
            <a-select
              :value="activePlanId"
              allow-clear
              placeholder="载入已保存的压测计划"
              class="plan-select"
              :options="plans.map((item) => ({ value: item.id, label: `${item.name}${item.requestName ? ` · ${item.requestName}` : ''}` }))"
              @change="applyPlan"
            />
            <a-input
              v-model:value="planName"
              class="plan-name"
              placeholder="计划名称（留空自动命名）"
            />
            <a-button :loading="planSaving" :disabled="!targetProjectId" @click="savePlan">
              <template #icon><SaveOutlined /></template>
              保存计划
            </a-button>
            <a-button
              v-if="activePlanId"
              danger
              type="text"
              @click="removePlan"
            >
              <template #icon><DeleteOutlined /></template>
              删除
            </a-button>
          </div>
        </div>

        <div class="modal-section">
          <div class="section-title">
            <span class="step">1</span>选择目标接口
            <span class="section-sub">Target</span>
          </div>
          <div class="target-grid">
            <div class="field">
              <div class="label">项目</div>
              <a-select
                v-model:value="targetProjectId"
                :options="projectOptions"
                placeholder="选择项目"
                show-search
                option-filter-prop="label"
                popup-class-name="stress-select-dropdown"
              />
            </div>
            <div class="field field-env">
              <div class="label">环境变量</div>
              <a-select
                v-model:value="modalEnvId"
                allow-clear
                placeholder="不使用环境变量"
                popup-class-name="stress-select-dropdown"
              >
                <a-select-option v-for="env in modalEnvironments" :key="env.id" :value="env.id">
                  {{ env.name }}
                </a-select-option>
              </a-select>
            </div>
            <div class="field field-request">
              <div class="label">
                接口
                <span class="label-hint">可多选：各接口保留自己的 Headers、Params、Body，共享同一 Token / 参数池</span>
              </div>
              <a-select
                v-model:value="targetRequestIds"
                mode="multiple"
                :loading="choicesLoading"
                placeholder="选择一个或多个接口一起压测"
                show-search
                option-filter-prop="label"
                option-label-prop="label"
                :max-tag-count="3"
                popup-class-name="stress-select-dropdown stress-req-dropdown"
                :not-found-content="choicesLoading ? '加载中…' : '该项目下暂无接口'"
              >
                <a-select-option
                  v-for="item in requestSelectOptions"
                  :key="item.value"
                  :value="item.value"
                  :label="item.label"
                >
                  <div class="req-option">
                    <span class="req-option-main">
                      <span class="method-tag method-mini" :class="`method-${item.method}`">{{ item.method }}</span>
                      <span class="req-option-name">{{ item.name }}</span>
                    </span>
                    <span class="req-option-url mono">{{ item.url }}</span>
                  </div>
                </a-select-option>
              </a-select>
            </div>
          </div>
          <div v-if="selectedChoices.length" class="target-card">
            <div class="target-card-head">
              <span class="target-card-badge">
                {{ selectedChoices.length > 1 ? `已选 ${selectedChoices.length} 个接口` : '已选目标' }}
              </span>
            </div>
            <div v-for="(item, index) in selectedChoices" :key="item.id" class="target-item">
              <div class="target-item-main">
                <span class="target-item-index">{{ index + 1 }}</span>
                <span class="method-tag" :class="`method-${item.method}`">{{ item.method }}</span>
                <span class="target-card-name" :title="item.name">{{ item.name }}</span>
                <span class="target-card-collection" :title="item.collectionName">
                  {{ item.collectionName }}
                </span>
                <span v-if="selectedChoices.length > 1" class="target-weight">
                  <span class="target-weight-label">权重</span>
                  <a-input-number
                    size="small"
                    :value="requestWeight(item.id)"
                    :min="0"
                    :max="1000"
                    :precision="0"
                    placeholder="平均"
                    @change="(value: number | string | null) => setRequestWeight(item.id, value)"
                  />
                </span>
              </div>
              <div class="target-card-url mono" :title="item.url">{{ item.url }}</div>
              <div
                v-if="index === 0 && previewUrl && previewUrl !== item.url"
                class="target-card-url resolved mono"
                :title="previewUrl"
              >
                解析后：{{ previewUrl }}
              </div>
            </div>
            <div v-if="weightHint" class="hint">{{ weightHint }}</div>
          </div>
          <div v-else-if="currentProject" class="hint">
            {{ currentProject.name }} · 共 {{ choices.length }} 个接口
          </div>
        </div>

        <div class="modal-section">
          <div class="section-title">
            <span class="step">2</span>配置压测参数
            <span class="section-sub">JMeter 风格</span>
          </div>
          <div class="modal-fields">
            <!-- 线程组 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">线程组</span>
                <span class="param-sub">Thread Group</span>
              </div>
              <div class="param-body">
                <div class="field">
                  <div class="label">加压模式</div>
                  <a-segmented
                    v-model:value="settings.mode"
                    :options="[
                      { label: '固定并发', value: 'fixed' },
                      { label: '阶梯加压', value: 'stages' },
                    ]"
                    block
                  />
                  <div class="hint">
                    {{ settings.mode === 'fixed'
                      ? '固定线程数持续压测，适合验证稳定吞吐'
                      : '按阶段逐步提升并发，用于寻找性能拐点' }}
                  </div>
                </div>

                <div class="field">
                  <div class="label">采集预设</div>
                  <a-segmented
                    v-model:value="settings.captureMode"
                    :options="[
                      { label: '自动', value: 'auto' },
                      { label: '容量', value: 'capacity' },
                      { label: '定位', value: 'locate' },
                    ]"
                    block
                  />
                  <div class="hint">
                    {{
                      settings.captureMode === 'capacity'
                        ? '容量模式按实测 QPS 自动降档，优先保证吞吐与稳定性'
                        : settings.captureMode === 'locate'
                          ? '定位模式保留最慢的 N 条请求，适合排查慢接口（建议低并发）'
                          : '自动模式按 QPS 阈值决定采集档位'
                    }}
                  </div>
                  <div v-if="settings.captureMode === 'locate'" class="field-row">
                    <div class="field">
                      <div class="label">慢请求 TopN</div>
                      <a-input-number
                        v-model:value="settings.topN"
                        :min="1"
                        :max="2000"
                        style="width: 100%"
                      />
                    </div>
                    <div class="field">
                      <div class="label">只记录慢于（ms，0 = 不限）</div>
                      <a-input-number
                        v-model:value="settings.slowThresholdMs"
                        :min="0"
                        :max="120000"
                        :step="100"
                        style="width: 100%"
                      />
                    </div>
                  </div>
                </div>

                <template v-if="settings.mode === 'fixed'">
                  <div class="field-row">
                    <div class="field">
                      <div class="label">线程数（并发）</div>
                      <a-input-number
                        v-model:value="settings.concurrent"
                        :min="1"
                        :max="MAX_CONCURRENT"
                        style="width: 100%"
                      />
                      <div class="hint">建议从 10～30 起步，观察错误率后再提高</div>
                    </div>
                    <div class="field">
                      <div class="label">持续时间（秒）</div>
                      <a-input-number
                        v-model:value="settings.durationSec"
                        :min="1"
                        :max="86400"
                        style="width: 100%"
                      />
                      <div class="hint">压测计划最长 24 小时；超过 2 小时需勾选风险确认</div>
                    </div>
                  </div>
                  <div class="field">
                    <div class="label">Ramp-Up 时间（秒）</div>
                    <a-input-number
                      v-model:value="settings.rampUpSec"
                      :min="0"
                      :max="600"
                      style="width: 100%"
                    />
                    <div class="label ramp-label">进场方式</div>
                    <a-segmented
                      v-model:value="settings.rampUpMode"
                      block
                      :options="[
                        { label: '线性', value: 'linear' },
                        { label: '阶梯', value: 'step' },
                        { label: '后段冲刺', value: 'spike' },
                      ]"
                    />
                    <div class="hint">并发从 0 逐步升到目标值的时间，需小于持续时间</div>
                  </div>
                  <div class="field">
                    <div class="label">VU 起始抖动（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.vuStartJitterMs"
                      :min="0"
                      :max="60000"
                      :step="100"
                      style="width: 100%"
                    />
                    <div class="hint">每个线程额外随机延迟，避免同一毫秒集中进场</div>
                  </div>
                </template>
                <template v-else>
                  <div class="field">
                    <div class="label stage-label">
                      <span>加压阶段</span>
                      <a-button size="small" type="link" @click="addStage">+ 添加阶段</a-button>
                    </div>
                    <div class="stage-list">
                      <div class="stage-row stage-header">
                        <span />
                        <span>并发数</span>
                        <span>时长（秒）</span>
                        <span />
                      </div>
                      <div v-for="(stage, index) in stages" :key="index" class="stage-row">
                        <span class="stage-index">{{ index + 1 }}</span>
                        <a-input-number v-model:value="stage.concurrent" :min="1" :max="MAX_CONCURRENT" />
                        <a-input-number v-model:value="stage.durationSec" :min="1" :max="3600" />
                        <a-button size="small" type="text" danger :disabled="stages.length <= 1" @click="removeStage(index)">
                          移除
                        </a-button>
                      </div>
                    </div>
                    <div class="hint">共 {{ totalSeconds }} 秒，峰值并发 {{ peakConcurrent }}</div>
                    <div v-if="peakConcurrent > HIGH_CONCURRENT_HINT" class="hint warn">
                      高并发将占用较多 CPU/内存，建议 8 核以上机器，并确认本机端口与文件句柄限制
                    </div>
                  </div>
                </template>
              </div>
            </div>

            <!-- HTTP 请求默认值 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">HTTP 请求默认值</span>
                <span class="param-sub">HTTP Request Defaults</span>
              </div>
              <div class="param-body">
                <div class="field-row">
                  <div class="field">
                    <div class="label">连接超时（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.connectTimeoutMs"
                      :min="500"
                      :max="120000"
                      :step="1000"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">请求超时（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.timeoutMs"
                      :min="500"
                      :max="120000"
                      :step="1000"
                      style="width: 100%"
                    />
                  </div>
                </div>
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.followRedirects">跟随 3xx 重定向</a-checkbox>
                  <a-checkbox v-model:checked="settings.insecure">忽略证书校验（自签证书）</a-checkbox>
                </div>
              </div>
            </div>

            <!-- 参数池 / Token 池 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">参数池 / Token 池</span>
                <span class="param-sub">Data Pools</span>
                <span class="param-head-actions">
                  <a-button size="small" type="link" @click="openPoolManager">管理池</a-button>
                  <a-button size="small" type="link" :disabled="running" @click="addPoolBinding">
                    + 添加绑定
                  </a-button>
                </span>
              </div>
              <div class="param-body">
                <div v-if="!settings.poolBindings.length" class="hint">
                  多账号 / 多 Token / 多手机号可在这里绑定：选池、填占位符名，请求任意位置写
                  &#123;&#123;名字&#125;&#125; 即可取到池值；选「全部列」时列名即变量名
                </div>
                <div v-for="(binding, index) in settings.poolBindings" :key="index" class="pool-row">
                  <a-select
                    v-model:value="binding.pool"
                    size="small"
                    style="width: 30%"
                    :disabled="running"
                    placeholder="选择池"
                    @change="onPoolChange(binding)"
                  >
                    <a-select-option v-for="pool in pools" :key="pool.id" :value="pool.name">
                      {{ pool.name }}（{{ pool.itemCount }}）
                    </a-select-option>
                  </a-select>
                  <a-select
                    v-if="poolColumns(binding.pool).length"
                    :value="binding.column"
                    size="small"
                    style="width: 18%"
                    :disabled="running"
                    placeholder="默认（首列）"
                    :options="poolColumnOptions(binding.pool)"
                    @change="setBindingColumn(binding, $event)"
                  />
                  <a-auto-complete
                    v-if="binding.column !== '*'"
                    v-model:value="binding.target"
                    size="small"
                    :style="{ width: poolColumns(binding.pool).length ? '22%' : '30%' }"
                    :disabled="running"
                    :options="poolTargetOptions"
                    placeholder="占位符名，如 phone / token"
                  />
                  <span v-else class="hint column-hint">列名即变量名</span>
                  <a-select
                    v-model:value="binding.mode"
                    size="small"
                    style="width: 20%"
                    :disabled="running"
                  >
                    <a-select-option value="per_vu">每 VU 固定</a-select-option>
                    <a-select-option value="per_request">逐请求轮换</a-select-option>
                    <a-select-option value="random">随机</a-select-option>
                  </a-select>
                  <a-button size="small" danger type="text" :disabled="running" @click="removePoolBinding(index)">
                    删除
                  </a-button>
                </div>
                <div v-if="settings.poolBindings.length" class="pool-actions">
                  <a-checkbox
                    v-for="(binding, index) in settings.poolBindings"
                    :key="`r-${index}`"
                    v-model:checked="binding.refreshOn401"
                    :disabled="running"
                  >
                    {{ binding.column === '*' ? binding.pool : binding.target || '绑定' }} 401 自动换下一条
                  </a-checkbox>
                  <a-button size="small" :loading="poolPreviewLoading" @click="previewPools">
                    预览分配
                  </a-button>
                </div>
                <div v-if="poolPreview" class="pool-preview">
                  <div v-if="poolPreview.problems.length" class="pool-problems">
                    <div v-for="(problem, index) in poolPreview.problems" :key="index">⚠ {{ problem }}</div>
                  </div>
                  <div v-else class="hint">未发现池配置问题</div>
                  <div class="pool-sample">
                    <div v-for="binding in poolPreview.bindings" :key="binding.pool" class="pool-sample-item">
                      <span class="mono">
                        {{ binding.pool }} →
                        {{ binding.column === '*' ? '自动注入' : binding.column || binding.target }}
                      </span>
                      <span v-if="binding.column === '*'" class="hint">
                        自动注入：{{ (binding.targets ?? []).join('、') || '无列定义' }}
                      </span>
                      <span v-else class="hint">列：{{ binding.column || '首列（默认）' }}</span>
                      <span class="hint">覆盖 {{ binding.vusCovered }} 个 VU · 共 {{ binding.itemCount }} 条</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- 流量与节奏 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">流量与节奏</span>
                <span class="param-sub">Timer &amp; Throughput</span>
              </div>
              <div class="param-body">
                <div class="field-row">
                  <div class="field">
                    <div class="label">思考时间</div>
                    <a-select v-model:value="settings.thinkTimeMode" size="small" style="width: 100%">
                      <a-select-option value="fixed">固定间隔</a-select-option>
                      <a-select-option value="uniform">均匀分布</a-select-option>
                      <a-select-option value="normal">正态分布</a-select-option>
                      <a-select-option value="lognormal">对数正态</a-select-option>
                    </a-select>
                    <a-input-number
                      v-if="settings.thinkTimeMode === 'fixed'"
                      v-model:value="settings.thinkTimeMs"
                      :min="0"
                      :max="60000"
                      :step="100"
                      style="width: 100%; margin-top: 6px"
                      addon-after="ms"
                    />
                    <div v-else-if="settings.thinkTimeMode === 'uniform'" class="field-row" style="margin-top: 6px">
                      <a-input-number v-model:value="settings.thinkTimeMinMs" :min="0" :max="60000" :step="100" addon-after="ms" />
                      <a-input-number v-model:value="settings.thinkTimeMaxMs" :min="0" :max="60000" :step="100" addon-after="ms" />
                    </div>
                    <div v-else class="field-row" style="margin-top: 6px">
                      <a-input-number v-model:value="settings.thinkTimeMeanMs" :min="0" :max="60000" :step="100" addon-before="均值" addon-after="ms" />
                      <a-input-number v-model:value="settings.thinkTimeStdMs" :min="0" :max="60000" :step="50" addon-before="标准差" addon-after="ms" />
                    </div>
                    <div class="hint">模拟真实用户停顿；固定间隔沿用毫秒值</div>
                  </div>
                  <div class="field">
                    <div class="label">目标 QPS 限速</div>
                    <a-input-number
                      v-model:value="settings.targetRps"
                      :min="0"
                      :max="100000"
                      :step="10"
                      style="width: 100%"
                    />
                    <div class="hint">0 表示不限速；每秒最多发出该数量的请求</div>
                  </div>
                </div>
              </div>
            </div>

            <!-- 自动停止 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">自动停止</span>
                <span class="param-sub">Auto Stop</span>
              </div>
              <div class="param-body">
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.stopOnErrorRate">错误率超过阈值时提前停止</a-checkbox>
                </div>
                <div v-if="settings.stopOnErrorRate" class="field-row">
                  <div class="field">
                    <div class="label">错误率阈值（%）</div>
                    <a-input-number
                      v-model:value="settings.stopErrorRate"
                      :min="0.1"
                      :max="100"
                      :step="0.5"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">最少请求数</div>
                    <a-input-number
                      v-model:value="settings.stopMinRequests"
                      :min="1"
                      :max="1000000"
                      :step="100"
                      style="width: 100%"
                    />
                    <div class="hint">达到该请求数后才开始判定，避免起步阶段误停</div>
                  </div>
                </div>
              </div>
            </div>

            <!-- SLA -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">SLA 验收</span>
                <span class="param-sub">Service Level</span>
              </div>
              <div class="param-body">
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.slaEnabled">设置 SLA 验收阈值</a-checkbox>
                </div>
                <div v-if="settings.slaEnabled" class="sla-grid">
                  <div class="field">
                    <div class="label">P95 ≤（ms）</div>
                    <a-input-number v-model:value="settings.slaP95" :min="0" :max="120000" style="width: 100%" />
                  </div>
                  <div class="field">
                    <div class="label">P99 ≤（ms）</div>
                    <a-input-number v-model:value="settings.slaP99" :min="0" :max="120000" style="width: 100%" />
                  </div>
                  <div class="field">
                    <div class="label">错误率 ≤（%）</div>
                    <a-input-number
                      v-model:value="settings.slaErrorRate"
                      :min="0"
                      :max="100"
                      :step="0.5"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">平均 QPS ≥</div>
                    <a-input-number v-model:value="settings.slaMinRps" :min="0" :max="100000" style="width: 100%" />
                  </div>
                </div>
                <div v-if="settings.slaEnabled" class="hint">报告生成时会逐项判定并给出是否达标</div>
              </div>
            </div>

            <!-- 长稳压测确认 -->
            <div v-if="longRunNeeded" class="param-card long-run-card">
              <div class="param-head">
                <span class="param-title">长稳压测确认</span>
                <span class="param-sub">Long Run</span>
              </div>
              <div class="param-body">
                <a-checkbox v-model:checked="settings.longRunAck">
                  已知悉同机施压风险：本次时长 {{ Math.round(totalSeconds / 60) }} 分钟，建议使用独立施压机
                </a-checkbox>
                <div class="hint">超过 2 小时的压测默认关闭明细采集，报告体积仍会随明细增长</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <div class="modal-foot">
          <span class="modal-foot-hint">
            <InfoCircleOutlined /> 开始后进入实时监控；关闭或离开页面不会中断任务，需要中止请点「停止压测」
          </span>
          <div class="modal-foot-actions">
            <a-button @click="configOpen = false">取消</a-button>
            <a-button type="primary" :loading="preparing" @click="startFromConfig">
              <template #icon><PlayCircleOutlined /></template>
              开始压测
            </a-button>
          </div>
        </div>
      </template>
    </a-modal>

    <!-- 参数池 / Token 池管理：建池、导入条目、查看脱敏值 -->
    <a-modal
      v-model:open="poolManagerOpen"
      title="参数池 / Token 池管理"
      :width="920"
      :footer="null"
      class="pool-manager-modal"
    >
      <div class="pool-manager">
        <div class="pool-side">
          <div class="pool-create">
            <a-input
              v-model:value="newPoolName"
              size="small"
              placeholder="新池名称"
              @press-enter="createPool"
            />
            <a-select v-model:value="newPoolKind" size="small" class="pool-kind">
              <a-select-option value="token" title="账号 / Token / API Key / Cookie 等凭证"
                >Token 池</a-select-option
              >
              <a-select-option value="parameter" title="关键词、页码、开关等业务字段"
                >参数池</a-select-option
              >
            </a-select>
            <a-checkbox v-model:checked="newPoolSensitive">敏感</a-checkbox>
            <a-button
              size="small"
              type="primary"
              :loading="poolManagerBusy"
              @click="createPool"
            >
              新建
            </a-button>
          </div>
          <div class="pool-list">
            <div
              v-for="pool in pools"
              :key="pool.id"
              class="pool-list-item"
              :class="{ active: pool.id === activePoolId }"
              @click="openPoolItems(pool.id)"
            >
              <div class="pool-list-main">
                <span class="pool-list-name">{{ pool.name }}</span>
                <span class="pool-list-tag">{{ pool.kind === 'token' ? 'Token' : '参数' }}</span>
                <span v-if="pool.sensitive" class="pool-list-tag sensitive">敏感</span>
              </div>
              <div class="pool-list-meta">
                <span>{{ pool.itemCount }} 条</span>
                <a-button size="small" type="link" danger @click.stop="deletePool(pool.id)">
                  删除
                </a-button>
              </div>
            </div>
            <div v-if="!pools.length" class="pool-empty">还没有池，先在上方新建一个</div>
          </div>
        </div>

        <div class="pool-detail">
          <template v-if="activePoolId">
            <div class="pool-detail-head">
              <span>条目（{{ activePoolItems.length }}）</span>
              <span class="hint">敏感池按脱敏值展示，执行时才取明文</span>
            </div>
            <div class="pool-item-list">
              <div v-for="item in activePoolItems" :key="item.id" class="pool-item">
                <span class="mono pool-item-value">{{ item.masked || item.value || '—' }}</span>
                <span v-if="item.account" class="pool-item-account">{{ item.account }}</span>
                <span class="pool-item-weight">权重 {{ item.weight }}</span>
                <a-button size="small" type="text" danger @click="removePoolItem(item.id)">
                  删除
                </a-button>
              </div>
              <div v-if="!activePoolItems.length" class="pool-empty">暂无条目，可在下方导入</div>
            </div>
            <div class="pool-import">
              <a-textarea
                v-model:value="poolImportText"
                :rows="5"
                :disabled="poolManagerBusy"
                placeholder="每行一条，逗号分隔「值,账号,权重」；也可粘贴 JSON 数组，如 [{&quot;token&quot;:&quot;t1&quot;,&quot;account&quot;:&quot;a&quot;}]"
              />
              <div class="pool-import-foot">
                <span class="hint"># 开头的行会被忽略，权重留空按 1 处理</span>
                <a-button
                  size="small"
                  type="primary"
                  :loading="poolManagerBusy"
                  @click="importPoolItems"
                >
                  导入
                </a-button>
              </div>
            </div>
          </template>
          <div v-else class="pool-empty">选择左侧的池查看条目</div>
        </div>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.reports-section {
  margin-top: 22px;
  padding-top: 4px;
}
.reports-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
}
.reports-title {
  font-size: 14px;
  font-weight: 600;
}
.reports-sub {
  font-size: 11px;
  color: var(--fm-muted);
  margin-top: 2px;
}

.layout {
  display: grid;
  grid-template-columns: 330px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}

/* ---------- 配置 ---------- */
.config {
  position: sticky;
  top: 20px;
  max-height: calc(100vh - 40px);
  overflow-y: auto;
}

.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 46px;
  padding: 0 16px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 13px;
  font-weight: 600;
}

.pane-tag {
  font-size: 10px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  padding: 2px 8px;
  border-radius: 999px;
  font-weight: 500;
}

.target {
  padding: 14px 16px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.target-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.target-name {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.target-url {
  margin-top: 8px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-2);
  word-break: break-all;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  padding: 7px 9px;
}

.target-meta {
  margin-top: 8px;
  font-size: 11px;
  color: var(--fm-muted);
}

.form {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.field-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.label {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
  margin-bottom: 6px;
}

.hint {
  margin-top: 5px;
  font-size: 10px;
  line-height: 1.6;
  color: var(--fm-muted);
  overflow-wrap: anywhere;
}

.hint.warn {
  color: var(--fm-warning);
}

.pool-label {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
}

.pool-row {
  display: flex;
  /* 配置卡片较窄时让控件按最小宽度换行，避免百分比宽度互相挤压 */
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
}

.pool-row > :deep(.ant-select) {
  flex: 1 1 120px;
  min-width: 0;
}

.pool-row > :deep(.ant-btn) {
  flex: none;
  margin-left: auto;
}

/* 行内提示默认带 margin-top，会让「列名即变量名」与控件错位 */
.pool-row > .column-hint {
  flex: 1 1 auto;
  margin-top: 0;
  white-space: nowrap;
}

.pool-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  margin-top: 6px;
}

.pool-preview {
  margin-top: 8px;
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-2);
}

.pool-problems {
  color: var(--fm-warning);
  font-size: 12px;
}

.pool-sample {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 6px;
}

.pool-sample-item {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
}

/* ---------- 本次变量（开跑前自查） ---------- */
.var-panel {
  margin-top: 10px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-2);
}

.var-head {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  border: none;
  background: transparent;
  font-size: 12px;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.var-title {
  font-weight: 500;
  color: var(--fm-text);
}

.var-flag {
  font-size: 11px;
}

.var-flag.bad {
  color: var(--fm-danger);
}

.var-flag.ok {
  color: var(--fm-success);
}

.var-caret {
  margin-left: auto;
  font-size: 11px;
  color: var(--fm-muted);
}

.var-list {
  display: flex;
  flex-direction: column;
  padding: 0 10px 8px;
}

.var-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  border-top: 1px solid var(--fm-border);
}

.var-name {
  flex: none;
  min-width: 168px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11.5px;
  color: var(--fm-text);
}

.var-kind {
  flex: none;
  width: 30px;
  font-size: 11px;
  color: var(--fm-muted);
}

.var-detail {
  flex: 1;
  min-width: 0;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 方形状态点：与纸感主题的小圆角一致，也避免再引入一个字面圆角 */
.var-dot {
  flex: none;
  width: 5px;
  height: 5px;
}

.var-dot.ok {
  background: var(--fm-success);
}

.var-dot.bad {
  background: var(--fm-danger);
}

/* ---------- 池管理弹窗 ---------- */
.pool-manager {
  display: grid;
  grid-template-columns: 300px minmax(0, 1fr);
  gap: 16px;
  min-height: 360px;
}

.pool-side {
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-right: 1px solid var(--fm-border);
  padding-right: 16px;
}

.pool-create {
  display: flex;
  align-items: center;
  gap: 6px;
}

.pool-create .pool-kind {
  width: 100px;
  flex: none;
}

.pool-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
  max-height: 420px;
}

.pool-list-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 9px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.pool-list-item:hover {
  border-color: var(--fm-primary);
}

.pool-list-item.active {
  border-color: var(--fm-primary);
  background: var(--fm-surface-2);
}

.pool-list-main {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.pool-list-name {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pool-list-tag {
  flex: none;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--fm-surface-2);
  border: 1px solid var(--fm-border);
  font-size: 10px;
  color: var(--fm-muted);
}

.pool-list-tag.sensitive {
  color: var(--fm-warning);
  border-color: var(--fm-warning);
}

.pool-list-meta {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
  font-size: 11px;
  color: var(--fm-muted);
}

.pool-detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}

.pool-detail-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  font-size: 13px;
  font-weight: 600;
}

.pool-item-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
  max-height: 240px;
}

.pool-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 8px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  font-size: 12px;
}

.pool-item-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pool-item-account,
.pool-item-weight {
  flex: none;
  font-size: 11px;
  color: var(--fm-muted);
}

.pool-import {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.pool-import-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.pool-empty {
  padding: 18px 10px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.config-foot {
  padding: 0 16px 16px;
}

.leave-hint {
  margin-top: 9px;
  text-align: center;
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 实时面板 ---------- */
.live {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-width: 0;
}

.status-card {
  padding: 16px 18px 18px;
}

.status-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.status-left {
  display: flex;
  align-items: center;
  gap: 9px;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.status-dot.live {
  background: var(--fm-success);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--fm-success) 14%, transparent);
  animation: pulse 1.6s infinite;
}

.status-dot.done {
  background: var(--fm-primary);
}

@keyframes pulse {
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

.status-text {
  font-size: 13px;
  font-weight: 600;
}

.status-elapsed {
  font-size: 11px;
  color: var(--fm-muted);
}

.status-name {
  font-size: 11px;
  color: var(--fm-muted);
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-stage {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 999px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

/* ---------- 阶梯加压阶段 ---------- */
.stage-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.stage-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.stage-row {
  display: grid;
  grid-template-columns: 20px 1fr 1fr auto;
  align-items: center;
  gap: 8px;
}

.stage-header {
  font-size: 10px;
  color: var(--fm-muted);
}

.stage-header span:not(:first-child):not(:last-child) {
  padding-left: 2px;
}

.stage-index {
  width: 20px;
  height: 20px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* ---------- SLA ---------- */
.sla-block {
  padding: 12px;
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
}

.sla-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 10px;
}

/* ---------- 状态码分布 ---------- */
.status-codes {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed var(--fm-border);
}

.codes-title {
  font-size: 10px;
  color: var(--fm-muted);
}

.status-code-item {
  display: flex;
  align-items: baseline;
  gap: 5px;
  padding: 3px 9px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.status-code-item .code {
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.status-code-item .code-ok {
  color: var(--fm-success);
}

.status-code-item .code-info {
  color: var(--fm-primary);
}

.status-code-item .code-warn {
  color: var(--fm-warning);
}

.status-code-item .code-bad {
  color: var(--fm-danger);
}

.status-code-item .count {
  font-size: 11px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.status-code-item .ratio {
  font-size: 9px;
  color: var(--fm-muted);
}

.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(118px, 1fr));
  gap: 10px;
  margin-top: 16px;
}

.stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 10px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
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
  font-size: 15px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
}

.stat-value i {
  font-style: normal;
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
}

.stat-value.warn {
  color: var(--fm-warning);
}

.stat-value.bad {
  color: var(--fm-danger);
}

.chart-card {
  padding: 14px 18px 6px;
}

.chart-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 2px;
}

.chart-title {
  font-size: 13px;
  font-weight: 600;
}

.chart-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

.result-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px 18px;
  border-color: var(--fm-success);
  background: var(--fm-success-soft);
}

.result-icon {
  width: 38px;
  height: 38px;
  flex-shrink: 0;
  border-radius: var(--fm-radius);
  background: var(--fm-success-soft);
  color: var(--fm-success);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
}

.result-main {
  flex: 1;
  min-width: 0;
}

.result-title {
  font-size: 13px;
  font-weight: 600;
}

.result-desc {
  margin-top: 3px;
  font-size: 11px;
  color: var(--fm-muted);
  line-height: 1.6;
}

.tips-card {
  padding: 16px 18px;
}

.tips-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  margin-bottom: 8px;
}

.tips-card ul {
  margin: 0;
  padding-left: 18px;
}

.tips-card li {
  font-size: 11px;
  line-height: 1.9;
  color: var(--fm-muted);
}

.guide-empty {
  padding: 64px 24px;
  text-align: center;
}

.empty-icon-wrap {
  width: 56px;
  height: 56px;
  margin: 0 auto 16px;
  border-radius: var(--fm-radius-lg);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.empty-desc {
  max-width: 420px;
  margin: 0 auto 18px;
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-muted);
}

/* ---------- 新建压测弹窗 ---------- */
.modal-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-height: 76vh;
  overflow-y: auto;
  padding: 2px 8px 2px 2px;
}

.modal-body .label {
  margin-bottom: 5px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.modal-body .hint {
  margin-top: 6px;
  font-size: 11px;
}

.label-hint {
  margin-left: 8px;
  font-size: 10.5px;
  font-weight: 400;
  color: var(--fm-muted);
}

/* 弹窗内控件统一 32px 高度与 13px 字号，避免默认尺寸下各控件高度参差 */
.modal-body :deep(.ant-select-single .ant-select-selector) {
  min-height: 32px;
  padding: 1px 11px;
  font-size: 13px;
  align-items: center;
}

.modal-body :deep(.ant-input-number) {
  font-size: 13px;
}

.modal-body :deep(.ant-input) {
  padding: 4px 11px;
  font-size: 13px;
}

.modal-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.section-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
}

.section-sub {
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.4px;
  color: var(--fm-muted);
  text-transform: uppercase;
}

.step {
  width: 18px;
  height: 18px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.target-grid {
  display: grid;
  grid-template-columns: 3fr 2fr 5fr;
  gap: 20px;
  align-items: end;
}

/* 接口列的提示文字较长，允许换行不被截断 */
.target-grid .label {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 2px 8px;
  min-height: 19px;
  margin-bottom: 7px;
}

.target-grid .label-hint {
  margin-left: 0;
}

.plan-row {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}

.plan-row .plan-select {
  flex: 1 1 240px;
  min-width: 200px;
}

.plan-row .plan-name {
  flex: 1 1 180px;
  min-width: 160px;
}

.plan-section {
  padding-bottom: 12px;
  border-bottom: 1px dashed var(--fm-border);
}

.target-grid .field,
.modal-section > .field {
  min-width: 0;
}

/* 三个控件必须撑满各自的栅格列，否则会收缩成内容宽度，30/20/50 的比例看不出来 */
.target-grid .field :deep(.ant-select) {
  width: 100%;
}

:global(.stress-select-dropdown) {
  min-width: 260px;
}

:global(.stress-select-dropdown .ant-select-item) {
  min-height: 38px;
  padding: 9px 12px;
  font-size: 13px;
}

:global(.stress-select-dropdown .ant-select-item-option-content) {
  white-space: normal;
  word-break: break-all;
  line-height: 1.5;
}

/* 下拉里的接口条目：第一行 方法 + 名称，第二行 URL 小字 */
:global(.stress-req-dropdown) {
  min-width: 560px;
  max-width: 680px;
}

:global(.stress-req-dropdown .ant-select-item) {
  padding: 7px 12px;
}

:global(.stress-req-dropdown .ant-select-item-option-content) {
  padding: 0;
}

.req-option {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 2px 0;
  min-width: 0;
}

.req-option-main {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.method-mini {
  min-width: 44px;
  height: 18px;
  font-size: 9.5px;
  flex-shrink: 0;
}

.req-option-name {
  font-size: 13px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.req-option-url {
  font-size: 10.5px;
  line-height: 1.4;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 选中后的目标卡片：让「选了什么」一眼可见 */
.target-card {
  padding: 10px 12px;
  border: 1px solid color-mix(in srgb, var(--fm-primary) 32%, var(--fm-border));
  border-radius: var(--fm-radius);
  background: color-mix(in srgb, var(--fm-primary) 5%, var(--fm-surface-alt));
}

.target-card-head {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

.target-card-badge {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: var(--fm-radius-sm);
  background: color-mix(in srgb, var(--fm-primary) 14%, transparent);
  color: var(--fm-primary);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.4px;
}

.target-card-name {
  font-size: 13.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.target-card-collection {
  margin-left: auto;
  flex-shrink: 0;
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.target-card-url {
  margin-top: 7px;
  padding: 6px 9px;
  border-radius: var(--fm-radius);
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--fm-text-2);
  word-break: break-all;
}

.target-card-url.resolved {
  margin-top: 4px;
  border-style: dashed;
  color: var(--fm-primary);
}

/* 多接口：每个接口一行，权重就地配置 */
.target-item {
  padding: 8px 0;
  border-bottom: 1px dashed var(--fm-border);
}

.target-item:last-of-type {
  border-bottom: none;
}

.target-item-main {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

.target-item-index {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--fm-primary) 14%, transparent);
  color: var(--fm-primary);
  font-size: 10.5px;
  font-weight: 600;
  line-height: 18px;
  text-align: center;
}

.target-weight {
  margin-left: 9px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--fm-muted);
}

.target-weight :deep(.ant-input-number) {
  width: 76px;
}

.capacity-hint {
  color: var(--fm-muted);
}

.modal-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 参数分组卡片 */
.param-card {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}

.param-head {
  display: flex;
  align-items: baseline;
  gap: 9px;
  padding: 8px 13px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
}

.param-title {
  font-size: 12.5px;
  font-weight: 600;
}

.param-head-actions {
  margin-left: auto;
  align-self: center;
  display: flex;
  align-items: center;
  gap: 4px;
}

.param-sub {
  font-size: 9.5px;
  letter-spacing: 0.4px;
  color: var(--fm-muted);
  text-transform: uppercase;
}

.param-body {
  display: flex;
  flex-direction: column;
  gap: 13px;
  padding: 15px;
}

.check-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 24px;
}

.check-row :deep(.ant-checkbox-wrapper) {
  font-size: 12.5px;
}

.modal-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.modal-foot-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--fm-muted);
}

.modal-foot-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

@media (max-width: 720px) {
  .target-grid,
  .field-row,
  .sla-grid {
    grid-template-columns: 1fr;
  }
  .modal-foot {
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
  }
}

@media (max-width: 1080px) {
  .layout {
    grid-template-columns: 1fr;
  }
  .config {
    position: static;
  }
  .stats {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}

/* 顶部 Tab：压测 / 计划 / 报告 */
.stress-top-tabs {
  margin: -8px 0 14px;
}

.stress-top-tabs :deep(.ant-tabs-nav) {
  margin: 0;
}

.label-required {
  margin-left: 6px;
  font-size: 10px;
  font-weight: 400;
  color: var(--fm-danger);
}

.capture-row {
  margin-top: 10px;
}

.long-run-ack {
  margin-top: 2px;
  padding: 8px 10px;
  border: 1px dashed var(--fm-warning);
  border-radius: 4px;
  background: color-mix(in srgb, var(--fm-warning) 8%, transparent);
}

.long-run-card {
  border-color: var(--fm-warning);
}

.plan-row .plan-template {
  flex: 0 0 160px;
  min-width: 140px;
}
</style>
