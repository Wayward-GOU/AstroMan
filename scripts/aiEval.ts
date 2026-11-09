import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { AGENT_SYSTEM_PROMPT } from '../src/utils/aiPrompt'
import {
  isWriteTool,
  nativeToolDefs,
  toolNames,
  toolParameterSchema,
} from '../src/utils/aiTools'

/**
 * AI-10 golden 评测运行器。
 *
 * 只做本地手动运行，不接 CI 真实模型：
 * - 未配置 AI_EVAL_* 环境变量时只校验用例集结构，不发起网络请求；
 * - 配置后按真实模型跑 20 条用例，输出命中率 / 轮数 / token / P95 时延，
 *   命中率 <85% 或平均轮数 >4 时测试失败（阻断发布）。
 *
 * 用法见 evals/ai/README.md。
 */

export interface EvalCase {
  id: string
  title: string
  input: string
  /** null 表示这轮不应该调用任何工具（闲聊 / 需要先确认的写入） */
  expectTool: string | null
  /** 允许的其他首步工具（等价路径，如先预检、先查状态） */
  acceptTools?: string[]
  /** 本轮出现即算违规的工具 */
  forbiddenTools?: string[]
  /** 关键参数（只比较列出的键，数字 / 字符串按文本比较） */
  expectArgs?: Record<string, unknown>
  /** 期望产物，供人工评分对照，不做机器断言 */
  expectedArtifact: string
  /** 模型没有调用工具时依次补充的用户消息，最多再问 3 轮 */
  followUps?: string[]
}

export interface ToolCall {
  name: string
  args: Record<string, unknown>
  source: 'native' | 'text'
}

export interface ModelReply {
  calls: ToolCall[]
  text: string
  promptTokens: number
  completionTokens: number
}

export interface CaseResult {
  id: string
  title: string
  hit: boolean
  toolHit: boolean
  argsHit: boolean
  rounds: number
  tokens: number
  latencyMs: number
  picked: string | null
  forbiddenHit: string[]
  error?: string
  manualScore?: number
}

export interface EvalMetrics {
  total: number
  hit: number
  hitRate: number
  avgRounds: number
  tokens: number
  p95LatencyMs: number
  manualScore: number | null
}

export interface EvalReport {
  startedAt: string
  mode: 'native' | 'text'
  model: string
  baseUrl: string
  cases: CaseResult[]
  metrics: EvalMetrics
  verdict: { pass: boolean; reasons: string[] }
}

export interface LiveOptions {
  mode: 'native' | 'text'
  baseUrl: string
  apiKey: string
  model: string
  temperature?: number
  timeoutMs?: number
  maxRounds?: number
  scores?: Record<string, number>
}

export const THRESHOLDS = { hitRate: 0.85, avgRounds: 4 }
export const EXPECTED_CASES = 20

const HERE = dirname(fileURLToPath(import.meta.url))
export const EVAL_DIR = resolve(HERE, '..', 'evals', 'ai')
const PROMPT_PATH = resolve(HERE, '..', 'src-tauri', 'src', 'prompts', 'hangzai_system_prompt.md')

/* ---------------- 用例加载与结构校验 ---------------- */

export function loadEvalCases(dir = EVAL_DIR): EvalCase[] {
  const files = readdirSync(dir)
    .filter((name) => name.endsWith('.json') && !name.startsWith('.'))
    .sort()
  return files.map((name) => {
    const raw = readFileSync(join(dir, name), 'utf8')
    return JSON.parse(raw) as EvalCase
  })
}

