/**
 * XML 格式化：只调整标签之间的换行与缩进，不改标签内部（属性顺序、引号、大小写原样保留）。
 *
 * 刻意保守的几处：
 * - 混合内容（元素里同时有文字和子元素，如 `<p>见 <b>说明</b></p>`）整体保持一行，
 *   否则插入的换行空白会进入文本节点，改变渲染语义；
 * - 纯文本元素单行输出；
 * - 注释 / CDATA / 处理指令 / DOCTYPE 原样保留。
 */

export interface XmlFormatOptions {
  indent?: string
}

interface ElementNode {
  type: 'element'
  offset: number
  /** 原始开始标签文本，如 `<a href="x">` */
  open: string
  name: string
  /** 原始结束标签文本；自闭合标签为空串 */
  close: string
  children: XmlNode[]
}

interface TextNode {
  type: 'text'
  text: string
}

interface RawNode {
  type: 'raw'
  text: string
}

type XmlNode = ElementNode | TextNode | RawNode

export function formatXml(text: string, options: XmlFormatOptions = {}): string {
  const indent = options.indent ?? '  '
  if (!text.trim()) throw new SyntaxError('内容为空')

  const nodes = parseXml(text)
  if (!nodes.length) throw new SyntaxError('没有可格式化的 XML 内容')

  const lines: string[] = []
  for (const node of nodes) lines.push(...serializeNode(node, 0, indent))
  return lines.join('\n')
}

/** 是否看起来像 XML（供 Raw 类型自动选择格式化方式）；规则与后端 `looks_like_xml` 对齐 */
export function looksLikeXml(text: string): boolean {
  const trimmed = text.trimStart()
  if (!trimmed.startsWith('<')) return false
  if (/^<\?xml[\s?>]/i.test(trimmed)) return true
  if (trimmed.startsWith('<!--')) return true
  if (/^<![A-Za-z]/.test(trimmed)) return true
  // 标签名允许 Unicode 字母（中文标签、命名空间前缀），且后面必须紧跟空白 / > / /
  return /^<\/?[\p{L}_:][\p{L}\p{N}_.:-]*[\s/>]/u.test(trimmed)
}

/* ---------- 解析 ---------- */

