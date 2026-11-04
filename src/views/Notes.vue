<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import dayjs, { type Dayjs } from 'dayjs'
import {
  AppstoreOutlined,
  BellOutlined,
  BulbOutlined,
  CalendarOutlined,
  CameraOutlined,
  CheckCircleOutlined,
  CheckOutlined,
  CheckSquareOutlined,
  ClockCircleOutlined,
  DeleteOutlined,
  FileTextOutlined,
  InboxOutlined,
  LinkOutlined,
  PaperClipOutlined,
  PauseCircleOutlined,
  PlusOutlined,
  PushpinFilled,
  PushpinOutlined,
  ScheduleOutlined,
  SearchOutlined,
  SettingOutlined,
  UnorderedListOutlined,
} from '@ant-design/icons-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import PageHeader from '@/components/PageHeader.vue'
import NotesEditor from '@/components/notes/NotesEditor.vue'
import NotesImageViewer from '@/components/notes/NotesImageViewer.vue'
import NotesImportDialog from '@/components/notes/NotesImportDialog.vue'
import {
  notesList,
  notesCreate,
  notesDelete,
  notesGet,
  notesRestore,
  notesClearDone,
  notesSnooze,
  notesTags,
  notesToggle,
  notesUpdate,
} from '@/api/notes'
import { aiProviderList } from '@/api'
import { useNotesStore } from '@/stores/notes'
import { formatDueLabel, parseQuickNote } from '@/utils/notesQuickParse'
import { groupDoneNotes, groupOpenNotes, isOverdue } from '@/utils/notesGrouping'
import { tagToneClass } from '@/utils/notesTags'
import type {
  CreateFromImageResult,
  NoteCounts,
  NoteDetail,
  NoteFile,
  NoteKind,
  NotePatch,
  NoteScope,
  NotesChangedEvent,
  NotesSettings,
  NoteSummary,
  TagCount,
} from '@/types/notes'
import type { AiProvider } from '@/types/models'

const router = useRouter()
const store = useNotesStore()

const SCOPES: { key: NoteScope; label: string }[] = [
  { key: 'today', label: '今天' },
  { key: 'week', label: '本周' },
  { key: 'all', label: '全部' },
  { key: 'done', label: '已完成' },
]

const KIND_OPTIONS: { key: NoteKind; label: string; desc: string }[] = [
  { key: 'todo', label: '待办', desc: '有明确动作，可设截止与提醒' },
  { key: 'note', label: '笔记', desc: '记录信息、会议与总结' },
  { key: 'idea', label: '灵感', desc: '随手记下的想法和点子' },
  { key: 'reference', label: '资料', desc: '链接、命令、配置片段' },
]

const scope = ref<NoteScope>('today')
const keyword = ref('')
const tag = ref('')
const kind = ref<NoteKind | ''>('')
const items = ref<NoteSummary[]>([])
const counts = ref<NoteCounts>({ open: 0, dueToday: 0, overdue: 0, done: 0 })
const tags = ref<TagCount[]>([])
const loading = ref(false)

/** 列表 / 卡片两种展示方式，记住上次选择 */
const VIEW_KEY = 'astroman:notes:view-mode'
type ViewMode = 'list' | 'card'
const viewMode = ref<ViewMode>(
  (() => {
    try {
      return localStorage.getItem(VIEW_KEY) === 'card' ? 'card' : 'list'
    } catch {
      return 'list'
    }
  })(),
)

/** 只有被使用过的标签才会从后端返回；这里允许先建一个空标签再往里记 */
const TAGS_KEY = 'astroman:notes:custom-tags'
const customTags = ref<string[]>(
  (() => {
    try {
      const raw = JSON.parse(localStorage.getItem(TAGS_KEY) ?? '[]')
      return Array.isArray(raw) ? raw.filter((item): item is string => typeof item === 'string') : []
    } catch {
      return []
    }
  })(),
)
const tagInputOpen = ref(false)
const tagInput = ref('')
const tagInputRef = ref<{ focus: () => void } | null>(null)

const mergedTags = computed<TagCount[]>(() => {
  const merged = new Map<string, number>()
  for (const entry of tags.value) merged.set(entry.name, entry.count)
  for (const name of customTags.value) {
    if (!merged.has(name)) merged.set(name, 0)
  }
  return Array.from(merged, ([name, count]) => ({ name, count })).sort((a, b) =>
    a.name.localeCompare(b.name, 'zh-Hans-CN'),
  )
})
const selectedId = ref<number | null>(null)
const detail = ref<NoteDetail | null>(null)

const quickInput = ref('')
const quickBody = ref('')
const quickRef = ref<{ focus: () => void } | null>(null)
const searchRef = ref<{ focus: () => void } | null>(null)
const creating = ref(false)

const importOpen = ref(false)
const viewerOpen = ref(false)
const viewerFiles = ref<NoteFile[]>([])
const viewerIndex = ref(0)

const settingsOpen = ref(false)
const settingsForm = ref({
  notifyEnabled: true,
  quietHoursEnabled: false,
  quietStart: dayjs('22:00', 'HH:mm'),
  quietEnd: dayjs('08:00', 'HH:mm'),
  globalShortcut: true,
  visionProviderId: null as number | null,
})
const visionProviders = ref<AiProvider[]>([])
const settingsSaving = ref(false)

const now = ref(Date.now())
const compact = ref(false)
const windowWidth = ref(window.innerWidth)
/** 详情抽屉占页面一半；窄窗口留边，避免挤压正文 */
const drawerWidth = computed(() =>
  compact.value
    ? Math.min(560, Math.max(320, windowWidth.value - 32))
    : Math.max(520, Math.round(windowWidth.value * 0.5)),
)

interface UndoState {
  kind: 'done' | 'delete'
  id: number
  text: string
}
const undo = ref<UndoState | null>(null)
let undoTimer: number | undefined

let keywordTimer: number | undefined
let changeTimer: number | undefined
let unlisten: UnlistenFn | null = null
let tickTimer: number | undefined

const quickParsed = computed(() => parseQuickNote(quickInput.value, new Date(now.value)))

/** 从“新建标签”或标签筛选进入时，快速记录自动带上当前标签 */
const quickTags = computed(() => {
  const parsed = quickParsed.value.tags
  if (!tag.value || parsed.includes(tag.value)) return parsed
  return [...parsed, tag.value]
})

