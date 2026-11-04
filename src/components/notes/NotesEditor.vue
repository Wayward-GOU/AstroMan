<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import dayjs, { type Dayjs } from 'dayjs'
import { PaperClipOutlined, PlusOutlined, DeleteOutlined, PushpinFilled, PushpinOutlined } from '@ant-design/icons-vue'
import {
  notesAttachmentAdd,
  notesAttachmentData,
  notesAttachmentList,
  notesAttachmentRemove,
  notesDelete,
  notesGet,
  notesUpdate,
} from '@/api/notes'
import { MAX_ATTACHMENTS, readImageFile, toDataUrl } from '@/utils/notesImage'
import { formatDueLabel } from '@/utils/notesQuickParse'
import type {
  NoteDetail,
  NoteFile,
  NoteKind,
  NotePatch,
  NoteRepeat,
  TagCount,
} from '@/types/notes'

const props = defineProps<{
  note: NoteDetail | null
  tags: TagCount[]
}>()

const emit = defineEmits<{
  (event: 'saved', note: NoteDetail): void
  (event: 'deleted', id: number): void
  (event: 'close'): void
  (event: 'open-image', payload: { files: NoteFile[]; index: number }): void
  (event: 'attachment-changed'): void
}>()

const form = reactive({
  title: '',
  content: '',
  kind: 'todo' as NoteKind,
  priority: 1,
  tags: [] as string[],
  due: null as Dayjs | null,
  lead: 0,
  repeat: 'none' as NoteRepeat,
  pinned: false,
})

const attachments = ref<NoteFile[]>([])
const previews = ref<Record<number, string>>({})
const saving = ref(false)
const uploading = ref(false)
const saveState = ref<'idle' | 'dirty' | 'saving' | 'saved'>('idle')
let autoTimer: number | undefined
let suppress = false
let ready = false
const fileInput = ref<HTMLInputElement | null>(null)

const priorityOptions = [
  { value: 0, label: '低' },
  { value: 1, label: '普通' },
  { value: 2, label: '高' },
]

const kindOptions: { value: NoteKind; label: string }[] = [
  { value: 'todo', label: '待办' },
  { value: 'note', label: '笔记' },
  { value: 'idea', label: '灵感' },
  { value: 'reference', label: '资料' },
]

const leadOptions = [
  { value: 0, label: '准点提醒' },
  { value: 5, label: '提前 5 分钟' },
  { value: 15, label: '提前 15 分钟' },
  { value: 30, label: '提前 30 分钟' },
  { value: 60, label: '提前 1 小时' },
  { value: 120, label: '提前 2 小时' },
  { value: 1440, label: '提前 1 天' },
]

const repeatOptions = [
  { value: 'none', label: '不重复' },
  { value: 'daily', label: '每天' },
  { value: 'weekdays', label: '工作日' },
  { value: 'weekly', label: '每周' },
  { value: 'monthly', label: '每月' },
]

const tagOptions = computed(() =>
  props.tags.map((tag) => ({ value: tag.name, label: `${tag.name} · ${tag.count}` })),
)

function applyNote(note: NoteDetail | null) {
  if (!note) return
  suppress = true
  ready = false
  form.title = note.title
  form.content = note.content
  form.kind = note.kind
  form.priority = note.priority
  form.tags = [...note.tags]
  form.due = note.dueAt ? dayjs(note.dueAt) : null
  form.lead = note.remindLeadMin
  form.repeat = note.repeat
  form.pinned = note.pinned
  saveState.value = 'idle'
  void nextTick(() => {
    suppress = false
    ready = true
  })
}

const saveText = computed(() => {
  if (saveState.value === 'saving') return '保存中…'
  if (saveState.value === 'dirty') return '未保存'
  if (saveState.value === 'saved') return '已保存'
  return ''
})

/** 自动保存除截止时间外的字段；清空截止仍需点保存并二次确认 */
function autoPatch(): NotePatch {
  return {
    title: form.title,
    content: form.content,
    kind: form.kind,
    priority: form.priority,
    tags: form.tags,
    remindLeadMin: dueEnabled.value ? form.lead : 0,
    repeat: dueEnabled.value ? form.repeat : 'none',
    pinned: form.pinned,
  }
}

async function autoSave() {
  if (!props.note || !ready) return
  autoTimer = undefined
  saveState.value = 'saving'
  try {
    const updated = await notesUpdate(props.note.id, autoPatch())
    saveState.value = 'saved'
    emit('saved', updated)
  } catch {
    saveState.value = 'dirty'
  }
}

