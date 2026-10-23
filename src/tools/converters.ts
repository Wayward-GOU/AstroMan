/* 结构化数据互转：JSON / XML / YAML / CSV / Query String */

export function parseJson(text: string): unknown {
  if (!text.trim()) throw new Error('请输入 JSON 内容')
  try {
    return JSON.parse(text)
  } catch (error) {
    throw new Error(`JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
}

export function stringifyJson(value: unknown, indent = 2): string {
  return JSON.stringify(value, null, indent === 0 ? undefined : ' '.repeat(indent))
}

/* ---------------- JSON ↔ Query String ---------------- */

function flattenQuery(value: unknown, prefix: string, out: string[]): void {
  if (value === null || value === undefined) {
    out.push(`${encodeURIComponent(prefix)}=`)
    return
  }
  if (Array.isArray(value)) {
    if (!value.length) {
      out.push(`${encodeURIComponent(prefix)}=`)
      return
    }
    value.forEach((item) => flattenQuery(item, prefix, out))
    return
  }
  if (typeof value === 'object') {
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
      const next = prefix ? `${prefix}[${key}]` : key
      flattenQuery(item, next, out)
    })
    return
  }
  out.push(`${encodeURIComponent(prefix)}=${encodeURIComponent(String(value))}`)
}

export function jsonToQuery(text: string): string {
  const value = parseJson(text)
  if (value === null || typeof value !== 'object') {
    throw new Error('JSON 根节点需要是对象或数组')
  }
  const parts: string[] = []
  if (Array.isArray(value)) {
    value.forEach((item) => {
      if (item !== null && typeof item === 'object') {
        flattenQuery(item, '', parts)
      } else {
        parts.push(encodeURIComponent(String(item)))
      }
    })
  } else {
    flattenQuery(value, '', parts)
  }
  return parts.join('&')
}

export function queryToJson(text: string): string {
  const raw = text.trim().replace(/^\?/, '')
  const result: Record<string, unknown> = {}
  if (!raw) return '{}'
  raw.split('&').forEach((pair) => {
    if (!pair) return
    const index = pair.indexOf('=')
    const key = decodeURIComponent((index >= 0 ? pair.slice(0, index) : pair).replace(/\+/g, ' '))
    const value = index >= 0 ? decodeURIComponent(pair.slice(index + 1).replace(/\+/g, ' ')) : ''
    if (!(key in result)) {
      result[key] = value
      return
    }
    const current = result[key]
    if (Array.isArray(current)) current.push(value)
    else result[key] = [current, value]
  })
  return stringifyJson(result, 2)
}

/* ---------------- JSON → YAML ---------------- */

function yamlScalar(value: unknown): string {
  if (value === null) return 'null'
  if (typeof value === 'boolean') return String(value)
  if (typeof value === 'number') return Number.isFinite(value) ? String(value) : 'null'
  const text = String(value)
  if (text === '') return "''"
  if (/^[A-Za-z0-9_./-]+$/.test(text) && !/^(true|false|null|yes|no|on|off|~)$/i.test(text)) {
    return text
  }
  return `'${text.replace(/'/g, "''")}'`
}

function yamlKey(key: string): string {
  return /^[A-Za-z0-9_-]+$/.test(key) ? key : `'${key.replace(/'/g, "''")}'`
}

function emitYaml(value: unknown, indent: number, lines: string[]): void {
  const pad = '  '.repeat(indent)
  if (Array.isArray(value)) {
    if (!value.length) {
      lines.push(`${pad}[]`)
      return
    }
    value.forEach((item) => {
      if (item !== null && typeof item === 'object') {
        const nested: string[] = []
        emitYaml(item, indent + 1, nested)
        const [first, ...rest] = nested
        lines.push(`${pad}- ${first.trimStart()}`)
        rest.forEach((line) => lines.push(line))
      } else {
        lines.push(`${pad}- ${yamlScalar(item)}`)
      }
    })
    return
  }
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    if (!entries.length) {
      lines.push(`${pad}{}`)
      return
    }
    entries.forEach(([key, item]) => {
      if (item !== null && typeof item === 'object') {
        if (Array.isArray(item) && !item.length) {
          lines.push(`${pad}${yamlKey(key)}: []`)
        } else if (!Array.isArray(item) && !Object.keys(item as object).length) {
          lines.push(`${pad}${yamlKey(key)}: {}`)
        } else {
          lines.push(`${pad}${yamlKey(key)}:`)
          emitYaml(item, indent + 1, lines)
        }
      } else {
        lines.push(`${pad}${yamlKey(key)}: ${yamlScalar(item)}`)
      }
    })
    return
  }
  lines.push(`${pad}${yamlScalar(value)}`)
}

export function jsonToYaml(text: string): string {
  const value = parseJson(text)
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    const lines: string[] = []
    emitYaml(value, 0, lines)
    return lines.join('\n')
  }
  return emitYamlRoot(value)
}

function emitYamlRoot(value: unknown): string {
  const lines: string[] = []
  emitYaml(value, 0, lines)
  return lines.join('\n')
}