function setViewMode(mode: ViewMode) {
  if (viewMode.value === mode) return
  viewMode.value = mode
  try {
    localStorage.setItem(VIEW_KEY, mode)
  } catch {
    // localStorage 不可用时只影响记忆，不影响当前切换
  }
}

function openTagInput() {
  tagInputOpen.value = true
  void nextTick(() => tagInputRef.value?.focus())
}

function cancelTagInput() {
  tagInputOpen.value = false
  tagInput.value = ''
}

function createTag() {
  const name = tagInput.value
    .trim()
    .replace(/^#+/, '')
    .replace(/[,，]/g, '')
    .replace(/\s+/g, ' ')
    .slice(0, 32)
  if (!name) {
    cancelTagInput()
    return
  }
  if (!customTags.value.includes(name) && !tags.value.some((entry) => entry.name === name)) {
    customTags.value = [...customTags.value, name]
    try {
      localStorage.setItem(TAGS_KEY, JSON.stringify(customTags.value))
    } catch {
      // 忽略存储失败，本次会话内仍然可用
    }
  }
  tag.value = name
  cancelTagInput()
  void loadList()
  focusQuick()
}

const scopeItems = computed(() => [
  {
    key: 'today' as NoteScope,
    label: '今天',
    count: counts.value.overdue + counts.value.dueToday,
  },
  { key: 'week' as NoteScope, label: '本周', count: null as number | null },
  { key: 'all' as NoteScope, label: '全部', count: counts.value.open },
  { key: 'done' as NoteScope, label: '已完成', count: counts.value.done },
])

function scopeIcon(key: NoteScope) {
  if (key === 'today') return CalendarOutlined
  if (key === 'week') return ScheduleOutlined
  if (key === 'all') return UnorderedListOutlined
  return CheckCircleOutlined
}

function kindIcon(value: NoteKind) {
  if (value === 'note') return FileTextOutlined
  if (value === 'idea') return BulbOutlined
  if (value === 'reference') return LinkOutlined
  return CheckSquareOutlined
}

/** 类型固定配色：待办主题色、笔记蓝、灵感黄、资料青 */
function kindTone(value: NoteKind): string {
  if (value === 'note') return 'tone-1'
  if (value === 'idea') return 'tone-4'
  if (value === 'reference') return 'tone-6'
  return 'tone-0'
}

function kindLabel(value: NoteKind): string {
  return KIND_OPTIONS.find((item) => item.key === value)?.label ?? '待办'
}

function onCreateMenu({ key }: { key: string }) {
  void createBlank(key as NoteKind)
}

const groups = computed(() => {
  if (scope.value === 'done') {
    return groupDoneNotes(items.value, now.value).map((group) => ({
      key: group.title,
      title: group.title,
      items: group.items,
    }))
  }
  return groupOpenNotes(items.value, now.value).map((group) => ({
    key: group.key,
    title: group.title,
    items: group.items,
  }))
})

const flatIds = computed(() => groups.value.flatMap((group) => group.items.map((item) => item.id)))

const emptyHint = computed(() => {
  if (keyword.value.trim()) return '没有匹配的记录'
  if (tag.value) return `标签 #${tag.value} 下还没有记录`
  if (kind.value) return `还没有${kindLabel(kind.value)}记录`
  if (scope.value === 'done') return '还没有完成的记录'
  if (scope.value === 'today') return '今天还没有待办，记一笔试试'
  return '还没有记录'
})

const pauseText = computed(() => {
  if (store.settings.pauseUntil <= now.value) return ''
  const until = new Date(store.settings.pauseUntil)
  const today = new Date(now.value)
  if (until.toDateString() === today.toDateString()) {
    return `提醒已暂停至 ${String(until.getHours()).padStart(2, '0')}:${String(
      until.getMinutes(),
    ).padStart(2, '0')}`
  }
  return `提醒已暂停至 ${until.getMonth() + 1}-${until.getDate()}`
})

const paused = computed(() => store.settings.pauseUntil > now.value)

const currentNote = computed(() =>
  detail.value && detail.value.id === selectedId.value ? detail.value : null,
)

function startOfTomorrow(): number {
  const date = new Date()
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime()
}

function startOfNextWeek(): number {
  const date = new Date()
  const delta = (8 - date.getDay()) % 7 || 7
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + delta).getTime()
}

function inScope(note: NoteDetail, target: NoteScope): boolean {
  if (target === 'all' || target === 'done') return true
  if (note.dueAt === null) return true
  return target === 'today' ? note.dueAt < startOfTomorrow() : note.dueAt < startOfNextWeek()
}

async function loadList(keepSelection = true, silent = false) {
  if (!silent) loading.value = true
  try {
    const result = await notesList({
      scope: scope.value,
      keyword: keyword.value.trim() || undefined,
      tag: tag.value || undefined,
      kind: kind.value || undefined,
    })
    items.value = result.items
    counts.value = result.counts
    store.counts = result.counts
    if (keepSelection && selectedId.value !== null) {
      if (!result.items.some((item) => item.id === selectedId.value)) {
        selectedId.value = null
        detail.value = null
      }
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    if (!silent) loading.value = false
  }
}

async function loadTags() {
  try {
    tags.value = await notesTags()
  } catch {
    tags.value = []
  }
}

async function selectNote(id: number) {
  if (selectedId.value === id) return
  selectedId.value = id
  try {
    detail.value = await notesGet(id)
  } catch (error) {
    message.error(String(error))
    detail.value = null
  }
}

function clearSelection() {
  selectedId.value = null
  detail.value = null
}

async function onSaved(note: NoteDetail) {
  if (detail.value?.id === note.id) detail.value = note
  await Promise.all([loadList(true, true), loadTags()])
}

async function onEditorDeleted(id: number) {
  await deleteNote(id)
}

function onOpenImage(payload: { files: NoteFile[]; index: number }) {
  viewerFiles.value = payload.files
  viewerIndex.value = payload.index
  viewerOpen.value = true
}

async function refreshAfterAttachment() {
  await Promise.all([loadList(), loadTags()])
  if (selectedId.value) {
    try {
      detail.value = await notesGet(selectedId.value)
    } catch {
      // 附件刷新失败不影响列表
    }
  }
}

/* ---------- 快速记录 ---------- */
function focusQuick() {
  void nextTick(() => quickRef.value?.focus())
}

function onQuickPaste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData('text') ?? ''
  if (!text.includes('\n')) return
  event.preventDefault()
  const lines = text.replace(/\r/g, '').split('\n')
  quickInput.value = lines.shift() ?? ''
  const rest = lines.join('\n').trim()
  if (rest) quickBody.value = quickBody.value ? `${quickBody.value}\n${rest}` : rest
}

