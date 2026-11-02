/* JSONPath 查询：支持 $ .key ['key'] [0] [*] ..key 与 $..* */

type PathStep =
  | { type: 'child'; key: string }
  | { type: 'index'; index: number }
  | { type: 'wildcard' }
  | { type: 'descend'; key: string }
  | { type: 'descendAll' }

interface PathNode {
  path: string
  value: unknown
}

export interface JsonPathHit {
  path: string
  value: unknown
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readName(text: string, start: number): { name: string; next: number } {
  let index = start
  let name = ''
  while (index < text.length && text[index] !== '.' && text[index] !== '[') {
    name += text[index]
    index += 1
  }
  return { name, next: index }
}

function parsePath(raw: string): PathStep[] {
  const text = raw.trim()
  if (!text) return []
  const body = text.startsWith('$') ? text.slice(1) : text
  const steps: PathStep[] = []
  let index = 0
  while (index < body.length) {
    const char = body[index]
    if (char === '.') {
      if (body[index + 1] === '.') {
        const result = readName(body, index + 2)
        index = result.next
        if (!result.name || result.name === '*') steps.push({ type: 'descendAll' })
        else steps.push({ type: 'descend', key: result.name })
        continue
      }
      const result = readName(body, index + 1)
      if (!result.name) throw new Error('路径中出现了空字段名，例如 $.a..b')
      index = result.next
      steps.push(result.name === '*' ? { type: 'wildcard' } : { type: 'child', key: result.name })
      continue
    }
    if (char === '[') {
      const end = body.indexOf(']', index)
      if (end < 0) throw new Error('路径中的 [ 没有闭合')
      const inner = body.slice(index + 1, end).trim()
      index = end + 1
      if (inner === '*') {
        steps.push({ type: 'wildcard' })
        continue
      }
      if (/^-?\d+$/.test(inner)) {
        steps.push({ type: 'index', index: Number(inner) })
        continue
      }
      const quoted = /^['"]([^'"]*)['"]$/.exec(inner)
      if (quoted) {
        steps.push({ type: 'child', key: quoted[1] })
        continue
      }
      if (inner && !/[[\]]/.test(inner)) {
        steps.push({ type: 'child', key: inner })
        continue
      }
      throw new Error(`无法识别的片段：[${inner}]`)
    }
    const result = readName(body, index)
    if (!result.name) {
      index += 1
      continue
    }
    index = result.next
    steps.push({ type: 'child', key: result.name })
  }
  return steps
}

/** 深度优先收集自身与全部后代节点 */
function collectDescendants(node: PathNode, out: PathNode[], includeSelf = true) {
  if (includeSelf) out.push(node)
  if (Array.isArray(node.value)) {
    node.value.forEach((item, index) => {
      collectDescendants({ path: `${node.path}[${index}]`, value: item }, out)
    })
    return
  }
  if (isPlainObject(node.value)) {
    Object.entries(node.value).forEach(([key, value]) => {
      collectDescendants({ path: formatPath(node.path, key), value }, out)
    })
  }
}

function formatPath(parent: string, key: string): string {
  return /^[A-Za-z_$][\w$]*$/.test(key) ? `${parent}.${key}` : `${parent}['${key}']`
}

function parseJson(input: string): unknown {
  const text = input.trim()
  if (!text) throw new Error('请粘贴 JSON 内容')
  try {
    return JSON.parse(text)
  } catch (error) {
    throw new Error(`JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
}

export function jsonPathHits(input: string, path: string): JsonPathHit[] {
  const root = parseJson(input)
  const steps = parsePath(path)
  let nodes: PathNode[] = [{ path: '$', value: root }]
  for (const step of steps) {
    const next: PathNode[] = []
    for (const node of nodes) {
      if (step.type === 'child') {
        if (!isPlainObject(node.value) || !(step.key in node.value)) continue
        next.push({ path: formatPath(node.path, step.key), value: node.value[step.key] })
        continue
      }
      if (step.type === 'index') {
        if (!Array.isArray(node.value)) continue
        const target = step.index < 0 ? node.value.length + step.index : step.index
        if (target < 0 || target >= node.value.length) continue
        next.push({ path: `${node.path}[${target}]`, value: node.value[target] })
        continue
      }
      if (step.type === 'wildcard') {
        if (Array.isArray(node.value)) {
          node.value.forEach((item, index) => next.push({ path: `${node.path}[${index}]`, value: item }))
        } else if (isPlainObject(node.value)) {
          Object.entries(node.value).forEach(([key, value]) => next.push({ path: formatPath(node.path, key), value }))
        }
        continue
      }
      const all: PathNode[] = []
      collectDescendants(node, all)
      if (step.type === 'descendAll') {
        all.forEach((item) => {
          if (item.path !== node.path) next.push(item)
        })
        continue
      }
      all.forEach((item) => {
        if (isPlainObject(item.value) && step.key in item.value) {
          next.push({ path: formatPath(item.path, step.key), value: item.value[step.key] })
        }
      })
    }
    nodes = next
    if (!nodes.length) break
  }
  return nodes.map((node) => ({ path: node.path, value: node.value }))
}

export function queryJsonPath(input: string, path: string): string {
  const hits = jsonPathHits(input, path)
  const label = path.trim() || '$'
  if (!hits.length) {
    return `路径：${label}\n没有匹配到内容，请检查字段名与层级是否正确。`
  }
  if (hits.length === 1) {
    const hit = hits[0]
    const value = hit.value
    if (value === null) return `路径：${label}\n匹配到 1 处\n\nnull\n\n位置：${hit.path}`
    if (typeof value === 'object') {
      return `路径：${label}\n匹配到 1 处，已按 JSON 输出\n\n${JSON.stringify(value, null, 2)}\n\n位置：${hit.path}`
    }
    return `路径：${label}\n匹配到 1 处\n\n${String(value)}\n\n位置：${hit.path}`
  }
  const detail = hits.map((hit, index) => {
    const text = JSON.stringify(hit.value) ?? 'null'
    const display = text.length > 240 ? `${text.slice(0, 240)}…` : text
    return `${index + 1}. ${hit.path} = ${display}`
  })
  return [
    `路径：${label}`,
    `匹配到 ${hits.length} 处`,
    '',
    detail.join('\n'),
    '',
    '合并结果：',
    JSON.stringify(
      hits.map((hit) => hit.value),
      null,
      2,
    ),
  ].join('\n')
}
