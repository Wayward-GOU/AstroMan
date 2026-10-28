<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Modal, message as notify } from 'ant-design-vue'
import {
  BarChartOutlined,
  DeleteOutlined,
  EditOutlined,
  HistoryOutlined,
  PaperClipOutlined,
  PlusOutlined,
  RobotOutlined,
  SendOutlined,
  SettingOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import PageHeader from '@/components/PageHeader.vue'
import {
  aiChat,
  aiMessageAdd,
  aiMessageClear,
  aiMessageList,
  aiProviderDelete,
  aiProviderList,
  aiProviderSave,
  aiProviderTest,
  aiSessionCreate,
  aiSessionDelete,
  aiSessionList,
  aiSessionRename,
  aiUsageClear,
  aiUsageLogs,
  aiUsageStats,
  fileReadText,
} from '@/api'
import { isDangerousTool, TOOL_CATALOG, executeTool, toolLabel, type ToolResult } from '@/utils/aiTools'
import { renderMarkdown } from '@/utils/markdown'
import type { AiMessage, AiProvider, AiSession, AiUsageLog, AiUsageStats } from '@/types/models'

const router = useRouter()

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
  isDefault: false,
})

const form = reactive<AiProvider>(blank())
const isEdit = computed(() => Boolean(form.id))