/** 结构问题全部一次性报出，避免改一条跑一遍 */
export function validateEvalCases(cases: EvalCase[]): string[] {
  const errors: string[] = []
  if (cases.length !== EXPECTED_CASES) {
    errors.push(`用例数应为 ${EXPECTED_CASES}，实际 ${cases.length}`)
  }
  const names = new Set(toolNames())
  const seen = new Set<string>()
  for (const item of cases) {
    const tag = item.id || '(缺少 id)'
    if (!/^ai-\d{2}$/.test(item.id)) errors.push(`${tag}: id 必须形如 ai-01`)
    if (seen.has(item.id)) errors.push(`${tag}: id 重复`)
    seen.add(item.id)
    if (!item.title?.trim()) errors.push(`${tag}: 缺少 title`)
    if (!item.input?.trim()) errors.push(`${tag}: 缺少 input`)
    if (!item.expectedArtifact?.trim()) errors.push(`${tag}: 缺少 expectedArtifact`)
    if (item.expectTool !== null && !names.has(item.expectTool)) {
      errors.push(`${tag}: expectTool ${item.expectTool} 不存在`)
    }
    for (const name of item.acceptTools ?? []) {
      if (!names.has(name)) errors.push(`${tag}: acceptTools 含未知工具 ${name}`)
      if (name === item.expectTool) errors.push(`${tag}: acceptTools 不应重复 expectTool`)
    }
    for (const name of item.forbiddenTools ?? []) {
      if (!names.has(name)) errors.push(`${tag}: forbiddenTools 含未知工具 ${name}`)
      if (name === item.expectTool) errors.push(`${tag}: forbiddenTools 不能包含 expectTool`)
    }
    if (item.followUps !== undefined) {
      if (!Array.isArray(item.followUps)) errors.push(`${tag}: followUps 必须是数组`)
      else if (item.followUps.some((text) => !String(text ?? '').trim())) {
        errors.push(`${tag}: followUps 不能有空消息`)
      }
    }
    const expectArgs = item.expectArgs ?? {}
    if (item.expectTool) {
      const schema = toolParameterSchema(item.expectTool)
      const properties = (schema.properties ?? {}) as Record<string, unknown>
      for (const key of Object.keys(expectArgs)) {
        if (!(key in properties)) errors.push(`${tag}: ${item.expectTool} 没有参数 ${key}`)
      }
      // 写工具用例必须自带用户确认语句，否则模型拒调是正确行为，评测会误判
      if (isWriteTool(item.expectTool) && !/确认|可以|直接|执行|保存|开始|启动|导出|跑一遍/.test(item.input)) {
        errors.push(`${tag}: 期望写工具 ${item.expectTool}，input 需包含用户确认语句`)
      }
    } else if (Object.keys(expectArgs).length > 0) {
      errors.push(`${tag}: expectTool 为 null 时不应声明 expectArgs`)
    }
  }
  return errors
}

/* ---------------- 协议解析 ---------------- */

/** 原生 tool_calls */
function readNativeCalls(payload: unknown): ToolCall[] {
  if (!Array.isArray(payload)) return []
  const calls: ToolCall[] = []
  for (const entry of payload) {
    const func = (entry as { function?: { name?: unknown; arguments?: unknown } })?.function
    const name = String(func?.name ?? '').trim()
    if (!name) continue
    let args: Record<string, unknown> = {}
    const raw = String(func?.arguments ?? '').trim()
    if (raw) {
      try {
        const parsed = JSON.parse(raw)
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
          args = parsed as Record<string, unknown>
        }
      } catch {
        // 参数不是合法 JSON：保留空对象，由参数校验判失败
      }
    }
    calls.push({ name, args, source: 'native' })
  }
  return calls
}

/** 文本协议的 fbin-tool 代码块 */
export function parseTextToolCalls(content: string): ToolCall[] {
  const calls: ToolCall[] = []
  const pattern = /```fbin-tool\s*([\s\S]*?)```/g
  for (const match of content.matchAll(pattern)) {
    const raw = match[1]?.trim()
    if (!raw) continue
    try {
      const parsed = JSON.parse(raw) as { tool?: unknown; args?: unknown }
      const name = String(parsed.tool ?? '').trim()
      if (!name) continue
      const args =
        parsed.args && typeof parsed.args === 'object' && !Array.isArray(parsed.args)
          ? (parsed.args as Record<string, unknown>)
          : {}
      calls.push({ name, args, source: 'text' })
    } catch {
      // 非法 JSON 不当作工具调用，与前端解析保持一致
    }
  }
  return calls
}

/* ---------------- 模型调用（OpenAI 兼容） ---------------- */

interface ChatMessage {
  role: 'system' | 'user' | 'assistant'
  content: string
}

export function buildMessages(caseItem: EvalCase, systemPrompt: string): ChatMessage[] {
  return [
    { role: 'system', content: systemPrompt },
    { role: 'user', content: caseItem.input },
  ]
}

/** 后端全局规则 + 前端工具协议，与真实对话的注入顺序一致 */
export function loadSystemPrompt(): string {
  const backend = readFileSync(PROMPT_PATH, 'utf8').trim()
  return `${backend}\n\n${AGENT_SYSTEM_PROMPT}`
}

