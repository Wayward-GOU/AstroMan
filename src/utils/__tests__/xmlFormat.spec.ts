import { describe, expect, it } from 'vitest'
import { formatXml, looksLikeXml } from '@/utils/xmlFormat'

describe('formatXml', () => {
  it('压缩的 XML 展开成两层缩进', () => {
    const out = formatXml('<root><a>1</a><b><c>2</c></b></root>')
    expect(out).toBe(
      ['<root>', '  <a>1</a>', '  <b>', '    <c>2</c>', '  </b>', '</root>'].join('\n'),
    )
  })

  it('已格式化的内容再次格式化结果不变（幂等）', () => {
    const once = formatXml('<root><a>1</a></root>')
    expect(formatXml(once)).toBe(once)
  })

  it('保留 XML 声明、注释、CDATA 与自闭合标签', () => {
    const out = formatXml(
      '<?xml version="1.0" encoding="UTF-8"?><!-- 备注 --><root><br/><![CDATA[a<b]]></root>',
    )
    expect(out).toBe(
      [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<!-- 备注 -->',
        '<root>',
        '  <br/>',
        '  <![CDATA[a<b]]>',
        '</root>',
      ].join('\n'),
    )
  })

  it('混合内容整体保持一行，不引入空白', () => {
    const out = formatXml('<p>见 <b>说明</b> 结束</p>')
    expect(out).toBe('<p>见 <b>说明</b> 结束</p>')
  })

  it('属性值里的 > 不影响标签边界', () => {
    const out = formatXml('<root><a title="1 > 0">x</a></root>')
    expect(out).toBe(['<root>', '  <a title="1 > 0">x</a>', '</root>'].join('\n'))
  })

  it('中文文本与命名空间', () => {
    const out = formatXml('<ns:r xmlns:ns="urn:x"><ns:名>张三</ns:名></ns:r>')
    expect(out).toBe(
      ['<ns:r xmlns:ns="urn:x">', '  <ns:名>张三</ns:名>', '</ns:r>'].join('\n'),
    )
  })

  it('空元素与无内容元素', () => {
    expect(formatXml('<root><a></a></root>')).toBe(['<root>', '  <a></a>', '</root>'].join('\n'))
  })

  it('标签不匹配时报错并带位置', () => {
    expect(() => formatXml('<root><a></b></root>')).toThrow(/与 <a> 不匹配/)
    expect(() => formatXml('<root><a></root>')).toThrow(/不匹配|未闭合/)
  })

  it('内容为空时报错', () => {
    expect(() => formatXml('   ')).toThrow(/内容为空/)
  })
})

describe('looksLikeXml', () => {
  it('识别 XML 与 HTML', () => {
    expect(looksLikeXml('<?xml version="1.0"?><a/>')).toBe(true)
    expect(looksLikeXml('<root/>')).toBe(true)
    expect(looksLikeXml('  <a>x</a>')).toBe(true)
    expect(looksLikeXml('{"a":1}')).toBe(false)
    expect(looksLikeXml('a < b')).toBe(false)
    expect(looksLikeXml('')).toBe(false)
  })

  it('中文标签名也算 XML', () => {
    expect(looksLikeXml('<数据><项>1</项></数据>')).toBe(true)
  })
})