watch(
  () => form.due?.valueOf() ?? null,
  () => {
    // 截止时间不自动保存（清空需二次确认），但要让用户知道还有改动没落库
    if (!props.note || suppress || !ready || saving.value) return
    if (saveState.value !== 'saving') saveState.value = 'dirty'
  },
)

watch(
  () => [form.title, form.content, form.kind, form.priority, form.tags.join('|'), form.lead, form.repeat, form.pinned],
  () => {
    if (!props.note || suppress || !ready) return
    saveState.value = 'dirty'
    if (autoTimer) window.clearTimeout(autoTimer)
    autoTimer = window.setTimeout(() => {
      void autoSave()
    }, 700)
  },
)

onBeforeUnmount(() => {
  if (!autoTimer) return
  window.clearTimeout(autoTimer)
  autoTimer = undefined
  void autoSave()
})

watch(
  () => props.note?.id,
  async (id, previous) => {
    // 切换条目前把还没落库的编辑写回上一条，避免 700ms 窗口内的内容丢失
    if (previous && previous !== id && autoTimer) {
      window.clearTimeout(autoTimer)
      autoTimer = undefined
      saveState.value = 'saving'
      try {
        await notesUpdate(previous, autoPatch())
      } catch {
        // 保存旧条目失败时保留当前内容，用户仍可在新条目里重新编辑
      }
    }
    if (!id) {
      ready = false
      suppress = true
      return
    }
    applyNote(props.note)
    await loadAttachments(id)
  },
  { immediate: true },
)

async function loadAttachments(noteId: number) {
  try {
    attachments.value = await notesAttachmentList(noteId)
    const next: Record<number, string> = {}
    await Promise.all(
      attachments.value.map(async (file) => {
        try {
          const data = await notesAttachmentData(file.id)
          next[file.id] = toDataUrl(data.base64, data.mime)
        } catch {
          // 单张读取失败不影响其余附件
        }
      }),
    )
    previews.value = next
  } catch (error) {
    message.error(String(error))
  }
}

const dueEnabled = computed(() => form.due !== null)

const remindHint = computed(() => {
  if (!form.due) return '未设置截止时间，不会提醒'
  const due = form.due.valueOf()
  const base = due - form.lead * 60_000
  const snooze = props.note?.snoozeUntil ?? 0
  // 稍后提醒不改截止时间，只顺延真正触发的那一刻；提示里要跟着变
  if (snooze > Date.now()) {
    return `下次提醒：${formatDueLabel(Math.max(base, snooze))}（已稍后）`
  }
  if (due < Date.now()) return '截止时间已过，保存后会立即提醒'
  return `下次提醒：${formatDueLabel(base)}`
})

async function save() {
  if (!props.note) return
  if (!form.title.trim() && !form.content.trim()) {
    message.warning('标题和正文不能同时为空')
    return
  }
  const nextDue = form.due ? form.due.valueOf() : null
  const currentDue = props.note.dueAt ?? null
  let patch: NotePatch = {
    title: form.title,
    content: form.content,
    kind: form.kind,
    priority: form.priority,
    tags: form.tags,
    remindLeadMin: dueEnabled.value ? form.lead : 0,
    repeat: dueEnabled.value ? form.repeat : 'none',
    pinned: form.pinned,
  }
  if (nextDue !== currentDue) {
    if (nextDue === null && currentDue !== null) {
      const confirmed = await new Promise<boolean>((resolve) => {
        Modal.confirm({
          title: '清空截止时间？',
          content: '清空后提醒时间、提前量与重复规则会一并清除。',
          okText: '清空',
          okType: 'danger',
          cancelText: '取消',
          onOk: () => resolve(true),
          onCancel: () => resolve(false),
        })
      })
      if (!confirmed) return
    }
    patch = { ...patch, dueAt: nextDue }
  }
  if (autoTimer) window.clearTimeout(autoTimer)
  autoTimer = undefined
  saving.value = true
  saveState.value = 'saving'
  try {
    const updated = await notesUpdate(props.note.id, patch)
    applyNote(updated)
    saveState.value = 'saved'
    emit('saved', updated)
    message.success('已保存')
  } catch (error) {
    saveState.value = 'dirty'
    message.error(String(error))
  } finally {
    saving.value = false
  }
}

function remove() {
  if (!props.note) return
  Modal.confirm({
    title: `删除「${props.note.title || '这条记录'}」？`,
    content: '删除后 6 秒内可以撤销，30 天后彻底清理。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: () => {
      if (autoTimer) {
        window.clearTimeout(autoTimer)
        autoTimer = undefined
      }
      emit('deleted', props.note!.id)
    },
  })
}

