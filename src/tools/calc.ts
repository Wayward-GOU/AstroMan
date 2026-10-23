/* 进制、单位、颜色、校验与随机生成 */

/* ---------------- 进制转换 ---------------- */

export function baseConvert(value: string, from: number): Record<number, string> {
  const text = value.trim().replace(/^0[xXbBoO]/, '')
  if (!text) throw new Error('请输入数值')
  if (from === 10 && !/^-?\d+$/.test(text)) throw new Error('十进制只允许数字')
  if (from === 16 && !/^-?[0-9a-fA-F]+$/.test(text)) throw new Error('十六进制只允许 0-9 / a-f')
  if (from === 8 && !/^-?[0-7]+$/.test(text)) throw new Error('八进制只允许 0-7')
  if (from === 2 && !/^-?[01]+$/.test(text)) throw new Error('二进制只允许 0、1')
  const negative = text.startsWith('-')
  const digits = negative ? text.slice(1) : text
  if (!/^[0-9a-zA-Z]+$/.test(digits)) throw new Error('包含非法字符')
  let result = 0n
  const base = BigInt(from)
  for (const char of digits.toLowerCase()) {
    const digit = parseInt(char, 36)
    if (digit >= from) throw new Error(`字符 ${char} 超出 ${from} 进制的范围`)
    result = result * base + BigInt(digit)
  }
  const signed = negative ? -result : result
  const output: Record<number, string> = {}
  ;[2, 8, 10, 16, 32, 36].forEach((target) => {
    output[target] = (signed < 0n ? '-' : '') + (-signed < 0n ? signed : signed).toString(target).toUpperCase()
  })
  return output
}

/* ---------------- 单位换算 ---------------- */

export type UnitGroup = 'length' | 'weight' | 'area' | 'volume' | 'speed' | 'data' | 'time' | 'temperature'

interface UnitDef {
  key: string
  label: string
  factor: number
}

export const UNIT_GROUPS: Record<UnitGroup, { label: string; units: UnitDef[] }> = {
  length: {
    label: '长度',
    units: [
      { key: 'mm', label: '毫米 mm', factor: 0.001 },
      { key: 'cm', label: '厘米 cm', factor: 0.01 },
      { key: 'm', label: '米 m', factor: 1 },
      { key: 'km', label: '千米 km', factor: 1000 },
      { key: 'in', label: '英寸 in', factor: 0.0254 },
      { key: 'ft', label: '英尺 ft', factor: 0.3048 },
      { key: 'yd', label: '码 yd', factor: 0.9144 },
      { key: 'mi', label: '英里 mi', factor: 1609.344 },
      { key: 'nmi', label: '海里 nmi', factor: 1852 },
    ],
  },
  weight: {
    label: '重量',
    units: [
      { key: 'mg', label: '毫克 mg', factor: 0.000001 },
      { key: 'g', label: '克 g', factor: 0.001 },
      { key: 'kg', label: '千克 kg', factor: 1 },
      { key: 't', label: '吨 t', factor: 1000 },
      { key: 'lb', label: '磅 lb', factor: 0.45359237 },
      { key: 'oz', label: '盎司 oz', factor: 0.028349523125 },
    ],
  },
  area: {
    label: '面积',
    units: [
      { key: 'mm2', label: '平方毫米 mm²', factor: 0.000001 },
      { key: 'cm2', label: '平方厘米 cm²', factor: 0.0001 },
      { key: 'm2', label: '平方米 m²', factor: 1 },
      { key: 'km2', label: '平方千米 km²', factor: 1000000 },
      { key: 'ha', label: '公顷 ha', factor: 10000 },
      { key: 'mu', label: '亩', factor: 666.6666667 },
      { key: 'ft2', label: '平方英尺 ft²', factor: 0.09290304 },
    ],
  },
  volume: {
    label: '体积',
    units: [
      { key: 'ml', label: '毫升 ml', factor: 0.001 },
      { key: 'l', label: '升 L', factor: 1 },
      { key: 'm3', label: '立方米 m³', factor: 1000 },
      { key: 'gal', label: '美制加仑 gal', factor: 3.785411784 },
      { key: 'qt', label: '美制夸脱 qt', factor: 0.946352946 },
    ],
  },
  speed: {
    label: '速度',
    units: [
      { key: 'mps', label: '米/秒 m/s', factor: 1 },
      { key: 'kmh', label: '千米/时 km/h', factor: 0.277777778 },
      { key: 'mph', label: '英里/时 mph', factor: 0.44704 },
      { key: 'knot', label: '节 kn', factor: 0.514444444 },
    ],
  },
  data: {
    label: '数据',
    units: [
      { key: 'b', label: '比特 bit', factor: 0.125 },
      { key: 'B', label: '字节 B', factor: 1 },
      { key: 'KB', label: '千字节 KB', factor: 1024 },
      { key: 'MB', label: '兆字节 MB', factor: 1024 ** 2 },
      { key: 'GB', label: '吉字节 GB', factor: 1024 ** 3 },
      { key: 'TB', label: '太字节 TB', factor: 1024 ** 4 },
    ],
  },
  time: {
    label: '时间',
    units: [
      { key: 'ms', label: '毫秒 ms', factor: 0.001 },
      { key: 's', label: '秒 s', factor: 1 },
      { key: 'min', label: '分钟 min', factor: 60 },
      { key: 'h', label: '小时 h', factor: 3600 },
      { key: 'd', label: '天 d', factor: 86400 },
      { key: 'w', label: '周 w', factor: 604800 },
    ],
  },
  temperature: {
    label: '温度',
    units: [
      { key: 'c', label: '摄氏度 °C', factor: 1 },
      { key: 'f', label: '华氏度 °F', factor: 1 },
      { key: 'k', label: '开尔文 K', factor: 1 },
    ],
  },
}

