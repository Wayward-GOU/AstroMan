import { describe, expect, it } from 'vitest'
import { inferBodyContentType } from '@/utils/bodyContentType'

describe('inferBodyContentType', () => {
  it('固定类型直接映射', () => {
    expect(inferBodyContentType('json', '{"a":1}')).toBe('application/json')
    expect(inferBodyContentType('json', '')).toBe('application/json')
    expect(inferBodyContentType('graphql', '{"query":"{a}"}')).toBe('application/json')
    expect(inferBodyContentType('form', 'a=1')).toBe('application/x-www-form-urlencoded')
    expect(inferBodyContentType('binary', '/tmp/a.png')).toBe('application/octet-stream')
  })

  it('Raw 内容按 JSON / XML / 文本识别', () => {
    expect(inferBodyContentType('raw', '{"a":1}')).toBe('application/json')
    expect(inferBodyContentType('raw', '[1,2]')).toBe('application/json')
    expect(inferBodyContentType('raw', '<root><a>1</a></root>')).toBe('application/xml; charset=utf-8')
    expect(inferBodyContentType('raw', '<?xml version="1.0"?><a/>')).toBe(
      'application/xml; charset=utf-8',
    )
    expect(inferBodyContentType('raw', 'hello world')).toBe('text/plain; charset=utf-8')
    expect(inferBodyContentType('raw', '')).toBe('text/plain; charset=utf-8')
  })

  it('Raw 里的非法 JSON 不硬标成 application/json', () => {
    expect(inferBodyContentType('raw', "{'a':1}")).toBe('text/plain; charset=utf-8')
    expect(inferBodyContentType('raw', '{"a":1')).toBe('text/plain; charset=utf-8')
  })

  it('Raw 里的 JSON 模板按 JSON 处理（{{变量}} 不算语法错误）', () => {
    expect(inferBodyContentType('raw', '{"ts": {{timestamp}}}')).toBe('application/json')
    expect(inferBodyContentType('raw', '{"id":"{{uuid}}","n":1}')).toBe('application/json')
  })
})
