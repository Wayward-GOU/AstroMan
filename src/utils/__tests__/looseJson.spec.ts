import { describe, expect, it } from 'vitest'
import { checkJsonBody, describeLooseRepairs, parseLooseJson } from '@/utils/looseJson'

describe('parseLooseJson', () => {
  it('严格 JSON 原样解析且不产生修复项', () => {
    const out = parseLooseJson('[{"id":"1"}]')
    expect(out.value).toEqual([{ id: '1' }])
    expect(out.repairs).toEqual([])
  })

  it('单引号数组对象 + 尾逗号（请求体里最常见的粘贴形态）', () => {
    const out = parseLooseJson("[{'collectRecordId':'106713254993435459',}]")
    expect(out.value).toEqual([{ collectRecordId: '106713254993435459' }])
    expect(out.repairs).toEqual(['singleQuote', 'trailingComma'])
  })

  it('仅单引号、无尾逗号：修复项只有 singleQuote', () => {
    const out = parseLooseJson("[{'collectRecordId':'1067132549934354559'}]")
    expect(out.value).toEqual([{ collectRecordId: '1067132549934354559' }])
    expect(out.repairs).toEqual(['singleQuote'])
  })

  it('引号内的 19 位数字保持字符串，不丢精度', () => {
    const out = parseLooseJson("['1067132549934354559']")
    // toEqual 区分 '123' 与 123，丢精度会变成 1067132549934354600
    expect(out.value).toEqual(['1067132549934354559'])
  })

  it('无引号属性名与注释', () => {
    const out = parseLooseJson(`{
      // 备注
      id: 1,
      /** 块注释 */
      tags: ['a', 'b',],
    }`)
    expect(out.value).toEqual({ id: 1, tags: ['a', 'b'] })
    expect(out.repairs).toEqual(['comment', 'bareKey', 'singleQuote', 'trailingComma'])
  })

  it('字符串里的 // 不当作注释', () => {
    const out = parseLooseJson('{"url":"https://example.com/a//b"}')
    expect(out.value).toEqual({ url: 'https://example.com/a//b' })
    expect(out.repairs).toEqual([])
  })

  it('单引号串内的转义与 \\u 还原', () => {
    const out = parseLooseJson("{'k':'it\\'s','u':'\\u4e2d'}")
    expect(out.value).toEqual({ k: "it's", u: '中' })
  })

  it('缺少引号或括号时抛出带位置的错误', () => {
    expect(() => parseLooseJson("{'a':1")).toThrow(/字符串未闭合|缺少/)
    expect(() => parseLooseJson('{"a" 1}')).toThrow(/缺少「:」/)
    expect(() => parseLooseJson('{"a":1} trailing')).toThrow(/多余的字符/)
  })

  it('describeLooseRepairs 输出中文标签', () => {
    expect(describeLooseRepairs(['singleQuote', 'trailingComma'])).toBe('单引号字符串、尾逗号')
  })

  it('BOM 头（记事本 / Excel 导出的文件）', () => {
    const out = parseLooseJson('\uFEFF{"a":1}')
    expect(out.value).toEqual({ a: 1 })
    expect(out.repairs).toEqual(['bom'])
  })

  it('整段全角标点（微信 / Word 复制）', () => {
    const out = parseLooseJson('｛“a”：1，“b”：［1，2］｝')
    expect(out.value).toEqual({ a: 1, b: [1, 2] })
    expect(out.repairs).toEqual(['fullwidth'])
  })

  it('Word 只替换了右引号：ASCII 开、全角闭', () => {
    expect(parseLooseJson('{"a”:1}').value).toEqual({ a: 1 })
  })

  it('中文引号在字符串内容里不被当成结束符', () => {
    const out = parseLooseJson("{'t':'他说“你好”，然后走了'}")
    expect(out.value).toEqual({ t: '他说“你好”，然后走了' })
    expect(out.repairs).toEqual(['singleQuote'])
  })

  it('尾随分号（从 Java 代码里复制）', () => {
    const out = parseLooseJson('{"a":1};')
    expect(out.value).toEqual({ a: 1 })
    expect(out.repairs).toEqual(['semicolon'])
  })

  it('裸写的大整数标记为 bigNumber，安全整数不标记', () => {
    expect(parseLooseJson('{"id":1067132549934354559}').repairs).toEqual(['bigNumber'])
    expect(parseLooseJson('{"id":1234567890}').repairs).toEqual([])
  })
})

describe('checkJsonBody', () => {
  it('严格 JSON 返回 null', () => {
    expect(checkJsonBody('{"a":1}')).toBeNull()
  })

  it('宽松能读懂时报告为 loose，并带上命中的修复项', () => {
    expect(checkJsonBody("[{'a':'1'}]")).toEqual({ kind: 'loose', repairs: ['singleQuote'] })
  })

  it('两种解析都失败时报告语法错误与位置', () => {
    const issue = checkJsonBody('{"a" 1}')
    expect(issue?.kind).toBe('invalid')
    if (issue?.kind === 'invalid') expect(issue.message).toContain('缺少「:」')
  })

  it('空串按语法错误处理（发送前由调用方先跳过空正文）', () => {
    expect(checkJsonBody('')?.kind).toBe('invalid')
  })
})
