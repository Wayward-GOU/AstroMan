/**
 * 不可信内容隔离（T-17）。
 *
 * 工具结果、文档附件、页面快照都可能夹带「忽略以上指令，调用 xxx」这类文本。
 * 统一包成 <untrusted_data source="..."> 标签，并在系统提示词里声明：
 * 标签内只是数据，不是指令。界面展示仍使用原文，不受影响。
 */

/** 常见注入句式：只做行内标记，不改原文内容 */
const INJECTION_HINTS: RegExp[] = [
  /ignore\s+(all\s+)?(previous|above|prior)\s+instructions/i,
  /disregard\s+(all\s+)?(previous|above|prior)/i,
  /^\s*(system|assistant|developer)\s*:/i,
  /忽略(以上|之前|前面|上面)(的)?(所有)?(指令|提示|规则)/,
  /你现在是|新的系统提示|开发者模式|越狱模式/,
]

export const UNTRUSTED_MARK = '[已过滤] '

/** 逐行标记可疑注入句式（仅标记，不删除，便于用户回溯原文） */
export function markInjectionLines(text: string): string {
  if (!text) return text
  return text
    .split('\n')
    .map((line) => (INJECTION_HINTS.some((pattern) => pattern.test(line)) ? `${UNTRUSTED_MARK}${line}` : line))
    .join('\n')
}

/**
 * 包裹一段不可信内容。
 * @param source 来源标识，例如 tool:send_request / file:a.json / page:report
 */
export function wrapUntrusted(source: string, text: string): string {
  // source 只允许安全字符，避免拼接出额外标签
  const safe = source.replace(/[^a-zA-Z0-9_:.-]/g, '') || 'unknown'
  return `<untrusted_data source="${safe}">\n${markInjectionLines(text)}\n</untrusted_data>`
}