export function convertUnit(
  group: UnitGroup,
  value: number,
  from: string,
  to: string,
): number {
  if (Number.isNaN(value)) throw new Error('请输入有效数字')
  if (group === 'temperature') {
    const celsius =
      from === 'c' ? value : from === 'f' ? (value - 32) / 1.8 : value - 273.15
    if (to === 'c') return celsius
    if (to === 'f') return celsius * 1.8 + 32
    return celsius + 273.15
  }
  const units = UNIT_GROUPS[group].units
  const fromUnit = units.find((unit) => unit.key === from)
  const toUnit = units.find((unit) => unit.key === to)
  if (!fromUnit || !toUnit) throw new Error('未知的单位')
  return (value * fromUnit.factor) / toUnit.factor
}

export function convertAllUnits(group: UnitGroup, value: number, from: string): string {
  const units = UNIT_GROUPS[group].units
  return units
    .map((unit) => {
      const converted = convertUnit(group, value, from, unit.key)
      const display = Math.abs(converted) >= 1e-6 || converted === 0
        ? Number(converted.toPrecision(10)).toString()
        : converted.toExponential(4)
      return `${unit.label} = ${display}`
    })
    .join('\n')
}

/* ---------------- 颜色转换 ---------------- */

export interface ColorInfo {
  hex: string
  rgb: string
  rgba: string
  hsl: string
  hsv: string
  cmyk: string
  luminance: number
  contrastWithWhite: number
  contrastWithBlack: number
}

