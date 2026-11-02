/* Base58 / Ascii85 / Quoted-Printable 编解码 */

const BASE58_ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'

export function base58Encode(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let zeros = 0
  while (zeros < bytes.length && bytes[zeros] === 0) zeros += 1
  let value = 0n
  for (const byte of bytes) value = (value << 8n) | BigInt(byte)
  let encoded = ''
  while (value > 0n) {
    encoded = BASE58_ALPHABET[Number(value % 58n)] + encoded
    value /= 58n
  }
  return '1'.repeat(zeros) + encoded
}

export function base58Decode(text: string): string {
  const raw = text.trim().replace(/\s/g, '')
  if (!raw) throw new Error('请输入 Base58 内容')
  let zeros = 0
  while (zeros < raw.length && raw[zeros] === '1') zeros += 1
  let value = 0n
  for (const char of raw) {
    const index = BASE58_ALPHABET.indexOf(char)
    if (index < 0) throw new Error(`包含非法 Base58 字符：${char}（字母表不含 0 O I l）`)
    value = value * 58n + BigInt(index)
  }
  const bytes: number[] = []
  while (value > 0n) {
    bytes.unshift(Number(value & 0xffn))
    value >>= 8n
  }
  const result = new Uint8Array(zeros + bytes.length)
  bytes.forEach((byte, index) => {
    result[zeros + index] = byte
  })
  return new TextDecoder().decode(result)
}

export function ascii85Encode(text: string, delimiters = true): string {
  const bytes = new TextEncoder().encode(text)
  let out = ''
  for (let index = 0; index < bytes.length; index += 4) {
    const chunk = bytes.subarray(index, index + 4)
    const padded = new Uint8Array(4)
    padded.set(chunk)
    let value = 0
    for (const byte of padded) value = value * 256 + byte
    if (chunk.length === 4 && value === 0) {
      out += 'z'
      continue
    }
    let group = ''
    let remaining = value
    for (let step = 0; step < 5; step += 1) {
      group = String.fromCharCode(33 + (remaining % 85)) + group
      remaining = Math.floor(remaining / 85)
    }
    out += chunk.length === 4 ? group : group.slice(0, chunk.length + 1)
  }
  return delimiters ? `<~${out}~>` : out
}

export function ascii85Decode(input: string): string {
  const text = input
    .trim()
    .replace(/^<~/, '')
    .replace(/~>$/, '')
    .replace(/\s+/g, '')
  if (!text) throw new Error('请输入 Ascii85 内容')
  const bytes: number[] = []
  let tuple = 0
  let count = 0
  for (const char of text) {
    if (char === 'z') {
      if (count !== 0) throw new Error('z 只能出现在 5 字符分组之间')
      bytes.push(0, 0, 0, 0)
      continue
    }
    const code = char.charCodeAt(0) - 33
    if (code < 0 || code > 84) throw new Error(`包含非法 Ascii85 字符：${char}`)
    tuple = tuple * 85 + code
    count += 1
    if (count === 5) {
      bytes.push((tuple >>> 24) & 0xff, (tuple >>> 16) & 0xff, (tuple >>> 8) & 0xff, tuple & 0xff)
      tuple = 0
      count = 0
    }
  }
  if (count > 0) {
    const pad = 5 - count
    for (let step = 0; step < pad; step += 1) tuple = tuple * 85 + 84
    const group = [(tuple >>> 24) & 0xff, (tuple >>> 16) & 0xff, (tuple >>> 8) & 0xff, tuple & 0xff]
    bytes.push(...group.slice(0, count - 1))
  }
  return new TextDecoder().decode(new Uint8Array(bytes))
}

export function quotedPrintableEncode(text: string): string {
  const bytes = new TextEncoder().encode(text)
  const lines: string[] = []
  let line = ''
  const flush = () => {
    lines.push(line)
    line = ''
  }
  for (const byte of bytes) {
    if (byte === 0x0a) {
      flush()
      continue
    }
    if (byte === 0x0d) continue
    const token =
      byte >= 33 && byte <= 126 && byte !== 0x3d
        ? String.fromCharCode(byte)
        : `=${byte.toString(16).toUpperCase().padStart(2, '0')}`
    if (line.length + token.length > 75) {
      lines.push(`${line}=`)
      line = ''
    }
    line += token
  }
  flush()
  return lines.join('\r\n')
}

export function quotedPrintableDecode(text: string): string {
  if (!text.trim()) throw new Error('请输入 Quoted-Printable 内容')
  const cleaned = text.replace(/=\r?\n/g, '').replace(/[ \t]+(?=\r?\n)/g, '')
  const bytes: number[] = []
  for (let index = 0; index < cleaned.length; index += 1) {
    const char = cleaned[index]
    if (char === '=' && /^[0-9A-Fa-f]{2}$/.test(cleaned.slice(index + 1, index + 3))) {
      bytes.push(parseInt(cleaned.slice(index + 1, index + 3), 16))
      index += 2
      continue
    }
    new TextEncoder().encode(char).forEach((byte) => bytes.push(byte))
  }
  return new TextDecoder().decode(new Uint8Array(bytes))
}
