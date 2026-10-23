export type SqlMode = 'literal' | 'placeholder'

export interface SqlParam {
  index: number
  name: string
  value: unknown
  jdbcType: string
  missing: boolean
}

export interface MybatisResult {
  sql: string
  params: SqlParam[]
  warnings: string[]
}

type TextNode = { kind: 'text'; text: string }
type TagNode = { kind: 'tag'; name: string; attrs: Record<string, string>; children: XmlNode[] }
type XmlNode = TextNode | TagNode
type Ctx = Record<string, unknown>

const STATEMENT_TAGS = ['select', 'insert', 'update', 'delete']

function decodeEntities(text: string): string {
  return text
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, '&')
}

function parseAttrs(source: string): Record<string, string> {
  const attrs: Record<string, string> = {}
  const re = /([A-Za-z_][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
  let match: RegExpExecArray | null
  while ((match = re.exec(source))) {
    attrs[match[1]] = decodeEntities(match[2] ?? match[3] ?? '')
  }
  return attrs
}

function parseXml(source: string): XmlNode[] {
  const root: XmlNode[] = []
  const stack: { node: TagNode }[] = []
  const push = (node: XmlNode) => {
    if (stack.length) stack[stack.length - 1].node.children.push(node)
    else root.push(node)
  }
  let i = 0
  while (i < source.length) {
    if (source.startsWith('<!--', i)) {
      const end = source.indexOf('-->', i + 4)
      i = end === -1 ? source.length : end + 3
      continue
    }
    if (source.startsWith('<![CDATA[', i)) {
      const end = source.indexOf(']]>', i + 9)
      push({ kind: 'text', text: source.slice(i + 9, end === -1 ? source.length : end) })
      i = end === -1 ? source.length : end + 3
      continue
    }
    if (source.startsWith('<?', i)) {
      const end = source.indexOf('?>', i + 2)
      i = end === -1 ? source.length : end + 2
      continue
    }
    if (source.startsWith('<!', i)) {
      const end = source.indexOf('>', i + 2)
      i = end === -1 ? source.length : end + 1
      continue
    }
    if (source[i] === '<') {
      let j = i + 1
      let quote: string | null = null
      while (j < source.length) {
        const ch = source[j]
        if (quote) {
          if (ch === quote) quote = null
        } else if (ch === '"' || ch === "'") {
          quote = ch
        } else if (ch === '>') {
          break
        }
        j += 1
      }
      const raw = source.slice(i + 1, j)
      i = j + 1
      if (raw.trimStart().startsWith('/')) {
        const name = raw.trim().slice(1).trim()
        for (let k = stack.length - 1; k >= 0; k -= 1) {
          if (stack[k].node.name === name) {
            stack.length = k
            break
          }
        }
        continue
      }
      const selfClosing = raw.trimEnd().endsWith('/')
      const body = selfClosing ? raw.trimEnd().slice(0, -1) : raw
      const nameMatch = /^([A-Za-z_][\w:.-]*)/.exec(body.trim())
      if (!nameMatch) {
        push({ kind: 'text', text: `<${raw}>` })
        continue
      }
      const name = nameMatch[1]
      const node: TagNode = {
        kind: 'tag',
        name,
        attrs: parseAttrs(body.slice(body.indexOf(name) + name.length)),
        children: [],
      }
      push(node)
      if (!selfClosing) stack.push({ node })
      continue
    }
    let next = source.indexOf('<', i)
    if (next === -1) next = source.length
    push({ kind: 'text', text: source.slice(i, next) })
    i = next
  }
  return root
}

type Token = { type: 'ident' | 'number' | 'string' | 'op'; value: string }

function tokenizeExpr(expr: string): Token[] {
  const tokens: Token[] = []
  let i = 0
  while (i < expr.length) {
    const ch = expr[i]
    if (/\s/.test(ch)) {
      i += 1
      continue
    }
    if (ch === "'" || ch === '"') {
      let j = i + 1
      let value = ''
      while (j < expr.length) {
        if (expr[j] === '\\' && j + 1 < expr.length) {
          value += expr[j + 1]
          j += 2
          continue
        }
        if (expr[j] === ch) {
          j += 1
          break
        }
        value += expr[j]
        j += 1
      }
      tokens.push({ type: 'string', value })
      i = j
      continue
    }
    if (/[0-9]/.test(ch)) {
      let j = i + 1
      while (j < expr.length && /[0-9.]/.test(expr[j])) j += 1
      tokens.push({ type: 'number', value: expr.slice(i, j) })
      i = j
      continue
    }
    if ('()[],'.includes(ch)) {
      tokens.push({ type: 'op', value: ch })
      i += 1
      continue
    }
    if ('!<>=&|'.includes(ch)) {
      const two = expr.slice(i, i + 2)
      if (['==', '!=', '>=', '<=', '&&', '||'].includes(two)) {
        tokens.push({ type: 'op', value: two })
        i += 2
        continue
      }
      tokens.push({ type: 'op', value: ch })
      i += 1
      continue
    }
    if ('+-*/'.includes(ch)) {
      tokens.push({ type: 'op', value: ch })
      i += 1
      continue
    }
    if (/[A-Za-z_$]/.test(ch)) {
      let j = i + 1
      while (j < expr.length && /[\w$.-]/.test(expr[j])) j += 1
      tokens.push({ type: 'ident', value: expr.slice(i, j) })
      i = j
      continue
    }
    i += 1
  }
  return tokens
}

function resolvePath(path: string, ctx: Ctx): { value: unknown; found: boolean } {
  const segments = path.match(/[A-Za-z_$][\w$]*|\[[^\]]*\]/g) ?? []
  if (!segments.length) return { value: undefined, found: false }
  let current: unknown = ctx
  for (const raw of segments) {
    const lower = raw.toLowerCase()
    if (raw.startsWith('[')) {
      const inner = raw.slice(1, -1).trim().replace(/^['"]|['"]$/g, '')
      if (current === null || current === undefined) return { value: undefined, found: false }
      if (Array.isArray(current)) {
        current = current[Number(inner)]
      } else if (typeof current === 'object') {
        current = (current as Record<string, unknown>)[inner]
      } else {
        return { value: undefined, found: false }
      }
      continue
    }
    if (current === null || current === undefined) return { value: undefined, found: false }
    if (typeof current === 'object' && !Array.isArray(current)) {
      const obj = current as Record<string, unknown>
      if (current === ctx) {
        if (!Object.prototype.hasOwnProperty.call(obj, raw)) {
          return { value: undefined, found: false }
        }
        current = obj[raw]
        continue
      }
      if (lower === 'size' || lower === 'length') {
        current = Object.keys(obj).length
        continue
      }
      if (lower === 'isempty') {
        current = Object.keys(obj).length === 0
        continue
      }
      if (!Object.prototype.hasOwnProperty.call(obj, raw)) return { value: undefined, found: false }
      current = obj[raw]
      continue
    }
    if (Array.isArray(current) || typeof current === 'string') {
      if (lower === 'size' || lower === 'length') {
        current = current.length
        continue
      }
      if (lower === 'isempty') {
        current = current.length === 0
        continue
      }
      return { value: undefined, found: false }
    }
    return { value: undefined, found: false }
  }
  return { value: current, found: true }
}

function truthy(value: unknown): boolean {
  if (Array.isArray(value)) return value.length > 0
  if (typeof value === 'string') return value.length > 0
  return Boolean(value)
}

function looseEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === null || a === undefined || b === null || b === undefined) return false
  if (typeof a === 'number' || typeof b === 'number') return Number(a) === Number(b)
  return String(a) === String(b)
}

function compareValues(a: unknown, b: unknown, op: string): boolean {
  switch (op) {
    case '==':
    case '=':
      return looseEqual(a, b)
    case '!=':
      return !looseEqual(a, b)
    default:
      break
  }
  const left = typeof a === 'number' || typeof b === 'number' ? Number(a) : String(a)
  const right = typeof a === 'number' || typeof b === 'number' ? Number(b) : String(b)
  if (op === '>') return left > right
  if (op === '<') return left < right
  if (op === '>=') return left >= right
  if (op === '<=') return left <= right
  return false
}

export function evalExpression(expr: string, ctx: Ctx): unknown {
  const tokens = tokenizeExpr(expr)
  let pos = 0

  const peek = (): Token | undefined => tokens[pos]
  const isOp = (value: string): boolean => {
    const token = peek()
    return !!token && token.type === 'op' && token.value === value
  }
  const isWord = (value: string): boolean => {
    const token = peek()
    return !!token && token.type === 'ident' && token.value.toLowerCase() === value
  }

  const parseOr = (): unknown => {
    let value = parseAnd()
    while (isWord('or') || isOp('||')) {
      pos += 1
      const right = parseAnd()
      value = truthy(value) || truthy(right)
    }
    return value
  }

  const parseAnd = (): unknown => {
    let value = parseUnary()
    while (isWord('and') || isOp('&&')) {
      pos += 1
      const right = parseUnary()
      value = truthy(value) && truthy(right)
    }
    return value
  }

  const parseUnary = (): unknown => {
    if (isWord('not') || isOp('!')) {
      pos += 1
      return !truthy(parseUnary())
    }
    return parseComparison()
  }

  const parseComparison = (): unknown => {
    const left = parseAdditive()
    const token = peek()
    if (token && token.type === 'op' && ['==', '!=', '>', '<', '>=', '<='].includes(token.value)) {
      pos += 1
      const right = parseAdditive()
      return compareValues(left, right, token.value)
    }
    return left
  }

  const parseAdditive = (): unknown => {
    let value = parsePrimary()
    while (isOp('+') || isOp('-')) {
      const op = tokens[pos].value
      pos += 1
      const right = parsePrimary()
      if (op === '+') {
        if (typeof value === 'string' || typeof right === 'string') {
          value = `${value ?? ''}${right ?? ''}`
        } else {
          value = Number(value) + Number(right)
        }
      } else {
        value = Number(value) - Number(right)
      }
    }
    return value
  }

  const parsePrimary = (): unknown => {
    const token = peek()
    if (!token) return undefined
    if (token.type === 'op' && token.value === '(') {
      pos += 1
      const value = parseOr()
      if (isOp(')')) pos += 1
      return value
    }
    if (token.type === 'string') {
      pos += 1
      return token.value
    }
    if (token.type === 'number') {
      pos += 1
      return Number(token.value)
    }
    if (token.type === 'ident') {
      pos += 1
      const lower = token.value.toLowerCase()
      if (lower === 'null') return null
      if (lower === 'true') return true
      if (lower === 'false') return false
      if (lower === 'empty') return undefined
      if (token.value.startsWith('@')) return undefined
      let value: unknown = resolvePath(token.value, ctx).value
      while (isOp('[')) {
        pos += 1
        const index = parseOr()
        if (isOp(']')) pos += 1
        if (value === null || value === undefined) {
          value = undefined
        } else if (Array.isArray(value)) {
          value = value[Number(index)]
        } else if (typeof value === 'object') {
          value = (value as Record<string, unknown>)[String(index)]
        } else {
          value = undefined
        }
      }
      if (isOp('(')) {
        pos += 1
        let args: unknown[] = []
        if (!isOp(')')) {
          args = [parseOr()]
          while (isOp(',')) {
            pos += 1
            args.push(parseOr())
          }
        }
        if (isOp(')')) pos += 1
        if (typeof value === 'function') {
          value = (value as (...params: unknown[]) => unknown)(...args)
        }
      }
      return value
    }
    pos += 1
    return undefined
  }

  return parseOr()
}

function sqlLiteral(value: unknown): string {
  if (value === null || value === undefined) return 'NULL'
  if (typeof value === 'string') return `'${value.replace(/'/g, "''")}'`
  if (typeof value === 'number') return Number.isFinite(value) ? String(value) : 'NULL'
  if (typeof value === 'boolean') return value ? '1' : '0'
  if (Array.isArray(value)) return value.map(sqlLiteral).join(', ')
  if (typeof value === 'object') return `'${JSON.stringify(value).replace(/'/g, "''")}'`
  return `'${String(value)}'`
}

function trimLeadingConnector(text: string): string {
  let out = text.trim()
  while (/^(and|or)\b/i.test(out)) {
    out = out.replace(/^(and|or)\b/i, '').trim()
  }
  return out
}

function stripOverride(content: string, overrides: string): string {
  let out = content.trim()
  const list = overrides
    .split('|')
    .map((item) => item.trim())
    .filter(Boolean)
  let changed = true
  while (changed) {
    changed = false
    for (const item of list) {
      if (out.toUpperCase().startsWith(item.toUpperCase())) {
        out = out.slice(item.length).trim()
        changed = true
      }
    }
  }
  return out
}

/** 合并空白，但保留字符串字面量内部内容 */
function tidy(sql: string): string {
  let out = ''
  let quote: string | null = null
  let pendingSpace = false
  let i = 0
  while (i < sql.length) {
    const ch = sql[i]
    if (quote) {
      out += ch
      if (ch === quote) {
        if (ch === "'" && sql[i + 1] === "'") {
          out += sql[i + 1]
          i += 2
          continue
        }
        quote = null
      }
      i += 1
      continue
    }
    if (ch === "'" || ch === '"' || ch === '`') {
      if (pendingSpace && out && !out.endsWith(' ')) out += ' '
      pendingSpace = false
      quote = ch
      out += ch
      i += 1
      continue
    }
    if (/\s/.test(ch)) {
      pendingSpace = true
      i += 1
      continue
    }
    if (pendingSpace) {
      if (out && !out.endsWith(' ')) out += ' '
      pendingSpace = false
    }
    out += ch
    i += 1
  }
  return out.trim()
}

const SQL_KEYWORDS = [
  'LEFT OUTER JOIN',
  'RIGHT OUTER JOIN',
  'FULL OUTER JOIN',
  'INSERT INTO',
  'DELETE FROM',
  'GROUP BY',
  'ORDER BY',
  'UNION ALL',
  'LEFT JOIN',
  'RIGHT JOIN',
  'INNER JOIN',
  'SELECT',
  'FROM',
  'WHERE',
  'HAVING',
  'LIMIT',
  'VALUES',
  'UPDATE',
  'SET',
  'JOIN',
  'UNION',
  'OFFSET',
]

function isWordChar(ch: string | undefined): boolean {
  return !!ch && /[A-Za-z0-9_.$]/.test(ch)
}

function formatSql(sql: string): string {
  const upper = sql.toUpperCase()
  let out = ''
  let quote: string | null = null
  let i = 0
  while (i < sql.length) {
    const ch = sql[i]
    if (quote) {
      out += ch
      if (ch === quote) {
        if (ch === "'" && sql[i + 1] === "'") {
          out += sql[i + 1]
          i += 2
          continue
        }
        quote = null
      }
      i += 1
      continue
    }
    if (ch === "'" || ch === '"' || ch === '`') {
      quote = ch
      out += ch
      i += 1
      continue
    }
    let matched = false
    if (!isWordChar(sql[i - 1])) {
      for (const keyword of SQL_KEYWORDS) {
        if (!upper.startsWith(keyword, i)) continue
        const after = sql[i + keyword.length]
        if (isWordChar(after)) continue
        if (out.trim()) out = `${out.trimEnd()}\n${sql.slice(i, i + keyword.length)}`
        else out += sql.slice(i, i + keyword.length)
        i += keyword.length
        matched = true
        break
      }
    }
    if (!matched) {
      out += ch
      i += 1
    }
  }
  return out
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line, index, lines) => line.length > 0 || index === lines.length - 1)
    .join('\n')
}