const presets = [
  { label: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  { label: 'DeepSeek', kind: 'openai', baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  { label: 'Kimi', kind: 'openai', baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  { label: '智谱', kind: 'openai', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash' },
  { label: '通义千问', kind: 'openai', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-plus' },
  { label: '硅基流动', kind: 'openai', baseUrl: 'https://api.siliconflow.cn/v1', model: 'Qwen/Qwen2.5-7B-Instruct' },
  { label: 'OpenRouter', kind: 'openai', baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o-mini' },
  { label: 'Ollama 本地', kind: 'openai', baseUrl: 'http://127.0.0.1:11434/v1', model: 'qwen2.5:7b' },
  { label: 'Claude', kind: 'anthropic', baseUrl: 'https://api.anthropic.com', model: 'claude-3-5-sonnet-latest' },
  { label: 'Gemini', kind: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com', model: 'gemini-2.0-flash' },
]

/** 各服务商常用模型：下拉直接选，也可以手动输入未列出的模型名 */
const presetModels: Record<string, string[]> = {
  OpenAI: ['gpt-4o-mini', 'gpt-4o', 'gpt-4.1-mini', 'gpt-4.1', 'o3-mini', 'o3', 'o4-mini'],
  DeepSeek: ['deepseek-chat', 'deepseek-reasoner'],
  Kimi: ['moonshot-v1-8k', 'moonshot-v1-32k', 'moonshot-v1-128k', 'kimi-latest'],
  智谱: ['glm-4-flash', 'glm-4-air', 'glm-4-plus', 'glm-4-long'],
  通义千问: ['qwen-turbo', 'qwen-plus', 'qwen-max', 'qwen-long'],
  硅基流动: [
    'Qwen/Qwen2.5-7B-Instruct',
    'Qwen/Qwen2.5-72B-Instruct',
    'deepseek-ai/DeepSeek-V3',
    'deepseek-ai/DeepSeek-R1',
  ],
  OpenRouter: [
    'openai/gpt-4o-mini',
    'anthropic/claude-3.5-sonnet',
    'google/gemini-2.0-flash-001',
    'deepseek/deepseek-chat',
  ],
  'Ollama 本地': ['qwen2.5:7b', 'qwen2.5:14b', 'llama3.1:8b', 'deepseek-r1:7b', 'gemma2:9b'],
  Claude: [
    'claude-3-5-haiku-latest',
    'claude-3-5-sonnet-latest',
    'claude-3-7-sonnet-latest',
    'claude-sonnet-4-5',
    'claude-opus-4-1',
  ],
  Gemini: [
    'gemini-1.5-flash',
    'gemini-1.5-pro',
    'gemini-2.0-flash',
    'gemini-2.5-flash',
    'gemini-2.5-pro',
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

/** 模型下拉是否展开：聚焦即展示全部候选，方便直接选 */
const modelOpen = ref(false)

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

const modelOptions = computed(() => {
  const label = matchedPreset.value?.label
  const list = label ? presetModels[label] ?? [] : []
  return [...list].sort(naturalCompare).map((value) => ({ value }))
})

const modelHint = computed(() => {
  const preset = matchedPreset.value
  if (!preset) return '未识别服务商，可直接输入服务商文档里的模型名'
  const count = modelOptions.value.length
  return `已列出 ${preset.label} 的 ${count} 个常用模型，也可直接输入其他模型名`
})

function filterModelOption(input: string, option: { value?: string | number }): boolean {
  if (!input) return true
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
  modelOpen.value = false
  if (providers.value.length === 0) form.isDefault = true
  modalOpen.value = true
}

function openEdit(provider: AiProvider) {
  Object.assign(form, { ...provider, apiKey: '' })
  modelOpen.value = false
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
  saving.value = true
  try {
    const saved = await aiProviderSave({ ...form })
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
const providerOptions = computed(() =>
  providers.value
    .filter((item) => item.id)
    .map((item) => ({ label: `${item.name} · ${item.model}`, value: item.id as number })),
)

/* ---------------- 会话 ---------------- */

const sessions = ref<AiSession[]>([])
const sessionId = ref<number | null>(null)
const messages = ref<ChatItem[]>([])
/** 只渲染最终回答：中间那段「先查项目 → 再查请求」的执行过程全部隐藏 */
const visibleMessages = computed(() => messages.value.filter((item) => !item.hidden))
const input = ref('')
const sending = ref(false)
const autoRun = ref(true)
const autoDepth = ref(0)
/** 与系统提示词保持一致：连续工具轮次上限 5，超过就停下来汇总 */
const MAX_TOOL_ROUNDS = 5
/** 连续「一轮多个工具」的次数，模型屡劝不改时提前终止，避免用户卡死 */
const rejections = ref(0)
const scrollRef = ref<HTMLElement | null>(null)

const renameOpen = ref(false)
const renameTarget = ref<AiSession | null>(null)
const renameText = ref('')

interface ChatItem {
  id: number
  role: 'user' | 'assistant'
  content: string
  /** 模型的思考过程（reasoning），默认折叠不展示 */
  reasoning?: string
  streaming?: boolean
  failed?: boolean
  /** 纯执行过程（调工具前的「我先获取…」）不展示，只留最终回答 */
  hidden?: boolean
  createdAt: number
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

interface Segment {
  type: 'text' | 'code' | 'tool'
  text: string
  lang: string
  code: string
  tool: string
  args: Record<string, unknown>
}

const FENCE_RE = /```([a-zA-Z0-9_-]*)[ \t]*\r?\n([\s\S]*?)```/g

function emptySegment(type: Segment['type']): Segment {
  return { type, text: '', lang: '', code: '', tool: '', args: {} }
}

function tryJson(text: string): Record<string, unknown> | null {
  try {
    const value = JSON.parse(text)
    return value && typeof value === 'object' ? (value as Record<string, unknown>) : null
  } catch {
    return null
  }
}

/** 工具段（保留原始下标，供 runState 使用） */
function toolParts(item: ChatItem) {
  return parseSegments(item.content)
    .map((seg, index) => ({ seg, index }))
    .filter(({ seg }) => seg.type === 'tool')
}

/** 正文段（文本 + 普通代码块；附件块只喂给模型，不在气泡里铺开） */
function contentParts(item: ChatItem): Segment[] {
  return parseSegments(item.content).filter(
    (seg) => seg.type !== 'tool' && !(seg.type === 'code' && seg.lang.startsWith('fbin-file')),
  )
}

function processTitle(item: ChatItem): string {
  const count = toolParts(item).length
  const head = item.reasoning ? '思考过程' : '执行过程'
  return count ? `${head} · ${count} 个工具调用` : head
}

const segmentCache = new Map<string, Segment[]>()

/**
 * 兜底：部分模型会漏出原生工具语法（如 `<| DSLM | invoke name="fbin-tool">`），
 * 这里把其中的 `{"tool":...}` JSON 抽出来还原成标准 fbin-tool 块，避免整段 XML 直接展示。
 */
function liftNativeCalls(content: string): string {
  if (!content.includes('<|')) return content
  const calls: { start: number; end: number; json: string }[] = []
  let i = 0
  while (i < content.length) {
    if (content[i] !== '{') {
      i += 1
      continue
    }
    let depth = 0
    let inString = false
    let escaped = false
    let end = -1
    for (let j = i; j < content.length; j += 1) {
      const ch = content[j]
      if (inString) {
        if (escaped) escaped = false
        else if (ch === '\\') escaped = true
        else if (ch === '"') inString = false
        continue
      }
      if (ch === '"') inString = true
      else if (ch === '{') depth += 1
      else if (ch === '}') {
        depth -= 1
        if (depth === 0) {
          end = j
          break
        }
      }
    }
    if (end < 0) break
    const text = content.slice(i, end + 1)
    const parsed = tryJson(text)
    if (parsed && typeof parsed.tool === 'string' && parsed.tool.trim()) {
      calls.push({ start: i, end: end + 1, json: text })
      i = end + 1
      continue
    }
    i += 1
  }
  if (!calls.length) return content

  let out = ''
  let cursor = 0
  for (const call of calls) {
    out += content.slice(cursor, call.start)
    out += '\n\n```fbin-tool\n' + call.json + '\n```\n\n'
    cursor = call.end
  }
  out += content.slice(cursor)
  // 去掉残留的原生标签，避免在气泡里显示乱码
  return out.replace(/<\/?\|[^>]*>/g, '')
}

function parseSegmentsRaw(content: string): Segment[] {
  const segments: Segment[] = []
  let cursor = 0
  let match: RegExpExecArray | null
  FENCE_RE.lastIndex = 0

  while ((match = FENCE_RE.exec(content))) {
    const [full, lang, body] = match
    if (match.index > cursor) {
      segments.push({ ...emptySegment('text'), text: content.slice(cursor, match.index) })
    }
    const fence = body.trim()
    const language = lang.toLowerCase()
    const parsed = tryJson(fence)
    const isToolCall =
      Boolean(parsed) &&
      (language === 'fbin-tool' ||
        language === 'tool' ||
        (language === 'json' && typeof parsed?.tool === 'string'))
    if (isToolCall && parsed) {
      const tool = String(parsed.tool ?? '')
      const rawArgs = parsed.args
      const args =
        rawArgs && typeof rawArgs === 'object'
          ? (rawArgs as Record<string, unknown>)
          : Object.fromEntries(Object.entries(parsed).filter(([key]) => key !== 'tool'))
      // 空 tool 名多半是模型流式输出时的残块，直接丢弃，不展示也不执行
      if (tool) {
        segments.push({ ...emptySegment('tool'), tool, args })
      }
    } else if ((language === 'http-request' || language === 'json') && parsed && parsed.url && parsed.method) {
      // 兼容旧格式：把裸请求自动映射成 send_request
      segments.push({
        ...emptySegment('tool'),
        tool: 'send_request',
        args: {
          method: String(parsed.method).toUpperCase(),
          url: parsed.url,
          headers: parsed.headers,
          bodyType: parsed.bodyType ?? (parsed.body ? 'json' : 'none'),
          body: parsed.body,
        },
      })
    } else {
      segments.push({ ...emptySegment('code'), lang: lang || 'text', code: fence })
    }
    cursor = match.index + full.length
  }

  let tail = content.slice(cursor)
  // 流式输出时未闭合的围栏 / 原生工具标签不展示，避免半截内容闪烁
  const pending = tail.indexOf('```')
  if (pending >= 0) tail = tail.slice(0, pending)
  const native = tail.indexOf('<|')
  if (native >= 0) tail = tail.slice(0, native)
  if (tail.trim()) segments.push({ ...emptySegment('text'), text: tail })
  return segments
}

/** 分段结果按内容缓存，模板里多处调用不会重复解析（长会话渲染提速） */
function parseSegments(content: string): Segment[] {
  const normalized = content.includes('<|') ? liftNativeCalls(content) : content
  const cached = segmentCache.get(normalized)
  if (cached) return cached
  const segments = parseSegmentsRaw(normalized)
  if (segmentCache.size > 200) segmentCache.clear()
  segmentCache.set(normalized, segments)
  return segments
}

interface ToolRun {
  tool: string
  args: Record<string, unknown>
  status: 'idle' | 'running' | 'ok' | 'error'
  output: string
  durationMs: number
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

// 身份、核心执行流程与业务规则由 Rust 后端注入
// （src-tauri/src/prompts/xiao_hi_system_prompt.md，include_str! 编译期嵌入），
// 前端这里只补充「工具协议」，不得与后端规则冲突：每轮最多 1 个工具调用、最多连续 5 轮。
const SYSTEM_PROMPT = `以下是 FbinHttp 的工具调用协议，与全局规则同时生效，冲突时以全局规则为准。

需要调用系统能力时，输出一个 fenced 代码块，语言标记为 fbin-tool，内容是严格 JSON：

\`\`\`fbin-tool
{"tool":"project_list","args":{}}
\`\`\`

可用工具（名称：用途；参数）：
${TOOL_CATALOG}

调用规则：
1. 每次思考后最多输出 1 个 fbin-tool 块，拿到该工具的真实结果后再决定下一步；禁止一轮并发输出多个工具块，也禁止使用原生工具语法（DSML / tool_call / XML 标签）。
2. 需要真实数据时先调用工具，不要编造结果；参数不确定时先向用户提问。
3. send_request 会真的发出网络请求，不确定目标地址、项目或环境时先向用户确认。
4. 新建请求前，先用 collection_tree 或 request_options 找到 collectionId。
5. 收到工具结果后，用中文说明：结论、状态码含义、耗时是否异常、响应结构要点、下一步建议。
6. 修改 / 删除 / 压测 / 批量执行 / 导出文件 / 写脚本类工具（project_update、project_delete、collection_rename、collection_delete、request_update、request_script_save、request_delete、env_var_save、stress_start、stress_stop、run_batch、history_export、project_export、openapi_export、report_export）属于敏感操作：必须先说明将影响哪个对象、影响范围，再输出工具块等用户点「运行」确认；一次只做一件，删除前先用列表工具确认对象 id 与名称。
7. 导入 / 导出类工具需要本机绝对路径（如 D:\\data\\api.json）；用户没给路径就先问，不要编造路径。
   - 用户要「某个接口最近 N 条执行记录」→ 先 request_search 拿到 id，再 history_export({ requestId, limit })，可选 projectId 过滤。
   - 用户要「某个项目最近 N 条请求记录」→ 先 project_list 拿 projectId，再 history_export({ projectId, limit })。不指定 format 时默认 CSV（Excel 可直接打开）。
   - 写前后置脚本用 request_script_save，不要用 request_update 覆盖整条请求（它会把没提到的字段清空）。
8. 压测默认从保守参数起步（首次 10 并发 × 30 秒），需要更大压力时先征得用户同意。
9. 只在需要操作软件时输出工具块，日常对话直接回答；全部使用中文，简洁可执行。

## 脚本引擎（写前置 / 后置脚本时必须遵守）

脚本是同步 JavaScript，由应用内置引擎执行，不能用 require / import / fetch / XMLHttpRequest / async-await / 定时器；不要写顶层 return，用条件判断控制流程。

前置脚本可用对象：
- request.method / request.url / request.headers（对象）/ request.params / request.body
- vars.get(key) / vars.set(key, value) / vars.unset(key) / vars.has(key) / vars.all()
- env.变量名 读取当前环境变量（只读）
- console.log(...) 输出到脚本日志；cipher 加解密；rsa 签名

后置脚本额外可用：
- response.status / response.headers（对象）/ response.text() / response.json() / response.durationMs
- test(名称, () => { assert(条件, '失败说明') }) 断言
- 提取值存变量：vars.set('token', response.json().data.token)

cipher 用法：cipher.md5(str)、cipher.sha256(str)、cipher.hmacSha256(str, key)、cipher.base64.encode(str)、cipher.aes.encrypt(text, key)。rsa 用 rsa.sign(text, privateKey)。

写脚本时：先说明思路与提取的字段，脚本用 request_script_save 保存（会做语法检查），再用 send_request 验证后置脚本是否按预期提取。脚本体不超过 30 行，关键步骤加中文注释。`

function toHistory(extra: AiMessage[] = []): AiMessage[] {
  const history: AiMessage[] = [{ role: 'system', content: SYSTEM_PROMPT }]
  for (const item of messages.value) {
    const content = item.content.trim()
    if (!content) continue
    history.push({ role: item.role, content })
  }
  const trimmed = history.slice(1).slice(-20)
  return [history[0], ...trimmed, ...extra]
}

function scrollBottom(force = false) {
  const el = scrollRef.value
  if (!el) return
  const near = el.scrollHeight - el.scrollTop - el.clientHeight < 140
  if (force || near) el.scrollTop = el.scrollHeight
}

async function ask(extra: AiMessage[] = []) {
  if (!activeProvider.value?.id) return
  sending.value = true
  const item: ChatItem = {
    id: nextId(),
    role: 'assistant',
    content: '',
    streaming: true,
    createdAt: Date.now(),
  }
  messages.value.push(item)
  scrollBottom(true)

  try {
    await aiChat(
      {
        providerId: activeProvider.value.id,
        messages: toHistory(extra),
        temperature: activeProvider.value.temperature ?? 0.3,
      },
      (event) => {
        if (event.kind === 'delta' && event.text) {
          item.content += event.text
          scrollBottom()
        } else if (event.kind === 'reasoning' && event.text) {
          item.reasoning = (item.reasoning ?? '') + event.text
          scrollBottom()
        } else if (event.kind === 'error') {
          item.content += `${item.content ? '\n\n' : ''}> ${event.text}`
          item.failed = true
        }
      },
    )
  } catch (error) {
    if (!item.content) item.content = String(error)
    item.failed = true
  } finally {
    item.streaming = false
    sending.value = false
    scrollBottom()
  }

  if (item.content.trim() && sessionId.value) {
    // 硬规则 7：思考内容随助手消息一起落库，便于回溯 Agent 行为
    await aiMessageAdd(sessionId.value, 'assistant', item.content, item.reasoning).catch(
      () => undefined,
    )
    void refreshSessions()
  }
  await afterReply(item)
}

/**
 * 历史消息里只含一个工具块 + 少量过程说明（如「我先获取项目列表」）的，视为执行过程。
 * 最终回答通常不含 fbin-tool 块，因此重开会话时也能保持「只看结果」。
 */
function isExecutionNote(content: string): boolean {
  if (!content.includes('```fbin-tool')) return false
  const rest = content.replace(/```fbin-tool[\s\S]*?```/g, '').trim()
  return rest.length < 200
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
  const tools = registerRuns(item)
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

  // 硬规则 1：最多连续 5 次工具调用
  if (autoDepth.value >= MAX_TOOL_ROUNDS) {
    notifyTerminated()
    return
  }

  autoDepth.value += 1

  // 硬规则 4：高危工具不直接执行，弹窗等用户确认；结果（含拒绝）回灌模型
  if (isDangerousTool(seg.tool)) {
    await runToolSegment(item, index, seg)
    return
  }

  // 自动执行的这一轮只是过程说明，隐藏掉，只保留最终汇总
  item.hidden = true
  const result = await execTool(item, index, seg)
  await ask([
    {
      role: 'user',
      content: `【工具 ${seg.tool} 的真实执行结果】\n${result.output}\n\n请据此继续下一步（每次最多调用 1 个工具）。`,
    },
  ])
}

async function execTool(item: ChatItem, index: number, seg: Segment): Promise<ToolResult> {
  const key = runKey(item.id, index)
  let run = toolRuns[key]
  if (!run) {
    run = { tool: seg.tool, args: seg.args, status: 'idle', output: '', durationMs: 0 }
    toolRuns[key] = run
  }
  run.status = 'running'
  run.output = ''
  const result = await executeTool(seg.tool, seg.args, sessionId.value)
  run.status = result.ok ? 'ok' : 'error'
  run.output = result.output
  run.durationMs = result.durationMs
  return result
}

async function runToolSegment(item: ChatItem, index: number, seg: Segment) {
  if (runState(item.id, index)?.status === 'running') return
  if (isDangerousTool(seg.tool)) {
    const confirmed = await new Promise<boolean>((resolve) => {
      Modal.confirm({
        title: `确认执行「${toolLabel(seg.tool)}」？`,
        content: '该操作会修改或删除软件里的数据（或启动压测），执行后无法撤销。',
        okText: '执行',
        okType: 'danger',
        cancelText: '取消',
        onOk: () => resolve(true),
        onCancel: () => resolve(false),
      })
    })
    if (!confirmed) {
      // 硬规则 4：用户拒绝的结果也要进入模型上下文
      if (!sending.value) {
        await ask([
          {
            role: 'user',
            content: `用户拒绝执行工具 ${seg.tool}，本次未做任何修改。不要再次自动执行该操作，请说明或询问用户下一步。`,
          },
        ])
      }
      return
    }
  }
  const result = await execTool(item, index, seg)
  if (sending.value) return
  await ask([
    {
      role: 'user',
      content: `【工具 ${seg.tool} 的真实执行结果】\n${result.output}\n\n请基于以上结果继续分析或完成下一步，必要时可调用其他工具。`,
    },
  ])
}

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
const MAX_ATTACH_CHARS = 60_000

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
      if (attachments.value.some((item) => item.path === path)) continue
      const content = await fileReadText(path)
      if (content.length > MAX_ATTACH_CHARS) {
        notify.warning(`${path} 超过 ${MAX_ATTACH_CHARS} 字符，已截断`)
      }
      attachments.value.push({
        name: path.split(/[\\/]/).pop() ?? path,
        path,
        content: content.slice(0, MAX_ATTACH_CHARS),
      })
    }
  } catch (error) {
    notify.error(`读取文件失败：${String(error)}`)
  }
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

async function send() {
  const content = input.value.trim()
  if ((!content && !attachments.value.length) || sending.value) return
  if (!canChat.value) {
    notify.warning('请先配置一个模型服务')
    serviceOpen.value = true
    return
  }
  input.value = ''
  autoDepth.value = 0
  rejections.value = 0
  const full = withAttachments(content)
  attachments.value = []
  const session = await ensureSession()
  messages.value.push({ id: nextId(), role: 'user', content: full, createdAt: Date.now() })
  scrollBottom(true)
  if (session) await aiMessageAdd(session.id, 'user', full).catch(() => undefined)
  void refreshSessions()
  await ask()
}

/** Enter 直接发送；Shift / Ctrl / ⌘ + Enter 换行；输入法组字状态下不触发 */
function onEnter(event: KeyboardEvent) {
  if (event.shiftKey || event.ctrlKey || event.metaKey || event.isComposing) return
  event.preventDefault()
  void send()
}

/* ---------------- 会话列表 ---------------- */

async function refreshSessions() {
  try {
    sessions.value = await aiSessionList()
    if (sessionId.value && !sessions.value.some((item) => item.id === sessionId.value)) {
      sessionId.value = null
    }
  } catch {
    // 列表刷新失败不影响对话
  }
}

async function selectSession(id: number) {
  if (sending.value || id === sessionId.value) return
  sessionId.value = id
  resetRuns()
  try {
    const stored = await aiMessageList(id)
    messages.value = stored
      .filter((item) => item.role === 'user' || item.role === 'assistant')
      .map((item) => ({
        id: nextId(),
        role: item.role === 'user' ? ('user' as const) : ('assistant' as const),
        content: item.content,
        reasoning: item.reasoning ?? undefined,
        // 历史里的「调工具前的过程说明」同样隐藏，只回放最终回答
        hidden: isExecutionNote(item.content),
        createdAt: item.createdAt,
      }))
  } catch (error) {
    notify.error(String(error))
    messages.value = []
  }
  const session = sessions.value.find((item) => item.id === id)
  if (session?.providerId) activeId.value = session.providerId
  nextTick(() => scrollBottom(true))
}

function newSession() {
  if (sending.value) return
  sessionId.value = null
  messages.value = []
  resetRuns()
  autoDepth.value = 0
  input.value = ''
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
  '列出本机所有项目，并按请求数量排序',
  '在第一个项目里新建登录接口，发送并分析返回',
  '给接口写一个前置签名脚本，再发一次验证',
  '导出某个接口最近 50 条执行记录为 CSV',
  '最近一次压测报告的关键指标怎么样',
  '帮我导入 Postman 集合并整理成集合结构',
]

const capabilities = [
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
  // 只预取服务商与会话列表；历史消息等用户点开某条会话再加载，
  // 否则长会话会在进入页面时一次性渲染，导致明显卡顿
  await Promise.all([reloadProviders(), refreshSessions()])
})
</script>

<template>
  <div class="page ai-page">
    <PageHeader title="小HI" subtitle="内置 AI 助手 · 会话留存 · 可直接调用系统功能">
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

        <div class="session-list">
          <div
            v-for="item in sessions"
            :key="item.id"
            class="session-item"
            :class="{ active: item.id === sessionId }"
            @click="selectSession(item.id)"
          >
            <div class="session-main">
              <span class="session-name">{{ item.title }}</span>
              <span class="session-meta">{{ item.messageCount }} 条 · {{ fmtTime(item.updatedAt) }}</span>
            </div>
            <div class="session-ops" @click.stop>
              <a-tooltip title="重命名">
                <EditOutlined class="op" @click="openRename(item)" />
              </a-tooltip>
              <a-tooltip title="删除">
                <DeleteOutlined class="op danger" @click="removeSession(item)" />
              </a-tooltip>
            </div>
          </div>
          <div v-if="!sessions.length" class="side-empty">
            还没有会话记录。发送第一条消息后会自动建会话并保存在本机。
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
            <div class="hi-avatar">HI</div>
            <div class="hi-text">
              <div class="hi-name">小HI</div>
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
            <a-button size="small" :disabled="!messages.length || sending" @click="clearMessages">
              清空会话
            </a-button>
          </div>
        </div>

        <div v-if="!canChat" class="chat-empty">
          <RobotOutlined class="empty-icon" />
          <h3>还没有可用的模型</h3>
          <p>小HI 由你自己的 API 驱动，密钥经加密后只保存在本机。不配置则不提供任何 AI 能力。</p>
          <a-button type="primary" @click="serviceOpen = true">去配置服务</a-button>
        </div>

        <template v-else>
          <div ref="scrollRef" class="chat-scroll">
            <div v-if="messages.length === 0" class="chat-hint">
              <div class="hint-hero">
                <div class="hint-avatar"><RobotOutlined /></div>
                <div class="hint-hero-text">
                  <div class="hint-title">你好，我是小HI</div>
                  <p class="hint-sub">
                    FbinHttp 内置的接口调试助手，由你自己配置的模型驱动，密钥只加密保存在本机。
                    你可以直接用一句话描述目标，我会自动挑工具去执行，并把结果讲清楚。
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
              <div v-if="item.role === 'assistant'" class="bubble-avatar">HI</div>
              <div class="bubble-main">
                <div class="bubble-meta">
                  <span class="bubble-name">{{ item.role === 'user' ? '你' : '小HI' }}</span>
                  <span class="bubble-time">{{ fmtTime(item.createdAt) }}</span>
                </div>
                <div class="bubble-body" :class="{ failed: item.failed }">
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
                </div>
              </div>
            </div>
          </div>

          <div class="composer">
            <div v-if="attachments.length" class="attach-list">
              <span v-for="(file, index) in attachments" :key="file.path" class="attach-chip">
                <PaperClipOutlined />
                <span class="attach-name">{{ file.name }}</span>
                <span class="attach-size">{{ fmtSize(file.content) }}</span>
                <button type="button" class="attach-remove" @click="attachments.splice(index, 1)">×</button>
              </span>
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
              <a-textarea
                v-model:value="input"
                :auto-size="{ minRows: 2, maxRows: 6 }"
                :disabled="sending"
                placeholder="和小HI聊聊，或让它帮你操作软件…（Enter 发送，Shift + Enter 换行）"
                @keydown.enter="onEnter"
                @keydown.ctrl.enter.prevent="send"
              />
              <a-button
                type="primary"
                class="send-btn"
                :loading="sending"
                :disabled="(!input.trim() && !attachments.length) || sending"
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
        <a-form-item label="模型" required>
          <a-auto-complete
            v-model:value="form.model"
            :options="modelOptions"
            :open="modelOpen"
            :filter-option="filterModelOption"
            placeholder="从列表选择，或直接输入模型名"
            @focus="modelOpen = true"
            @blur="modelOpen = false"
            @select="modelOpen = false"
          >
            <template #notFoundContent>
              <span class="dialog-hint">列表里没有匹配项，可直接输入模型名</span>
            </template>
          </a-auto-complete>
          <template #extra>
            <span class="dialog-hint">{{ modelHint }}</span>
          </template>
        </a-form-item>
        <a-form-item :label="isEdit ? 'API Key（留空表示不修改）' : 'API Key'">
          <a-input-password v-model:value="form.apiKey" placeholder="sk-..." />
        </a-form-item>
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
          <span class="default-desc">进入小HI 时自动使用该服务，也可在顶部随时切换</span>
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
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-top: 1px solid var(--fm-border);
}

.composer-row {
  display: flex;
  gap: 8px;
  align-items: flex-end;
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
  gap: 6px;
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
</style>
