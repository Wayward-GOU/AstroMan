/**
 * 自动补的 Content-Type 必须与后端 `http.rs::infer_content_type` 完全一致，
 * 否则「实际请求」页签展示的头会和真正发出去的不一样。
 *
 * Raw 类型按内容猜：JSON / XML / 纯文本。猜不中时用 text/plain。
 * 用户在 Headers 里手写过 Content-Type 时不会被覆盖（调用方先判断）。
 */

import { looksLikeXml } from './xmlFormat'

export function inferBodyContentType(bodyType: string, body: string): string {
  if (bodyType === 'json' || bodyType === 'graphql') return 'application/json'
  if (bodyType === 'form') return 'application/x-www-form-urlencoded'
  if (bodyType === 'binary') return 'application/octet-stream'

  const text = (body ?? '').trim()
  if (!text) return 'text/plain; charset=utf-8'
  if (looksLikeXml(text)) return 'application/xml; charset=utf-8'
  if (looksLikeJsonBody(text)) return 'application/json'
  return 'text/plain; charset=utf-8'
}

/** 只认严格 JSON：内容会原样发出，标成 application/json 却解析不了反而误导排查 */
function looksLikeJsonBody(text: string): boolean {
  const head = text[0]
  if (head !== '{' && head !== '[' && head !== '"') return false
  // 请求模板里的 {{变量}} 不是合法 JSON；换成 0 再判断，否则 Raw 写 JSON 模板会退成 text/plain
  const probe = text.replace(/\{\{[\s\S]*?\}\}/g, '0')
  try {
    JSON.parse(probe)
    return true
  } catch {
    return false
  }
}
