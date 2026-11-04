import { invoke } from '@tauri-apps/api/core'
import type {
  CreateFromImageResult,
  ExtractResult,
  NoteCounts,
  NoteDetail,
  NoteFile,
  NoteFileData,
  NoteInput,
  NotePatch,
  NotesFilter,
  NotesListResult,
  NotesSettings,
  TagCount,
  ToggleResult,
} from '@/types/notes'

export const notesList = (filter: NotesFilter) =>
  invoke<NotesListResult>('notes_list', { filter })

export const notesGet = (id: number) => invoke<NoteDetail>('notes_get', { id })

export const notesCreate = (input: NoteInput) => invoke<NoteDetail>('notes_create', { input })

export const notesUpdate = (id: number, patch: NotePatch) =>
  invoke<NoteDetail>('notes_update', { id, patch })

export const notesToggle = (id: number, done: boolean) =>
  invoke<ToggleResult>('notes_toggle', { id, done })

export const notesSnooze = (id: number, minutes: number) =>
  invoke<NoteDetail>('notes_snooze', { id, minutes })

export const notesDelete = (id: number) => invoke<void>('notes_delete', { id })

export const notesRestore = (id: number) => invoke<NoteDetail>('notes_restore', { id })

export const notesClearDone = () => invoke<number>('notes_clear_done')

export const notesCounts = () => invoke<NoteCounts>('notes_counts')

export const notesTags = () => invoke<TagCount[]>('notes_tags')

export const notesSettingsGet = () => invoke<NotesSettings>('notes_settings_get')

export const notesSettingsSet = (settings: NotesSettings) =>
  invoke<NotesSettings>('notes_settings_set', { settings })

export const notesQuickShow = () => invoke<void>('notes_quick_show')

export const notesAttachmentAdd = (payload: {
  noteId: number
  fileName?: string
  mime?: string
  base64?: string
  path?: string
}) => invoke<NoteFile>('notes_attachment_add', payload)

export const notesAttachmentList = (noteId: number) =>
  invoke<NoteFile[]>('notes_attachment_list', { noteId })

export const notesAttachmentData = (fileId: number) =>
  invoke<NoteFileData>('notes_attachment_data', { fileId })

export const notesAttachmentRemove = (noteId: number, fileId: number) =>
  invoke<void>('notes_attachment_remove', { noteId, fileId })

export const notesAttachmentSave = (fileId: number, path: string) =>
  invoke<void>('notes_attachment_save', { fileId, path })

export const notesExtractImage = (payload: {
  providerId?: number | null
  mime: string
  base64: string
  requestId: string
}) => invoke<ExtractResult>('notes_extract_image', payload)

export const notesExtractCancel = (requestId: string) =>
  invoke<boolean>('notes_extract_cancel', { requestId })

export const notesCreateFromImage = (payload: {
  fileName?: string
  mime?: string
  base64?: string
  path?: string
  tasks: NoteInput[]
}) => invoke<CreateFromImageResult>('notes_create_from_image', payload)