async function uploadBase64(payload: { name: string; mime: string; base64: string }) {
  if (!props.note) return
  if (attachments.value.length >= MAX_ATTACHMENTS) {
    message.warning(`单条记录最多 ${MAX_ATTACHMENTS} 张图片`)
    return
  }
  uploading.value = true
  try {
    await notesAttachmentAdd({
      noteId: props.note.id,
      fileName: payload.name,
      mime: payload.mime,
      base64: payload.base64,
    })
    await loadAttachments(props.note.id)
    emit('attachment-changed')
    const fresh = await notesGet(props.note.id)
    emit('saved', fresh)
  } catch (error) {
    message.error(String(error))
  } finally {
    uploading.value = false
  }
}

function pickFile() {
  fileInput.value?.click()
}

async function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  try {
    await uploadBase64(await readImageFile(file))
  } catch (error) {
    message.error(String(error))
  }
}

async function onDropImage(event: DragEvent) {
  const file = Array.from(event.dataTransfer?.files ?? []).find((item) =>
    item.type.startsWith('image/'),
  )
  if (!file) return
  event.preventDefault()
  try {
    await uploadBase64(await readImageFile(file))
  } catch (error) {
    message.error(String(error))
  }
}

async function onPaste(event: ClipboardEvent) {
  const file = Array.from(event.clipboardData?.files ?? []).find((item) =>
    item.type.startsWith('image/'),
  )
  if (!file) return
  event.preventDefault()
  try {
    await uploadBase64(await readImageFile(file))
  } catch (error) {
    message.error(String(error))
  }
}

function removeAttachment(file: NoteFile) {
  if (!props.note) return
  Modal.confirm({
    title: '移除这张图片？',
    content: '只解除与当前记录的关联，仍被其他记录引用的图片不会删除。',
    okText: '移除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        await notesAttachmentRemove(props.note!.id, file.id)
        await loadAttachments(props.note!.id)
        emit('attachment-changed')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

function onKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
    event.preventDefault()
    void save()
  }
}
</script>

<template>
  <div
    v-if="note"
    class="editor"
    tabindex="0"
    @keydown="onKeydown"
    @dragover.prevent
    @drop.prevent="onDropImage"
  >
    <div class="editor-head">
      <input v-model="form.title" class="title-input" placeholder="标题" maxlength="200" />
      <a-tooltip :title="form.pinned ? '取消置顶' : '置顶'">
        <button
          class="pin-btn"
          :class="{ active: form.pinned }"
          type="button"
          @click="form.pinned = !form.pinned"
        >
          <PushpinFilled v-if="form.pinned" />
          <PushpinOutlined v-else />
        </button>
      </a-tooltip>
      <a-tooltip title="收起编辑">
        <a-button size="small" type="text" @click="emit('close')">×</a-button>
      </a-tooltip>
    </div>

    <div class="kind-row">
      <a-radio-group v-model:value="form.kind" size="small" button-style="solid" class="kind-switch">
        <a-radio-button v-for="option in kindOptions" :key="option.value" :value="option.value">
          {{ option.label }}
        </a-radio-button>
      </a-radio-group>
    </div>

    <div class="meta-row">
      <a-select
        v-model:value="form.priority"
        size="small"
        :options="priorityOptions"
        style="width: 78px"
      />
      <a-select
        v-model:value="form.tags"
        size="small"
        mode="tags"
        :options="tagOptions"
        :max-tag-count="3"
        placeholder="标签"
        style="flex: 1; min-width: 120px"
      />
    </div>

    <a-textarea
      v-model:value="form.content"
      class="content-input"
      placeholder="正文…（可直接粘贴截图作为附件）"
      :auto-size="{ minRows: 8, maxRows: 20 }"
      @paste="onPaste"
    />

    <div class="section-line">
      <span class="section-label">附件</span>
      <span v-if="attachments.length" class="attach-count">
        {{ attachments.length }}/{{ MAX_ATTACHMENTS }}
      </span>
    </div>
    <div class="attachments">
      <button
        v-for="(file, index) in attachments"
        :key="file.id"
        type="button"
        class="attachment"
        :title="file.fileName"
        @click="emit('open-image', { files: attachments, index })"
      >
        <img v-if="previews[file.id]" :src="previews[file.id]" :alt="file.fileName" />
        <span v-else class="attachment-fallback"><PaperClipOutlined /></span>
        <span class="attachment-remove" title="移除" @click.stop="removeAttachment(file)">×</span>
      </button>
      <button
        v-if="attachments.length < MAX_ATTACHMENTS"
        type="button"
        class="attachment add"
        :disabled="uploading"
        title="添加图片"
        @click="pickFile"
      >
        <PlusOutlined />
      </button>
      <input
        ref="fileInput"
        type="file"
        accept="image/png,image/jpeg,image/webp"
        hidden
        @change="onFileChange"
      />
    </div>

    <div class="section-label">截止与提醒</div>
    <div class="schedule-grid">
      <a-date-picker
        v-model:value="form.due"
        show-time
        format="YYYY-MM-DD HH:mm"
        placeholder="无截止时间"
        :allow-clear="true"
        style="width: 100%"
      />
      <a-select
        v-model:value="form.lead"
        :options="leadOptions"
        :disabled="!dueEnabled"
        style="width: 100%"
      />
      <a-select
        v-model:value="form.repeat"
        :options="repeatOptions"
        :disabled="!dueEnabled"
        style="width: 100%"
      />
    </div>
    <div class="remind-hint">{{ remindHint }}</div>

    <div class="editor-actions">
      <a-button type="text" danger size="small" @click="remove">
        <template #icon><DeleteOutlined /></template>
        删除
      </a-button>
      <span v-if="saveText" class="save-state" :class="saveState">{{ saveText }}</span>
      <span class="spacer" />
      <a-button type="primary" size="small" :loading="saving" @click="save">
        保存
      </a-button>
    </div>
  </div>
  <div v-else class="editor empty">
    <div class="empty-title">选择一条记录查看详情</div>
    <div class="empty-desc">也可以在左上角先记一笔，随时回来补充。</div>
  </div>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
  outline: none;
}