async function saveQuick() {
  if (creating.value) return
  if (!quickInput.value.trim()) return
  if (!quickParsed.value.title.trim()) {
    message.warning('再写点内容')
    return
  }
  creating.value = true
  try {
    const note = await notesCreate({
      title: quickParsed.value.title,
      content: quickBody.value.trim() || undefined,
      kind: kind.value || 'todo',
      dueAt: quickParsed.value.dueAt,
      repeat: quickParsed.value.repeat,
      tags: quickTags.value,
      priority: quickParsed.value.priority,
    })
    quickInput.value = ''
    quickBody.value = ''
    if (!inScope(note, scope.value)) scope.value = 'all'
    await loadList()
    await selectNote(note.id)
    await loadTags()
    focusQuick()
  } catch (error) {
    message.error(String(error))
  } finally {
    creating.value = false
  }
}

function onQuickKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
    event.preventDefault()
    void saveQuick()
  }
}

async function createBlank(nextKind: NoteKind = 'todo') {
  try {
    const note = await notesCreate({
      title: nextKind === 'todo' ? '新记录' : `新${kindLabel(nextKind)}`,
      kind: nextKind,
      tags: tag.value ? [tag.value] : undefined,
    })
    if (kind.value && kind.value !== nextKind) kind.value = nextKind
    if (!inScope(note, scope.value)) scope.value = 'all'
    await loadList()
    await selectNote(note.id)
  } catch (error) {
    message.error(String(error))
  }
}

/* ---------- 完成 / 删除 / 撤销 ---------- */
function showUndo(next: UndoState) {
  if (undoTimer) window.clearTimeout(undoTimer)
  undo.value = next
  undoTimer = window.setTimeout(() => {
    undo.value = null
    undoTimer = undefined
  }, 6000)
}

async function toggleNote(item: NoteSummary, done: boolean) {
  const snapshot = items.value
  if (done && scope.value !== 'done') {
    items.value = items.value.filter((entry) => entry.id !== item.id)
  }
  try {
    const result = await notesToggle(item.id, done)
    if (done) {
      if (result.rolledTo) {
        message.info(`已顺延到 ${formatDueLabel(result.rolledTo)}`)
      } else {
        showUndo({ kind: 'done', id: item.id, text: `已完成「${item.title || '未命名'}」` })
      }
    }
    if (selectedId.value === item.id) detail.value = result.note
    await Promise.all([loadList(), loadTags()])
  } catch (error) {
    items.value = snapshot
    message.error(String(error))
  }
}

async function deleteNote(id: number) {
  const target = items.value.find((item) => item.id === id)
  try {
    await notesDelete(id)
    if (selectedId.value === id) clearSelection()
    await Promise.all([loadList(), loadTags()])
    showUndo({ kind: 'delete', id, text: `已删除「${target?.title || '这条记录'}」` })
  } catch (error) {
    message.error(String(error))
  }
}

async function runUndo() {
  const target = undo.value
  if (!target) return
  if (undoTimer) window.clearTimeout(undoTimer)
  undoTimer = undefined
  undo.value = null
  try {
    if (target.kind === 'done') {
      await notesToggle(target.id, false)
    } else {
      await notesRestore(target.id)
    }
    await Promise.all([loadList(), loadTags()])
    if (target.kind === 'delete') await selectNote(target.id)
  } catch (error) {
    message.error(String(error))
  }
}

async function snoozeNote(item: NoteSummary) {
  try {
    const updated = await notesSnooze(item.id, 10)
    const until = updated.snoozeUntil ?? Date.now() + 10 * 60_000
    message.success(`已稍后，${formatDueLabel(until)} 再提醒`)
    if (selectedId.value === item.id) detail.value = updated
    await loadList()
  } catch (error) {
    message.error(String(error))
  }
}

async function pinNote(item: NoteSummary) {
  try {
    const patch: NotePatch = { pinned: !item.pinned }
    const note = await notesUpdate(item.id, patch)
    if (selectedId.value === item.id) detail.value = note
    await loadList()
  } catch (error) {
    message.error(String(error))
  }
}

function canSnooze(item: NoteSummary): boolean {
  if (item.dueAt === null) return false
  return item.dueAt - item.remindLeadMin * 60_000 <= now.value
}

/* ---------- 设置 ---------- */
function openSettings() {
  const settings = store.settings
  settingsForm.value = {
    notifyEnabled: settings.notifyEnabled,
    quietHoursEnabled: settings.quietHoursEnabled,
    quietStart: dayjs(settings.quietStart, 'HH:mm'),
    quietEnd: dayjs(settings.quietEnd, 'HH:mm'),
    globalShortcut: settings.globalShortcut,
    visionProviderId: settings.visionProviderId,
  }
  settingsOpen.value = true
  void loadVisionProviders()
}

async function loadVisionProviders() {
  try {
    const list = await aiProviderList()
    visionProviders.value = list.filter((item) => item.supportsVision && item.id != null)
  } catch {
    visionProviders.value = []
  }
}

async function saveSettings() {
  settingsSaving.value = true
  const form = settingsForm.value
  const patch: Partial<NotesSettings> = {
    notifyEnabled: form.notifyEnabled,
    quietHoursEnabled: form.quietHoursEnabled,
    quietStart: form.quietStart.format('HH:mm'),
    quietEnd: form.quietEnd.format('HH:mm'),
    globalShortcut: form.globalShortcut,
    visionProviderId: form.visionProviderId,
  }
  try {
    await store.saveSettings(patch)
    settingsOpen.value = false
    message.success('设置已保存')
  } catch (error) {
    message.error(String(error))
    await store.loadSettings()
  } finally {
    settingsSaving.value = false
  }
}

async function patchSettings(patch: Partial<NotesSettings>) {
  try {
    await store.saveSettings(patch)
  } catch (error) {
    message.error(String(error))
    await store.loadSettings()
  }
}

function onBellMenu({ key }: { key: string }) {
  if (key === 'notify') {
    void patchSettings({ notifyEnabled: !store.settings.notifyEnabled })
  } else if (key === 'quiet') {
    void patchSettings({ quietHoursEnabled: !store.settings.quietHoursEnabled })
  } else if (key === 'settings') {
    openSettings()
  }
}

