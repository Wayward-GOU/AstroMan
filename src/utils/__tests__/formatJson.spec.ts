import { describe, expect, it } from 'vitest'
import {
  checkJsonTemplate,
  formatJsonTemplateAsync,
  formatJsonTemplateLooseAsync,
} from '@/utils/formatJson'

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

describe('formatJsonTemplateLooseAsync', () => {
  it('严格 JSON 数组正常格式化，且不报修复项', async () => {
    const result = await formatJsonTemplateLooseAsync('[{"collectRecordId":"106713254993435459"}]')
    expect(result.text).toBe('[\n  {\n    "collectRecordId": "106713254993435459"\n  }\n]')
    expect(result.repairs).toEqual([])
  })

  it('单引号 + 尾逗号的数组对象不再报错，并回报修复项', async () => {
    const result = await formatJsonTemplateLooseAsync("[{'collectRecordId':'106713254993435459',}]")
    expect(result.text).toBe('[\n  {\n    "collectRecordId": "106713254993435459"\n  }\n]')
    expect(result.repairs).toEqual(['singleQuote', 'trailingComma'])
  })

  it('只有单引号、没有尾逗号（截图原始内容，43 B）也能格式化', async () => {
    const source = "[{'collectRecordId':'1067132549934354559'}]"
    expect(Buffer.byteLength(source)).toBe(43)
    const result = await formatJsonTemplateLooseAsync(source)
    expect(result.text).toBe('[\n  {\n    "collectRecordId": "1067132549934354559"\n  }\n]')
    expect(result.repairs).toEqual(['singleQuote'])
    // 19 位数字在引号内必须保持字符串，不能变成精度丢失的 number
    expect(result.text).toContain('"1067132549934354559"')
  })

  it('单引号串内的占位符不补引号，还原后仍是字符串', async () => {
    const result = await formatJsonTemplateLooseAsync("{'id':'{{uuid}}'}")
    expect(result.text).toBe('{\n  "id": "{{uuid}}"\n}')
    expect(result.repairs).toEqual(['singleQuote'])
  })

  it('单引号 + 裸值占位符混用时占位符保持不带引号', async () => {
    const result = await formatJsonTemplateLooseAsync("{'id':'{{uuid}}','ts':{{timestamp}}}")
    expect(result.text).toBe('{\n  "id": "{{uuid}}",\n  "ts": {{timestamp}}\n}')
  })

  it('全角标点整段修复为标准 JSON', async () => {
    const result = await formatJsonTemplateLooseAsync('｛“a”：1｝')
    expect(result.text).toBe('{\n  "a": 1\n}')
    expect(result.repairs).toEqual(['fullwidth'])
  })

  it('BOM 头不影响格式化', async () => {
    const result = await formatJsonTemplateLooseAsync('\uFEFF{"a":1}')
    expect(result.text).toBe('{\n  "a": 1\n}')
    expect(result.repairs).toEqual(['bom'])
  })
})

describe('checkJsonTemplate（发送前体检）', () => {
  it('裸值占位符不被当成语法错误', () => {
    expect(checkJsonTemplate('{"ts": {{timestamp}}}')).toBeNull()
    expect(checkJsonTemplate('{"id":"{{uuid}}","n":{{n}}}')).toBeNull()
  })

  it('占位符之外的宽松问题仍能识别', () => {
    expect(checkJsonTemplate('{"ts": {{timestamp}},}')?.kind).toBe('loose')
    expect(checkJsonTemplate("{'id':'{{uuid}}'}")?.kind).toBe('loose')
  })

  it('真正的语法错误报告为 invalid', () => {
    expect(checkJsonTemplate('{"a" {{x}}}')?.kind).toBe('invalid')
  })
})
