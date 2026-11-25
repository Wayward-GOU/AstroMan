/**
 * urlencoded（application/x-www-form-urlencoded）正文的解析与编码。
 *
 * 解析侧刻意比规范宽松，因为用户经常把参数列表从别处整段粘进来：
 *
 * - 换行也当分隔符：`a=1\nb=2`（从文档 / Excel 复制的多行参数）
 * - `key: value` 行式：DevTools 的 Form Data 面板、接口文档常用这种写法
 * - 前导 `?`：直接粘贴整段 query string
 * - 全角 `＆` `＝` `：`：从微信 / Word 复制后出现
 *
 * 编码侧保持标准（`&` 分隔、`encodeURIComponent`），`{{变量}}` 原样保留，
 * 避免被编码后发送时无法替换。
 */

export interface FormPair {
  key: string
  value: string
}

const SEPARATOR = /[&\n\r]|＆/

/** `%XX` 与 `+`（表单语义的空格）；非法百分号序列按原文保留 */
export function decodeFormPart(part: string): string {
  try {
    return decodeURIComponent(part.replace(/\+/g, ' '))
  } catch {
    return part
  }
}

export function encodeFormPart(part: string): string {
  // {{变量}} 保持原样，避免被编码后无法在发送时替换
  return part
    .split(/(\{\{[^{}]*\}\})/)
    .map((segment) => (/^\{\{[^{}]*\}\}$/.test(segment) ? segment : encodeURIComponent(segment)))
    .join('')
}

/** 把正文按分隔符拆成键值对；解析不出的段落按「无值」保留，不丢用户输入 */
export function parseFormBody(body: string): FormPair[] {
  let text = (body ?? '').trim()
  if (text.startsWith('?')) text = text.slice(1)

  const pairs: FormPair[] = []
  for (const raw of text.split(SEPARATOR)) {
    const chunk = raw.trim()
    if (!chunk) continue

    const equals = firstIndex(chunk, ['=', '＝'])
    if (equals >= 0) {
      pairs.push({
        key: decodeFormPart(chunk.slice(0, equals)).trim(),
        value: decodeFormPart(chunk.slice(equals + 1)),
      })
      continue
    }

    const colon = firstIndex(chunk, [':', '：'])
    if (colon >= 0) {
      pairs.push({
        key: decodeFormPart(chunk.slice(0, colon)).trim(),
        value: decodeFormPart(chunk.slice(colon + 1)).trim(),
      })
      continue
    }

    pairs.push({ key: decodeFormPart(chunk).trim(), value: '' })
  }
  return pairs
}

function firstIndex(text: string, chars: string[]): number {
  let best = -1
  for (const ch of chars) {
    const index = text.indexOf(ch)
    if (index >= 0 && (best < 0 || index < best)) best = index
  }
  return best
}