async function callModel(
  options: LiveOptions,
  messages: ChatMessage[],
): Promise<ModelReply> {
  const endpoint = `${options.baseUrl.replace(/\/+$/, '')}/chat/completions`
  const body: Record<string, unknown> = {
    model: options.model,
    temperature: options.temperature ?? 0,
    messages,
  }
  if (options.mode === 'native') {
    body.tools = nativeToolDefs().map((tool) => ({
      type: 'function',
      function: { name: tool.name, description: tool.description, parameters: tool.parameters },
    }))
    body.tool_choice = 'auto'
  }
  const response = await fetch(endpoint, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${options.apiKey}`,
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(options.timeoutMs ?? 90_000),
  })
  const text = await response.text()
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${text.slice(0, 300)}`)
  }
  let payload: {
    choices?: {
      message?: {
        content?: string | null
        tool_calls?: unknown
      }
    }[]
    usage?: { prompt_tokens?: number; completion_tokens?: number }
  }
  try {
    payload = JSON.parse(text)
  } catch {
    throw new Error(`返回不是 JSON：${text.slice(0, 200)}`)
  }
  const message = payload.choices?.[0]?.message
  const content = String(message?.content ?? '')
  const native = readNativeCalls(message?.tool_calls)
  // 原生不可用或模型混用文本协议时，两种来源都收；原生优先
  const calls = native.length ? native : parseTextToolCalls(content)
  return {
    calls,
    text: content,
    promptTokens: Math.max(0, Number(payload.usage?.prompt_tokens) || 0),
    completionTokens: Math.max(0, Number(payload.usage?.completion_tokens) || 0),
  }
}

/* ---------------- 指标 ---------------- */

export function percentile(values: number[], ratio: number): number {
  if (!values.length) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * ratio) - 1))
  return sorted[index]
}

function argsMatch(call: ToolCall, expectArgs: Record<string, unknown>): boolean {
  return Object.entries(expectArgs).every(([key, expected]) => {
    const actual = call.args[key]
    if (actual === undefined) return false
    return String(actual) === String(expected)
  })
}

export function summarize(cases: CaseResult[]): EvalMetrics {
  const total = cases.length
  const hit = cases.filter((item) => item.hit).length
  const rounds = cases.reduce((sum, item) => sum + item.rounds, 0)
  const tokens = cases.reduce((sum, item) => sum + item.tokens, 0)
  const scores = cases
    .map((item) => item.manualScore)
    .filter((score): score is number => typeof score === 'number' && Number.isFinite(score))
  return {
    total,
    hit,
    hitRate: total ? hit / total : 0,
    avgRounds: total ? rounds / total : 0,
    tokens,
    p95LatencyMs: percentile(cases.map((item) => item.latencyMs), 0.95),
    manualScore: scores.length ? scores.reduce((sum, score) => sum + score, 0) / scores.length : null,
  }
}

export function verdictOf(metrics: EvalMetrics): { pass: boolean; reasons: string[] } {
  const reasons: string[] = []
  if (metrics.hitRate < THRESHOLDS.hitRate) {
    reasons.push(`命中率 ${(metrics.hitRate * 100).toFixed(1)}% 低于 ${(THRESHOLDS.hitRate * 100).toFixed(0)}%`)
  }
  if (metrics.avgRounds > THRESHOLDS.avgRounds) {
    reasons.push(`平均轮数 ${metrics.avgRounds.toFixed(2)} 超过 ${THRESHOLDS.avgRounds}`)
  }
  return { pass: reasons.length === 0, reasons }
}

/* ---------------- 运行 ---------------- */

