<script setup lang="ts">
import { computed, h, nextTick, onActivated, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Modal, message as notify } from 'ant-design-vue'
import {
  BarChartOutlined,
  ClockCircleOutlined,
  CommentOutlined,
  CopyOutlined,
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  EyeOutlined,
  HistoryOutlined,
  LinkOutlined,
  PaperClipOutlined,
  PictureOutlined,
  PlusOutlined,
  PushpinFilled,
  PushpinOutlined,
  ReloadOutlined,
  SearchOutlined,
  SendOutlined,
  SettingOutlined,
  StopOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import AstroBuddyIcon from '@/components/AstroBuddyIcon.vue'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import PageHeader from '@/components/PageHeader.vue'
import AiConfirmCard from '@/components/AiConfirmCard.vue'
import {
  aiChat,
  aiChatCancel,
  aiConfirmAudit,
  aiContextMeta,
  aiMessageAdd,
  aiMessageClear,
  aiMessageDelete,
  aiMessageDeleteFrom,
  aiMessageList,
  aiModelsList,
  aiProviderDelete,
  aiProviderList,
  aiProviderSave,
  aiProviderTest,
  aiSessionCreate,
  aiSessionDelete,
  aiSessionList,
  aiSessionRename,
  aiSessionSummarySave,
  aiSessionPin,
  aiToolLogAdd,
  aiToolLogList,
  aiUsageClear,
  aiUsageLogs,
  aiUsageStats,
  fileExists,
  fileReadText,
  fileWriteText,
  requestRevisionRollbackTrace,
  settingsGet,
  testCaseGet,
  testDatasetGet,
  testPreflight,
  testRunGet,
  testSuiteGet,
  toolCancel,
} from '@/api'
import { captureFlowDetail } from '@/api/capture'
import { decideSend, decideWrite, isDangerousTool, isWriteTool, maskSensitive, maskedArgsForLog, nativeToolDefs, normalizeAiPermissionMode, previewSendRequest, testCaseKindLabel, executeTool, toolLabel, type ToolResult } from '@/utils/aiTools'
import { callKey, hasNativeToolSyntax, parseSegments, sanitizeAssistant, stripNativeTags, type Segment } from '@/utils/aiProtocol'
import {
  nativeRunPlan,
  normalizeNativeHistory,
  parseNativeArguments,
  parseNativeToolCallPayload,
  stripSyntheticToolBlocks,
} from '@/utils/aiNative'
import { buildContext, estimateMessages, estimateTokens, SUMMARY_PREFIX } from '@/utils/contextBudget'
import { estimateCost, fmtUnit, fmtUsd, priceOf, type PriceOverride } from '@/utils/aiPricing'
import { budgetState, monthStartMs, type BudgetState } from '@/utils/aiBudget'
import { contextWindowOf, effectiveWindow, inputBudgetOf, outputReserveOf } from '@/utils/modelMeta'
import { isTextAttachment, prepareAttachment } from '@/utils/attachBudget'
import {
  compressImageFile,
  estimateImageTokens,
  formatImageSize,
  MAX_IMAGES,
  supportedImageMime,
  type AiImagePayload,
  type PendingImage,
} from '@/utils/aiImages'
import { clipStoredPrompt, compactPrompt } from '@/utils/promptText'
import { extractCurlCommand } from '@/utils/curl'
import { wrapUntrusted } from '@/utils/untrusted'
import { renderMarkdown } from '@/utils/markdown'
import { useHangzaiTaskStore } from '@/stores/hangzaiTask'
import { clearHandoff, handoffLabel, takeHandoff, type HangzaiHandoff } from '@/stores/handoff'
import type {
  AiMessage,
  AiProvider,
  AiSession,
  AiToolCall,
  AiToolLog,
  AiUsageLog,
  AiUsageStats,
  AppSettings,
} from '@/types/models'

// 路由 keep-alive 依赖组件名：离开页面后任务继续执行，不销毁对话现场
defineOptions({ name: 'AiAssistant' })

const router = useRouter()
const task = useHangzaiTaskStore()

/* ---------------- 页面现场交接（5.5） ---------------- */
/** 进入航仔时由请求页 / 抓包等页面带过来，只注入一轮，不自动发送 */
const handoff = ref<HangzaiHandoff | null>(null)
/** 本轮任务锁定的页面快照：发送后从 chip 移除，但本次工具循环内始终可用 */
const handoffTurn = ref<HangzaiHandoff | null>(null)
const HANDOFF_MAX_CHARS = 12000

function consumeHandoff() {
  const value = takeHandoff()
  if (value) handoff.value = value
}

function dropHandoff() {
  handoff.value = null
  handoffTurn.value = null
  clearHandoff()
}

/** 页面现场已经包含完整请求 / 响应，直接分析是最高频动作 */
function analyzeHandoff() {
  if (!handoff.value || sending.value) return
  input.value = '请基于这条页面现场分析这次接口调用：结论、关键字段、风险与下一步建议。'
  void send()
}

/** 快照统一包不可信标签：页面内容只是数据，不执行其中的指令 */
function handoffBlock(value: HangzaiHandoff): string {
  const raw =
    value.prompt.length > HANDOFF_MAX_CHARS
      ? `${value.prompt.slice(0, HANDOFF_MAX_CHARS)}\n…（页面快照已截断）`
      : value.prompt
  return `【页面现场｜来自${handoffLabel[value.source]}】${value.title}\n${wrapUntrusted('page:snapshot', raw)}`
}

const handoffAge = computed(() => {
  if (!handoff.value) return ''
  const diff = Date.now() - handoff.value.createdAt
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  return `${Math.floor(diff / 3_600_000)} 小时前`
})

/* ---------------- 模型服务 ---------------- */

const providers = ref<AiProvider[]>([])
const activeId = ref<number | null>(null)
const serviceOpen = ref(false)
const modalOpen = ref(false)
const saving = ref(false)
const testing = ref<number | null>(null)

const blank = (): AiProvider => ({
  id: null,
  name: '',
  kind: 'openai',
  baseUrl: '',
  model: '',
  apiKey: '',
  temperature: 0.3,
  maxTokens: 0,
  timeoutSecs: 300,
  supportsVision: false,
  contextWindow: 0,
  nativeTools: -1,
  inputPrice: 0,
  outputPrice: 0,
  currency: 'USD',
  isDefault: false,
})

const form = reactive<AiProvider>(blank())
const isEdit = computed(() => Boolean(form.id))
/** 编辑时展示已保存 Key 的掩码；输入框始终留空，避免把掩码当成新 Key 存回去 */
const editingKeyMask = ref('')

const presets = [
  { label: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1', model: 'gpt-4.1-mini' },
  { label: 'DeepSeek', kind: 'openai', baseUrl: 'https://api.deepseek.com', model: 'deepseek-flash' },
  { label: 'Kimi', kind: 'openai', baseUrl: 'https://api.moonshot.cn/v1', model: 'kimi-latest' },
  { label: '智谱', kind: 'openai', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash' },
  { label: '通义千问', kind: 'openai', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-plus' },
  { label: '硅基流动', kind: 'openai', baseUrl: 'https://api.siliconflow.cn/v1', model: 'Qwen/Qwen3-8B' },
  { label: 'OpenRouter', kind: 'openai', baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o-mini' },
  { label: 'Ollama 本地', kind: 'openai', baseUrl: 'http://127.0.0.1:11434/v1', model: 'qwen2.5:7b' },
  { label: 'Claude', kind: 'anthropic', baseUrl: 'https://api.anthropic.com', model: 'claude-sonnet-4-5' },
  { label: 'Gemini', kind: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com', model: 'gemini-2.5-flash' },
]

/**
 * 各服务商真实存在的模型 ID（只列官方文档确认过的，避免出现厂商没有的版本名）。
 * 下拉选不到时仍可手动输入，服务商新增模型无需等程序更新。
 */
const presetModels: Record<string, string[]> = {
  OpenAI: [
    'gpt-4.1-mini',
    'gpt-4.1',
    'gpt-4.1-nano',
    'gpt-4o-mini',
    'gpt-4o',
    'o4-mini',
    'o3',
    'o3-mini',
  ],
  // DeepSeek
  DeepSeek: ['deepseek-flash','deepseek-v4-pro','deepseek-chat','deepseek-reasoner'],
  Kimi: [
    'kimi-latest',
    'kimi-k2-0711-preview',
    'moonshot-v1-8k',
    'moonshot-v1-32k',
    'moonshot-v1-128k',
  ],
  智谱: [
    'glm-4-flash',
    'glm-4-flashx',
    'glm-4-air',
    'glm-4-plus',
    'glm-4-long',
    'glm-4.5',
    'glm-4.5-air',
    'glm-z1-air',
  ],
  通义千问: [
    'qwen-plus',
    'qwen-turbo',
    'qwen-max',
    'qwen-long',
    'qwen3-max',
    'qwen3-235b-a22b',
    'qwen3-coder-plus',
  ],
  硅基流动: [
    'Qwen/Qwen3-8B',
    'Qwen/Qwen3-32B',
    'Qwen/Qwen2.5-72B-Instruct',
    'deepseek-ai/DeepSeek-V3',
    'deepseek-ai/DeepSeek-R1',
    'moonshotai/Kimi-K2-Instruct',
  ],
  OpenRouter: [
    'openai/gpt-4o-mini',
    'openai/gpt-4.1-mini',
    'anthropic/claude-sonnet-4.5',
    'google/gemini-2.5-flash',
    'deepseek/deepseek-chat',
    'deepseek/deepseek-r1',
  ],
  'Ollama 本地': ['qwen2.5:7b', 'qwen2.5:14b', 'llama3.1:8b', 'deepseek-r1:7b', 'gemma3:4b'],
  Claude: [
    'claude-sonnet-4-5',
    'claude-opus-4-1',
    'claude-haiku-4-5',
    'claude-3-7-sonnet-latest',
    'claude-3-5-haiku-latest',
  ],
  Gemini: [
    'gemini-2.5-flash',
    'gemini-2.5-pro',
    'gemini-2.5-flash-lite',
    'gemini-2.0-flash',
    'gemini-2.0-flash-lite',
  ],
}

/** Base URL 主机名 → 服务商，用于手动改地址后仍能给出模型候选 */
const hostHints: [string, string][] = [
  ['api.openai.com', 'OpenAI'],
  ['deepseek.com', 'DeepSeek'],
  ['moonshot.cn', 'Kimi'],
  ['bigmodel.cn', '智谱'],
  ['dashscope.aliyuncs.com', '通义千问'],
  ['siliconflow.cn', '硅基流动'],
  ['openrouter.ai', 'OpenRouter'],
  ['11434', 'Ollama 本地'],
  ['anthropic.com', 'Claude'],
  ['generativelanguage.googleapis.com', 'Gemini'],
]

/** 根据当前 Base URL 判断用户选的是哪家服务商，决定模型候选 */
const matchedPreset = computed(() => {
  const url = form.baseUrl.trim().toLowerCase()
  if (!url) return null
  const hint = hostHints.find(([host]) => url.includes(host))
  if (hint) return presets.find((item) => item.label === hint[1]) ?? null
  return presets.find((item) => item.baseUrl.toLowerCase() === url) ?? null
})

/** 版本号按数值比较：gemini-2.5 排在 gemini-2.10 前面，而不是按字符顺序 */
function naturalCompare(left: string, right: string): number {
  return left.localeCompare(right, 'en', { numeric: true, sensitivity: 'base' })
}

/** 服务商 /v1/models 拉取结果（按 provider 缓存 24 小时，失败回退内置清单） */
const remoteModels = ref<string[]>([])
const fetchingModels = ref(false)
const MODEL_CACHE_KEY = 'fbin.ai.modelCache.v1'
const MODEL_CACHE_TTL = 24 * 3600 * 1000

interface ModelCacheEntry {
  at: number
  models: string[]
}

function readModelCache(): Record<string, ModelCacheEntry> {
  try {
    const raw = localStorage.getItem(MODEL_CACHE_KEY)
    const parsed = raw ? (JSON.parse(raw) as Record<string, ModelCacheEntry>) : {}
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

function cachedModels(providerId: number | null | undefined): string[] {
  if (!providerId) return []
  const entry = readModelCache()[String(providerId)]
  if (!entry?.models?.length) return []
  if (Date.now() - entry.at > MODEL_CACHE_TTL) return []
  return entry.models
}

function rememberModels(providerId: number, models: string[]) {
  try {
    const cache = readModelCache()
    cache[String(providerId)] = { at: Date.now(), models }
    localStorage.setItem(MODEL_CACHE_KEY, JSON.stringify(cache))
  } catch {
    // 本地存储不可用时只影响缓存，不影响本次选择
  }
}

function forgetModels(providerId: number | null | undefined) {
  if (!providerId) return
  try {
    const cache = readModelCache()
    delete cache[String(providerId)]
    localStorage.setItem(MODEL_CACHE_KEY, JSON.stringify(cache))
  } catch {
    // 忽略
  }
}

/** 先保存服务（拿到 id 和当前 Base URL/Key），再拉取真实模型清单 */
async function fetchModelList() {
  if (!form.id) {
    notify.info('请先保存模型服务，再拉取该服务商的可用模型')
    return
  }
  fetchingModels.value = true
  try {
    const list = await aiModelsList(form.id)
    remoteModels.value = [...list].sort(naturalCompare)
    rememberModels(form.id, remoteModels.value)
    notify.success(`已拉取 ${remoteModels.value.length} 个模型，可直接在下拉中选择`)
  } catch (error) {
    notify.warning(`拉取模型失败，继续使用内置清单：${String(error)}`)
  } finally {
    fetchingModels.value = false
  }
}

const modelOptions = computed(() => {
  const label = matchedPreset.value?.label
  // 没识别出服务商时给全量内置模型，保证下拉永远有内容可选
  const preset = label ? presetModels[label] ?? [] : Object.values(presetModels).flat()
  const list = [...new Set([...remoteModels.value, ...preset])].sort(naturalCompare)
  return list.map((value) => ({
    value,
    label: `${value} · ${fmtNum(contextWindowOf(value))} 窗口`,
  }))
})

const modelHint = computed(() => {
  const preset = matchedPreset.value
  const remote = remoteModels.value.length
  const count = modelOptions.value.length
  if (remote) {
    return `已从服务商拉取 ${remote} 个模型（缓存 24 小时），下拉里标注了估算上下文窗口`
  }
  if (!preset) return `未识别服务商，已列出 ${count} 个内置模型，也可直接输入模型名`
  return `已列出 ${preset.label} 的 ${count} 个常用模型，也可直接输入其他模型名`
})

function filterModelOption(input: string, option: { value?: string | number }): boolean {
  if (!input) return true
  // 输入内容本身已是完整模型名（例如点了快速填充）时不过滤，方便继续挑别的模型
  if (modelOptions.value.some((item) => item.value === input)) return true
  return String(option.value ?? '').toLowerCase().includes(input.toLowerCase())
}

function applyPreset(label: string) {
  const preset = presets.find((item) => item.label === label)
  if (!preset) return
  form.kind = preset.kind
  form.baseUrl = preset.baseUrl
  form.model = preset.model
  if (!form.name) form.name = preset.label
}

function openCreate() {
  Object.assign(form, blank())
  editingKeyMask.value = ''
  remoteModels.value = []
  if (providers.value.length === 0) form.isDefault = true
  modalOpen.value = true
}

function openEdit(provider: AiProvider) {
  // 列表只返回掩码：输入框留空，保存时后端沿用原 Key
  Object.assign(form, {
    ...provider,
    apiKey: '',
    maxTokens: provider.maxTokens ?? 0,
    timeoutSecs: provider.timeoutSecs ?? 300,
    contextWindow: provider.contextWindow ?? 0,
    nativeTools: provider.nativeTools ?? -1,
    inputPrice: provider.inputPrice ?? 0,
    outputPrice: provider.outputPrice ?? 0,
    currency: provider.currency ?? 'USD',
  })
  editingKeyMask.value = provider.apiKeyMasked ?? ''
  remoteModels.value = cachedModels(provider.id)
  modalOpen.value = true
}

async function reloadProviders() {
  providers.value = await aiProviderList()
  if (!providers.value.some((item) => item.id === activeId.value)) {
    const preferred = providers.value.find((item) => item.isDefault) ?? providers.value[0]
    activeId.value = preferred?.id ?? null
  }
}

async function saveProvider() {
  if (!form.name.trim() || !form.baseUrl.trim() || !form.model.trim()) {
    notify.warning('名称、Base URL、模型都必须填写')
    return
  }
  if (!isEdit.value && !form.apiKey.trim()) {
    notify.warning('首次添加需要填写 API Key')
    return
  }
  if (isEdit.value && !form.apiKey.trim() && !form.hasKey) {
    notify.warning('该服务还没有保存过 Key，请先填写')
    return
  }
  saving.value = true
  try {
    const saved = await aiProviderSave({ ...form })
    // 配置可能改过 Base URL / Key，旧模型清单不再可信，下次编辑时重新拉取
    forgetModels(saved.id ?? form.id)
    await reloadProviders()
    if (saved.id) activeId.value = saved.id
    modalOpen.value = false
    notify.success('已保存')
  } catch (error) {
    notify.error(String(error))
  } finally {
    saving.value = false
  }
}

function removeProvider(provider: AiProvider) {
  if (!provider.id) return
  Modal.confirm({
    title: `删除「${provider.name}」？`,
    content: '密钥会一并从本机数据库删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await aiProviderDelete(provider.id as number)
      await reloadProviders()
      notify.success('已删除')
    },
  })
}

async function testProvider(provider: AiProvider) {
  if (!provider.id) return
  testing.value = provider.id
  try {
    const reply = await aiProviderTest(provider.id)
    notify.success(`${provider.name} 连通：${reply}`)
  } catch (error) {
    notify.error(String(error))
  } finally {
    testing.value = null
  }
}

const activeProvider = computed(
  () => providers.value.find((item) => item.id === activeId.value) ?? null,
)
const canChat = computed(() => Boolean(activeProvider.value?.id))
/**
 * 原生 function calling 是否可用：-1 自动（三种协议都支持，默认开启）、
 * 0 关闭、1 开启；一旦原生请求失败就整体退回文本协议。
 */
const nativeToolsEnabled = computed(() => {
  const flag = activeProvider.value?.nativeTools ?? -1
  return flag !== 0 && !nativeFallback.value
})
const providerOptions = computed(() =>
  providers.value
    .filter((item) => item.id)
    .map((item) => ({ label: `${item.name} · ${item.model}`, value: item.id as number })),
)

/* ---------------- 成本治理（AI-05 / 06 / 07） ---------------- */

/** 全局 AI 设置：权限模式、摘要模型、月度预算 */
const appSettings = ref<AppSettings | null>(null)
/** 本月已花费（美元）与最近一次刷新时间：预算判定用，60 秒内复用 */
const monthUsedUsd = ref(0)
let monthUsedAt = 0

async function reloadAppSettings() {
  try {
    appSettings.value = await settingsGet()
  } catch {
    appSettings.value = null
  }
}

/** 服务商自定义单价：0 / 空时 aiPricing 会自动回落到内置价格表 */
function priceOverrideOf(provider: AiProvider | null | undefined): PriceOverride | null {
  if (!provider) return null
  return {
    inputPrice: provider.inputPrice ?? 0,
    outputPrice: provider.outputPrice ?? 0,
    currency: provider.currency ?? 'USD',
  }
}

/** 摘要专用模型；未配置或已被删除时沿用主 provider（AI-07） */
const summaryProvider = computed(() => {
  const id = appSettings.value?.summaryProviderId
  if (!id) return null
  return providers.value.find((item) => item.id === id) ?? null
})

const budgetInfo = computed<BudgetState>(() =>
  budgetState(monthUsedUsd.value, appSettings.value ?? {}),
)

/** 本月用量按自然月统计；读取失败不阻断对话，只按 0 处理 */
async function refreshMonthUsage(force = false) {
  if (!(Number(appSettings.value?.monthlyBudget) > 0)) {
    monthUsedUsd.value = 0
    monthUsedAt = Date.now()
    return
  }
  if (!force && Date.now() - monthUsedAt < 60_000) return
  try {
    const stats = await aiUsageStats(31, monthStartMs())
    let sum = 0
    for (const item of stats.items) {
      const provider =
        (item.providerId ? providers.value.find((p) => p.id === item.providerId) : undefined) ??
        providers.value.find((p) => p.name === item.providerName)
      sum += estimateCost(
        item.model,
        item.promptTokens,
        item.completionTokens,
        priceOverrideOf(provider),
      ).total
    }
    monthUsedUsd.value = sum
    monthUsedAt = Date.now()
  } catch {
    // 预算统计失败不阻断对话
  }
}

/** 80% / 超支提醒每月只弹一次，避免每轮都打扰 */
function alertBudgetOnce(state: BudgetState) {
  const month = new Date().toISOString().slice(0, 7)
  const scope = state.ratio >= 1 ? 'spent' : '80'
  const key = 'astroman:ai-budget-alert'
  const stamp = `${month}:${state.budget}:${scope}`
  if (localStorage.getItem(key) === stamp) return
  localStorage.setItem(key, stamp)
  const pct = Math.round(state.ratio * 100)
  if (state.ratio >= 1) {
    notify.warning(
      `本月 AI 预算已用尽：${fmtUsd(state.used)} / ${fmtUsd(state.budget)}（${pct}%），可在设置里调整预算或开启「用尽停止」`,
    )
  } else {
    notify.warning(
      `本月 AI 用量已达预算的 ${pct}%（${fmtUsd(state.used)} / ${fmtUsd(state.budget)}），请注意控制`,
    )
  }
}

/**
 * 发送前的预算门：未设预算直接放行；开启 `budgetHardStop` 且用尽时阻断。
 * 只有新提问才检查，工具循环中的续轮不重复拦截，避免任务半途而废。
 */
async function budgetGate(): Promise<boolean> {
  if (!(Number(appSettings.value?.monthlyBudget) > 0)) return true
  await refreshMonthUsage(true)
  const state = budgetInfo.value
  if (state.level === 'blocked') {
    notify.error(
      `本月 AI 预算已用尽（${fmtUsd(state.used)} / ${fmtUsd(state.budget)}），已按设置停止发送；可在「设置 → AI 助手」调整预算或关闭「用尽停止」`,
    )
    return false
  }
  if (state.level === 'warn') alertBudgetOnce(state)
  return true
}

/* ---------------- 会话 ---------------- */

const sessions = ref<AiSession[]>([])
const sessionId = ref<number | null>(null)
const messages = ref<ChatItem[]>([])
/** 只渲染最终回答：中间那段「先查项目 → 再查请求」的执行过程全部隐藏 */
const visibleMessages = computed(() => messages.value.filter((item) => !item.hidden))
const input = ref('')
/** 输入框上方的引用条：发送时随提问一起写给模型 */
const quote = ref<ChatQuote | null>(null)
const inputRef = ref<{ focus?: () => void } | null>(null)
const sending = ref(false)
/** 本轮在途请求 ID：停止生成时用它通知后端取消 */
const requestId = ref<string | null>(null)
/** 本轮任务的链路 ID：工具流水用它串起「一次提问 → 多次工具」 */
const traceId = ref<string | null>(null)
/** 用户点了停止：后续增量、工具结果一律丢弃 */
const aborted = ref(false)
/** 正在异步压缩历史会话 */
const summarizing = ref(false)
/** 摘要连续失败次数，达到 3 次暂停自动摘要 */
const summaryFails = ref(0)
const summaryPaused = ref(false)
/** 当前会话已落库的摘要（下一轮起替换被压缩的原文） */
const sessionSummary = ref<{ text: string; until: number } | null>(null)
/** 本轮组装后因预算被省略的条数 / 估算 token，用于顶部预算提示 */
const contextDropped = ref(0)
const contextTokens = ref(0)
/** 本轮用量估算：模型没有返回 usage 时用字符估算，只做量级参考 */
const turnPromptTokens = ref(0)
const turnOutputTokens = ref(0)
const turnCost = ref(0)
/** 本次请求实际切换到的备用模型（AI-12）；为空表示仍用当前主模型 */
const turnProvider = ref<AiProvider | null>(null)
const autoRun = ref(true)
const autoDepth = ref(0)
/** 与系统提示词保持一致：连续工具轮次上限 8（压测计划这类任务需要多步查询），超过就停下来汇总 */
const MAX_TOOL_ROUNDS = 8
/** 连续「一轮多个工具」的次数，模型屡劝不改时提前终止，避免用户卡死 */
const rejections = ref(0)
/** 输出原生工具标记（系统无法识别）的次数，最多自动纠正 2 次 */
const repairs = ref(0)
/** 同一工具 + 同一参数的重复调用次数，模型原地打转时强制收尾 */
const repeats = ref(0)
/** 已执行工具的调用结果缓存，重复调用直接复用，不再重复跑 */
const callCache = new Map<string, ToolResult>()
/** 结果未知的写工具：超时后后台可能已完成，禁止用相同参数自动重试 */
const uncertainWrites = new Set<string>()
/** 轮次将尽 / 反复重复后的「禁止再调工具」阶段 */
const forcedFinal = ref(false)
/** 原生 function calling 失败过一次：本次会话退回文本 fbin-tool 协议 */
const nativeFallback = ref(false)
const scrollRef = ref<HTMLElement | null>(null)

const renameOpen = ref(false)
const renameTarget = ref<AiSession | null>(null)
const renameText = ref('')

interface ChatItem {
  id: number
  role: 'user' | 'assistant' | 'tool'
  content: string
  /** 原生 function calling：助手这一轮要求执行的工具（AI-04） */
  toolCalls?: AiToolCall[]
  /** 原生工具结果对应的 tool_call_id */
  toolCallId?: string
  /** 原生工具结果对应的工具名 */
  toolName?: string
  /** 数据库消息 id：消息级删除用；本地尚未落库时为空 */
  dbId?: number
  /** 发送时引用的上一条消息，落库时以【引用…】标记写进正文 */
  quote?: ChatQuote
  /** 模型的思考过程（reasoning），默认折叠不展示 */
  reasoning?: string
  streaming?: boolean
  failed?: boolean
  /** 用户主动停止 */
  stopped?: boolean
  /** 流式期间的临时状态（自动重试提示等） */
  statusText?: string
  /** 纯执行过程（调工具前的「我先获取…」）不展示，只留最终回答 */
  hidden?: boolean
  createdAt: number
}

interface ChatQuote {
  role: 'user' | 'assistant'
  content: string
}

interface ChatAttachment {
  name: string
  path: string
  content: string
}

let localSeq = 0
const nextId = () => {
  localSeq += 1
  return Date.now() * 1000 + localSeq
}

/* ---------------- 回复分段 ---------------- */

/** 工具段（保留原始下标，供 runState 使用） */
function toolParts(item: ChatItem) {
  return parseSegments(item.content, item.role === 'assistant')
    .map((seg, index) => ({ seg, index }))
    .filter(({ seg }) => seg.type === 'tool')
}

/** 正文段（文本 + 普通代码块；附件块只喂给模型，不在气泡里铺开） */
function contentParts(item: ChatItem): Segment[] {
  return parseSegments(item.content, item.role === 'assistant').filter(
    (seg) => seg.type !== 'tool' && !(seg.type === 'code' && seg.lang.startsWith('fbin-file')),
  )
}

function processTitle(item: ChatItem): string {
  const count = toolParts(item).length
  const head = item.reasoning ? '思考过程' : '执行过程'
  return count ? `${head} · ${count} 个工具调用` : head
}

interface ToolRun {
  tool: string
  args: Record<string, unknown>
  status: 'idle' | 'running' | 'ok' | 'error'
  output: string
  durationMs: number
  /** 一句话摘要：卡片头部先看这条，全文收进详情 */
  summary?: string
  total?: number
  truncated?: boolean
}

const toolRuns = reactive<Record<string, ToolRun>>({})
const expandedTool = ref<string | null>(null)
const runKey = (itemId: number, index: number) => `${itemId}:${index}`
const runState = (itemId: number, index: number) => toolRuns[runKey(itemId, index)]

function toggleTool(itemId: number, index: number) {
  const key = runKey(itemId, index)
  expandedTool.value = expandedTool.value === key ? null : key
}

function resetRuns() {
  for (const key of Object.keys(toolRuns)) delete toolRuns[key]
}

function text(value: unknown, fallback = ''): string {
  if (value === null || value === undefined) return fallback
  return typeof value === 'string' ? value : String(value)
}

function toolSummary(seg: Segment): string {
  if (seg.tool === 'send_request' || seg.tool === 'request_create') {
    return `${text(seg.args.method, 'GET').toUpperCase()} ${text(seg.args.url)}`.trim().slice(0, 90)
  }
  const pairs = Object.entries(seg.args)
    .slice(0, 2)
    .map(([key, value]) => `${key}=${value && typeof value === 'object' ? JSON.stringify(value) : text(value)}`)
  return pairs.join(' · ').slice(0, 90)
}

function prettyArgs(args: Record<string, unknown>): string {
  try {
    return JSON.stringify(args, null, 2)
  } catch {
    return String(args)
  }
}

/* ---------------- 对话主流程 ---------------- */

// 身份、核心流程与工具协议拆到 @/utils/aiPrompt，评测运行器复用同一份提示词
import { AGENT_SYSTEM_PROMPT as SYSTEM_PROMPT } from '@/utils/aiPrompt'

/* ---------------- 上下文预算（F-5） ---------------- */

/** 后端注入的航仔提示词规模：进页面取一次，失败退回 0 */
const contextMeta = reactive({ systemPromptChars: 0, defaultMaxTokens: 4096 })

async function loadContextMeta() {
  try {
    Object.assign(contextMeta, await aiContextMeta())
  } catch {
    // 取不到就按 0 处理：前端仍会预留自身提示词与输出额度
  }
}

/** 当前模型实际窗口：用户显式配置优先，否则按模型名识别 */
const modelWindow = computed(() =>
  effectiveWindow(activeProvider.value?.model ?? '', activeProvider.value?.contextWindow),
)
/** 输入侧预算：窗口先扣掉输出预留，系统提示词再从这里面扣 */
const contextBudget = computed(() =>
  inputBudgetOf(modelWindow.value, activeProvider.value?.maxTokens),
)
/** 编辑弹窗里按当前输入预估窗口，给「0 = 自动识别」做参考 */
const formWindow = computed(() => effectiveWindow(form.model, form.contextWindow))
/** 后端提示词 token 保守估算（按字符 × 0.7） */
const backendPromptTokens = computed(() => Math.ceil(contextMeta.systemPromptChars * 0.7))
const outputReserve = computed(() =>
  outputReserveOf(modelWindow.value, activeProvider.value?.maxTokens),
)
const modelPrice = computed(() =>
  priceOf(activeProvider.value?.model ?? '', priceOverrideOf(activeProvider.value)),
)
const roundLabel = computed(() => `${Math.min(autoDepth.value, MAX_TOOL_ROUNDS)}/${MAX_TOOL_ROUNDS}`)
/** 上下文已用 85% 就变色提醒：再聊几轮会触发压缩或丢较早消息 */
const contextTight = computed(
  () => contextBudget.value > 0 && contextTokens.value / contextBudget.value >= 0.85,
)
/** 系统提示词 + 输出预留已接近整个窗口：再怎么裁剪也放不下多少历史 */
const contextWindowTiny = computed(() => {
  const systemReserve = estimateTokens(SYSTEM_PROMPT) + backendPromptTokens.value
  return modelWindow.value > 0 && systemReserve + outputReserve.value >= modelWindow.value * 0.9
})

/** 工具结果 / 附件正文进入模型前统一包上不可信数据标签（T-17） */
function hardenContent(content: string): string {
  return content.replace(
    /```fbin-file ([^\n]+)\n([\s\S]*?)\n```/g,
    (_match, name: string, body: string) =>
      `\`\`\`fbin-file ${name}\n${wrapUntrusted(`file:${name}`, body)}\n\`\`\``,
  )
}

/** 组装进模型的历史：跳过已被摘要覆盖的原文 */
function historyItems(): AiMessage[] {
  const list: AiMessage[] = []
  for (const item of messages.value) {
    const content = item.content.trim()
    if (!content) continue
    if (sessionSummary.value && item.createdAt <= sessionSummary.value.until) continue
    if (item.role === 'tool') {
      list.push({
        role: 'tool',
        content: hardenContent(content),
        toolCallId: item.toolCallId,
        name: item.toolName,
      })
      continue
    }
    // 引用内容随用户消息一起进上下文，模型才知道「这条回复」指哪一段
    const text = item.quote ? `${quotePrefix(item.quote)}${content}` : content
    if (item.role === 'assistant' && item.toolCalls?.length) {
      list.push({
        role: 'assistant',
        content: hardenContent(stripSyntheticToolBlocks(text)),
        toolCalls: item.toolCalls,
      })
      continue
    }
    list.push({ role: item.role, content: hardenContent(text) })
  }
  return list
}

/**
 * system → 会话摘要 → 预算内的最近原文。
 * 超出预算时硬裁剪并显式告诉模型「省略了 N 条」，避免它假装记得。
 */
function toHistory(): AiMessage[] {
  const budget = contextBudget.value
  const summaryText = sessionSummary.value?.text?.trim()
  const handoffText = handoffTurn.value ? handoffBlock(handoffTurn.value) : ''
  // 系统提示词、摘要与页面现场固定占用，先扣掉再给历史分配
  const reserved =
    estimateTokens(SYSTEM_PROMPT) +
    backendPromptTokens.value +
    (summaryText ? estimateTokens(summaryText) + 8 : 0) +
    (handoffText ? estimateTokens(handoffText) + 8 : 0) +
    240
  const plan = buildContext(historyItems(), Math.max(2000, budget - reserved))
  contextDropped.value = plan.dropped
  contextTokens.value = plan.tokens + reserved
  const head: AiMessage[] = [{ role: 'system', content: SYSTEM_PROMPT }]
  if (summaryText) head.push({ role: 'system', content: `${SUMMARY_PREFIX}${summaryText}` })
  if (handoffText) head.push({ role: 'system', content: handoffText })
  if (plan.dropped > 0) {
    head.push({
      role: 'system',
      content: `【上下文提示】较早的 ${plan.dropped} 条对话因超出模型上下文预算已省略；缺少信息时先向用户确认，不要编造。`,
    })
  }
  return [...head, ...normalizeNativeHistory(plan.messages)]
}

function scrollBottom(force = false) {
  const el = scrollRef.value
  if (!el) return
  const near = el.scrollHeight - el.scrollTop - el.clientHeight < 140
  if (force || near) el.scrollTop = el.scrollHeight
}

function newRequestId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `req-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

async function ask(extra: AiMessage[] = []) {
  if (!activeProvider.value?.id || aborted.value) return
  // 工具结果 / 纠正指令必须进入本地历史：否则模型下一轮看不到执行结果，会反复调用同一个工具
  for (const msg of extra) {
    if (msg.role !== 'user' && msg.role !== 'assistant' && msg.role !== 'tool') continue
    messages.value.push({
      id: nextId(),
      role: msg.role,
      content: msg.content,
      toolCallId: msg.toolCallId,
      toolName: msg.name,
      createdAt: Date.now(),
      hidden: true,
    })
    if (sessionId.value) {
      await aiMessageAdd(sessionId.value, msg.role, msg.content).catch(() => undefined)
    }
  }
  sending.value = true
  // 必须是响应式对象：直接改局部原始对象不会触发渲染，会变成「等全部生成完才显示」
  const item = reactive<ChatItem>({
    id: nextId(),
    role: 'assistant',
    content: '',
    streaming: true,
    createdAt: Date.now(),
  })
  messages.value.push(item)
  scrollBottom(true)

  const rid = newRequestId()
  requestId.value = rid
  const history = toHistory()
  // 原生 function calling：轮次将尽 / 已强制收尾时不再下发工具，让模型直接给结论
  const useNative =
    nativeToolsEnabled.value && !forcedFinal.value && autoDepth.value < MAX_TOOL_ROUNDS
  const nativeCalls: AiToolCall[] = []
  // 图片只进发送后的首轮：工具循环的后续轮次立即置空，避免每轮重复占用预算
  const imagesForTurn = turnImages.value
  turnImages.value = []
  turnPromptTokens.value = estimateMessages(history) + estimateImageTokens(imagesForTurn.length)
  turnOutputTokens.value = 0
  turnCost.value = 0
  turnProvider.value = null
  /** 首个 delta 到达后就不再静默重试，避免用户看到半句话 + 完整重来 */
  let committed = false

  // 流式增量先攒 50ms 再刷一次，既保持边生成边显示，又避免每个 token 都重排 Markdown
  let buffer = ''
  let reasoningBuffer = ''
  let lastPaint = 0
  const paint = (force = false) => {
    const now = performance.now()
    if (!force && now - lastPaint < 50) return
    if (buffer) {
      item.content += buffer
      buffer = ''
    }
    if (reasoningBuffer) {
      item.reasoning = (item.reasoning ?? '') + reasoningBuffer
      reasoningBuffer = ''
    }
    lastPaint = now
    scrollBottom()
  }

  try {
    await aiChat(
      {
        providerId: activeProvider.value.id,
        messages: history,
        temperature: activeProvider.value.temperature ?? 0.3,
        requestId: rid,
        scene: 'agent',
        contextWindow: modelWindow.value,
        images: imagesForTurn.length ? imagesForTurn : undefined,
        tools: useNative ? nativeToolDefs() : undefined,
      },
      (event) => {
        // 用户已停止：后续增量、重试提示一律丢弃
        if (aborted.value) return
        if (event.kind === 'delta' && event.text) {
          committed = true
          item.statusText = ''
          buffer += event.text
          if (task.active) task.update({ phase: 'streaming', canStop: true })
          paint()
        } else if (event.kind === 'reasoning' && event.text) {
          reasoningBuffer += event.text
          paint()
        } else if (event.kind === 'tool_call') {
          const call = parseNativeToolCallPayload(event.text)
          if (call) {
            committed = true
            nativeCalls.push(call)
          }
        } else if (event.kind === 'retry') {
          item.statusText = event.text ?? '正在自动重试…'
          scrollBottom()
        } else if (event.kind === 'fallback') {
          // 后端已切到备用模型：本轮费用按实际模型估算，并明确告知用户
          try {
            const info = JSON.parse(event.text || '{}') as {
              providerId?: number
              name?: string
              model?: string
            }
            const known = providers.value.find((provider) => provider.id === info.providerId)
            turnProvider.value =
              known ??
              ({
                id: info.providerId,
                name: info.name ?? '备用模型',
                model: info.model ?? '',
              } as AiProvider)
            notify.info(`主模型不可用，已切换到 ${info.name ?? '备用模型'}${info.model ? ` · ${info.model}` : ''}`)
          } catch {
            notify.info('主模型不可用，已自动切换到备用模型')
          }
          scrollBottom()
        } else if (event.kind === 'cancelled') {
          item.stopped = true
        } else if (event.kind === 'error') {
          paint(true)
          item.statusText = ''
          const tail = committed ? '（已输出部分内容，重试会清空重来）' : ''
          item.content += `${item.content ? '\n\n' : ''}> ${event.text}${tail}`
          item.failed = true
        }
      },
    )
  } catch (error) {
    paint(true)
    item.statusText = ''
    if (!item.content) item.content = String(error)
    item.failed = true
  } finally {
    paint(true)
    item.streaming = false
    item.statusText = ''
    sending.value = false
    requestId.value = null
    turnOutputTokens.value = estimateTokens(item.content)
    const costProvider = turnProvider.value ?? activeProvider.value
    turnCost.value = estimateCost(
      costProvider?.model ?? '',
      turnPromptTokens.value,
      turnOutputTokens.value,
      priceOverrideOf(costProvider ?? undefined),
    ).total
    if (aborted.value) item.stopped = true
    scrollBottom()
  }

  // 原生模式在「没有任何输出」时失败：网关 / 模型不支持 tools，退回文本协议重问一次
  if (useNative && !committed && !aborted.value && item.failed) {
    messages.value = messages.value.filter((msg) => msg.id !== item.id)
    nativeFallback.value = true
    notify.info('当前模型不支持原生工具调用，已自动回退到文本工具协议')
    return ask()
  }

  // 原生工具调用也要有可见卡片：沿用 fbin-tool 块，界面与文本协议一致
  if (nativeCalls.length) {
    item.toolCalls = nativeCalls
    for (const call of nativeCalls) {
      const args = parseNativeArguments(call.arguments) ?? {}
      item.content += `\n\n\`\`\`fbin-tool\n${JSON.stringify({ tool: call.name, args })}\n\`\`\`\n`
    }
  }

  if (item.content.trim() && sessionId.value) {
    // 硬规则 7：思考内容随助手消息一起落库，便于回溯 Agent 行为
    const stored = await aiMessageAdd(
      sessionId.value,
      'assistant',
      item.content,
      item.reasoning,
    ).catch(() => undefined)
    if (stored?.id) item.dbId = stored.id
    void refreshSessions()
  }

  // 停止 = 中止模型流 + 丢弃后续工具结果，不再自动调工具
  if (aborted.value) return
  if (item.toolCalls?.length) {
    await afterNativeReply(item)
    return
  }
  await afterReply(item)
}

/** 停止生成：通知后端取消，本级立即标记停止，不再回灌工具结果 */
async function stop() {
  if (!sending.value) return
  aborted.value = true
  if (task.active) task.update({ phase: 'thinking', canStop: false, awaitingConfirm: false })
  const rid = requestId.value
  if (rid) {
    try {
      await aiChatCancel(rid)
    } catch {
      // 请求可能已经自然结束，取消失败不影响本地停止
    }
  }
  // AI-02：按 traceId 中断在途工具请求（send_request / run_batch 正在打的 HTTP）
  const trace = traceId.value
  if (trace) {
    try {
      await toolCancel(trace)
    } catch {
      // 工具可能已经结束或没有可取消的请求
    }
  }
  notify.info('已停止，本轮工具结果已丢弃')
}

/**
 * 重新生成：删掉这条回复（含它之后的分支），复用原历史再问一次。
 * 不重复落库用户消息；已被用户停止 / 失败、或想换个答案时都能用。
 */
async function regenerate(item: ChatItem) {
  if (sending.value || item.role !== 'assistant') return
  const index = messages.value.findIndex((msg) => msg.id === item.id)
  if (index < 0) return
  if (sessionId.value) {
    // 先清库再截断本地，避免刷新后旧分支又出现
    await aiMessageDeleteFrom(sessionId.value, item.createdAt).catch(() => undefined)
  }
  messages.value.splice(index)
  aborted.value = false
  autoDepth.value = 0
  rejections.value = 0
  repairs.value = 0
  repeats.value = 0
  forcedFinal.value = false
  callCache.clear()
  await ask()
}

/** 把引用内容打包进用户消息：既让模型看到，也能落库后重新解析 */
const QUOTE_LIMIT = 400
const quoteLabel = (role: ChatQuote['role']) => (role === 'user' ? '你' : '航仔')

function clipQuote(text: string): string {
  const flat = text.trim()
  return flat.length > QUOTE_LIMIT ? `${flat.slice(0, QUOTE_LIMIT)}…` : flat
}

function quotePrefix(item: ChatQuote): string {
  return `【引用${quoteLabel(item.role)}】\n${clipQuote(item.content)}\n\n`
}

/** 从落库正文里还原引用与真正的提问内容 */
function parseStoredQuote(content: string): { body: string; quote?: ChatQuote } {
  const match = content.match(/^【引用(你|航仔|小HI)】\n([\s\S]*?)(?:\n\n|$)/)
  if (!match) return { body: content }
  return {
    body: content.slice(match[0].length),
    quote: { role: match[1] === '你' ? 'user' : 'assistant', content: match[2] },
  }
}

/** 引用时只取用户能看到的正文：附件正文与工具块不算回答内容 */
function visibleText(content: string): string {
  return content
    .replace(/```fbin-file [^\n]*\n[\s\S]*?\n```/g, '')
    .replace(/```fbin-tool[\s\S]*?```/g, '')
    .trim()
}

/** 引用某条消息：内容进输入框上方的引用条，发送时随提问一起写入上下文 */
function quoteMessage(item: ChatItem) {
  const text = visibleText(item.content)
  if (sending.value || !text || item.role === 'tool') return
  quote.value = { role: item.role, content: clipQuote(text) }
  nextTick(() => inputRef.value?.focus?.())
}

function clearQuote() {
  quote.value = null
  nextTick(() => inputRef.value?.focus?.())
}

/** 删除单条消息：先删库再删本地，删掉的消息不再进入后续上下文 */
function removeMessage(item: ChatItem) {
  if (sending.value || item.streaming) return
  Modal.confirm({
    title: '删除这条消息？',
    content:
      item.role === 'user'
        ? '删除后这条提问不再显示，之后的对话也不会再带上它。'
        : '删除后这条回复不再显示，也不会进入后续上下文。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      if (item.dbId && sessionId.value) {
        try {
          await aiMessageDelete(item.dbId)
        } catch (error) {
          notify.error(`删除失败：${String(error)}`)
          return
        }
      }
      const index = messages.value.findIndex((msg) => msg.id === item.id)
      if (index >= 0) messages.value.splice(index, 1)
      if (quote.value?.content === item.content) quote.value = null
      notify.success('已删除')
    },
  })
}

/** 复制消息：优先用剪贴板 API，桌面端受限时退回 textarea + execCommand */
async function copyMessage(item: ChatItem) {
  const text = item.content.trim()
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    notify.success('已复制')
    return
  } catch {
    // 继续走兜底方案
  }
  try {
    const area = document.createElement('textarea')
    area.value = text
    area.style.position = 'fixed'
    area.style.opacity = '0'
    document.body.appendChild(area)
    area.select()
    const ok = document.execCommand('copy')
    document.body.removeChild(area)
    if (ok) notify.success('已复制')
    else notify.warning('当前环境不支持自动复制，请手动选择文本')
  } catch {
    notify.warning('当前环境不支持自动复制，请手动选择文本')
  }
}

/** 编辑并重发用户消息：截断该条及其后的对话，内容放回输入框 */
async function editMessage(item: ChatItem) {
  if (sending.value || item.role !== 'user') return
  const index = messages.value.findIndex((msg) => msg.id === item.id)
  if (index < 0) return
  const content = item.content
  if (sessionId.value) {
    try {
      // 先删库再截断本地，避免刷新后旧分支又出现
      await aiMessageDeleteFrom(sessionId.value, item.createdAt)
    } catch (error) {
      notify.error(`编辑失败：${String(error)}`)
      return
    }
  }
  messages.value.splice(index)
  input.value = content
  quote.value = item.quote ?? null
  aborted.value = false
  autoDepth.value = 0
  forcedFinal.value = false
  callCache.clear()
  notify.info('已把这条消息放回输入框，修改后可直接发送')
}

/**
 * 历史消息里只含一个工具块 + 少量过程说明（如「我先获取项目列表」）的，视为执行过程。
 * 最终回答通常不含 fbin-tool 块，因此重开会话时也能保持「只看结果」。
 */
function isExecutionNote(content: string): boolean {
  const normalized = sanitizeAssistant(content)
  if (!normalized.includes('```fbin-tool')) return false
  const rest = normalized.replace(/```fbin-tool[\s\S]*?```/g, '').trim()
  return rest.length < 200
}

/** 工具结果 / 纠正指令这类内部回灌消息，重开会话时不展示，但会进入上下文 */
const CONTROL_PREFIXES = ['【工具 ', '【格式纠正】', '【重复调用', '【轮次用尽】']

function isControlMessage(content: string): boolean {
  const head = content.trimStart()
  return CONTROL_PREFIXES.some((prefix) => head.startsWith(prefix))
}

/** 历史里只剩原生工具标记残渣（没能还原成工具块）的消息，同样按执行过程隐藏 */
function isToolLeakNote(content: string): boolean {
  if (!hasNativeToolSyntax(content)) return false
  return stripNativeTags(content).length < 200
}

function registerRuns(item: ChatItem) {
  const found: { seg: Segment; index: number }[] = []
  parseSegments(item.content).forEach((seg, index) => {
    if (seg.type !== 'tool') return
    const key = runKey(item.id, index)
    if (!toolRuns[key]) {
      toolRuns[key] = { tool: seg.tool, args: seg.args, status: 'idle', output: '', durationMs: 0 }
    }
    found.push({ seg, index })
  })
  return found
}

/** 硬规则 1：达到轮次上限直接终止，并把提示返回给用户 */
function notifyTerminated() {
  const text = '已达到最大执行轮次，本次任务终止，请调整你的需求重试'
  notify.warning(text)
  messages.value.push({
    id: nextId(),
    role: 'assistant',
    content: text,
    createdAt: Date.now(),
    failed: true,
  })
  scrollBottom(true)
}

async function afterReply(item: ChatItem) {
  // 用户已停止：本轮工具结果一律不回灌、不继续自动调工具
  if (aborted.value) return
  const tools = registerRuns(item)

  // 硬规则 3：模型输出了系统无法识别的原生工具标记 → 拦截乱码并自动纠正
  if (!tools.length && hasNativeToolSyntax(item.content)) {
    if (repairs.value >= 2) {
      item.content = '这次输出的工具调用格式异常，已中断。请把需求再描述一次，或换一个模型重试。'
      item.failed = true
      scrollBottom(true)
      return
    }
    repairs.value += 1
    item.hidden = true
    item.content = '（上一条回复使用了无法识别的工具调用格式，已拦截）'
    await ask([
      {
        role: 'user',
        content:
          '【格式纠正】你刚才使用了系统无法识别的工具调用标记，内容已被丢弃。如果需要调用工具，请只输出一个 ```fbin-tool 代码块（严格 JSON）；如果不需要工具，请直接用自然语言回答。',
      },
    ])
    return
  }

  if (!tools.length || !autoRun.value || sending.value) return

  // 硬规则 2：一次输出多个 tool_call → 全部拒绝，把错误回灌给模型
  if (tools.length > 1) {
    autoDepth.value += 1
    rejections.value += 1
    // 模型连续两次不听劝就提前终止，避免用户卡在「无法回答」空转
    if (autoDepth.value > MAX_TOOL_ROUNDS || rejections.value >= 2) {
      notifyTerminated()
      return
    }
    await ask([
      {
        role: 'user',
        content: '单次仅允许调用一个工具，请重新选择工具与参数。',
      },
    ])
    return
  }
  rejections.value = 0

  const { seg, index } = tools[0]
  if (runState(item.id, index)?.status !== 'idle') return

  // 硬规则 1：轮次用尽 → 先要求直接给结论，再不行才终止
  if (autoDepth.value >= MAX_TOOL_ROUNDS || forcedFinal.value) {
    if (!forcedFinal.value) {
      forcedFinal.value = true
      item.hidden = true
      await ask([
        {
          role: 'user',
          content:
            '【轮次用尽】禁止再调用任何工具。请立刻用中文输出：已完成的步骤、已知的关键信息、最终方案或结论；若确实还缺信息，明确说明缺什么。',
        },
      ])
      return
    }
    notifyTerminated()
    return
  }

  // 硬规则 5：同一工具 + 同一参数反复调用 → 复用缓存结果，两次后强制收尾
  const cacheKey = callKey(seg.tool, seg.args)
  const cached = callCache.get(cacheKey)
  if (cached) {
    repeats.value += 1
    item.hidden = true
    if (repeats.value >= 2) {
      forcedFinal.value = true
      await ask([
        {
          role: 'user',
          content: `【重复调用拦截】${seg.tool} 用相同参数已经执行过多次，结果如下：\n${wrapUntrusted(`tool:${seg.tool}`, cached.output)}\n\n禁止再调用任何工具，立刻用中文输出最终方案或结论。`,
        },
      ])
      return
    }
    await ask([
      {
        role: 'user',
        content: `【重复调用】${seg.tool} 用相同参数已经执行过，结果在上文，不要重复调用。请直接推进：改用不同工具或参数，或者立刻输出最终回答。`,
      },
    ])
    return
  }

  autoDepth.value += 1

  // 硬规则 4：写入 / 触发执行的工具不直接跑；发送请求按 AI 权限模式判定
  if (isWriteTool(seg.tool) || seg.tool === 'send_request') {
    await runToolSegment(item, index, seg)
    return
  }

  // 自动执行的这一轮只是过程说明，隐藏掉，只保留最终汇总
  item.hidden = true
  const result = await execTool(item, index, seg)
  callCache.set(cacheKey, result)
  const remaining = MAX_TOOL_ROUNDS - autoDepth.value
  const tail =
    remaining <= 2
      ? '\n\n注意：轮次即将用尽，这一轮之后必须直接输出最终回答（如需落库，现在就一次性调用写入类工具完成）。'
      : ''
  await ask([
    {
      role: 'user',
      content: `【工具 ${seg.tool} 的真实执行结果】\n${wrapUntrusted(`tool:${seg.tool}`, result.output)}\n\n请据此继续下一步（每次最多调用 1 个工具，禁止用相同参数重复调用已执行过的工具）。${tail}`,
    },
  ])
}

interface NativePendingCall {
  call: AiToolCall
  args: Record<string, unknown>
  seg: Segment
  index: number
  cacheKey: string
  uncertainKey: string
  confirmed: boolean
  result?: ToolResult
}

/** 执行原生 function calling 的一轮工具，并把结果按 tool_call_id 回灌模型 */
async function afterNativeReply(item: ChatItem) {
  if (aborted.value || sending.value) return
  const calls = item.toolCalls ?? []
  if (!calls.length) return
  // 与文本协议一致：用户关掉「自动执行」时只展示卡片，等用户手动运行
  if (!autoRun.value) return

  const tools = registerRuns(item)
  autoDepth.value += 1
  // 原生调用块是最后追加的：模型正文里即使混了 fbin-tool，也不会打乱对应关系
  const base = Math.max(0, tools.length - calls.length)
  const pending: NativePendingCall[] = calls.map((call, index) => {
    const args = parseNativeArguments(call.arguments)
    const parsed = tools[base + index]
    const seg =
      parsed?.seg ??
      ({
        type: 'tool',
        text: '',
        lang: '',
        code: '',
        tool: call.name,
        args: args ?? {},
      } as Segment)
    return {
      call,
      args: args ?? {},
      seg,
      index: parsed?.index ?? index,
      cacheKey: callKey(call.name, args ?? {}),
      uncertainKey: `${traceId.value ?? ''}:${callKey(call.name, args ?? {})}`,
      confirmed: false,
    }
  })

  // 先做参数合法性检查；权限门放到执行前，避免多个写工具一次性弹完所有确认框
  for (const entry of pending) {
    if (entry.call.arguments.trim() && parseNativeArguments(entry.call.arguments) === null) {
      entry.result = {
        ok: false,
        output: `参数不是合法 JSON：${entry.call.arguments.slice(0, 200)}`,
        durationMs: 0,
        outcome: 'error',
      }
    }
  }

  const plan = nativeRunPlan(calls)
  const needsGate = plan.needsGate
  // 只读过程隐藏；写 / 发送保留卡片，用户能回看改了哪条数据
  item.hidden = !needsGate

  const runOne = async (entry: NativePendingCall) => {
    if (entry.result) return
    const cached = callCache.get(entry.cacheKey)
    if (cached) {
      repeats.value += 1
      if (repeats.value >= 2) forcedFinal.value = true
      entry.result = cached
      return
    }
    if (needsGate) {
      const gate = await authorizeToolCall(entry.call.name, entry.args, entry.uncertainKey)
      if (!gate.allowed) {
        entry.result = {
          ok: false,
          output: gate.reason ?? '工具调用已被拒绝，未做任何修改。',
          durationMs: 0,
          outcome: 'error',
        }
        return
      }
      entry.confirmed = gate.confirmed === true
    }
    const result = await execTool(
      item,
      entry.index,
      entry.seg,
      entry.args,
      entry.confirmed,
    )
    entry.result = result
    // 只读与发送结果可复用；写工具不缓存，避免把已执行的写操作当成重复调用
    if (!isWriteTool(entry.call.name)) callCache.set(entry.cacheKey, result)
    if (result.outcome === 'unknown') uncertainWrites.add(entry.uncertainKey)
  }

  if (needsGate) {
    // 写工具串行：一次确认一个，执行完再下一个
    for (const entry of pending) await runOne(entry)
  } else {
    // 只读工具一轮最多 3 个并行，超过就分批
    for (const batch of plan.batches) {
      await Promise.all(batch.map((index) => runOne(pending[index])))
    }
  }

  if (aborted.value) return
  // 每个 tool_call_id 都必须有结果，否则下一轮协议直接 400
  for (const entry of pending) {
    const output = entry.result?.output ?? '工具未执行。'
    const body = wrapUntrusted(`tool:${entry.call.name}`, output)
    messages.value.push({
      id: nextId(),
      role: 'tool',
      content: body,
      toolCallId: entry.call.id,
      toolName: entry.call.name,
      createdAt: Date.now(),
      hidden: true,
    })
    if (sessionId.value) {
      await aiMessageAdd(sessionId.value, 'tool', body).catch(() => undefined)
    }
  }
  scrollBottom()

  if (forcedFinal.value || autoDepth.value >= MAX_TOOL_ROUNDS) {
    forcedFinal.value = true
    await ask([
      {
        role: 'user',
        content:
          '【轮次用尽】禁止再调用任何工具。请立刻用中文输出：已完成的步骤、已知的关键信息、最终方案或结论；若确实还缺信息，明确说明缺什么。',
      },
    ])
    return
  }
  const remaining = MAX_TOOL_ROUNDS - autoDepth.value
  if (remaining <= 2) {
    await ask([
      {
        role: 'user',
        content:
          '轮次即将用尽：这一轮之后必须直接输出最终回答；如需落库，现在就一次性调用写入类工具完成。',
      },
    ])
    return
  }
  await ask()
}

async function execTool(
  item: ChatItem,
  index: number,
  seg: Segment,
  argsOverride?: Record<string, unknown>,
  confirmed = false,
): Promise<ToolResult> {
  const key = runKey(item.id, index)
  let run = toolRuns[key]
  if (!run) {
    run = { tool: seg.tool, args: seg.args, status: 'idle', output: '', durationMs: 0 }
    toolRuns[key] = run
  }
  run.status = 'running'
  run.output = ''
  if (task.active) {
    task.update({
      phase: 'tool',
      round: Math.max(1, autoDepth.value),
      tool: seg.tool,
      canStop: true,
      awaitingConfirm: false,
    })
  }
  const result = await executeTool(seg.tool, argsOverride ?? seg.args, sessionId.value, {
    round: Math.max(1, autoDepth.value),
    traceId: traceId.value,
    confirmed,
    requestKey: traceId.value,
    isCancelled: () => aborted.value,
  })
  run.durationMs = result.durationMs
  if (aborted.value) {
    // 工具允许自然结束（不做硬杀），但停止后结果丢弃，只标记状态供界面查看
    run.status = 'error'
    run.output = '已停止，本轮工具结果已丢弃'
    return { ...result, ok: false, output: '用户已停止，本轮工具结果已丢弃' }
  }
  run.status = result.ok ? 'ok' : 'error'
  run.output = result.output
  run.summary = result.summary
  run.total = result.total
  run.truncated = result.truncated
  if (task.active) task.update({ phase: 'thinking', tool: null, awaitingConfirm: false })
  return result
}

const CONFIRM_TEXT: Record<string, string> = {
  plan_save: '将把下面这份压测计划保存到项目，之后可在「压测」页载入执行（不会立即开始压测）。',
  plan_delete: '将永久删除这份压测计划，无法撤销。',
  stress_start: '将立即开始真实压测，会向目标地址持续发送请求。',
  stress_stop: '将立即停止正在运行的压测。',
  run_batch: '将批量执行请求，会向真实地址发送 HTTP 请求。',
  project_create: '将在本机新建一个项目。',
  project_update: '将修改项目的名称或描述。',
  project_delete: '将删除项目及其下全部集合与请求，无法撤销。',
  collection_create: '将新建一个集合。',
  collection_rename: '将重命名集合。',
  collection_delete: '将删除集合及其下全部请求，无法撤销。',
  request_create: '将新建并保存一个请求。',
  request_update: '将修改已保存请求的配置。',
  request_delete: '将永久删除该请求，无法撤销。',
  request_script_save: '将覆盖该请求的前置 / 后置脚本。',
  curl_import: '将把 cURL 解析结果保存为新请求。',
  env_create: '将新建一个环境。',
  env_var_save: '将新增或覆盖环境变量。',
  env_activate: '将切换项目的当前环境。',
  history_export: '将把执行记录写出到本地文件。',
  report_export: '将把压测报告写出到本地文件。',
  project_export: '将把项目导出到本地文件。',
  project_import: '将把文件内容导入为本机项目。',
  api_import: '将把 Postman / Apifox 文件或 HAR 抓包导入为本机项目。',
  openapi_import: '将把 OpenAPI 文档导入为本机项目。',
  openapi_export: '将导出 OpenAPI 文档到本地文件。',
  env_delete: '将删除该环境及其变量，无法撤销；引用此环境的请求会回退到无环境。',
  env_script_save: '将覆盖该环境的环境脚本，之后每次发送请求前都会执行新脚本。',
  report_delete: '将永久删除该份压测报告与采样数据，无法撤销。',
  test_suite_create: '将在本机新建一个测试套件，之后可往里添加用例。',
  test_suite_update: '将修改测试套件的名称 / 标签 / 配置或前后置脚本。',
  test_suite_delete: '将删除该测试套件及其全部用例、数据集与运行记录，无法撤销。',
  test_case_create: '将往套件里新增一条测试用例。',
  test_case_update: '将修改该测试用例的断言 / 脚本 / 压测快照。',
  test_case_delete: '将永久删除该测试用例，无法撤销。',
  test_run_start: '将真实执行整个测试套件，按顺序发送请求或运行脚本。',
  test_run_stop: '将立即停止正在运行的测试套件，已完成的用例结果会保留。',
  test_report_export: '将为该次运行生成并写出测试报告文件。',
  test_dataset_save: '将新增或覆盖一个测试数据集（json / csv）。',
  test_dataset_delete: '将永久删除该测试数据集，无法撤销；引用它的用例需要重新指定。',
  test_suite_import: '将把套件包导入为本机测试套件，同名套件按冲突策略处理。',
  test_suite_export: '将把测试套件写出为套件包文件。',
  test_artifact_export: '将把测试运行附件复制到指定路径。',
  request_batch_create: '将在同一集合下批量新建多个请求。',
  capture_start: '将启动本地抓包代理并开始真实监听流量。',
  capture_stop: '将停止抓包代理，已抓到的流量会保留。',
  capture_repeat: '将按抓包记录重发一次真实 HTTP 请求；录制中会生成新流量。',
  capture_export: '将把抓包流量写出为 HAR 或 ZIP 文件。',
  send_request: '将向真实地址发送 HTTP 请求，并执行该请求的环境 / 前置 / 后置脚本与断言。',
}

function describeWrite(tool: string): string {
  return CONFIRM_TEXT[tool] ?? '该操作会修改本机数据，确认后立即执行。'
}

/** 确认卡「参数」表里用到的中文字段名（测试域） */
const TEST_FIELD_LABELS: Record<string, string> = {
  name: '名称',
  description: '描述',
  envId: '环境',
  tags: '标签',
  config: '套件配置',
  setupScript: '前置脚本',
  teardownScript: '后置脚本',
  requestId: '绑定的接口',
  datasetId: '数据集',
  dependsOn: '依赖用例',
  assertions: '断言',
  variables: '变量',
  preScript: '前置脚本',
  postScript: '后置脚本',
  mainScript: '主脚本',
  stress: '压测配置',
  timeoutMs: '超时（毫秒）',
  retries: '重试次数',
  kind: '类型',
  failFast: '失败即停',
  vars: '临时变量',
  secrets: '临时密钥',
  formats: '报告格式',
  format: '格式',
  path: '路径',
  content: '内容',
  onConflict: '冲突策略',
  artifactId: '附件',
}

/** 只挑出用户真正改到的字段，忽略 id 类入参 */
function changedFields(args: Record<string, unknown>, skip: string[]): string[] {
  return Object.keys(args)
    .filter((key) => !skip.includes(key))
    .filter((key) => args[key] !== undefined && args[key] !== null && args[key] !== '')
    .map((key) => TEST_FIELD_LABELS[key] ?? key)
}

function countKeys(value: unknown): number {
  return value && typeof value === 'object' ? Object.keys(value as Record<string, unknown>).length : 0
}

/** 测试域的确认卡预览：能查到名字的（套件 / 用例 / 运行）顺带补上名字 */
async function previewTest(tool: string, args: Record<string, unknown>): Promise<string> {
  const text = (key: string) => (args[key] === undefined || args[key] === null ? '' : String(args[key]))
  const num = (key: string) => {
    const value = typeof args[key] === 'number' ? (args[key] as number) : Number(args[key])
    return Number.isFinite(value) && value > 0 ? value : 0
  }
  const lines: string[] = []

  if (tool === 'test_run_start') {
    const suiteId = num('suiteId')
    const envId = num('envId')
    try {
      const preflight = await testPreflight(suiteId, envId || undefined)
      lines.push(
        `套件：${preflight.suiteName}`,
        `环境：${preflight.envName || '套件默认环境'}`,
        `用例：启用 ${preflight.enabledCases} / 共 ${preflight.totalCases} 个`,
        `预检：${
          preflight.errors
            ? `${preflight.errors} 个错误，执行会被拦截`
            : preflight.warnings
              ? `全部通过，${preflight.warnings} 条提示`
              : '全部通过'
        }`,
      )
    } catch (error) {
      lines.push(`套件：#${suiteId || '?'}`, `预检：读取失败（${String(error)}）`)
    }
    lines.push(`失败即停：${args.failFast === true ? '是' : '否'}`)
    const vars = countKeys(args.vars)
    const secrets = countKeys(args.secrets)
    if (vars) lines.push(`临时变量：${vars} 个`)
    if (secrets) lines.push(`临时密钥：${secrets} 个（仅本次运行使用，不落库）`)
    return lines.join('\n')
  }

  if (tool === 'test_run_stop') {
    const runId = num('runId')
    lines.push(`运行：#${runId || '?'}（停止后已完成的用例结果会保留）`)
    try {
      const { run } = await testRunGet(runId)
      const done = run.passed + run.failed + run.skipped + run.errored
      lines.push(`进度：已完成 ${done} / ${run.total} 个用例`)
      if (done) {
        lines.push(
          `结果：通过 ${run.passed}，失败 ${run.failed}，错误 ${run.errored}，跳过 ${run.skipped}`,
        )
      }
      if (run.envName) lines.push(`环境：${run.envName}`)
    } catch {
      // 读不到就只报 id，不影响确认
    }
    return lines.join('\n')
  }

  if (tool === 'test_report_export') {
    const formats = Array.isArray(args.formats) ? args.formats.map((item) => String(item)) : []
    lines.push(`运行：#${text('runId') || '?'}`, `格式：${formats.join(' / ') || '默认（html + json）'}`)
    return lines.join('\n')
  }

  if (tool === 'test_suite_delete' || tool === 'test_case_delete') {
    const id = num('id')
    try {
      if (tool === 'test_suite_delete') {
        const detail = await testSuiteGet(id)
        const enabled = detail.cases.filter((item) => item.enabled).length
        lines.push(
          `套件：${detail.suite.name}`,
          `用例：${detail.cases.length} 个（启用 ${enabled} 个）`,
          `数据集：${detail.datasets.length} 个`,
          '删除后连同全部运行记录一起消失',
        )
      } else {
        const item = await testCaseGet(id)
        lines.push(
          `用例：${item.name || '(未命名)'}`,
          `类型：${testCaseKindLabel(item.kind) || item.kind}`,
          `所属套件：#${item.suiteId}`,
        )
        if (item.requestId) lines.push(`绑定接口：#${item.requestId}`)
      }
    } catch {
      lines.push(`${tool === 'test_suite_delete' ? '套件' : '用例'}：#${id || '?'}`)
    }
    return lines.join('\n')
  }

  if (tool === 'test_suite_create' || tool === 'test_suite_update') {
    if (tool === 'test_suite_create') {
      lines.push(`套件：${text('name') || '(未命名)'}`, `项目：#${text('projectId') || '未指定'}`)
      if (text('tags')) lines.push(`标签：${text('tags')}`)
      lines.push(`套件配置：${countKeys(args.config) ? `${countKeys(args.config)} 项` : '默认'}`)
      const scripts = ['setupScript', 'teardownScript'].filter((key) => text(key)).length
      if (scripts) lines.push(`脚本：${scripts} 段（前置 / 后置）`)
    } else {
      lines.push(`套件：#${text('id') || '?'}`, `将修改：${changedFields(args, ['id']).join('、') || '无'}`)
    }
    return lines.join('\n')
  }

  if (tool === 'test_case_create' || tool === 'test_case_update') {
    if (tool === 'test_case_create') {
      lines.push(
        `所属套件：#${text('suiteId') || '?'}`,
        `用例：${text('name') || '(未命名)'}（${testCaseKindLabel(args.kind) || text('kind') || '类型未指定'}）`,
      )
      if (text('requestId')) lines.push(`绑定接口：#${text('requestId')}`)
      if (text('datasetId')) lines.push(`数据集：#${text('datasetId')}`)
      const assertions = Array.isArray(args.assertions) ? args.assertions.length : 0
      lines.push(`断言：${assertions ? `${assertions} 条` : '未设置'}`)
      if (text('mainScript')) lines.push(`主脚本：${text('mainScript').split('\n').length} 行`)
      if (countKeys(args.stress)) lines.push('压测：内联压测快照（并发 / 时长 / SLA 见参数表）')
    } else {
      lines.push(`用例：#${text('id') || '?'}`, `将修改：${changedFields(args, ['id']).join('、') || '无'}`)
    }
    return lines.join('\n')
  }

  if (tool === 'test_dataset_save' || tool === 'test_dataset_delete') {
    const id = num('id')
    if (id) {
      try {
        const dataset = await testDatasetGet(id)
        lines.push(
          `数据集：${dataset.name}`,
          `所属套件：#${dataset.suiteId}`,
          `格式：${dataset.format.toUpperCase()}（${dataset.rowCount} 行）`,
        )
        if (dataset.path) lines.push(`路径：${dataset.path}`)
      } catch {
        lines.push(`数据集：#${id}`)
      }
    }
    if (tool === 'test_dataset_delete') {
      lines.push('删除后引用该数据集的用例需要重新指定')
    } else {
      if (!id) lines.push(`所属套件：#${text('suiteId') || '?'}`, `名称：${text('name') || '(未命名)'}`)
      if (!id || text('format')) lines.push(`格式：${(text('format') || 'json').toUpperCase()}`)
      if (text('path')) lines.push(`来源文件：${text('path')}`)
      if (text('content')) lines.push(`内联内容：${text('content').length} 字符`)
    }
    return lines.join('\n')
  }

  if (tool === 'test_suite_import') {
    lines.push(
      `套件包：${text('path') || '(未填写)'}`,
      `导入到项目：#${text('projectId') || '自动选择'}`,
      `同名冲突：${text('onConflict') || 'rename（自动改名）'}`,
    )
    return lines.join('\n')
  }

  if (tool === 'test_suite_export') {
    const suiteId = num('suiteId')
    lines.push(`套件：#${suiteId || '?'}`, `导出到：${text('path') || '(未填写)'}`)
    if (suiteId) {
      try {
        const detail = await testSuiteGet(suiteId)
        lines[0] = `套件：${detail.suite.name}（${detail.cases.length} 个用例）`
      } catch {
        // 读不到就只报 id
      }
    }
    return lines.join('\n')
  }

  if (tool === 'test_artifact_export') {
    lines.push(`附件：#${text('artifactId') || '?'}`, `复制到：${text('path') || '(未填写)'}`)
    return lines.join('\n')
  }

  return lines.join('\n')
}

/** 抓包域的确认卡预览：启动参数、重发覆盖项、导出范围 */
async function previewCapture(tool: string, args: Record<string, unknown>): Promise<string> {
  const text = (key: string) => (args[key] === undefined || args[key] === null ? '' : String(args[key]))
  const list = (key: string) => (Array.isArray(args[key]) ? (args[key] as unknown[]).map((item) => String(item)) : [])
  const lines: string[] = []

  if (tool === 'capture_start') {
    lines.push(
      `端口：${text('port') || '沿用当前配置'}`,
      `监听地址：${text('bindAddr') || '默认（本机）'}`,
    )
    const hosts = list('sslHosts')
    if (hosts.length) lines.push(`解密域名：${hosts.slice(0, 10).join('、')}${hosts.length > 10 ? ` 等 ${hosts.length} 个` : ''}`)
    const ignored = list('ignorePatterns')
    if (ignored.length) lines.push(`忽略规则：${ignored.slice(0, 10).join('、')}`)
    if (text('upstreamProxy')) lines.push(`上游代理：${text('upstreamProxy')}`)
    if (args.captureStatic !== undefined) lines.push(`抓静态资源：${args.captureStatic === true ? '是' : '否'}`)
    return lines.join('\n')
  }

  if (tool === 'capture_stop') {
    lines.push('停止后不再录制新流量，已抓到的流量会保留在列表中')
    return lines.join('\n')
  }

  if (tool === 'capture_repeat') {
    const id = Number(args.id)
    if (Number.isFinite(id) && id > 0) {
      try {
        const flow = await captureFlowDetail(id)
        lines.push(`原始请求：${flow.method} ${flow.scheme}://${flow.host}${flow.path}`)
      } catch {
        lines.push(`流量：#${id}`)
      }
    }
    if (text('method')) lines.push(`覆盖方法：${text('method').toUpperCase()}`)
    if (text('url')) lines.push(`覆盖地址：${text('url')}`)
    if (args.headers !== undefined) {
      const count = Array.isArray(args.headers) ? args.headers.length : countKeys(args.headers)
      if (count) lines.push(`覆盖请求头：${count} 项`)
    }
    if (text('body')) lines.push(`覆盖 Body：${text('body').length} 字符`)
    return lines.join('\n')
  }

  if (tool === 'capture_export') {
    const ids = list('ids')
    const format = text('format') || (text('path').toLowerCase().endsWith('.har') ? 'har' : 'zip')
    lines.push(
      `导出到：${text('path') || '(未填写)'}`,
      `格式：${format.toUpperCase()}`,
      `范围：${ids.length ? `${ids.length} 条指定流量` : '当前全部流量'}`,
    )
    return lines.join('\n')
  }

  return lines.join('\n')
}

/** 把工具参数整理成人能看懂的预览，重点展示计划类与测试运行这类需要人工核对的配置 */
async function previewArgs(
  tool: string,
  args: Record<string, unknown>,
): Promise<string | undefined> {
  const text = (key: string) => (args[key] === undefined || args[key] === null ? '' : String(args[key]))
  if (tool === 'plan_save' || tool === 'stress_start') {
    const mode = text('mode') || (Array.isArray(args.stages) ? 'stages' : 'fixed')
    const lines = [
      `名称：${text('name') || '(未命名)'}`,
      `关联接口：${text('requestId') || text('id') || '未指定'}`,
      `模式：${mode === 'stages' ? '梯度加压' : '固定并发'}`,
    ]
    if (mode === 'stages' && Array.isArray(args.stages)) {
      const stages = args.stages as Array<Record<string, unknown>>
      lines.push(
        `阶段：${stages.map((s) => `${s.concurrent ?? '?'} 并发 × ${s.durationSec ?? '?'}s`).join(' → ')}`,
      )
    } else {
      lines.push(
        `并发：${text('concurrent') || '10'}`,
        `时长：${text('durationSec') || '30'} 秒`,
      )
    }
    if (text('rampUpSec')) lines.push(`爬坡：${text('rampUpSec')} 秒`)
    if (text('thinkTimeMs')) lines.push(`思考时间：${text('thinkTimeMs')} 毫秒`)
    if (text('targetRps')) lines.push(`目标 RPS：${text('targetRps')}`)
    const sla = ['slaP95', 'slaP99', 'slaErrorRate', 'slaMinRps'].some((key) => text(key))
    if (sla) {
      lines.push(
        `SLA：P95 ≤ ${text('slaP95') || '-'}ms，P99 ≤ ${text('slaP99') || '-'}ms，错误率 ≤ ${text('slaErrorRate') || '-'}%，最低 RPS ≥ ${text('slaMinRps') || '-'}`,
      )
    }
    if (text('envId')) lines.push(`环境：#${text('envId')}`)
    return lines.join('\n')
  }
  if (tool.startsWith('test_')) {
    const preview = await previewTest(tool, args)
    return preview || undefined
  }
  if (tool.startsWith('capture_')) {
    const preview = await previewCapture(tool, args)
    return preview || undefined
  }
  if (tool === 'request_batch_create') {
    const items = Array.isArray(args.requests) ? (args.requests as Array<Record<string, unknown>>) : []
    const lines = [`目标集合：#${text('collectionId') || '?'}`, `请求数：${items.length} 个`]
    for (const item of items.slice(0, 10)) {
      const method = String(item.method ?? 'GET').toUpperCase()
      lines.push(`- ${String(item.name ?? '(未命名)')}：${method} ${String(item.url ?? '')}`)
    }
    if (items.length > 10) lines.push(`…（其余 ${items.length - 10} 个见参数表）`)
    return lines.join('\n')
  }
  return undefined
}

/** 最近一条真实用户输入（跳过工具结果回灌的伪用户消息） */
function latestUserText(): string {
  for (let i = messages.value.length - 1; i >= 0; i -= 1) {
    const item = messages.value[i]
    if (item.role !== 'user') continue
    const text = item.content
    if (!text.trim()) continue
    if (text.startsWith('【工具 ') || text.startsWith('用户拒绝执行工具')) continue
    return item.quote ? `${item.quote}\n${text}` : text
  }
  return ''
}

function squash(text: string) {
  return text.replace(/\s+/g, '')
}

/**
 * 模型复述超长工具参数（尤其是 cURL）时会被输出上限截断。
 * 这里按「用户消息里的原文」补全：模型给的是原文前缀、或参数里出现省略号，就换成完整原文。
 */
function resolveToolArgs(seg: Segment): Record<string, unknown> {
  if (seg.tool !== 'curl_import') return seg.args
  const arg = typeof seg.args.curl === 'string' ? seg.args.curl.trim() : ''
  const fromUser = extractCurlCommand(latestUserText())
  if (!fromUser) return seg.args
  const candidate = fromUser.trim()
  if (!arg) return { ...seg.args, curl: candidate }
  if (candidate.length <= arg.length) return seg.args
  const truncated = /…|\.\.\.|（已截断）|已省略|\[truncated\]/i.test(arg)
  const head = squash(arg).slice(0, 160)
  const isPrefix = head.length >= 40 && squash(candidate).startsWith(head)
  return truncated || isPrefix ? { ...seg.args, curl: candidate } : seg.args
}

/** 导出类工具：目标文件已存在时必须二次确认，避免静默覆盖 */
const EXPORT_TOOLS = new Set([
  'project_export',
  'report_export',
  'history_export',
  'openapi_export',
  'capture_export',
  'test_suite_export',
  'test_artifact_export',
])

interface ToolGateResult {
  allowed: boolean
  /** 拒绝原因，可直接回灌模型 */
  reason?: string
  /** send_request 弹过确认框（命令层最终判定仍会校验） */
  confirmed?: boolean
}

/** 工具决策落复盘日志：用户取消、只读拒绝同样算一次 Agent 决策点 */
function logToolDecision(tool: string, args: Record<string, unknown>, preview: string) {
  void aiToolLogAdd({
    sessionId: sessionId.value,
    tool,
    args: maskedArgsForLog(tool, args),
    ok: false,
    durationMs: 0,
    resultPreview: preview,
    round: Math.max(1, autoDepth.value),
    traceId: traceId.value,
  }).catch(() => undefined)
}

/**
 * 写工具 / send_request 的统一权限门（AI-01）：
 * 只读模式拒绝、结果未知禁止重试、导出覆盖确认、计划预览确认；
 * 拒绝时返回可直接回灌模型的中文说明，文本协议与原生协议共用。
 */
async function authorizeToolCall(
  tool: string,
  args: Record<string, unknown>,
  uncertainKey: string,
): Promise<ToolGateResult> {
  if (tool === 'send_request') {
    if (uncertainWrites.has(uncertainKey)) {
      return {
        allowed: false,
        reason:
          `【结果未知】send_request 上一次已经超时，后台可能已完成，禁止再次自动执行。` +
          '请先用文字让用户核对执行记录或服务端数据，确认后再重试。',
      }
    }
    const mode = await currentAiPermissionMode()
    const preview = await previewSendRequest(args).catch(() => null)
    const method = preview?.method ?? 'GET'
    // 预览失败时按最保守的「需要确认」处理，由命令层最终判定
    const decision = preview ? decideSend(mode, method, preview.isProd) : 'confirm'
    const deny = (reason: string, logText: string): ToolGateResult => {
      auditAiDecision('send_request', preview?.url, false, mode)
      logToolDecision('send_request', args, logText)
      return { allowed: false, reason }
    }
    if (decision === 'deny') {
      return deny(
        `当前为只读权限模式，已拦截 ${method} 请求（本次未发送）。请只做查询；需要写操作时让用户到「设置 → 网络与安全 → AI 权限」调整 AI 权限模式。`,
        '只读权限模式已拦截非只读请求（未发送）',
      )
    }
    if (decision === 'confirm') {
      const prod = preview?.isProd === true
      if (task.active) {
        task.update({
          phase: 'confirm',
          awaitingConfirm: true,
          canStop: false,
          tool,
          round: Math.max(1, autoDepth.value),
        })
      }
      const confirmed = await new Promise<boolean>((resolve) => {
        Modal.confirm({
          title: prod
            ? `生产环境请求，确认发送「${method}」？`
            : `确认发送「${method}」请求？`,
          width: 620,
          content: h(AiConfirmCard, {
            tool: 'send_request',
            args,
            description: describeWrite('send_request'),
            planText: preview
              ? `${preview.method} ${preview.url}${prod ? '\n\n该环境已标记为生产：即使是 GET 也会二次确认。' : ''}`
              : undefined,
          }),
          okText: prod ? '确认发送到生产环境' : '确认发送',
          okType: prod ? 'danger' : 'primary',
          cancelText: '取消',
          onOk: () => resolve(true),
          onCancel: () => resolve(false),
        })
      })
      if (task.active) task.update({ phase: 'thinking', awaitingConfirm: false, tool: null })
      if (!confirmed) {
        return deny('用户拒绝发送本次请求，未向目标地址发出任何流量。', '用户取消发送（未发送）')
      }
    }
    return { allowed: true, confirmed: decision === 'confirm' }
  }

  if (!isWriteTool(tool)) return { allowed: true }

  const mode = await currentAiPermissionMode()
  // 只读模式：写工具一律拒绝，不进入确认流程
  if (decideWrite(mode) === 'deny') {
    auditAiDecision(tool, writeTarget(args), false, mode)
    logToolDecision(tool, args, '只读权限模式已拒绝写操作（未修改任何数据）')
    return {
      allowed: false,
      reason: `当前为只读权限模式，工具 ${tool} 已被拒绝，未做任何修改。请只做查询与说明；需要写操作时让用户到「设置 → 网络与安全 → AI 权限」把 AI 权限模式改为 confirm_write 或 trusted。`,
    }
  }
  // 上一次同工具同参数已经超时但后台可能执行成功：先让用户核对，禁止自动重试
  if (uncertainWrites.has(uncertainKey)) {
    return {
      allowed: false,
      reason:
        `【结果未知】${tool} 上一次已经超时，后台可能已完成，禁止再次自动执行。` +
        '请先用文字告诉用户去核对结果（数据是否已导入、文件是否已生成），并询问是否需要手动重试；在用户明确确认前不要再次调用该工具。',
    }
  }
  let overwrite = false
  if (EXPORT_TOOLS.has(tool) && typeof args.path === 'string') {
    overwrite = await fileExists(args.path).catch(() => false)
  }
  // 计划类 / 测试域：先把人话预览算出来（test_run_start 会顺带查一次预检），再弹卡
  const planText = await previewArgs(tool, args)
  if (task.active) {
    task.update({
      phase: 'confirm',
      awaitingConfirm: true,
      canStop: false,
      tool,
      round: Math.max(1, autoDepth.value),
    })
  }
  const confirmed = await new Promise<boolean>((resolve) => {
    Modal.confirm({
      title: overwrite
        ? `文件已存在，确认覆盖「${toolLabel(tool)}」？`
        : `确认执行「${toolLabel(tool)}」？`,
      width: 620,
      content: h(AiConfirmCard, {
        tool,
        args,
        description: describeWrite(tool),
        overwrite,
        path: typeof args.path === 'string' ? args.path : undefined,
        planText,
      }),
      okText: overwrite ? '覆盖写入' : '确认执行',
      okType: isDangerousTool(tool) || overwrite ? 'danger' : 'primary',
      cancelText: '取消',
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
  if (task.active) task.update({ phase: 'thinking', awaitingConfirm: false, tool: null })
  auditAiDecision(tool, writeTarget(args), confirmed, mode)
  if (!confirmed) {
    logToolDecision(tool, args, '用户取消执行（未修改任何数据）')
    return {
      allowed: false,
      reason: `用户拒绝执行工具 ${tool}，本次未做任何修改。不要再次自动执行该操作，请先用文字把方案或计划完整说清楚，等用户明确同意后再执行。`,
    }
  }
  return { allowed: true }
}

async function runToolSegment(item: ChatItem, index: number, seg: Segment) {
  if (runState(item.id, index)?.status === 'running') return
  // 长 cURL 经模型复述时可能被输出长度截断：优先用用户消息里的原文补全
  const effectiveArgs = resolveToolArgs(seg)
  // 补全后同步回工具卡，重新运行时也走完整参数（卡片详情里不再显示截断版）
  if (effectiveArgs !== seg.args) seg.args = effectiveArgs
  const cacheKey = callKey(seg.tool, effectiveArgs)
  const uncertainKey = `${traceId.value ?? ''}:${cacheKey}`

  const gate = await authorizeToolCall(seg.tool, effectiveArgs, uncertainKey)
  if (!gate.allowed) {
    // 硬规则 4：拒绝结果也要进入模型上下文
    if (!sending.value && gate.reason) {
      await ask([{ role: 'user', content: gate.reason }])
    }
    return
  }

  if (seg.tool === 'send_request') item.hidden = true
  const result = await execTool(item, index, seg, effectiveArgs, gate.confirmed === true)
  if (seg.tool === 'send_request') callCache.set(cacheKey, result)
  // 写工具超时：后台可能已经完成，锁住同工具同参数，避免模型看到“失败”后重复执行
  if (result.outcome === 'unknown') uncertainWrites.add(uncertainKey)
  if (aborted.value) return
  if (sending.value) return
  await ask([
    {
      role: 'user',
      content: `【工具 ${seg.tool} 的真实执行结果】\n${wrapUntrusted(`tool:${seg.tool}`, result.output)}\n\n请基于以上结果继续分析或完成下一步，必要时可调用其他工具。`,
    },
  ])
}

/** AI 权限模式的当前值；读不到按默认 confirm_write */
async function currentAiPermissionMode(): Promise<string> {
  try {
    return normalizeAiPermissionMode((await settingsGet()).aiPermissionMode)
  } catch {
    return 'confirm_write'
  }
}

/** 确认审计：target 只放能定位对象的短标识，避免把整段参数写进审计 */
function writeTarget(args: Record<string, unknown>): string | undefined {
  if (typeof args.path === 'string' && args.path.trim()) return args.path.trim()
  for (const key of ['id', 'requestId', 'envId', 'projectId', 'suiteId', 'runId']) {
    const value = args[key]
    if (typeof value === 'number' && Number.isFinite(value)) return `#${value}`
  }
  return undefined
}

function auditAiDecision(
  tool: string,
  target: string | undefined,
  allowed: boolean,
  mode: string,
) {
  void aiConfirmAudit({
    tool,
    target: target ?? null,
    allowed,
    mode,
    traceId: traceId.value,
  }).catch(() => undefined)
}

/** send_request 的权限门：放行 / 确认 / 拒绝，与后端 ai_policy 同一套判定 */
async function ensureSession(): Promise<AiSession | null> {
  if (sessionId.value) {
    return sessions.value.find((item) => item.id === sessionId.value) ?? null
  }
  try {
    const created = await aiSessionCreate(undefined, activeId.value)
    sessions.value.unshift(created)
    sessionId.value = created.id
    return created
  } catch (error) {
    notify.error(`会话创建失败：${String(error)}`)
    return null
  }
}

const attachments = ref<ChatAttachment[]>([])
const dropActive = ref(false)
/** 附件总量估算：输入框下方直接展示，避免拖入多个日志后才在发送时超窗 */
const attachTokens = computed(() =>
  attachments.value.reduce((sum, file) => sum + estimateTokens(file.content), 0),
)

/* ---------------- 视觉输入（AI-03） ---------------- */
const pendingImages = ref<PendingImage[]>([])
const imageBusy = ref(false)
const imageInput = ref<HTMLInputElement | null>(null)
/** 只随当前轮发送；发送开始时快照并清空，工具循环的后续轮次不重复带图 */
const turnImages = ref<AiImagePayload[]>([])
const visionEnabled = computed(() => activeProvider.value?.supportsVision === true)
const imageTokens = computed(() => estimateImageTokens(pendingImages.value.length))

function newImageId(): string {
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`
}

async function addImageFiles(files: File[]) {
  if (!visionEnabled.value) {
    notify.warning('当前模型未开启图片输入，请先在模型服务里勾选「支持图片输入」')
    return
  }
  const room = MAX_IMAGES - pendingImages.value.length
  if (room <= 0) {
    notify.warning(`一次最多 ${MAX_IMAGES} 张图片`)
    return
  }
  const accepted = files.filter((file) => supportedImageMime(file.type || '')).slice(0, room)
  if (accepted.length < files.length) {
    notify.warning('只支持 JPG / PNG / WebP 图片，超出数量的已跳过')
  }
  if (!accepted.length) return
  imageBusy.value = true
  try {
    for (const file of accepted) {
      try {
        const compressed = await compressImageFile(file)
        pendingImages.value.push({
          id: newImageId(),
          name: file.name || '粘贴的图片',
          mime: compressed.mime,
          data: compressed.data,
          preview: compressed.preview,
          size: compressed.size,
        })
      } catch (error) {
        notify.warning(`${file.name || '图片'}：${error instanceof Error ? error.message : String(error)}`)
      }
    }
  } finally {
    imageBusy.value = false
  }
}

function pickImages() {
  imageInput.value?.click()
}

async function onImagePicked(event: Event) {
  const inputEl = event.target as HTMLInputElement
  const files = Array.from(inputEl.files ?? [])
  inputEl.value = ''
  if (files.length) await addImageFiles(files)
}

function removeImage(id: string) {
  pendingImages.value = pendingImages.value.filter((item) => item.id !== id)
}

/** 粘贴截图：只拦图片项，文本粘贴保持原有行为 */
function onPasteImages(event: ClipboardEvent) {
  const files: File[] = []
  for (const item of Array.from(event.clipboardData?.items ?? [])) {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) continue
    const file = item.getAsFile()
    if (file) files.push(file)
  }
  if (!files.length) return
  event.preventDefault()
  void addImageFiles(files)
}

/** 统一入口：去重、单文件与总量双上限都走这里 */
function addAttachment(name: string, path: string, raw: string) {
  if (attachments.value.some((item) => item.path === path)) return
  const used = attachments.value.reduce((sum, file) => sum + file.content.length, 0)
  const plan = prepareAttachment(used, raw)
  if (!plan.accepted) {
    notify.warning(plan.warning ?? '附件无法加入')
    return
  }
  if (plan.warning) notify.warning(`${name}：${plan.warning}`)
  attachments.value.push({ name, path, content: plan.content })
}

async function pickAttachments() {
  try {
    const picked = await openDialog({
      multiple: true,
      title: '选择要作为上下文的文本文件',
      filters: [
        {
          name: '文本',
          extensions: [
            'txt', 'json', 'md', 'yaml', 'yml', 'xml', 'csv', 'log',
            'js', 'ts', 'py', 'java', 'go', 'rs', 'sql', 'sh', 'http', 'har',
          ],
        },
      ],
    })
    if (!picked) return
    const paths = Array.isArray(picked) ? picked : [picked]
    for (const path of paths) {
      const content = await fileReadText(path)
      addAttachment(path.split(/[\\/]/).pop() ?? path, path, content)
    }
  } catch (error) {
    notify.error(`读取文件失败：${String(error)}`)
  }
}

/** 只在拖入文件时高亮，拖动输入框里的文字不受影响 */
function onDragOverAttachments(event: DragEvent) {
  if (!event.dataTransfer?.types.includes('Files')) return
  event.preventDefault()
  dropActive.value = true
}

function onDragLeaveAttachments(event: DragEvent) {
  const related = event.relatedTarget as Node | null
  if (related && (event.currentTarget as HTMLElement).contains(related)) return
  dropActive.value = false
}

/** 拖拽：WebView 里拿不到本地路径，直接读 File 内容，路径用合成键去重 */
async function onDropAttachments(event: DragEvent) {
  const files = Array.from(event.dataTransfer?.files ?? [])
  if (!files.length) return
  event.preventDefault()
  dropActive.value = false
  const images: File[] = []
  for (const file of files) {
    if (file.type.startsWith('image/')) {
      images.push(file)
      continue
    }
    if (!isTextAttachment(file.name)) {
      notify.warning(`${file.name} 不是文本文件，已跳过`)
      continue
    }
    try {
      addAttachment(file.name, `drop:${file.name}:${file.size}:${file.lastModified}`, await file.text())
    } catch (error) {
      notify.warning(`${file.name} 读取失败：${String(error)}`)
    }
  }
  if (images.length) await addImageFiles(images)
}

function fmtSize(text: string): string {
  const kb = text.length / 1024
  return kb < 1 ? `${text.length} 字符` : `${kb.toFixed(1)} KB`
}

/** 附件拼进用户消息，模型与历史都能看到 */
function withAttachments(content: string): string {
  if (!attachments.value.length) return content
  const blocks = attachments.value.map(
    (file) => `\`\`\`fbin-file ${file.name}\n${file.content}\n\`\`\``,
  )
  return `${content}\n\n${blocks.join('\n\n')}`
}

/** 落库时把附件正文换成摘要，大段文件内容只喂给模型，不写进数据库 */
function forStorage(content: string): string {
  return content.replace(
    /```fbin-file ([^\n]+)\n([\s\S]*?)\n```/g,
    (_match, name: string, body: string) => `【附件 ${name} · ${body.length} 字符，正文未入库】`,
  )
}

/* ---------------- AI-13 发送前预览：本次到底会把什么发给模型 ---------------- */

const PERMISSION_LABEL: Record<string, string> = {
  readonly: '只读（拒绝一切写操作）',
  confirm_write: '写操作需确认',
  trusted: '自动执行写操作',
}

const previewOpen = ref(false)
const previewLoading = ref(false)
const previewRows = ref<Array<{ label: string; value: string }>>([])
const previewNotes = ref<string[]>([])

/** 从 baseUrl 取主机名；解析失败时退化为字符串截取，不抛错 */
function previewHost(url: string): string {
  const text = (url || '').trim()
  if (!text) return '—'
  try {
    return new URL(text).host || text
  } catch {
    return text.replace(/^https?:\/\//i, '').split('/')[0] || text
  }
}

function previewIsLocal(url: string): boolean {
  const host = previewHost(url).toLowerCase()
  return (
    host.startsWith('localhost') ||
    host.startsWith('127.') ||
    host.startsWith('[::1]') ||
    host.startsWith('::1') ||
    host.startsWith('0.0.0.0')
  )
}

/** 只读预览：不触发发送，也不改动上下文裁剪状态 */
async function openSendPreview() {
  if (!activeProvider.value) {
    notify.warning('请先配置一个模型服务')
    return
  }
  previewOpen.value = true
  previewLoading.value = true
  try {
    const provider = activeProvider.value
    const rows: Array<{ label: string; value: string }> = []
    const noteLines: string[] = []

    const local = previewIsLocal(provider.baseUrl)
    rows.push({ label: '服务商', value: `${provider.name} · ${provider.kind}` })
    rows.push({ label: '模型', value: provider.model || '—' })
    rows.push({
      label: '请求地址',
      value: `${previewHost(provider.baseUrl)}${local ? '（本机 / 内网回环）' : '（云端）'}`,
    })

    const permission = await currentAiPermissionMode()
    rows.push({ label: '权限模式', value: PERMISSION_LABEL[permission] ?? permission })

    const systemTokens = estimateTokens(SYSTEM_PROMPT) + backendPromptTokens.value
    rows.push({
      label: '系统提示词',
      value: `${fmtNum(estimateTokens(SYSTEM_PROMPT))} token（前端 ${SYSTEM_PROMPT.length} 字）+ 后端 ${fmtNum(backendPromptTokens.value)} token`,
    })

    const history = historyItems()
    rows.push({
      label: '历史消息',
      value: `${history.length} 条 · 约 ${fmtNum(estimateMessages(history))} token（超出窗口预算会自动裁剪）`,
    })

    const typed = input.value.trim()
    const attachChars = attachments.value.reduce((sum, file) => sum + file.content.length, 0)
    const inputTokens =
      estimateTokens(typed) +
      attachments.value.reduce((sum, file) => sum + estimateTokens(file.content), 0)
    rows.push({
      label: '本次输入',
      value: typed
        ? `${typed.length} 字${attachChars ? ` + 附件 ${attachments.value.length} 个 / ${attachChars} 字` : ''}`
        : attachments.value.length
          ? `仅附件 ${attachments.value.length} 个 / ${attachChars} 字`
          : pendingImages.value.length
            ? '仅图片（发送时自动补默认提示语）'
            : '尚未输入',
    })

    if (pendingImages.value.length) {
      rows.push({
        label: '图片',
        value: `${pendingImages.value.length} 张 · 约 ${fmtNum(imageTokens.value)} token`,
      })
    }

    const useNative =
      nativeToolsEnabled.value && !forcedFinal.value && autoDepth.value < MAX_TOOL_ROUNDS
    rows.push({
      label: '工具',
      value: `${nativeToolDefs().length} 个 · ${useNative ? '原生 function calling' : '文本协议（工具定义随提示词下发）'}`,
    })
    rows.push({
      label: '上下文预算',
      value: `窗口 ${fmtNum(modelWindow.value)} token，输入上限 ${fmtNum(contextBudget.value)}，输出预留 ${fmtNum(outputReserve.value)}`,
    })

    const fallbackIds = String(appSettings.value?.aiFallbackIds || '')
      .split(',')
      .map((item) => Number(item.trim()))
      .filter((id) => Number.isInteger(id) && id > 0 && id !== provider.id)
    rows.push({
      label: '备用模型',
      value: fallbackIds.length
        ? `${fallbackIds.length} 个已启用，首字前失败时按设置顺序切换`
        : '未启用',
    })

    rows.push({
      label: '费用预估',
      value: `输入约 ${fmtNum(systemTokens + estimateMessages(history) + inputTokens)} token · ${fmtUsd(
        estimateCost(
          provider.model,
          systemTokens + estimateMessages(history) + inputTokens,
          0,
          priceOverrideOf(provider),
        ).total,
      )}`,
    })

    noteLines.push(
      'API Key 只作为 HTTP 鉴权头发送，不会出现在提示词、日志或导出内容中。',
      local ? '当前地址是本机 / 回环，内容不会离开这台机器。' : '当前地址在云端，系统提示词、历史消息与附件正文会发送给该服务商。',
      '企业策略若开启「禁止云端 AI」，非回环地址会被后端直接拒绝。',
    )
    previewNotes.value = noteLines
    previewRows.value = rows
  } finally {
    previewLoading.value = false
  }
}

async function send() {
  const images = pendingImages.value.map((item) => ({ mime: item.mime, data: item.data }))
  const typed = input.value.trim()
  const content = typed || (images.length ? '请分析这些图片。' : '')
  if ((!content && !attachments.value.length) || sending.value) return
  if (!canChat.value) {
    notify.warning('请先配置一个模型服务')
    serviceOpen.value = true
    return
  }
  if (images.length && !visionEnabled.value) {
    notify.warning('当前模型未开启图片输入，请换用支持视觉的模型')
    return
  }
  // 月度预算：开启「用尽停止」且已超额时拒绝新提问；工具续轮不受影响
  if (!(await budgetGate())) return
  turnImages.value = images
  pendingImages.value = []
  input.value = ''
  aborted.value = false
  autoDepth.value = 0
  rejections.value = 0
  repairs.value = 0
  repeats.value = 0
  forcedFinal.value = false
  turnPromptTokens.value = 0
  turnOutputTokens.value = 0
  turnCost.value = 0
  turnProvider.value = null
  traceId.value = newRequestId()
  callCache.clear()
  uncertainWrites.clear()
  const currentQuote = quote.value
  quote.value = null
  const full = compactPrompt(
    withAttachments(images.length ? `${content}\n\n（附 ${images.length} 张图片）` : content),
  )
  attachments.value = []
  const session = await ensureSession()
  const userItem: ChatItem = {
    id: nextId(),
    role: 'user',
    content: full,
    quote: currentQuote ?? undefined,
    createdAt: Date.now(),
  }
  messages.value.push(userItem)
  scrollBottom(true)
  // 入库只保留前 1000 字，避免长文本把会话表撑大；引用内容以标记形式一起落库
  if (session) {
    const storedContent = currentQuote ? `${quotePrefix(currentQuote)}${full}` : full
    const stored = await aiMessageAdd(session.id, 'user', clipStoredPrompt(storedContent)).catch(
      () => undefined,
    )
    // 对齐数据库时间：编辑重发按 createdAt 删库，本地时间必须和库里一致
    if (stored?.createdAt) userItem.createdAt = stored.createdAt
    if (stored?.id) userItem.dbId = stored.id
  }
  void refreshSessions()
  // 页面现场只注入本轮：chip 立即消失，避免后续每轮重复占用上下文预算
  handoffTurn.value = handoff.value
  handoff.value = null
  task.bindStop(stop)
  task.update({
    active: true,
    runId: traceId.value,
    sessionId: session?.id ?? sessionId.value,
    sessionTitle: session?.title || '新对话',
    phase: 'thinking',
    round: 0,
    tool: null,
    startedAt: Date.now(),
    awaitingConfirm: false,
    canStop: true,
    lastError: '',
  })
  try {
    await ask()
    // 离开航仔页面后任务仍在后台执行；完成提示对用户可见
    if (!aborted.value) notify.success('航仔任务已完成')
  } finally {
    task.clear()
    handoffTurn.value = null
  }
  // 摘要异步执行、下一轮生效：本轮已经答完，压缩不占用户等待时间
  void maybeSummarize()
}

const SUMMARY_INSTRUCTION = `你是会话压缩器。请把下面这段较早的对话压缩成结构化摘要，供后续继续对话使用。
必须保留：用户目标、已确认的决策、已执行的工具及关键结果、未完成事项、关键 ID / 名称 / 路径 / 数值。
禁止编造未出现的事实；不要输出寒暄或解释；用中文分条列出，控制在 600 字以内。`

/**
 * 摘要调用统一走这里；AI-07 允许配置专用小模型，失败时由调用方回退主模型。
 */
async function requestSummary(target: AiProvider, source: string): Promise<string> {
  let summary = ''
  await aiChat(
    {
      providerId: target.id as number,
      messages: [
        { role: 'system', content: SUMMARY_INSTRUCTION },
        { role: 'user', content: source },
      ],
      temperature: 0.2,
      scene: 'summary',
      contextWindow: effectiveWindow(target.model, target.contextWindow),
    },
    (event) => {
      if (event.kind === 'delta' && event.text) summary += event.text
    },
  )
  if (!summary.trim()) throw new Error('摘要为空')
  return summary.trim()
}

/**
 * T-07：历史占用超过预算 60% 时异步压缩较早对话，落库到 ai_sessions.summary。
 * 最近 6 条原文永远不压缩，避免刚说过的话被摘要吃掉。
 */
async function maybeSummarize(force = false) {
  const sid = sessionId.value
  const provider = activeProvider.value
  const summarizer = summaryProvider.value ?? provider
  if (!sid || !provider?.id || !summarizer?.id || summarizing.value || aborted.value) return
  if (!force && summaryPaused.value) return
  if (force) summaryPaused.value = false
  const pending = messages.value.filter(
    (item) =>
      item.content.trim() &&
      !(sessionSummary.value && item.createdAt <= sessionSummary.value.until),
  )
  // 最近几条原文永远保留，避免刚说过的话被摘要吃掉
  const keep = force ? 2 : 6
  if (pending.length <= keep + 2) {
    if (force) notify.info('当前对话还不长，无需压缩')
    return
  }
  const used = estimateMessages(historyItems())
  if (!force && used < contextBudget.value * 0.6) return
  const older = pending.slice(0, pending.length - keep)
  if (!older.length) return

  summarizing.value = true
  try {
    const source =
      (sessionSummary.value ? `已有摘要：\n${sessionSummary.value.text}\n\n新增对话：\n` : '') +
      older
        .map((item) => `${item.role === 'user' ? '用户' : '助手'}：${item.content.slice(0, 1200)}`)
        .join('\n\n')
    let summary = ''
    try {
      summary = await requestSummary(summarizer, source)
    } catch (error) {
      // 专用摘要模型失败时回退主模型，避免摘要能力被小模型拖垮
      if (summarizer.id === provider.id) throw error
      summary = await requestSummary(provider, source)
    }
    const until = older[older.length - 1].createdAt
    await aiSessionSummarySave(sid, summary, until)
    sessionSummary.value = { text: summary, until }
    summaryFails.value = 0
    void refreshSessions()
  } catch {
    // 摘要失败不阻断对话：继续按预算硬裁剪；连续 3 次失败就停掉自动摘要
    summaryFails.value += 1
    if (summaryFails.value >= 3) {
      summaryPaused.value = true
      notify.warning('自动摘要连续失败，已暂停；可稍后在会话菜单手动压缩')
    }
  } finally {
    summarizing.value = false
  }
}

/** Enter 直接发送；Shift / Ctrl / ⌘ + Enter 换行；输入法组字状态下不触发 */
function onEnter(event: KeyboardEvent) {
  if (event.shiftKey || event.ctrlKey || event.metaKey || event.isComposing) return
  event.preventDefault()
  void send()
}

/** 手动压缩：自动摘要暂停或用户想立刻省 token 时使用 */
function manualCompress() {
  if (sending.value) {
    notify.warning('请先等本轮回答结束或点击停止')
    return
  }
  void maybeSummarize(true)
}

/* ---------------- 会话列表 ---------------- */

/** 会话搜索：标题与消息正文都参与匹配，输入停顿 300ms 再查库 */
const sessionKeyword = ref('')
let sessionTimer: ReturnType<typeof setTimeout> | null = null

watch(sessionKeyword, () => {
  if (sessionTimer) clearTimeout(sessionTimer)
  sessionTimer = setTimeout(() => void refreshSessions(), 300)
})

async function refreshSessions() {
  try {
    const keyword = sessionKeyword.value.trim()
    sessions.value = await aiSessionList(keyword || undefined)
    if (sessionId.value && !sessions.value.some((item) => item.id === sessionId.value)) {
      // 搜索命中不到当前会话时不要丢会话，等清空搜索后再回填
      if (!keyword) sessionId.value = null
    }
  } catch {
    // 列表刷新失败不影响对话
  }
}

async function togglePin(session: AiSession) {
  if (!session.id) return
  try {
    await aiSessionPin(session.id, !session.pinned)
    await refreshSessions()
    notify.success(session.pinned ? '已取消置顶' : '已置顶')
  } catch (error) {
    notify.error(String(error))
  }
}

async function selectSession(id: number) {
  if (sending.value || id === sessionId.value) return
  sessionId.value = id
  quote.value = null
  resetRuns()
  try {
    const stored = await aiMessageList(id)
    messages.value = stored
      .filter((item) => item.role === 'user' || item.role === 'assistant')
      .map((item) => {
        const parsed = parseStoredQuote(item.content)
        return {
          id: nextId(),
          dbId: item.id,
          role: item.role === 'user' ? ('user' as const) : ('assistant' as const),
          content: parsed.body,
          quote: parsed.quote,
          reasoning: item.reasoning ?? undefined,
          // 历史里的「调工具前的过程说明」同样隐藏，只回放最终回答
          hidden:
            isControlMessage(item.content) ||
            (item.role === 'assistant' &&
              (isExecutionNote(item.content) || isToolLeakNote(item.content))),
          createdAt: item.createdAt,
        }
      })
  } catch (error) {
    notify.error(String(error))
    messages.value = []
  }
  const session = sessions.value.find((item) => item.id === id)
  if (session?.providerId) activeId.value = session.providerId
  // 摘要随会话恢复：被压缩的原文不再重复进上下文
  sessionSummary.value =
    session?.summary && session.summary.trim()
      ? { text: session.summary, until: session.summaryUntil ?? 0 }
      : null
  aborted.value = false
  summaryFails.value = 0
  summaryPaused.value = false
  nextTick(() => scrollBottom(true))
}

function resetContextState() {
  sessionSummary.value = null
  contextDropped.value = 0
  contextTokens.value = 0
  summaryFails.value = 0
  summaryPaused.value = false
  aborted.value = false
  nativeFallback.value = false
}

// 换模型后重新试一次原生 function calling：上一家网关不支持不代表下一家不支持
watch(activeId, () => {
  nativeFallback.value = false
})

function newSession() {
  if (sending.value) return
  sessionId.value = null
  messages.value = []
  resetRuns()
  resetContextState()
  autoDepth.value = 0
  repeats.value = 0
  forcedFinal.value = false
  callCache.clear()
  uncertainWrites.clear()
  input.value = ''
  quote.value = null
  dropHandoff()
}

function clearMessages() {
  Modal.confirm({
    title: '清空当前会话？',
    content: '会话内的消息会同时从数据库删除，工具调用流水会保留。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      if (sessionId.value) await aiMessageClear(sessionId.value)
      messages.value = []
      resetRuns()
      resetContextState()
      autoDepth.value = 0
      repeats.value = 0
      forcedFinal.value = false
      callCache.clear()
      void refreshSessions()
    },
  })
}

function openRename(session: AiSession) {
  renameTarget.value = session
  renameText.value = session.title
  renameOpen.value = true
}

async function confirmRename() {
  const target = renameTarget.value
  const title = renameText.value.trim()
  if (!target || !title) {
    notify.warning('标题不能为空')
    return
  }
  try {
    await aiSessionRename(target.id, title)
    renameOpen.value = false
    notify.success('已重命名')
    void refreshSessions()
  } catch (error) {
    notify.error(String(error))
  }
}

function removeSession(session: AiSession) {
  Modal.confirm({
    title: `删除会话「${session.title}」？`,
    content: '会话消息与该会话的工具流水会一并删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await aiSessionDelete(session.id)
      if (sessionId.value === session.id) newSession()
      notify.success('已删除')
      void refreshSessions()
    },
  })
}

/* ---------------- 会话导出 / 复盘时间线 ---------------- */

const timelineOpen = ref(false)
const timelineLoading = ref(false)
const timelineLogs = ref<AiToolLog[]>([])
const sessionExporting = ref(false)

async function loadToolLogs(sid: number): Promise<AiToolLog[]> {
  const page = await aiToolLogList({ sessionId: sid, days: 365, page: 1, pageSize: 200 })
  return [...page.items].sort((a, b) => a.createdAt - b.createdAt)
}

async function openTimeline() {
  if (!sessionId.value) {
    notify.info('当前还没有会话，先发送一条消息再看复盘时间线')
    return
  }
  timelineOpen.value = true
  timelineLoading.value = true
  try {
    timelineLogs.value = await loadToolLogs(sessionId.value)
  } catch (error) {
    notify.error(String(error))
    timelineLogs.value = []
  } finally {
    timelineLoading.value = false
  }
}

/** 按 traceId（同一次提问）分组；老数据没有 traceId 时退回按轮次分组 */
const timelineGroups = computed(() => {
  const map = new Map<string, AiToolLog[]>()
  for (const log of timelineLogs.value) {
    const key = log.traceId?.trim() || `round-${log.round ?? 0}`
    const list = map.get(key)
    if (list) list.push(log)
    else map.set(key, [log])
  }
  return [...map.entries()].map(([key, items]) => {
    const sorted = items.slice().sort((a, b) => a.createdAt - b.createdAt)
    return {
      key,
      round: sorted.find((item) => (item.round ?? 0) > 0)?.round ?? 0,
      startedAt: sorted[0]?.createdAt ?? 0,
      items: sorted,
      totalMs: sorted.reduce((sum, item) => sum + (item.durationMs || 0), 0),
      failed: sorted.filter((item) => !item.ok).length,
      writes: sorted.filter((item) => isWriteTool(item.tool) && item.ok).length,
    }
  })
})

/** AI-09：按本轮 traceId 回滚全部写操作（删除类不在可回滚范围内） */
const traceRollbackBusy = ref<string | null>(null)

function rollbackTrace(group: { key: string; writes: number }) {
  if (!group.key || group.key.startsWith('round-')) {
    notify.info('这轮记录没有 traceId（旧数据），只能逐条查看，无法按轮次回滚')
    return
  }
  Modal.confirm({
    title: '回滚本轮 AI 写操作？',
    content: `将把本轮 ${group.writes} 次写操作涉及的接口恢复到执行前的快照，并生成新的版本记录；删除类操作不在此范围内。`,
    okText: '回滚',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      traceRollbackBusy.value = group.key
      try {
        const result = await requestRevisionRollbackTrace(group.key)
        const skipped = result.skipped
        if (result.restored.length) {
          notify.success(
            `已回滚 ${result.restored.length} 个接口${skipped.length ? `，${skipped.length} 个跳过` : ''}`,
          )
        } else {
          notify.warning('没有可回滚的接口，可能已被删除或快照不完整')
        }
        if (skipped.length) {
          Modal.warning({
            title: '部分接口未回滚',
            content: skipped
              .map((item) => `#${item.requestId}（版本 ${item.revision}）：${item.reason}`)
              .join('\n'),
          })
        }
      } catch (error) {
        notify.error(String(error))
        throw error
      } finally {
        traceRollbackBusy.value = null
      }
    },
  })
}

function traceLabel(group: { key: string; round: number; startedAt: number }): string {
  const head = group.round ? `第 ${group.round} 轮` : '未标记轮次'
  return `${head} · ${fmtTime(group.startedAt)}`
}

/** 导出脱敏诊断包：参数与结果预览统一打码，便于贴给同事排查 */
async function exportDiagnostics() {
  if (!sessionId.value || !timelineLogs.value.length) {
    notify.info('当前会话还没有工具调用记录')
    return
  }
  const session = sessions.value.find((item) => item.id === sessionId.value)
  const stamp = formatFileStamp(Date.now())
  const target = await saveDialog({
    title: '导出 Agent 诊断包',
    defaultPath: `agent-trace-${stamp}.json`,
    filters: [{ name: 'JSON', extensions: ['json'] }],
  })
  if (!target) return
  const payload = {
    exportedAt: stamp,
    app: 'AstroMan',
    session: {
      id: sessionId.value,
      title: session?.title ?? '',
      provider: activeProvider.value?.name ?? '',
      model: activeProvider.value?.model ?? '',
    },
    groups: timelineGroups.value.map((group) => ({
      traceId: group.key,
      round: group.round,
      startedAt: group.startedAt,
      totalMs: group.totalMs,
      failed: group.failed,
      items: group.items.map((item) => ({
        tool: item.tool,
        label: toolLabel(item.tool),
        ok: item.ok,
        timeout: Boolean(item.timeout),
        durationMs: item.durationMs,
        at: item.createdAt,
        args: maskSensitive(item.args),
        resultPreview: maskSensitive(item.resultPreview),
      })),
    })),
  }
  try {
    await fileWriteText(target, JSON.stringify(payload, null, 2))
    notify.success('诊断包已导出（已脱敏）')
  } catch (error) {
    notify.error(String(error))
  }
}

function formatFileStamp(ms: number): string {
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}-${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
}

/** 导出当前会话为 Markdown（对话 + 工具调用），密钥统一打码 */
async function exportSession() {
  if (!sessionId.value) {
    notify.info('当前还没有会话可导出')
    return
  }
  const session = sessions.value.find((item) => item.id === sessionId.value)
  const stamp = formatFileStamp(Date.now())
  const target = await saveDialog({
    title: '导出会话',
    defaultPath: `chat-${stamp}.md`,
    filters: [{ name: 'Markdown', extensions: ['md'] }],
  })
  if (!target) return
  sessionExporting.value = true
  try {
    const stored = await aiMessageList(sessionId.value)
    const logs = await loadToolLogs(sessionId.value).catch(() => [] as AiToolLog[])
    const lines: string[] = [
      `# ${session?.title ?? '航仔 会话'}`,
      '',
      `- 导出时间：${new Date().toLocaleString()}`,
      `- 模型：${activeProvider.value?.name ?? '—'} / ${activeProvider.value?.model ?? '—'}`,
      `- 消息数：${stored.length}`,
      '',
      '## 对话',
      '',
    ]
    for (const msg of stored) {
      if (msg.role !== 'user' && msg.role !== 'assistant') continue
      lines.push(msg.role === 'user' ? '### 你' : '### 航仔', '', maskSensitive(msg.content.trim()), '')
    }
    if (logs.length) {
      lines.push('## 工具调用', '')
      for (const log of logs) {
        lines.push(
          `- ${fmtTime(log.createdAt)} · 第 ${log.round || '—'} 轮 · ${toolLabel(log.tool)} · ${
            log.ok ? '成功' : '失败'
          } · ${log.durationMs}ms`,
        )
      }
      lines.push('')
    }
    await fileWriteText(target, lines.join('\n'))
    notify.success('会话已导出为 Markdown')
  } catch (error) {
    notify.error(String(error))
  } finally {
    sessionExporting.value = false
  }
}

/* ---------------- 工具流水 ---------------- */

function openLogs() {
  void router.push({ name: 'ai-logs' })
}

/* ---------------- 模型用量统计 ---------------- */

const usageOpen = ref(false)
const usageLoading = ref(false)
const usageDays = ref(30)
const usage = ref<AiUsageStats | null>(null)
/** 调用明细：调一次记一条，分页查看 */
const usageTab = ref<'logs' | 'models'>('logs')
const usagePage = ref(1)
const usagePageSize = ref(20)
const usageTotal = ref(0)
const usageLogs = ref<AiUsageLog[]>([])

async function loadUsage() {
  usageLoading.value = true
  try {
    const [stats, page] = await Promise.all([
      aiUsageStats(usageDays.value),
      aiUsageLogs(usageDays.value, usagePage.value, usagePageSize.value),
    ])
    usage.value = stats
    usageLogs.value = page.items
    usageTotal.value = page.total
  } catch (error) {
    notify.error(`读取用量统计失败：${String(error)}`)
  } finally {
    usageLoading.value = false
  }
}

function openUsage() {
  // 用量统计已独立成页（/ai/usage），工具栏直接跳转，弹窗不再启用
  void router.push({ name: 'ai-usage' })
}

watch(usageDays, () => {
  if (!usageOpen.value) return
  usagePage.value = 1
  void loadUsage()
})

watch([usagePage, usagePageSize], () => {
  if (usageOpen.value) void loadUsage()
})

/** 每日趋势：柱高按 token 占比，悬停看当天调用次数 */
const usageSeries = computed(() => {
  const days = usage.value?.daily ?? []
  const max = Math.max(...days.map((item) => item.totalTokens), 1)
  const step = Math.max(1, Math.ceil(days.length / 10))
  return days.map((item, index) => ({
    day: item.day,
    calls: item.calls,
    height: item.totalTokens ? Math.max(4, Math.round((item.totalTokens / max) * 100)) : 0,
    showLabel: index % step === 0 || index === days.length - 1,
    title: `${item.day} · ${item.calls} 次 · ${item.totalTokens.toLocaleString()} token`,
  }))
})

function pct(value: number): string {
  return `${Math.round(value * 1000) / 10}%`
}

function fmtNum(value: number): string {
  return value >= 10000 ? `${(value / 1000).toFixed(1)}k` : String(value)
}

/** 统一使用完整的 YYYY-MM-DD HH:MM:SS，便于对账 */
function fmtDate(ms: number): string {
  if (!ms) return '—'
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  const ymd = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
  return `${ymd} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/** 每次模型调用的明细列 */
const usageLogColumns = [
  { title: '时间', dataIndex: 'createdAt', key: 'createdAt', width: 168 },
  { title: '服务商', dataIndex: 'providerName', key: 'provider', width: 130, ellipsis: true },
  { title: '模型', dataIndex: 'model', key: 'model', width: 180, ellipsis: true },
  { title: '输入 token', key: 'prompt', width: 100, align: 'right' as const },
  { title: '输出 token', key: 'completion', width: 100, align: 'right' as const },
  { title: '合计', key: 'total', width: 90, align: 'right' as const },
  { title: '耗时', key: 'latency', width: 88, align: 'right' as const },
  { title: '结果', key: 'ok', width: 200, ellipsis: true },
]

const usageColumns = [
  { title: '服务商', dataIndex: 'providerName', key: 'provider', width: 130, ellipsis: true },
  { title: '模型', dataIndex: 'model', key: 'model', width: 180, ellipsis: true },
  { title: '调用', dataIndex: 'calls', key: 'calls', width: 70, align: 'right' as const },
  { title: '成功率', key: 'rate', width: 88, align: 'right' as const },
  { title: '工具成功率', key: 'toolRate', width: 104, align: 'right' as const },
  { title: '输入 token', key: 'prompt', width: 104, align: 'right' as const },
  { title: '输出 token', key: 'completion', width: 104, align: 'right' as const },
  { title: '合计', key: 'total', width: 92, align: 'right' as const },
  { title: '平均耗时', key: 'latency', width: 96, align: 'right' as const },
  { title: '最近使用', key: 'last', width: 168, align: 'right' as const },
]

async function clearUsage() {
  Modal.confirm({
    title: '清空模型用量记录？',
    content: '会删除本机所有模型调用与 token 统计，不影响会话内容。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      const removed = await aiUsageClear(true)
      notify.success(`已清空 ${removed} 条用量记录`)
      await loadUsage()
    },
  })
}

/* ---------------- 展示辅助 ---------------- */

function fmtTime(ms: number): string {
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

const suggestions = [
  '用通俗的话讲讲 TPS、P95、错误率该怎么看',
  '列出本机所有项目，并按请求数量排序',
  '在第一个项目里新建登录接口，发送并分析返回',
  '给接口写一个前置签名脚本，再发一次验证',
  '导出某个接口最近 50 条执行记录为 CSV',
  '最近一次压测报告的关键指标怎么样',
  '帮我导入 Postman 集合并整理成集合结构',
]

const capabilities = [
  {
    icon: AstroBuddyIcon,
    title: '聊 & 答',
    items: ['技术概念讲解', '方案与文案撰写', '报错排查思路', '代码解释与改写'],
  },
  {
    icon: ThunderboltOutlined,
    title: '调接口',
    items: ['新建 / 修改 / 删除请求', '真实发送并解读响应', '写前置与后置脚本', '批量执行、启动压测'],
  },
  {
    icon: HistoryOutlined,
    title: '做工程',
    items: ['项目与集合管理', '环境变量读写与切换', '导入 Postman / OpenAPI / cURL', '导出项目与 OpenAPI'],
  },
  {
    icon: BarChartOutlined,
    title: '查数据',
    items: ['请求历史与慢请求', '压测报告指标', '数据大屏统计', '导出记录为 CSV / JSON'],
  },
  {
    icon: SettingOutlined,
    title: '有留痕',
    items: ['每次工具调用单独记录', '模型用量与成功率', '思考过程可展开', '敏感操作先确认'],
  },
]

onMounted(async () => {
  // 页面现场必须在首屏就绪时取走，否则切走再回来会被 TTL 清掉
  consumeHandoff()
  // 只预取服务商与会话列表；历史消息等用户点开某条会话再加载，
  // 否则长会话会在进入页面时一次性渲染，导致明显卡顿
  await Promise.all([reloadProviders(), refreshSessions(), loadContextMeta(), reloadAppSettings()])
  // 首屏顺手拉一次本月用量，预算提示不必等到第一次发送才出现
  void refreshMonthUsage()
})

// 被 keep-alive 缓存后重新进入：刷新会话列表并回到最新消息，任务本身不会中断
onActivated(() => {
  consumeHandoff()
  void refreshSessions()
  // 设置页可能改了预算 / 摘要模型，回来时重新读一次
  void reloadAppSettings().then(() => refreshMonthUsage())
  nextTick(() => scrollBottom(true))
})
</script>

<template>
  <div class="page ai-page">
    <PageHeader title="航仔" subtitle="内置 AI 助手 · 会话留存 · 可直接调用系统功能">
      <div class="head-actions">
        <a-switch v-model:checked="autoRun" size="small" />
        <span class="switch-label">自动执行工具</span>
        <a-divider type="vertical" />
        <a-button size="small" @click="openLogs">
          <template #icon><HistoryOutlined /></template>
          工具记录
        </a-button>
        <a-button size="small" @click="openUsage">
          <template #icon><BarChartOutlined /></template>
          用量统计
        </a-button>
        <a-button size="small" @click="serviceOpen = true">
          <template #icon><SettingOutlined /></template>
          服务配置
        </a-button>
      </div>
    </PageHeader>

    <div class="ai-layout">
      <aside class="session-side fm-card">
        <div class="side-head">
          <span class="side-title">历史会话</span>
          <a-tooltip title="新建对话">
            <a-button size="small" type="primary" ghost @click="newSession">
              <template #icon><PlusOutlined /></template>
            </a-button>
          </a-tooltip>
        </div>

        <a-input
          v-model:value="sessionKeyword"
          size="small"
          allow-clear
          class="session-search"
          placeholder="搜索标题或对话内容"
        >
          <template #prefix><SearchOutlined /></template>
        </a-input>

        <div class="session-list">
          <div
            v-for="item in sessions"
            :key="item.id"
            class="session-item"
            :class="{ active: item.id === sessionId, pinned: item.pinned }"
            @click="selectSession(item.id)"
          >
            <div class="session-main">
              <span class="session-name">
                <PushpinFilled v-if="item.pinned" class="pin-flag" />
                {{ item.title }}
              </span>
              <span class="session-meta">
                <template v-if="item.summary">已摘要 · </template>{{ item.messageCount }} 条 ·
                {{ fmtTime(item.updatedAt) }}
              </span>
            </div>
            <div class="session-ops" @click.stop>
              <a-tooltip :title="item.pinned ? '取消置顶' : '置顶会话'">
                <PushpinOutlined class="op" :class="{ on: item.pinned }" @click="togglePin(item)" />
              </a-tooltip>
              <a-tooltip v-if="item.id === sessionId" title="压缩较早对话为摘要">
                <HistoryOutlined class="op" @click="manualCompress" />
              </a-tooltip>
              <a-tooltip title="重命名">
                <EditOutlined class="op" @click="openRename(item)" />
              </a-tooltip>
              <a-tooltip title="删除">
                <DeleteOutlined class="op danger" @click="removeSession(item)" />
              </a-tooltip>
            </div>
          </div>
          <div v-if="!sessions.length" class="side-empty">
            {{ sessionKeyword.trim() ? '没有匹配的会话，试试更短的关键词' : '还没有会话记录。发送第一条消息后会自动建会话并保存在本机。' }}
          </div>
        </div>

        <div class="side-foot">
          <a-button block size="small" @click="openLogs">
            <template #icon><HistoryOutlined /></template>
            工具调用记录
          </a-button>
          <a-button block size="small" @click="serviceOpen = true">
            <template #icon><SettingOutlined /></template>
            模型服务配置
          </a-button>
        </div>
      </aside>

      <section class="chat fm-card">
        <div class="chat-head">
          <div class="hi">
            <div class="hi-avatar">航</div>
            <div class="hi-text">
              <div class="hi-name">航仔</div>
              <div class="hi-meta">
                {{ activeProvider ? `${activeProvider.name} · ${activeProvider.model}` : '尚未配置模型服务' }}
              </div>
            </div>
          </div>
          <div class="chat-tools">
            <a-select
              v-if="providers.length"
              v-model:value="activeId"
              size="small"
              class="provider-select"
              :options="providerOptions"
              placeholder="选择模型"
            />
            <a-tooltip title="发送前预览：本次会发给模型的提示词、历史与工具（不含密钥）">
              <a-button size="small" :disabled="!canChat" @click="openSendPreview">
                <template #icon><EyeOutlined /></template>
              </a-button>
            </a-tooltip>
            <a-tooltip title="导出当前会话为 Markdown（密钥打码）">
              <a-button
                size="small"
                :disabled="!sessionId || sessionExporting"
                :loading="sessionExporting"
                @click="exportSession"
              >
                <template #icon><DownloadOutlined /></template>
              </a-button>
            </a-tooltip>
            <a-tooltip title="复盘时间线：按轮次查看工具调用与耗时">
              <a-button size="small" :disabled="!sessionId" @click="openTimeline">
                <template #icon><ClockCircleOutlined /></template>
              </a-button>
            </a-tooltip>
            <a-button size="small" :disabled="!messages.length || sending" @click="clearMessages">
              清空会话
            </a-button>
          </div>
        </div>

        <div v-if="!canChat" class="chat-empty">
          <AstroBuddyIcon class="empty-icon" />
          <h3>还没有可用的模型</h3>
          <p>航仔 由你自己的 API 驱动，密钥经加密后只保存在本机。不配置则不提供任何 AI 能力。</p>
          <a-button type="primary" @click="serviceOpen = true">去配置服务</a-button>
        </div>

        <template v-else>
          <div ref="scrollRef" class="chat-scroll">
            <div v-if="messages.length === 0" class="chat-hint">
              <div class="hint-hero">
                <div class="hint-avatar"><AstroBuddyIcon /></div>
                <div class="hint-hero-text">
                  <div class="hint-title">你好，我是航仔</div>
                  <p class="hint-sub">
                    AstroMan 内置的 AI 助手，由你自己配置的模型驱动，密钥只加密保存在本机。
                    可以聊技术、答问题、写代码，也可以直接用一句话描述目标，我会自动挑工具去执行，并把结果讲清楚。
                  </p>
                </div>
              </div>

              <div class="hint-grid">
                <div v-for="cap in capabilities" :key="cap.title" class="hint-card">
                  <div class="hint-card-head">
                    <component :is="cap.icon" class="hint-card-icon" />
                    {{ cap.title }}
                  </div>
                  <ul>
                    <li v-for="line in cap.items" :key="line">{{ line }}</li>
                  </ul>
                </div>
              </div>

              <p class="hint-note">
                删除、批量执行、压测、导出这类会改动数据的操作，我会先请你确认再动手。
                每次工具调用都会单独留痕，随时能在「工具记录」里回看。
              </p>

              <div class="hint-chips">
                <button
                  v-for="item in suggestions"
                  :key="item"
                  type="button"
                  class="hint-chip"
                  @click="input = item"
                >
                  {{ item }}
                </button>
              </div>
            </div>

            <div v-for="item in visibleMessages" :key="item.id" class="bubble" :class="item.role">
              <div v-if="item.role === 'assistant'" class="bubble-avatar">航</div>
              <div class="bubble-main">
                <div class="bubble-meta">
                  <span class="bubble-name">{{ item.role === 'user' ? '你' : '航仔' }}</span>
                  <span class="bubble-time">{{ fmtTime(item.createdAt) }}</span>
                </div>
                <div class="bubble-body" :class="{ failed: item.failed }">
                  <div v-if="item.quote" class="bubble-quote">
                    <div class="bubble-quote-head">引用 {{ item.quote.role === 'user' ? '你' : '航仔' }}</div>
                    <div class="bubble-quote-text">{{ item.quote.content }}</div>
                  </div>
                  <template v-if="item.role === 'assistant' && (item.reasoning || toolParts(item).length)">
                    <a-collapse ghost class="think-panel" :bordered="false">
                      <a-collapse-panel key="process" :header="processTitle(item)">
                        <div v-if="item.reasoning" class="think-text">{{ item.reasoning }}</div>
                        <div
                          v-for="part in toolParts(item)"
                          :key="part.index"
                          class="tool-card"
                          :class="{ open: expandedTool === runKey(item.id, part.index) }"
                        >
                          <div class="tool-head" @click="toggleTool(item.id, part.index)">
                            <ThunderboltOutlined class="tool-icon" />
                            <span class="tool-title">{{ toolLabel(part.seg.tool) }}</span>
                            <span class="tool-name">{{ part.seg.tool }}</span>
                            <a-tag v-if="runState(item.id, part.index)?.status === 'ok'" color="green">成功</a-tag>
                            <a-tag v-else-if="runState(item.id, part.index)?.status === 'error'" color="red">失败</a-tag>
                            <a-tag v-else-if="runState(item.id, part.index)?.status === 'running'" color="blue">执行中</a-tag>
                            <a-tag
                              v-else-if="isDangerousTool(part.seg.tool)"
                              color="orange"
                              title="会修改或删除数据，需点击运行确认"
                            >
                              需确认
                            </a-tag>
                            <span v-if="runState(item.id, part.index)?.durationMs" class="tool-cost">
                              {{ runState(item.id, part.index)?.durationMs }}ms
                            </span>
                            <span class="flex-spacer" />
                            <span class="tool-toggle">
                              {{ expandedTool === runKey(item.id, part.index) ? '收起' : '详情' }}
                            </span>
                            <a-button
                              size="small"
                              type="text"
                              class="tool-run"
                              :disabled="sending || item.streaming"
                              :loading="runState(item.id, part.index)?.status === 'running'"
                              @click.stop="runToolSegment(item, part.index, part.seg)"
                            >
                              {{ runState(item.id, part.index)?.status && runState(item.id, part.index)?.status !== 'idle' ? '重新运行' : '运行' }}
                            </a-button>
                          </div>
                          <div class="tool-summary">{{ toolSummary(part.seg) }}</div>
                          <div
                            v-if="runState(item.id, part.index)?.summary"
                            class="tool-result-summary"
                          >
                            <span>{{ runState(item.id, part.index)?.summary }}</span>
                            <a-tag v-if="runState(item.id, part.index)?.truncated" color="orange">
                              已截断
                            </a-tag>
                          </div>
                          <div v-if="expandedTool === runKey(item.id, part.index)" class="tool-body">
                            <div class="tool-sub">请求参数</div>
                            <pre>{{ prettyArgs(part.seg.args) }}</pre>
                            <template v-if="runState(item.id, part.index)?.output">
                              <div class="tool-sub">返回结果</div>
                              <pre>{{ runState(item.id, part.index)?.output }}</pre>
                            </template>
                          </div>
                        </div>
                      </a-collapse-panel>
                    </a-collapse>
                  </template>

                  <template v-for="(seg, index) in contentParts(item)" :key="`c${index}`">
                    <template v-if="seg.type === 'text'">
                      <div v-if="item.role === 'assistant'" class="md" v-html="renderMarkdown(seg.text)" />
                      <span v-else class="seg-text">{{ seg.text }}</span>
                    </template>
                    <pre v-else-if="seg.type === 'code'" class="seg-code">{{ seg.code }}</pre>
                  </template>
                  <span v-if="item.streaming && !item.content" class="typing">思考中…</span>
                  <span v-else-if="item.streaming" class="typing-dot">▍</span>
                  <div v-if="item.statusText" class="bubble-status">{{ item.statusText }}</div>
                </div>
                <div
                  v-if="!item.streaming && item.content.trim()"
                  class="msg-actions"
                  :class="{ right: item.role === 'user' }"
                >
                  <a-tooltip title="复制">
                    <button type="button" class="msg-act" @click="copyMessage(item)">
                      <CopyOutlined />
                    </button>
                  </a-tooltip>
                  <a-tooltip :title="item.role === 'user' ? '引用这条提问' : '引用这条回复'">
                    <button type="button" class="msg-act" @click="quoteMessage(item)">
                      <CommentOutlined />
                    </button>
                  </a-tooltip>
                  <a-tooltip v-if="item.role === 'assistant'" title="重新生成">
                    <button
                      type="button"
                      class="msg-act"
                      :disabled="sending"
                      @click="regenerate(item)"
                    >
                      <ReloadOutlined />
                    </button>
                  </a-tooltip>
                  <a-tooltip v-else title="编辑重发">
                    <button
                      type="button"
                      class="msg-act"
                      :disabled="sending"
                      @click="editMessage(item)"
                    >
                      <EditOutlined />
                    </button>
                  </a-tooltip>
                  <a-tooltip title="删除这条消息">
                    <button
                      type="button"
                      class="msg-act danger"
                      :disabled="sending"
                      @click="removeMessage(item)"
                    >
                      <DeleteOutlined />
                    </button>
                  </a-tooltip>
                  <a-tag v-if="item.stopped" color="orange">已停止</a-tag>
                </div>
              </div>
            </div>
          </div>

          <div
            class="composer"
            :class="{ 'composer-drop': dropActive }"
            @dragover="onDragOverAttachments"
            @dragleave="onDragLeaveAttachments"
            @drop="onDropAttachments"
          >
            <div v-if="dropActive" class="drop-hint">松开以添加文本或图片附件</div>
            <div v-if="messages.length" class="ctx-bar">
              <span :class="{ 'ctx-warn': autoDepth >= MAX_TOOL_ROUNDS - 2 }">第 {{ roundLabel }} 轮</span>
              <span v-if="turnPromptTokens || turnOutputTokens">
                · 本轮 ≈ {{ fmtNum(turnPromptTokens + turnOutputTokens) }} token
              </span>
              <span v-if="turnCost > 0">
                · ≈ {{ fmtUsd(turnCost) }}
                <a-tooltip
                  :title="`按${modelPrice.label}估算（输入 ${fmtUnit(modelPrice.listInput, modelPrice.currency)}，输出 ${fmtUnit(modelPrice.listOutput, modelPrice.currency)}）${modelPrice.fallback ? '，模型未匹配价格表，按默认价估算' : ''}`"
                >
                  <span class="ctx-hint">{{ modelPrice.fallback ? '（默认估算）' : '' }}</span>
                </a-tooltip>
              </span>
              <span v-if="budgetInfo.level !== 'off'" :class="{ 'ctx-warn': budgetInfo.level !== 'ok' }">
                · 本月 {{ fmtUsd(budgetInfo.used) }} / {{ fmtUsd(budgetInfo.budget) }}
              </span>
              <span :class="{ 'ctx-warn': contextTight }">· 上下文 {{ fmtNum(contextTokens) }} / {{ fmtNum(contextBudget) }} token</span>
              <span v-if="contextDropped">· 已省略较早 {{ contextDropped }} 条</span>
              <span v-if="summarizing">· 正在压缩较早对话…</span>
              <span v-else-if="sessionSummary">· 已启用摘要</span>
              <span v-if="summaryPaused" class="ctx-warn">· 自动摘要已暂停</span>
              <span class="ctx-hint">· 窗口 {{ fmtNum(modelWindow) }}</span>
              <span v-if="contextWindowTiny" class="ctx-warn">· 窗口过小，建议换模型或精简提示词</span>
            </div>
            <div v-if="handoff" class="handoff-bar">
              <div class="handoff-head">
                <LinkOutlined />
                <span class="handoff-tag">来自{{ handoffLabel[handoff.source] }}</span>
                <span class="handoff-title" :title="handoff.title">{{ handoff.title }}</span>
                <span class="handoff-age">{{ handoffAge }}</span>
                <button type="button" class="handoff-go" @click="analyzeHandoff">直接分析</button>
                <button type="button" class="quote-close" title="移除页面现场" @click="dropHandoff">×</button>
              </div>
              <div class="handoff-hint">发送时自动带上这次页面现场，仅本轮生效，不会自动发送</div>
            </div>
            <div v-if="quote" class="quote-bar">
              <div class="quote-bar-head">
                <CommentOutlined />
                <span>引用 {{ quote.role === 'user' ? '你' : '航仔' }}</span>
                <button type="button" class="quote-close" title="取消引用" @click="clearQuote">×</button>
              </div>
              <div class="quote-bar-text">{{ quote.content }}</div>
            </div>
            <div v-if="attachments.length" class="attach-list">
              <span v-for="(file, index) in attachments" :key="file.path" class="attach-chip">
                <PaperClipOutlined />
                <span class="attach-name">{{ file.name }}</span>
                <span class="attach-size">
                  {{ fmtSize(file.content) }} · ≈ {{ fmtNum(estimateTokens(file.content)) }} token
                </span>
                <button type="button" class="attach-remove" @click="attachments.splice(index, 1)">×</button>
              </span>
              <span class="attach-total">附件合计 ≈ {{ fmtNum(attachTokens) }} token</span>
            </div>
            <div v-if="pendingImages.length" class="image-list">
              <span v-for="item in pendingImages" :key="item.id" class="image-chip">
                <img class="image-thumb" :src="item.preview" :alt="item.name" />
                <span class="image-meta">
                  <span class="image-name" :title="item.name">{{ item.name }}</span>
                  <span class="image-size">{{ formatImageSize(item.size) }} · 仅本轮</span>
                </span>
                <button type="button" class="attach-remove" title="移除图片" @click="removeImage(item.id)">×</button>
              </span>
              <span class="attach-total">图片合计 ≈ {{ fmtNum(imageTokens) }} token</span>
            </div>
            <div class="composer-row">
              <a-button
                type="text"
                class="attach-btn"
                :disabled="sending"
                title="上传文本文件作为上下文"
                @click="pickAttachments"
              >
                <template #icon><PaperClipOutlined /></template>
              </a-button>
              <a-tooltip v-if="visionEnabled" title="添加图片（最多 3 张，仅随本轮发送）">
                <a-button
                  type="text"
                  class="attach-btn"
                  :disabled="sending"
                  :loading="imageBusy"
                  @click="pickImages"
                >
                  <template #icon><PictureOutlined /></template>
                </a-button>
              </a-tooltip>
              <input
                ref="imageInput"
                type="file"
                accept="image/jpeg,image/png,image/webp"
                multiple
                class="image-input"
                @change="onImagePicked"
              />
              <a-textarea
                ref="inputRef"
                v-model:value="input"
                :auto-size="{ minRows: 2, maxRows: 6 }"
                :disabled="sending"
                placeholder="和航仔聊聊，或让它帮你操作软件…（Enter 发送，Shift + Enter 换行，可直接粘贴截图）"
                @keydown.enter="onEnter"
                @keydown.ctrl.enter.prevent="send"
                @paste="onPasteImages"
              />
              <a-button
                v-if="sending"
                danger
                class="send-btn"
                @click="stop"
              >
                <template #icon><StopOutlined /></template>
                停止
              </a-button>
              <a-button
                v-else
                type="primary"
                class="send-btn"
                :disabled="!input.trim() && !attachments.length && !pendingImages.length"
                @click="send"
              >
                <template #icon><SendOutlined /></template>
                发送
              </a-button>
            </div>
          </div>
        </template>
      </section>
    </div>

    <!-- 模型用量统计 -->
    <a-modal
      v-model:open="usageOpen"
      title="模型用量统计"
      :footer="null"
      :width="980"
      wrap-class-name="usage-modal"
    >
      <div class="usage-head">
        <div class="usage-cards">
          <div class="usage-card">
            <span class="usage-label">调用次数</span>
            <span class="usage-value">{{ usage?.totalCalls ?? 0 }}</span>
          </div>
          <div class="usage-card">
            <span class="usage-label">调用成功率</span>
            <span class="usage-value">{{ pct(usage?.successRate ?? 0) }}</span>
          </div>
          <div class="usage-card">
            <span class="usage-label">工具成功率</span>
            <span class="usage-value">{{ pct(usage?.toolSuccessRate ?? 0) }}</span>
          </div>
          <div class="usage-card">
            <span class="usage-label">总 token</span>
            <span class="usage-value">{{ fmtNum(usage?.totalTokens ?? 0) }}</span>
          </div>
          <div class="usage-card">
            <span class="usage-label">平均耗时</span>
            <span class="usage-value">{{ usage?.avgLatencyMs ?? 0 }} ms</span>
          </div>
        </div>
        <div class="usage-tools">
          <a-radio-group v-model:value="usageDays" size="small" button-style="solid">
            <a-radio-button :value="7">近 7 天</a-radio-button>
            <a-radio-button :value="30">近 30 天</a-radio-button>
            <a-radio-button :value="90">近 90 天</a-radio-button>
            <a-radio-button :value="365">近 1 年</a-radio-button>
          </a-radio-group>
          <a-button size="small" @click="loadUsage">刷新</a-button>
          <a-button size="small" danger @click="clearUsage">清空</a-button>
        </div>
      </div>

      <p class="usage-tip">
        成功率 = 模型调用正常结束的占比；工具成功率 = 该模型会话内工具调用成功的占比。
        输入 / 输出 token 优先取厂商返回的 usage，未返回时按字符数估算（中文约 1 字 1 token）。
      </p>

      <div class="usage-chart">
        <div class="usage-chart-head">
          <span>每日 token 消耗</span>
          <span class="sub-text">共 {{ usageSeries.length }} 天 · 悬停查看当天调用次数</span>
        </div>
        <div class="usage-chart-body">
          <div
            v-for="point in usageSeries"
            :key="point.day"
            class="usage-chart-col"
            :title="point.title"
          >
            <div class="usage-chart-bar" :style="{ height: `${point.height}%` }"></div>
            <span class="usage-chart-tick">{{ point.showLabel ? point.day.slice(5) : '' }}</span>
          </div>
          <span v-if="!usageSeries.length" class="usage-chart-empty">暂无数据</span>
        </div>
      </div>

      <a-tabs v-model:activeKey="usageTab" size="small">
        <a-tab-pane key="logs" :tab="`调用记录（${usageTotal}）`">
          <a-table
            :data-source="usageLogs"
            :columns="usageLogColumns"
            row-key="id"
            size="small"
            :loading="usageLoading"
            :pagination="false"
            :scroll="{ x: 1000 }"
            :locale="{ emptyText: '所选时间范围内还没有调用记录' }"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'createdAt'">{{ fmtDate(record.createdAt) }}</template>
              <template v-else-if="column.key === 'prompt'">
                {{ record.promptTokens.toLocaleString() }}
              </template>
              <template v-else-if="column.key === 'completion'">
                {{ record.completionTokens.toLocaleString() }}
              </template>
              <template v-else-if="column.key === 'total'">
                <strong>{{ record.totalTokens.toLocaleString() }}</strong>
              </template>
              <template v-else-if="column.key === 'latency'">{{ record.latencyMs }} ms</template>
              <template v-else-if="column.key === 'ok'">
                <span :class="record.ok ? 'ok-text' : 'warn-text'">
                  {{ record.ok ? '成功' : '失败' }}
                </span>
                <span v-if="record.error" class="sub-text"> {{ record.error }}</span>
              </template>
            </template>
          </a-table>
          <div class="usage-pager">
            <span class="sub-text">共 {{ usageTotal }} 条，每条代表一次模型调用</span>
            <a-pagination
              v-model:current="usagePage"
              v-model:page-size="usagePageSize"
              size="small"
              :total="usageTotal"
              :page-size-options="['20', '50', '100']"
              show-size-changer
              show-less-items
            />
          </div>
        </a-tab-pane>

        <a-tab-pane key="models" tab="模型汇总">
          <a-table
            :data-source="usage?.items ?? []"
            :columns="usageColumns"
            row-key="providerId"
            size="small"
            :loading="usageLoading"
            :pagination="false"
            :scroll="{ x: 1100 }"
            :locale="{ emptyText: '所选时间范围内还没有调用记录' }"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'rate'">
                <span :class="record.successRate >= 0.9 ? 'ok-text' : 'warn-text'">
                  {{ pct(record.successRate) }}
                </span>
                <span class="sub-text"> ({{ record.okCalls }}/{{ record.calls }})</span>
              </template>
              <template v-else-if="column.key === 'toolRate'">
                <span v-if="record.toolCalls">{{ pct(record.toolSuccessRate) }}</span>
                <span v-else class="sub-text">—</span>
              </template>
              <template v-else-if="column.key === 'prompt'">
                {{ record.promptTokens.toLocaleString() }}
              </template>
              <template v-else-if="column.key === 'completion'">
                {{ record.completionTokens.toLocaleString() }}
              </template>
              <template v-else-if="column.key === 'total'">
                <strong>{{ record.totalTokens.toLocaleString() }}</strong>
              </template>
              <template v-else-if="column.key === 'latency'">{{ record.avgLatencyMs }} ms</template>
              <template v-else-if="column.key === 'last'">{{ fmtDate(record.lastUsedAt) }}</template>
            </template>
          </a-table>
        </a-tab-pane>
      </a-tabs>
    </a-modal>

    <!-- 模型服务配置 -->
    <a-drawer v-model:open="serviceOpen" title="模型服务配置" placement="right" :width="440">
      <div class="service-body">
        <p class="service-tip">
          支持 OpenAI 兼容协议（DeepSeek / Kimi / 智谱 / 通义 / 硅基流动 / OpenRouter / Ollama）、Claude 与 Gemini。
          密钥使用 AES-256-GCM 加密后写入本机数据库。
        </p>

        <div v-if="!providers.length" class="side-empty">
          还没有配置任何模型服务，点击下方按钮添加。
        </div>

        <ul v-else class="provider-list">
          <li
            v-for="item in providers"
            :key="item.id ?? item.name"
            class="provider-item"
            :class="{ active: item.id === activeId }"
            @click="activeId = item.id ?? null"
          >
            <div class="provider-main">
              <span class="provider-name">
                {{ item.name }}
                <a-tag v-if="item.isDefault" color="blue">默认</a-tag>
              </span>
              <span class="provider-meta">{{ item.kind }} · {{ item.model }}</span>
            </div>
            <div class="provider-actions" @click.stop>
              <a-button type="link" size="small" :loading="testing === item.id" @click="testProvider(item)">
                测试
              </a-button>
              <a-button type="link" size="small" @click="openEdit(item)">编辑</a-button>
              <a-button type="link" size="small" danger @click="removeProvider(item)">
                <template #icon><DeleteOutlined /></template>
              </a-button>
            </div>
          </li>
        </ul>
      </div>

      <template #footer>
        <a-button block type="primary" @click="openCreate">
          <template #icon><PlusOutlined /></template>
          添加模型服务
        </a-button>
      </template>
    </a-drawer>

    <!-- 添加 / 编辑服务 -->
    <a-modal
      v-model:open="modalOpen"
      :title="isEdit ? '编辑模型服务' : '添加模型服务'"
      :confirm-loading="saving"
      ok-text="保存"
      cancel-text="取消"
      width="620px"
      wrap-class-name="provider-modal"
      @ok="saveProvider"
    >
      <a-form layout="vertical" class="provider-form">
        <a-form-item label="快速填充">
          <a-space wrap :size="[6, 6]">
            <button
              v-for="preset in presets"
              :key="preset.label"
              type="button"
              class="preset"
              :class="{ active: form.baseUrl === preset.baseUrl }"
              @click="applyPreset(preset.label)"
            >
              {{ preset.label }}
            </button>
          </a-space>
        </a-form-item>
        <a-form-item label="名称" required>
          <a-input v-model:value="form.name" placeholder="例如 公司 DeepSeek" />
        </a-form-item>
        <a-form-item label="协议">
          <a-select v-model:value="form.kind">
            <a-select-option value="openai">OpenAI 兼容</a-select-option>
            <a-select-option value="anthropic">Anthropic（Claude）</a-select-option>
            <a-select-option value="gemini">Google Gemini</a-select-option>
          </a-select>
        </a-form-item>
        <a-form-item label="Base URL" required>
          <a-input v-model:value="form.baseUrl" placeholder="https://api.deepseek.com" />
        </a-form-item>
        <a-form-item required>
          <template #label>
            <span class="label-row">
              <span>模型</span>
              <a-button
                type="link"
                size="small"
                :loading="fetchingModels"
                :disabled="!isEdit"
                :title="isEdit ? '从服务商 /models 拉取真实模型清单' : '保存后可拉取服务商模型清单'"
                @click="fetchModelList"
              >
                拉取模型
              </a-button>
            </span>
          </template>
          <a-auto-complete
            v-model:value="form.model"
            :options="modelOptions"
            :filter-option="filterModelOption"
            :allow-clear="true"
            placeholder="点击下拉选择，或直接输入模型名"
          >
            <template #notFoundContent>
              <span class="dialog-hint">列表里没有匹配项，可直接输入模型名</span>
            </template>
          </a-auto-complete>
          <template #extra>
            <span class="dialog-hint">{{ modelHint }}</span>
          </template>
        </a-form-item>
        <a-form-item label="API Key">
          <a-input-password
            v-model:value="form.apiKey"
            :placeholder="isEdit && editingKeyMask ? `已保存：${editingKeyMask}（留空不修改）` : 'sk-...'"
          />
          <template #extra>
            <span class="dialog-hint">
              {{ isEdit ? '密钥只以掩码回显，输入新值才会覆盖；留空则沿用原 Key' : '密钥加密后保存在本机，不会上传' }}
            </span>
          </template>
        </a-form-item>
        <a-form-item label="图片识别">
          <a-checkbox v-model:checked="form.supportsVision">支持图片输入</a-checkbox>
          <template #extra>
            <span class="dialog-hint">
              勾选后可在「记事本 → 截图生成」里用这个模型识别截图；纯文本模型保持不勾
            </span>
          </template>
        </a-form-item>
        <a-row :gutter="12">
          <a-col :span="12">
            <a-form-item label="最大输出 token">
              <a-input-number
                v-model:value="form.maxTokens"
                :min="0"
                :max="32768"
                :step="256"
                style="width: 100%"
              />
              <template #extra>
                <span class="dialog-hint">0 = 按协议默认；Claude 未配置时按 4096</span>
              </template>
            </a-form-item>
          </a-col>
          <a-col :span="12">
            <a-form-item label="请求超时（秒）">
              <a-input-number
                v-model:value="form.timeoutSecs"
                :min="10"
                :max="3600"
                :step="10"
                style="width: 100%"
              />
              <template #extra>
                <span class="dialog-hint">默认 300 秒，长回答或慢模型可调大</span>
              </template>
            </a-form-item>
          </a-col>
        </a-row>
        <a-form-item label="上下文窗口（token）">
          <a-input-number
            v-model:value="form.contextWindow"
            :min="0"
            :max="10000000"
            :step="1000"
            style="width: 100%"
          />
          <template #extra>
            <span class="dialog-hint">
              0 = 按模型名自动识别（当前按 {{ fmtNum(formWindow) }} 预留）；本地小模型或私有网关请填真实窗口，程序据此裁剪历史并预留输出
            </span>
          </template>
        </a-form-item>
        <a-row :gutter="12">
          <a-col :span="12">
            <a-form-item label="原生工具调用">
              <a-select
                v-model:value="form.nativeTools"
                :options="[
                  { value: -1, label: '自动（按协议默认）' },
                  { value: 1, label: '开启（function calling）' },
                  { value: 0, label: '关闭（文本协议）' },
                ]"
              />
              <template #extra>
                <span class="dialog-hint">网关不支持原生调用时自动回退文本协议</span>
              </template>
            </a-form-item>
          </a-col>
          <a-col :span="12">
            <a-form-item label="计价币种">
              <a-select
                v-model:value="form.currency"
                :options="[
                  { value: 'USD', label: '美元 USD' },
                  { value: 'CNY', label: '人民币 CNY' },
                ]"
              />
              <template #extra>
                <span class="dialog-hint">仅影响成本展示</span>
              </template>
            </a-form-item>
          </a-col>
        </a-row>
        <a-row :gutter="12">
          <a-col :span="12">
            <a-form-item label="输入单价 / 百万 token">
              <a-input-number v-model:value="form.inputPrice" :min="0" :step="0.1" style="width: 100%" />
              <template #extra>
                <span class="dialog-hint">0 = 使用内置价格表</span>
              </template>
            </a-form-item>
          </a-col>
          <a-col :span="12">
            <a-form-item label="输出单价 / 百万 token">
              <a-input-number v-model:value="form.outputPrice" :min="0" :step="0.1" style="width: 100%" />
              <template #extra>
                <span class="dialog-hint">0 = 使用内置价格表</span>
              </template>
            </a-form-item>
          </a-col>
        </a-row>
        <a-form-item>
          <template #label>
            <span class="label-row">
              <span>Temperature</span>
              <span class="temp-value">{{ Number(form.temperature ?? 0.3).toFixed(1) }}</span>
            </span>
          </template>
          <a-slider v-model:value="form.temperature" :min="0" :max="2" :step="0.1" />
          <div class="temp-axis">
            <span>精确</span>
            <span>平衡</span>
            <span>发散</span>
          </div>
        </a-form-item>
        <a-form-item class="default-item">
          <a-checkbox v-model:checked="form.isDefault">设为默认服务</a-checkbox>
          <span class="default-desc">进入航仔 时自动使用该服务，也可在顶部随时切换</span>
        </a-form-item>
      </a-form>
    </a-modal>

    <!-- 重命名会话 -->
    <a-modal
      v-model:open="renameOpen"
      title="重命名会话"
      ok-text="保存"
      cancel-text="取消"
      @ok="confirmRename"
    >
      <a-input v-model:value="renameText" placeholder="会话标题" @keydown.enter="confirmRename" />
    </a-modal>

    <!-- AI-13 发送前预览：只读展示本轮将发送的内容 -->
    <a-modal v-model:open="previewOpen" title="发送前预览" :footer="null" :width="620">
      <a-spin :spinning="previewLoading">
        <div class="preview-list">
          <div v-for="row in previewRows" :key="row.label" class="preview-row">
            <div class="preview-label">{{ row.label }}</div>
            <div class="preview-value">{{ row.value }}</div>
          </div>
        </div>
        <ul class="preview-notes">
          <li v-for="note in previewNotes" :key="note">{{ note }}</li>
        </ul>
        <div class="preview-foot">
          <span>以上为估算值，预览不会真正发送请求。</span>
          <a-button size="small" @click="previewOpen = false">关闭</a-button>
        </div>
      </a-spin>
    </a-modal>

    <!-- 复盘时间线 -->
    <a-drawer
      v-model:open="timelineOpen"
      title="复盘时间线"
      placement="right"
      :width="560"
      class="trace-drawer"
    >
      <template #extra>
        <a-button size="small" :disabled="!timelineLogs.length" @click="exportDiagnostics">
          <template #icon><DownloadOutlined /></template>
          导出诊断包
        </a-button>
      </template>
      <a-spin :spinning="timelineLoading">
        <a-empty v-if="!timelineLoading && !timelineLogs.length" description="当前会话还没有工具调用记录" />
        <div v-for="group in timelineGroups" :key="group.key" class="trace-group">
          <div class="trace-head">
            <span class="trace-title">{{ traceLabel(group) }}</span>
            <span class="trace-meta">
              共 {{ group.items.length }} 次 · {{ group.totalMs }}ms
              <a-tag v-if="group.failed" color="red">{{ group.failed }} 次失败</a-tag>
              <a-button
                v-if="group.writes"
                size="small"
                danger
                :loading="traceRollbackBusy === group.key"
                @click="rollbackTrace(group)"
              >
                回滚本轮写操作
              </a-button>
            </span>
          </div>
          <div
            v-for="log in group.items"
            :key="log.id"
            class="trace-item"
            :class="{ bad: !log.ok }"
          >
            <span class="trace-dot" />
            <div class="trace-body">
              <div class="trace-tool">
                {{ toolLabel(log.tool) }}
                <span class="trace-name">{{ log.tool }}</span>
                <a-tag v-if="log.timeout" color="orange">超时</a-tag>
                <a-tag v-else :color="log.ok ? 'green' : 'red'">{{ log.ok ? '成功' : '失败' }}</a-tag>
              </div>
              <div class="trace-preview">{{ maskSensitive(log.resultPreview) || '（无输出）' }}</div>
              <div class="trace-foot">{{ fmtTime(log.createdAt) }} · {{ log.durationMs }}ms</div>
            </div>
          </div>
        </div>
      </a-spin>
    </a-drawer>
  </div>
</template>

<style scoped>
.ai-page {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.switch-label {
  font-size: 12px;
  color: var(--fm-text-muted, var(--fm-muted));
}

.ai-layout {
  display: grid;
  grid-template-columns: 240px minmax(0, 1fr);
  gap: 14px;
  align-items: stretch;
  flex: 1;
  min-height: 0;
}

/* ---------- 会话侧栏 ---------- */

.session-side {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 12px;
  gap: 10px;
}

.side-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.side-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.session-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-right: 2px;
}

.session-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 9px;
  border: 1px solid transparent;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease;
}

.session-item:hover {
  background: var(--fm-surface-alt);
}

.session-item.active {
  border-color: color-mix(in srgb, var(--fm-primary) 30%, transparent);
  background: var(--fm-primary-soft);
}

.session-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.session-name {
  font-size: 12.5px;
  font-weight: 500;
  color: var(--fm-text);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.session-meta {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.session-ops {
  display: flex;
  gap: 8px;
  opacity: 0;
  transition: opacity 0.15s ease;
}

.session-item:hover .session-ops {
  opacity: 1;
}

.op {
  font-size: 12px;
  color: var(--fm-muted);
  cursor: pointer;
}

.op:hover {
  color: var(--fm-primary);
}

.op.danger:hover {
  color: var(--fm-danger);
}

.op.on {
  color: var(--fm-primary);
}

.session-search {
  padding: 0 0 6px;
}

.session-search .ant-input-affix-wrapper {
  font-size: 12px;
}

.pin-flag {
  font-size: 11px;
  color: var(--fm-primary);
  flex-shrink: 0;
}

.side-empty {
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-muted);
  padding: 8px 2px;
}

.side-foot {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: 8px;
  border-top: 1px solid var(--fm-border);
}

/* ---------- 对话区 ---------- */

.chat {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.chat-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--fm-border);
}

.hi {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.hi-avatar {
  width: 32px;
  height: 32px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.3px;
  color: #fff;
  background: linear-gradient(135deg, var(--fm-primary), color-mix(in srgb, var(--fm-primary) 55%, var(--fm-purple)));
}

.hi-text {
  min-width: 0;
}

.hi-name {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--fm-text);
}

.hi-meta {
  font-size: 11px;
  color: var(--fm-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.chat-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.provider-select {
  width: 210px;
}

.chat-empty {
  margin: auto;
  text-align: center;
  padding: 40px 24px;
  max-width: 460px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: center;
}

.chat-empty .empty-icon {
  font-size: 30px;
  color: var(--fm-primary);
  opacity: 0.7;
}

.chat-empty h3 {
  margin: 0;
  font-size: 15px;
}

.chat-empty p {
  margin: 0 0 4px;
  font-size: 12.5px;
  line-height: 1.8;
  color: var(--fm-muted);
}

.chat-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.chat-hint {
  padding: 22px 8px 8px;
  color: var(--fm-muted);
  font-size: 12.5px;
  line-height: 1.85;
}

.hint-hero {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}

.hint-avatar {
  width: 38px;
  height: 38px;
  flex-shrink: 0;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  color: #fff;
  background: linear-gradient(135deg, var(--fm-primary), color-mix(in srgb, var(--fm-primary) 50%, var(--fm-purple)));
  box-shadow: 0 6px 14px color-mix(in srgb, var(--fm-primary) 22%, transparent);
}

.hint-hero-text {
  min-width: 0;
}

.hint-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--fm-text);
  margin-bottom: 6px;
}

.hint-sub {
  margin: 0;
  max-width: 620px;
}

.hint-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
  gap: 10px;
  margin-top: 16px;
}

.hint-card {
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface-alt);
  padding: 10px 12px;
}

.hint-card-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
  margin-bottom: 6px;
}

.hint-card-icon {
  font-size: 13px;
  color: var(--fm-primary);
}

.hint-card ul {
  margin: 0;
  padding-left: 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  line-height: 1.7;
}

.hint-note {
  margin: 14px 0 0;
  padding: 8px 11px;
  border-left: 2px solid color-mix(in srgb, var(--fm-primary) 45%, transparent);
  background: var(--fm-surface-alt);
  border-radius: 0 8px 8px 0;
  font-size: 12px;
}

.hint-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
}

.hint-chip {
  padding: 6px 11px;
  border: 1px solid var(--fm-border);
  border-radius: 999px;
  background: var(--fm-surface);
  color: var(--fm-text-2);
  font-size: 12px;
  cursor: pointer;
  transition: border-color 0.15s ease, color 0.15s ease;
}

.hint-chip:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.bubble {
  display: flex;
  gap: 10px;
  align-items: flex-start;
}

.bubble.user {
  flex-direction: row-reverse;
}

.bubble-avatar {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10.5px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(135deg, var(--fm-primary), color-mix(in srgb, var(--fm-primary) 55%, var(--fm-purple)));
  margin-top: 18px;
}

.bubble-main {
  min-width: 0;
  max-width: min(820px, 92%);
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.bubble.user .bubble-main {
  align-items: flex-end;
  max-width: min(720px, 84%);
}

.bubble-meta {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.bubble.user .bubble-meta {
  flex-direction: row-reverse;
}

.bubble-name {
  font-weight: 600;
  color: var(--fm-text-2);
}

.bubble-body {
  font-size: 13px;
  line-height: 1.78;
  word-break: break-word;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  max-width: 100%;
}

.bubble.user .bubble-body {
  padding: 9px 13px;
  border-radius: 12px;
  background: var(--fm-primary-soft);
  border: 1px solid color-mix(in srgb, var(--fm-primary) 22%, transparent);
}

.bubble-body.failed {
  border-left: 2px solid color-mix(in srgb, var(--fm-danger) 55%, transparent);
  padding-left: 9px;
}

.bubble-status {
  font-size: 11.5px;
  color: var(--fm-muted);
}

.msg-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-top: 2px;
  opacity: 0.5;
  transition: opacity 0.15s ease;
}

.bubble:hover .msg-actions,
.msg-actions:focus-within {
  opacity: 1;
}

.msg-actions.right {
  justify-content: flex-end;
}

.msg-act {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12.5px;
  color: var(--fm-muted);
  cursor: pointer;
  transition: color 0.14s ease, background 0.14s ease;
}

.msg-act:hover {
  color: var(--fm-primary);
  background: var(--fm-surface-alt);
}

.msg-act.danger:hover {
  color: var(--fm-danger);
  background: color-mix(in srgb, var(--fm-danger) 10%, transparent);
}

.msg-act:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.bubble-quote {
  border-left: 2px solid color-mix(in srgb, var(--fm-primary) 55%, transparent);
  background: var(--fm-surface-alt);
  border-radius: 6px;
  padding: 6px 9px;
}

.bubble-quote-head {
  font-size: 10.5px;
  color: var(--fm-muted);
  margin-bottom: 2px;
}

.bubble-quote-text {
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.handoff-bar {
  border: 1px solid color-mix(in srgb, var(--fm-primary) 35%, var(--fm-border));
  border-left: 2px solid var(--fm-primary);
  background: color-mix(in srgb, var(--fm-primary) 7%, var(--fm-surface-alt));
  border-radius: 6px;
  padding: 6px 9px;
  margin-bottom: 8px;
}

.handoff-head {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 11px;
  color: var(--fm-muted);
}

.handoff-tag {
  flex: none;
  color: var(--fm-primary);
  font-weight: 600;
}

.handoff-title {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--fm-text);
}

.handoff-age {
  flex: none;
}

.handoff-go {
  flex: none;
  margin-left: auto;
  padding: 0 6px;
  height: 20px;
  border: 1px solid color-mix(in srgb, var(--fm-primary) 40%, transparent);
  border-radius: 4px;
  background: transparent;
  color: var(--fm-primary);
  font-size: 11px;
  cursor: pointer;
}

.handoff-go:hover {
  background: color-mix(in srgb, var(--fm-primary) 10%, transparent);
}

.handoff-hint {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-muted);
}

.quote-bar {
  border: 1px solid var(--fm-border);
  border-left: 2px solid color-mix(in srgb, var(--fm-primary) 55%, transparent);
  background: var(--fm-surface-alt);
  border-radius: 6px;
  padding: 6px 9px;
  margin-bottom: 8px;
}

.quote-bar-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--fm-muted);
}

.quote-close {
  margin-left: auto;
  border: none;
  background: transparent;
  font-size: 14px;
  line-height: 1;
  color: var(--fm-muted);
  cursor: pointer;
}

.quote-close:hover {
  color: var(--fm-danger);
}

.quote-bar-text {
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.seg-text {
  white-space: pre-wrap;
}

/* ---------- Markdown 正文 ---------- */

.md {
  min-width: 0;
  max-width: 100%;
}

.md :deep(p) {
  margin: 0 0 8px;
}

.md :deep(p:last-child) {
  margin-bottom: 0;
}

.md :deep(h1),
.md :deep(h2),
.md :deep(h3),
.md :deep(h4) {
  margin: 12px 0 6px;
  font-weight: 600;
  line-height: 1.45;
  color: var(--fm-text);
}

.md :deep(h1) {
  font-size: 16.5px;
}

.md :deep(h2) {
  font-size: 15px;
}

.md :deep(h3),
.md :deep(h4) {
  font-size: 13.5px;
}

.md :deep(ul),
.md :deep(ol) {
  margin: 2px 0 8px;
  padding-left: 20px;
}

.md :deep(li) {
  margin: 2px 0;
}

.md :deep(li::marker) {
  color: var(--fm-muted);
}

.md :deep(strong) {
  font-weight: 600;
  color: var(--fm-text);
}

.md :deep(blockquote) {
  margin: 8px 0;
  padding: 2px 10px;
  border-left: 3px solid var(--fm-border-strong);
  color: var(--fm-muted);
}

.md :deep(a) {
  color: var(--fm-primary);
  text-decoration: none;
}

.md :deep(a:hover) {
  text-decoration: underline;
}

.md :deep(hr) {
  border: none;
  border-top: 1px solid var(--fm-border);
  margin: 12px 0;
}

.md :deep(code) {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 11.5px;
}

.md :deep(pre) {
  margin: 8px 0;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  overflow-x: auto;
  max-height: 340px;
}

.md :deep(pre code) {
  padding: 0;
  background: transparent;
  font-size: 11.5px;
  line-height: 1.68;
  white-space: pre;
}

.md :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
  font-size: 12px;
  display: block;
  overflow-x: auto;
  max-width: 100%;
}

.md :deep(th),
.md :deep(td) {
  border: 1px solid var(--fm-border);
  padding: 6px 10px;
  text-align: left;
  white-space: nowrap;
}

.md :deep(th) {
  background: var(--fm-surface-alt);
  font-weight: 600;
}

.seg-code {
  margin: 0;
  padding: 9px 11px;
  border-radius: 7px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.65;
  overflow-x: auto;
  max-height: 320px;
  white-space: pre-wrap;
}

.typing {
  font-size: 12px;
  color: var(--fm-muted);
}

.typing-dot {
  color: var(--fm-primary);
  animation: blink 1s steps(2, start) infinite;
  font-size: 12px;
}

@keyframes blink {
  to {
    visibility: hidden;
  }
}

/* ---------- 工具卡片 ---------- */

.tool-card {
  border: 1px solid color-mix(in srgb, var(--fm-primary) 24%, transparent);
  border-radius: 8px;
  background: var(--fm-surface);
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.tool-head {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  cursor: pointer;
  user-select: none;
}

.tool-icon {
  color: var(--fm-primary);
  font-size: 12px;
}

.tool-title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
  white-space: nowrap;
}

.tool-name {
  font-size: 10px;
  color: var(--fm-muted);
  font-family: 'JetBrains Mono', Consolas, monospace;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
}

.tool-toggle {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.tool-card.open .tool-toggle,
.tool-head:hover .tool-toggle {
  color: var(--fm-primary);
}

.tool-run {
  font-size: 11px;
  height: 22px;
  padding: 0 6px;
}

.tool-summary {
  font-size: 11px;
  color: var(--fm-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  max-width: 100%;
  font-family: 'JetBrains Mono', Consolas, monospace;
}

/* 结果摘要：先给一句话结论，完整内容收在「详情」里 */
.tool-result-summary {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  font-size: 11.5px;
  color: var(--fm-text);
  font-weight: 500;
}

.tool-result-summary span {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.flex-spacer {
  flex: 1;
}

.tool-cost {
  font-size: 10.5px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.tool-body {
  border-top: 1px dashed var(--fm-border);
  padding-top: 7px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tool-sub {
  font-size: 10.5px;
  font-weight: 600;
  color: var(--fm-muted);
}

.tool-body pre {
  margin: 0;
  max-height: 200px;
  overflow: auto;
  padding: 7px 9px;
  border-radius: 6px;
  background: var(--fm-surface-alt);
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}

/* ---------- 输入区 ---------- */

.composer {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-top: 1px solid var(--fm-border);
}

.composer-drop {
  outline: 1.5px dashed #009688;
  outline-offset: -6px;
  background: rgba(0, 150, 136, 0.06);
}

.drop-hint {
  position: absolute;
  top: 8px;
  left: 50%;
  z-index: 2;
  transform: translateX(-50%);
  padding: 2px 10px;
  border-radius: 10px;
  background: rgba(0, 150, 136, 0.92);
  color: #fff;
  font-size: 11.5px;
  pointer-events: none;
}

.composer-row {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}

.ctx-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  font-size: 11px;
  color: var(--fm-muted);
}

.ctx-warn {
  color: var(--fm-danger);
}

.ctx-hint {
  color: var(--fm-muted);
  opacity: 0.75;
}

.attach-btn {
  height: 34px;
  width: 34px;
  padding: 0;
  color: var(--fm-muted);
}

.attach-list {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.attach-total {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.attach-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 260px;
  padding: 3px 6px 3px 8px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  color: var(--fm-text-2);
}

.attach-name {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.attach-size {
  font-size: 10.5px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.attach-remove {
  border: none;
  background: transparent;
  color: var(--fm-muted);
  cursor: pointer;
  font-size: 13px;
  line-height: 1;
  padding: 0 1px;
}

.attach-remove:hover {
  color: #ff4d4f;
}

.image-list {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.image-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 240px;
  padding: 3px 6px 3px 4px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  color: var(--fm-text-2);
}

.image-thumb {
  width: 34px;
  height: 34px;
  border-radius: 4px;
  object-fit: cover;
  background: #fff;
}

.image-meta {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.3;
}

.image-name {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.image-size {
  font-size: 10.5px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.image-input {
  display: none;
}

/* ---------- 思考 / 执行过程（默认折叠） ---------- */

.think-panel {
  margin-bottom: 6px;
}

.think-panel :deep(.ant-collapse-header) {
  padding: 2px 0 !important;
  font-size: 11.5px;
  color: var(--fm-muted) !important;
}

.think-panel :deep(.ant-collapse-content-box) {
  padding: 6px 0 0 14px !important;
}

.think-text {
  margin-bottom: 8px;
  padding: 8px 10px;
  border-left: 2px solid var(--fm-border);
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-muted);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 220px;
  overflow: auto;
}

.send-btn {
  height: 34px;
}

/* ---------- 服务配置 ---------- */

.service-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.service-tip {
  margin: 0;
  padding: 9px 11px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-alt);
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

/* ---------- 模型用量统计 ---------- */

.usage-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
  flex-wrap: wrap;
}

.usage-cards {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.usage-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 104px;
  padding: 8px 12px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-alt);
}

.usage-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.usage-value {
  font-size: 17px;
  font-weight: 600;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.usage-tools {
  display: flex;
  align-items: center;
  gap: 8px;
}

.usage-tip {
  margin: 10px 0 12px;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.usage-chart {
  margin-bottom: 12px;
  padding: 10px 12px 6px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-alt);
}

.usage-chart-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  font-size: 12px;
  color: var(--fm-text);
  margin-bottom: 8px;
}

.usage-chart-body {
  position: relative;
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 120px;
}

.usage-chart-col {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-end;
  height: 100%;
  min-width: 2px;
}

.usage-chart-bar {
  width: 100%;
  max-width: 22px;
  border-radius: 3px 3px 0 0;
  background: linear-gradient(180deg, #60a5fa, #2563eb);
  transition: opacity 0.15s ease;
}

.usage-chart-col:hover .usage-chart-bar {
  opacity: 0.75;
}

.usage-chart-tick {
  height: 14px;
  margin-top: 4px;
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.usage-chart-empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.usage-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-top: 10px;
  flex-wrap: wrap;
}

.ok-text {
  color: #16a34a;
}

.warn-text {
  color: #d97706;
}

.sub-text {
  font-size: 11px;
  color: var(--fm-muted);
}

.provider-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.provider-item {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 11px 12px 11px 14px;
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface);
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease, box-shadow 0.15s ease;
}

.provider-item::before {
  content: '';
  position: absolute;
  left: 0;
  top: 50%;
  width: 3px;
  height: 0;
  border-radius: 0 3px 3px 0;
  background: var(--fm-primary);
  transform: translateY(-50%);
  transition: height 0.18s ease;
}

.provider-item:hover {
  border-color: color-mix(in srgb, var(--fm-primary) 45%, var(--fm-border));
  box-shadow: 0 4px 14px rgba(15, 23, 42, 0.06);
}

.provider-item.active {
  border-color: var(--fm-primary);
  background: color-mix(in srgb, var(--fm-primary) 6%, var(--fm-surface));
}

.provider-item.active::before {
  height: 22px;
}

.provider-main {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  flex: 1;
}

.provider-name {
  font-size: 13.5px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--fm-text);
}

.provider-meta {
  font-size: 11px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-top: 0;
  flex-shrink: 0;
  opacity: 0.6;
  transition: opacity 0.15s ease;
}

.provider-item:hover .provider-actions {
  opacity: 1;
}

.provider-actions :deep(.ant-btn-link) {
  padding-inline: 6px;
  height: 24px;
  font-size: 12px;
}

.provider-form :deep(.ant-form-item) {
  margin-bottom: 14px;
}

.provider-form :deep(.ant-form-item-label > label) {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.provider-form :deep(.ant-input),
.provider-form :deep(.ant-input-affix-wrapper),
.provider-form :deep(.ant-select-selector) {
  border-radius: 8px;
}

.provider-form :deep(.ant-input),
.provider-form :deep(.ant-input-affix-wrapper) {
  padding-block: 7px;
}

.preset {
  display: inline-flex;
  align-items: center;
  height: 26px;
  padding: 0 11px;
  border: 1px solid var(--fm-border);
  border-radius: 13px;
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  transition: all 0.15s ease;
}

.preset:hover {
  border-color: color-mix(in srgb, var(--fm-primary) 55%, var(--fm-border));
  color: var(--fm-primary);
  background: color-mix(in srgb, var(--fm-primary) 7%, var(--fm-surface));
}

.preset.active {
  border-color: var(--fm-primary);
  background: color-mix(in srgb, var(--fm-primary) 12%, var(--fm-surface));
  color: var(--fm-primary);
  font-weight: 600;
}

.label-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.temp-value {
  padding: 0 6px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  color: var(--fm-primary);
  font-size: 11px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.temp-axis {
  display: flex;
  justify-content: space-between;
  margin-top: -6px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.default-item :deep(.ant-form-item-control-input-content) {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.default-desc {
  font-size: 11px;
  color: var(--fm-muted);
}

.dialog-hint {
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.provider-form :deep(.ant-select),
.provider-form :deep(.ant-select-selector) {
  width: 100%;
}

.provider-modal :deep(.ant-modal-header) {
  margin-bottom: 14px;
}

.provider-modal :deep(.ant-modal-title) {
  font-size: 15px;
  font-weight: 600;
}

.provider-modal :deep(.ant-modal-footer) {
  margin-top: 6px;
  padding-top: 12px;
  border-top: 1px solid var(--fm-border);
}

.provider-modal :deep(.ant-modal-footer .ant-btn) {
  border-radius: 8px;
  min-width: 76px;
}

/* ---------- 复盘时间线 ---------- */

.trace-group + .trace-group {
  margin-top: 16px;
}

.trace-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 6px;
  border-bottom: 1px dashed var(--fm-border);
}

.trace-title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
}

.trace-meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--fm-muted);
}

.trace-item {
  display: flex;
  gap: 10px;
  padding: 8px 0 8px 4px;
  position: relative;
}

.trace-item + .trace-item {
  border-top: 1px solid var(--fm-border);
}

.trace-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  margin-top: 6px;
  flex-shrink: 0;
  background: var(--fm-primary);
}

.trace-item.bad .trace-dot {
  background: var(--fm-danger);
}

.trace-body {
  min-width: 0;
  flex: 1;
}

.trace-tool {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--fm-text);
  font-weight: 500;
}

.trace-name {
  font-family: 'JetBrains Mono', 'Consolas', monospace;
  font-size: 10.5px;
  font-weight: 400;
  color: var(--fm-muted);
}

.trace-preview {
  margin-top: 3px;
  font-size: 11px;
  line-height: 1.65;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 96px;
  overflow: hidden;
}

.trace-foot {
  margin-top: 3px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

@media (max-width: 1080px) {
  .ai-layout {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: 160px minmax(0, 1fr);
  }

  .session-list {
    flex-direction: row;
    overflow-x: auto;
  }

  .session-item {
    min-width: 180px;
  }
}

/* ---------- AI-13 发送前预览 ---------- */

.preview-list {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--fm-border, rgba(0, 0, 0, 0.08));
  border-radius: 4px;
  overflow: hidden;
}

.preview-row {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr);
  gap: 10px;
  padding: 8px 12px;
  font-size: 12.5px;
}

.preview-row + .preview-row {
  border-top: 1px solid var(--fm-border, rgba(0, 0, 0, 0.06));
}

.preview-label {
  color: var(--fm-muted);
}

.preview-value {
  color: var(--fm-text, inherit);
  word-break: break-all;
}

.preview-notes {
  margin: 12px 0 0;
  padding-left: 18px;
  color: var(--fm-muted);
  font-size: 12px;
  line-height: 1.7;
}

.preview-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 16px;
  color: var(--fm-muted);
  font-size: 12px;
}
</style>