function onPauseMenu({ key }: { key: string }) {
  if (key === 'hour') {
    void patchSettings({ pauseUntil: Date.now() + 60 * 60 * 1000 })
  } else if (key === 'today') {
    void patchSettings({ pauseUntil: startOfTomorrow() })
  } else if (key === 'cancel') {
    void patchSettings({ pauseUntil: 0 })
  }
}

/* ---------- 截图导入 ---------- */
async function onImported(result: CreateFromImageResult) {
  scope.value = 'all'
  await Promise.all([loadList(false), loadTags()])
  if (result.noteIds.length) await selectNote(result.noteIds[0])
}

/* ---------- 筛选与生命周期 ---------- */
watch(scope, () => {
  clearSelection()
  void loadList()
})

watch(tag, () => {
  clearSelection()
  void loadList()
})

watch(kind, () => {
  clearSelection()
  void loadList()
})

watch(keyword, () => {
  if (keywordTimer) window.clearTimeout(keywordTimer)
  keywordTimer = window.setTimeout(() => {
    clearSelection()
    void loadList()
  }, 240)
})

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  if (target.closest('input, textarea, select, .ant-select, .ant-picker')) return true
  return false
}

function moveSelection(delta: number) {
  const ids = flatIds.value
  if (!ids.length) return
  const index = selectedId.value === null ? -1 : ids.indexOf(selectedId.value)
  const next = Math.min(ids.length - 1, Math.max(0, index + delta))
  void selectNote(ids[next])
}

function onHotkeys(event: KeyboardEvent) {
  if (event.altKey || event.ctrlKey || event.metaKey) return
  const editable = isEditableTarget(event.target)
  if (event.key === 'Escape') {
    if (editable && event.target instanceof HTMLElement) {
      event.target.blur()
      return
    }
    if (selectedId.value !== null) {
      event.preventDefault()
      clearSelection()
    }
    return
  }
  if (editable) return
  const key = event.key.toLowerCase()
  if (event.key === ' ') {
    const current = items.value.find((item) => item.id === selectedId.value)
    if (!current) return
    event.preventDefault()
    void toggleNote(current, current.status !== 'done')
    return
  }
  if (key === 'n') {
    event.preventDefault()
    focusQuick()
  } else if (event.key === '/') {
    event.preventDefault()
    searchRef.value?.focus()
  } else if (key === 'j') {
    event.preventDefault()
    moveSelection(1)
  } else if (key === 'k') {
    event.preventDefault()
    moveSelection(-1)
  }
}

function syncCompact() {
  windowWidth.value = window.innerWidth
  compact.value = windowWidth.value < 1100
}

function onVisibilityChange() {
  if (document.visibilityState !== 'visible') return
  now.value = Date.now()
  store.touchNow()
  void loadList()
  void loadTags()
}

async function onExternalChange() {
  if (changeTimer) window.clearTimeout(changeTimer)
  changeTimer = window.setTimeout(async () => {
    await Promise.all([loadList(true, true), loadTags()])
    // 卡片里点「稍后」也会走到这里，抽屉里的提醒时间要跟着刷新
    if (selectedId.value !== null) {
      try {
        detail.value = await notesGet(selectedId.value)
      } catch {
        // 条目可能刚被删除，列表刷新已经处理
      }
    }
  }, 200)
}

