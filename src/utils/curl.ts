import type { EnvVariable, HttpRequestDetail, KeyValueItem } from '@/types/models'

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

function buildUrl(request: HttpRequestDetail): string {
  const params = enabled(request.params)
  let url = (request.url ?? '').trim()
  if (!params.length) return url
  url = url.replace(/[?&]$/, '')
  const query = params
    .map((item) => `${encodeURIComponent(item.key)}=${encodeURIComponent(item.value ?? '')}`)
    .join('&')
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

function headerList(request: HttpRequestDetail): KeyValueItem[] {
  const list = enabled(request.headers).map((item) => ({ ...item }))
  const bodyType = request.bodyType ?? 'none'
  const body = (request.body ?? '').trim()
  // multipart 的 Content-Type 必须由工具自动生成 boundary，不能手写
  if (body && bodyType !== 'none' && bodyType !== 'multipart') {
    const hasContentType = list.some((item) => item.key.toLowerCase() === 'content-type')
    if (!hasContentType) {
      list.push({
        key: 'Content-Type',
        value:
          bodyType === 'form'
            ? 'application/x-www-form-urlencoded'
            : bodyType === 'binary'
              ? 'application/octet-stream'
              : 'application/json',
        enabled: true,
      })
    }
  }
  return list
}

function parseFormFields(body: string): { key: string; value: string; kind: string }[] {
  try {
    const parsed = JSON.parse(body || '[]')
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((item) => item && item.enabled !== false && String(item.key ?? '').trim())
      .map((item) => ({
        key: String(item.key).trim(),
        value: String(item.value ?? ''),
        kind: item.kind === 'file' ? 'file' : 'text',
      }))
  } catch {
    return []
  }
}

/** 把请求配置转换成可分享的 cURL 命令；传入环境变量时会替换 {{变量}}，敏感变量保持占位符 */
export function buildCurl(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toUpperCase()
  const parts: string[] = [`curl -X ${method} ${quote(buildUrl(request))}`]

  for (const header of headerList(request)) {
    parts.push(`  -H ${quote(`${header.key}: ${header.value ?? ''}`)}`)
  }

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
  const url = buildUrl(request)
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
      const code = `const form = new FormData()\n${appends}\n\nconst response = await fetch(${quoteJs(buildUrl(request))}, {\n${options.join('\n')}\n})\n\nconst data = await response.text()\nconsole.log(response.status, data)`
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
  const code = `const response = await fetch(${quoteJs(buildUrl(request))}, {\n${options.join('\n')}\n})\n\nconst data = await response.text()\nconsole.log(response.status, data)`
  return resolveVariables(code, variables)
}

/** Python requests 代码 */
export function buildPython(request: HttpRequestDetail, variables?: EnvVariable[]): string {
  const method = (request.method || 'GET').toLowerCase()
  const headerEntries = headerList(request)
    .map((item) => `    ${quoteJs(item.key)}: ${quoteJs(item.value ?? '')},`)
    .join('\n')
  const lines = ['import requests', '', `url = ${quoteJs(buildUrl(request))}`]
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

function quoteJs(value: string): string {
  return JSON.stringify(value)
}

export function resolveCurlVars(text: string, variables?: EnvVariable[]): string {
  return resolveVariables(text, variables)
}
