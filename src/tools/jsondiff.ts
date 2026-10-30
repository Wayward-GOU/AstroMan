/* JSON 结构化对比：按路径输出新增 / 删除 / 修改 */

export interface JsonDiffEntry {
  kind: 'add' | 'remove' | 'change'
  path: string
  before?: unknown
  after?: unknown
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function formatPath(parent: string, key: string): string {
  return /^[A-Za-z_$][\w$]*$/.test(key) ? `${parent}.${key}` : `${parent}['${key}']`
}

function display(value: unknown): string {
  const text = JSON.stringify(value) ?? 'null'
  return text.length > 120 ? `${text.slice(0, 120)}…` : text
}

function sameValue(left: unknown, right: unknown): boolean {
  if (left === right) return true
  if (typeof left !== typeof right) return false
  if (typeof left !== 'object' || left === null || right === null) return false
  try {
    return JSON.stringify(left) === JSON.stringify(right)
  } catch {
    return false
  }
}

function walk(left: unknown, right: unknown, path: string, out: JsonDiffEntry[]) {
  if (sameValue(left, right)) return

  if (Array.isArray(left) && Array.isArray(right)) {
    const max = Math.max(left.length, right.length)
    for (let index = 0; index < max; index += 1) {
      const nextPath = `${path}[${index}]`
      if (index >= right.length) {
        out.push({ kind: 'remove', path: nextPath, before: left[index] })
        continue
      }
      if (index >= left.length) {
        out.push({ kind: 'add', path: nextPath, after: right[index] })
        continue
      }
      walk(left[index], right[index], nextPath, out)
    }
    return
  }

  if (isPlainObject(left) && isPlainObject(right)) {
    const keys = new Set([...Object.keys(left), ...Object.keys(right)])
    for (const key of keys) {
      const nextPath = formatPath(path, key)
      const inLeft = key in left
      const inRight = key in right
      if (inLeft && !inRight) {
        out.push({ kind: 'remove', path: nextPath, before: left[key] })
        continue
      }
      if (!inLeft && inRight) {
        out.push({ kind: 'add', path: nextPath, after: right[key] })
        continue
      }
      walk(left[key], right[key], nextPath, out)
    }
    return
  }

  out.push({ kind: 'change', path, before: left, after: right })
}

/** 把两段 JSON 文本按对路径的差异展开 */
export function diffJsonValue(left: unknown, right: unknown): JsonDiffEntry[] {
  const out: JsonDiffEntry[] = []
  walk(left, right, '$', out)
  return out
}

const SEPARATOR = /^\s*-{3,}\s*$/

export function jsonDiff(input: string): string {
  const text = input.trim()
  if (!text) throw new Error('请粘贴两段 JSON，用单独一行 --- 分隔')
  const lines = text.split(/\r?\n/)
  const splitAt = lines.findIndex((line) => SEPARATOR.test(line))
  if (splitAt < 0) throw new Error('缺少分隔行：请用单独一行 --- 把两段 JSON 分开')
  const leftText = lines.slice(0, splitAt).join('\n').trim()
  const rightText = lines
    .slice(splitAt + 1)
    .join('\n')
    .trim()
  if (!leftText || !rightText) throw new Error('分隔符两侧都要有 JSON 内容')

  const parse = (raw: string, label: string) => {
    try {
      return JSON.parse(raw)
    } catch (error) {
      throw new Error(`${label} JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
    }
  }
  const left = parse(leftText, '左侧')
  const right = parse(rightText, '右侧')
  const entries = diffJsonValue(left, right)
  if (!entries.length) return '两侧 JSON 完全一致，没有差异。'

  const add = entries.filter((item) => item.kind === 'add').length
  const remove = entries.filter((item) => item.kind === 'remove').length
  const change = entries.filter((item) => item.kind === 'change').length
  const detail = entries.map((item) => {
    if (item.kind === 'add') return `+ ${item.path} = ${display(item.after)}`
    if (item.kind === 'remove') return `- ${item.path} 原值 ${display(item.before)}`
    return `~ ${item.path}: ${display(item.before)} → ${display(item.after)}`
  })
  return [
    `共 ${entries.length} 处差异（修改 ${change} · 新增 ${add} · 删除 ${remove}）`,
    '',
    detail.join('\n'),
  ].join('\n')
}