function parseColorInput(text: string): [number, number, number, number] {
  const raw = text.trim().toLowerCase()
  if (raw.startsWith('#')) {
    let hex = raw.slice(1)
    if (hex.length === 3 || hex.length === 4) {
      hex = hex.split('').map((char) => char + char).join('')
    }
    if (hex.length !== 6 && hex.length !== 8) throw new Error('HEX 颜色需要 3/4/6/8 位')
    const value = parseInt(hex.slice(0, 6), 16)
    if (Number.isNaN(value)) throw new Error('HEX 颜色格式错误')
    const alpha = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1
    return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff, alpha]
  }
  const match = /^(rgb|rgba|hsl|hsla)\s*\(([^)]+)\)$/.exec(raw)
  if (!match) {
    const nums = raw.split(/[\s,]+/).filter(Boolean).map(Number)
    if (nums.length >= 3 && nums.every((num) => !Number.isNaN(num))) {
      return [nums[0], nums[1], nums[2], nums[3] ?? 1]
    }
    throw new Error('请使用 #RRGGBB、rgb()、rgba()、hsl() 或空格分隔的三个数字')
  }
  const type = match[1].startsWith('hsl') ? 'hsl' : 'rgb'
  const parts = match[2].split(/[\s,/]+/).filter(Boolean)
  const numbers = parts.map((part) => (part.endsWith('%') ? Number(part.slice(0, -1)) : Number(part)))
  if (type === 'rgb') {
    return [numbers[0], numbers[1], numbers[2], numbers[3] ?? 1]
  }
  const [r, g, b] = hslToRgb(numbers[0], numbers[1], numbers[2])
  return [r, g, b, numbers[3] ?? 1]
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const hue = ((h % 360) + 360) % 360
  const sat = Math.min(1, Math.max(0, s / 100))
  const light = Math.min(1, Math.max(0, l / 100))
  const c = (1 - Math.abs(2 * light - 1)) * sat
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1))
  const m = light - c / 2
  let rgb: [number, number, number]
  if (hue < 60) rgb = [c, x, 0]
  else if (hue < 120) rgb = [x, c, 0]
  else if (hue < 180) rgb = [0, c, x]
  else if (hue < 240) rgb = [0, x, c]
  else if (hue < 300) rgb = [x, 0, c]
  else rgb = [c, 0, x]
  return rgb.map((value) => Math.round((value + m) * 255)) as [number, number, number]
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const light = (max + min) / 2
  if (max === min) return [0, 0, Math.round(light * 100)]
  const delta = max - min
  const sat = light > 0.5 ? delta / (2 - max - min) : delta / (max + min)
  let hue = 0
  if (max === rn) hue = ((gn - bn) / delta + (gn < bn ? 6 : 0)) * 60
  else if (max === gn) hue = ((bn - rn) / delta + 2) * 60
  else hue = ((rn - gn) / delta + 4) * 60
  return [Math.round(hue), Math.round(sat * 100), Math.round(light * 100)]
}

