/**
 * 宽松 JSON 解析：在严格 JSON 解析失败时兜底，处理手写 / 粘贴请求体里最常见的几类问题：
 *
 * - 单引号字符串：`{'id':'1'}`（从控制台、Java / Python 日志里复制出来的最常见）
 * - 尾逗号：`{"a":1,}`、`[1,2,]`
 * - 无引号属性名：`{id:1}`
 * - 注释：`// 行注释`、`/* 块注释 *\/`
 * - 全角标点：`｛"a"：1，｝`（从微信 / 钉钉 / Word / WPS 复制后常见）
 * - BOM 头、尾随分号
 *
 * 只用于「格式化 / 预览 / 表单解析」这类用户主动触发的场景，不参与发送前的严格校验：
 * 解析结果会重新序列化为标准 JSON，调用方必须把 `repairs` 告知用户，
 * 因为规范化会改变实际发出的正文字节。
 */

export type LooseRepair =
  | 'singleQuote'
  | 'trailingComma'
  | 'bareKey'
  | 'comment'
  | 'fullwidth'
  | 'bom'
  | 'semicolon'
  /** 不是「已修正」，而是提醒：裸写的大整数超出安全整数范围，规范化后精度已丢失 */
  | 'bigNumber'

export interface LooseParseResult {
  value: unknown
  /** 命中的宽松处理项，去重且按首次出现顺序排列 */
  repairs: LooseRepair[]
}

const IDENT_START = /[A-Za-z_$]/
const IDENT_PART = /[A-Za-z0-9_$-]/
const NUMBER_RE = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y

/** 结构位置上的全角字符 → 对应 ASCII。字符串内部不做这层替换 */
const FULLWIDTH_ASCII: Record<string, string> = {
  '｛': '{',
  '｝': '}',
  '［': '[',
  '］': ']',
  '：': ':',
  '，': ',',
  '＆': '&',
  '“': '"',
  '”': '"',
  '‘': "'",
  '’': "'",
  '　': ' ',
}

/** 把各种引号归一成 JSON 的双 / 单引号两族，便于判断闭合 */
const QUOTE_FAMILY: Record<string, '"' | "'"> = {
  '"': '"',
  '“': '"',
  '”': '"',
  "'": "'",
  '‘': "'",
  '’': "'",
}

class LooseJsonParser {
  private index = 0
  private readonly repairs = new Set<LooseRepair>()

  constructor(private readonly text: string) {}

  parse(): LooseParseResult {
    // 记事本 / Excel 导出的 UTF-8 文件常带 BOM，严格 JSON.parse 会直接失败
    if (this.text.charCodeAt(0) === 0xfeff) {
      this.repairs.add('bom')
      this.index = 1
    }
    this.skipTrivia()
    const value = this.parseValue()
    this.skipTrivia()
    // Java 代码里复制出来的 `String json = "{...}";` 会带分号
    while (this.text[this.index] === ';') {
      this.repairs.add('semicolon')
      this.index += 1
      this.skipTrivia()
    }
    if (this.index < this.text.length) {
      throw this.error('此处存在多余的字符')
    }
    return { value, repairs: [...this.repairs] }
  }

  /* ---------- 基础扫描 ---------- */

  private skipTrivia() {
    while (this.index < this.text.length) {
      const ch = this.text[this.index]
      if (ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r') {
        this.index += 1
        continue
      }
      if (ch === '　') {
        this.repairs.add('fullwidth')
        this.index += 1
        continue
      }
      const next = this.text[this.index + 1]
      if (ch === '/' && next === '/') {
        this.repairs.add('comment')
        this.index += 2
        while (this.index < this.text.length && this.text[this.index] !== '\n') this.index += 1
        continue
      }
      if (ch === '/' && next === '*') {
        this.repairs.add('comment')
        const end = this.text.indexOf('*/', this.index + 2)
        if (end < 0) throw this.error('块注释未闭合')
        this.index = end + 2
        continue
      }
      break
    }
  }

  private error(message: string): SyntaxError {
    let line = 1
    let column = 1
    for (let i = 0; i < this.index && i < this.text.length; i += 1) {
      if (this.text[i] === '\n') {
        line += 1
        column = 1
      } else {
        column += 1
      }
    }
    return new SyntaxError(`${message}（第 ${line} 行第 ${column} 列）`)
  }

