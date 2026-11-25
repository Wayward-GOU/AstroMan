import type { EnvVariable, HttpRequestDetail, KeyValueItem } from '@/types/models'
import { base64Encode } from '@/tools/transforms'
import { parseLooseJson } from '@/utils/looseJson'
import { inferBodyContentType } from '@/utils/bodyContentType'

export interface Snippet {
  key: string
  label: string
  language: string
  code: string
}

/** POSIX 单引号转义，可直接粘贴到 macOS / Linux 终端 */
function quote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

function enabled(items: KeyValueItem[] | undefined): KeyValueItem[] {
  return (items ?? []).filter((item) => item.enabled && item.key.trim())
}

function resolveVariables(text: string, variables?: EnvVariable[]): string {
  if (!variables?.length) return text
  return text.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (raw, name: string) => {
    const hit = variables.find((item) => item.key === name.trim())
    if (!hit || hit.isSecret || hit.value === undefined || hit.value === null) return raw
    return hit.value
  })
}

/** 编码查询参数，但保留 {{变量}} 原样，避免导出后变量失效 */
function encodeQueryPart(value: string): string {
  return value
    .split(/(\{\{[^{}]*\}\})/)
    .map((part, index) => (index % 2 === 1 ? part : encodeURIComponent(part)))
    .join('')
}

function authHeader(request: HttpRequestDetail): KeyValueItem | null {
  const auth = request.auth
  if (!auth || auth.kind === 'none') return null
  if (auth.kind === 'bearer' || auth.kind === 'oauth2') {
    const token = (auth.token ?? '').trim()
    return token ? { key: 'Authorization', value: `Bearer ${token}`, enabled: true } : null
  }
  if (auth.kind === 'basic') {
    return {
      key: 'Authorization',
      value: `Basic ${base64Encode(`${auth.username ?? ''}:${auth.password ?? ''}`)}`,
      enabled: true,
    }
  }
  if (auth.kind === 'apikey') {
    const key = (auth.key ?? '').trim()
    return key && auth.addTo !== 'query'
      ? { key, value: auth.value ?? '', enabled: true }
      : null
  }
  return null
}

function authQuery(request: HttpRequestDetail): [string, string] | null {
  const auth = request.auth
  if (auth?.kind !== 'apikey' || auth.addTo !== 'query') return null
  const key = (auth.key ?? '').trim()
  return key ? [key, auth.value ?? ''] : null
}

function buildUrl(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const params = enabled(request.params)
  let url = (request.url ?? '').trim()
  const pairs = params.map((item) => {
    const key = resolveVariables(item.key, variables)
    const value = resolveVariables(item.value ?? '', variables)
    return `${encodeQueryPart(key)}=${encodeQueryPart(value)}`
  })
  const extra = authQuery(request)
  if (extra) {
    const key = resolveVariables(extra[0], variables)
    const value = resolveVariables(extra[1], variables)
    pairs.push(`${encodeQueryPart(key)}=${encodeQueryPart(value)}`)
  }
  if (!pairs.length) return url
  url = url.replace(/[?&]$/, '')
  const query = pairs.join('&')
  return url.includes('?') ? `${url}&${query}` : `${url}?${query}`
}

function formPairs(body: string): string[] {
  return body
    .split('&')
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => {
      try {
        return decodeURIComponent(item.replace(/\+/g, ' '))
      } catch {
        return item
      }
    })
}

/** 请求头来源：手填 / 认证注入 / 工具自动补 */
export type HeaderSource = 'user' | 'auth' | 'auto'

export interface PreviewHeader {
  key: string
  value: string
  source: HeaderSource
}

/**
 * 组装最终会发出的请求头，并标注每一条的来源。
 * buildCurl / buildHttpRaw / previewRequest 共用这一份逻辑，避免导出与实际不一致。
 */
