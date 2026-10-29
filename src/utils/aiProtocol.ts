import { TOOL_LABELS } from './aiTools'

/**
 * 小HI 的工具协议解析：伪围栏还原、原生标记清理、消息分段、调用指纹。
 * 从 AiAssistant.vue 抽离，便于单测与复用（页面内 AI / 复盘同样需要）。
 */

export interface Segment {
  type: 'text' | 'code' | 'tool'
  text: string
  lang: string
  code: string
  tool: string
  args: Record<string, unknown>
}

const FENCE_RE = /```([a-zA-Z0-9_-]*)[ \t]*\r?\n([\s\S]*?)```/g

function emptySegment(type: Segment['type']): Segment {
  return { type, text: '', lang: '', code: '', tool: '', args: {} }
}

function tryJson(text: string): Record<string, unknown> | null {
  try {
    const value = JSON.parse(text)
    return value && typeof value === 'object' ? (value as Record<string, unknown>) : null
  } catch {
    return null
  }
}

/** 同一工具 + 规范化参数（键排序）的指纹，用于重复调用拦截 */
export function callKey(tool: string, args: Record<string, unknown>): string {
  const sorted = Object.keys(args)
    .sort()
    .reduce<Record<string, unknown>>((acc, key) => {
      acc[key] = args[key]
      return acc
    }, {})
  return `${tool}:${JSON.stringify(sorted)}`
}

/**
 * 部分模型会漏出原生工具调用标记，写法五花八门：`<|...|>`、`<｜｜DSML｜｜ ...>`（全角竖线）、
 * `<{tool_call}>`、`<\ |\ DSLM\ |\ ...>`、`<tool_call>`、`[TOOL_CALLS]` 等。
 * 这里不逐一枚举写法，而是识别「`<` + 标签符号 + 工具关键字」的通用形态，
 * 把其中的工具 JSON 还原成标准 fbin-tool 块，其余标记一律清掉。
 */
