/* 本地二维码：内容编码、自定义样式绘制、SVG 导出与图片识别 */

import QrCode from 'qrcode'
import jsQR from 'jsqr'

export type QrContentType = 'text' | 'wifi' | 'vcard' | 'tel' | 'sms' | 'email' | 'geo'
export type QrDotStyle = 'square' | 'rounded' | 'dot'
export type QrEyeStyle = 'square' | 'rounded'
export type QrLevel = 'L' | 'M' | 'Q' | 'H'

export interface QrContent {
  text: string
  wifiSsid: string
  wifiPassword: string
  wifiSecurity: 'WPA' | 'WEP' | 'nopass'
  wifiHidden: boolean
  vName: string
  vPhone: string
  vEmail: string
  vOrg: string
  vTitle: string
  vUrl: string
  vAddress: string
  tel: string
  smsPhone: string
  smsMessage: string
  emailTo: string
  emailSubject: string
  emailBody: string
  geoLat: string
  geoLng: string
}

export interface QrStyle {
  size: number
  margin: number
  level: QrLevel
  fg: string
  bg: string
  dot: QrDotStyle
  eye: QrEyeStyle
  logoRatio: number
}

export const QR_TYPES: { value: QrContentType; label: string; hint: string }[] = [
  { value: 'text', label: '文本 / 网址', hint: '扫码后可复制文本，网址可直接打开' },
  { value: 'wifi', label: 'WiFi', hint: '扫码后手机可直接连接该网络' },
  { value: 'vcard', label: '名片', hint: '扫码后可保存联系人信息' },
  { value: 'tel', label: '电话', hint: '扫码后唤起拨号' },
  { value: 'sms', label: '短信', hint: '扫码后打开短信并填入内容' },
  { value: 'email', label: '邮件', hint: '扫码后打开邮件应用并填入主题' },
  { value: 'geo', label: '定位', hint: '扫码后在地图中打开该坐标' },
]