function headerListAll(request: HttpRequestDetail): PreviewHeader[] {
  const list: PreviewHeader[] = enabled(request.headers).map((item) => ({
    key: item.key,
    value: item.value ?? '',
    source: 'user',
  }))
  const added = authHeader(request)
  if (added && !list.some((item) => item.key.toLowerCase() === added.key.toLowerCase())) {
    list.push({ key: added.key, value: added.value ?? '', source: 'auth' })
  }
  const bodyType = request.bodyType ?? 'none'
  // multipart 的 Content-Type 必须由工具自动生成 boundary，不能手写。
  // 其余类型无论正文是否为空都要补（后端 build_body_plan 同样无条件补），
  // 否则「实际请求」页签和真正发出的请求会对不上。
  if (bodyType !== 'none' && bodyType !== 'multipart') {
    const hasContentType = list.some((item) => item.key.toLowerCase() === 'content-type')
    if (!hasContentType) {
      list.push({
        key: 'Content-Type',
        value: inferBodyContentType(bodyType, request.body ?? ''),
        source: 'auto',
      })
    }
  }
  return list
}

function headerList(request: HttpRequestDetail): KeyValueItem[] {
  return headerListAll(request).map((item) => ({ key: item.key, value: item.value, enabled: true }))
}

function parseFormFields(body: string): { key: string; value: string; kind: string }[] {
  const text = (body || '').trim()
  if (!text) return []
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    // 字段列表被手改成单引号 / 尾逗号时也要能生成 curl，否则会导出成空 multipart
    try {
      parsed = parseLooseJson(text).value
    } catch {
      return []
    }
  }
  if (!Array.isArray(parsed)) return []
  return parsed
    .filter((item) => item && item.enabled !== false && String(item.key ?? '').trim())
    .map((item) => ({
      key: String(item.key).trim(),
      value: String(item.value ?? ''),
      kind: item.kind === 'file' ? 'file' : 'text',
    }))
}

/** Basic 认证用 curl -u 表达，避免把占位符提前 base64 后无法再替换 */
function basicAuthPair(request: HttpRequestDetail): string | null {
  const auth = request.auth
  if (auth?.kind !== 'basic') return null
  const hasManual = enabled(request.headers).some(
    (item) => item.key.toLowerCase() === 'authorization',
  )
  if (hasManual) return null
  return `${auth.username ?? ''}:${auth.password ?? ''}`
}

/** 把请求配置转换成可分享的 cURL 命令；传入环境变量时会替换 {{变量}}，敏感变量保持占位符 */
export function buildCurl(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toUpperCase()
  const parts: string[] = [`curl -X ${method} ${quote(buildUrl(request, variables))}`]
  const basicPair = basicAuthPair(request)

  for (const header of headerList(request)) {
    if (basicPair && header.key.toLowerCase() === 'authorization') continue
    parts.push(`  -H ${quote(`${header.key}: ${header.value ?? ''}`)}`)
  }
  if (basicPair) parts.push(`  -u ${quote(basicPair)}`)

  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  if (bodyType !== 'none' && body) {
    if (bodyType === 'form') {
      for (const pair of formPairs(body)) {
        parts.push(`  --data-urlencode ${quote(pair)}`)
      }
    } else if (bodyType === 'multipart') {
      for (const field of parseFormFields(body)) {
        parts.push(
          field.kind === 'file'
            ? `  -F ${quote(`${field.key}=@${field.value}`)}`
            : `  -F ${quote(`${field.key}=${field.value}`)}`,
        )
      }
    } else if (bodyType === 'binary') {
      parts.push(`  --data-binary ${quote(`@${body}`)}`)
    } else {
      parts.push(`  --data-raw ${quote(body)}`)
    }
  }

  return resolveVariables(parts.join(' \\\n'), variables)
}

/** 原始 HTTP 报文，方便直接贴给后端同学 */
export function buildHttpRaw(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toUpperCase()
  const url = buildUrl(request, variables)
  let path = url
  let host = ''
  try {
    const parsed = new URL(url)
    host = parsed.host
    path = `${parsed.pathname || '/'}${parsed.search}`
  } catch {
    /* 相对地址时保持原样 */
  }
  const lines = [`${method} ${path} HTTP/1.1`]
  if (host) lines.push(`Host: ${host}`)
  for (const header of headerList(request)) {
    lines.push(`${header.key}: ${header.value ?? ''}`)
  }
  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  if (bodyType !== 'none' && body) {
    lines.push('', body)
  }
  return resolveVariables(lines.join('\n'), variables)
}

