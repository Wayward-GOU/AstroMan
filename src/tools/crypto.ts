/* 加解密、JWT、正则测试等与浏览器 WebCrypto 相关的工具 */

import { toolHmac } from '@/api'
import { bytesToBase64, base64ToBytes, base64UrlDecode, base64UrlEncode } from './transforms'

export async function deriveAesKey(secret: string): Promise<CryptoKey> {
  if (!secret) throw new Error('请填写 AES 密钥')
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(secret))
  return crypto.subtle.importKey('raw', digest, { name: 'AES-CBC' }, false, ['encrypt', 'decrypt'])
}

export async function aesEncrypt(text: string, secret: string): Promise<string> {
  if (!text) throw new Error('请输入要加密的明文')
  const key = await deriveAesKey(secret)
  const iv = crypto.getRandomValues(new Uint8Array(16))
  const encrypted = await crypto.subtle.encrypt(
    { name: 'AES-CBC', iv },
    key,
    new TextEncoder().encode(text),
  )
  return `${bytesToBase64(iv)}:${bytesToBase64(new Uint8Array(encrypted))}`
}

export async function aesDecrypt(payload: string, secret: string): Promise<string> {
  const key = await deriveAesKey(secret)
  const parts = payload.trim().split(':')
  if (parts.length !== 2) throw new Error('密文格式应为 IV:密文（Base64）')
  const iv = base64ToBytes(parts[0])
  if (iv.length !== 16) throw new Error('IV 必须是 16 字节的 Base64')
  const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-CBC', iv },
    key,
    base64ToBytes(parts[1]),
  )
  return new TextDecoder().decode(decrypted)
}

export interface JwtParts {
  header: string
  payload: string
  signature: string
  headerRaw: string
  payloadRaw: string
}

/// 把 HMAC 的十六进制结果转成 Base64URL 签名段
function hexToBase64Url(hex: string): string {
  const cleaned = hex.trim().replace(/[^0-9a-fA-F]/g, '')
  const bytes = new Uint8Array(Math.floor(cleaned.length / 2))
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = parseInt(cleaned.slice(index * 2, index * 2 + 2), 16)
  }
  return bytesToBase64(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export async function signJwt(
  payloadText: string,
  secret: string,
  algorithm: 'HS256' | 'HS512' = 'HS256',
  expireMinutes = 60,
): Promise<string> {
  if (!secret) throw new Error('请填写签名密钥')
  if (!payloadText.trim()) throw new Error('请填写 Payload JSON')
  let payload: Record<string, unknown>
  try {
    const parsed = JSON.parse(payloadText)
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error('Payload 需要是 JSON 对象')
    }
    payload = parsed as Record<string, unknown>
  } catch (error) {
    throw new Error(`Payload 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
  const now = Math.floor(Date.now() / 1000)
  if (!payload.iat) payload.iat = now
  if (expireMinutes > 0) payload.exp = now + Math.round(expireMinutes * 60)
  const header = base64UrlEncode(JSON.stringify({ alg: algorithm, typ: 'JWT' }))
  const body = base64UrlEncode(JSON.stringify(payload))
  const signingInput = `${header}.${body}`
  const digest = algorithm === 'HS512' ? 'sha512' : 'sha256'
  const signature = hexToBase64Url(await toolHmac(signingInput, secret, digest))
  return `${signingInput}.${signature}`
}

export function decodeJwt(token: string): JwtParts {
  const parts = token.trim().replace(/^Bearer\s+/i, '').split('.')
  if (parts.length < 2) throw new Error('JWT 需要至少包含 Header 与 Payload 两段')
  const decodePart = (part: string) => {
    try {
      return JSON.stringify(JSON.parse(base64UrlDecode(part)), null, 2)
    } catch {
      return base64UrlDecode(part)
    }
  }
  return {
    headerRaw: parts[0],
    payloadRaw: parts[1],
    signature: parts[2] ?? '',
    header: decodePart(parts[0]),
    payload: decodePart(parts[1]),
  }
}

export interface RegexMatch {
  index: number
  text: string
  groups: string[]
  named: Record<string, string>
}

export interface RegexResult {
  matches: RegexMatch[]
  text: string
}

export function runRegex(pattern: string, flags: string, text: string): RegexResult {
  if (!pattern) throw new Error('请填写正则表达式')
  const uniqueFlags = Array.from(new Set((flags || 'g').replace(/[^gimsuy]/g, '') + 'g')).join('')
  const regex = new RegExp(pattern, uniqueFlags)
  const matches: RegexMatch[] = []
  let match: RegExpExecArray | null
  let guard = 0
  while ((match = regex.exec(text)) !== null) {
    matches.push({
      index: match.index,
      text: match[0],
      groups: match.slice(1).map((item) => item ?? ''),
      named: { ...(match.groups ?? {}) },
    })
    if (match[0] === '') regex.lastIndex += 1
    guard += 1
    if (guard > 5000) break
  }
  if (!matches.length) {
    return { matches, text: '没有匹配到内容' }
  }
  const lines = matches.map((item, order) => {
    const parts = [`#${order + 1} 位置 ${item.index} 长度 ${item.text.length}：${item.text}`]
    if (item.groups.length) {
      parts.push(`  分组：${item.groups.map((group, index) => `$${index + 1}=${group}`).join('  ')}`)
    }
    if (Object.keys(item.named).length) {
      parts.push(`  命名分组：${Object.entries(item.named).map(([key, value]) => `${key}=${value}`).join('  ')}`)
    }
    return parts.join('\n')
  })
  const highlighted = text.replace(regex, (value) => `[[${value}]]`)
  return {
    matches,
    text: `${lines.join('\n')}\n\n共 ${matches.length} 处匹配\n\n标记结果：\n${highlighted}`,
  }
}
