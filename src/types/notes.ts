export type NoteScope = 'today' | 'week' | 'all' | 'done'
export type NoteStatus = 'open' | 'done' | 'deleted'
export type NoteRepeat = 'none' | 'daily' | 'weekdays' | 'weekly' | 'monthly'
/** 待办：有动作与完成态；笔记：信息记录；灵感：随手想法；资料：链接 / 命令 / 片段 */
export type NoteKind = 'todo' | 'note' | 'idea' | 'reference'

export interface NotesFilter {
  scope: NoteScope
  keyword?: string
  tag?: string
  kind?: NoteKind
  limit?: number
}

export interface NoteCounts {
  open: number
  /** 今天到期且未完成，不含逾期 */
  dueToday: number
  /** 已过截止时间且未完成 */
  overdue: number
  done: number
}

export interface NoteSummary {
  id: number
  title: string
  excerpt: string
  status: NoteStatus
  kind: NoteKind
  priority: number
  tags: string[]
  dueAt: number | null
  remindAt: number | null
  remindLeadMin: number
  repeat: NoteRepeat
  snoozeUntil: number | null
  pinned: boolean
  doneAt: number | null
  updatedAt: number
  attachmentCount: number
}

export interface NoteDetail extends NoteSummary {
  content: string
  createdAt: number
}

export interface NotesListResult {
  items: NoteSummary[]
  counts: NoteCounts
}

export interface TagCount {
  name: string
  count: number
}

export interface ToggleResult {
  note: NoteDetail
  /** 重复条目顺延后的下次截止时间 */
  rolledTo: number | null
}

export interface NoteInput {
  title?: string
  content?: string
  kind?: NoteKind
  priority?: number
  tags?: string[]
  dueAt?: number | null
  remindLeadMin?: number
  repeat?: NoteRepeat
  pinned?: boolean
}

export interface NotePatch {
  title?: string
  content?: string
  kind?: NoteKind
  priority?: number
  tags?: string[]
  /** 缺省 = 不动；null = 清空截止 */
  dueAt?: number | null
  remindLeadMin?: number
  repeat?: NoteRepeat
  pinned?: boolean
}

export interface NotesSettings {
  notifyEnabled: boolean
  quietHoursEnabled: boolean
  quietStart: string
  quietEnd: string
  pauseUntil: number
  globalShortcut: boolean
  visionProviderId: number | null
}

export interface NoteFile {
  id: number
  fileName: string
  mime: string
  size: number
  createdAt: number
}

export interface NoteFileData {
  mime: string
  base64: string
}

export interface ExtractItem {
  title: string
  detail: string
  dueAt: number | null
  repeat: NoteRepeat
  priority: number
  tags: string[]
  warning: string | null
}

export interface ExtractResult {
  items: ExtractItem[]
  summary: string
  rawText: string
}

export interface CreateFromImageResult {
  noteIds: number[]
  fileId: number
}

export interface NotesReminderItem {
  id: number
  title: string
  dueAt: number | null
  overdue: boolean
}

export interface NotesReminderEvent {
  items: NotesReminderItem[]
  firedAt: number
}

export interface NotesChangedEvent {
  reason: 'create' | 'update' | 'toggle' | 'delete' | 'restore' | 'settings'
  id?: number | null
}
