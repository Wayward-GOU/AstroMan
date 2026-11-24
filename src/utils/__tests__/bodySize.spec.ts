import { describe, expect, it } from 'vitest'
import {
  REQUEST_BODY_MAX_BYTES,
  REQUEST_BODY_WARN_BYTES,
  UPLOAD_FILE_MAX_BYTES,
  bodyBytes,
  bodySizeLevel,
  isFileBackedBodyType,
} from '@/utils/bodySize'

describe('bodyBytes', () => {
  it('空值返回 0', () => {
    expect(bodyBytes('')).toBe(0)
    expect(bodyBytes(null)).toBe(0)
    expect(bodyBytes(undefined)).toBe(0)
  })

  it('ASCII 一个字符一字节', () => {
    expect(bodyBytes('abc')).toBe(3)
    expect(bodyBytes('{"a":1}')).toBe(7)
  })

  it('中文按 UTF-8 算三字节，不能按字符数判断', () => {
    expect(bodyBytes('中文')).toBe(6)
    expect(bodyBytes('a中')).toBe(4)
  })

  it('emoji（代理对）算四字节', () => {
    expect(bodyBytes('😀')).toBe(4)
    expect(bodyBytes('😀a')).toBe(5)
  })

  it('中文正文的字节数约为字符数的三倍', () => {
    const text = '汉'.repeat(1000)
    expect(bodyBytes(text)).toBe(3000)
  })
})

describe('bodySizeLevel', () => {
  it('分界线本身属于下一档的边界外', () => {
    expect(bodySizeLevel(0)).toBe('ok')
    expect(bodySizeLevel(REQUEST_BODY_WARN_BYTES)).toBe('ok')
    expect(bodySizeLevel(REQUEST_BODY_WARN_BYTES + 1)).toBe('warn')
    expect(bodySizeLevel(REQUEST_BODY_MAX_BYTES)).toBe('warn')
    expect(bodySizeLevel(REQUEST_BODY_MAX_BYTES + 1)).toBe('blocked')
  })

  it('两个阈值是递进关系', () => {
    expect(REQUEST_BODY_WARN_BYTES).toBeLessThan(REQUEST_BODY_MAX_BYTES)
  })
})

describe('isFileBackedBodyType', () => {
  it('binary 与 multipart 的正文只存文件路径，不计入体积上限', () => {
    expect(isFileBackedBodyType('binary')).toBe(true)
    expect(isFileBackedBodyType('multipart')).toBe(true)
  })

  it('文本类正文照常计入', () => {
    for (const type of ['json', 'raw', 'form', 'graphql']) {
      expect(isFileBackedBodyType(type)).toBe(false)
    }
    expect(isFileBackedBodyType('none')).toBe(false)
    expect(isFileBackedBodyType('')).toBe(false)
    expect(isFileBackedBodyType(null)).toBe(false)
  })
})

describe('UPLOAD_FILE_MAX_BYTES', () => {
  it('单个上传文件上限是 2GB', () => {
    expect(UPLOAD_FILE_MAX_BYTES).toBe(2 * 1024 * 1024 * 1024)
  })

  it('远大于正文硬上限，文件不会被正文阈值误伤', () => {
    expect(UPLOAD_FILE_MAX_BYTES).toBeGreaterThan(REQUEST_BODY_MAX_BYTES * 10)
  })
})