  /** 当前位置是否是指定结构符（含全角等价写法） */
  private at(ch: string): boolean {
    const current = this.text[this.index]
    if (current === undefined) return false
    return current === ch || FULLWIDTH_ASCII[current] === ch
  }

  private expect(ch: string) {
    if (!this.at(ch)) throw this.error(`缺少「${ch}」`)
    if (this.text[this.index] !== ch) this.repairs.add('fullwidth')
    this.index += 1
  }

  /* ---------- 值 ---------- */

  private parseValue(): unknown {
    this.skipTrivia()
    const ch = this.text[this.index]
    if (ch === undefined) throw this.error('内容意外结束')
    if (this.at('{')) return this.parseObject()
    if (this.at('[')) return this.parseArray()
    const quote = QUOTE_FAMILY[ch]
    if (quote) return this.parseString(quote)
    if (ch === '-' || (ch >= '0' && ch <= '9')) return this.parseNumber()
    if (this.text.startsWith('true', this.index)) {
      this.index += 4
      return true
    }
    if (this.text.startsWith('false', this.index)) {
      this.index += 5
      return false
    }
    if (this.text.startsWith('null', this.index)) {
      this.index += 4
      return null
    }
    throw this.error('无法识别的值')
  }

  private parseObject(): Record<string, unknown> {
    this.expect('{')
    const result: Record<string, unknown> = {}
    for (;;) {
      this.skipTrivia()
      if (this.at('}')) {
        this.expect('}')
        return result
      }
      const key = this.parseKey()
      this.skipTrivia()
      this.expect(':')
      result[key] = this.parseValue()
      this.skipTrivia()
      if (this.at(',')) {
        this.expect(',')
        this.skipTrivia()
        // 尾逗号：逗号后面直接是右括号
        if (this.at('}')) {
          this.repairs.add('trailingComma')
          this.expect('}')
          return result
        }
        continue
      }
      if (this.at('}')) {
        this.expect('}')
        return result
      }
      throw this.error('属性之间缺少逗号')
    }
  }

  private parseArray(): unknown[] {
    this.expect('[')
    const result: unknown[] = []
    for (;;) {
      this.skipTrivia()
      if (this.at(']')) {
        this.expect(']')
        return result
      }
      result.push(this.parseValue())
      this.skipTrivia()
      if (this.at(',')) {
        this.expect(',')
        this.skipTrivia()
        if (this.at(']')) {
          this.repairs.add('trailingComma')
          this.expect(']')
          return result
        }
        continue
      }
      if (this.at(']')) {
        this.expect(']')
        return result
      }
      throw this.error('元素之间缺少逗号')
    }
  }

  private parseKey(): string {
    const ch = this.text[this.index]
    const quote = ch === undefined ? undefined : QUOTE_FAMILY[ch]
    if (quote) return this.parseString(quote)
    if (ch !== undefined && IDENT_START.test(ch)) {
      let end = this.index + 1
      while (end < this.text.length && IDENT_PART.test(this.text[end])) end += 1
      const key = this.text.slice(this.index, end)
      this.index = end
      this.repairs.add('bareKey')
      return key
    }
    throw this.error('属性名必须是字符串或标识符')
  }

