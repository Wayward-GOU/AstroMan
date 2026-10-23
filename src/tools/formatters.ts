/* 轻量代码格式化与压缩：HTML / XML / CSS / JS / Java / C# / Go / PHP / C / SQL */

export type CodeLanguage =
  | 'html'
  | 'xml'
  | 'css'
  | 'js'
  | 'java'
  | 'csharp'
  | 'go'
  | 'php'
  | 'c'
  | 'sql'

export type FormatMode = 'pretty' | 'minify'

const VOID_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param',
  'source', 'track', 'wbr',
])

const RAW_TAGS = new Set(['pre', 'textarea', 'script', 'style'])
const INLINE_TAGS = new Set([
  'a', 'b', 'i', 'em', 'strong', 'span', 'code', 'small', 'sub', 'sup', 'label', 'abbr', 'cite',
])

function indentOf(size: number): string {
  return ' '.repeat(size)
}

/* ---------------- HTML / XML ---------------- */

function normalizeAngle(text: string): string {
  return text.replace(/>\s+</g, '><')
}

export function formatMarkup(text: string, mode: FormatMode, indentSize = 2): string {
  if (!text.trim()) return ''
  if (mode === 'minify') {
    return normalizeAngle(
      text
        .replace(/<!--[\s\S]*?-->/g, '')
        .replace(/\s{2,}/g, ' ')
        .replace(/>\s+</g, '><')
        .trim(),
    )
  }

  const tokens = text.split(/(<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<[^>]*>)/g)
  const lines: string[] = []
  let depth = 0
  let rawTag: string | null = null

  tokens.forEach((token) => {
    if (!token) return
    if (token.startsWith('<!--')) {
      lines.push(indentOf(depth * indentSize) + token.trim())
      return
    }
    if (token.startsWith('<!')) {
      lines.push(indentOf(depth * indentSize) + token.trim())
      return
    }
    if (token.startsWith('</')) {
      const name = token.replace(/^<\/\s*/, '').replace(/\s*>$/, '').toLowerCase()
      if (RAW_TAGS.has(name)) rawTag = null
      depth = Math.max(0, depth - 1)
      lines.push(indentOf(depth * indentSize) + token.trim())
      return
    }
    if (token.startsWith('<')) {
      const name = token.replace(/^<\s*/, '').replace(/\/?\s*>$.*/, '').split(/[\s/>]/)[0].toLowerCase()
      const selfClosing = /\/>$/.test(token.trim()) || VOID_TAGS.has(name)
      lines.push(indentOf(depth * indentSize) + token.trim())
      if (!selfClosing) {
        depth += 1
        if (RAW_TAGS.has(name)) rawTag = name
      }
      return
    }
    // 文本节点
    const content = token.replace(/\s+/g, ' ').trim()
    if (!content) return
    if (rawTag) {
      lines.push(content)
      return
    }
    const last = lines[lines.length - 1]
    if (
      last &&
      INLINE_TAGS.has(last.replace(/^<\s*/, '').split(/[\s>/]/)[0].toLowerCase()) &&
      !/<[a-z]/i.test(content)
    ) {
      lines[lines.length - 1] = `${last}${content}`
      return
    }
    lines.push(indentOf(depth * indentSize) + content)
  })

  return lines.join('\n')
}

/* ---------------- CSS ---------------- */

export function formatCss(text: string, mode: FormatMode, indentSize = 2): string {
  if (!text.trim()) return ''
  if (mode === 'minify') {
    return text
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .replace(/\s+/g, ' ')
      .replace(/\s*([{}:;,>])\s*/g, '$1')
      .replace(/;}/g, '}')
      .trim()
  }
  const tokens = text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split(/([{};])/)
    .map((token) => token.trim())
    .filter(Boolean)
  const lines: string[] = []
  let depth = 0
  let buffer = ''
  tokens.forEach((token) => {
    if (token === '{') {
      lines.push(`${indentOf(depth * indentSize)}${buffer.trim()} {`)
      buffer = ''
      depth += 1
      return
    }
    if (token === '}') {
      if (buffer.trim()) lines.push(`${indentOf(depth * indentSize)}${buffer.trim()};`)
      buffer = ''
      depth = Math.max(0, depth - 1)
      lines.push(`${indentOf(depth * indentSize)}}`)
      return
    }
    if (token === ';') {
      if (buffer.trim()) lines.push(`${indentOf(depth * indentSize)}${buffer.trim()};`)
      buffer = ''
      return
    }
    buffer += token
  })
  if (buffer.trim()) lines.push(`${indentOf(depth * indentSize)}${buffer.trim()}`)
  return lines.join('\n')
}

/* ---------------- JS 类代码 ---------------- */

function stripJsComments(text: string): string {
  let out = ''
  let quote: string | null = null
  let regex = false
  let prevSignificant = ''
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i]
    const next = text[i + 1]
    if (quote) {
      out += char
      if (char === '\\') {
        out += next ?? ''
        i += 1
      } else if (char === quote) {
        quote = null
      }
      continue
    }
    if (regex) {
      out += char
      if (char === '\\') {
        out += next ?? ''
        i += 1
      } else if (char === '/') {
        regex = false
      }
      continue
    }
    if (char === '"' || char === "'" || char === '`') {
      quote = char
      out += char
      continue
    }
    if (char === '/' && next === '/') {
      while (i < text.length && text[i] !== '\n') i += 1
      out += '\n'
      continue
    }
    if (char === '/' && next === '*') {
      i += 2
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i += 1
      i += 1
      out += ' '
      continue
    }
    if (char === '/' && /^[(,=:[!&|?{};+\-*%<>~^]$/.test(prevSignificant)) {
      regex = true
      out += char
      continue
    }
    if (!/\s/.test(char)) prevSignificant = char
    out += char
  }
  return out
}

