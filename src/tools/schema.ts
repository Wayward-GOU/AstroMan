/* 由 JSON 样例推断 JSON Schema（draft-07 子集） */

type Schema = Record<string, unknown>

const DATE_TIME = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$/
const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/
const URI = /^https?:\/\/\S+$/i
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function inferScalarString(value: string): Schema {
  if (DATE_TIME.test(value)) return { type: 'string', format: 'date-time' }
  if (DATE_ONLY.test(value)) return { type: 'string', format: 'date' }
  if (EMAIL.test(value)) return { type: 'string', format: 'email' }
  if (UUID.test(value)) return { type: 'string', format: 'uuid' }
  if (URI.test(value)) return { type: 'string', format: 'uri' }
  return { type: 'string' }
}

function mergeObjects(list: Schema[]): Schema {
  const propertyLists = list.map((item) => (item.properties ?? {}) as Record<string, Schema>)
  const keys: string[] = []
  propertyLists.forEach((properties) => {
    Object.keys(properties).forEach((key) => {
      if (!keys.includes(key)) keys.push(key)
    })
  })
  const properties: Record<string, Schema> = {}
  keys.forEach((key) => {
    const candidates = propertyLists
      .map((item) => item[key])
      .filter((item): item is Schema => Boolean(item))
    properties[key] = mergeSchemas(candidates)
  })
  const required = keys.filter((key) => propertyLists.every((item) => key in item))
  const schema: Schema = { type: 'object', properties }
  if (required.length) schema.required = required
  return schema
}

function mergeSchemas(list: Schema[]): Schema {
  if (!list.length) return {}
  if (list.length === 1) return list[0]
  const objects = list.filter((item) => item.type === 'object')
  if (objects.length === list.length) return mergeObjects(list)
  const arrays = list.filter((item) => item.type === 'array')
  if (arrays.length === list.length) {
    const items = mergeSchemas(arrays.map((item) => (item.items ?? {}) as Schema).filter((item) => Object.keys(item).length))
    return Object.keys(items).length ? { type: 'array', items } : { type: 'array' }
  }
  const types: string[] = []
  list.forEach((item) => {
    const type = item.type as string | undefined
    if (type && !types.includes(type)) types.push(type)
  })
  if (types.length === 1) return { ...list[0] }
  // integer + number 合并为 number，避免类型爆炸
  const normalized = types.filter((type) => !(type === 'integer' && types.includes('number')))
  const schema: Schema = { type: normalized.length === 1 ? normalized[0] : normalized }
  const formats = Array.from(
    new Set(list.map((item) => item.format).filter((format): format is string => typeof format === 'string')),
  )
  if (formats.length === 1) schema.format = formats[0]
  return schema
}

function infer(value: unknown): Schema {
  if (value === null) return { type: 'null' }
  if (Array.isArray(value)) {
    if (!value.length) return { type: 'array', items: {} }
    return { type: 'array', items: mergeSchemas(value.map(infer)) }
  }
  if (isPlainObject(value)) {
    const properties: Record<string, Schema> = {}
    Object.entries(value).forEach(([key, item]) => {
      properties[key] = infer(item)
    })
    const schema: Schema = { type: 'object', properties }
    const required = Object.keys(properties)
    if (required.length) schema.required = required
    return schema
  }
  if (typeof value === 'string') return inferScalarString(value)
  if (typeof value === 'number') return Number.isInteger(value) ? { type: 'integer' } : { type: 'number' }
  if (typeof value === 'boolean') return { type: 'boolean' }
  return {}
}

export function inferJsonSchema(input: string, options: { title?: string; includeSchemaUrl?: boolean } = {}): string {
  const text = input.trim()
  if (!text) throw new Error('请粘贴 JSON 样例')
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch (error) {
    throw new Error(`JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
  const schema: Schema = {
    ...(options.includeSchemaUrl === false ? {} : { $schema: 'http://json-schema.org/draft-07/schema#' }),
    title: options.title?.trim() || 'InferredSchema',
    ...infer(parsed),
  }
  return JSON.stringify(schema, null, 2)
}
