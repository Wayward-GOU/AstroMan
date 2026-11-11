// 接口实体（请求 / 返回字段树）的前端工具
//
// 与后端 `entity::model` 保持同一套规则：
// - 路径用点号表示嵌套，数组节点自身带 `[]`（`data.roles[]`，元素字段 `data.roles[].name`）；
// - 根节点 `$` 不参与路径；
// - 只处理字段结构，不携带任何取值。

import type { EntityDoc, EntityNode } from '@/types/models'

export const ENTITY_VERSION = 1

/** 可新增字段的类型；自动推断出的类型只读，这里仅用于手工新增 */
export const ENTITY_TYPES = [
  'string',
  'number',
  'boolean',
  'object',
  'array',
  'file',
  'null',
  'unknown',
] as const

export interface EntityRow {
  /** 字段路径，同时作为行 key */
  path: string
  node: EntityNode
  depth: number
  /** 展示用类型：数组折叠为元素类型，空容器带中文标注 */
  typeLabel: string
}

export function createEntityNode(name = '', typeName = 'string'): EntityNode {
  return {
    name,
    type: typeName,
    itemType: '',
    required: false,
    description: '',
    manual: false,
    stale: false,
    children: [],
  }
}

export function createEmptyEntity(rootType = 'object'): EntityDoc {
  return {
    version: ENTITY_VERSION,
    updatedAt: 0,
    hiddenPaths: [],
    root: {
      ...createEntityNode('$', rootType),
      required: false,
    },
  }
}

/** 宽松解析：损坏或版本不认识时按「无实体」处理，不抛错 */
export function parseEntityDoc(raw?: string | null): EntityDoc | null {
  if (!raw || !raw.trim()) return null
  try {
    const parsed = JSON.parse(raw) as Partial<EntityDoc> | null
    if (!parsed || typeof parsed !== 'object') return null
    if (parsed.version !== ENTITY_VERSION) return null
    const root = parsed.root
    if (!root || typeof root !== 'object' || !Array.isArray(root.children)) return null
    return {
      version: ENTITY_VERSION,
      updatedAt: Number(parsed.updatedAt) || 0,
      hiddenPaths: Array.isArray(parsed.hiddenPaths)
        ? parsed.hiddenPaths.filter((item): item is string => typeof item === 'string')
        : [],
      root: normalizeNode(root as EntityNode, '$'),
    }
  } catch {
    return null
  }
}

function normalizeNode(node: EntityNode, fallbackName: string): EntityNode {
  return {
    name: typeof node.name === 'string' && node.name ? node.name : fallbackName,
    type: typeof node.type === 'string' && node.type ? node.type : 'string',
    itemType: typeof node.itemType === 'string' ? node.itemType : '',
    required: node.required === true,
    description: typeof node.description === 'string' ? node.description : '',
    manual: node.manual === true,
    stale: node.stale === true,
    children: Array.isArray(node.children)
      ? node.children.map((child) => normalizeNode(child, 'field'))
      : [],
  }
}

export function serializeEntityDoc(doc: EntityDoc): string {
  return JSON.stringify(doc)
}

/** 深拷贝，避免编辑器改动直接污染 form 里的原对象 */
export function cloneEntityDoc(doc: EntityDoc): EntityDoc {
  return JSON.parse(JSON.stringify(doc)) as EntityDoc
}

/** 与后端 flatten 保持一致的展示类型 */
export function entityTypeLabel(node: EntityNode): string {
  if (node.type === 'array') {
    if (node.itemType) return node.itemType
    if (node.children.length === 0) return 'array（空数组）'
    return 'array'
  }
  if (node.type === 'object' && node.children.length === 0) return 'object（空对象）'
  return node.type
}

/** 拼接字段路径：父路径为空 / `$` 时取字段名；数组节点自身带 `[]` */
export function entityPath(parentPath: string, node: EntityNode): string {
  const base = !parentPath || parentPath === '$' ? node.name : `${parentPath}.${node.name}`
  return node.type === 'array' ? `${base}[]` : base
}

export function flattenEntityRows(doc: EntityDoc): EntityRow[] {
  const rows: EntityRow[] = []
  const walk = (parent: EntityNode, parentPath: string, depth: number) => {
    for (const child of parent.children) {
      const path = entityPath(parentPath, child)
      rows.push({ path, node: child, depth, typeLabel: entityTypeLabel(child) })
      walk(child, path, depth + 1)
    }
  }
  walk(doc.root, '', 0)
  return rows
}

export function findEntityNode(doc: EntityDoc, path: string): EntityNode | null {
  if (!path || path === '$') return doc.root
  const walk = (parent: EntityNode, parentPath: string): EntityNode | null => {
    for (const child of parent.children) {
      const childPath = entityPath(parentPath, child)
      if (childPath === path) return child
      const hit = walk(child, childPath)
      if (hit) return hit
    }
    return null
  }
  return walk(doc.root, '')
}

