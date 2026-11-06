import { describe, expect, it } from 'vitest'
import {
  MAX_ATTACH_CHARS,
  MAX_ATTACH_TOTAL,
  isTextAttachment,
  prepareAttachment,
} from '@/utils/attachBudget'

describe('prepareAttachment', () => {
  it('正常文件原样接收', () => {
    const result = prepareAttachment(0, 'hello')
    expect(result).toEqual({ accepted: true, content: 'hello' })
  })

  it('单文件超限时截断并提示', () => {
    const result = prepareAttachment(0, 'a'.repeat(MAX_ATTACH_CHARS + 10))
    expect(result.accepted).toBe(true)
    expect(result.content).toHaveLength(MAX_ATTACH_CHARS)
    expect(result.warning).toContain('单文件')
  })

  it('总量超限时按剩余额度截断', () => {
    const result = prepareAttachment(MAX_ATTACH_TOTAL - 2_000, 'b'.repeat(5_000))
    expect(result.accepted).toBe(true)
    expect(result.content).toHaveLength(2_000)
    expect(result.warning).toContain('总量')
  })

  it('剩余额度不足 1000 字符时拒绝，避免生成无意义的碎片', () => {
    const result = prepareAttachment(MAX_ATTACH_TOTAL - 10, 'ccccc')
    expect(result.accepted).toBe(false)
    expect(result.content).toBe('')
    expect(result.warning).toContain('上限')
  })
})

describe('isTextAttachment', () => {
  it('按扩展名放行文本文件，大小写不敏感', () => {
    expect(isTextAttachment('a.JSON')).toBe(true)
    expect(isTextAttachment('notes.md')).toBe(true)
    expect(isTextAttachment('photo.png')).toBe(false)
    expect(isTextAttachment('noext')).toBe(false)
  })
})
