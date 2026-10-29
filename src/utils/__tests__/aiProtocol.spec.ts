import { describe, expect, it } from 'vitest'
import {
  callKey,
  liftNativeCalls,
  normalizePseudoFence,
  parseSegments,
  sanitizeAssistant,
} from '@/utils/aiProtocol'

describe('callKey', () => {
  it('参数顺序不同但语义相同得到同一指纹', () => {
    const a = callKey('request_get', { id: 1, name: 'x' })
    const b = callKey('request_get', { name: 'x', id: 1 })
    expect(a).toBe(b)
  })

  it('参数不同指纹不同', () => {
    expect(callKey('request_get', { id: 1 })).not.toBe(callKey('request_get', { id: 2 }))
  })

  it('嵌套对象与数组保持稳定', () => {
    const a = callKey('send_request', { headers: { b: 2, a: 1 }, ids: [3, 1] })
    const b = callKey('send_request', { ids: [3, 1], headers: { b: 2, a: 1 } })
    expect(a).toBe(b)
  })
})

describe('normalizePseudoFence', () => {
  it('把 <fbin-tool> 伪围栏还原成标准围栏', () => {
    const out = normalizePseudoFence('<fbin-tool>{"tool":"project_list","args":{}}</fbin-tool>')
    expect(out).toContain('```fbin-tool')
    expect(out).toContain('{"tool":"project_list","args":{}}')
    expect(out).not.toContain('<fbin-tool>')
  })

  it('普通正文原样返回', () => {
    expect(normalizePseudoFence('你好')).toBe('你好')
  })
})

describe('liftNativeCalls', () => {
  it('还原带原生标记的工具 JSON', () => {
    const out = liftNativeCalls('<|DSML|>{"name":"project_list","arguments":{}}<|/DSML|>')
    expect(out).toContain('```fbin-tool')
    expect(out).toContain('"tool":"project_list"')
    expect(out).not.toContain('DSML')
  })

  it('无标记时只识别已知工具，普通 JSON 原样保留', () => {
    const plain = '示例：{"foo":"bar"}'
    expect(liftNativeCalls(plain)).toBe(plain)
  })

  it('代码块里的 JSON 不当成调用', () => {
    const text = '```json\n{"tool":"project_list","args":{}}\n```'
    expect(liftNativeCalls(text)).toBe(text)
  })
})

describe('parseSegments', () => {
  it('解析 fbin-tool 围栏为工具段', () => {
    const segments = parseSegments('```fbin-tool\n{"tool":"project_list","args":{}}\n```')
    expect(segments).toHaveLength(1)
    expect(segments[0].type).toBe('tool')
    expect(segments[0].tool).toBe('project_list')
  })

  it('空工具块被丢弃，不展示也不执行', () => {
    expect(parseSegments('```fbin-tool\n\n```')).toHaveLength(0)
  })

  it('普通代码块保留为 code 段', () => {
    const segments = parseSegments('```js\nconsole.log(1)\n```')
    expect(segments[0].type).toBe('code')
    expect(segments[0].code).toBe('console.log(1)')
  })

  it('流式未闭合围栏不展示', () => {
    expect(parseSegments('正在查询\n```fbin-tool\n{"tool":')).toHaveLength(1)
  })

  it('伪围栏消息经 sanitize 后可解析出工具段', () => {
    const segments = parseSegments('<fbin-tool>{"tool":"project_list","args":{}}</fbin-tool>', true)
    expect(segments.some((seg) => seg.type === 'tool')).toBe(true)
  })

  it('sanitizeAssistant 清理原生标签', () => {
    expect(sanitizeAssistant('好的<|DSML|>')).not.toContain('DSML')
  })
})