onMounted(async () => {
  syncCompact()
  window.addEventListener('resize', syncCompact)
  window.addEventListener('keydown', onHotkeys)
  document.addEventListener('visibilitychange', onVisibilityChange)

  tickTimer = window.setInterval(() => {
    now.value = Date.now()
    store.touchNow()
  }, 30_000)

  await store.loadSettings()
  await Promise.all([loadList(), loadTags()])

  try {
    unlisten = await listen<NotesChangedEvent>('notes:changed', (event) => {
      if (event.payload?.reason === 'settings') void store.loadSettings()
      void onExternalChange()
    })
  } catch {
    unlisten = null
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', syncCompact)
  window.removeEventListener('keydown', onHotkeys)
  document.removeEventListener('visibilitychange', onVisibilityChange)
  if (tickTimer) window.clearInterval(tickTimer)
  if (keywordTimer) window.clearTimeout(keywordTimer)
  if (changeTimer) window.clearTimeout(changeTimer)
  if (undoTimer) window.clearTimeout(undoTimer)
  unlisten?.()
  unlisten = null
})

</script>

<template>
  <div class="notes-page">
    <PageHeader
      eyebrow="Notes"
      title="记事本"
      subtitle="本地待办与随手记 · 到期提醒、截图生成、图片附件"
    >
      <span v-if="pauseText" class="pause-state">{{ pauseText }}</span>
      <span v-else-if="store.quietText" class="pause-state quiet">
        免打扰 {{ store.quietText }}
      </span>
      <a-button @click="importOpen = true">
        <template #icon><CameraOutlined /></template>
        截图生成
      </a-button>
      <a-dropdown trigger="click">
        <a-button :type="paused ? 'primary' : 'default'">
          <template #icon><PauseCircleOutlined /></template>
          {{ paused ? '暂停中' : '提醒' }}
        </a-button>
        <template #overlay>
          <a-menu @click="onPauseMenu">
            <a-menu-item key="hour">暂停 1 小时</a-menu-item>
            <a-menu-item key="today">暂停到今天结束</a-menu-item>
            <a-menu-item key="cancel" :disabled="!paused">取消暂停</a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-dropdown trigger="click">
        <a-button>
          <template #icon><BellOutlined /></template>
        </a-button>
        <template #overlay>
          <a-menu @click="onBellMenu">
            <a-menu-item key="notify">
              {{ store.settings.notifyEnabled ? '关闭到点提醒' : '开启到点提醒' }}
            </a-menu-item>
            <a-menu-item key="quiet">
              {{ store.settings.quietHoursEnabled ? '关闭免打扰' : '开启免打扰' }}
            </a-menu-item>
            <a-menu-divider />
            <a-menu-item key="settings">记事本设置</a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-button @click="openSettings">
        <template #icon><SettingOutlined /></template>
      </a-button>
      <a-dropdown trigger="click">
        <a-button type="primary">
          <template #icon><PlusOutlined /></template>
          新建
        </a-button>
        <template #overlay>
          <a-menu @click="onCreateMenu">
            <a-menu-item v-for="option in KIND_OPTIONS" :key="option.key">
              <component :is="kindIcon(option.key)" />
              <span class="menu-title">{{ option.label }}</span>
              <span class="menu-desc">{{ option.desc }}</span>
            </a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-input
        ref="searchRef"
        v-model:value="keyword"
        class="search-input"
        allow-clear
        placeholder="搜索标题或正文…"
      >
        <template #prefix><SearchOutlined /></template>
      </a-input>
    </PageHeader>

    <div class="notes-grid">
      <aside class="scope-pane fm-card">
        <div class="pane-caption">视图</div>
        <button
          v-for="item in scopeItems"
          :key="item.key"
          type="button"
          class="scope-item"
          :class="{ active: scope === item.key }"
          @click="scope = item.key"
        >
          <component :is="scopeIcon(item.key)" class="scope-icon" />
          <span>{{ item.label }}</span>
          <span v-if="item.count" class="scope-count">{{ item.count }}</span>
        </button>

        <div class="pane-caption kind-caption">类型</div>
        <button
          type="button"
          class="scope-item"
          :class="{ active: kind === '' }"
          @click="kind = ''"
        >
          <AppstoreOutlined class="scope-icon" />
          <span>全部类型</span>
        </button>
        <button
          v-for="option in KIND_OPTIONS"
          :key="option.key"
          type="button"
          class="scope-item"
          :class="{ active: kind === option.key }"
          @click="kind = kind === option.key ? '' : option.key"
        >
          <component
            :is="kindIcon(option.key)"
            class="scope-icon kind-dot"
            :class="kindTone(option.key)"
          />
          <span>{{ option.label }}</span>
        </button>

        <div class="pane-caption tags-caption">
          <span>标签</span>
          <span class="tag-caption-actions">
            <button v-if="tag" type="button" class="clear-tag" @click="tag = ''">清除</button>
            <a-tooltip :title="tagInputOpen ? '收起' : '新建标签'">
              <button
                type="button"
                class="tag-add"
                :class="{ active: tagInputOpen }"
                @click="tagInputOpen ? cancelTagInput() : openTagInput()"
              >
                <PlusOutlined />
                <span>新建</span>
              </button>
            </a-tooltip>
          </span>
        </div>
        <form v-if="tagInputOpen" class="tag-create" @submit.prevent="createTag">
          <a-input
            ref="tagInputRef"
            v-model:value="tagInput"
            size="small"
            :maxlength="32"
            placeholder="输入标签名，回车创建"
            @keydown.esc.prevent="cancelTagInput"
          />
        </form>
        <div v-if="!mergedTags.length" class="tag-empty">还没有标签</div>
        <button
          v-for="entry in mergedTags"
          :key="entry.name"
          type="button"
          class="tag-item tag-chip"
          :class="[
            tagToneClass(entry.name),
            { active: tag === entry.name, unused: !entry.count },
          ]"
          @click="tag = tag === entry.name ? '' : entry.name"
        >
          <span class="tag-name">#{{ entry.name }}</span>
          <span class="tag-count">{{ entry.count }}</span>
        </button>
      </aside>

      <section class="list-pane fm-card">
        <div class="quick-box">
          <a-input
            ref="quickRef"
            v-model:value="quickInput"
            class="quick-input"
            placeholder="记一笔…  试试：明天10点 交周报 #工作 !"
            allow-clear
            @keydown="onQuickKeydown"
            @paste="onQuickPaste"
          >
            <template #suffix>
              <a-spin v-if="creating" size="small" />
            </template>
          </a-input>
          <div class="quick-preview">
            <span v-for="label in quickParsed.labels" :key="label" class="preview-tag">
              {{ label }}
            </span>
            <span
              v-if="tag && !quickParsed.tags.includes(tag)"
              class="preview-tag active-tag"
              :class="tagToneClass(tag)"
            >
              #{{ tag }}
            </span>
            <span v-if="quickBody" class="preview-body">
              正文 {{ quickBody.split('\n').length }} 行
              <button type="button" @click="quickBody = ''">清空</button>
            </span>
            <span v-if="!quickParsed.labels.length && !quickBody" class="preview-hint">
              回车保存 · 多行粘贴自动拆成标题与正文
            </span>
            <div class="view-toggle" role="group" aria-label="展示方式">
              <a-tooltip title="列表视图">
                <button
                  type="button"
                  :class="{ active: viewMode === 'list' }"
                  @click="setViewMode('list')"
                >
                  <UnorderedListOutlined />
                </button>
              </a-tooltip>
              <a-tooltip title="卡片视图">
                <button
                  type="button"
                  :class="{ active: viewMode === 'card' }"
                  @click="setViewMode('card')"
                >
                  <AppstoreOutlined />
                </button>
              </a-tooltip>
            </div>
          </div>
        </div>

        <div class="list-scroll">
          <a-spin :spinning="loading" class="list-spin">
            <div v-if="!items.length && !loading" class="list-empty">
              <InboxOutlined />
              <div>{{ emptyHint }}</div>
            </div>
            <div v-for="group in groups" :key="group.key" class="group">
              <div class="group-title" :class="{ overdue: group.key === 'overdue' }">
                <span>{{ group.title }}</span>
                <span class="group-count">{{ group.items.length }}</span>
              </div>
              <div class="note-items" :class="viewMode">
                <div
                  v-for="item in group.items"
                  :key="item.id"
                  class="note-row"
                  :class="{
                    selected: selectedId === item.id,
                    done: item.status === 'done',
                    pinned: item.pinned,
                    overdue: isOverdue(item, now),
                  }"
                  @click="selectNote(item.id)"
                >
                  <a-checkbox
                    class="row-check"
                    :checked="item.status === 'done'"
                    @click.stop
                    @change="toggleNote(item, Boolean(($event.target as HTMLInputElement)?.checked))"
                  />
                  <div class="row-body">
                    <div class="row-title" :title="kindLabel(item.kind)">
                      <component
                        :is="kindIcon(item.kind)"
                        class="kind-mark"
                        :class="kindTone(item.kind)"
                      />
                      <span class="row-title-text">{{ item.title || '未命名' }}</span>
                    </div>
                    <div class="row-meta">
                      <span
                        v-if="item.dueAt"
                        class="due"
                        :class="{ overdue: isOverdue(item, now) }"
                      >
                        <ClockCircleOutlined />
                        {{ formatDueLabel(item.dueAt, now) }}
                      </span>
                      <span v-else class="due none">无截止时间</span>
                      <span v-if="item.repeat !== 'none'" class="meta-chip">{{
                        { daily: '每天', weekdays: '工作日', weekly: '每周', monthly: '每月' }[
                          item.repeat
                        ]
                      }}</span>
                      <span v-if="isOverdue(item, now)" class="meta-chip danger">已逾期</span>
                      <span
                        v-if="item.snoozeUntil && item.snoozeUntil > now"
                        class="meta-chip snoozed"
                      >
                        已稍后 {{ formatDueLabel(item.snoozeUntil, now) }}
                      </span>
                      <span v-if="item.priority === 2" class="meta-chip high">高</span>
                      <span v-if="item.attachmentCount" class="meta-chip">
                        <PaperClipOutlined /> {{ item.attachmentCount }}
                      </span>
                      <span
                        v-for="name in item.tags.slice(0, 2)"
                        :key="name"
                        class="note-tag"
                        :class="tagToneClass(name)"
                      >
                        #{{ name }}
                      </span>
                      <span v-if="item.tags.length > 2" class="note-tag more">
                        +{{ item.tags.length - 2 }}
                      </span>
                    </div>
                    <div v-if="item.excerpt" class="row-excerpt">{{ item.excerpt }}</div>
                  </div>
                  <div class="row-actions" @click.stop>
                    <a-tooltip v-if="item.status === 'open'" title="稍后 10 分钟">
                      <button
                        type="button"
                        class="row-btn"
                        :disabled="!canSnooze(item)"
                        @click="snoozeNote(item)"
                      >
                        <BellOutlined />
                      </button>
                    </a-tooltip>
                    <a-tooltip :title="item.pinned ? '取消置顶' : '置顶'">
                      <button type="button" class="row-btn" @click="pinNote(item)">
                        <PushpinFilled v-if="item.pinned" />
                        <PushpinOutlined v-else />
                      </button>
                    </a-tooltip>
                    <a-tooltip title="删除">
                      <button type="button" class="row-btn danger" @click="deleteNote(item.id)">
                        <DeleteOutlined />
                      </button>
                    </a-tooltip>
                  </div>
                </div>
              </div>
            </div>
          </a-spin>
        </div>

        <div v-if="scope === 'done' && items.length" class="list-foot">
          <a-button
            size="small"
            danger
            type="text"
            @click="
              Modal.confirm({
                title: '清空已完成？',
                content: '会删除所有已完成记录，30 天后彻底清理。',
                okText: '清空',
                okType: 'danger',
                cancelText: '取消',
                onOk: async () => {
                  await notesClearDone()
                  await loadList()
                },
              })
            "
          >
            清空已完成
          </a-button>
        </div>
      </section>
    </div>

    <a-drawer
      :open="Boolean(selectedId)"
      placement="right"
      :width="drawerWidth"
      :closable="false"
      :keyboard="true"
      :destroy-on-close="true"
      :body-style="{ padding: '14px 18px' }"
      root-class-name="notes-detail-drawer"
      @close="clearSelection"
    >
      <NotesEditor
        :note="currentNote"
        :tags="mergedTags"
        @saved="onSaved"
        @deleted="onEditorDeleted"
        @close="clearSelection"
        @open-image="onOpenImage"
        @attachment-changed="refreshAfterAttachment"
      />
    </a-drawer>

    <transition name="undo-fade">
      <div v-if="undo" class="undo-bar">
        <CheckOutlined />
        <span>{{ undo.text }}</span>
        <a-button size="small" type="link" @click="runUndo">撤销</a-button>
      </div>
    </transition>

    <NotesImportDialog v-model:open="importOpen" @created="onImported" />
    <NotesImageViewer
      v-model:open="viewerOpen"
      v-model:index="viewerIndex"
      :files="viewerFiles"
    />

    <a-modal
      v-model:open="settingsOpen"
      title="记事本设置"
      :confirm-loading="settingsSaving"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveSettings"
    >
      <div class="settings-form">
        <div class="setting-row">
          <div class="setting-text">
            <div class="setting-title">到点提醒</div>
            <div class="setting-desc">关闭后不再弹出提醒卡片，只保留角标与托盘提示</div>
          </div>
          <a-switch v-model:checked="settingsForm.notifyEnabled" />
        </div>
        <div class="setting-row">
          <div class="setting-text">
            <div class="setting-title">免打扰</div>
            <div class="setting-desc">期间不弹提醒卡片，结束后合并提醒一次</div>
          </div>
          <a-switch v-model:checked="settingsForm.quietHoursEnabled" />
        </div>
        <div v-if="settingsForm.quietHoursEnabled" class="setting-row indent">
          <a-time-picker v-model:value="settingsForm.quietStart" format="HH:mm" size="small" />
          <span class="range-dash">至</span>
          <a-time-picker v-model:value="settingsForm.quietEnd" format="HH:mm" size="small" />
        </div>
        <div class="setting-row">
          <div class="setting-text">
            <div class="setting-title">全局快捷键</div>
            <div class="setting-desc">Ctrl + Alt + N 唤起快速记录窗口</div>
          </div>
          <a-switch v-model:checked="settingsForm.globalShortcut" />
        </div>
        <div class="setting-row">
          <div class="setting-text">
            <div class="setting-title">截图识别服务</div>
            <div class="setting-desc">
              只列出勾选了「支持图片识别」的模型服务
            </div>
          </div>
          <a-select
            v-model:value="settingsForm.visionProviderId"
            size="small"
            allow-clear
            style="width: 180px"
            placeholder="未选择"
            :options="
              visionProviders.map((item) => ({ value: item.id!, label: item.name }))
            "
          />
        </div>
        <div v-if="!visionProviders.length" class="settings-tip">
          还没有支持图片的服务商，去
          <a @click="settingsOpen = false; router.push('/ai')">AI 助手 → 模型服务</a>
          添加并勾选「支持图片识别」。
        </div>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.notes-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px 18px 14px;
  box-sizing: border-box;
  overflow: hidden;
  background-image: radial-gradient(var(--fm-border-strong) 0.7px, transparent 0.7px);
  background-size: 18px 18px;
  background-position: 3px 3px;
}

