import { describe, expect, it } from 'vitest'
import {
  DEFAULT_CHUNK_OPTIONS,
  formatCharCount,
  nextChunkEnd,
  splitChunks,
} from '@/utils/textChunk'

/** 造一段每行固定长度、带换行的文本 */
function makeLines(count: number, lineLength = 40): string {
  const line = 'x'.repeat(lineLength)
  return Array.from({ length: count }, () => line).join('\n')
}

describe('nextChunkEnd', () => {
  it('剩余内容不足一块时返回全文长度', () => {
    expect(nextChunkEnd('abc', 0, 10)).toBe(3)
  })

  it('起点已到末尾时返回长度，不越界', () => {
    const text = makeLines(3)
    expect(nextChunkEnd(text, text.length, 10)).toBe(text.length)
    expect(nextChunkEnd(text, text.length + 50, 10)).toBe(text.length)
  })

  it('切点落在换行符之后', () => {
    // 每行 5 字符 + 换行，第 1 块想取 8 个字符，应回退到第一个换行后（位置 6）
    const text = 'aaaaa\nbbbbb\nccccc'
    const end = nextChunkEnd(text, 0, 8)
    expect(end).toBe(6)
    expect(text[end - 1]).toBe('\n')
  })

  it('单行超长（无换行）时按字符切，仍能推进', () => {
    const text = 'x'.repeat(1000)
    const end = nextChunkEnd(text, 0, 100)
    expect(end).toBe(100)
    expect(end).toBeGreaterThan(0)
  })

  it('want 为 0 或非正数时至少推进 1 个字符', () => {
    const text = 'x'.repeat(100)
    expect(nextChunkEnd(text, 0, 0)).toBe(1)
    expect(nextChunkEnd(text, 0, -5)).toBe(1)
  })

  it('换行只出现在起点之前时退回按长度切', () => {
    const text = '\n' + 'x'.repeat(100)
    // 从 1 开始，后面 50 个字符没有换行
    expect(nextChunkEnd(text, 1, 50)).toBe(51)
  })

  it('块长度不超过请求长度（除最后一块）', () => {
    const text = makeLines(100, 30)
    for (let from = 0; from < text.length; ) {
      const end = nextChunkEnd(text, from, 64)
      if (end < text.length) expect(end - from).toBeLessThanOrEqual(64)
      expect(end).toBeGreaterThan(from)
      from = end
    }
  })
})

describe('splitChunks', () => {
  it('空文本返回空数组', () => {
    expect(splitChunks('')).toEqual([])
  })

  it('短文本只切一块', () => {
    expect(splitChunks('hello', { initialChars: 1024, stepChars: 1024 })).toEqual(['hello'])
  })

  it('分块拼接后与原文完全一致，不丢字符也不重复', () => {
    const text = makeLines(5000, 60)
    const chunks = splitChunks(text, { initialChars: 1000, stepChars: 2000 })
    expect(chunks.join('')).toBe(text)
    expect(chunks.length).toBeGreaterThan(1)
  })

  it('首块按 initialChars 控制，后续按 stepChars 控制', () => {
    const text = 'x'.repeat(10_000)
    const chunks = splitChunks(text, { initialChars: 1000, stepChars: 3000 })
    expect(chunks[0].length).toBe(1000)
    expect(chunks[1].length).toBe(3000)
    expect(chunks[2].length).toBe(3000)
    expect(chunks.join('')).toBe(text)
  })

  it('默认参数下 5MB 文本的首块远小于全文', () => {
    const text = 'x'.repeat(5 * 1024 * 1024)
    const chunks = splitChunks(text)
    expect(chunks[0].length).toBe(DEFAULT_CHUNK_OPTIONS.initialChars)
    expect(chunks.length).toBeGreaterThan(30)
    expect(chunks.join('').length).toBe(text.length)
  })

  it('单行超长文本也能完整切完，不陷入死循环', () => {
    const text = 'x'.repeat(5000)
    const chunks = splitChunks(text, { initialChars: 100, stepChars: 900 })
    expect(chunks.join('')).toBe(text)
    expect(chunks.every((item) => item.length <= 900)).toBe(true)
  })
})

describe('formatCharCount', () => {
  it('按量级选择单位', () => {
    expect(formatCharCount(0)).toBe('0 字符')
    expect(formatCharCount(999)).toBe('999 字符')
    expect(formatCharCount(1500)).toBe('1.5k 字符')
    expect(formatCharCount(2_500_000)).toBe('2.50M 字符')
  })
})