function escapeWifi(value: string): string {
  return value.replace(/([\\;,":])/g, '\\$1')
}

function escapeVcard(value: string): string {
  return value
    .replace(/\\/g, '\\\\')
    .replace(/;/g, '\\;')
    .replace(/,/g, '\\,')
    .replace(/\r?\n/g, '\\n')
}

/** 根据内容类型生成二维码实际编码的字符串；字段不完整时返回空串 */
export function buildPayload(type: QrContentType, content: QrContent): string {
  switch (type) {
    case 'text':
      return content.text.trim()
    case 'wifi': {
      const ssid = content.wifiSsid.trim()
      if (!ssid) return ''
      const hidden = content.wifiHidden ? 'H:true;' : ''
      const password = content.wifiSecurity === 'nopass' ? '' : content.wifiPassword
      return `WIFI:T:${content.wifiSecurity};S:${escapeWifi(ssid)};P:${escapeWifi(password)};${hidden};`
    }
    case 'vcard': {
      const name = content.vName.trim()
      const phone = content.vPhone.trim()
      const email = content.vEmail.trim()
      if (!name && !phone && !email) return ''
      const lines = ['BEGIN:VCARD', 'VERSION:3.0']
      if (name) {
        lines.push(`FN:${escapeVcard(name)}`, `N:${escapeVcard(name)};;;;`)
      }
      if (content.vOrg.trim()) lines.push(`ORG:${escapeVcard(content.vOrg.trim())}`)
      if (content.vTitle.trim()) lines.push(`TITLE:${escapeVcard(content.vTitle.trim())}`)
      if (phone) lines.push(`TEL;TYPE=CELL:${escapeVcard(phone)}`)
      if (email) lines.push(`EMAIL:${escapeVcard(email)}`)
      if (content.vUrl.trim()) lines.push(`URL:${escapeVcard(content.vUrl.trim())}`)
      if (content.vAddress.trim()) lines.push(`ADR;TYPE=WORK:;;${escapeVcard(content.vAddress.trim())}`)
      lines.push('END:VCARD')
      return lines.join('\n')
    }
    case 'tel': {
      const phone = content.tel.trim()
      return phone ? `tel:${phone}` : ''
    }
    case 'sms': {
      const phone = content.smsPhone.trim()
      if (!phone) return ''
      return `SMSTO:${phone}:${content.smsMessage}`
    }
    case 'email': {
      const to = content.emailTo.trim()
      if (!to) return ''
      const query: string[] = []
      if (content.emailSubject.trim()) query.push(`subject=${encodeURIComponent(content.emailSubject.trim())}`)
      if (content.emailBody.trim()) query.push(`body=${encodeURIComponent(content.emailBody)}`)
      return `mailto:${to}${query.length ? `?${query.join('&')}` : ''}`
    }
    case 'geo': {
      const lat = content.geoLat.trim()
      const lng = content.geoLng.trim()
      if (!lat || !lng) return ''
      return `geo:${lat},${lng}`
    }
    default:
      return ''
  }
}

/* ---------------- 绘制 ---------------- */

const FINDER = 7

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

function isFinderCell(row: number, col: number, count: number): boolean {
  const inTop = row < FINDER
  const inBottom = row >= count - FINDER
  const inLeft = col < FINDER
  const inRight = col >= count - FINDER
  return (inTop && inLeft) || (inTop && inRight) || (inBottom && inLeft)
}

function roundedPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  const r = clamp(radius, 0, Math.min(width, height) / 2)
  ctx.beginPath()
  ctx.moveTo(x + r, y)
  ctx.lineTo(x + width - r, y)
  ctx.arcTo(x + width, y, x + width, y + r, r)
  ctx.lineTo(x + width, y + height - r)
  ctx.arcTo(x + width, y + height, x + width - r, y + height, r)
  ctx.lineTo(x + r, y + height)
  ctx.arcTo(x, y + height, x, y + height - r, r)
  ctx.lineTo(x, y + r)
  ctx.arcTo(x, y, x + r, y, r)
  ctx.closePath()
}

function drawModule(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  cell: number,
  style: QrDotStyle,
) {
  if (style === 'square') {
    ctx.fillRect(x, y, cell, cell)
    return
  }
  const gap = cell * 0.06
  const size = cell - gap * 2
  if (style === 'dot') {
    ctx.beginPath()
    ctx.arc(x + cell / 2, y + cell / 2, size / 2, 0, Math.PI * 2)
    ctx.fill()
    return
  }
  roundedPath(ctx, x + gap, y + gap, size, size, size * 0.32)
  ctx.fill()
}

function drawEye(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  cell: number,
  style: QrEyeStyle,
  fg: string,
  bg: string,
) {
  const radius = style === 'rounded' ? cell * 1.8 : 0
  ctx.fillStyle = fg
  roundedPath(ctx, x, y, cell * 7, cell * 7, radius)
  ctx.fill()
  ctx.fillStyle = bg
  roundedPath(ctx, x + cell, y + cell, cell * 5, cell * 5, Math.max(0, radius - cell * 0.6))
  ctx.fill()
  ctx.fillStyle = fg
  roundedPath(ctx, x + cell * 2, y + cell * 2, cell * 3, cell * 3, Math.max(0, radius - cell * 1.4))
  ctx.fill()
}

function drawLogo(
  ctx: CanvasRenderingContext2D,
  size: number,
  logo: HTMLImageElement,
  style: QrStyle,
) {
  const ratio = clamp(style.logoRatio || 0.22, 0.1, 0.3)
  const box = Math.round(size * ratio)
  const padding = Math.max(2, Math.round(box * 0.12))
  const left = Math.round((size - box) / 2)
  const top = Math.round((size - box) / 2)

  ctx.fillStyle = style.bg
  roundedPath(ctx, left - padding, top - padding, box + padding * 2, box + padding * 2, box * 0.18)
  ctx.fill()

  ctx.save()
  roundedPath(ctx, left, top, box, box, Math.max(0, box * 0.16))
  ctx.clip()
  const scale = Math.min(box / logo.width, box / logo.height)
  const width = logo.width * scale
  const height = logo.height * scale
  ctx.drawImage(logo, left + (box - width) / 2, top + (box - height) / 2, width, height)
  ctx.restore()
}

/** 在 canvas 上绘制二维码，尺寸即导出像素尺寸 */
export function renderQrCanvas(
  canvas: HTMLCanvasElement,
  text: string,
  style: QrStyle,
  logo?: HTMLImageElement | null,
) {
  const qr = QrCode.create(text, { errorCorrectionLevel: style.level })
  const count = qr.modules.size
  const margin = clamp(Math.round(style.margin), 0, 8)
  const size = clamp(Math.round(style.size), 64, 2048)
  const total = count + margin * 2
  const cell = size / total

  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('无法创建画布')
  ctx.fillStyle = style.bg
  ctx.fillRect(0, 0, size, size)

  ctx.fillStyle = style.fg
  for (let row = 0; row < count; row += 1) {
    for (let col = 0; col < count; col += 1) {
      if (!qr.modules.get(row, col)) continue
      if (isFinderCell(row, col, count)) continue
      drawModule(ctx, (margin + col) * cell, (margin + row) * cell, cell, style.dot)
    }
  }

  drawEye(ctx, margin * cell, margin * cell, cell, style.eye, style.fg, style.bg)
  drawEye(ctx, (margin + count - FINDER) * cell, margin * cell, cell, style.eye, style.fg, style.bg)
  drawEye(ctx, margin * cell, (margin + count - FINDER) * cell, cell, style.eye, style.fg, style.bg)

  if (logo) drawLogo(ctx, size, logo, style)
}

/* ---------------- SVG ---------------- */

function escapeXml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/** 生成矢量 SVG，坐标以模块为单位，width/height 为导出尺寸 */
export function buildQrSvg(text: string, style: QrStyle, logoDataUrl?: string): string {
  const qr = QrCode.create(text, { errorCorrectionLevel: style.level })
  const count = qr.modules.size
  const margin = clamp(Math.round(style.margin), 0, 8)
  const total = count + margin * 2
  const size = clamp(Math.round(style.size), 64, 2048)
  const fg = style.fg
  const shapes: string[] = []

  for (let row = 0; row < count; row += 1) {
    for (let col = 0; col < count; col += 1) {
      if (!qr.modules.get(row, col)) continue
      if (isFinderCell(row, col, count)) continue
      const x = margin + col
      const y = margin + row
      if (style.dot === 'dot') {
        shapes.push(`<circle cx="${x + 0.5}" cy="${y + 0.5}" r="0.44" fill="${fg}"/>`)
      } else if (style.dot === 'rounded') {
        shapes.push(
          `<rect x="${x + 0.05}" y="${y + 0.05}" width="0.9" height="0.9" rx="0.3" fill="${fg}"/>`,
        )
      } else {
        shapes.push(`<rect x="${x}" y="${y}" width="1" height="1" fill="${fg}"/>`)
      }
    }
  }

  const radius = style.eye === 'rounded' ? 1.8 : 0
  const eyes = [
    [margin, margin],
    [margin + count - FINDER, margin],
    [margin, margin + count - FINDER],
  ]
    .map(([x, y]) => {
      const innerRadius = Math.max(0, radius - 0.6).toFixed(2)
      const centerRadius = Math.max(0, radius - 1.4).toFixed(2)
      return (
        `<rect x="${x}" y="${y}" width="7" height="7" rx="${radius}" fill="${fg}"/>` +
        `<rect x="${x + 1}" y="${y + 1}" width="5" height="5" rx="${innerRadius}" fill="${style.bg}"/>` +
        `<rect x="${x + 2}" y="${y + 2}" width="3" height="3" rx="${centerRadius}" fill="${fg}"/>`
      )
    })
    .join('')

  const logoParts: string[] = []
  if (logoDataUrl) {
    const ratio = clamp(style.logoRatio || 0.22, 0.1, 0.3)
    const box = total * ratio
    const padding = box * 0.12
    const left = (total - box) / 2
    const top = (total - box) / 2
    logoParts.push(
      `<rect x="${left - padding}" y="${top - padding}" width="${box + padding * 2}" height="${box + padding * 2}" rx="${box * 0.18}" fill="${style.bg}"/>`,
      `<image href="${escapeXml(logoDataUrl)}" x="${left}" y="${top}" width="${box}" height="${box}" preserveAspectRatio="xMidYMid meet"/>`,
    )
  }

  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${total} ${total}" shape-rendering="${style.dot === 'square' ? 'crispEdges' : 'geometricPrecision'}">`,
    `<rect width="${total}" height="${total}" fill="${style.bg}"/>`,
    shapes.join(''),
    eyes,
    logoParts.join(''),
    '</svg>',
  ].join('')
}

/* ---------------- 识别 ---------------- */

export interface DecodedQr {
  data: string
  version: number
}

export function loadImage(source: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error('图片加载失败'))
    image.src = source
  })
}