export function renderMybatis(
  template: string,
  rawParams: unknown,
  mode: SqlMode,
  pretty: boolean,
): MybatisResult {
  const warnings = new Set<string>()
  const params: SqlParam[] = []

  const vars: Ctx = {}
  if (Array.isArray(rawParams)) {
    vars.list = rawParams
    vars.collection = rawParams
    vars.array = rawParams
  } else if (rawParams && typeof rawParams === 'object') {
    Object.assign(vars, rawParams as Record<string, unknown>)
  } else if (rawParams !== undefined && rawParams !== null) {
    vars.value = rawParams
    vars.list = rawParams
  }

  const bindParam = (expr: string, ctx: Ctx): string => {
    const parts = expr.split(',')
    const name = parts[0].trim()
    let jdbcType = ''
    for (const part of parts.slice(1)) {
      const match = /jdbcType\s*=\s*([A-Za-z0-9_]+)/i.exec(part)
      if (match) jdbcType = match[1].toUpperCase()
    }
    const { value, found } = resolvePath(name, ctx)
    if (!found) warnings.add(`缺少参数：${name}`)
    params.push({
      index: params.length + 1,
      name,
      value,
      jdbcType,
      missing: !found,
    })
    return mode === 'placeholder' ? '?' : sqlLiteral(found ? value : null)
  }

  const bindRaw = (expr: string, ctx: Ctx): string => {
    const trimmed = expr.trim()
    const path = resolvePath(trimmed, ctx)
    let value: unknown = path.value
    if (!path.found) {
      try {
        value = evalExpression(trimmed, ctx)
      } catch {
        warnings.add(`无法解析 \${${trimmed}}`)
        return ''
      }
    }
    if (value === null || value === undefined) return ''
    if (typeof value === 'string') return value
    if (typeof value === 'object') return JSON.stringify(value)
    return String(value)
  }

  const applyParams = (text: string, ctx: Ctx): string => {
    const withHash = text.replace(/#\{\s*([^{}]+?)\s*\}/g, (_match, expr: string) =>
      bindParam(expr, ctx),
    )
    return withHash.replace(/\$\{\s*([^{}]+?)\s*\}/g, (_match, expr: string) => bindRaw(expr, ctx))
  }

  const renderNodes = (nodes: XmlNode[], ctx: Ctx): string =>
    nodes
      .map((node) => (node.kind === 'text' ? applyParams(decodeEntities(node.text), ctx) : renderTag(node, ctx)))
      .join('')

  const renderTag = (node: TagNode, ctx: Ctx): string => {
    const name = node.name.toLowerCase()
    const attrs = node.attrs
    switch (name) {
      case 'if': {
        let passed = false
        try {
          passed = truthy(evalExpression(attrs.test ?? '', ctx))
        } catch {
          warnings.add(`条件无法求值：${attrs.test ?? ''}`)
        }
        return passed ? renderNodes(node.children, ctx) : ''
      }
      case 'where': {
        const inner = trimLeadingConnector(renderNodes(node.children, ctx))
        return inner ? ` WHERE ${inner} ` : ''
      }
      case 'set': {
        const inner = renderNodes(node.children, ctx)
          .trim()
          .replace(/,\s*$/, '')
        return inner ? ` SET ${inner} ` : ''
      }
      case 'trim': {
        let inner = renderNodes(node.children, ctx).trim()
        inner = stripOverride(inner, attrs.prefixOverrides ?? '')
        inner = stripOverride(inner, attrs.suffixOverrides ?? '')
        if (!inner) return ''
        const prefix = attrs.prefix ?? ''
        const suffix = attrs.suffix ?? ''
        return `${prefix ? ` ${prefix} ` : ' '}${inner}${suffix ? ` ${suffix} ` : ' '}`
      }
      case 'foreach': {
        const collection = resolvePath(attrs.collection ?? '', ctx)
        const list = collection.value
        if (!collection.found || list === null || list === undefined) {
          warnings.add(`foreach 集合不存在：${attrs.collection ?? ''}`)
          return ''
        }
        const itemName = attrs.item ?? 'item'
        const indexName = attrs.index ?? 'index'
        const entries: { key: unknown; value: unknown }[] = Array.isArray(list)
          ? list.map((value, index) => ({ key: index, value }))
          : typeof list === 'object'
            ? Object.entries(list as Record<string, unknown>).map(([key, value]) => ({ key, value }))
            : [{ key: 0, value: list }]
        if (!entries.length) return ''
        const parts = entries.map((entry) =>
          renderNodes(node.children, { ...ctx, [itemName]: entry.value, [indexName]: entry.key }),
        )
        return `${attrs.open ?? ''}${parts.join(attrs.separator ?? '')}${attrs.close ?? ''}`
      }
      case 'choose': {
        for (const child of node.children) {
          if (child.kind !== 'tag') continue
          const childName = child.name.toLowerCase()
          if (childName === 'when') {
            let passed = false
            try {
              passed = truthy(evalExpression(child.attrs.test ?? '', ctx))
            } catch {
              warnings.add(`条件无法求值：${child.attrs.test ?? ''}`)
            }
            if (passed) return renderNodes(child.children, ctx)
          } else if (childName === 'otherwise') {
            return renderNodes(child.children, ctx)
          }
        }
        return ''
      }
      case 'bind': {
        const target = attrs.name ?? ''
        if (!target) return ''
        try {
          ctx[target] = evalExpression(attrs.value ?? '', ctx)
        } catch {
          warnings.add(`bind 表达式无法求值：${attrs.value ?? ''}`)
        }
        return ''
      }
      case 'include':
        warnings.add(`include 需要完整 mapper 才能解析，已跳过：${attrs.refid ?? ''}`)
        return ''
      case 'selectkey':
      case 'sql':
        return ''
      default:
        return renderNodes(node.children, ctx)
    }
  }

  let nodes = parseXml(template)
  for (let guard = 0; guard < 4; guard += 1) {
    const meaningful = nodes.filter((node) => node.kind === 'tag' || node.text.trim())
    if (meaningful.length !== 1 || meaningful[0].kind !== 'tag') break
    const name = meaningful[0].name.toLowerCase()
    if (name === 'mapper') {
      const statements = meaningful[0].children.filter(
        (node): node is TagNode =>
          node.kind === 'tag' && STATEMENT_TAGS.includes(node.name.toLowerCase()),
      )
      if (statements.length > 1) warnings.add('mapper 中包含多个语句，仅渲染第一个')
      if (!statements.length) break
      nodes = statements[0].children
      continue
    }
    if (STATEMENT_TAGS.includes(name) || name === 'sql') {
      nodes = meaningful[0].children
      continue
    }
    break
  }

  const raw = renderNodes(nodes, vars)
  const cleaned = tidy(raw)
  return {
    sql: pretty ? formatSql(cleaned) : cleaned,
    params,
    warnings: Array.from(warnings),
  }
}