/* ---------------- YAML → JSON（常用子集） ---------------- */

function stripComment(line: string): string {
  let inSingle = false
  let inDouble = false
  for (let i = 0; i < line.length; i += 1) {
    const char = line[i]
    if (char === "'" && !inDouble) inSingle = !inSingle
    else if (char === '"' && !inSingle) inDouble = !inDouble
    else if (char === '#' && !inSingle && !inDouble) {
      return line.slice(0, i)
    }
  }
  return line
}

function parseYamlScalar(raw: string): unknown {
  const text = raw.trim()
  if (text === '' || text === '~' || text === 'null') return null
  if (text === 'true') return true
  if (text === 'false') return false
  if (/^-?\d+$/.test(text)) return Number(text)
  if (/^-?\d*\.\d+$/.test(text)) return Number(text)
  if (text.startsWith("'") && text.endsWith("'") && text.length >= 2) {
    return text.slice(1, -1).replace(/''/g, "'")
  }
  if (text.startsWith('"') && text.endsWith('"') && text.length >= 2) {
    return text
      .slice(1, -1)
      .replace(/\\n/g, '\n')
      .replace(/\\t/g, '\t')
      .replace(/\\"/g, '"')
  }
  if (text.startsWith('[') || text.startsWith('{')) {
    try {
      return JSON.parse(text.replace(/'/g, '"'))
    } catch {
      return text
    }
  }
  return text
}

interface YamlLine {
  indent: number
  text: string
}

function yamlParse(lines: YamlLine[], start: number, indent: number): [unknown, number] {
  if (start >= lines.length) return [null, start]
  const isArray = lines[start].text.startsWith('- ') || lines[start].text === '-'
  if (isArray) {
    const result: unknown[] = []
    let index = start
    while (index < lines.length && lines[index].indent === indent && (lines[index].text.startsWith('- ') || lines[index].text === '-')) {
      const body = lines[index].text.slice(1).trim()
      if (!body) {
        const [nested, next] = yamlParse(lines, index + 1, nextIndent(lines, index + 1, indent))
        result.push(nested)
        index = next
        continue
      }
      const colon = body.search(/:(?:\s|$)/)
      if (colon > 0 && !body.startsWith('"') && !body.startsWith("'")) {
        const key = body.slice(0, colon).trim()
        const rest = body.slice(colon + 1).trim()
        const item: Record<string, unknown> = {}
        if (rest) {
          item[key] = parseYamlScalar(rest)
          index += 1
        } else {
          const [nested, next] = yamlParse(lines, index + 1, nextIndent(lines, index + 1, indent + 1))
          item[key] = nested
          index = next
        }
        while (index < lines.length && lines[index].indent > indent) {
          const line = lines[index]
          const split = line.text.search(/:(?:\s|$)/)
          if (split <= 0) break
          const k = line.text.slice(0, split).trim()
          const v = line.text.slice(split + 1).trim()
          if (v) {
            item[k] = parseYamlScalar(v)
            index += 1
          } else {
            const [nested, next] = yamlParse(lines, index + 1, nextIndent(lines, index + 1, line.indent + 1))
            item[k] = nested
            index = next
          }
        }
        result.push(item)
        continue
      }
      result.push(parseYamlScalar(body))
      index += 1
    }
    return [result, index]
  }

  const result: Record<string, unknown> = {}
  let index = start
  while (index < lines.length && lines[index].indent === indent) {
    const line = lines[index].text
    const colon = line.search(/:(?:\s|$)/)
    if (colon <= 0) {
      throw new Error(`无法解析第 ${index + 1} 行：${line}`)
    }
    const key = line.slice(0, colon).trim().replace(/^['"]|['"]$/g, '')
    const rest = line.slice(colon + 1).trim()
    if (rest) {
      result[key] = parseYamlScalar(rest)
      index += 1
    } else {
      const [nested, next] = yamlParse(lines, index + 1, nextIndent(lines, index + 1, indent + 1))
      result[key] = nested
      index = next
    }
  }
  return [result, index]
}

function nextIndent(lines: YamlLine[], from: number, fallback: number): number {
  return from < lines.length ? lines[from].indent : fallback
}

export function yamlToJson(text: string): string {
  if (!text.trim()) throw new Error('请输入 YAML 内容')
  const lines: YamlLine[] = text
    .split(/\r?\n/)
    .map((raw) => stripComment(raw).replace(/\t/g, '  '))
    .filter((line) => line.trim() !== '' && line.trim() !== '---')
    .map((line) => ({
      indent: line.match(/^\s*/)?.[0].length ?? 0,
      text: line.trim(),
    }))
  if (!lines.length) return '{}'
  const [value] = yamlParse(lines, 0, lines[0].indent)
  return stringifyJson(value, 2)
}

/* ---------------- JSON ↔ XML ---------------- */

function xmlTagName(key: string): string {
  const clean = key.replace(/[^\w.-]/g, '_')
  return /^[A-Za-z_]/.test(clean) ? clean : `item_${clean}`
}

function escapeXml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function emitXml(key: string, value: unknown, indent: number, lines: string[]): void {
  const pad = '  '.repeat(indent)
  const tag = xmlTagName(key)
  if (value === null || value === undefined) {
    lines.push(`${pad}<${tag} />`)
    return
  }
  if (Array.isArray(value)) {
    if (!value.length) {
      lines.push(`${pad}<${tag} />`)
      return
    }
    value.forEach((item) => emitXml(key, item, indent, lines))
    return
  }
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    if (!entries.length) {
      lines.push(`${pad}<${tag} />`)
      return
    }
    lines.push(`${pad}<${tag}>`)
    entries.forEach(([childKey, childValue]) => {
      if (childKey.startsWith('@')) {
        return
      }
      emitXml(childKey, childValue, indent + 1, lines)
    })
    // 属性统一放在结束前处理
    const attrs = entries
      .filter(([childKey]) => childKey.startsWith('@'))
      .map(([childKey, childValue]) => ` ${xmlTagName(childKey.slice(1))}="${escapeXml(String(childValue))}"`)
      .join('')
    if (attrs) {
      const last = lines.pop() ?? ''
      lines.push(last.replace(`<${tag}>`, `<${tag}${attrs}>`))
    }
    lines.push(`${pad}</${tag}>`)
    return
  }
  lines.push(`${pad}<${tag}>${escapeXml(String(value))}</${tag}>`)
}

export function jsonToXml(text: string, rootName = 'root'): string {
  const value = parseJson(text)
  const lines = ['<?xml version="1.0" encoding="UTF-8"?>']
  emitXml(rootName, value, 0, lines)
  return lines.join('\n')
}

function elementToValue(element: Element): unknown {
  const children = Array.from(element.children)
  const text = element.textContent?.trim() ?? ''
  if (!children.length) {
    return text
  }
  const result: Record<string, unknown> = {}
  if (text) result['#text'] = text
  Array.from(element.attributes).forEach((attr) => {
    result[`@${attr.name}`] = attr.value
  })
  children.forEach((child) => {
    const name = child.tagName
    const value = elementToValue(child)
    if (name in result) {
      const current = result[name]
      if (Array.isArray(current)) current.push(value)
      else result[name] = [current, value]
    } else {
      result[name] = value
    }
  })
  return result
}

export function xmlToJson(text: string): string {
  if (!text.trim()) throw new Error('请输入 XML 内容')
  const doc = new DOMParser().parseFromString(text, 'application/xml')
  const error = doc.querySelector('parsererror')
  if (error) throw new Error(`XML 解析失败：${error.textContent?.trim().slice(0, 200) ?? '格式错误'}`)
  const root = doc.documentElement
  const value = { [root.tagName]: elementToValue(root) }
  return stringifyJson(value, 2)
}

/* ---------------- JSON ↔ CSV ---------------- */

function parseCsv(text: string, delimiter: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let inQuotes = false
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i]
    if (inQuotes) {
      if (char === '"') {
        if (text[i + 1] === '"') {
          field += '"'
          i += 1
        } else {
          inQuotes = false
        }
      } else {
        field += char
      }
      continue
    }
    if (char === '"') {
      inQuotes = true
    } else if (char === delimiter) {
      row.push(field)
      field = ''
    } else if (char === '\n') {
      row.push(field)
      rows.push(row)
      row = []
      field = ''
    } else if (char === '\r') {
      continue
    } else {
      field += char
    }
  }
  if (field !== '' || row.length) {
    row.push(field)
    rows.push(row)
  }
  return rows.filter((item) => item.some((cell) => cell !== ''))
}

export function csvToJson(text: string, delimiter = ','): string {
  const rows = parseCsv(text.trim(), delimiter)
  if (!rows.length) throw new Error('CSV 内容为空')
  const header = rows[0].map((cell, index) => cell.trim() || `column_${index + 1}`)
  const list = rows.slice(1).map((row) => {
    const item: Record<string, string> = {}
    header.forEach((key, index) => {
      item[key] = row[index] ?? ''
    })
    return item
  })
  return stringifyJson(list, 2)
}

export function jsonToCsv(text: string, delimiter = ','): string {
  const value = parseJson(text)
  if (!Array.isArray(value)) throw new Error('需要 JSON 数组（每项为对象）才能转换为 CSV')
  const rows = value.filter((item): item is Record<string, unknown> => item !== null && typeof item === 'object')
  if (!rows.length) throw new Error('数组中缺少对象数据')
  const header: string[] = []
  rows.forEach((row) => {
    Object.keys(row).forEach((key) => {
      if (!header.includes(key)) header.push(key)
    })
  })
  const escapeCell = (cell: unknown): string => {
    const text = cell === null || cell === undefined ? '' : String(cell)
    return /["\n\r,\t]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
  }
  const lines = [header.map(escapeCell).join(delimiter)]
  rows.forEach((row) => {
    lines.push(
      header
        .map((key) => {
          const cell = row[key]
          if (cell !== null && typeof cell === 'object') return escapeCell(JSON.stringify(cell))
          return escapeCell(cell)
        })
        .join(delimiter),
    )
  })
  return lines.join('\n')
}
