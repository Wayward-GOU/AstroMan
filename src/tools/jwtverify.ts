/* JWT 验签：支持 HS / RS / PS / ES 系列，公钥可用 PEM 或 JWK */

import { formatDateTime } from '@/utils/datetime'

export type JwtAlg = 'HS256' | 'HS384' | 'HS512' | 'RS256' | 'RS384' | 'RS512' | 'PS256' | 'PS384' | 'PS512' | 'ES256' | 'ES384' | 'ES512' | 'none'

interface ParsedJwt {
  header: Record<string, unknown>
  payload: Record<string, unknown>
  signingInput: string
  signature: Uint8Array
  alg: JwtAlg
}

function base64UrlToBytes(part: string): Uint8Array {
  const normalized = part.replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=')
  let binary: string
  try {
    binary = atob(padded)
  } catch {
    throw new Error('签名段不是合法的 Base64URL')
  }
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

function base64UrlToText(part: string): string {
  return new TextDecoder().decode(base64UrlToBytes(part))
}

/** 复制为独立 ArrayBuffer，规避 TypedArray<ArrayBufferLike> 与 BufferSource 的类型分歧 */
function toBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
}

export function parseJwt(token: string): ParsedJwt {
  const raw = token.trim().replace(/^Bearer\s+/i, '')
  const parts = raw.split('.')
  if (parts.length < 3) throw new Error('JWT 需要 Header.Payload.Signature 三段，当前不足三段，无法验签')
  const parseJson = (text: string, label: string) => {
    try {
      const value = JSON.parse(text)
      if (typeof value !== 'object' || value === null) throw new Error('不是对象')
      return value as Record<string, unknown>
    } catch {
      throw new Error(`${label} 不是合法的 JSON`)
    }
  }
  const header = parseJson(base64UrlToText(parts[0]), 'Header')
  const payload = parseJson(base64UrlToText(parts[1]), 'Payload')
  const alg = String(header.alg ?? '') as JwtAlg
  if (!alg) throw new Error('Header 中缺少 alg 字段')
  return {
    header,
    payload,
    signingInput: `${parts[0]}.${parts[1]}`,
    signature: base64UrlToBytes(parts[2]),
    alg,
  }
}

function hashOf(alg: string): 'SHA-256' | 'SHA-384' | 'SHA-512' {
  if (alg.endsWith('384')) return 'SHA-384'
  if (alg.endsWith('512')) return 'SHA-512'
  return 'SHA-256'
}

function curveOf(alg: string): 'P-256' | 'P-384' | 'P-521' {
  if (alg === 'ES384') return 'P-384'
  if (alg === 'ES512') return 'P-521'
  return 'P-256'
}

function pemToBytes(pem: string): Uint8Array {
  const body = pem
    .replace(/-----BEGIN [^-]+-----/g, '')
    .replace(/-----END [^-]+-----/g, '')
    .replace(/\s+/g, '')
  if (!body) throw new Error('公钥内容为空')
  let binary: string
  try {
    binary = atob(body)
  } catch {
    throw new Error('公钥不是合法的 PEM / Base64 内容')
  }
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

async function importPublicKey(alg: JwtAlg, key: string): Promise<CryptoKey> {
  const hash = hashOf(alg)
  const family = alg.slice(0, 2)
  const algorithm =
    family === 'RS'
      ? { name: 'RSASSA-PKCS1-v1_5', hash }
      : family === 'PS'
        ? { name: 'RSA-PSS', hash }
        : { name: 'ECDSA', namedCurve: curveOf(alg) }
  const trimmed = key.trim()
  if (trimmed.startsWith('{')) {
    let jwk: JsonWebKey
    try {
      jwk = JSON.parse(trimmed)
    } catch {
      throw new Error('JWK 不是合法的 JSON')
    }
    return crypto.subtle.importKey('jwk', jwk, algorithm, false, ['verify'])
  }
  return crypto.subtle.importKey('spki', toBuffer(pemToBytes(trimmed)), algorithm, false, ['verify'])
}

async function verifySignature(parsed: ParsedJwt, key: string): Promise<boolean> {
  const { alg } = parsed
  if (alg === 'none') return false
  const data = new TextEncoder().encode(parsed.signingInput)
  const signature = toBuffer(parsed.signature)
  const family = alg.slice(0, 2)
  if (family === 'HS') {
    if (!key) throw new Error('HS 系列算法需要填写共享密钥')
    const cryptoKey = await crypto.subtle.importKey(
      'raw',
      toBuffer(new TextEncoder().encode(key)),
      { name: 'HMAC', hash: hashOf(alg) },
      false,
      ['verify'],
    )
    return crypto.subtle.verify('HMAC', cryptoKey, signature, toBuffer(data))
  }
  if (!key.trim()) throw new Error(`${alg} 需要填写公钥（PEM 或 JWK JSON）`)
  const cryptoKey = await importPublicKey(alg, key)
  if (family === 'PS') {
    const saltLength = alg.endsWith('384') ? 48 : alg.endsWith('512') ? 64 : 32
    return crypto.subtle.verify({ name: 'RSA-PSS', saltLength }, cryptoKey, signature, toBuffer(data))
  }
  if (family === 'ES') {
    return crypto.subtle.verify({ name: 'ECDSA', hash: hashOf(alg) }, cryptoKey, signature, toBuffer(data))
  }
  return crypto.subtle.verify('RSASSA-PKCS1-v1_5', cryptoKey, signature, toBuffer(data))
}

function claimLine(label: string, seconds: unknown): string | null {
  if (typeof seconds !== 'number' || !Number.isFinite(seconds)) return null
  return `${label}：${formatDateTime(new Date(seconds * 1000))}（${seconds}）`
}

export async function verifyJwt(token: string, key: string): Promise<string> {
  const parsed = parseJwt(token)
  const { alg, header, payload } = parsed
  const lines: string[] = [`算法：${alg}`]

  if (alg === 'none') {
    lines.push('签名校验：跳过（alg=none 表示不签名，仅可用于本地调试，服务端不应接受）')
  } else {
    let ok = false
    try {
      ok = await verifySignature(parsed, key)
    } catch (error) {
      throw new Error(`验签失败：${error instanceof Error ? error.message : String(error)}`)
    }
    lines.push(`签名校验：${ok ? '通过 ✅' : '未通过 ❌（密钥不匹配或内容已被篡改）'}`)
  }

  const now = Math.floor(Date.now() / 1000)
  const exp = typeof payload.exp === 'number' ? payload.exp : null
  const nbf = typeof payload.nbf === 'number' ? payload.nbf : null
  const iat = typeof payload.iat === 'number' ? payload.iat : null

  const claimLines = [
    claimLine('签发时间 iat', iat),
    claimLine('生效时间 nbf', nbf),
    claimLine('过期时间 exp', exp),
  ].filter((line): line is string => Boolean(line))
  if (claimLines.length) lines.push('', ...claimLines)

  if (exp !== null) {
    if (exp <= now) {
      const minutes = Math.round((now - exp) / 60)
      lines.push(`状态：已过期 ${minutes} 分钟`)
    } else {
      const remain = exp - now
      const text = remain >= 3600 ? `${Math.round(remain / 3600)} 小时` : `${Math.round(remain / 60)} 分钟`
      lines.push(`状态：有效，剩余约 ${text}`)
    }
  }
  if (nbf !== null && nbf > now) lines.push(`状态：尚未生效（还需 ${nbf - now} 秒）`)

  lines.push(
    '',
    'Header：',
    JSON.stringify(header, null, 2),
    '',
    'Payload：',
    JSON.stringify(payload, null, 2),
  )
  return lines.join('\n')
}