/** 从图片文件识别二维码，长边超过 1600px 时等比缩小以加快识别 */
export async function decodeQrImage(file: File): Promise<DecodedQr> {
  const url = URL.createObjectURL(file)
  try {
    const image = await loadImage(url)
    const scale = Math.min(1, 1600 / Math.max(image.width, image.height))
    const width = Math.max(1, Math.round(image.width * scale))
    const height = Math.max(1, Math.round(image.height * scale))
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) throw new Error('无法创建画布')
    ctx.drawImage(image, 0, 0, width, height)
    const pixels = ctx.getImageData(0, 0, width, height)
    const code = jsQR(pixels.data, width, height, { inversionAttempts: 'attemptBoth' })
    if (!code) throw new Error('未识别到二维码，请换一张更清晰、完整的图片')
    return { data: code.data, version: code.version }
  } finally {
    URL.revokeObjectURL(url)
  }
}

export interface QrSummary {
  label: string
  fields: { key: string; value: string }[]
}

function splitWifi(value: string): string[] {
  const parts: string[] = []
  let current = ''
  let escaped = false
  for (const char of value) {
    if (escaped) {
      current += char
      escaped = false
    } else if (char === '\\') {
      escaped = true
    } else if (char === ';') {
      parts.push(current)
      current = ''
    } else {
      current += char
    }
  }
  parts.push(current)
  return parts
}