export async function runEval(
  cases: EvalCase[],
  options: LiveOptions,
): Promise<EvalReport> {
  const systemPrompt = loadSystemPrompt()
  const maxRounds = Math.min(4, Math.max(1, options.maxRounds ?? 4))
  const results: CaseResult[] = []
  for (const caseItem of cases) {
    const messages = buildMessages(caseItem, systemPrompt)
    const started = Date.now()
    let rounds = 0
    let tokens = 0
    let calls: ToolCall[] = []
    let error: string | undefined
    try {
      const followUps = caseItem.followUps ?? []
      const limit = Math.min(maxRounds, 1 + followUps.length)
      while (rounds < limit) {
        rounds += 1
        const reply = await callModel(options, messages)
        tokens += reply.promptTokens + reply.completionTokens
        calls = reply.calls
        if (calls.length) break
        const next = followUps[rounds - 1]
        if (!next) break
        messages.push(
          { role: 'assistant', content: reply.text || '（本轮没有输出）' },
          { role: 'user', content: next },
        )
      }
    } catch (err) {
      error = err instanceof Error ? err.message : String(err)
    }
    const latencyMs = Date.now() - started
    const forbiddenHit = calls
      .map((call) => call.name)
      .filter((name) => (caseItem.forbiddenTools ?? []).includes(name))
    const picked = calls[0] ?? null
    const expected = caseItem.expectTool
    const allowed = new Set([expected, ...(caseItem.acceptTools ?? [])].filter(Boolean) as string[])
    const toolHit = expected === null ? calls.length === 0 : Boolean(picked && allowed.has(picked.name))
    const argsHit = toolHit && picked && expected ? argsMatch(picked, caseItem.expectArgs ?? {}) : toolHit
    results.push({
      id: caseItem.id,
      title: caseItem.title,
      hit: !error && toolHit && argsHit && forbiddenHit.length === 0,
      toolHit,
      argsHit,
      rounds,
      tokens,
      latencyMs,
      picked: picked?.name ?? null,
      forbiddenHit,
      error,
      manualScore: options.scores?.[caseItem.id],
    })
  }
  const metrics = summarize(results)
  return {
    startedAt: new Date().toISOString(),
    mode: options.mode,
    model: options.model,
    baseUrl: options.baseUrl,
    cases: results,
    metrics,
    verdict: verdictOf(metrics),
  }
}

/* ---------------- 环境配置与报告 ---------------- */

export function readLiveOptions(env: NodeJS.ProcessEnv = process.env): LiveOptions | null {
  const baseUrl = String(env.AI_EVAL_BASE_URL ?? '').trim()
  const apiKey = String(env.AI_EVAL_API_KEY ?? '').trim()
  const model = String(env.AI_EVAL_MODEL ?? '').trim()
  if (!baseUrl || !apiKey || !model) return null
  const mode = String(env.AI_EVAL_PROTOCOL ?? 'native').trim() === 'text' ? 'text' : 'native'
  const temperature = Number(env.AI_EVAL_TEMPERATURE)
  const timeoutMs = Number(env.AI_EVAL_TIMEOUT_MS)
  const scoresPath = String(env.AI_EVAL_SCORES ?? '').trim()
  return {
    mode,
    baseUrl,
    apiKey,
    model,
    temperature: Number.isFinite(temperature) ? temperature : 0,
    timeoutMs: Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : 90_000,
    scores: scoresPath ? loadScores(scoresPath) : undefined,
  }
}

export function loadScores(path: string): Record<string, number> {
  const raw = JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>
  const scores: Record<string, number> = {}
  for (const [id, value] of Object.entries(raw)) {
    const score = Number(value)
    if (Number.isFinite(score) && score >= 1 && score <= 5) scores[id] = score
  }
  return scores
}

export function writeReport(path: string, report: EvalReport): void {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`, 'utf8')
}

export function formatSummary(report: EvalReport): string {
  const lines = report.cases.map((item) => {
    const mark = item.hit ? '✓' : '✗'
    const picked = item.picked ?? '未调用工具'
    const extra = item.error
      ? ` 错误：${item.error}`
      : item.forbiddenHit.length
        ? ` 违规：${item.forbiddenHit.join(', ')}`
        : item.toolHit && !item.argsHit
          ? ' 参数不匹配'
          : ''
    return `${mark} ${item.id} ${picked} ${item.rounds}轮 ${item.tokens}tok ${item.latencyMs}ms${extra}`
  })
  const { metrics } = report
  lines.push(
    '',
    `命中率 ${(metrics.hitRate * 100).toFixed(1)}% (${metrics.hit}/${metrics.total}) · ` +
      `平均轮数 ${metrics.avgRounds.toFixed(2)} · tokens ${metrics.tokens} · ` +
      `P95 ${metrics.p95LatencyMs}ms` +
      (metrics.manualScore === null ? '' : ` · 人工评分 ${metrics.manualScore.toFixed(1)}/5`),
    report.verdict.pass
      ? `门槛通过：命中率 ≥${(THRESHOLDS.hitRate * 100).toFixed(0)}%、平均轮数 ≤${THRESHOLDS.avgRounds}`
      : `门槛未通过：${report.verdict.reasons.join('；')}`,
  )
  return lines.join('\n')
}

export function defaultReportPath(): string {
  return join(EVAL_DIR, '.last-run.json')
}
