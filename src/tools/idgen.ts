/* ULID / NanoID 生成 */

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
const NANOID_ALPHABET = 'useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict'

function randomChars(count: number, alphabet: string): string {
  const bytes = new Uint8Array(count)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (byte) => alphabet[byte % alphabet.length]).join('')
}

function encodeTime(timestamp: number): string {
  let value = Math.max(0, Math.floor(timestamp))
  let out = ''
  for (let index = 0; index < 10; index += 1) {
    out = CROCKFORD[value % 32] + out
    value = Math.floor(value / 32)
  }
  return out
}

export function generateUlid(timestamp = Date.now()): string {
  return encodeTime(timestamp) + randomChars(16, CROCKFORD)
}

export function decodeUlid(ulid: string): number | null {
  const text = ulid.trim().toUpperCase()
  if (text.length !== 26) return null
  let value = 0
  for (const char of text.slice(0, 10)) {
    const index = CROCKFORD.indexOf(char)
    if (index < 0) return null
    value = value * 32 + index
  }
  return value
}

export function generateNanoId(length = 21, alphabet = NANOID_ALPHABET): string {
  const pool = alphabet.replace(/[^A-Za-z0-9_-]/g, '') || NANOID_ALPHABET
  const size = Math.min(64, Math.max(4, Math.floor(length)))
  return randomChars(size, pool)
}

export function generateIds(
  kind: 'ulid' | 'nanoid' | 'both' = 'ulid',
  count = 5,
  nanoidLength = 21,
  timestamp?: number,
): string {
  const size = Math.min(100, Math.max(1, Math.floor(count)))
  const base = timestamp ?? Date.now()
  if (kind === 'nanoid') {
    return Array.from({ length: size }, () => generateNanoId(nanoidLength)).join('\n')
  }
  const ulids: string[] = []
  for (let index = 0; index < size; index += 1) {
    // 同毫秒内递增 1ms，保证字典序与生成顺序一致
    ulids.push(generateUlid(base + index))
  }
  if (kind === 'ulid') {
    return [
      ulids.join('\n'),
      '',
      `共 ${size} 个，时间前缀 ${new Date(base).toISOString()}`,
      'ULID = 48 位毫秒时间（10 字符）+ 80 位随机数（16 字符），Crockford Base32，字典序即时间序。',
    ].join('\n')
  }
  return [
    'ULID：',
    ...ulids,
    '',
    `NanoID（长度 ${nanoidLength}）：`,
    ...Array.from({ length: size }, () => generateNanoId(nanoidLength)),
  ].join('\n')
}