/** JavaScript fetch 代码 */
export function buildFetch(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toUpperCase()
  const headerEntries = headerList(request)
    .map((item) => `    ${quoteJs(item.key)}: ${quoteJs(item.value ?? '')},`)
    .join('\n')
  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  const options: string[] = [`  method: ${quoteJs(method)},`]
  if (headerEntries) options.push(`  headers: {\n${headerEntries}\n  },`)
  if (bodyType === 'multipart') {
    const appends = parseFormFields(body)
      .map((field) =>
        field.kind === 'file'
          ? `  form.append(${quoteJs(field.key)}, fileInput.files[0])`
          : `  form.append(${quoteJs(field.key)}, ${quoteJs(field.value)})`,
      )
      .join('\n')
    if (appends) {
      options.push('  body: form,')
      const code = `const form = new FormData()\n${appends}\n\nconst response = await fetch(${quoteJs(buildUrl(request, variables))}, {\n${options.join('\n')}\n})\n\nconst data = await response.text()\nconsole.log(response.status, data)`
      return resolveVariables(code, variables)
    }
  }
  if (bodyType === 'binary') {
    options.push(`  body: binaryBody, // 需自行读取本地文件：${body}`)
  } else if (bodyType === 'form' && body) {
    const pairs = formPairs(body)
      .map((pair) => {
        const index = pair.indexOf('=')
        const key = index >= 0 ? pair.slice(0, index) : pair
        const value = index >= 0 ? pair.slice(index + 1) : ''
        return `  ${quoteJs(key)}: ${quoteJs(value)},`
      })
      .join('\n')
    options.push(`  body: new URLSearchParams({\n${pairs}\n  }),`)
  } else if (bodyType !== 'none' && body) {
    options.push(`  body: ${quoteJs(body)},`)
  }
  const code = `const response = await fetch(${quoteJs(buildUrl(request, variables))}, {\n${options.join('\n')}\n})\n\nconst data = await response.text()\nconsole.log(response.status, data)`
  return resolveVariables(code, variables)
}

/** Python requests 代码 */
export function buildPython(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toLowerCase()
  const headerEntries = headerList(request)
    .map((item) => `    ${quoteJs(item.key)}: ${quoteJs(item.value ?? '')},`)
    .join('\n')
  const lines = ['import requests', '', `url = ${quoteJs(buildUrl(request, variables))}`]
  if (headerEntries) lines.push(`headers = {\n${headerEntries}\n}`)
  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  const args = ['url']
  if (headerEntries) args.push('headers=headers')
  if (bodyType === 'multipart') {
    const fields = parseFormFields(body)
    const dataLines = fields
      .filter((field) => field.kind !== 'file')
      .map((field) => `    ${quoteJs(field.key)}: ${quoteJs(field.value)},`)
      .join('\n')
    const fileLines = fields
      .filter((field) => field.kind === 'file')
      .map((field) => `    ${quoteJs(field.key)}: open(${quoteJs(field.value)}, 'rb'),`)
      .join('\n')
    if (dataLines) {
      lines.push(`data = {\n${dataLines}\n}`)
      args.push('data=data')
    }
    if (fileLines) {
      lines.push(`files = {\n${fileLines}\n}`)
      args.push('files=files')
    }
  } else if (bodyType === 'binary') {
    lines.push(`with open(${quoteJs(body)}, 'rb') as file:`, '    payload = file.read()')
    args.push('data=payload')
  } else if (bodyType === 'form' && body) {
    const pairs = formPairs(body)
      .map((pair) => {
        const index = pair.indexOf('=')
        const key = index >= 0 ? pair.slice(0, index) : pair
        const value = index >= 0 ? pair.slice(index + 1) : ''
        return `    ${quoteJs(key)}: ${quoteJs(value)},`
      })
      .join('\n')
    lines.push(`data = {\n${pairs}\n}`)
    args.push('data=data')
  } else if (bodyType !== 'none' && body) {
    lines.push(`payload = ${quoteJs(body)}`)
    args.push('data=payload')
  }
  lines.push('', `response = requests.${method}(${args.join(', ')})`, 'print(response.status_code, response.text)')
  return resolveVariables(lines.join('\n'), variables)
}

