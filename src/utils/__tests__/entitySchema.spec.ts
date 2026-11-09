import { describe, expect, it } from 'vitest'
import {
  addEntityField,
  clearStaleFields,
  countEntityFields,
  countStaleFields,
  createEmptyEntity,
  entityPath,
  entityTypeLabel,
  findEntityNode,
  flattenEntityRows,
  markEntityRequired,
  mergeEntityEdits,
  parseEntityDoc,
  removeEntityField,
  serializeEntityDoc,
} from '@/utils/entitySchema'
import type { EntityNode } from '@/types/models'

function node(name: string, type = 'string', children: EntityNode[] = []): EntityNode {
  return {
    name,
    type,
    itemType: '',
    required: true,
    description: '',
    manual: false,
    stale: false,
    children,
  }
}

function docWith(children: EntityNode[]) {
  const doc = createEmptyEntity()
  doc.root.children = children
  return doc
}

describe('entitySchema', () => {
  it('解析损坏或版本不认识的实体时返回 null', () => {
    expect(parseEntityDoc('')).toBeNull()
    expect(parseEntityDoc('{')).toBeNull()
    expect(
      parseEntityDoc(
        '{"version":2,"updatedAt":0,"hiddenPaths":[],"root":{"name":"$","type":"object","children":[]}}',
      ),
    ).toBeNull()
    expect(
      parseEntityDoc(
        '{"version":1,"updatedAt":0,"hiddenPaths":[],"root":{"name":"$","type":"object","children":[]}}',
      ),
    ).not.toBeNull()
  })

  it('序列化后可以原样解析回来', () => {
    const doc = docWith([node('id', 'number'), node('roles', 'array')])
    const parsed = parseEntityDoc(serializeEntityDoc(doc))
    expect(parsed).toEqual(doc)
  })

  it('路径与数组后缀与后端一致', () => {
    const roles = node('roles', 'array', [node('name')])
    const doc = docWith([node('data', 'object', [roles])])
    expect(entityPath('$', node('id'))).toBe('id')
    expect(entityPath('data', roles)).toBe('data.roles[]')
    expect(entityPath('data.roles[]', node('name'))).toBe('data.roles[].name')

    const rows = flattenEntityRows(doc)
    expect(rows.map((row) => [row.path, row.depth])).toEqual([
      ['data', 0],
      ['data.roles[]', 1],
      ['data.roles[].name', 2],
    ])
  })

  it('展示类型折叠数组元素并标注空容器', () => {
    const scalarArray = { ...node('tags', 'array'), itemType: 'string' }
    expect(entityTypeLabel(scalarArray)).toBe('string')
    expect(entityTypeLabel(node('list', 'array'))).toBe('array（空数组）')
    expect(entityTypeLabel(node('obj', 'object'))).toBe('object（空对象）')
    expect(entityTypeLabel(node('plain', 'number'))).toBe('number')
  })

  it('新增字段标记手工并在父节点下追加', () => {
    const doc = docWith([node('data', 'object')])
    const added = addEntityField(doc, 'data', 'extra', 'number')
    expect(added?.manual).toBe(true)
    expect(added?.required).toBe(false)
    expect(findEntityNode(doc, 'data.extra')).toBe(added)

    const atRoot = addEntityField(doc, '$', 'traceId')
    expect(atRoot?.name).toBe('traceId')
    expect(countEntityFields(doc)).toBe(3)

    expect(addEntityField(doc, 'data', 'extra')).toBeNull()
    expect(addEntityField(doc, 'data', '  ')).toBeNull()
  })

  it('删除自动字段记入隐藏路径，删除手工字段不留记录', () => {
    const doc = docWith([node('debug', 'object'), node('custom', 'string')])
    doc.root.children[1].manual = true

    expect(removeEntityField(doc, 'debug')).toBe(true)
    expect(doc.hiddenPaths).toEqual(['debug'])
    expect(findEntityNode(doc, 'debug')).toBeNull()

    expect(removeEntityField(doc, 'custom')).toBe(true)
    expect(doc.hiddenPaths).toEqual(['debug'])

    expect(removeEntityField(doc, 'missing')).toBe(false)
  })

  it('删除父字段时清掉子字段的隐藏记录，重新新增会取消隐藏', () => {
    const doc = docWith([node('data', 'object', [node('id', 'number')])])
    doc.hiddenPaths = ['data.id']
    removeEntityField(doc, 'data')
    expect(doc.hiddenPaths).toEqual(['data'])

    addEntityField(doc, '$', 'data', 'object')
    expect(doc.hiddenPaths).toEqual([])
    expect(findEntityNode(doc, 'data')?.manual).toBe(true)
  })

  it('清理过期字段只删除 stale 节点并记录隐藏路径', () => {
    const stale = node('legacy', 'string')
    stale.stale = true
    const nestedStale = node('removed', 'number')
    nestedStale.stale = true
    const doc = docWith([node('data', 'object', [node('id', 'number'), nestedStale]), stale])

    expect(countStaleFields(doc)).toBe(2)
    expect(clearStaleFields(doc)).toBe(2)
    expect(findEntityNode(doc, 'legacy')).toBeNull()
    expect(findEntityNode(doc, 'data.removed')).toBeNull()
    expect(findEntityNode(doc, 'data.id')).not.toBeNull()
    expect(doc.hiddenPaths.sort()).toEqual(['data.removed', 'legacy'])
  })

  it('改写必填会标记手工，自动同步不再覆盖', () => {
    const field = node('id', 'number')
    markEntityRequired(field, false)
    expect(field.required).toBe(false)
    expect(field.manual).toBe(true)
  })

  it('发送后刷新时保留本地未保存的说明、手工字段与删除', () => {
    const remote = docWith([
      node('id', 'number'),
      node('name', 'string'),
      node('debug', 'object'),
      node('data', 'object', [node('fresh', 'boolean')]),
    ])
    const local = docWith([
      { ...node('id', 'number'), description: '用户 ID' },
      { ...node('data', 'object', [node('fresh', 'boolean')]), description: '业务数据' },
      { ...node('custom', 'string'), manual: true, required: true },
    ])
    local.hiddenPaths = ['debug']
    // 本地把 id 的必填改成否
    local.root.children[0].required = false
    local.root.children[0].manual = true

    const merged = mergeEntityEdits(remote, local)!
    expect(merged.hiddenPaths).toEqual(['debug'])
    expect(findEntityNode(merged, 'debug')).toBeNull()
    expect(findEntityNode(merged, 'id')?.description).toBe('用户 ID')
    expect(findEntityNode(merged, 'id')?.required).toBe(false)
    expect(findEntityNode(merged, 'data')?.description).toBe('业务数据')
    expect(findEntityNode(merged, 'data.fresh')).not.toBeNull()
    expect(findEntityNode(merged, 'custom')?.manual).toBe(true)
    expect(findEntityNode(merged, 'custom')?.required).toBe(true)
  })

  it('没有远端实体时保留本地手工实体', () => {
    const local = docWith([{ ...node('custom', 'string'), manual: true }])
    expect(mergeEntityEdits(null, local)).toEqual(local)
    expect(mergeEntityEdits(null, null)).toBeNull()
  })

  it('重新生成时不再套用本地隐藏路径，删过的字段允许回归', () => {
    const remote = docWith([node('debug', 'object')])
    const local = docWith([])
    local.hiddenPaths = ['debug']
    const merged = mergeEntityEdits(remote, local, false)!
    expect(merged.hiddenPaths).toEqual([])
    expect(findEntityNode(merged, 'debug')).not.toBeNull()
  })
})
