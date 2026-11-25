import { describe, expect, it } from 'vitest'
import { encodeFormPart, parseFormBody } from '@/utils/formBody'

describe('parseFormBody', () => {
  it('标准 a=b&c=d', () => {
    expect(parseFormBody('a=1&b=2')).toEqual([
      { key: 'a', value: '1' },
      { key: 'b', value: '2' },
    ])
  })

  it('多行粘贴按行拆开（原来会被合并成一条错误参数）', () => {
    expect(parseFormBody('a=1\nb=2\r\nc=3')).toEqual([
      { key: 'a', value: '1' },
      { key: 'b', value: '2' },
      { key: 'c', value: '3' },
    ])
  })

  it('DevTools 的 key: value 行式', () => {
    expect(parseFormBody('name: 张三\nage: 18')).toEqual([
      { key: 'name', value: '张三' },
      { key: 'age', value: '18' },
    ])
  })

  it('前导问号来自整段 query string', () => {
    expect(parseFormBody('?a=1&b=2')).toEqual([
      { key: 'a', value: '1' },
      { key: 'b', value: '2' },
    ])
  })

  it('全角 & 与 ＝ 也能拆', () => {
    expect(parseFormBody('a＝1＆b＝2')).toEqual([
      { key: 'a', value: '1' },
      { key: 'b', value: '2' },
    ])
  })

  it('重复 key 不合并', () => {
    expect(parseFormBody('tag=a&tag=b')).toEqual([
      { key: 'tag', value: 'a' },
      { key: 'tag', value: 'b' },
    ])
  })

  it('值里的等号只按第一个等号切', () => {
    expect(parseFormBody('token=abc==&x=1')).toEqual([
      { key: 'token', value: 'abc==' },
      { key: 'x', value: '1' },
    ])
  })

  it('中文未编码与 + 号空格', () => {
    expect(parseFormBody('name=张三&q=hello+world')).toEqual([
      { key: 'name', value: '张三' },
      { key: 'q', value: 'hello world' },
    ])
  })

  it('空值与无值段都保留', () => {
    expect(parseFormBody('a=&flag')).toEqual([
      { key: 'a', value: '' },
      { key: 'flag', value: '' },
    ])
  })

  it('空正文返回空数组', () => {
    expect(parseFormBody('   ')).toEqual([])
  })
})

describe('encodeFormPart', () => {
  it('保留 {{变量}} 不编码', () => {
    expect(encodeFormPart('{{token}}')).toBe('{{token}}')
    expect(encodeFormPart('Bearer {{token}}')).toBe('Bearer%20{{token}}')
  })

  it('编码 & = 与中文', () => {
    expect(encodeFormPart('a&b=c')).toBe('a%26b%3Dc')
    expect(encodeFormPart('张三')).toBe('%E5%BC%A0%E4%B8%89')
  })
})