const NATIVE_TAG_RE =
  /<\s*(?:[\\|｜︱丨{]|\/+[\s]*(?:[|｜︱丨{]|(?:DSML|tool_calls?|tool_use|function_calls?|invoke|parameter)\b)|(?:DSML|tool_calls?|tool_use|function_calls?|invoke|parameter)\b)|\[\s*TOOL_CALLS\s*\]/i

const NATIVE_TAG_STRIP_RE =
  /<\s*[\\/|｜︱丨{\s]*(?:DSML|tool_calls?|tool_use|function_calls?|invoke|parameter|calls)\b[^>\n]*>/gi

/** 兜底：清掉不带工具关键字、只靠竖线包裹的标记（如 <|endoftext|>） */
const PIPE_TAG_STRIP_RE = /<\s*[\\/|｜︱丨{\s]*[|｜︱丨][^>\n]*>/g

const KNOWN_TOOLS = new Set(Object.keys(TOOL_LABELS))

export function hasNativeToolSyntax(content: string): boolean {
  return NATIVE_TAG_RE.test(content)
}

function nativeSyntaxIndex(content: string): number {
  const match = NATIVE_TAG_RE.exec(content)
  return match ? match.index : -1
}

/** 去掉原生标记（含全角竖线、大括号、空格、反斜杠转义等变体），只留正文 */
export function stripNativeTags(content: string): string {
  return content
    .replace(NATIVE_TAG_STRIP_RE, '')
    .replace(PIPE_TAG_STRIP_RE, '')
    .replace(/\[\s*TOOL_CALLS\s*\]/gi, '')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

/** 把一段 JSON 归一化成 { tool, args }，兼容原生工具调用的几种字段命名 */
function toToolCall(
  json: Record<string, unknown>,
): { tool: string; args: Record<string, unknown> } | null {
  if (typeof json.tool === 'string' && json.tool.trim()) {
    const rawArgs = json.args
    return {
      tool: json.tool,
      args:
        rawArgs && typeof rawArgs === 'object'
          ? (rawArgs as Record<string, unknown>)
          : Object.fromEntries(Object.entries(json).filter(([key]) => key !== 'tool')),
    }
  }
  if (typeof json.name === 'string' && json.name.trim()) {
    const rawArgs = json.arguments ?? json.parameters ?? json.args
    if (rawArgs && typeof rawArgs === 'object') {
      return { tool: json.name, args: rawArgs as Record<string, unknown> }
    }
  }
  return null
}

/** 代码块范围：块内的 JSON 是示例，不能被当成真实工具调用 */
function fencedRanges(content: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = []
  FENCE_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = FENCE_RE.exec(content))) {
    ranges.push([match.index, match.index + match[0].length])
  }
  return ranges
}

function fenceEndAt(index: number, ranges: Array<[number, number]>): number {
  for (const [start, end] of ranges) {
    if (index >= start && index < end) return end
  }
  return -1
}

export function liftNativeCalls(content: string): string {
  const hasTag = hasNativeToolSyntax(content)
  const fences = fencedRanges(content)
  const calls: {
    start: number
    end: number
    call: { tool: string; args: Record<string, unknown> }
  }[] = []
  let i = 0
  while (i < content.length) {
    if (content[i] !== '{') {
      i += 1
      continue
    }
    const skipTo = fenceEndAt(i, fences)
    if (skipTo > i) {
      i = skipTo
      continue
    }
    let depth = 0
    let inString = false
    let escaped = false
    let end = -1
    for (let j = i; j < content.length; j += 1) {
      const ch = content[j]
      if (inString) {
        if (escaped) escaped = false
        else if (ch === '\\') escaped = true
        else if (ch === '"') inString = false
        continue
      }
      if (ch === '"') inString = true
      else if (ch === '{') depth += 1
      else if (ch === '}') {
        depth -= 1
        if (depth === 0) {
          end = j
          break
        }
      }
    }
    if (end < 0) break
    const parsed = tryJson(content.slice(i, end + 1))
    const call = parsed ? toToolCall(parsed) : null
    // 没有原生标记时只认已知工具名，避免把普通 JSON 示例误判成调用
    if (call && (hasTag || KNOWN_TOOLS.has(call.tool))) {
      calls.push({ start: i, end: end + 1, call })
      i = end + 1
      continue
    }
    i += 1
  }

  if (!calls.length) return hasTag ? stripNativeTags(content) : content

  let out = content
  // 从后往前替换，保证前面的下标不被打乱
  for (const item of [...calls].reverse()) {
    const json = JSON.stringify(item.call)
    out =
      out.slice(0, item.start) +
      `\n\n\`\`\`fbin-tool\n${json}\n\`\`\`\n\n` +
      out.slice(item.end)
  }
  return hasTag ? stripNativeTags(out) : out.trim()
}

function parseSegmentsRaw(content: string): Segment[] {
  const segments: Segment[] = []
  let cursor = 0
  let match: RegExpExecArray | null
  FENCE_RE.lastIndex = 0

  while ((match = FENCE_RE.exec(content))) {
    const [full, lang, body] = match
    if (match.index > cursor) {
      segments.push({ ...emptySegment('text'), text: content.slice(cursor, match.index) })
    }
    const fence = body.trim()
    const language = lang.toLowerCase()
    const parsed = tryJson(fence)
    const isToolCall =
      Boolean(parsed) &&
      (language === 'fbin-tool' ||
        language === 'tool' ||
        (language === 'json' && typeof parsed?.tool === 'string'))
    if (isToolCall && parsed) {
      const tool = String(parsed.tool ?? '')
      const rawArgs = parsed.args
      const args =
        rawArgs && typeof rawArgs === 'object'
          ? (rawArgs as Record<string, unknown>)
          : Object.fromEntries(Object.entries(parsed).filter(([key]) => key !== 'tool'))
      // 空 tool 名多半是模型流式输出时的残块，直接丢弃，不展示也不执行
      if (tool) {
        segments.push({ ...emptySegment('tool'), tool, args })
      }
    } else if ((language === 'http-request' || language === 'json') && parsed && parsed.url && parsed.method) {
      // 兼容旧格式：把裸请求自动映射成 send_request
      segments.push({
        ...emptySegment('tool'),
        tool: 'send_request',
        args: {
          method: String(parsed.method).toUpperCase(),
          url: parsed.url,
          headers: parsed.headers,
          bodyType: parsed.bodyType ?? (parsed.body ? 'json' : 'none'),
          body: parsed.body,
        },
      })
    } else if (!fence && (language === 'fbin-tool' || language === 'tool')) {
      // 空工具块：流式残块或格式退化产物，既不展示也不执行
    } else {
      segments.push({ ...emptySegment('code'), lang: lang || 'text', code: fence })
    }
    cursor = match.index + full.length
  }

  let tail = content.slice(cursor)
  // 流式输出时未闭合的围栏 / 原生工具标签不展示，避免半截内容闪烁
  const pending = tail.indexOf('```')
  if (pending >= 0) tail = tail.slice(0, pending)
  const native = nativeSyntaxIndex(tail)
  if (native >= 0) tail = tail.slice(0, native)
  if (tail.trim()) segments.push({ ...emptySegment('text'), text: tail })
  return segments
}

/**
 * 模型经常把围栏代码块写成 `<fbin-tool>…</fbin-tool>` 这种伪标签，
 * 这里先统一还原成标准围栏，后续解析、隐藏、执行才能一致。
 */
export function normalizePseudoFence(content: string): string {
  if (!/<\s*\/?\s*fbin[-_\s]*tool\s*>/i.test(content)) return content
  return content
    .replace(/<\s*\/\s*fbin[-_\s]*tool\s*>/gi, '\n```\n')
    .replace(/<\s*fbin[-_\s]*tool\s*>/gi, '\n```fbin-tool\n')
}

/** 助手消息统一走这里：清理原生标记，并把漏在正文里的工具 JSON 还原成工具块 */
export function sanitizeAssistant(content: string): string {
  const normalized = normalizePseudoFence(content)
  if (!hasNativeToolSyntax(normalized) && !normalized.includes('"tool"')) return normalized
  return liftNativeCalls(normalized)
}

const segmentCache = new Map<string, Segment[]>()

/** 分段结果按内容缓存，模板里多处调用不会重复解析（长会话渲染提速） */
export function parseSegments(content: string, sanitize = true): Segment[] {
  const normalized = sanitize ? sanitizeAssistant(content) : content
  const cached = segmentCache.get(normalized)
  if (cached) return cached
  const segments = parseSegmentsRaw(normalized)
  if (segmentCache.size > 200) segmentCache.clear()
  segmentCache.set(normalized, segments)
  return segments
}

/** 测试用：清空解析缓存 */
export function clearSegmentCache() {
  segmentCache.clear()
}
