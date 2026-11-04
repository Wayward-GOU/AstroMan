<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import dayjs, { type Dayjs } from 'dayjs'
import {
  CameraOutlined,
  DeleteOutlined,
  InboxOutlined,
  LoadingOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons-vue'
import { aiProviderList } from '@/api'
import {
  notesCreateFromImage,
  notesExtractCancel,
  notesExtractImage,
} from '@/api/notes'
import { useNotesStore } from '@/stores/notes'
import type { AiProvider } from '@/types/models'
import type { CreateFromImageResult, ExtractItem, NoteInput, NoteRepeat } from '@/types/notes'
import {
  MAX_IMAGE_BYTES,
  compressForRecognition,
  formatBytes,
  readImageFile,
  toDataUrl,
} from '@/utils/notesImage'

const props = defineProps<{ open: boolean }>()

const emit = defineEmits<{
  (event: 'update:open', value: boolean): void
  (event: 'created', result: CreateFromImageResult): void
}>()

interface ImageInfo {
  fileName: string
  mime: string
  base64: string
  size: number
  previewUrl: string
}

interface EditableItem {
  key: number
  selected: boolean
  title: string
  detail: string
  due: Dayjs | null
  repeat: NoteRepeat
  priority: number
  tags: string[]
  warning: string | null
}

const router = useRouter()
const notesStore = useNotesStore()

const providers = ref<AiProvider[]>([])
const providerId = ref<number | null>(null)
const providersLoaded = ref(false)

const image = ref<ImageInfo | null>(null)
const dragging = ref(false)
const fileInput = ref<HTMLInputElement | null>(null)

const state = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
const errorText = ref('')
const items = ref<EditableItem[]>([])
const summary = ref('')
const rawText = ref('')
const generating = ref(false)
let requestId = ''
let rowSeed = 1

const repeatOptions = [
  { value: 'none', label: '不重复' },
  { value: 'daily', label: '每天' },
  { value: 'weekdays', label: '工作日' },
  { value: 'weekly', label: '每周' },
  { value: 'monthly', label: '每月' },
]

const priorityOptions = [
  { value: 0, label: '低' },
  { value: 1, label: '普通' },
  { value: 2, label: '高' },
]

const visionProviders = computed(() =>
  providers.value.filter((item) => item.supportsVision && item.id != null),
)

const currentProvider = computed(
  () => visionProviders.value.find((item) => item.id === providerId.value) ?? null,
)

const selectedCount = computed(() => items.value.filter((item) => item.selected).length)

const canExtract = computed(
  () => Boolean(image.value) && Boolean(providerId.value) && state.value !== 'loading',
)

async function loadProviders() {
  if (providersLoaded.value) return
  try {
    const [list, settings] = await Promise.all([
      aiProviderList(),
      notesStore.loadSettings().then(() => notesStore.settings),
    ])
    providers.value = list
    providersLoaded.value = true
    const preferred = settings.visionProviderId
    if (preferred && visionProviders.value.some((item) => item.id === preferred)) {
      providerId.value = preferred
    } else {
      providerId.value = visionProviders.value[0]?.id ?? null
    }
  } catch {
    providers.value = []
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return
    errorText.value = ''
    if (!providersLoaded.value) void loadProviders()
  },
)

function reset() {
  if (state.value === 'loading' && requestId) {
    void notesExtractCancel(requestId).catch(() => undefined)
  }
  requestId = ''
  image.value = null
  items.value = []
  summary.value = ''
  rawText.value = ''
  state.value = 'idle'
  errorText.value = ''
  generating.value = false
}

function close() {
  if (generating.value) return
  reset()
  emit('update:open', false)
}

function pickFile() {
  if (state.value === 'loading') return
  fileInput.value?.click()
}

async function acceptFile(file: File) {
  if (generating.value) return
  try {
    const payload = await readImageFile(file)
    image.value = {
      fileName: payload.name,
      mime: payload.mime,
      base64: payload.base64,
      size: payload.size,
      previewUrl: toDataUrl(payload.base64, payload.mime),
    }
    items.value = []
    summary.value = ''
    rawText.value = ''
    state.value = 'idle'
    errorText.value = ''
  } catch (error) {
    message.error(String(error instanceof Error ? error.message : error))
  }
}

async function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (file) await acceptFile(file)
}

function onPaste(event: ClipboardEvent) {
  if (!props.open || generating.value) return
  const file = Array.from(event.clipboardData?.items ?? [])
    .find((item) => item.type.startsWith('image/'))
    ?.getAsFile()
  if (!file) return
  event.preventDefault()
  void acceptFile(file)
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  dragging.value = false
  if (generating.value) return
  const file = event.dataTransfer?.files?.[0]
  if (file) void acceptFile(file)
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  dragging.value = true
}