/* 标签整块配色：同一标签在侧栏、列表和卡片里颜色一致 */
.tone-0 {
  --tag-bg: #d9f3ef;
  --tag-fg: #0f766e;
}

.tone-1 {
  --tag-bg: #dce8ff;
  --tag-fg: #1d4ed8;
}

.tone-2 {
  --tag-bg: #ebe4ff;
  --tag-fg: #6d28d9;
}

.tone-3 {
  --tag-bg: #ffe1e8;
  --tag-fg: #be123c;
}

.tone-4 {
  --tag-bg: #fdf0cc;
  --tag-fg: #a16207;
}

.tone-5 {
  --tag-bg: #dcf7e4;
  --tag-fg: #15803d;
}

.tone-6 {
  --tag-bg: #d6f4fb;
  --tag-fg: #0e7490;
}

.tone-7 {
  --tag-bg: #ffe7d6;
  --tag-fg: #c2410c;
}

.pause-state {
  font-size: 11px;
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
  border-radius: 4px;
  padding: 2px 8px;
}

.pause-state.quiet {
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.search-input {
  width: 200px;
}

.permission-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  padding: 6px 10px;
  border: 1px solid #f2d9a8;
  background: var(--fm-warning-soft);
  border-radius: var(--fm-radius-sm);
  font-size: 11px;
  color: var(--fm-warning);
}

