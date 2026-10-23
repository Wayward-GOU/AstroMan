/* JSON / SQL 生成实体类：Java、C#、Go、TypeScript */

export type EntityTarget = 'java' | 'csharp' | 'go' | 'ts'

interface FieldType {
  kind: 'string' | 'number' | 'integer' | 'boolean' | 'object' | 'array' | 'any'
  className?: string
  item?: FieldType
  nullable?: boolean
}

interface ModelField {
  name: string
  type: FieldType
}

interface ModelClass {
  name: string
  fields: ModelField[]
}

function pascalCase(name: string): string {
  return name
    .replace(/[^A-Za-z0-9]+/g, ' ')
    .trim()
    .split(/\s+/)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join('') || 'Field'
}

function camelCase(name: string): string {
  const pascal = pascalCase(name)
  return pascal.charAt(0).toLowerCase() + pascal.slice(1)
}

function snakeCase(name: string): string {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase() || 'field'
}

function uniqueName(base: string, used: Set<string>): string {
  let name = base
  let index = 2
  while (used.has(name)) {
    name = `${base}${index}`
    index += 1
  }
  used.add(name)
  return name
}

function inferType(value: unknown, fieldName: string, classes: ModelClass[], used: Set<string>): FieldType {
  if (value === null || value === undefined) {
    return { kind: 'any', nullable: true }
  }
  if (Array.isArray(value)) {
    const first = value.find((item) => item !== null && item !== undefined)
    return {
      kind: 'array',
      item: first === undefined ? { kind: 'any' } : inferType(first, fieldName, classes, used),
    }
  }
  if (typeof value === 'object') {
    const className = uniqueName(pascalCase(fieldName), used)
    const model: ModelClass = { name: className, fields: [] }
    const innerUsed = new Set<string>()
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
      const name = uniqueName(camelCase(key), innerUsed)
      model.fields.push({ name, type: inferType(item, key, classes, used) })
    })
    classes.push(model)
    return { kind: 'object', className }
  }
  if (typeof value === 'boolean') return { kind: 'boolean' }
  if (typeof value === 'number') {
    return Number.isInteger(value) ? { kind: 'integer' } : { kind: 'number' }
  }
  if (typeof value === 'string') return { kind: 'string' }
  return { kind: 'any' }
}

function buildModel(value: unknown, rootName: string): { root: ModelClass; classes: ModelClass[] } {
  const classes: ModelClass[] = []
  const used = new Set<string>()
  const root: ModelClass = { name: pascalCase(rootName), fields: [] }

  if (Array.isArray(value)) {
    const first = value.find((item) => item !== null && typeof item === 'object')
    if (first) {
      const itemType = inferType(first, rootName, classes, used)
      if (itemType.kind === 'array') {
        root.fields.push({ name: 'items', type: itemType })
      } else {
        root.fields.push({ name: 'items', type: { kind: 'array', item: itemType } })
      }
    } else {
      root.fields.push({ name: 'items', type: { kind: 'array', item: { kind: 'any' } } })
    }
    return { root, classes }
  }

  if (value === null || typeof value !== 'object') {
    root.fields.push({ name: 'value', type: inferType(value, 'value', classes, used) })
    return { root, classes }
  }

  const innerUsed = new Set<string>()
  Object.entries(value as Record<string, unknown>).forEach(([key, item]) => {
    const name = uniqueName(camelCase(key), innerUsed)
    root.fields.push({ name, type: inferType(item, key, classes, used) })
  })
  return { root, classes }
}

function javaType(type: FieldType): string {
  switch (type.kind) {
    case 'string':
      return 'String'
    case 'integer':
      return 'Long'
    case 'number':
      return 'Double'
    case 'boolean':
      return 'Boolean'
    case 'object':
      return type.className ?? 'Object'
    case 'array':
      return `List<${javaType(type.item ?? { kind: 'any' })}>`
    default:
      return 'Object'
  }
}

