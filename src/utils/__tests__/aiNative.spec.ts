import { describe, expect, it } from 'vitest'
import {
  nativeRunPlan,
  normalizeNativeHistory,
  parseNativeArguments,
  parseNativeToolCallPayload,
  stripSyntheticToolBlocks,
} from '@/utils/aiNative'
import type { AiMessage, AiToolCall } from '@/types/models'

const call = (id: string, name = 'project_list'): AiToolCall => ({
  id,
  name,
  arguments: '{}',
})

describe('parseNativeToolCallPayload', () => {
  it('解析后端 tool_call 事件体', () => {
    expect(
      parseNativeToolCallPayload('{"id":"call_1","name":"project_list","arguments":"{}"}'),
    ).toEqual({ id: 'call_1', name: 'project_list', arguments: '{}' })
  })

  it('arguments 是对象时序列化成 JSON 文本', () => {
    const parsed = parseNativeToolCallPayload(
      '{"id":"call_2","name":"request_get","arguments":{"id":3}}',
    )
    expect(parsed?.arguments).toBe('{"id":3}')
  })

  it('非法 JSON / 缺工具名返回 null', () => {
    expect(parseNativeToolCallPayload('not-json')).toBeNull()
    expect(parseNativeToolCallPayload('{"id":"x"}')).toBeNull()
    expect(parseNativeToolCallPayload(undefined)).toBeNull()
  })
})

describe('parseNativeArguments', () => {
  it('空字符串按空对象处理', () => {
    expect(parseNativeArguments('')).toEqual({})
  })

  it('非法 JSON 返回 null，交给上层回灌明确错误', () => {
    expect(parseNativeArguments('{"id":')).toBeNull()
    expect(parseNativeArguments('[1,2]')).toBeNull()
  })
})

describe('stripSyntheticToolBlocks', () => {
  it('只去掉工具块，保留正文与普通代码块', () => {
    const content = [
      '先查一下项目。',
      '```fbin-tool',
      '{"tool":"project_list","args":{}}',
      '```',
      '```ts',
      'const a = 1',
      '```',
    ].join('\n')
    const stripped = stripSyntheticToolBlocks(content)
    expect(stripped).toContain('先查一下项目。')
    expect(stripped).toContain('const a = 1')
    expect(stripped).not.toContain('fbin-tool')
  })
})

describe('normalizeNativeHistory', () => {
  it('assistant.tool_calls 与 tool 结果成对时原样保留', () => {
    const list: AiMessage[] = [
      { role: 'user', content: '查项目' },
      { role: 'assistant', content: '', toolCalls: [call('c1')] },
      { role: 'tool', content: '{"items":[]}', toolCallId: 'c1', name: 'project_list' },
    ]
    expect(normalizeNativeHistory(list)).toEqual(list)
  })

  it('没有对应结果的 tool 消息被丢弃', () => {
    const list: AiMessage[] = [
      { role: 'user', content: '查项目' },
      { role: 'tool', content: '孤儿结果', toolCallId: 'missing', name: 'project_list' },
    ]
    expect(normalizeNativeHistory(list)).toEqual([{ role: 'user', content: '查项目' }])
  })

  it('结果被预算截断时去掉结构化字段，只保留有内容的正文', () => {
    const list: AiMessage[] = [
      { role: 'assistant', content: '我查一下', toolCalls: [call('c1'), call('c2')] },
      { role: 'tool', content: '结果 1', toolCallId: 'c1', name: 'project_list' },
    ]
    expect(normalizeNativeHistory(list)).toEqual([{ role: 'assistant', content: '我查一下' }])
  })
})

describe('nativeRunPlan', () => {
  it('只读工具按 3 个一批并行', () => {
    const plan = nativeRunPlan([1, 2, 3, 4].map((n) => call(`c${n}`)))
    expect(plan.needsGate).toBe(false)
    expect(plan.batches).toEqual([[0, 1, 2], [3]])
  })

  it('含写工具时全部串行，逐个确认', () => {
    const plan = nativeRunPlan([call('c1'), call('c2', 'project_create'), call('c3')])
    expect(plan.needsGate).toBe(true)
    expect(plan.batches).toEqual([[0], [1], [2]])
  })
})
