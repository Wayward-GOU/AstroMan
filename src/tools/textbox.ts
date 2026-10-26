/* 文本一键处理：清理、排序、连接、表格转换与 MyBatis 日志提取 */

import { fillLogSql, parseMybatisLog } from '@/utils/mybatis'

/* ---------------- 清理 ---------------- */

export function removeBlankLines(text: string): string {
  return text
    .split(/\r?\n/)
    .filter((line) => line.trim())
    .join('\n')
}

/** 删除半角、全角空格与制表符，保留换行 */
export function removeSpaces(text: string): string {
  return text.replace(/[ \t\u00a0\u3000]+/g, '')
}

export function dedupeLines(text: string): string {
  const seen = new Set<string>()
  return text
    .split(/\r?\n/)
    .filter((line) => {
      if (seen.has(line)) return false
      seen.add(line)
      return true
    })
    .join('\n')
}

/** 只保留出现两次及以上的行，重复项只输出一次 */
export function duplicateLines(text: string): string {
  const counts = new Map<string, number>()
  const order: string[] = []
  for (const line of text.split(/\r?\n/)) {
    if (!counts.has(line)) order.push(line)
    counts.set(line, (counts.get(line) ?? 0) + 1)
  }
  return order.filter((line) => line.trim() && (counts.get(line) ?? 0) > 1).join('\n')
}

export function reverseText(text: string): string {
  return Array.from(text).reverse().join('')
}

/* ---------------- 排序与连接 ---------------- */

export function sortLinesAsText(text: string): string {
  return text
    .split(/\r?\n/)
    .sort((left, right) => left.localeCompare(right, 'zh-CN'))
    .join('\n')
}

/** 按每行第一个数字升序；没有数字的行排在最后，保持原有先后 */
export function sortLinesAsNumber(text: string): string {
  return text
    .split(/\r?\n/)
    .map((line, index) => {
      const matched = /-?\d+(?:\.\d+)?/.exec(line)
      return {
        line,
        index,
        value: matched ? Number(matched[0]) : Number.POSITIVE_INFINITY,
      }
    })
    .sort((left, right) =>
      left.value === right.value ? left.index - right.index : left.value - right.value,
    )
    .map((item) => item.line)
    .join('\n')
}

export function joinLines(text: string, separator: string): string {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .join(separator)
}

/** 生成 SQL IN 列表：支持按换行或逗号拆分，所有值统一加单引号，可直接复制执行 */
export function sqlInList(text: string): string {
  const values = text
    .split(/[\r\n,，]+/)
    .map((value) => value.trim())
    .filter(Boolean)
  if (!values.length) throw new Error('请输入需要生成 SQL IN 的内容')
  const items = values.map((value) => `'${value.replace(/'/g, "''")}'`)
  return `IN(${items.join(',')});`
}

/* ---------------- 表格 ---------------- */

function tableLines(text: string): string[] {
  return text.split(/\r?\n/).filter((line) => line.trim())
}

function splitRow(line: string, delimiter: string): string[] {
  return line.split(delimiter).map((cell) => cell.trim())
}

function parseTable(text: string, delimiter: string): string[][] {
  return tableLines(text).map((line) => splitRow(line, delimiter))
}

/** 自动识别 Tab、逗号、竖线、分号或连续空格 */
export function autoDelimiter(text: string): string {
  const sample = tableLines(text)[0] ?? ''
  if (sample.includes('\t')) return '\t'
  if (sample.includes(',')) return ','
  if (sample.includes('|')) return '|'
  if (sample.includes(';')) return ';'
  if (/\s{2,}/.test(sample)) return '  '
  return ','
}

/** 列转行：每行按分隔符拆开，每个值单独占一行 */
export function splitColumnsToRows(text: string, delimiter: string): string {
  return tableLines(text)
    .reduce<string[]>((rows, line) => rows.concat(splitRow(line, delimiter)), [])
    .join('\n')
}

/** 行列互换；未指定分隔符时自动识别 */
export function transposeTable(text: string, delimiter?: string): string {
  const separator = delimiter || autoDelimiter(text)
  const rows = parseTable(text, separator)
  if (!rows.length) throw new Error('请输入表格内容')
  const width = Math.max(...rows.map((row) => row.length))
  const output: string[] = []
  for (let column = 0; column < width; column += 1) {
    output.push(rows.map((row) => row[column] ?? '').join(separator))
  }
  return output.join('\n')
}

export function tableToMarkdown(text: string): string {
  const rows = parseTable(text, autoDelimiter(text))
  if (!rows.length) throw new Error('请输入表格内容')
  const width = Math.max(...rows.map((row) => row.length))
  const renderCells = (row: string[]) =>
    Array.from({ length: width }, (_, index) =>
      (row[index] ?? '').replace(/\|/g, '\\|'),
    ).join(' | ')
  return [
    `| ${renderCells(rows[0])} |`,
    `| ${Array.from({ length: width }, () => '---').join(' | ')} |`,
    ...rows.slice(1).map((row) => `| ${renderCells(row)} |`),
  ].join('\n')
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

export function tableToHtml(text: string): string {
  const rows = parseTable(text, autoDelimiter(text))
  if (!rows.length) throw new Error('请输入表格内容')
  const width = Math.max(...rows.map((row) => row.length))
  const renderRow = (row: string[], tag: 'th' | 'td') =>
    [
      '    <tr>',
      ...Array.from(
        { length: width },
        (_, index) => `      <${tag}>${escapeHtml(row[index] ?? '')}</${tag}>`,
      ),
      '    </tr>',
    ].join('\n')
  return [
    '<table>',
    '  <thead>',
    renderRow(rows[0], 'th'),
    '  </thead>',
    '  <tbody>',
    ...rows.slice(1).map((row) => renderRow(row, 'td')),
    '  </tbody>',
    '</table>',
  ].join('\n')
}

/** 多表连接：用空行分隔多个表格，按行左右拼接为一张表 */
export function joinTables(text: string, delimiter?: string): string {
  const blocks = text
    .trim()
    .split(/\r?\n\s*\r?\n/)
    .filter((block) => block.trim())
  if (blocks.length < 2) throw new Error('请用空行分隔至少两个表格')
  const separator = delimiter || autoDelimiter(text)
  const tables = blocks.map((block) => parseTable(block, separator))
  const widths = tables.map((table) => Math.max(0, ...table.map((row) => row.length)))
  const height = Math.max(...tables.map((table) => table.length))
  const lines: string[] = []
  for (let index = 0; index < height; index += 1) {
    const cells: string[] = []
    tables.forEach((table, tableIndex) => {
      const row = table[index] ?? []
      for (let column = 0; column < widths[tableIndex]; column += 1) {
        cells.push(row[column] ?? '')
      }
    })
    lines.push(cells.join(separator))
  }
  return lines.join('\n')
}

/* ---------------- JSON 与 MyBatis ---------------- */

export function formatJson(text: string): string {
  const raw = text.trim()
  if (!raw) throw new Error('请输入 JSON 内容')
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch (error) {
    throw new Error(`JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
}

export function extractMybatisSql(text: string): string {
  if (!text.trim()) throw new Error('请粘贴 MyBatis DEBUG 日志')
  const { statements } = parseMybatisLog(text)
  if (!statements.length) throw new Error('未找到 Preparing 日志，请确认日志级别为 DEBUG')
  return statements.map((statement) => fillLogSql(statement, true).sql).join('\n\n')
}
