import { describe, expect, it } from 'vitest'
import { formatJsonTemplateAsync } from '@/utils/formatJson'

describe('formatJsonTemplateAsync', () => {
  it('裸值占位符格式化后保持不带引号', async () => {
    const out = await formatJsonTemplateAsync('{ "id": "{{uuid}}", "ts": {{timestamp}} }')
    expect(out).toBe('{\n  "id": "{{uuid}}",\n  "ts": {{timestamp}}\n}')
  })

  it('字符串内占位符保留引号与前后文本', async () => {
    const out = await formatJsonTemplateAsync(
      '{"name":"fink_{{randomString(4, lower)}}","n":1}',
    )
    expect(out).toContain('"name": "fink_{{randomString(4, lower)}}"')
    expect(out).toContain('"n": 1')
  })

  it('数组里的裸值占位符', async () => {
    const out = await formatJsonTemplateAsync('{"ids":[{{a}},2]}')
    expect(out).toBe('{\n  "ids": [\n    {{a}},\n    2\n  ]\n}')
  })

  it('普通 JSON 行为不变', async () => {
    const out = await formatJsonTemplateAsync('{"a":1}')
    expect(out).toBe('{\n  "a": 1\n}')
  })
})
