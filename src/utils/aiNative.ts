import type { AiMessage, AiToolCall } from '@/types/models'
import { isWriteTool } from '@/utils/aiTools'

/**
 * 原生 function calling（AI-04）的前端协议工具。
 * 只放纯函数：事件解析、历史成对性修正、一轮执行计划，便于单测。
 */

let callSeq = 0

function fallbackCallId(): string {
  callSeq += 1
  return `call_${Date.now().toString(36)}_${callSeq}`
}

/** 解析后端 tool_call 事件体：{"id","name","arguments"} */
export function parseNativeToolCallPayload(payload: string | undefined): AiToolCall | null {
  if (!payload) return null
  try {
    const parsed = JSON.parse(payload) as Record<string, unknown>
    const name = typeof parsed.name === 'string' ? parsed.name.trim() : ''
    if (!name) return null
    return {
      id: typeof parsed.id === 'string' && parsed.id ? parsed.id : fallbackCallId(),
      name,
      arguments:
        typeof parsed.arguments === 'string'
          ? parsed.arguments
          : JSON.stringify(parsed.arguments ?? {}),
    }
  } catch {
    return null
  }
}

/** 原生工具参数是 JSON 文本；解析失败返回 null，由调用方回灌明确错误 */
export function parseNativeArguments(raw: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(raw || '{}')
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null
  } catch {
    return null
  }
}

/** 从助手正文里去掉为界面合成出来的工具块，结构化历史只保留自然语言 */
export function stripSyntheticToolBlocks(content: string): string {
  return content
    .replace(/```(?:fbin-tool|tool)[ \t]*\r?\n[\s\S]*?```/g, '')
    .replace(/<\s*\/?\s*fbin[-_\s]*tool\s*>/gi, '')
    .trim()
}

/**
 * 保证原生工具历史成对出现：assistant.tool_calls 的每个 id 都必须有对应
 * 的 tool 消息；预算裁剪或摘要丢了一半时，去掉结构化字段只留正文，
 * 避免 OpenAI / Anthropic 因 tool_call_id 不匹配直接 400。
 */
export function normalizeNativeHistory(list: AiMessage[]): AiMessage[] {
  const toolById = new Map<string, AiMessage>()
  for (const message of list) {
    if (message.role === 'tool' && message.toolCallId) toolById.set(message.toolCallId, message)
  }
  const validIds = new Set<string>()
  for (const message of list) {
    if (message.role !== 'assistant' || !message.toolCalls?.length) continue
    if (message.toolCalls.every((call) => toolById.has(call.id))) {
      for (const call of message.toolCalls) validIds.add(call.id)
    }
  }
  const out: AiMessage[] = []
  for (const message of list) {
    if (message.role === 'tool') {
      if (message.toolCallId && validIds.has(message.toolCallId)) out.push(message)
      continue
    }
    if (message.role === 'assistant' && message.toolCalls?.length) {
      const calls = message.toolCalls.filter((call) => validIds.has(call.id))
      if (calls.length === message.toolCalls.length) {
        out.push({ ...message, toolCalls: calls })
      } else if (message.content.trim()) {
        out.push({ role: 'assistant', content: message.content })
      }
      continue
    }
    out.push(message)
  }
  return out
}

export interface NativeRunPlan {
  /** 含写工具 / send_request：必须串行并逐个确认 */
  needsGate: boolean
  /** 每批要执行的下标；只读按 3 个一批，写操作每批 1 个 */
  batches: number[][]
}

/** 一轮原生工具的执行计划：只读最多 3 个并行，写工具串行 */
export function nativeRunPlan(calls: AiToolCall[]): NativeRunPlan {
  if (!calls.length) return { needsGate: false, batches: [] }
  const needsGate = calls.some(
    (call) => isWriteTool(call.name) || call.name === 'send_request',
  )
  if (needsGate) {
    return { needsGate, batches: calls.map((_call, index) => [index]) }
  }
  const batches: number[][] = []
  for (let i = 0; i < calls.length; i += 3) {
    batches.push(calls.slice(i, i + 3).map((_call, offset) => i + offset))
  }
  return { needsGate, batches }
}
