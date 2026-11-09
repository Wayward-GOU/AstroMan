/**
 * JSON / 纯文本着色与搜索标记。
 *
 * 输出是已经转义的安全 HTML，可直接交给 v-html：
 * - 语法片段带 tok-* 类名，字段名与字符串同时带 data-json-token（供双击查同名字段）
 * - 搜索命中包 <mark data-match-index>，当前命中额外带 hl-current
 */

export interface TextMatch {
  start: number
  end: number
}

export interface HighlightOptions {
  query?: string
  currentMatch?: number
  caseSensitive?: boolean
}

interface Token {
  start: number
  end: number
  cls: string | null
}

export function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function escapeAttr(text: string): string {
  return escapeHtml(text).replace(/"/g, '&quot;')
}

/** 返回不重叠的命中区间；默认忽略大小写 */
export function findMatches(text: string, query: string, caseSensitive = false): TextMatch[] {
  if (!text || !query) return []
  const haystack = caseSensitive ? text : text.toLowerCase()
  const needle = caseSensitive ? query : query.toLowerCase()
  if (!needle) return []
  const out: TextMatch[] = []
  let from = 0
  while (from <= haystack.length - needle.length) {
    const index = haystack.indexOf(needle, from)
    if (index === -1) break
    out.push({ start: index, end: index + needle.length })
    from = index + needle.length
  }
  return out
}

/** 双击字段名 / 字符串时取搜索词：去掉两侧引号，保留内部转义 */
export function tokenSearchText(raw: string): string {
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) return raw.slice(1, -1)
  return raw
}

function isDigit(text: string, index: number) {
  const code = text.charCodeAt(index)
  return index < text.length && code >= 48 && code <= 57
}

/** 把文本切成覆盖全文的片段，cls 为 null 表示普通文本 */
function tokenizeJson(text: string): Token[] {
  const tokens: Token[] = []
  const length = text.length
  let index = 0
  let plainStart = 0

  const push = (cls: string | null, start: number, end: number) => {
    if (end <= start) return
    if (start > plainStart) tokens.push({ start: plainStart, end: start, cls: null })
    tokens.push({ start, end, cls })
    plainStart = end
  }

  while (index < length) {
    const code = text.charCodeAt(index)

    // 双引号字符串 / 字段名
    if (code === 34) {
      let end = index + 1
      while (end < length) {
        const next = text.charCodeAt(end)
        if (next === 92) {
          end += 2
          continue
        }
        if (next === 34) {
          end += 1
          break
        }
        end += 1
      }
      let probe = end
      while (probe < length) {
        const next = text.charCodeAt(probe)
        if (next === 32 || next === 9 || next === 10 || next === 13) {
          probe += 1
          continue
        }
        break
      }
      const isKey = probe < length && text.charCodeAt(probe) === 58
      const cls = isKey ? 'tok-key' : 'tok-string'

      // 字符串里的 {{变量}} 单独着色，便于一眼看出未渲染的变量
      let cursor = index
      let varIndex = text.indexOf('{{', index)
      while (varIndex !== -1 && varIndex < end) {
        const varEnd = text.indexOf('}}', varIndex + 2)
        if (varEnd === -1 || varEnd + 2 > end) break
        push(cls, cursor, varIndex)
        push('tok-var', varIndex, varEnd + 2)
        cursor = varEnd + 2
        varIndex = text.indexOf('{{', cursor)
      }
      push(cls, cursor, end)
      index = end
      continue
    }

    // 行注释
    if (code === 47 && text.charCodeAt(index + 1) === 47) {
      let end = text.indexOf('\n', index)
      if (end === -1) end = length
      push('tok-comment', index, end)
      index = end
      continue
    }

    // 块注释
    if (code === 47 && text.charCodeAt(index + 1) === 42) {
      const close = text.indexOf('*/', index + 2)
      const end = close === -1 ? length : close + 2
      push('tok-comment', index, end)
      index = end
      continue
    }

    // {{变量}}
    if (code === 123 && text.charCodeAt(index + 1) === 123) {
      const close = text.indexOf('}}', index + 2)
      if (close !== -1) {
        push('tok-var', index, close + 2)
        index = close + 2
        continue
      }
    }

    // 数字（含小数与科学计数法）
    if (code === 45 || (code >= 48 && code <= 57)) {
      let end = index
      if (text.charCodeAt(end) === 45) end += 1
      const digitsStart = end
      while (isDigit(text, end)) end += 1
      if (end > digitsStart) {
        if (text.charCodeAt(end) === 46 && isDigit(text, end + 1)) {
          end += 1
          while (isDigit(text, end)) end += 1
        }
        const exponent = text.charCodeAt(end)
        if (exponent === 101 || exponent === 69) {
          let expEnd = end + 1
          const sign = text.charCodeAt(expEnd)
          if (sign === 43 || sign === 45) expEnd += 1
          const expDigits = expEnd
          while (isDigit(text, expEnd)) expEnd += 1
          if (expEnd > expDigits) end = expEnd
        }
        push('tok-number', index, end)
        index = end
        continue
      }
    }

    // 标点
    if (
      code === 123 ||
      code === 125 ||
      code === 91 ||
      code === 93 ||
      code === 44 ||
      code === 58
    ) {
      push('tok-punct', index, index + 1)
      index += 1
      continue
    }

    index += 1
  }

  if (plainStart < length) tokens.push({ start: plainStart, end: length, cls: null })
  return tokens
}