.editor.empty {
  align-items: center;
  justify-content: center;
  color: var(--fm-muted);
  text-align: center;
  gap: 6px;
}

.empty-title {
  font-size: 13px;
  color: var(--fm-text-2);
}

.empty-desc {
  font-size: 11px;
}

.editor-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.title-input {
  flex: 1;
  min-width: 0;
  border: none;
  border-bottom: 1px dashed var(--fm-border-strong);
  background: transparent;
  color: var(--fm-text);
  font-size: 18px;
  font-weight: 600;
  padding: 4px 0 8px;
  outline: none;
}

.title-input::placeholder {
  color: var(--fm-muted);
  font-weight: 400;
}

.pin-btn {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  color: var(--fm-muted);
  cursor: pointer;
  border-radius: 6px;
}

.pin-btn.active {
  color: var(--fm-primary);
}

.kind-row {
  display: flex;
  align-items: center;
}

.kind-switch :deep(.ant-radio-button-wrapper) {
  height: 26px;
  line-height: 24px;
  padding: 0 10px;
  font-size: 12px;
  color: var(--fm-text-2);
  background: var(--fm-surface);
  border-color: var(--fm-border);
}

.kind-switch :deep(.ant-radio-button-wrapper-checked) {
  color: var(--fm-primary);
  border-color: var(--fm-primary);
  background: var(--fm-primary-soft);
  box-shadow: none;
}

.meta-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.meta-row :deep(.ant-select-selection-item) {
  background: var(--fm-primary-soft);
  border-color: transparent;
  color: var(--fm-primary);
}

.meta-row :deep(.ant-select-selection-item-remove) {
  color: var(--fm-primary);
}

.content-input {
  resize: none;
  font-size: 13px;
  line-height: 26px;
  padding: 4px 8px;
  background-color: transparent;
  background-image: repeating-linear-gradient(
    to bottom,
    transparent 0 25px,
    var(--fm-border) 25px 26px
  );
  background-attachment: local;
}

.section-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 2px;
}

.section-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.attach-count {
  font-size: 10px;
  color: var(--fm-muted);
}

.attachments {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.attachment {
  position: relative;
  width: 52px;
  height: 52px;
  border-radius: 6px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  overflow: hidden;
  cursor: pointer;
  display: grid;
  place-items: center;
  padding: 0;
}

.attachment img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.attachment.add {
  color: var(--fm-muted);
  border-style: dashed;
}

.attachment-remove {
  position: absolute;
  top: 1px;
  right: 1px;
  width: 15px;
  height: 15px;
  line-height: 13px;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  font-size: 11px;
  opacity: 0;
}

.attachment:hover .attachment-remove {
  opacity: 1;
}

.schedule-grid {
  display: grid;
  gap: 6px;
}

.remind-hint {
  font-size: 10px;
  color: var(--fm-muted);
}

.save-state {
  font-size: 11px;
  color: var(--fm-muted);
}

.save-state.dirty {
  color: var(--fm-warning);
}

.save-state.saved {
  color: var(--fm-success);
}

.editor-actions {
  display: flex;
  align-items: center;
  margin-top: auto;
  padding-top: 8px;
  border-top: 1px solid var(--fm-border);
}

.spacer {
  flex: 1;
}
</style>
