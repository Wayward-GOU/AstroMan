/**
 * 供脚本沙箱使用的同步加解密工具：AES / DES / 3DES、MD5 / SHA / HMAC、Base64 / Hex。
 *
 * 之所以不直接用浏览器 WebCrypto：脚本是同步执行的（new Function），而 WebCrypto 只有异步 API。
 * 这里的模式与填充命名对齐 Java：AES/CBC/PKCS5Padding 对应 mode: 'cbc' + padding: 'pkcs7'（PKCS5 与 PKCS7 在 16 字节分组下一致）。
 */

import CryptoJS from 'crypto-js'

export type TextEncoding = 'utf8' | 'hex' | 'base64'
export type CipherModeName = 'cbc' | 'ecb' | 'cfb' | 'ofb' | 'ctr'
export type CipherPaddingName = 'pkcs7' | 'zero' | 'none'

export interface CipherOptions {
  /** 默认 cbc；ecb 不需要 iv */
  mode?: CipherModeName
  /** 初始向量，格式由 ivFormat 决定 */
  iv?: string
  ivFormat?: TextEncoding
  /** 密钥格式，默认 utf8 */
  keyFormat?: TextEncoding
  /** 默认 pkcs7（等价 Java 的 PKCS5Padding） */
  padding?: CipherPaddingName
  /** 加密结果的输出格式，默认 base64 */
  output?: 'base64' | 'hex'
  /** 解密入参的格式，默认 base64 */
  input?: 'base64' | 'hex'
}

const MODES: Record<CipherModeName, unknown> = {
  cbc: CryptoJS.mode.CBC,
  ecb: CryptoJS.mode.ECB,
  cfb: CryptoJS.mode.CFB,
  ofb: CryptoJS.mode.OFB,
  ctr: CryptoJS.mode.CTR,
}

function parseText(text: string, format: TextEncoding = 'utf8'): CryptoJS.lib.WordArray {
  if (format === 'hex') return CryptoJS.enc.Hex.parse(text.replace(/\s/g, ''))
  if (format === 'base64') return CryptoJS.enc.Base64.parse(text.replace(/\s+/g, ''))
  return CryptoJS.enc.Utf8.parse(text)
}

function resolveOptions(options: CipherOptions) {
  const modeName: CipherModeName = options.mode ?? 'cbc'
  const mode = MODES[modeName]
  if (!mode) throw new Error(`不支持的加密模式：${modeName}`)
  const paddingName: CipherPaddingName = options.padding ?? 'pkcs7'
  const padding =
    paddingName === 'zero'
      ? CryptoJS.pad.ZeroPadding
      : paddingName === 'none'
        ? CryptoJS.pad.NoPadding
        : CryptoJS.pad.Pkcs7
  const iv =
    modeName === 'ecb' || !options.iv
      ? undefined
      : parseText(options.iv, options.ivFormat ?? 'utf8')
  return { mode, padding, iv }
}

function expectLength(binary: CryptoJS.lib.WordArray, allowed: number[], label: string) {
  const size = binary.sigBytes
  if (!allowed.includes(size)) {
    throw new Error(`${label}长度应为 ${allowed.join(' / ')} 字节，当前为 ${size} 字节`)
  }
}

function blockCipher(
  algorithm: 'aes' | 'des' | 'tripleDes',
  keySizes: number[],
  ivSizes: number[],
) {
  const engine =
    algorithm === 'aes' ? CryptoJS.AES : algorithm === 'des' ? CryptoJS.DES : CryptoJS.TripleDES
  const label = algorithm === 'aes' ? 'AES 密钥' : algorithm === 'des' ? 'DES 密钥' : '3DES 密钥'

  return {
    encrypt(text: string, key: string, options: CipherOptions = {}): string {
      const binaryKey = parseText(key, options.keyFormat ?? 'utf8')
      expectLength(binaryKey, keySizes, label)
      const { mode, padding, iv } = resolveOptions(options)
      if (iv && !ivSizes.includes(iv.sigBytes)) {
        throw new Error(`IV 长度应为 ${ivSizes.join(' / ')} 字节，当前为 ${iv.sigBytes} 字节`)
      }
      const encrypted = engine.encrypt(text, binaryKey, { mode, padding, iv } as never)
      return options.output === 'hex'
        ? encrypted.ciphertext.toString(CryptoJS.enc.Hex)
        : encrypted.ciphertext.toString(CryptoJS.enc.Base64)
    },

    decrypt(cipherText: string, key: string, options: CipherOptions = {}): string {
      const binaryKey = parseText(key, options.keyFormat ?? 'utf8')
      expectLength(binaryKey, keySizes, label)
      const { mode, padding, iv } = resolveOptions(options)
      if (iv && !ivSizes.includes(iv.sigBytes)) {
        throw new Error(`IV 长度应为 ${ivSizes.join(' / ')} 字节，当前为 ${iv.sigBytes} 字节`)
      }
      const source = (options.input === 'hex' ? CryptoJS.enc.Hex : CryptoJS.enc.Base64).parse(
        cipherText.replace(/\s+/g, ''),
      )
      const params = CryptoJS.lib.CipherParams.create({ ciphertext: source })
      const decrypted = engine.decrypt(params, binaryKey, { mode, padding, iv } as never)
      return decrypted.toString(CryptoJS.enc.Utf8)
    },
  }
}