function parseXml(text: string): XmlNode[] {
  const root: XmlNode[] = []
  const stack: ElementNode[] = []
  let index = 0

  const push = (node: XmlNode) => {
    const parent = stack[stack.length - 1]
    if (parent) parent.children.push(node)
    else root.push(node)
  }

  while (index < text.length) {
    if (text[index] !== '<') {
      const next = text.indexOf('<', index)
      const end = next < 0 ? text.length : next
      push({ type: 'text', text: text.slice(index, end) })
      index = end
      continue
    }

    if (text.startsWith('<!--', index)) {
      const end = text.indexOf('-->', index + 4)
      if (end < 0) throw errorAt(text, index, '注释未闭合')
      push({ type: 'raw', text: text.slice(index, end + 3) })
      index = end + 3
      continue
    }

    if (text.startsWith('<![CDATA[', index)) {
      const end = text.indexOf(']]>', index + 9)
      if (end < 0) throw errorAt(text, index, 'CDATA 段未闭合')
      push({ type: 'raw', text: text.slice(index, end + 3) })
      index = end + 3
      continue
    }

    if (text.startsWith('<!', index)) {
      const end = findDeclarationEnd(text, index)
      if (end < 0) throw errorAt(text, index, '声明未闭合')
      push({ type: 'raw', text: text.slice(index, end + 1) })
      index = end + 1
      continue
    }

    if (text.startsWith('<?', index)) {
      const end = text.indexOf('?>', index + 2)
      if (end < 0) throw errorAt(text, index, '处理指令未闭合')
      push({ type: 'raw', text: text.slice(index, end + 2) })
      index = end + 2
      continue
    }

    const end = findTagEnd(text, index)
    if (end < 0) throw errorAt(text, index, '标签未闭合')

    if (text[index + 1] === '/') {
      const name = text.slice(index + 2, end).trim()
      const node = stack.pop()
      if (!node) throw errorAt(text, index, `多余的结束标签 </${name}>`)
      if (node.name !== name) {
        throw errorAt(text, index, `结束标签 </${name}> 与 <${node.name}> 不匹配`)
      }
      node.close = text.slice(index, end + 1)
      index = end + 1
      continue
    }

    const open = text.slice(index, end + 1)
    const name = tagName(open)
    // XML 的 NameStartChar 包含大量 Unicode 字符（中文标签、命名空间前缀都合法），
    // 这里只挡掉明显不可能出现在标签名里的字符，配对检查交给栈
    if (!name || /[\s<>/"'=!?]/.test(name)) {
      throw errorAt(text, index, `标签名「${name}」不合法`)
    }
    const node: ElementNode = { type: 'element', offset: index, open, name, close: '', children: [] }
    push(node)
    if (!open.endsWith('/>')) stack.push(node)
    index = end + 1
  }

  const unclosed = stack[stack.length - 1]
  if (unclosed) throw errorAt(text, unclosed.offset, `标签 <${unclosed.name}> 未闭合`)
  return root
}

/** 从 `<` 起找标签结束的 `>`，跳过属性值引号里的 `>` */
function findTagEnd(text: string, start: number): number {
  let quote: '"' | "'" | null = null
  for (let i = start + 1; i < text.length; i += 1) {
    const ch = text[i]
    if (quote) {
      if (ch === quote) quote = null
      continue
    }
    if (ch === '"' || ch === "'") {
      quote = ch
      continue
    }
    if (ch === '>') return i
  }
  return -1
}

/** `<!DOCTYPE ... [ ... ]>` 内部可能有 `[]`，需要跳过 */
function findDeclarationEnd(text: string, start: number): number {
  let depth = 0
  for (let i = start + 2; i < text.length; i += 1) {
    const ch = text[i]
    if (ch === '[') depth += 1
    else if (ch === ']') depth = Math.max(0, depth - 1)
    else if (ch === '>' && depth === 0) return i
  }
  return -1
}

function tagName(open: string): string {
  const body = open.slice(1)
  const name = body.split(/[\s/>]/, 1)[0] ?? ''
  return name.replace(/\/$/, '')
}

function errorAt(text: string, index: number, message: string): SyntaxError {
  let line = 1
  let column = 1
  for (let i = 0; i < index && i < text.length; i += 1) {
    if (text[i] === '\n') {
      line += 1
      column = 1
    } else {
      column += 1
    }
  }
  return new SyntaxError(`${message}（第 ${line} 行第 ${column} 列）`)
}

/* ---------- 序列化 ---------- */

function serializeNode(node: XmlNode, depth: number, indent: string): string[] {
  if (node.type === 'text') {
    const text = node.text.trim()
    return text ? [indent.repeat(depth) + text] : []
  }
  if (node.type === 'raw') {
    const pad = indent.repeat(depth)
    return node.text
      .trim()
      .split('\n')
      .map((line, i) => (i === 0 ? pad + line.trim() : pad + line.trim()))
  }

  const pad = indent.repeat(depth)
  const close = node.close || `</${node.name}>`
  if (node.open.endsWith('/>')) return [pad + node.open]

  const hasElement = node.children.some((child) => child.type === 'element')
  const textChildren = node.children.filter((child): child is TextNode => child.type === 'text')
  const hasText = textChildren.some((child) => child.text.trim() !== '')

  if (!hasElement) {
    const content = textChildren.map((child) => child.text).join('')
    if (!hasText) return [pad + node.open + close]
    const raw = content.trim()
    if (!raw.includes('\n')) return [pad + node.open + raw + close]
    const lines = raw.split('\n').map((line) => line.trim())
    // 续行对齐到首行内容的起始列，读起来像原文折行
    const contPad = pad + ' '.repeat(node.open.length)
    return [
      pad + node.open + lines[0],
      ...lines.slice(1, -1).map((line) => contPad + line),
      contPad + lines[lines.length - 1] + close,
    ]
  }

  if (hasText) {
    // 混合内容：整体单行，保留原有空白，避免引入新的文本节点空白
    return [pad + node.open + node.children.map(inlineNode).join('') + close]
  }

  const lines = [pad + node.open]
  for (const child of node.children) lines.push(...serializeNode(child, depth + 1, indent))
  lines.push(pad + close)
  return lines
}

function inlineNode(node: XmlNode): string {
  if (node.type === 'text' || node.type === 'raw') return node.text
  const close = node.close || `</${node.name}>`
  if (node.open.endsWith('/>')) return node.open
  return node.open + node.children.map(inlineNode).join('') + close
}
