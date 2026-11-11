import type { CaptureFlow } from '@/types/capture'

/** 路径树中的一个目录节点（域名是根，下面按 / 分段） */
export interface CaptureTreeNode {
  key: string
  name: string
  /** 不含查询串的完整路径前缀，例如 /v1/users */
  path: string
  /** 路径恰好停在这里的请求 */
  flows: CaptureFlow[]
  children: CaptureTreeNode[]
  /** 本节点及所有子节点的请求数 */
  count: number
  /** 本节点及所有子节点的错误数（网络错误或 5xx） */
  errors: number
}

/** 一个域名根节点 */
export interface CaptureHostTree {
  key: string
  host: string
  /** 路径为 / 的请求直接挂在域名下 */
  flows: CaptureFlow[]
  children: CaptureTreeNode[]
  count: number
  errors: number
  /** 该域名最近一条请求的状态码，用于根节点右侧提示 */
  lastStatus: number | null
  /** 首次出现顺序，保持与抓包时间线一致 */
  firstId: number
}

interface BuildNode {
  key: string
  name: string
  path: string
  flows: CaptureFlow[]
  children: Map<string, BuildNode>
}

interface BuildHost extends BuildNode {
  host: string
  firstId: number
  lastId: number
  lastStatus: number | null
}

/** 把 /a/b?x=1 拆成路径与查询串 */
export function splitFlowPath(path: string): { pathname: string; query: string } {
  const index = path.indexOf('?')
  if (index < 0) return { pathname: path || '/', query: '' }
  return { pathname: path.slice(0, index) || '/', query: path.slice(index + 1) }
}

/** 路径分段：忽略空段，/ 返回空数组 */
export function capturePathSegments(pathname: string): string[] {
  return pathname.split('/').filter(Boolean)
}

function isError(flow: CaptureFlow): boolean {
  return !!flow.error || (flow.status ?? 0) >= 500
}

function finalizeNode(node: BuildNode): CaptureTreeNode {
  node.flows.sort((a, b) => a.id - b.id)
  const children = [...node.children.values()]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(finalizeNode)

  let count = node.flows.length
  let errors = node.flows.filter(isError).length
  for (const child of children) {
    count += child.count
    errors += child.errors
  }
  return {
    key: node.key,
    name: node.name,
    path: node.path,
    flows: node.flows,
    children,
    count,
    errors,
  }
}

/**
 * 把流量列表整理成「域名 → 路径分段」的树。
 * 同一路径的请求保留在对应节点；子目录按名称排序，请求按抓包顺序。
 */
export function buildCaptureTree(flows: CaptureFlow[]): CaptureHostTree[] {
  const hosts = new Map<string, BuildHost>()

  for (const flow of flows) {
    let host = hosts.get(flow.host)
    if (!host) {
      host = {
        key: `h:${flow.host}`,
        host: flow.host,
        name: flow.host,
        path: '/',
        flows: [],
        children: new Map(),
        firstId: flow.id,
        lastId: flow.id,
        lastStatus: flow.status,
      }
      hosts.set(flow.host, host)
    }
    host.firstId = Math.min(host.firstId, flow.id)
    if (flow.id >= host.lastId) {
      host.lastId = flow.id
      host.lastStatus = flow.status
    }

    const { pathname } = splitFlowPath(flow.path)
    const segments = capturePathSegments(pathname)
    let parent: BuildNode = host
    let prefix = ''
    for (const segment of segments) {
      prefix += `/${segment}`
      let node = parent.children.get(segment)
      if (!node) {
        node = {
          key: `d:${flow.host}:${prefix}`,
          name: segment,
          path: prefix,
          flows: [],
          children: new Map(),
        }
        parent.children.set(segment, node)
      }
      parent = node
    }
    parent.flows.push(flow)
  }

  return [...hosts.values()]
    .sort((a, b) => a.firstId - b.firstId)
    .map((host) => {
      host.flows.sort((a, b) => a.id - b.id)
      const children = [...host.children.values()]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map(finalizeNode)
      let count = host.flows.length
      let errors = host.flows.filter(isError).length
      for (const child of children) {
        count += child.count
        errors += child.errors
      }
      return {
        key: host.key,
        host: host.host,
        flows: host.flows,
        children,
        count,
        errors,
        lastStatus: host.lastStatus,
        firstId: host.firstId,
      }
    })
}

