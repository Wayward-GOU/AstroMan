/* 网络文本解析：User-Agent、原始 HTTP 报文 */

interface UaRule {
  label: string
  pattern: RegExp
  versionGroup?: number
}

const BROWSER_RULES: UaRule[] = [
  { label: '微信内置浏览器', pattern: /MicroMessenger\/([\d.]+)/, versionGroup: 1 },
  { label: '企业微信', pattern: /wxwork\/([\d.]+)/, versionGroup: 1 },
  { label: '钉钉', pattern: /DingTalk\/([\d.]+)/, versionGroup: 1 },
  { label: '支付宝', pattern: /AlipayClient\/([\d.]+)/, versionGroup: 1 },
  { label: 'QQ 浏览器', pattern: /MQQBrowser\/([\d.]+)/, versionGroup: 1 },
  { label: 'Edge', pattern: /Edg(?:e|A|iOS)?\/([\d.]+)/, versionGroup: 1 },
  { label: 'Opera', pattern: /OPR\/([\d.]+)/, versionGroup: 1 },
  { label: 'Firefox', pattern: /Firefox\/([\d.]+)/, versionGroup: 1 },
  { label: 'Chrome', pattern: /Chrome\/([\d.]+)/, versionGroup: 1 },
  { label: 'Safari', pattern: /Version\/([\d.]+).*Safari\//, versionGroup: 1 },
  { label: 'Internet Explorer', pattern: /(?:MSIE |rv:)([\d.]+).*Trident/, versionGroup: 1 },
  { label: 'curl', pattern: /curl\/([\d.]+)/, versionGroup: 1 },
  { label: 'PostmanRuntime', pattern: /PostmanRuntime\/([\d.]+)/, versionGroup: 1 },
  { label: 'Python Requests', pattern: /python-requests\/([\d.]+)/, versionGroup: 1 },
  { label: 'Go HTTP Client', pattern: /Go-http-client\/([\d.]+)/, versionGroup: 1 },
]

const OS_RULES: UaRule[] = [
  { label: 'Windows 11 / 10', pattern: /Windows NT 10\.0/ },
  { label: 'Windows 8.1', pattern: /Windows NT 6\.3/ },
  { label: 'Windows 8', pattern: /Windows NT 6\.2/ },
  { label: 'Windows 7', pattern: /Windows NT 6\.1/ },
  { label: 'Windows Vista', pattern: /Windows NT 6\.0/ },
  { label: 'Windows XP', pattern: /Windows NT 5\.1/ },
  { label: 'HarmonyOS', pattern: /HarmonyOS/ },
  { label: 'Android', pattern: /Android\s?([\d.]+)?/ },
  { label: 'iOS', pattern: /(?:iPhone )?OS ([\d_]+) like Mac OS X/, versionGroup: 1 },
  { label: 'macOS', pattern: /Mac OS X ([\d_.]+)/, versionGroup: 1 },
  { label: 'ChromeOS', pattern: /CrOS/ },
  { label: 'Linux', pattern: /Linux/ },
]

const DEVICE_RULES: UaRule[] = [
  { label: '爬虫 / 机器人', pattern: /(bot|crawler|spider|slurp)/i },
  { label: 'iPad', pattern: /iPad/ },
  { label: 'iPhone', pattern: /iPhone/ },
  { label: 'Android 手机', pattern: /Android.*Mobile/ },
  { label: 'Android 平板', pattern: /Android/ },
  { label: '桌面设备', pattern: /(Windows NT|Macintosh|X11)/ },
]

function matchRule(rules: UaRule[], ua: string): string {
  for (const rule of rules) {
    const hit = rule.pattern.exec(ua)
    if (!hit) continue
    if (rule.versionGroup && hit[rule.versionGroup]) {
      return `${rule.label} ${hit[rule.versionGroup].replace(/_/g, '.')}`
    }
    return rule.label
  }
  return '未知'
}

export function parseUserAgent(input: string): string {
  const ua = input.trim()
  if (!ua) throw new Error('请粘贴 User-Agent 字符串')
  const browser = matchRule(BROWSER_RULES, ua)
  const os = matchRule(OS_RULES, ua)
  const device = matchRule(DEVICE_RULES, ua)
  const engine = /AppleWebKit\//.test(ua)
    ? /Chrome\/|Edg\//.test(ua)
      ? 'Blink（基于 WebKit/537.36）'
      : 'WebKit'
    : /Gecko\/|Firefox\//.test(ua) && !/like Gecko/.test(ua)
      ? 'Gecko'
      : /Trident\//.test(ua)
        ? 'Trident'
        : '未知'
  const arch = /(x86_64|Win64|WOW64|arm64|aarch64)/i.exec(ua)?.[1] ?? '未标注'
  return [
    `浏览器 / 客户端：${browser}`,
    `操作系统：${os}`,
    `设备类型：${device}`,
    `渲染引擎：${engine}`,
    `CPU 架构：${arch}`,
    `字符长度：${ua.length}`,
    '',
    '原始 UA：',
    ua,
  ].join('\n')
}

export interface ParsedHttpMessage {
  kind: 'request' | 'response'
  method: string
  url: string
  protocol: string
  status: string
  headers: { name: string; value: string }[]
  body: string
}

export function parseHttpMessage(input: string): ParsedHttpMessage {
  const text = input.replace(/\r\n/g, '\n').trim()
  if (!text) throw new Error('请粘贴原始 HTTP 报文')
  const lines = text.split('\n')
  const first = lines[0].trim()
  const requestMatch = /^([A-Z]+)\s+(\S+)\s+HTTP\/([\d.]+)$/.exec(first)
  const responseMatch = /^HTTP\/([\d.]+)\s+(\d{3})\s*(.*)$/.exec(first)
  if (!requestMatch && !responseMatch) {
    throw new Error('首行既不是请求行（GET /path HTTP/1.1），也不是状态行（HTTP/1.1 200 OK）')
  }
  const headers: { name: string; value: string }[] = []
  let index = 1
  for (; index < lines.length; index += 1) {
    const line = lines[index]
    if (!line.trim()) {
      index += 1
      break
    }
    const separator = line.indexOf(':')
    if (separator < 0) continue
    headers.push({ name: line.slice(0, separator).trim(), value: line.slice(separator + 1).trim() })
  }
  return {
    kind: requestMatch ? 'request' : 'response',
    method: requestMatch ? requestMatch[1] : '',
    url: requestMatch ? requestMatch[2] : '',
    protocol: requestMatch ? `HTTP/${requestMatch[3]}` : `HTTP/${responseMatch![1]}`,
    status: responseMatch ? `${responseMatch[2]} ${responseMatch[3] ?? ''}`.trim() : '',
    headers,
    body: lines.slice(index).join('\n'),
  }
}

export function httpMessageInfo(input: string): string {
  const parsed = parseHttpMessage(input)
  const headerMap = new Map(parsed.headers.map((item) => [item.name.toLowerCase(), item.value]))
  const lines: string[] = []
  if (parsed.kind === 'request') {
    const [path, query = ''] = parsed.url.split('?')
    lines.push(`报文类型：HTTP 请求`, `方法：${parsed.method}`, `协议：${parsed.protocol}`, `路径：${path || '/'}`)
    if (query) {
      const params = new URLSearchParams(query)
      const list = Array.from(params.entries())
      lines.push(`查询参数（${list.length}）：`, ...list.map(([key, value]) => `  ${key} = ${value || '(空)'}`))
    }
  } else {
    lines.push(`报文类型：HTTP 响应`, `状态：${parsed.status}`, `协议：${parsed.protocol}`)
  }
  lines.push('', `请求头 / 响应头（${parsed.headers.length}）：`)
  lines.push(...parsed.headers.map((item) => `  ${item.name}: ${item.value}`))
  if (parsed.body) {
    lines.push('', `Body 长度：${parsed.body.length} 字符`)
    if (!headerMap.get('content-type')) lines.push('提示：缺少 Content-Type，服务端可能按 text/plain 处理。')
    lines.push('', 'Body 预览：', parsed.body.slice(0, 2000))
  } else {
    lines.push('', 'Body：空')
  }
  const warnings: string[] = []
  if (!headerMap.get('host') && parsed.kind === 'request') warnings.push('缺少 Host 头，HTTP/1.1 请求必须携带。')
  if (parsed.body && parsed.kind === 'request' && !headerMap.get('content-length') && !headerMap.get('transfer-encoding')) {
    warnings.push('有 Body 但缺少 Content-Length / Transfer-Encoding。')
  }
  if (warnings.length) lines.push('', '检查建议：', ...warnings.map((item) => `  · ${item}`))
  return lines.join('\n')
}
