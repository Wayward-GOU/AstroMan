import { describe, expect, it } from 'vitest'
import { TAG_TONES, tagTone, tagToneClass } from './notesTags'

describe('notesTags', () => {
  it('同名标签始终落在同一个色号', () => {
    expect(tagTone('工作')).toBe(tagTone('工作'))
    expect(tagToneClass('工作')).toBe(tagToneClass('工作'))
  })

  it('色号始终在调色板范围内', () => {
    const names = ['工作', '个人', '学习', 'A', 'b', '#', '很长很长的标签名称']
    names.forEach((name) => {
      const tone = tagTone(name)
      expect(tone).toBeGreaterThanOrEqual(0)
      expect(tone).toBeLessThan(TAG_TONES)
      expect(tagToneClass(name)).toBe(`tone-${tone}`)
    })
  })

  it('不同标签大概率不会全挤在同一色号', () => {
    const tones = new Set(
      ['工作', '个人', '学习', '项目', '生活', '灵感', '待读', '购物'].map(tagTone),
    )
    expect(tones.size).toBeGreaterThan(1)
  })
})
