/**
 * 航仔视觉输入（AI-03）：图片校验、压缩与预算估算。
 *
 * 约束与设计文档一致：
 * - 一次最多 3 张；
 * - 长边压到 1568，JPEG q80；
 * - 压缩后单图 ≤1MB；
 * - 每张按 800 token 估算，只随当前轮发送，不落库。
 */

export const MAX_IMAGES = 3
/** 单图上限：压缩后的原始字节（base64 解码后） */
export const MAX_IMAGE_BYTES = 1024 * 1024
/** 每张图片估算 token */
export const IMAGE_TOKEN_ESTIMATE = 800
const MAX_EDGE = 1568

export type SupportedImageMime = 'image/jpeg' | 'image/png' | 'image/webp'

/** 发给后端的图片结构（与 Rust AiImage 对应） */
export interface AiImagePayload {
  mime: string
  data: string
}

/** 输入框上方的待发送图片 */
export interface PendingImage extends AiImagePayload {
  id: string
  name: string
  /** 压缩后的原始字节数 */
  size: number
  /** data URL，用于缩略图预览 */
  preview: string
}

/** 只接受常见位图；GIF / SVG / BMP 统一提示不支持 */
export function supportedImageMime(type: string): SupportedImageMime | null {
  const mime = (type ?? '').trim().toLowerCase()
  if (mime === 'image/jpeg' || mime === 'image/jpg') return 'image/jpeg'
  if (mime === 'image/png') return 'image/png'
  if (mime === 'image/webp') return 'image/webp'
  return null
}

export function estimateImageTokens(count: number): number {
  return Math.max(0, Math.floor(count || 0)) * IMAGE_TOKEN_ESTIMATE
}

/** base64 字符数换算原始字节数（估算，够用于 1MB 门禁） */
export function base64Bytes(data: string): number {
  const text = (data ?? '').replace(/\s+/g, '')
  if (!text) return 0
  const padding = text.endsWith('==') ? 2 : text.endsWith('=') ? 1 : 0
  return Math.floor((text.length * 3) / 4) - padding
}

export function formatImageSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 KB'
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function loadImage(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file)
    const image = new Image()
    image.onload = () => {
      URL.revokeObjectURL(url)
      resolve(image)
    }
    image.onerror = () => {
      URL.revokeObjectURL(url)
      reject(new Error('图片解码失败，请换一张图片'))
    }
    image.src = url
  })
}

/** 等比缩放尺寸：长边不超过 maxEdge */
export function fitSize(width: number, height: number, maxEdge = MAX_EDGE): { width: number; height: number } {
  const safeWidth = Math.max(1, Math.floor(width || 0))
  const safeHeight = Math.max(1, Math.floor(height || 0))
  const longEdge = Math.max(safeWidth, safeHeight)
  if (longEdge <= maxEdge) return { width: safeWidth, height: safeHeight }
  const scale = maxEdge / longEdge
  return {
    width: Math.max(1, Math.round(safeWidth * scale)),
    height: Math.max(1, Math.round(safeHeight * scale)),
  }
}

/**
 * 压缩为 JPEG：长边 1568、q80；仍超 1MB 时依次降质量 / 缩尺寸。
 * 透明区域铺白底，避免 PNG 转 JPEG 后变黑。
 */
export async function compressImageFile(
  file: File,
): Promise<{ mime: 'image/jpeg'; data: string; preview: string; size: number }> {
  const type = supportedImageMime(file.type || '')
  if (!type) throw new Error(`不支持的图片格式：${file.type || file.name}`)
  const image = await loadImage(file)
  const base = fitSize(image.naturalWidth || image.width, image.naturalHeight || image.height)
  const canvas = document.createElement('canvas')
  const context = canvas.getContext('2d')
  if (!context) throw new Error('当前环境不支持图片压缩')

  const attempts: Array<{ scale: number; quality: number }> = [
    { scale: 1, quality: 0.8 },
    { scale: 1, quality: 0.7 },
    { scale: 0.85, quality: 0.7 },
    { scale: 0.7, quality: 0.6 },
    { scale: 0.55, quality: 0.55 },
  ]

  for (const attempt of attempts) {
    const width = Math.max(1, Math.round(base.width * attempt.scale))
    const height = Math.max(1, Math.round(base.height * attempt.scale))
    canvas.width = width
    canvas.height = height
    context.fillStyle = '#ffffff'
    context.fillRect(0, 0, width, height)
    context.drawImage(image, 0, 0, width, height)
    const dataUrl = canvas.toDataURL('image/jpeg', attempt.quality)
    const data = dataUrl.split(',')[1] ?? ''
    if (base64Bytes(data) <= MAX_IMAGE_BYTES) {
      return { mime: 'image/jpeg', data, preview: `data:image/jpeg;base64,${data}`, size: base64Bytes(data) }
    }
  }
  throw new Error(
    `图片压缩后仍超过 ${formatImageSize(MAX_IMAGE_BYTES)}，请换一张或先手动缩小分辨率`,
  )
}