export function jsonToJava(
  text: string,
  options: { rootName?: string; packageName?: string; lombok?: boolean; comments?: boolean },
): string {
  const { root, classes } = buildModel(JSON.parse(text || '{}'), options.rootName || 'Root')
  const lines: string[] = []
  if (options.packageName) {
    lines.push(`package ${options.packageName};`, '')
  }
  const hasList = [root, ...classes].some((model) =>
    model.fields.some((field) => field.type.kind === 'array'),
  )
  if (hasList) lines.push('import java.util.List;', '')
  if (options.lombok) {
    lines.push(
      'import lombok.Data;',
      'import lombok.NoArgsConstructor;',
      'import lombok.AllArgsConstructor;',
      '',
    )
  }

  const render = (model: ModelClass) => {
    if (options.lombok) {
      lines.push('@Data', '@NoArgsConstructor', '@AllArgsConstructor')
    }
    lines.push(`public class ${model.name} {`)
    model.fields.forEach((field) => {
      if (options.comments) lines.push(`  /** ${field.name} */`)
      lines.push(`  private ${javaType(field.type)} ${field.name};`)
      if (!options.lombok) lines.push('')
    })
    if (!options.lombok) {
      model.fields.forEach((field) => {
        const type = javaType(field.type)
        const cap = field.name.charAt(0).toUpperCase() + field.name.slice(1)
        lines.push(`  public ${type} get${cap}() {`, `    return ${field.name};`, '  }', '')
        lines.push(
          `  public void set${cap}(${type} ${field.name}) {`,
          `    this.${field.name} = ${field.name};`,
          '  }',
          '',
        )
      })
    }
    lines.push('}', '')
  }

  render(root)
  classes.forEach(render)
  return lines.join('\n').replace(/\n{3,}/g, '\n\n').trim()
}

function csharpType(type: FieldType): string {
  switch (type.kind) {
    case 'string':
      return 'string'
    case 'integer':
      return 'long'
    case 'number':
      return 'double'
    case 'boolean':
      return 'bool'
    case 'object':
      return type.className ?? 'object'
    case 'array':
      return `List<${csharpType(type.item ?? { kind: 'any' })}>`
    default:
      return 'object'
  }
}

export function jsonToCsharp(
  text: string,
  options: { rootName?: string; namespace?: string },
): string {
  const { root, classes } = buildModel(JSON.parse(text || '{}'), options.rootName || 'Root')
  const lines: string[] = []
  if (options.namespace) {
    lines.push(`namespace ${options.namespace};`, '')
  }
  if (classes.some((model) => model.fields.some((field) => field.type.kind === 'array'))) {
    lines.push('using System.Collections.Generic;', '')
  }
  const render = (model: ModelClass) => {
    lines.push(`public class ${model.name}`, '{')
    model.fields.forEach((field) => {
      lines.push(`    public ${csharpType(field.type)} ${pascalCase(field.name)} { get; set; }`)
    })
    lines.push('}', '')
  }
  render(root)
  classes.forEach(render)
  return lines.join('\n').trim()
}

function goType(type: FieldType): string {
  switch (type.kind) {
    case 'string':
      return 'string'
    case 'integer':
      return 'int64'
    case 'number':
      return 'float64'
    case 'boolean':
      return 'bool'
    case 'object':
      return type.className ?? 'interface{}'
    case 'array':
      return `[]${goType(type.item ?? { kind: 'any' })}`
    default:
      return 'interface{}'
  }
}

export function jsonToGo(text: string, options: { rootName?: string; packageName?: string }): string {
  const { root, classes } = buildModel(JSON.parse(text || '{}'), options.rootName || 'Root')
  const lines: string[] = [`package ${options.packageName || 'main'}`, '']
  const render = (model: ModelClass) => {
    lines.push(`type ${model.name} struct {`)
    model.fields.forEach((field) => {
      const jsonName = field.name
      lines.push(`\t${pascalCase(field.name)} ${goType(field.type)} \`json:"${jsonName}"\``)
    })
    lines.push('}', '')
  }
  render(root)
  classes.forEach(render)
  return lines.join('\n').trim()
}

function tsType(type: FieldType): string {
  switch (type.kind) {
    case 'string':
      return 'string'
    case 'integer':
    case 'number':
      return 'number'
    case 'boolean':
      return 'boolean'
    case 'object':
      return type.className ?? 'Record<string, unknown>'
    case 'array':
      return `${tsType(type.item ?? { kind: 'any' })}[]`
    default:
      return 'unknown'
  }
}