/** 展开全部 / 折叠全部用的稳定键：域名 + 每一级目录 */
export function captureTreeKeys(tree: CaptureHostTree[]): string[] {
  const keys: string[] = []
  const walk = (nodes: CaptureTreeNode[]) => {
    for (const node of nodes) {
      keys.push(node.key)
      walk(node.children)
    }
  }
  for (const host of tree) {
    keys.push(host.key)
    walk(host.children)
  }
  return keys
}

/** 抓包树各类行的固定高度（px）；域名行含 1px 分隔线 */
export const CAPTURE_ROW_HEIGHT = { host: 29, folder: 25, flow: 27 } as const

export interface CaptureWindowItem<T> {
  row: T
  /** 该行在整张列表里的 y 偏移，用于平铺渲染 */
  top: number
}

export interface CaptureWindow<T> {
  items: CaptureWindowItem<T>[]
  /** 窗口第一行的 y 偏移，作为 translateY 使用 */
  top: number
  /** 整张列表的总高度，用于撑开滚动条 */
  height: number
}

/**
 * 虚拟滚动：按固定行高算出当前视口附近要渲染的行。
 *
 * 行高固定（域名 29 / 目录 25 / 请求 27），所以只需一次累计即可定位窗口，
 * 5000 条流量也只渲染几十行 DOM。`overscan` 是上下额外保留的行数，
 * 避免快速滚动时出现空白。
 */
export function captureTreeWindow<T extends { kind: keyof typeof CAPTURE_ROW_HEIGHT }>(
  rows: T[],
  scrollTop: number,
  viewportHeight: number,
  overscan = 8,
): CaptureWindow<T> {
  if (!rows.length) return { items: [], top: 0, height: 0 }

  const tops: number[] = new Array(rows.length)
  let height = 0
  for (let index = 0; index < rows.length; index += 1) {
    tops[index] = height
    height += CAPTURE_ROW_HEIGHT[rows[index].kind]
  }

  const overscanPx = Math.max(0, overscan) * CAPTURE_ROW_HEIGHT.flow
  const maxTop = Math.max(0, height - Math.max(0, viewportHeight))
  const scrolled = Math.min(Math.max(0, scrollTop), maxTop)
  const startY = Math.max(0, scrolled - overscanPx)
  const endY = scrolled + Math.max(0, viewportHeight) + overscanPx

  // 第一行：底边超过 startY；最后一行：顶边仍小于 endY
  let low = 0
  let high = rows.length
  while (low < high) {
    const mid = (low + high) >> 1
    if (tops[mid] + CAPTURE_ROW_HEIGHT[rows[mid].kind] <= startY) low = mid + 1
    else high = mid
  }
  const first = low
  low = 0
  high = rows.length
  while (low < high) {
    const mid = (low + high) >> 1
    if (tops[mid] < endY) low = mid + 1
    else high = mid
  }
  const last = low - 1
  if (first > last || first >= rows.length) return { items: [], top: 0, height }

  const items = rows.slice(first, last + 1).map((row, offset) => ({
    row,
    top: tops[first + offset],
  }))
  return { items, top: tops[first], height }
}

/** 请求叶子的显示名：最后一段路径，查询串保留用于区分同路径的多次请求 */
export function captureFlowLabel(flow: CaptureFlow): string {
  const { pathname, query } = splitFlowPath(flow.path)
  const segments = capturePathSegments(pathname)
  const name = segments.length ? segments[segments.length - 1] : '/'
  return query ? `${name}?${query}` : name
}