function relativeLuminance(r: number, g: number, b: number): number {
  const channel = (value: number) => {
    const normalized = value / 255
    return normalized <= 0.03928 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

export function colorInfo(text: string): ColorInfo {
  const [r, g, b, a] = parseColorInput(text).map((value, index) =>
    index < 3 ? Math.round(Math.min(255, Math.max(0, value))) : Math.min(1, Math.max(0, value)),
  )
  const [h, s, l] = rgbToHsl(r, g, b)
  const max = Math.max(r, g, b) / 255
  const min = Math.min(r, g, b) / 255
  const delta = max - min
  const v = max
  const sv = max === 0 ? 0 : delta / max
  const k = 1 - max
  const c = (1 - Math.abs(2 * (1 - k) - 1)) * (max === 0 ? 0 : delta / max)
  const cmykK = v === 0 ? 0 : 1 - v
  const cmykC = v === 0 ? 0 : (1 - r / 255 - cmykK) / (1 - cmykK)
  const cmykM = v === 0 ? 0 : (1 - g / 255 - cmykK) / (1 - cmykK)
  const cmykY = v === 0 ? 0 : (1 - b / 255 - cmykK) / (1 - cmykK)
  const luminance = relativeLuminance(r, g, b)
  const hex = `#${[r, g, b].map((value) => value.toString(16).padStart(2, '0')).join('')}`.toUpperCase()
  return {
    hex,
    rgb: `rgb(${r}, ${g}, ${b})`,
    rgba: `rgba(${r}, ${g}, ${b}, ${Number(a.toFixed(3))})`,
    hsl: `hsl(${h}, ${s}%, ${l}%)`,
    hsv: `hsv(${Math.round(((delta / (max || 1)) * 60 + (max === min ? 0 : 0)) || 0)}, ${Math.round(sv * 100)}%, ${Math.round(v * 100)}%)`,
    cmyk: `cmyk(${Math.round(cmykC * 100)}%, ${Math.round(cmykM * 100)}%, ${Math.round(cmykY * 100)}%, ${Math.round(cmykK * 100)}%)`,
    luminance: Number(luminance.toFixed(4)),
    contrastWithWhite: Number(((1.05) / (luminance + 0.05)).toFixed(2)),
    contrastWithBlack: Number(((luminance + 0.05) / 0.05).toFixed(2)),
  }
}

/* ---------------- CRC 校验 ---------------- */

const CRC32_TABLE: number[] = (() => {
  const table: number[] = []
  for (let i = 0; i < 256; i += 1) {
    let value = i
    for (let j = 0; j < 8; j += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
    }
    table.push(value >>> 0)
  }
  return table
})()

export function crc32(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let crc = 0xffffffff
  bytes.forEach((byte) => {
    crc = CRC32_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8)
  })
  return ((crc ^ 0xffffffff) >>> 0).toString(16).padStart(8, '0').toUpperCase()
}

export function crc16(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let crc = 0xffff
  bytes.forEach((byte) => {
    crc ^= byte
    for (let i = 0; i < 8; i += 1) {
      crc = crc & 1 ? (crc >>> 1) ^ 0xa001 : crc >>> 1
    }
  })
  return crc.toString(16).padStart(4, '0').toUpperCase()
}

export function luhnCheck(text: string): string {
  const digits = text.replace(/\s/g, '')
  if (!/^\d+$/.test(digits)) throw new Error('仅支持数字（可含空格）')
  let sum = 0
  let double = false
  for (let i = digits.length - 1; i >= 0; i -= 1) {
    let value = Number(digits[i])
    if (double) {
      value *= 2
      if (value > 9) value -= 9
    }
    sum += value
    double = !double
  }
  return sum % 10 === 0 ? `校验通过（校验值 ${sum}）` : `校验失败（校验值 ${sum}，需要为 10 的倍数）`
}

/* ---------------- 随机生成 ---------------- */

export function randomPassword(
  length: number,
  options: { upper?: boolean; lower?: boolean; digit?: boolean; symbol?: boolean; excludeAmbiguous?: boolean },
): string {
  let pool = ''
  if (options.upper !== false) pool += 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
  if (options.lower !== false) pool += 'abcdefghijklmnopqrstuvwxyz'
  if (options.digit !== false) pool += '0123456789'
  if (options.symbol) pool += '!@#$%^&*()-_=+[]{};:,.<>?/~'
  if (options.excludeAmbiguous) pool = pool.replace(/[IlO0o1|`'"]/g, '')
  if (!pool) throw new Error('请至少选择一种字符类型')
  const size = Math.min(256, Math.max(1, length))
  const bytes = new Uint32Array(size)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (value) => pool[value % pool.length]).join('')
}

export function randomString(length: number, pool: string): string {
  if (!pool) throw new Error('字符集不能为空')
  const bytes = new Uint32Array(Math.min(4096, Math.max(0, length)))
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (value) => pool[value % pool.length]).join('')
}

export function uuidV4(): string {
  return crypto.randomUUID()
}

/* ---------------- 时间戳 ---------------- */

export function timestampInfo(input: string): string {
  const raw = input.trim()
  let date: Date
  if (!raw) {
    date = new Date()
  } else if (/^\d{10}$/.test(raw)) {
    date = new Date(Number(raw) * 1000)
  } else if (/^\d{13}$/.test(raw)) {
    date = new Date(Number(raw))
  } else if (/^\d{16,19}$/.test(raw)) {
    date = new Date(Number(BigInt(raw) / 1000000n))
  } else {
    const parsed = new Date(raw.replace(/-/g, '/'))
    if (Number.isNaN(parsed.getTime())) throw new Error('无法识别的时间格式')
    date = parsed
  }
  if (Number.isNaN(date.getTime())) throw new Error('时间超出有效范围')
  const pad = (value: number, size = 2) => String(value).padStart(size, '0')
  const local =
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  const offsetMinutes = -date.getTimezoneOffset()
  const sign = offsetMinutes >= 0 ? '+' : '-'
  const iso =
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}` +
    `.${pad(date.getMilliseconds(), 3)}${sign}${pad(Math.floor(Math.abs(offsetMinutes) / 60))}:${pad(Math.abs(offsetMinutes) % 60)}`
  const week = ['日', '一', '二', '三', '四', '五', '六'][date.getDay()]
  return [
    `秒级时间戳：${Math.floor(date.getTime() / 1000)}`,
    `毫秒时间戳：${date.getTime()}`,
    `本地时间：${local}`,
    `ISO 8601：${iso}`,
    `UTC 时间：${date.toISOString()}`,
    `星期：星期${week}`,
    `UTC 时区：UTC${sign}${pad(Math.floor(Math.abs(offsetMinutes) / 60))}:${pad(Math.abs(offsetMinutes) % 60)}`,
  ].join('\n')
}

/* ---------------- 键盘按键码 ---------------- */

export interface KeyCodeItem {
  key: string
  code: string
  keyCode: number
}

export const KEY_CODES: KeyCodeItem[] = [
  ...Array.from({ length: 26 }, (_, index) => {
    const letter = String.fromCharCode(65 + index)
    return { key: letter, code: `Key${letter}`, keyCode: 65 + index }
  }),
  ...Array.from({ length: 10 }, (_, index) => ({
    key: String(index),
    code: `Digit${index}`,
    keyCode: 48 + index,
  })),
  { key: 'Enter', code: 'Enter', keyCode: 13 },
  { key: 'Esc', code: 'Escape', keyCode: 27 },
  { key: 'Space', code: 'Space', keyCode: 32 },
  { key: 'Tab', code: 'Tab', keyCode: 9 },
  { key: 'Backspace', code: 'Backspace', keyCode: 8 },
  { key: 'Delete', code: 'Delete', keyCode: 46 },
  { key: 'Insert', code: 'Insert', keyCode: 45 },
  { key: 'Home', code: 'Home', keyCode: 36 },
  { key: 'End', code: 'End', keyCode: 35 },
  { key: 'PageUp', code: 'PageUp', keyCode: 33 },
  { key: 'PageDown', code: 'PageDown', keyCode: 34 },
  { key: 'ArrowUp', code: 'ArrowUp', keyCode: 38 },
  { key: 'ArrowDown', code: 'ArrowDown', keyCode: 40 },
  { key: 'ArrowLeft', code: 'ArrowLeft', keyCode: 37 },
  { key: 'ArrowRight', code: 'ArrowRight', keyCode: 39 },
  ...Array.from({ length: 12 }, (_, index) => ({
    key: `F${index + 1}`,
    code: `F${index + 1}`,
    keyCode: 112 + index,
  })),
  { key: 'Shift', code: 'ShiftLeft', keyCode: 16 },
  { key: 'Control', code: 'ControlLeft', keyCode: 17 },
  { key: 'Alt', code: 'AltLeft', keyCode: 18 },
  { key: 'Meta', code: 'MetaLeft', keyCode: 91 },
  { key: '-', code: 'Minus', keyCode: 189 },
  { key: '=', code: 'Equal', keyCode: 187 },
  { key: '[', code: 'BracketLeft', keyCode: 219 },
  { key: ']', code: 'BracketRight', keyCode: 221 },
  { key: '\\', code: 'Backslash', keyCode: 220 },
  { key: ';', code: 'Semicolon', keyCode: 186 },
  { key: "'", code: 'Quote', keyCode: 222 },
  { key: ',', code: 'Comma', keyCode: 188 },
  { key: '.', code: 'Period', keyCode: 190 },
  { key: '/', code: 'Slash', keyCode: 191 },
  { key: '`', code: 'Backquote', keyCode: 192 },
]