/** 生成 Postman 风格的代码片段列表 */
export function buildSnippets(request: HttpRequestDetail, variables?: EnvVariable[]): Snippet[] {
  return [
    { key: 'curl', label: 'cURL', language: 'bash', code: buildCurl(request, variables) },
    { key: 'http', label: 'HTTP', language: 'http', code: buildHttpRaw(request, variables) },
    { key: 'fetch', label: 'JavaScript', language: 'javascript', code: buildFetch(request, variables) },
    { key: 'python', label: 'Python', language: 'python', code: buildPython(request, variables) },
  ]
}

export interface RequestPreview {
  method: string
  /** 变量替换、Query 拼接后的最终地址 */
  url: string
  headers: PreviewHeader[]
  /** 环境里找不到定义、会原样带 {{name}} 发出去的变量名 */
  unresolved: string[]
  /** 已定义但标记为敏感、故意不展开的变量名 */
  secretVars: string[]
  bodyType: string
  /** 文本正文字节数；文件上传类为 0（内容不在这里展开） */
  bodyBytes: number
}

/**
 * 发送前预览：把「变量替换 + Query 拼接 + 认证头注入 + 默认 Content-Type」
 * 都算一遍，口径与 cURL 导出完全一致。
 *
 * 注意：这不等同于抓包看到的原始报文。UA、Accept-Encoding、Host、
 * Content-Length 等由 HTTP 客户端与网络层补齐，不会出现在这里。
 */
export function previewRequest(
  request: HttpRequestDetail,
  variables?: EnvVariable[],
): RequestPreview {
  const url = resolveVariables(buildUrl(request, variables), variables)
  const headers = headerListAll(request).map((item) => ({
    ...item,
    value: resolveVariables(item.value, variables),
  }))
  const definitions = new Map((variables ?? []).map((item) => [item.key, item]))
  const unresolved = new Set<string>()
  const secretVars = new Set<string>()
  for (const match of [url, ...headers.map((item) => item.value)].join('\n').matchAll(
    /\{\{\s*([^}]+?)\s*\}\}/g,
  )) {
    const name = (match[1] ?? '').trim()
    if (!name) continue
    const hit = definitions.get(name)
    if (!hit) unresolved.add(name)
    else if (hit.isSecret) secretVars.add(name)
  }
  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  const countable = bodyType !== 'none' && bodyType !== 'binary' && bodyType !== 'multipart'
  return {
    method: (request.method || 'GET').toUpperCase(),
    url,
    headers,
    unresolved: [...unresolved],
    secretVars: [...secretVars],
    bodyType,
    bodyBytes: countable && body ? new TextEncoder().encode(body).length : 0,
  }
}

function quoteJs(value: string): string {
  return JSON.stringify(value)
}

export function resolveCurlVars(text: string, variables?: EnvVariable[]): string {
  return resolveVariables(text, variables)
}

const CURL_HEAD_RE = /(^|[\s`>$])curl\s/i

/**
 * 从一段自由文本（用户粘贴的消息、代码块、附件）里摘出 cURL 命令原文。
 *
 * 用途：模型在工具参数里复述长 cURL 时可能被输出长度限制截断，
 * 这时直接拿用户消息里的原文补全，保证导入的是完整命令。
 * 优先取 ``` 代码块内的内容，没有代码块则从 curl 关键字一直取到文本末尾。
 */
export function extractCurlCommand(text: string): string | null {
  if (!text) return null
  const normalized = text.replace(/\r\n/g, '\n')

  // 1. 代码块内的 curl 最可信
  const fenceRe = /```[a-zA-Z0-9_-]*[ \t]*\n([\s\S]*?)```/g
  let fence: RegExpExecArray | null
  while ((fence = fenceRe.exec(normalized))) {
    const block = fence[1] ?? ''
    if (CURL_HEAD_RE.test(block)) return block.trim()
  }

  // 2. 正文里直接出现 curl：取到文本结尾
  const match = CURL_HEAD_RE.exec(normalized)
  if (!match) return null
  const start = match.index + match[1].length
  const body = normalized.slice(start).trim()
  return body || null
}
