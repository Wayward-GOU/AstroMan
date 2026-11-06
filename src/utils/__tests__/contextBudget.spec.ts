import { describe, expect, it } from 'vitest'
import { buildContext, clipToTokens, estimateTokens } from '@/utils/contextBudget'
import {
  DEFAULT_CONTEXT_WINDOW,
  contextBudgetOf,
  contextWindowOf,
  effectiveWindow,
  inputBudgetOf,
  outputReserveOf,
} from '@/utils/modelMeta'
import type { AiMessage } from '@/types/models'

const msg = (role: AiMessage['role'], content: string): AiMessage => ({ role, content })

describe('estimateTokens', () => {
  it('中文按 0.7 token/字，英文按 4 字符 1 token', () => {
    expect(estimateTokens('你好世界')).toBe(3)
    expect(estimateTokens('abcdefgh')).toBe(2)
    expect(estimateTokens('')).toBe(0)
  })
})

describe('contextWindowOf', () => {
  it('常见模型窗口正确', () => {
    expect(contextWindowOf('gpt-4o')).toBe(128_000)
    expect(contextWindowOf('claude-sonnet-4-5')).toBe(200_000)
    expect(contextWindowOf('deepseek-chat')).toBe(128_000)
    expect(contextWindowOf('gemini-2.5-pro')).toBe(1_000_000)
  })

  it('未知模型回退保守值', () => {
    expect(contextWindowOf('my-local-model')).toBe(DEFAULT_CONTEXT_WINDOW)
    expect(contextWindowOf('')).toBe(DEFAULT_CONTEXT_WINDOW)
  })

  it('预算为窗口的 60%', () => {
    expect(contextBudgetOf('gpt-4o')).toBe(76_800)
  })
})

describe('effectiveWindow', () => {
  it('用户显式配置优先，0 / 空回退模型名识别', () => {
    expect(effectiveWindow('gpt-4o', 8_000)).toBe(8_000)
    expect(effectiveWindow('gpt-4o', 0)).toBe(128_000)
    expect(effectiveWindow('gpt-4o')).toBe(128_000)
  })
})

describe('outputReserveOf / inputBudgetOf', () => {
  it('输出预留跟随 maxTokens，但不吞掉一半以上窗口', () => {
    expect(outputReserveOf(128_000, 8_192)).toBe(8_192)
    expect(outputReserveOf(8_000, 8_192)).toBe(4_000)
    expect(outputReserveOf(128_000)).toBe(4_096)
  })

  it('输入预算 = 窗口 - 输出预留，最低保留 1500', () => {
    expect(inputBudgetOf(128_000, 8_192)).toBe(119_808)
    expect(inputBudgetOf(2_000, 4_096)).toBe(1_500)
  })
})

describe('buildContext', () => {
  it('预算充足时全部保留', () => {
    const history = [msg('user', '你好'), msg('assistant', '你好，有什么可以帮你')]
    const plan = buildContext(history, 1000)
    expect(plan.messages).toHaveLength(2)
    expect(plan.dropped).toBe(0)
  })

  it('超预算时保留最近的轮次并告知省略条数', () => {
    const history = Array.from({ length: 50 }, (_, index) =>
      msg(index % 2 ? 'assistant' : 'user', '这是一段比较长的历史消息，用来占满上下文预算。'.repeat(5)),
    )
    const plan = buildContext(history, 500)
    expect(plan.messages.length).toBeLessThan(history.length)
    expect(plan.dropped).toBe(history.length - plan.messages.length)
    expect(plan.messages[plan.messages.length - 1]).toEqual(history[history.length - 1])
  })

  it('单条超预算时截断但不截在代码围栏内', () => {
    const code = '```json\n' + '{"a":1},\n'.repeat(200) + '```'
    const plan = buildContext([msg('user', code)], 120)
    expect(plan.messages).toHaveLength(1)
    const fences = (plan.messages[0].content.match(/```/g) ?? []).length
    expect(fences % 2).toBe(0)
  })

  it('空历史返回空结果', () => {
    expect(buildContext([], 100)).toEqual({ messages: [], dropped: 0, tokens: 0 })
  })
})

describe('clipToTokens', () => {
  it('未超限原样返回', () => {
    expect(clipToTokens('short', 100)).toBe('short')
  })
})
