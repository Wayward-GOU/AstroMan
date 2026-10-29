import type { AiMessage } from '@/types/models'

/**
 * token 估算与上下文组装。
 *
 * 估算刻意保守：中文按 1 字 ≈ 0.7 token，其他字符 4 个 ≈ 1 token。
 * 宁可少放几条历史，也不要让模型端因为超出窗口直接报错。
 */

export function estimateTokens(text: string): number {
  if (!text) return 0
  let cjk = 0
  let other = 0
  for (const ch of text) {
    if (/[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/.test(ch)) {
      cjk += 1
    } else {
      other += 1
    }
  }
  return Math.ceil(cjk * 0.7 + other / 4)
}

export function estimateMessages(messages: AiMessage[]): number {
  let total = 0
  for (const message of messages) {
    // 每条消息固定 4 token 的角色 / 分隔开销
    total += estimateTokens(message.content) + 4
  }
  return total
}

export interface ContextPlan {
  messages: AiMessage[]
  /** 因预算被省略的消息条数 */
  dropped: number
  /** 组装后的估算 token（不含 system） */
  tokens: number
}

/**
 * 按预算从「最近」往前装历史。
 * - 不拆分消息，代码块永远完整；
 * - 单条超预算时按字符截断，且不截在 Markdown 围栏中间；
 * - 预算至少保留最后一条，避免出现空上下文。
 */
export function buildContext(history: AiMessage[], budget: number): ContextPlan {
  if (!history.length) return { messages: [], dropped: 0, tokens: 0 }
  const picked: AiMessage[] = []
  let used = 0
  for (let index = history.length - 1; index >= 0; index -= 1) {
    const message = history[index]
    const cost = estimateTokens(message.content) + 4
    if (used + cost <= budget) {
      picked.unshift(message)
      used += cost
      continue
    }
    if (!picked.length) {
      // 最后一条本身就超预算：截断后仍然保留，保证模型能看到本轮问题
      const room = Math.max(200, budget - 4)
      picked.unshift({ ...message, content: clipToTokens(message.content, room) })
      used = estimateTokens(picked[0].content) + 4
    }
    break
  }
  return { messages: picked, dropped: history.length - picked.length, tokens: used }
}

/** 按 token 预算截断文本，并尽量避开代码围栏 */
export function clipToTokens(text: string, tokens: number): string {
  if (estimateTokens(text) <= tokens) return text
  // 先按比例估算字符数，再多退 10% 留余量
  const ratio = tokens / Math.max(1, estimateTokens(text))
  let chars = Math.floor(text.length * ratio * 0.9)
  let cut = text.slice(0, chars)
  const fences = (cut.match(/```/g) ?? []).length
  if (fences % 2 === 1) {
    // 落在围栏内部：补一个收尾围栏，避免模型看到半截代码块
    cut += '\n```'
  }
  return `${cut}\n…（上下文预算不足，已截断）`
}

/** 会话摘要消息的统一前缀 */
export const SUMMARY_PREFIX = '【会话摘要】较早的对话已压缩如下，禁止编造未出现的事实：\n'