function digestOf(text: string, algorithm: 'md5' | 'sha1' | 'sha256', output = 'hex') {
  const buffer =
    algorithm === 'md5' ? CryptoJS.MD5(text) : algorithm === 'sha1' ? CryptoJS.SHA1(text) : CryptoJS.SHA256(text)
  return buffer.toString(output === 'base64' ? CryptoJS.enc.Base64 : CryptoJS.enc.Hex)
}

function hmacOf(text: string, key: string, algorithm: 'sha1' | 'sha256', output = 'hex') {
  const fn = algorithm === 'sha1' ? CryptoJS.HmacSHA1 : CryptoJS.HmacSHA256
  return fn(text, key).toString(output === 'base64' ? CryptoJS.enc.Base64 : CryptoJS.enc.Hex)
}

export interface CipherFacade {
  /** AES 加密，默认 CBC + PKCS7，返回 Base64 */
  aes: {
    encrypt(text: string, key: string, options?: CipherOptions): string
    decrypt(cipherText: string, key: string, options?: CipherOptions): string
  }
  /** DES 加密（密钥 8 字节） */
  des: {
    encrypt(text: string, key: string, options?: CipherOptions): string
    decrypt(cipherText: string, key: string, options?: CipherOptions): string
  }
  /** 3DES 加密（密钥 24 字节） */
  tripleDes: {
    encrypt(text: string, key: string, options?: CipherOptions): string
    decrypt(cipherText: string, key: string, options?: CipherOptions): string
  }
  md5(text: string, output?: 'hex' | 'base64'): string
  sha1(text: string, output?: 'hex' | 'base64'): string
  sha256(text: string, output?: 'hex' | 'base64'): string
  hmacSha1(text: string, key: string, output?: 'hex' | 'base64'): string
  hmacSha256(text: string, key: string, output?: 'hex' | 'base64'): string
  base64: { encode(text: string): string; decode(text: string): string }
  hex: { encode(text: string): string; decode(text: string): string }
}

export function createCipher(): CipherFacade {
  return {
    aes: blockCipher('aes', [16, 24, 32], [16]),
    des: blockCipher('des', [8], [8]),
    tripleDes: blockCipher('tripleDes', [24], [8]),
    md5: (text, output) => digestOf(String(text ?? ''), 'md5', output),
    sha1: (text, output) => digestOf(String(text ?? ''), 'sha1', output),
    sha256: (text, output) => digestOf(String(text ?? ''), 'sha256', output),
    hmacSha1: (text, key, output) => hmacOf(String(text ?? ''), String(key ?? ''), 'sha1', output),
    hmacSha256: (text, key, output) =>
      hmacOf(String(text ?? ''), String(key ?? ''), 'sha256', output),
    base64: {
      encode: (text) =>
        CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse(String(text ?? ''))),
      decode: (text) =>
        CryptoJS.enc.Base64.parse(String(text ?? '').replace(/\s+/g, '')).toString(
          CryptoJS.enc.Utf8,
        ),
    },
    hex: {
      encode: (text) => CryptoJS.enc.Hex.stringify(CryptoJS.enc.Utf8.parse(String(text ?? ''))),
      decode: (text) =>
        CryptoJS.enc.Hex.parse(String(text ?? '').replace(/\s+/g, '')).toString(
          CryptoJS.enc.Utf8,
        ),
    },
  }
}
