export interface DiffLine {
  type: 'same' | 'add' | 'del'
  text: string
}

/**
 * 按行做 LCS 差异比较。为避免大响应体撑爆内存，最多比较 maxLines 行。
 */
export function diffLines(left: string, right: string, maxLines = 800): DiffLine[] {
  const a = left.split('\n').slice(0, maxLines)
  const b = right.split('\n').slice(0, maxLines)
  const n = a.length
  const m = b.length

  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ type: 'same', text: a[i] })
      i += 1
      j += 1
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ type: 'del', text: a[i] })
      i += 1
    } else {
      out.push({ type: 'add', text: b[j] })
      j += 1
    }
  }
  while (i < n) out.push({ type: 'del', text: a[i++] })
  while (j < m) out.push({ type: 'add', text: b[j++] })
  return out
}

export function diffStats(lines: DiffLine[]) {
  let added = 0
  let removed = 0
  for (const line of lines) {
    if (line.type === 'add') added += 1
    else if (line.type === 'del') removed += 1
  }
  return { added, removed }
}
