/* 文本差异对比（行级 LCS）与差异统计 */

export type DiffKind = 'same' | 'add' | 'remove'

export interface DiffLine {
  kind: DiffKind
  text: string
  leftNo: number | null
  rightNo: number | null
}

export interface DiffResult {
  lines: DiffLine[]
  added: number
  removed: number
  same: number
}

const MAX_LINES = 4000

export function diffLines(left: string, right: string): DiffResult {
  const a = left.replace(/\r\n/g, '\n').split('\n')
  const b = right.replace(/\r\n/g, '\n').split('\n')
  if (a.length > MAX_LINES || b.length > MAX_LINES) {
    throw new Error(`文本行数超过 ${MAX_LINES} 行，请拆分后再对比`)
  }

  const n = a.length
  const m = b.length
  // LCS 动态规划表（n*m 控制在 4000*4000 以内）
  const table: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1))
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }

  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  let leftNo = 1
  let rightNo = 1
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      lines.push({ kind: 'same', text: a[i], leftNo, rightNo })
      i += 1
      j += 1
      leftNo += 1
      rightNo += 1
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      lines.push({ kind: 'remove', text: a[i], leftNo, rightNo: null })
      i += 1
      leftNo += 1
    } else {
      lines.push({ kind: 'add', text: b[j], leftNo: null, rightNo })
      j += 1
      rightNo += 1
    }
  }
  while (i < n) {
    lines.push({ kind: 'remove', text: a[i], leftNo, rightNo: null })
    i += 1
    leftNo += 1
  }
  while (j < m) {
    lines.push({ kind: 'add', text: b[j], leftNo: null, rightNo })
    j += 1
    rightNo += 1
  }

  return {
    lines,
    added: lines.filter((line) => line.kind === 'add').length,
    removed: lines.filter((line) => line.kind === 'remove').length,
    same: lines.filter((line) => line.kind === 'same').length,
  }
}

export function diffAsText(result: DiffResult): string {
  return result.lines
    .map((line) => {
      const prefix = line.kind === 'add' ? '+ ' : line.kind === 'remove' ? '- ' : '  '
      return prefix + line.text
    })
    .join('\n')
}

/** 逐字符差异，用于短文本比对 */
export function diffChars(left: string, right: string): { added: number; removed: number } {
  const a = Array.from(left)
  const b = Array.from(right)
  const table: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  )
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }
  const same = table[0][0]
  return { added: b.length - same, removed: a.length - same }
}
