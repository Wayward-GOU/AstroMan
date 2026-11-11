export type DocScope = 'request' | 'collection' | 'project'
export type DocFormat = 'markdown' | 'html'

export interface DocsOptions {
  includeEnvironment: boolean
  includeScripts: boolean
  includeAssertions: boolean
  includeResponseExample: boolean
  resolveEnvironment: boolean
  useRunExamples: boolean
  autoExamples: boolean
  runExamplesMaxAgeDays: number
  pageBreakPerRequest: boolean
  includeGeneratedAt: boolean
}

export const defaultDocsOptions = (): DocsOptions => ({
  includeEnvironment: true,
  includeScripts: false,
  includeAssertions: false,
  includeResponseExample: true,
  resolveEnvironment: true,
  useRunExamples: true,
  autoExamples: true,
  runExamplesMaxAgeDays: 30,
  pageBreakPerRequest: true,
  includeGeneratedAt: false,
})

export interface DocOutlineField {
  key: string
  name: string
  location: string
  value: string
  source: string
  sensitive: boolean
}

export interface DocOutlineRequest {
  key: string
  name: string
  method: string
  url: string
  description: string
  included: boolean
  params: DocOutlineField[]
  warnings: string[]
}

export interface DocOutlineGroup {
  key: string
  name: string
  path: string[]
  requests: DocOutlineRequest[]
  children: DocOutlineGroup[]
}

export interface DocOutline {
  groups: DocOutlineGroup[]
}

export interface DocNodeEdit {
  description?: string
  exampleValues: Record<string, string>
}

export interface DocEditOverlay {
  revision: string
  meta: { title?: string; subtitle?: string }
  nodes: Record<string, DocNodeEdit>
  excluded: string[]
  order: Record<string, string[]>
}

export interface DocMetaDefaults {
  title: string
  subtitle: string
}

export interface DocsPreparePayload {
  scope: DocScope
  targetId: number
  environmentId: number | null
  options: DocsOptions
}

export interface DocsPrepareResult {
  draftId: string
  revision: string
  meta: DocMetaDefaults
  outline: DocOutline
  warnings: string[]
}

export interface DocsRenderPayload {
  draftId: string
  overlay: DocEditOverlay
  format: DocFormat
}

export interface DocsRenderResult {
  content: string
  bytes: number
  warnings: string[]
}

export type DocsExportSource =
  | {
      kind: 'scope'
      scope: DocScope
      targetId: number
      environmentId: number | null
      options: DocsOptions
    }
  | { kind: 'draft'; draftId: string; overlay: DocEditOverlay }

export interface DocsExportPayload {
  format: DocFormat
  path: string
  source: DocsExportSource
}

export interface DocsSiteStartPayload {
  projectId: number
  environmentId: number | null
  /** 为空表示由系统分配空闲端口 */
  port: number | null
  /** 仅支持 127.0.0.1 / 0.0.0.0 */
  host: string | null
}

export interface DocsSiteStatus {
  running: boolean
  url: string
  host: string
  port: number
  lan: boolean
  /** 局域网模式下的访问令牌 */
  token: string | null
  projectId: number | null
  environmentId: number | null
  requestCount: number
  pageCount: number
  startedAt: number
  warnings: string[]
}

export interface DocsExportResult {
  path: string
  format: string
  scope: string
  requestCount: number
  collectionCount: number
  bytes: number
  warnings: string[]
  pdfEngine: string | null
  fallbackHtmlPath: string | null
}