function unescapeWifi(value: string): string {
  return value.replace(/\\([\\;,":])/g, '$1')
}

/** 解析常见二维码内容，用于识别结果的结构化展示 */
export function describeQrPayload(data: string): QrSummary {
  const text = data.trim()
  if (text.startsWith('WIFI:')) {
    const fields: QrSummary['fields'] = []
    for (const part of splitWifi(text.slice(5))) {
      const index = part.indexOf(':')
      if (index <= 0) continue
      const key = part.slice(0, index)
      const value = unescapeWifi(part.slice(index + 1))
      if (!value) continue
      if (key === 'S') fields.push({ key: '网络名称', value })
      else if (key === 'P') fields.push({ key: '密码', value })
      else if (key === 'T') fields.push({ key: '加密方式', value })
      else if (key === 'H' && value === 'true') fields.push({ key: '隐藏网络', value: '是' })
    }
    return { label: 'WiFi 连接', fields }
  }
  if (text.startsWith('BEGIN:VCARD')) {
    const fields: QrSummary['fields'] = []
    let hasName = false
    for (const line of text.split(/\r?\n/)) {
      const [rawKey, ...rest] = line.split(':')
      const value = rest.join(':').trim()
      if (!value) continue
      const key = rawKey.split(';')[0].toUpperCase()
      if (key === 'FN') {
        hasName = true
        fields.push({ key: '姓名', value })
      } else if (key === 'N') {
        if (!hasName) fields.push({ key: '姓名', value: value.replace(/;+/g, ' ').trim() })
      } else if (key === 'TEL') fields.push({ key: '电话', value })
      else if (key === 'EMAIL') fields.push({ key: '邮箱', value })
      else if (key === 'ORG') fields.push({ key: '公司', value })
      else if (key === 'TITLE') fields.push({ key: '职位', value })
      else if (key === 'URL') fields.push({ key: '网址', value })
      else if (key === 'ADR') fields.push({ key: '地址', value: value.replace(/;+/g, ' ').trim() })
    }
    return { label: '联系人名片', fields }
  }
  if (/^https?:\/\//i.test(text)) {
    return { label: '网址', fields: [{ key: '链接', value: text }] }
  }
  if (text.startsWith('mailto:')) {
    try {
      const url = new URL(text)
      const fields: QrSummary['fields'] = [{ key: '收件人', value: url.pathname }]
      if (url.searchParams.get('subject')) fields.push({ key: '主题', value: url.searchParams.get('subject') || '' })
      if (url.searchParams.get('body')) fields.push({ key: '正文', value: url.searchParams.get('body') || '' })
      return { label: '邮件', fields }
    } catch {
      return { label: '邮件', fields: [{ key: '内容', value: text }] }
    }
  }
  if (text.startsWith('SMSTO:')) {
    const rest = text.slice(6)
    const index = rest.indexOf(':')
    const phone = index >= 0 ? rest.slice(0, index) : rest
    const message = index >= 0 ? rest.slice(index + 1) : ''
    const fields: QrSummary['fields'] = [{ key: '号码', value: phone }]
    if (message) fields.push({ key: '内容', value: message })
    return { label: '短信', fields }
  }
  if (text.startsWith('tel:')) {
    return { label: '电话', fields: [{ key: '号码', value: text.slice(4) }] }
  }
  if (text.startsWith('geo:')) {
    const [lat, lng] = text.slice(4).split(',')
    return {
      label: '定位',
      fields: [
        { key: '纬度', value: lat ?? '' },
        { key: '经度', value: lng ?? '' },
      ],
    }
  }
  return { label: '文本', fields: [] }
}
