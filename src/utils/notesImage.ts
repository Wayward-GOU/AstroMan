/** 截图与附件的通用工具：魔数校验、base64 编解码、识别前压缩 */

export const MAX_IMAGE_BYTES = 8 * 1024 * 1024
export const MAX_ATTACHMENTS = 5
const RECOGNIZE_MAX_EDGE = 1600

const MIME_BY_EXT: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
}

/** 按文件头魔数判定真实图片类型，扩展名只作兜底 */
export function sniffImageMime(bytes: Uint8Array): string | null {
  if (bytes.length >= 8) {
    const png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
    if (png.every((value, index) => bytes[index] === value)) return 'image/png'
  }
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) {
    return 'image/jpeg'
  }
  if (
    bytes.length >= 12 &&
    String.fromCharCode(...bytes.slice(0, 4)) === 'RIFF' &&
    String.fromCharCode(...bytes.slice(8, 12)) === 'WEBP'
  ) {
    return 'image/webp'
  }
  return null
}

export function mimeForFileName(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  return MIME_BY_EXT[ext] ?? 'application/octet-stream'
}

export function isSupportedImage(name: string, mime: string): boolean {
  return mime === 'image/png' || mime === 'image/jpeg' || mime === 'image/webp'
}

export function stripDataUrl(value: string): string {
  const trimmed = value.trim()
  const index = trimmed.indexOf(',')
  if (trimmed.startsWith('data:') && index >= 0) return trimmed.slice(index + 1)
  return trimmed
}

export function toDataUrl(base64: string, mime: string): string {
  return `data:${mime};base64,${stripDataUrl(base64)}`
}

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(0)} KB`
  return `${(size / 1024 / 1024).toFixed(1)} MB`
}

function readAsArrayBuffer(file: Blob): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as ArrayBuffer)
    reader.onerror = () => reject(new Error('读取图片失败'))
    reader.readAsArrayBuffer(file)
  })
}

export function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

/** 读取文件并校验大小与真实类型；返回 base64 与嗅探出的 mime */
export async function readImageFile(file: File): Promise<{
  name: string
  mime: string
  base64: string
  size: number
}> {
  if (file.size > MAX_IMAGE_BYTES) {
    throw new Error(`图片不能超过 ${formatBytes(MAX_IMAGE_BYTES)}`)
  }
  const buffer = await readAsArrayBuffer(file)
  const sniffed = sniffImageMime(new Uint8Array(buffer))
  const mime = sniffed ?? mimeForFileName(file.name)
  if (!isSupportedImage(file.name, mime)) {
    throw new Error('只支持 PNG / JPG / WebP 图片')
  }
  return {
    name: file.name || '截图',
    mime,
    base64: arrayBufferToBase64(buffer),
    size: file.size,
  }
}

/** 长边超过 1600px 时等比压缩一份用于识别，原图仍按原样保存 */
export async function compressForRecognition(
  base64: string,
  mime: string,
): Promise<{ base64: string; mime: string }> {
  try {
    const dataUrl = toDataUrl(base64, mime)
    const bitmap = await createImageBitmap(await (await fetch(dataUrl)).blob())
    const longEdge = Math.max(bitmap.width, bitmap.height)
    if (longEdge <= RECOGNIZE_MAX_EDGE) {
      bitmap.close()
      return { base64, mime }
    }
    const scale = RECOGNIZE_MAX_EDGE / longEdge
    const width = Math.max(1, Math.round(bitmap.width * scale))
    const height = Math.max(1, Math.round(bitmap.height * scale))
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    const context = canvas.getContext('2d')
    if (!context) {
      bitmap.close()
      return { base64, mime }
    }
    context.drawImage(bitmap, 0, 0, width, height)
    bitmap.close()
    const compressed = canvas.toDataURL('image/jpeg', 0.86)
    return { base64: stripDataUrl(compressed), mime: 'image/jpeg' }
  } catch {
    // 压缩失败就按原图识别，交给后端体积校验
    return { base64, mime }
  }
}