export function jsonToTs(text: string, options: { rootName?: string }): string {
  const { root, classes } = buildModel(JSON.parse(text || '{}'), options.rootName || 'Root')
  const lines: string[] = []
  const render = (model: ModelClass) => {
    lines.push(`export interface ${model.name} {`)
    model.fields.forEach((field) => {
      lines.push(`  ${field.name}: ${tsType(field.type)}`)
    })
    lines.push('}', '')
  }
  render(root)
  classes.forEach(render)
  return lines.join('\n').trim()
}

export function jsonToEntity(
  target: EntityTarget,
  text: string,
  options: {
    rootName?: string
    packageName?: string
    namespace?: string
    lombok?: boolean
    comments?: boolean
  },
): string {
  if (!text.trim()) throw new Error('请输入 JSON 内容')
  try {
    JSON.parse(text)
  } catch (error) {
    throw new Error(`JSON 解析失败：${error instanceof Error ? error.message : String(error)}`)
  }
  switch (target) {
    case 'java':
      return jsonToJava(text, options)
    case 'csharp':
      return jsonToCsharp(text, options)
    case 'go':
      return jsonToGo(text, options)
    default:
      return jsonToTs(text, options)
  }
}

/* ---------------- SQL CREATE TABLE → Java ---------------- */

const SQL_JAVA_TYPES: Array<[RegExp, string]> = [
  [/^(bigint|int8|bigserial)/i, 'Long'],
  [/^(int|integer|int4|serial|mediumint|smallint|tinyint)/i, 'Integer'],
  [/^(decimal|numeric|money)/i, 'BigDecimal'],
  [/^(double|float|real)/i, 'Double'],
  [/^(bit|bool)/i, 'Boolean'],
  [/^(date|timestamp|datetime|time)/i, 'Date'],
  [/^(text|varchar|char|nvarchar|json|jsonb|longtext|mediumtext|clob|blob|bytea)/i, 'String'],
]

function sqlTypeToJava(type: string): string {
  for (const [pattern, java] of SQL_JAVA_TYPES) {
    if (pattern.test(type)) return java
  }
  return 'String'
}

export function sqlToJava(
  sql: string,
  options: { packageName?: string; lombok?: boolean; tableName?: string },
): string {
  const tableMatch = /create\s+table\s+(?:if\s+not\s+exists\s+)?[`"']?([\w.]+)[`"']?\s*\(([\s\S]*?)\)\s*(?:engine|comment|;|$)/i.exec(
    sql,
  )
  if (!tableMatch) throw new Error('未识别到 CREATE TABLE 语句')
  const tableName = options.tableName || tableMatch[1].split('.').pop() || 'Entity'
  const body = tableMatch[2]
  const columns: Array<{ name: string; type: string; comment?: string }> = []
  const segments = body.split(/,\s*(?=(?:[^`'"]|`[^`]*`|'[^']*'|"[^"]*")*$)/)
  segments.forEach((segment) => {
    const text = segment.trim()
    if (!text) return
    if (/^(primary|unique|key|index|constraint|foreign|check)\b/i.test(text)) return
    const match = /^[`"']?([\w]+)[`"']?\s+([A-Za-z]+(?:\s*\([^)]*\))?)/.exec(text)
    if (!match) return
    const commentMatch = /comment\s+'([^']*)'/i.exec(text)
    columns.push({
      name: match[1],
      type: match[2],
      comment: commentMatch?.[1],
    })
  })
  if (!columns.length) throw new Error('未解析到字段定义')

  const className = pascalCase(tableName)
  const lines: string[] = []
  if (options.packageName) lines.push(`package ${options.packageName};`, '')
  const imports = new Set<string>()
  columns.forEach((column) => {
    const java = sqlTypeToJava(column.type)
    if (java === 'BigDecimal') imports.add('java.math.BigDecimal')
    if (java === 'Date') imports.add('java.util.Date')
  })
  if (imports.size) {
    imports.forEach((item) => lines.push(`import ${item};`))
    lines.push('')
  }
  if (options.lombok) {
    lines.push('import lombok.Data;', '')
    lines.push('@Data')
  }
  lines.push(`public class ${className} {`, '')
  const used = new Set<string>()
  columns.forEach((column) => {
    const fieldName = uniqueName(camelCase(column.name), used)
    if (column.comment) lines.push(`  /** ${column.comment} */`)
    lines.push(`  private ${sqlTypeToJava(column.type)} ${fieldName};`, '')
  })
  lines.push('}')
  return lines.join('\n')
}
