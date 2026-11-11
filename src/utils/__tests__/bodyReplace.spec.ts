import { describe, expect, it } from 'vitest'
import { replaceAllLiteral } from '@/utils/bodyReplace'

describe('replaceAllLiteral', () => {
  it('默认忽略大小写，替换全部命中', () => {
    expect(replaceAllLiteral('Abc abc ABC', 'abc', 'x')).toEqual({
      text: 'x x x',
      count: 3,
    })
  })

  it('区分大小写时只替换完全一致的片段', () => {
    expect(replaceAllLiteral('Abc abc ABC', 'abc', 'x', true)).toEqual({
      text: 'Abc x ABC',
      count: 1,
    })
  })

  it('空关键字时原样返回', () => {
    expect(replaceAllLiteral('{"a":1}', '', 'x')).toEqual({ text: '{"a":1}', count: 0 })
  })

  it('替换内容包含关键字也不会死循环', () => {
    expect(replaceAllLiteral('aaa', 'a', 'aa')).toEqual({ text: 'aaaaaa', count: 3 })
  })

  it('支持替换成空字符串（删除）', () => {
    expect(replaceAllLiteral('{"a":1,"b":2}', ',"b":2', '')).toEqual({
      text: '{"a":1}',
      count: 1,
    })
  })
})
