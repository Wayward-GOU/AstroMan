/* 通用文本、编码、加解密变换：全部在本地执行 */

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
}

export function base64ToBytes(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text.replace(/\s/g, ''))
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

export function base64Encode(text: string): string {
  return bytesToBase64(new TextEncoder().encode(text))
}

export function base64Decode(text: string): string {
  return new TextDecoder().decode(base64ToBytes(text))
}

export function base64UrlEncode(text: string): string {
  return base64Encode(text).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export function base64UrlDecode(text: string): string {
  const normalized = text.replace(/-/g, '+').replace(/_/g, '/')
  return base64Decode(normalized)
}

export function unicodeEscape(text: string): string {
  return text.replace(/[^\x00-\x7f]/g, (char) => {
    const code = char.charCodeAt(0)
    return `\\u${code.toString(16).padStart(4, '0')}`
  })
}

export function unicodeUnescape(text: string): string {
  return text
    .replace(/\\u\{([0-9a-fA-F]{1,6})\}/g, (_m, hex: string) =>
      String.fromCodePoint(parseInt(hex, 16)),
    )
    .replace(/\\u([0-9a-fA-F]{4})/g, (_m, hex: string) =>
      String.fromCharCode(parseInt(hex, 16)),
    )
    .replace(/\\U([0-9a-fA-F]{8})/g, (_m, hex: string) =>
      String.fromCodePoint(parseInt(hex, 16)),
    )
}

export function htmlEscape(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

const HTML_NAMED: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: '\u00a0',
  copy: '\u00a9',
  reg: '\u00ae',
  trade: '\u2122',
  hellip: '\u2026',
  mdash: '\u2014',
  ndash: '\u2013',
  middot: '\u00b7',
  times: '\u00d7',
  divide: '\u00f7',
  yen: '\u00a5',
  euro: '\u20ac',
  deg: '\u00b0',
  plusmn: '\u00b1',
}

export function htmlUnescape(text: string): string {
  return text
    .replace(/&#x([0-9a-fA-F]+);/g, (_m, hex: string) =>
      String.fromCodePoint(parseInt(hex, 16)),
    )
    .replace(/&#(\d+);/g, (_m, num: string) => String.fromCodePoint(Number(num)))
    .replace(/&([a-zA-Z]+);/g, (match, name: string) => HTML_NAMED[name] ?? match)
}

export function asciiEncode(text: string, radix: 'dec' | 'hex' = 'dec'): string {
  return Array.from(text)
    .map((char) =>
      radix === 'hex'
        ? char.codePointAt(0)!.toString(16).toUpperCase().padStart(2, '0')
        : String(char.codePointAt(0)),
    )
    .join(' ')
}

export function asciiDecode(text: string, radix: 'dec' | 'hex' = 'dec'): string {
  return text
    .split(/[\s,]+/)
    .filter(Boolean)
    .map((part) => {
      const code = parseInt(part, radix === 'hex' ? 16 : 10)
      if (Number.isNaN(code)) throw new Error(`无法识别的数值：${part}`)
      return String.fromCodePoint(code)
    })
    .join('')
}

export type CaseStyle =
  | 'upper'
  | 'lower'
  | 'title'
  | 'camel'
  | 'pascal'
  | 'snake'
  | 'kebab'
  | 'constant'
  | 'dot'
  | 'space'

function splitWords(text: string): string[] {
  return text
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
    .replace(/[^A-Za-z0-9]+/g, ' ')
    .trim()
    .split(/\s+/)
    .filter(Boolean)
}

export function convertCase(text: string, style: CaseStyle): string {
  if (style === 'upper') return text.toUpperCase()
  if (style === 'lower') return text.toLowerCase()
  if (style === 'title') {
    return text.replace(/\b([a-z])([a-z']*)/gi, (_m, first: string, rest: string) => {
      return first.toUpperCase() + rest.toLowerCase()
    })
  }
  const words = splitWords(text)
  if (!words.length) return ''
  const lower = words.map((word) => word.toLowerCase())
  const upperFirst = lower.map((word) => word[0].toUpperCase() + word.slice(1))
  switch (style) {
    case 'camel':
      return lower[0] + upperFirst.slice(1).join('')
    case 'pascal':
      return upperFirst.join('')
    case 'snake':
      return lower.join('_')
    case 'kebab':
      return lower.join('-')
    case 'constant':
      return lower.join('_').toUpperCase()
    case 'dot':
      return lower.join('.')
    default:
      return lower.join(' ')
  }
}

export function toFullWidth(text: string): string {
  return text.replace(/[\x21-\x7e]/g, (char) =>
    String.fromCharCode(char.charCodeAt(0) + 0xfee0),
  ).replace(/ /g, '\u3000')
}

export function toHalfWidth(text: string): string {
  return text
    .replace(/[\uff01-\uff5e]/g, (char) => String.fromCharCode(char.charCodeAt(0) - 0xfee0))
    .replace(/\u3000/g, ' ')
}

export function reverseText(text: string, mode: 'reverse' | 'mirror'): string {
  if (mode === 'mirror') {
    const map: Record<string, string> = {
      a: '\u0250', b: 'q', c: '\u0254', d: 'p', e: '\u01dd', f: '\u025f',
      g: '\u0183', h: '\u0265', i: '\u0131', j: '\u027e', k: '\u029e', l: 'l',
      m: '\u026f', n: 'u', o: 'o', p: 'd', r: '\u0279', s: 's', t: '\u0287',
      u: 'n', v: '\u028c', w: 'w', x: 'x', y: '\u028e', z: 'z',
    }
    return Array.from(text)
      .reverse()
      .map((char) => {
        const lower = char.toLowerCase()
        const mapped = map[lower]
        if (!mapped) return char
        return char === lower ? mapped : mapped.toUpperCase()
      })
      .join('')
  }
  return Array.from(text).reverse().join('')
}

export function dedupeLines(
  text: string,
  options: { sort?: 'none' | 'asc' | 'desc'; ignoreCase?: boolean; trim?: boolean },
): string {
  const { sort = 'none', ignoreCase = false, trim = true } = options
  const lines = text.split('\n').map((line) => (trim ? line.trim() : line))
  const seen = new Set<string>()
  const result: string[] = []
  for (const line of lines) {
    const key = ignoreCase ? line.toLowerCase() : line
    if (seen.has(key)) continue
    seen.add(key)
    result.push(line)
  }
  if (sort !== 'none') {
    result.sort((a, b) => {
      const left = ignoreCase ? a.toLowerCase() : a
      const right = ignoreCase ? b.toLowerCase() : b
      return sort === 'asc' ? left.localeCompare(right, 'zh-CN') : right.localeCompare(left, 'zh-CN')
    })
  }
  return result.join('\n')
}

export function textStats(text: string): string {
  const chars = Array.from(text)
  const lines = text ? text.split('\n') : []
  const words = text.trim() ? text.trim().split(/\s+/).filter(Boolean) : []
  const chinese = chars.filter((char) => /[\u4e00-\u9fa5]/.test(char)).length
  const letters = chars.filter((char) => /[A-Za-z]/.test(char)).length
  const digits = chars.filter((char) => /[0-9]/.test(char)).length
  const spaces = chars.filter((char) => /\s/.test(char)).length
  const others = chars.length - chinese - letters - digits - spaces
  return [
    `字符数：${chars.length}`,
    `字符数（不含空格）：${chars.length - spaces}`,
    `行数：${lines.length}`,
    `非空行：${lines.filter((line) => line.trim()).length}`,
    `单词数（空白分隔）：${words.length}`,
    `中文汉字：${chinese}`,
    `英文字母：${letters}`,
    `数字：${digits}`,
    `空白字符：${spaces}`,
    `其他符号：${others}`,
  ].join('\n')
}

export function replaceText(
  text: string,
  find: string,
  replacement: string,
  options: { regex?: boolean; ignoreCase?: boolean },
): string {
  if (!find) throw new Error('请填写要查找的内容')
  const flags = options.ignoreCase ? 'gi' : 'g'
  if (options.regex) {
    return text.replace(new RegExp(find, flags), replacement)
  }
  const escaped = find.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return text.replace(new RegExp(escaped, flags), replacement)
}

const CN_DIGITS = '零壹贰叁肆伍陆柒捌玖'
const CN_UNITS = ['', '拾', '佰', '仟']
const CN_BIG = ['', '万', '亿', '兆']

export function rmbUpper(input: string): string {
  const raw = input.trim().replace(/[,，\s￥¥]/g, '')
  if (!raw) throw new Error('请输入金额')
  if (!/^-?\d+(\.\d+)?$/.test(raw)) throw new Error('金额格式不正确，示例：1234.56')
  const negative = raw.startsWith('-')
  const [intRaw, decRaw = ''] = raw.replace('-', '').split('.')
  if (decRaw.length > 2) throw new Error('小数最多两位')
  const jiao = Number(decRaw[0] ?? 0)
  const fen = Number(decRaw[1] ?? 0)
  let intStr = intRaw.replace(/^0+/, '')
  if (!intStr && jiao === 0 && fen === 0) return '零元整'

  let out = ''
  if (intStr) {
    const groups: string[] = []
    while (intStr.length > 0) {
      groups.unshift(intStr.slice(-4))
      intStr = intStr.slice(0, -4)
    }
    let zeroPending = false
    groups.forEach((group, index) => {
      const value = Number(group)
      const bigIndex = groups.length - 1 - index
      if (value === 0) {
        if (out) zeroPending = true
        return
      }
      if (zeroPending && !out.endsWith('零')) out += '零'
      zeroPending = false
      let groupOut = ''
      for (let i = 0; i < group.length; i += 1) {
        const digit = Number(group[i])
        const unit = CN_UNITS[group.length - i - 1]
        if (digit === 0) {
          if (groupOut && !groupOut.endsWith('零')) groupOut += '零'
        } else {
          groupOut += CN_DIGITS[digit] + unit
        }
      }
      groupOut = groupOut.replace(/零+$/, '')
      out += groupOut + CN_BIG[bigIndex]
      if (group.length < 4) zeroPending = true
    })
  }

  if (jiao === 0 && fen === 0) {
    out += '元整'
  } else {
    out += '元'
    if (jiao > 0) out += `${CN_DIGITS[jiao]}角`
    else if (fen > 0) out += '零'
    if (fen > 0) out += `${CN_DIGITS[fen]}分`
    else out += '整'
  }
  return (negative ? '负' : '') + out
}

const MORSE_MAP: Record<string, string> = {
  a: '.-', b: '-...', c: '-.-.', d: '-..', e: '.', f: '..-.', g: '--.', h: '....',
  i: '..', j: '.---', k: '-.-', l: '.-..', m: '--', n: '-.', o: '---', p: '.--.',
  q: '--.-', r: '.-.', s: '...', t: '-', u: '..-', v: '...-', w: '.--', x: '-..-',
  y: '-.--', z: '--..',
  '0': '-----', '1': '.----', '2': '..---', '3': '...--', '4': '....-',
  '5': '.....', '6': '-....', '7': '--...', '8': '---..', '9': '----.',
  '.': '.-.-.-', ',': '--..--', '?': '..--..', "'": '.----.', '!': '-.-.--',
  '/': '-..-.', '(': '-.--.', ')': '-.--.-', '&': '.-...', ':': '---...',
  ';': '-.-.-.', '=': '-...-', '+': '.-.-.', '-': '-....-', _: '..--.-',
  '"': '.-..-.', $: '...-..-', '@': '.--.-.',
}

export function morseEncode(text: string): string {
  return text
    .toLowerCase()
    .split(/\s+/)
    .map((word) =>
      Array.from(word)
        .map((char) => MORSE_MAP[char] ?? '')
        .filter(Boolean)
        .join(' '),
    )
    .filter(Boolean)
    .join(' / ')
}

export function morseDecode(text: string): string {
  const reverse: Record<string, string> = {}
  Object.entries(MORSE_MAP).forEach(([char, code]) => {
    reverse[code] = char
  })
  return text
    .trim()
    .split(/\s*\/\s*|\s{3,}/)
    .map((word) =>
      word
        .split(/\s+/)
        .map((code) => reverse[code] ?? '')
        .join(''),
    )
    .join(' ')
}

export async function gzipToBase64(text: string): Promise<string> {
  const Compressor = (window as unknown as { CompressionStream?: typeof CompressionStream })
    .CompressionStream
  if (!Compressor) throw new Error('当前环境不支持 gzip 压缩')
  const input = new TextEncoder().encode(text)
  const stream = new Blob([input]).stream().pipeThrough(new Compressor('gzip'))
  const buffer = await new Response(stream).arrayBuffer()
  return bytesToBase64(new Uint8Array(buffer))
}

export async function gunzipFromBase64(text: string): Promise<string> {
  const Decompressor = (window as unknown as { DecompressionStream?: typeof DecompressionStream })
    .DecompressionStream
  if (!Decompressor) throw new Error('当前环境不支持 gzip 解压')
  const bytes = base64ToBytes(text)
  const stream = new Blob([bytes]).stream().pipeThrough(new Decompressor('gzip'))
  const buffer = await new Response(stream).arrayBuffer()
  return new TextDecoder().decode(buffer)
}

export function ipToLong(ip: string): number {
  const parts = ip.trim().split('.')
  if (parts.length !== 4) throw new Error('IPv4 格式不正确')
  let result = 0
  for (const part of parts) {
    const value = Number(part)
    if (!/^\d+$/.test(part) || value > 255) throw new Error(`无效的网段：${part}`)
    result = result * 256 + value
  }
  return result
}

export function longToIp(long: number): string {
  if (!Number.isInteger(long) || long < 0 || long > 0xffffffff) {
    throw new Error('数字超出 IPv4 范围（0 - 4294967295）')
  }
  return [24, 16, 8, 0].map((shift) => Math.floor(long / 2 ** shift) % 256).join('.')
}

export function rc4(text: string, key: string): string {
  if (!key) throw new Error('请填写密钥')
  const input = Array.from(new TextEncoder().encode(text))
  const keyBytes = Array.from(new TextEncoder().encode(key))
  const s = Array.from({ length: 256 }, (_, i) => i)
  let j = 0
  for (let i = 0; i < 256; i += 1) {
    j = (j + s[i] + keyBytes[i % keyBytes.length]) % 256
    ;[s[i], s[j]] = [s[j], s[i]]
  }
  let i = 0
  j = 0
  const output = input.map((byte) => {
    i = (i + 1) % 256
    j = (j + s[i]) % 256
    ;[s[i], s[j]] = [s[j], s[i]]
    return byte ^ s[(s[i] + s[j]) % 256]
  })
  return bytesToBase64(new Uint8Array(output))
}

export function bytesText(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes)
}

export function formatBytes(value: number): string {
  if (!value || value <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let size = value
  let index = 0
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024
    index += 1
  }
  return `${size >= 100 || index === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[index]}`
}
