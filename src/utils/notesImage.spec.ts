import { describe, expect, it } from 'vitest'
import { formatBytes, mimeForFileName, sniffImageMime, stripDataUrl, toDataUrl } from './notesImage'

function bytes(values: number[]): Uint8Array {
  return new Uint8Array(values)
}

describe('sniffImageMime', () => {
  it('识别 PNG / JPEG / WebP 魔数', () => {
    expect(sniffImageMime(bytes([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))).toBe(
      'image/png',
    )
    expect(sniffImageMime(bytes([0xff, 0xd8, 0xff, 0xe0]))).toBe('image/jpeg')
    const webp = [
      0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
    ]
    expect(sniffImageMime(bytes(webp))).toBe('image/webp')
  })

  it('非图片返回 null', () => {
    expect(sniffImageMime(bytes([0x25, 0x50, 0x44, 0x46]))).toBeNull()
    expect(sniffImageMime(bytes([]))).toBeNull()
  })
})

describe('数据地址与展示工具', () => {
  it('剥离 data URL 前缀', () => {
    expect(stripDataUrl('data:image/png;base64,AAAA')).toBe('AAAA')
    expect(stripDataUrl('AAAA')).toBe('AAAA')
    expect(toDataUrl('AAAA', 'image/png')).toBe('data:image/png;base64,AAAA')
  })

  it('扩展名映射与体积格式化', () => {
    expect(mimeForFileName('shot.JPG')).toBe('image/jpeg')
    expect(mimeForFileName('shot.webp')).toBe('image/webp')
    expect(mimeForFileName('unknown.bin')).toBe('application/octet-stream')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2 KB')
    expect(formatBytes(8 * 1024 * 1024)).toBe('8.0 MB')
  })
})