/** 渲染片段，并只对命中区间包 mark；跨片段的命中会在每个片段里各包一次 */
function renderTokens(
  text: string,
  tokens: Token[],
  matches: TextMatch[],
  currentMatch: number,
): string {
  const parts: string[] = []
  let markIndex = 0

  for (const token of tokens) {
    while (markIndex < matches.length && matches[markIndex].end <= token.start) markIndex += 1

    let cursor = token.start
    let probe = markIndex
    const inner: string[] = []

    while (probe < matches.length && matches[probe].start < token.end) {
      const match = matches[probe]
      const start = Math.max(match.start, token.start)
      const end = Math.min(match.end, token.end)
      if (end > start) {
        if (start > cursor) inner.push(escapeHtml(text.slice(cursor, start)))
        const cls = probe === currentMatch ? 'hl-match hl-current' : 'hl-match'
        inner.push(
          `<mark class="${cls}" data-match-index="${probe}">${escapeHtml(text.slice(start, end))}</mark>`,
        )
        cursor = end
      }
      probe += 1
    }

    if (cursor < token.end) inner.push(escapeHtml(text.slice(cursor, token.end)))

    const html = inner.join('')
    if (html) {
      if (token.cls) {
        const raw = text.slice(token.start, token.end)
        let attrs = ''
        if (token.cls === 'tok-key' || token.cls === 'tok-string') {
          const kind = token.cls === 'tok-key' ? 'key' : 'string'
          attrs = ` data-json-token="${escapeAttr(tokenSearchText(raw))}" data-json-kind="${kind}"`
        }
        parts.push(`<span class="${token.cls}"${attrs}>${html}</span>`)
      } else {
        parts.push(html)
      }
    }

    // 跨片段的命中要留到下一个片段继续标记
    markIndex = probe
    if (probe > 0 && matches[probe - 1].end > token.end) markIndex = probe - 1
  }

  return parts.join('')
}

export function highlightJson(text: string, options: HighlightOptions = {}): string {
  if (!text) return ''
  const matches = options.query ? findMatches(text, options.query, options.caseSensitive) : []
  return renderTokens(text, tokenizeJson(text), matches, options.currentMatch ?? -1)
}

export function highlightPlain(text: string, options: HighlightOptions = {}): string {
  if (!text) return ''
  const matches = options.query ? findMatches(text, options.query, options.caseSensitive) : []
  return renderTokens(
    text,
    [{ start: 0, end: text.length, cls: null }],
    matches,
    options.currentMatch ?? -1,
  )
}