.permission-bar span {
  flex: 1;
}

.permission-bar a {
  color: var(--fm-primary);
}

.notes-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 176px minmax(0, 1fr);
  gap: 12px;
}

.scope-pane,
.list-pane {
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 10px;
  overflow: hidden;
  border-radius: var(--fm-radius-lg);
  box-shadow: var(--fm-shadow);
  background:
    linear-gradient(180deg, var(--fm-primary-soft) 0, transparent 132px),
    var(--fm-surface);
}

:global(.notes-detail-drawer .ant-drawer-content) {
  background:
    linear-gradient(180deg, var(--fm-primary-soft) 0, transparent 132px),
    var(--fm-surface);
}

:global(.notes-detail-drawer .ant-drawer-body) {
  position: relative;
  padding-left: 26px;
}

:global(.notes-detail-drawer .ant-drawer-body::before) {
  content: '';
  position: absolute;
  left: 16px;
  top: 16px;
  bottom: 16px;
  width: 1px;
  background: var(--fm-border-strong);
  opacity: 0.65;
}

.pane-caption {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--fm-muted);
  padding: 2px 6px 6px;
}

.tags-caption {
  margin-top: 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.clear-tag {
  border: none;
  background: transparent;
  font-size: 10px;
  color: var(--fm-primary);
  cursor: pointer;
  padding: 0;
}

.scope-item,
.tag-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  border: none;
  background: transparent;
  color: var(--fm-text-2);
  border-radius: var(--fm-radius-sm);
  padding: 6px 8px;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.scope-icon {
  font-size: 12px;
  color: var(--fm-muted);
}

.scope-icon.kind-dot {
  color: var(--tag-fg, var(--fm-muted));
}

.scope-item.active .scope-icon.kind-dot {
  color: var(--tag-fg, var(--fm-primary));
}

.scope-item:hover,
.tag-item:hover {
  background: var(--fm-surface-alt);
}

.scope-item.active,
.tag-item.active {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.scope-item.active .scope-icon {
  color: var(--fm-primary);
}

.tag-chip {
  background: var(--tag-bg);
  color: var(--tag-fg);
  margin-bottom: 3px;
  font-weight: 500;
}

.tag-chip:hover {
  background: var(--tag-bg);
  filter: brightness(0.97);
}

.tag-chip .tag-count {
  color: var(--tag-fg);
  background: rgba(255, 255, 255, 0.62);
}

.tag-chip.active {
  background: var(--tag-fg);
  color: #ffffff;
}

.tag-chip.active .tag-count {
  color: var(--tag-fg);
  background: rgba(255, 255, 255, 0.9);
}

.scope-count,
.tag-count {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-radius: 8px;
  padding: 0 6px;
}

.scope-item.active .scope-count,
.tag-item.active .tag-count {
  color: var(--fm-primary);
  background: #ffffff;
}

.tag-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag-caption-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.tag-add {
  height: 18px;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 5px;
  border: 1px dashed var(--fm-border-strong);
  border-radius: 4px;
  background: transparent;
  color: var(--fm-muted);
  font-size: 10px;
  line-height: 1;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s, background 0.15s;
}

.tag-add span {
  letter-spacing: 0.2px;
}

.tag-add:hover,
.tag-add.active {
  color: var(--fm-primary);
  border-color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.tag-create {
  padding: 2px 4px 6px;
}

.tag-create :deep(.ant-input) {
  font-size: 11px;
  padding: 2px 7px;
}

.tag-empty {
  font-size: 11px;
  color: var(--fm-muted);
  padding: 4px 8px;
}

.tag-item.unused {
  color: var(--fm-muted);
}

.tag-item.unused .tag-count {
  opacity: 0.55;
}

.tag-item.unused.active {
  color: var(--fm-primary);
}

.quick-box {
  margin-bottom: 8px;
  padding: 8px 8px 6px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  box-shadow: var(--fm-shadow-sm);
}

.quick-input {
  font-size: 13px;
}

.quick-preview {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 5px;
  min-height: 18px;
  padding: 3px 2px 0;
}

.preview-tag {
  font-size: 10px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 4px;
  padding: 1px 6px;
}

.preview-hint,
.preview-body {
  font-size: 10px;
  color: var(--fm-muted);
}

.preview-body button {
  border: none;
  background: transparent;
  color: var(--fm-primary);
  font-size: 10px;
  cursor: pointer;
  padding: 0 0 0 4px;
}

.preview-tag.active-tag {
  background: var(--tag-bg, var(--fm-surface));
  border: 1px solid var(--tag-fg, var(--fm-primary));
  color: var(--tag-fg, var(--fm-primary));
  padding: 0 5px;
}

.view-toggle {
  margin-left: auto;
  display: inline-flex;
  gap: 2px;
  padding: 1px;
  background: var(--fm-surface-alt);
  border-radius: 5px;
}

.view-toggle button {
  width: 22px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--fm-muted);
  font-size: 11px;
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.view-toggle button:hover {
  color: var(--fm-primary);
}

.view-toggle button.active {
  background: var(--fm-surface);
  color: var(--fm-primary);
  box-shadow: var(--fm-shadow-sm);
}

.list-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.list-spin {
  display: block;
}

.list-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 60px 10px;
  color: var(--fm-muted);
  font-size: 12px;
}

.list-empty :deep(svg) {
  font-size: 26px;
}

.group {
  margin-bottom: 8px;
}

.group-title {
  position: sticky;
  top: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 4px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.5px;
  color: var(--fm-muted);
  background: var(--fm-surface);
}

.group-title::before {
  content: '';
  width: 3px;
  height: 11px;
  border-radius: 2px;
  background: var(--fm-primary);
  opacity: 0.55;
}

.group-title.overdue {
  color: var(--fm-danger);
}

.group-title.overdue::before {
  background: var(--fm-danger);
  opacity: 0.85;
}

.group-count {
  font-weight: 500;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 8px;
  padding: 0 6px;
}

.group-title.overdue .group-count {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
}

.note-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 7px 8px;
  border-radius: var(--fm-radius-sm);
  cursor: pointer;
  border-left: 2px solid transparent;
}

.note-row:hover {
  background: var(--fm-surface-alt);
}

.note-row.selected {
  background: var(--fm-primary-soft);
  border-left-color: var(--fm-primary);
}

.note-row.overdue:not(.selected) {
  border-left-color: var(--fm-danger);
}

.note-items.list {
  display: flex;
  flex-direction: column;
}

.note-items.card {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
  padding: 2px;
}

.note-items.card .note-row {
  position: relative;
  flex-direction: column;
  gap: 4px;
  min-height: 96px;
  padding: 10px 10px 26px;
  border: 1px solid var(--fm-border);
  border-left: 3px solid var(--fm-border-strong);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  box-shadow: var(--fm-shadow-sm);
  transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
}

.note-items.card .note-row:hover {
  background: var(--fm-surface);
  border-color: var(--fm-border-strong);
  box-shadow: var(--fm-shadow);
  transform: translateY(-1px);
}

.note-items.card .note-row.selected {
  background: linear-gradient(180deg, var(--fm-primary-soft), var(--fm-surface) 72%);
  border-color: var(--fm-primary);
  border-left-color: var(--fm-primary);
  box-shadow: 0 0 0 2px var(--fm-primary-soft), var(--fm-shadow-sm);
}

.note-items.card .note-row.overdue:not(.selected) {
  background: linear-gradient(180deg, var(--fm-danger-soft), var(--fm-surface) 58%);
  border-left-color: var(--fm-danger);
}

.note-items.card .note-row.done {
  background: var(--fm-surface-alt);
  border-color: var(--fm-border);
  box-shadow: none;
}

.note-items.card .note-row.done:hover {
  transform: none;
}

.note-items.card .row-check {
  position: absolute;
  top: 7px;
  right: 8px;
  margin: 0;
}

.note-items.card .row-body {
  display: flex;
  flex-direction: column;
  flex: 1;
  padding-right: 18px;
}

.note-items.card .row-title {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  font-weight: 500;
}

.note-items.card .row-meta {
  margin-top: auto;
  padding-top: 6px;
}

.note-items.card .row-excerpt {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  white-space: normal;
}

.note-items.card .row-actions {
  position: absolute;
  right: 6px;
  bottom: 3px;
}

.note-items.card .row-btn:hover {
  background: var(--fm-surface-alt);
}

.row-check {
  margin-top: 2px;
}

.row-body {
  flex: 1;
  min-width: 0;
}

.row-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--fm-text);
  line-height: 1.45;
  word-break: break-word;
}

