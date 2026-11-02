/* TOTP / HOTP：RFC 4226 / RFC 6238，全部在本地 WebCrypto 计算 */

const BASE32_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'

export type OtpAlgorithm = 'SHA-1' | 'SHA-256' | 'SHA-512'

export function base32ToBytes(input: string): Uint8Array {
  const cleaned = input.toUpperCase().replace(/[\s-]/g, '').replace(/=+$/, '')
  if (!cleaned) throw new Error('请填写 Base32 密钥')
  let bits = 0
  let value = 0
  const output: number[] = []
  for (const char of cleaned) {
    const index = BASE32_ALPHABET.indexOf(char)
    if (index < 0) throw new Error(`密钥包含非法 Base32 字符：${char}`)
    value = ((value << 5) | index) & 0xffff
    bits += 5
    if (bits >= 8) {
      output.push((value >>> (bits - 8)) & 0xff)
      bits -= 8
    }
  }
  if (!output.length) throw new Error('密钥长度不足，无法解析出有效字节')
  return new Uint8Array(output)
}

/** 复制为独立 ArrayBuffer，规避 TypedArray<ArrayBufferLike> 与 BufferSource 的类型分歧 */
function toBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
}

async function hmac(algorithm: OtpAlgorithm, key: Uint8Array, message: Uint8Array): Promise<Uint8Array> {
  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    toBuffer(key),
    { name: 'HMAC', hash: algorithm },
    false,
    ['sign'],
  )
  return new Uint8Array(await crypto.subtle.sign('HMAC', cryptoKey, toBuffer(message)))
}

function counterToBytes(counter: number): Uint8Array {
  const bytes = new Uint8Array(8)
  let value = BigInt(Math.max(0, Math.floor(counter)))
  for (let index = 7; index >= 0; index -= 1) {
    bytes[index] = Number(value & 0xffn)
    value >>= 8n
  }
  return bytes
}

function dynamicTruncate(hash: Uint8Array, digits: number): string {
  const offset = hash[hash.length - 1] & 0x0f
  const binary =
    ((hash[offset] & 0x7f) << 24) |
    ((hash[offset + 1] & 0xff) << 16) |
    ((hash[offset + 2] & 0xff) << 8) |
    (hash[offset + 3] & 0xff)
  return String(binary % 10 ** digits).padStart(digits, '0')
}

export async function hotp(
  secret: string,
  counter: number,
  digits = 6,
  algorithm: OtpAlgorithm = 'SHA-1',
): Promise<string> {
  const key = base32ToBytes(secret)
  const hash = await hmac(algorithm, key, counterToBytes(counter))
  return dynamicTruncate(hash, digits)
}

export async function totp(
  secret: string,
  options: { period?: number; digits?: number; algorithm?: OtpAlgorithm; timestamp?: number } = {},
): Promise<{ code: string; remaining: number; counter: number; period: number }> {
  const period = Math.min(300, Math.max(1, options.period ?? 30))
  const digits = Math.min(10, Math.max(4, options.digits ?? 6))
  const algorithm = options.algorithm ?? 'SHA-1'
  const now = options.timestamp ?? Date.now()
  const counter = Math.floor(now / 1000 / period)
  const code = await hotp(secret, counter, digits, algorithm)
  const remaining = period - (Math.floor(now / 1000) % period)
  return { code, remaining, counter, period }
}

export interface OtpOptions {
  mode?: 'totp' | 'hotp'
  digits?: number
  period?: number
  algorithm?: OtpAlgorithm
  counter?: number
  issuer?: string
  account?: string
}

export async function otpInfo(input: string, options: OtpOptions = {}): Promise<string> {
  const secret = input.trim().replace(/\s/g, '')
  if (!secret) throw new Error('请填写 Base32 密钥，例如 JBSWY3DPEHPK3PXP')
  const mode = options.mode ?? 'totp'
  const digits = Math.min(10, Math.max(4, Number(options.digits ?? 6)))
  const algorithm = options.algorithm ?? 'SHA-1'
  const issuer = options.issuer?.trim() || 'AstroMan'
  const account = options.account?.trim() || 'user@example.com'
  base32ToBytes(secret)

  if (mode === 'hotp') {
    const counter = Math.max(0, Math.floor(Number(options.counter ?? 0)))
    const code = await hotp(secret, counter, digits, algorithm)
    return [
      `密钥：${secret}`,
      `类型：HOTP（基于计数器）`,
      `算法：${algorithm} · ${digits} 位`,
      `计数器：${counter}`,
      '',
      `验证码：${code}`,
      '',
      `otpauth://hotp/${encodeURIComponent(issuer)}:${encodeURIComponent(account)}?secret=${secret}` +
        `&counter=${counter}&digits=${digits}&algorithm=${algorithm.replace('-', '')}&issuer=${encodeURIComponent(issuer)}`,
    ].join('\n')
  }

  const period = Math.min(300, Math.max(1, Number(options.period ?? 30)))
  const current = await totp(secret, { period, digits, algorithm })
  const previous = await hotp(secret, current.counter - 1, digits, algorithm)
  const next = await hotp(secret, current.counter + 1, digits, algorithm)
  return [
    `密钥：${secret}`,
    `类型：TOTP（基于时间）`,
    `算法：${algorithm} · ${digits} 位 · ${period} 秒一周期`,
    '',
    `当前验证码：${current.code}`,
    `剩余有效：${current.remaining} 秒（周期 ${current.counter}）`,
    `上一周期：${previous}`,
    `下一周期：${next}`,
    '',
    `otpauth://totp/${encodeURIComponent(issuer)}:${encodeURIComponent(account)}?secret=${secret}` +
      `&period=${period}&digits=${digits}&algorithm=${algorithm.replace('-', '')}&issuer=${encodeURIComponent(issuer)}`,
    '',
    '提示：服务端时钟偏差超过一个周期会导致校验失败，可先用「上一 / 下一周期」验证是否为时钟问题。',
  ].join('\n')
}