  /**
   * 单 / 双引号字符串（含全角引号）；转义按 JSON 规则处理
   */
  private parseString(quote: '"' | "'"): string {
    const open = this.text[this.index]
    if (open === '“' || open === '”' || open === '‘' || open === '’') {
      this.repairs.add('fullwidth')
    }
    if (quote === "'") this.repairs.add('singleQuote')
    // ASCII 开引号时只在"后面已经没有同类 ASCII 闭合引号"的前提下接受全角闭合，
    // 否则 `{"t":"他说“你好”"}` 里的中文引号会被误判成结束符
    const allowFullwidthClose = open !== quote || this.text.indexOf(quote, this.index + 1) < 0
    this.index += 1
    let out = ''
    while (this.index < this.text.length) {
      const ch = this.text[this.index]
      if (ch !== undefined && this.closesString(open, ch, quote, allowFullwidthClose)) {
        if (ch !== open) this.repairs.add('fullwidth')
        this.index += 1
        return out
      }
      if (ch === '\\') {
        const esc = this.text[this.index + 1]
        this.index += 2
        switch (esc) {
          case '"':
            out += '"'
            break
          case "'":
            out += "'"
            break
          case '\\':
            out += '\\'
            break
          case '/':
            out += '/'
            break
          case 'b':
            out += '\b'
            break
          case 'f':
            out += '\f'
            break
          case 'n':
            out += '\n'
            break
          case 'r':
            out += '\r'
            break
          case 't':
            out += '\t'
            break
          case 'u': {
            const hex = this.text.slice(this.index, this.index + 4)
            if (!/^[0-9a-fA-F]{4}$/.test(hex)) throw this.error('\\u 转义不完整')
            out += String.fromCharCode(Number.parseInt(hex, 16))
            this.index += 4
            break
          }
          case undefined:
            throw this.error('转义符后内容意外结束')
          default:
            // 未知转义按字面保留，与 JS 单引号串的宽松行为一致
            out += esc
        }
        continue
      }
      if (ch === '\n' || ch === '\r') throw this.error('字符串未闭合')
      out += ch
      this.index += 1
    }
    throw this.error('字符串未闭合')
  }

  /**
   * 字符串闭合判定。
   *
   * ASCII 开引号在存在同类 ASCII 闭合引号时只认该字符：中文正文里的 `“”` 是内容，
   * 不能当成结束符，否则 `"他说“你好”"` 会被截断；只有在后面已经没有 ASCII 闭合引号时
   * （Word 把右引号替换成 `”` 的情况）才接受全角闭合。
   */
  private closesString(
    open: string,
    ch: string,
    quote: '"' | "'",
    allowFullwidthClose: boolean,
  ): boolean {
    if (ch === open) return true
    if (open === '"' || open === "'") return allowFullwidthClose && QUOTE_FAMILY[ch] === quote
    return QUOTE_FAMILY[ch] === quote
  }

  private parseNumber(): number {
    NUMBER_RE.lastIndex = this.index
    const matched = NUMBER_RE.exec(this.text)
    if (!matched) throw this.error('数字格式不正确')
    const literal = matched[0]
    this.index += literal.length
    const value = Number(literal)
    // 纯整数且超出安全范围：规范化后必然丢精度，只能提醒用户加引号
    if (!/[.eE]/.test(literal) && !Number.isSafeInteger(value)) {
      this.repairs.add('bigNumber')
    }
    return value
  }
}

/**
 * 解析宽松 JSON。严格 JSON 能过的文本也能过，且 `repairs` 为空；
 * 失败时抛出带行列位置的 `SyntaxError`。
 */
export function parseLooseJson(text: string): LooseParseResult {
  return new LooseJsonParser(text).parse()
}

export type JsonBodyIssue =
  | { kind: 'loose'; repairs: LooseRepair[] }
  | { kind: 'invalid'; message: string }

/**
 * 发送前的正文体检：返回 null 表示严格 JSON 合法。
 *
 * - `loose`：原文不是标准 JSON，但能被宽松规则读懂（格式化即可修复）
 * - `invalid`：两种解析都失败，发出去大概率被服务端拒绝
 *
 * 只做判断，不改写正文 —— 是否发送由调用方确认。
 */
export function checkJsonBody(text: string): JsonBodyIssue | null {
  try {
    JSON.parse(text)
    return null
  } catch {
    // 继续尝试宽松解析，拿更具体的错误位置
  }
  try {
    const { repairs } = parseLooseJson(text)
    return { kind: 'loose', repairs }
  } catch (error) {
    return { kind: 'invalid', message: error instanceof Error ? error.message : String(error) }
  }
}

/** 把命中的宽松项翻成给人看的短标签 */
export function describeLooseRepairs(repairs: LooseRepair[]): string {
  const labels: Record<LooseRepair, string> = {
    singleQuote: '单引号字符串',
    trailingComma: '尾逗号',
    bareKey: '无引号属性名',
    comment: '注释',
    fullwidth: '全角标点',
    bom: 'BOM 头',
    semicolon: '尾随分号',
    bigNumber: '大整数精度',
  }
  return repairs.map((item) => labels[item]).join('、')
}