.row-title-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.kind-mark {
  flex: none;
  font-size: 12px;
  color: var(--tag-fg, var(--fm-muted));
}

.note-row.done .row-title {
  text-decoration: line-through;
  color: var(--fm-muted);
}

.row-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 2px;
  font-size: 10px;
  color: var(--fm-muted);
}

.due {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.due.overdue {
  color: var(--fm-danger);
}

.due.none {
  color: var(--fm-muted);
}

.meta-chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  background: var(--fm-surface-alt);
  border-radius: 3px;
  padding: 0 4px;
}

.meta-chip.danger {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
}

.meta-chip.high {
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
}

.meta-chip.snoozed {
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
}

.note-tag {
  display: inline-flex;
  align-items: center;
  max-width: 100px;
  padding: 0 5px;
  border-radius: 4px;
  font-size: 9px;
  line-height: 15px;
  color: var(--tag-fg, var(--fm-text-2));
  background: var(--tag-bg, var(--fm-surface-alt));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.note-tag.more {
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
}

.row-excerpt {
  margin-top: 2px;
  font-size: 10px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
}

.note-row:hover .row-actions,
.note-row.selected .row-actions {
  opacity: 1;
}

.row-btn {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--fm-muted);
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}

.row-btn:hover {
  background: var(--fm-surface);
  color: var(--fm-primary);
}

.row-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.row-btn.danger:hover {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
}

.list-foot {
  border-top: 1px solid var(--fm-border);
  padding-top: 6px;
  margin-top: 4px;
  text-align: right;
}

.menu-title {
  margin-left: 6px;
  font-weight: 500;
}

.menu-desc {
  display: block;
  margin-left: 22px;
  font-size: 10px;
  line-height: 1.5;
  color: var(--fm-muted);
}

.undo-bar {
  position: fixed;
  left: 50%;
  bottom: 26px;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  background: var(--fm-text);
  color: #fff;
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow-lg);
  font-size: 12px;
  z-index: 30;
}

.undo-bar :deep(.ant-btn-link) {
  color: #9db1ff;
  padding: 0;
  height: auto;
}

.undo-fade-enter-active,
.undo-fade-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.undo-fade-enter-from,
.undo-fade-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}

.settings-form {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 4px 0;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.setting-row.indent {
  justify-content: flex-start;
  padding-left: 8px;
}

.range-dash {
  font-size: 11px;
  color: var(--fm-muted);
}

.setting-title {
  font-size: 12px;
  color: var(--fm-text);
}

.setting-desc {
  font-size: 11px;
  color: var(--fm-muted);
  margin-top: 2px;
}

.settings-tip {
  font-size: 11px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-sm);
  padding: 8px 10px;
  line-height: 1.6;
}
.notes-page :deep(.page-header) {
  flex-wrap: wrap;
}

.notes-page :deep(.actions) {
  flex-wrap: wrap;
  justify-content: flex-end;
}
</style>