export function formatCode(text: string, mode: FormatMode, indentSize = 2): string {
  if (!text.trim()) return ''
  const source = stripJsComments(text)
  if (mode === 'minify') {
    let out = ''
    let quote: string | null = null
    for (let i = 0; i < source.length; i += 1) {
      const char = source[i]
      if (quote) {
        out += char
        if (char === '\\') {
          out += source[i + 1] ?? ''
          i += 1
        } else if (char === quote) {
          quote = null
        }
        continue
      }
      if (char === '"' || char === "'" || char === '`') {
        quote = char
        out += char
        continue
      }
      if (/\s/.test(char)) {
        if (out && !/[\s(=,+\-*/{[]$/.test(out) && !/^[)\]},;]/.test(source.slice(i + 1).trimStart())) {
          out += ' '
        }
        continue
      }
      out += char
    }
    return out.replace(/\s+([)\]};,])/g, '$1').trim()
  }

  const lines: string[] = []
  let current = ''
  let depth = 0
  let quote: string | null = null
  let regex = false
  let prevSignificant = ''

  const flush = () => {
    if (current.trim()) lines.push(indentOf(depth * indentSize) + current.trim())
    current = ''
  }

  for (let i = 0; i < source.length; i += 1) {
    const char = source[i]
    if (quote) {
      current += char
      if (char === '\\') {
        current += source[i + 1] ?? ''
        i += 1
      } else if (char === quote) {
        quote = null
      }
      continue
    }
    if (regex) {
      current += char
      if (char === '\\') {
        current += source[i + 1] ?? ''
        i += 1
      } else if (char === '/') {
        regex = false
      }
      continue
    }
    if (char === '"' || char === "'" || char === '`') {
      quote = char
      current += char
      continue
    }
    if (char === '/' && /^[(,=:[!&|?{};+\-*%<>~^]$/.test(prevSignificant)) {
      regex = true
      current += char
      continue
    }
    if (/\s/.test(char)) {
      if (current && !current.endsWith(' ')) current += ' '
      continue
    }
    if (char === '{') {
      current = current.trimEnd()
      if (current) lines.push(indentOf(depth * indentSize) + current)
      lines.push(indentOf(depth * indentSize) + '{')
      depth += 1
      current = ''
      prevSignificant = char
      continue
    }
    if (char === '}') {
      flush()
      depth = Math.max(0, depth - 1)
      lines.push(indentOf(depth * indentSize) + '}')
      prevSignificant = char
      continue
    }
    if (char === ';') {
      current += ';'
      flush()
      prevSignificant = char
      continue
    }
    current += char
    if (!/\s/.test(char)) prevSignificant = char
  }
  flush()
  return lines.join('\n')
}

/* ---------------- SQL ---------------- */

const SQL_BREAK_KEYWORDS = [
  'SELECT', 'FROM', 'WHERE', 'GROUP BY', 'ORDER BY', 'HAVING', 'LIMIT', 'OFFSET',
  'INSERT INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE FROM', 'LEFT JOIN', 'RIGHT JOIN',
  'INNER JOIN', 'OUTER JOIN', 'FULL JOIN', 'CROSS JOIN', 'JOIN', 'UNION ALL', 'UNION',
  'ON', 'AND', 'OR',
]

export function formatSql(text: string, mode: FormatMode, indentSize = 2): string {
  if (!text.trim()) return ''
  if (mode === 'minify') {
    return text.trim().replace(/\s+/g, ' ')
  }

  // 保护字符串与反引号内容
  const slots: string[] = []
  const protectedText = text.replace(/('(?:[^'\\]|\\.)*'|`[^`]*`|"[^"]*")/g, (match) => {
    slots.push(match)
    return `\u0000${slots.length - 1}\u0000`
  })

  let upper = protectedText.replace(
    /\b(select|from|where|group\s+by|order\s+by|having|limit|offset|insert\s+into|values|update|set|delete\s+from|inner\s+join|left\s+join|right\s+join|full\s+join|cross\s+join|join|union\s+all|union|on|and|or|as|asc|desc|create\s+table|alter\s+table|drop\s+table|if\s+not\s+exists|primary\s+key|foreign\s+key|not\s+null|default|distinct|like|in|between|is\s+null|is\s+not\s+null)\b/gi,
    (match) => match.toUpperCase().replace(/\s+/g, ' '),
  )

  SQL_BREAK_KEYWORDS.forEach((keyword) => {
    const pattern = new RegExp(`\\s+(?=${keyword.replace(/\s+/g, '\\s+')}\\b)`, 'g')
    if (keyword === 'ON' || keyword === 'AND' || keyword === 'OR') {
      upper = upper.replace(pattern, `\n${indentOf(indentSize)}${keyword} `)
    } else {
      upper = upper.replace(pattern, `\n${keyword} `)
    }
  })

  return upper
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .join('\n')
    .replace(/\u0000(\d+)\u0000/g, (_m, index: string) => slots[Number(index)] ?? '')
}

/* ---------------- 统一入口 ---------------- */

export function formatCodeByLanguage(
  language: CodeLanguage,
  text: string,
  mode: FormatMode,
  indentSize = 2,
): string {
  if (!text.trim()) return ''
  switch (language) {
    case 'html':
    case 'xml':
      return formatMarkup(text, mode, indentSize)
    case 'css':
      return formatCss(text, mode, indentSize)
    case 'sql':
      return formatSql(text, mode, indentSize)
    default:
      return formatCode(text, mode, indentSize)
  }
}
