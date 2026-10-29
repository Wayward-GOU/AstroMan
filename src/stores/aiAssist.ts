import { defineStore } from 'pinia'
import { computed, reactive, ref } from 'vue'
import { aiChat, aiProviderList } from '@/api'
import { buildContext, estimateTokens } from '@/utils/contextBudget'
import { contextBudgetOf } from '@/utils/modelMeta'
import { compactPrompt } from '@/utils/promptText'
import { wrapUntrusted } from '@/utils/untrusted'
import type { AiMessage, AiProvider } from '@/types/models'

export interface AssistMessage {
  role: 'user' | 'assistant'
  content: string
  streaming?: boolean
  failed?: boolean
}

/** 页面内分析模式：只做解读与建议，不调用工具，也不允许输出工具标记 */
const SYSTEM_PROMPT = `你是 AstroMan 桌面工具内置的接口分析助手，当前处于「页面内分析」模式。
用户会给你一段当前页面的上下文（请求、响应、压测报告、请求历史、环境变量、批量执行结果等）以及具体诉求。

要求：
1. 直接输出中文结论，使用 Markdown，小标题 + 要点，不要寒暄，不要重复上下文原文。
2. 需要给脚本时使用 \`\`\`js 代码块；脚本必须遵循 AstroMan 脚本引擎：可用 vars / env / cipher / rsa / test / assert，不能用 require / import / fetch / async-await / 定时器。
3. 只基于用户给出的上下文推理，信息不足时明确说明缺少什么，禁止编造响应体、状态码、压测指标。
4. 不要输出任何工具调用块、尖括号标记或特殊标记，也不要声称自己已经发送了请求或执行了压测。
5. 涉及密钥、Token 时提醒放到环境变量里，不要明文回显。`

const MAX_ROUNDS = 12

export const useAiAssistStore = defineStore('aiAssist', () => {
  const open = ref(false)
  const title = ref('AI 分析')
  const hint = ref('')
  const providers = ref<AiProvider[]>([])
  const providerId = ref<number | null>(null)
  const messages = ref<AssistMessage[]>([])
  const streaming = ref(false)
  const error = ref('')

  /** 页面上下文只进入模型，不铺在对话里 */
  let context = ''

  const currentProvider = computed(
    () => providers.value.find((item) => item.id === providerId.value) ?? null,
  )
  const ready = computed(() => Boolean(providerId.value))
  const canSend = computed(() => ready.value && !streaming.value)

  /**
   * 大报告头尾都保留：头部是结构，尾部通常才是最新 / 汇总的关键指标。
   * 只截中间，避免「只截前 12k」丢掉结论。
   */
  function clipMiddle(text: string, limit: number): string {
    if (text.length <= limit) return text
    const head = Math.floor(limit * 0.6)
    const tail = Math.max(0, limit - head)
    return `${text.slice(0, head)}\n…（中间省略 ${text.length - limit} 字符）…\n${text.slice(-tail)}`
  }

  async function ensureProviders(): Promise<number | null> {
    if (!providers.value.length) {
      try {
        providers.value = await aiProviderList()
      } catch {
        providers.value = []
      }
    }
    if (!providerId.value) {
      const fallback =
        providers.value.find((item) => item.isDefault && item.id) ??
        providers.value.find((item) => item.id)
      providerId.value = fallback?.id ?? null
    }
    return providerId.value
  }

  /**
   * 按模型窗口动态分配：页面快照最多占 70%，剩下留给追问历史。
   * 快照同样包不可信标签（T-17），避免报告正文里藏指令。
   */
  function payload(extra: AssistMessage[]): AiMessage[] {
    const budget = contextBudgetOf(currentProvider.value?.model ?? '')
    const room = Math.max(2000, budget - estimateTokens(SYSTEM_PROMPT) - 400)
    const head: AiMessage[] = [{ role: 'system', content: SYSTEM_PROMPT }]
    const dialogue: AiMessage[] = []
    let left = room
    if (context) {
      const snapshot = wrapUntrusted('page:snapshot', compactPrompt(context))
      const text = clipMiddle(snapshot, Math.max(1200, Math.floor(room * 0.7)))
      head.push({ role: 'user', content: text })
      left = Math.max(800, room - estimateTokens(text))
    }
    for (const item of extra.slice(-MAX_ROUNDS)) {
      if (!item.content.trim()) continue
      dialogue.push({ role: item.role, content: compactPrompt(item.content) })
    }
    return [...head, ...buildContext(dialogue, left).messages]
  }

  async function run() {
    const id = await ensureProviders()
    if (!id) {
      error.value = '还没有可用的模型服务，请先到「航仔」页面配置模型。'
      return
    }
    streaming.value = true
    error.value = ''
    // 响应式对象：直接改原始对象不会触发视图更新，会退化成「全部生成完才渲染」
    const item = reactive<AssistMessage>({ role: 'assistant', content: '', streaming: true })
    messages.value.push(item)
    // 攒 50ms 刷一次，边生成边渲染，同时避免每个 token 都重排 Markdown
    let buffer = ''
    let lastPaint = 0
    const paint = (force = false) => {
      if (!buffer) return
      const now = performance.now()
      if (!force && now - lastPaint < 50) return
      item.content += buffer
      buffer = ''
      lastPaint = now
    }
    try {
      await aiChat(
        {
          providerId: id,
          messages: payload(messages.value.slice(0, -1)),
          temperature: currentProvider.value?.temperature ?? 0.3,
        },
        (event) => {
          if (event.kind === 'delta' && event.text) {
            buffer += event.text
            paint()
          } else if (event.kind === 'error' && event.text) {
            paint(true)
            error.value = event.text
            item.failed = true
          }
        },
      )
    } catch (err) {
      paint(true)
      error.value = String(err)
      item.failed = true
      if (!item.content) item.content = String(err)
    } finally {
      paint(true)
      if (!item.content && error.value) {
        item.content = error.value
        item.failed = true
      }
      item.streaming = false
      streaming.value = false
    }
  }

  /** 打开抽屉并立即分析 */
  async function show(options: { title: string; prompt: string; hint?: string }) {
    title.value = options.title
    hint.value = options.hint ?? ''
    context = options.prompt
    messages.value = []
    error.value = ''
    open.value = true
    await run()
  }

  /** 在抽屉里继续追问 */
  async function ask(text: string) {
    const content = text.trim()
    if (!content || streaming.value) return
    messages.value.push({ role: 'user', content })
    await run()
  }

  function close() {
    open.value = false
  }

  function reset() {
    messages.value = []
    error.value = ''
    context = ''
  }

  return {
    open,
    title,
    hint,
    providers,
    providerId,
    currentProvider,
    messages,
    streaming,
    error,
    ready,
    canSend,
    show,
    ask,
    close,
    reset,
  }
})
