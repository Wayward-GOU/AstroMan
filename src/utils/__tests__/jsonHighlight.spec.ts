import { describe, expect, it } from 'vitest'
import {
  findMatches,
  highlightJson,
  highlightPlain,
  tokenSearchText,
} from '@/utils/jsonHighlight'

describe('findMatches', () => {
  it('默认忽略大小写并返回不重叠区间', () => {
    expect(findMatches('Name name NAME', 'name')).toEqual([
      { start: 0, end: 4 },
      { start: 5, end: 9 },
      { start: 10, end: 14 },
    ])
  })

  it('区分大小写时只命中完全一致的片段', () => {
    expect(findMatches('Name name NAME', 'name', true)).toEqual([{ start: 5, end: 9 }])
  })

  it('空关键字不产生匹配', () => {
    expect(findMatches('anything', '')).toEqual([])
  })
})

describe('highlightJson', () => {
  it('区分字段名、字符串、数字、注释与 {{变量}}', () => {
    const html = highlightJson(
      '{"name": "fink", "age": 18, // note\n "tpl": "{{id}}", "on": true}',
    )
    expect(html).toContain(
      '<span class="tok-key" data-json-token="name" data-json-kind="key">"name"</span>',
    )
    expect(html).toContain(
      '<span class="tok-string" data-json-token="fink" data-json-kind="string">"fink"</span>',
    )
    expect(html).toContain('<span class="tok-number">18</span>')
    expect(html).toContain('<span class="tok-comment">// note</span>')
    expect(html).toContain('<span class="tok-var">{{id}}</span>')
  })

  it('转义 HTML，避免响应内容被当成标签执行', () => {
    expect(highlightJson('{"html": "<b>&</b>"}')).toContain('&lt;b&gt;&amp;&lt;/b&gt;')
  })

  it('按关键字包 mark，并区分当前命中', () => {
    const html = highlightJson('{"name":"name"}', { query: 'name', currentMatch: 1 })
    expect(html).toContain('<mark class="hl-match" data-match-index="0">name</mark>')
    expect(html).toContain('<mark class="hl-match hl-current" data-match-index="1">name</mark>')
    expect(html.match(/<mark/g)).toHaveLength(2)
  })
})

describe('highlightPlain', () => {
  it('不添加语法着色，只转义并标记搜索命中', () => {
    const html = highlightPlain('<p>Hello hello</p>', { query: 'hello' })
    expect(html).toContain('&lt;p&gt;')
    expect(html).toContain('<mark class="hl-match" data-match-index="0">Hello</mark>')
    expect(html).toContain('<mark class="hl-match" data-match-index="1">hello</mark>')
    expect(html).not.toContain('tok-')
  })
})

describe('tokenSearchText', () => {
  it('去掉字符串两侧引号', () => {
    expect(tokenSearchText('"name"')).toBe('name')
  })

  it('保留内部转义，保证还能在原文本里搜到', () => {
    expect(tokenSearchText('"a\\"b"')).toBe('a\\"b')
  })
})
