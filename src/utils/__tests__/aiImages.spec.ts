import { describe, expect, it } from 'vitest'
import {
  IMAGE_TOKEN_ESTIMATE,
  MAX_IMAGE_BYTES,
  base64Bytes,
  estimateImageTokens,
  fitSize,
  formatImageSize,
  supportedImageMime,
} from '@/utils/aiImages'

describe('supportedImageMime', () => {
  it('接受常见位图并归一化 jpg', () => {
    expect(supportedImageMime('image/jpeg')).toBe('image/jpeg')
    expect(supportedImageMime('image/JPG')).toBe('image/jpeg')
    expect(supportedImageMime('image/png')).toBe('image/png')
    expect(supportedImageMime('image/webp')).toBe('image/webp')
  })

  it('拒绝动图、矢量图与空值', () => {
    expect(supportedImageMime('image/gif')).toBeNull()
    expect(supportedImageMime('image/svg+xml')).toBeNull()
    expect(supportedImageMime('image/bmp')).toBeNull()
    expect(supportedImageMime('')).toBeNull()
  })
})

describe('estimateImageTokens', () => {
  it('每张 800 token，负数与非法值归零', () => {
    expect(estimateImageTokens(0)).toBe(0)
    expect(estimateImageTokens(1)).toBe(IMAGE_TOKEN_ESTIMATE)
    expect(estimateImageTokens(3)).toBe(IMAGE_TOKEN_ESTIMATE * 3)
    expect(estimateImageTokens(-2)).toBe(0)
  })
})

describe('base64Bytes', () => {
  it('按 base64 长度换算原始字节，含 padding', () => {
    // "abc" -> 4 字符
    expect(base64Bytes('YWJj')).toBe(3)
    // "ab" -> 4 字符带一个 =
    expect(base64Bytes('YWI=')).toBe(2)
    // "a" -> 4 字符带两个 =
    expect(base64Bytes('YQ==')).toBe(1)
  })

  it('忽略换行并处理空值', () => {
    expect(base64Bytes('')).toBe(0)
    expect(base64Bytes('YWJj\n')).toBe(3)
  })
})

describe('fitSize', () => {
  it('长边不超过 1568 时保持原尺寸', () => {
    expect(fitSize(800, 600)).toEqual({ width: 800, height: 600 })
    expect(fitSize(1568, 900)).toEqual({ width: 1568, height: 900 })
  })

  it('等比缩放到长边 1568，宽图高图都覆盖', () => {
    expect(fitSize(3136, 1000)).toEqual({ width: 1568, height: 500 })
    expect(fitSize(1000, 3136)).toEqual({ width: 500, height: 1568 })
  })

  it('非法尺寸按 1 处理，避免除零', () => {
    expect(fitSize(0, 0)).toEqual({ width: 1, height: 1 })
  })
})

describe('formatImageSize', () => {
  it('KB / MB 分档展示', () => {
    expect(formatImageSize(0)).toBe('0 KB')
    expect(formatImageSize(512)).toBe('1 KB')
    expect(formatImageSize(MAX_IMAGE_BYTES)).toBe('1.0 MB')
  })
})
