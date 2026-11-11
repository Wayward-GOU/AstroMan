import { findMatches } from './jsonHighlight'

export interface ReplaceResult {
  text: string
  count: number
}

/**
 * 纯文本批量替换：默认忽略大小写，替换内容按原样插入（不做正则解释）。
 * 用于请求 Body 的批量替换，避免用户输入 `$&`、`.` 等被当成正则语法。
 */
export function replaceAllLiteral(
  text: string,
  find: string,
  replacement: string,
  caseSensitive = false,
): ReplaceResult {
  if (!text || !find) return { text, count: 0 }
  const matches = findMatches(text, find, caseSensitive)
  if (!matches.length) return { text, count: 0 }

  const parts: string[] = []
  let cursor = 0
  for (const match of matches) {
    parts.push(text.slice(cursor, match.start))
    parts.push(replacement)
    cursor = match.end
  }
  parts.push(text.slice(cursor))
  return { text: parts.join(''), count: matches.length }
}