export function countEntityFields(doc: EntityDoc): number {
  let total = 0
  const walk = (node: EntityNode) => {
    for (const child of node.children) {
      total += 1
      walk(child)
    }
  }
  walk(doc.root)
  return total
}

/** 「最近一次发送未出现」的字段数量，用于工具栏批量清理入口 */
export function countStaleFields(doc: EntityDoc): number {
  let total = 0
  const walk = (node: EntityNode) => {
    for (const child of node.children) {
      if (child.stale) total += 1
      walk(child)
    }
  }
  walk(doc.root)
  return total
}

function removeHiddenPrefix(doc: EntityDoc, path: string) {
  doc.hiddenPaths = doc.hiddenPaths.filter(
    (item) => item !== path && !item.startsWith(`${path}.`) && !item.startsWith(`${path}[]`),
  )
}

/**
 * 在父节点下新增手工字段。
 *
 * `parentPath` 为 `$`（或空）时加到根；数组元素字段用 `roles[]` 作为父路径。
 * 同层重名直接返回 null，由调用方提示。
 */
export function addEntityField(
  doc: EntityDoc,
  parentPath: string,
  name: string,
  typeName = 'string',
): EntityNode | null {
  const field = name.trim()
  if (!field) return null
  const parent = findEntityNode(doc, parentPath || '$')
  if (!parent) return null
  if (parent.children.some((item) => item.name === field)) return null
  const node: EntityNode = { ...createEntityNode(field, typeName), manual: true }
  parent.children.push(node)
  removeHiddenPrefix(doc, entityPath(parentPath || '$', node))
  return node
}

/**
 * 删除字段：
 * - 自动推断的字段把路径记入 `hiddenPaths`，后续同步不再加回；
 * - 手工新增的字段直接移除，不留隐藏记录。
 */
export function removeEntityField(doc: EntityDoc, path: string): boolean {
  const walk = (parent: EntityNode, parentPath: string): boolean => {
    const index = parent.children.findIndex((child) => entityPath(parentPath, child) === path)
    if (index >= 0) {
      const [removed] = parent.children.splice(index, 1)
      if (!removed.manual && !doc.hiddenPaths.includes(path)) {
        doc.hiddenPaths.push(path)
      }
      doc.hiddenPaths = doc.hiddenPaths.filter(
        (item) => !item.startsWith(`${path}.`) && !item.startsWith(`${path}[]`),
      )
      return true
    }
    return parent.children.some((child) => walk(child, entityPath(parentPath, child)))
  }
  return walk(doc.root, '')
}

/** 清理所有「最近一次发送未出现」的字段，返回删除数量 */
export function clearStaleFields(doc: EntityDoc): number {
  let removed = 0
  const walk = (parent: EntityNode, parentPath: string) => {
    parent.children = parent.children.filter((child) => {
      if (!child.stale) {
        walk(child, entityPath(parentPath, child))
        return true
      }
      const path = entityPath(parentPath, child)
      if (!child.manual && !doc.hiddenPaths.includes(path)) doc.hiddenPaths.push(path)
      removed += 1
      return false
    })
  }
  walk(doc.root, '')
  return removed
}

/** 改写必填后标记为手工，自动同步不再覆盖 */
export function markEntityRequired(node: EntityNode, required: boolean) {
  node.required = required
  node.manual = true
}

/**
 * 把本地未保存的编辑合并进后端刚返回的实体：
 * - 说明以本地为准（用户正在填的内容不会因为一次发送被冲掉）；
 * - 手工改过必填的节点保留required；
 * - 本地手工新增、后端还没有的字段补回去；
 * - 隐藏路径取并集（本地刚删掉、还没同步到数据库的字段不会复活）。
 *
 * `keepHidden = false` 用于「重新生成」：清空隐藏记录，被删过的字段允许回归。
 */
export function mergeEntityEdits(
  base: EntityDoc | null,
  local: EntityDoc | null,
  keepHidden = true,
): EntityDoc | null {
  if (!base) return local ? cloneEntityDoc(local) : null
  if (!local) return base
  const result = cloneEntityDoc(base)
  const hidden = new Set(keepHidden ? [...result.hiddenPaths, ...local.hiddenPaths] : result.hiddenPaths)
  result.hiddenPaths = [...hidden]
  const walk = (target: EntityNode, source: EntityNode, parentPath: string) => {
    target.children = target.children.filter(
      (child) => !hidden.has(entityPath(parentPath, child)),
    )
    for (const sourceChild of source.children) {
      const path = entityPath(parentPath, sourceChild)
      const hit = target.children.find((child) => child.name === sourceChild.name)
      if (hit) {
        hit.description = sourceChild.description
        if (sourceChild.manual) {
          hit.manual = true
          hit.required = sourceChild.required
        }
        walk(hit, sourceChild, path)
      } else if (sourceChild.manual && !hidden.has(path)) {
        const copy: EntityNode = JSON.parse(JSON.stringify(sourceChild))
        copy.stale = false
        target.children.push(copy)
      }
    }
  }
  if (result.root.type === local.root.type) {
    walk(result.root, local.root, '$')
  }
  return result
}

