/**
 * RSA 公钥加密（PKCS#1 v1.5 + Base64），与 Java 中
 * Cipher.getInstance("RSA") / Cipher.getInstance("RSA/ECB/PKCS1Padding") 的结果一致。
 *
 * 只做公钥加密，不依赖第三方库：DER 解析 + 大数模幂都在这里实现，
 * 以便在脚本沙箱（同步执行）中直接调用。
 */

interface DerNode {
  tag: number
  contentStart: number
  contentEnd: number
}

function base64ToBytes(text: string): Uint8Array {
  const binary = atob(text.replace(/\s/g, ''))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
}

function readDerNode(der: Uint8Array, offset: number): DerNode {
  if (offset >= der.length) throw new Error('公钥 DER 结构不完整')
  const tag = der[offset]
  let cursor = offset + 1
  let length = der[cursor]
  cursor += 1
  if (length & 0x80) {
    const count = length & 0x7f
    length = 0
    for (let index = 0; index < count; index += 1) {
      length = length * 256 + der[cursor]
      cursor += 1
    }
  }
  if (cursor + length > der.length) throw new Error('公钥 DER 长度越界')
  return { tag, contentStart: cursor, contentEnd: cursor + length }
}

function bytesToBigInt(bytes: Uint8Array): bigint {
  let hex = ''
  bytes.forEach((byte) => {
    hex += byte.toString(16).padStart(2, '0')
  })
  return hex ? BigInt(`0x${hex}`) : 0n
}

function readInteger(der: Uint8Array, node: DerNode): bigint {
  if (node.tag !== 0x02) throw new Error('公钥格式错误：缺少 INTEGER')
  return bytesToBigInt(der.slice(node.contentStart, node.contentEnd))
}

interface RsaPublicKey {
  modulus: bigint
  exponent: bigint
}

/** 支持 X.509 SubjectPublicKeyInfo（PEM / 纯 Base64）与 PKCS#1 RSAPublicKey 两种公钥格式 */
function parsePublicKey(publicKey: string): RsaPublicKey {
  const body = publicKey
    .replace(/-----(BEGIN|END)[^-]*-----/g, '')
    .replace(/\s+/g, '')
  if (!body) throw new Error('请填写 RSA 公钥')
  const der = base64ToBytes(body)
  const outer = readDerNode(der, 0)
  if (outer.tag !== 0x30) throw new Error('公钥不是合法的 DER 结构')

  const first = readDerNode(der, outer.contentStart)
  if (first.tag === 0x02) {
    const modulus = readInteger(der, first)
    const exponent = readInteger(der, readDerNode(der, first.contentEnd))
    return { modulus, exponent }
  }

  if (first.tag !== 0x30) throw new Error('公钥格式错误：未找到算法标识')
  const bitString = readDerNode(der, first.contentEnd)
  if (bitString.tag !== 0x03) throw new Error('公钥格式错误：缺少 BIT STRING')
  // BIT STRING 第一个字节是未使用位数，公钥内容从下一个字节开始
  const keyNode = readDerNode(der, bitString.contentStart + 1)
  if (keyNode.tag !== 0x30) throw new Error('公钥格式错误：BIT STRING 内不是 RSAPublicKey')
  const modulusNode = readDerNode(der, keyNode.contentStart)
  const exponentNode = readDerNode(der, modulusNode.contentEnd)
  return { modulus: readInteger(der, modulusNode), exponent: readInteger(der, exponentNode) }
}

function modPow(base: bigint, exponent: bigint, modulus: bigint): bigint {
  let result = 1n
  let factor = base % modulus
  let power = exponent
  while (power > 0n) {
    if (power & 1n) result = (result * factor) % modulus
    factor = (factor * factor) % modulus
    power >>= 1n
  }
  return result
}

function bigIntToBytes(value: bigint, length: number): Uint8Array {
  const bytes = new Uint8Array(length)
  let rest = value
  for (let index = length - 1; index >= 0; index -= 1) {
    bytes[index] = Number(rest & 0xffn)
    rest >>= 8n
  }
  if (rest > 0n) throw new Error('RSA 加密结果超出模长')
  return bytes
}

/** 公钥加密：返回 Base64 密文，等价于 Java 的 RSAUtils.publicEncrypt(text, publicKey) */
export function rsaPublicEncrypt(text: string, publicKey: string): string {
  const { modulus, exponent } = parsePublicKey(publicKey)
  const keyLength = (modulus.toString(16).length + 1) >> 1
  if (keyLength < 32) throw new Error('RSA 公钥长度异常，请检查公钥内容')

  const message = new TextEncoder().encode(text)
  if (message.length > keyLength - 11) {
    throw new Error(`明文过长：${message.length} 字节，当前公钥最多支持 ${keyLength - 11} 字节`)
  }

  // EM = 0x00 || 0x02 || 非零随机填充 || 0x00 || 明文
  const paddingLength = keyLength - message.length - 3
  const block = new Uint8Array(keyLength)
  block[1] = 0x02
  const random = new Uint8Array(paddingLength)
  for (let index = 0; index < paddingLength; index += 1) {
    let byte = 0
    while (byte === 0) byte = crypto.getRandomValues(new Uint8Array(1))[0]
    random[index] = byte
  }
  block.set(random, 2)
  block.set(message, 3 + paddingLength)

  const cipher = modPow(bytesToBigInt(block), exponent, modulus)
  return bytesToBase64(bigIntToBytes(cipher, keyLength))
}