function onDragLeave() {
  dragging.value = false
}

function blankRow(): EditableItem {
  return {
    key: rowSeed++,
    selected: true,
    title: '',
    detail: '',
    due: null,
    repeat: 'none',
    priority: 1,
    tags: [],
    warning: null,
  }
}

function toEditable(item: ExtractItem): EditableItem {
  return {
    key: rowSeed++,
    selected: true,
    title: item.title,
    detail: item.detail,
    due: item.dueAt ? dayjs(item.dueAt) : null,
    repeat: (item.repeat as NoteRepeat) ?? 'none',
    priority: item.priority,
    tags: [...item.tags],
    warning: item.warning,
  }
}

function newRequestId(): string {
  const cryptoRef = globalThis.crypto as Crypto | undefined
  if (cryptoRef && typeof cryptoRef.randomUUID === 'function') return cryptoRef.randomUUID()
  return `notes-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

async function extract() {
  if (!image.value || !providerId.value || state.value === 'loading') return
  state.value = 'loading'
  errorText.value = ''
  const current = newRequestId()
  requestId = current
  try {
    const compressed = await compressForRecognition(image.value.base64, image.value.mime)
    const result = await notesExtractImage({
      providerId: providerId.value,
      mime: compressed.mime,
      base64: compressed.base64,
      requestId: current,
    })
    if (requestId !== current) return
    summary.value = result.summary
    rawText.value = result.rawText
    items.value = result.items.map(toEditable)
    state.value = 'ready'
  } catch (error) {
    if (requestId !== current) return
    state.value = 'error'
    errorText.value = String(error)
  } finally {
    if (requestId === current) requestId = ''
  }
}

function cancelExtract() {
  if (requestId) {
    void notesExtractCancel(requestId).catch(() => undefined)
    requestId = ''
  }
  state.value = 'idle'
}

function retry() {
  void extract()
}

function manual() {
  state.value = 'ready'
  items.value = [blankRow()]
  summary.value = ''
  rawText.value = ''
  errorText.value = ''
}

function toggleAll(value: boolean) {
  items.value.forEach((item) => {
    item.selected = value
  })
}

function removeRow(index: number) {
  items.value.splice(index, 1)
}

function addRow() {
  items.value.push(blankRow())
}

function buildTasks(): NoteInput[] {
  return items.value
    .filter((item) => item.selected && (item.title.trim() || item.detail.trim()))
    .map((item) => ({
      title: item.title.trim(),
      content: item.detail.trim(),
      priority: item.priority,
      tags: item.tags,
      dueAt: item.due ? item.due.valueOf() : null,
      repeat: item.due ? item.repeat : 'none',
    }))
}

async function generate() {
  if (!image.value || generating.value) return
  const tasks = buildTasks()
  if (!tasks.length) {
    message.warning('先勾选至少一条待办')
    return
  }
  if (!providerId.value) {
    message.warning('请先在设置里选择识别服务')
    return
  }
  generating.value = true
  try {
    const result = await notesCreateFromImage({
      fileName: image.value.fileName,
      mime: image.value.mime,
      base64: image.value.base64,
      tasks,
    })
    message.success(`已生成 ${result.noteIds.length} 条，原图已附上`)
    emit('created', result)
    reset()
    emit('update:open', false)
  } catch (error) {
    message.error(String(error))
  } finally {
    generating.value = false
  }
}

async function saveAsSingle() {
  if (!image.value || generating.value) return
  const title = summary.value.trim() || image.value.fileName.replace(/\.[^.]+$/, '') || '图片记录'
  const content = rawText.value.trim() || summary.value.trim()
  generating.value = true
  try {
    const result = await notesCreateFromImage({
      fileName: image.value.fileName,
      mime: image.value.mime,
      base64: image.value.base64,
      tasks: [{ title, content }],
    })
    message.success('已保存为一条记录，原图已附上')
    emit('created', result)
    reset()
    emit('update:open', false)
  } catch (error) {
    message.error(String(error))
  } finally {
    generating.value = false
  }
}

function goProviders() {
  close()
  void router.push('/ai')
}
</script>

<template>
  <a-modal
    :open="open"
    :width="900"
    :mask-closable="!generating"
    :closable="!generating"
    :keyboard="!generating"
    title="截图生成待办"
    wrap-class-name="notes-import-modal"
    @cancel="close"
  >
    <div
      class="import-body"
      @paste="onPaste"
      @drop="onDrop"
      @dragover="onDragOver"
      @dragleave="onDragLeave"
    >
      <section class="image-pane">
        <div class="pane-title">原图</div>
        <div v-if="!image" class="drop-zone" :class="{ dragging }" @click="pickFile">
          <InboxOutlined class="drop-icon" />
          <div class="drop-title">粘贴截图（Ctrl+V）· 拖入图片 · 选择文件</div>
          <div class="drop-desc">PNG / JPG / WebP，单张不超过 {{ formatBytes(MAX_IMAGE_BYTES) }}</div>
        </div>
        <div v-else class="image-card">
          <img :src="image.previewUrl" :alt="image.fileName" />
          <div class="image-meta">
            <span class="image-name" :title="image.fileName">{{ image.fileName }}</span>
            <span class="image-size">{{ formatBytes(image.size) }}</span>
          </div>
          <div class="image-actions">
            <a-button size="small" :disabled="state === 'loading'" @click="pickFile">
              更换图片
            </a-button>
          </div>
        </div>
        <div class="provider-row">
          <span class="provider-label">识别服务</span>
          <a-select
            v-model:value="providerId"
            size="small"
            style="flex: 1; min-width: 0"
            :options="visionProviders.map((item) => ({ value: item.id!, label: item.name }))"
            :placeholder="visionProviders.length ? '选择服务商' : '没有支持图片的服务商'"
          />
        </div>
        <div v-if="visionProviders.length" class="provider-hint">
          图片将发送至 {{ currentProvider?.name ?? '所选服务商' }}，仅用于本次识别。
        </div>
        <div v-else class="provider-hint warn">
          还没有勾选「支持图片识别」的模型服务。
          <a @click="goProviders">去 AI 助手配置</a>
        </div>
        <input
          ref="fileInput"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          hidden
          @change="onFileChange"
        />
      </section>

      <section class="list-pane">
        <div class="pane-head">
          <div class="pane-title">待办清单</div>
          <div v-if="state === 'ready' && items.length" class="select-tools">
            <a-checkbox
              :checked="selectedCount === items.length"
              :indeterminate="selectedCount > 0 && selectedCount < items.length"
              @change="toggleAll(Boolean($event.target?.checked))"
            >
              全选
            </a-checkbox>
            <span class="select-count">识别到 {{ items.length }} 条</span>
            <a-button size="small" type="text" @click="retry">重新识别</a-button>
          </div>
        </div>

        <div v-if="state === 'loading'" class="state-box">
          <LoadingOutlined class="state-icon spin" />
          <div class="state-title">正在识别…</div>
          <div class="state-desc">最长等待 60 秒，识别结果只用于预览，不会直接保存。</div>
          <a-button size="small" @click="cancelExtract">取消</a-button>
        </div>

        <div v-else-if="state === 'error'" class="state-box">
          <div class="state-title error">识别失败</div>
          <div class="state-desc error-text">{{ errorText }}</div>
          <div class="state-actions">
            <a-button size="small" @click="retry">
              <template #icon><ReloadOutlined /></template>
              重试
            </a-button>
            <a-button size="small" @click="manual">转为手动记录</a-button>
          </div>
        </div>

        <div v-else-if="state === 'ready' && !items.length" class="state-box">
          <div class="state-title">没有识别出待办</div>
          <div class="state-desc">可以把识别出的文字保存成一条普通记录，原图同样会附上。</div>
          <div class="state-actions">
            <a-button size="small" @click="manual">
              <template #icon><PlusOutlined /></template>
              手动补一条
            </a-button>
            <a-button type="primary" size="small" :loading="generating" @click="saveAsSingle">
              保存为一条记录
            </a-button>
          </div>
        </div>

        <div v-else-if="state === 'ready'" class="rows">
          <div v-if="summary" class="summary-line">{{ summary }}</div>
          <div
            v-for="(item, index) in items"
            :key="item.key"
            class="row"
            :class="{ unchecked: !item.selected, warned: Boolean(item.warning) }"
          >
            <a-checkbox v-model:checked="item.selected" class="row-check" />
            <div class="row-main">
              <a-input v-model:value="item.title" size="small" placeholder="标题" maxlength="200" />
              <div class="row-options">
                <a-date-picker
                  v-model:value="item.due"
                  size="small"
                  show-time
                  format="YYYY-MM-DD HH:mm"
                  placeholder="无截止时间"
                  style="width: 158px"
                />
                <a-select
                  v-model:value="item.repeat"
                  size="small"
                  :options="repeatOptions"
                  :disabled="!item.due"
                  style="width: 92px"
                />
                <a-select
                  v-model:value="item.priority"
                  size="small"
                  :options="priorityOptions"
                  style="width: 82px"
                />
                <a-select
                  v-model:value="item.tags"
                  size="small"
                  mode="tags"
                  :max-tag-count="2"
                  placeholder="标签"
                  style="flex: 1; min-width: 90px"
                />
              </div>
              <a-input
                v-if="item.detail"
                v-model:value="item.detail"
                size="small"
                class="detail-input"
                placeholder="补充说明"
              />
              <div v-if="item.warning" class="row-warning">{{ item.warning }}</div>
            </div>
            <a-button
              type="text"
              size="small"
              danger
              class="row-remove"
              @click="removeRow(index)"
            >
              <template #icon><DeleteOutlined /></template>
            </a-button>
          </div>
          <button type="button" class="add-row" @click="addRow">
            <PlusOutlined /> 补充一条
          </button>
        </div>

        <div v-else class="state-box idle">
          <CameraOutlined class="state-icon" />
          <div class="state-title">先放一张截图</div>
          <div class="state-desc">
            可以直接粘贴截图，识别前会先压缩一份用于上传，原图仍按原样保存为附件。
          </div>
        </div>
      </section>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <span class="footer-hint">识别结果确认后才会写入记事本</span>
        <a-button :disabled="generating" @click="close">取消</a-button>
        <a-button v-if="state !== 'ready'" type="primary" :disabled="!canExtract" @click="extract">
          {{ state === 'error' ? '重新识别' : '开始识别' }}
        </a-button>
        <a-button
          v-else
          type="primary"
          :loading="generating"
          :disabled="!selectedCount"
          @click="generate"
        >
          生成 {{ selectedCount }} 条待办
        </a-button>
      </div>
    </template>
  </a-modal>
</template>

<style scoped>
.import-body {
  display: grid;
  grid-template-columns: minmax(260px, 0.85fr) minmax(360px, 1.15fr);
  gap: 14px;
  min-height: 420px;
}

@media (max-width: 760px) {
  .import-body {
    grid-template-columns: 1fr;
  }
}

.image-pane,
.list-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.pane-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.drop-zone {
  flex: 1;
  min-height: 260px;
  border: 1px dashed var(--fm-border-strong);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  text-align: center;
  padding: 20px;
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}

.drop-zone.dragging {
  border-color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.drop-icon {
  font-size: 30px;
  color: var(--fm-primary);
}

.drop-title {
  font-size: 12px;
  color: var(--fm-text-2);
}

.drop-desc {
  font-size: 11px;
  color: var(--fm-muted);
}

.image-card {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.image-card img {
  display: block;
  width: 100%;
  max-height: 300px;
  object-fit: contain;
  background: #0f1420;
}

.image-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  font-size: 11px;
  border-top: 1px solid var(--fm-border);
}

.image-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.image-size {
  color: var(--fm-muted);
}

.image-actions {
  padding: 0 8px 8px;
}

.provider-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.provider-label {
  font-size: 11px;
  color: var(--fm-muted);
  flex-shrink: 0;
}

.provider-hint {
  font-size: 11px;
  color: var(--fm-muted);
}

.provider-hint.warn {
  color: var(--fm-warning);
}

.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.select-tools {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 11px;
}

.select-count {
  color: var(--fm-muted);
}

.state-box {
  flex: 1;
  min-height: 260px;
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  text-align: center;
  padding: 20px;
}

.state-box.idle {
  color: var(--fm-muted);
}

.state-icon {
  font-size: 28px;
  color: var(--fm-muted);
}

.state-icon.spin {
  animation: notes-spin 1s linear infinite;
  color: var(--fm-primary);
}

@keyframes notes-spin {
  to {
    transform: rotate(360deg);
  }
}

.state-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.state-title.error {
  color: var(--fm-danger);
}

.state-desc {
  font-size: 11px;
  color: var(--fm-muted);
  max-width: 320px;
}

.state-desc.error-text {
  color: var(--fm-danger);
  word-break: break-word;
}

.state-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 420px;
  overflow-y: auto;
  padding-right: 2px;
}

.summary-line {
  font-size: 11px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-radius: var(--fm-radius-sm);
  padding: 6px 8px;
}

.row {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 8px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
}

.row.unchecked {
  opacity: 0.55;
}

.row.warned {
  background: var(--fm-warning-soft);
  border-color: #f2d9a8;
}

.row-check {
  margin-top: 4px;
}

.row-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.row-options {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.detail-input {
  font-size: 11px;
}

.row-warning {
  font-size: 11px;
  color: var(--fm-warning);
}

.row-remove {
  flex-shrink: 0;
}

.add-row {
  border: 1px dashed var(--fm-border-strong);
  border-radius: var(--fm-radius-sm);
  background: transparent;
  color: var(--fm-muted);
  font-size: 12px;
  padding: 6px;
  cursor: pointer;
}

.add-row:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.dialog-footer {
  display: flex;
  align-items: center;
  gap: 8px;
}

.footer-hint {
  flex: 1;
  text-align: left;
  font-size: 11px;
  color: var(--fm-muted);
}
</style>
