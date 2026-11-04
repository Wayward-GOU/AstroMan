<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { notesCreate } from '@/api/notes'
import { formatDueLabel, parseQuickNote } from '@/utils/notesQuickParse'

const DRAFT_KEY = 'astroman:notes:quick-draft'
const HINT_KEY = 'astroman:notes:quick-hint'

const input = ref('')
const saving = ref(false)
const errorText = ref('')
const showHint = ref(false)
const inputRef = ref<HTMLTextAreaElement | null>(null)
let composing = false

const parsed = computed(() => parseQuickNote(input.value, new Date()))
const canSave = computed(() => Boolean(input.value.trim()) && Boolean(parsed.value.title.trim()))

function loadDraft() {
  try {
    input.value = localStorage.getItem(DRAFT_KEY) ?? ''
    showHint.value = localStorage.getItem(HINT_KEY) !== '0' && !input.value
  } catch {
    input.value = ''
  }
}

function saveDraft() {
  try {
    if (input.value) localStorage.setItem(DRAFT_KEY, input.value)
    else localStorage.removeItem(DRAFT_KEY)
  } catch {
    // 忽略存储失败
  }
}

function clearDraft() {
  try {
    localStorage.removeItem(DRAFT_KEY)
  } catch {
    // 忽略存储失败
  }
}

function dismissHint() {
  showHint.value = false
  try {
    localStorage.setItem(HINT_KEY, '0')
  } catch {
    // 忽略存储失败
  }
}

async function hideWindow() {
  saveDraft()
  try {
    await getCurrentWindow().hide()
  } catch {
    // 浏览器预览无法隐藏窗口
  }
}

async function save() {
  if (saving.value) return
  if (!input.value.trim()) return
  if (!parsed.value.title.trim()) {
    errorText.value = '再写点内容'
    return
  }
  saving.value = true
  errorText.value = ''
  try {
    await notesCreate({
      title: parsed.value.title,
      dueAt: parsed.value.dueAt,
      repeat: parsed.value.repeat,
      tags: parsed.value.tags,
      priority: parsed.value.priority,
    })
    input.value = ''
    clearDraft()
    showHint.value = false
    await hideWindow()
  } catch (error) {
    errorText.value = String(error)
  } finally {
    saving.value = false
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    void hideWindow()
    return
  }
  if (event.key === 'Enter' && !event.shiftKey) {
    if (composing || event.isComposing) return
    event.preventDefault()
    void save()
  }
}

function onCompositionStart() {
  composing = true
}

function onCompositionEnd() {
  composing = false
}

async function focusInput() {
  await nextTick()
  inputRef.value?.focus()
}

onMounted(async () => {
  loadDraft()
  await focusInput()
  try {
    await getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) {
        loadDraft()
        void focusInput()
      }
    })
  } catch {
    // 浏览器预览不支持窗口焦点事件
  }
})

onBeforeUnmount(() => {
  saveDraft()
})
</script>

<template>
  <div class="quick">
    <div class="quick-bar" data-tauri-drag-region>
      <span class="quick-label" data-tauri-drag-region>快速记录</span>
      <span class="quick-shortcut">Ctrl+Alt+N</span>
      <button class="quick-close" type="button" title="关闭" @click="hideWindow">×</button>
    </div>

    <div class="quick-body">
      <textarea
        ref="inputRef"
        v-model="input"
        class="quick-input"
        rows="2"
        placeholder="试试：明天10点 交周报 #工作 !"
        spellcheck="false"
        @input="saveDraft"
        @keydown="onKeydown"
        @compositionstart="onCompositionStart"
        @compositionend="onCompositionEnd"
      />
      <div class="quick-preview">
        <span v-for="label in parsed.labels" :key="label" class="preview-tag">{{ label }}</span>
        <span v-if="!parsed.labels.length" class="preview-placeholder">
          可带日期、重复、标签与优先级，不带时间也能记
        </span>
        <a-spin v-if="saving" size="small" />
      </div>
      <div v-if="errorText" class="quick-error">{{ errorText }}</div>
      <div v-else-if="showHint" class="quick-hint">
        全局快捷键 Ctrl+Alt+N 可随时唤起，Esc 关闭不保存。
        <button type="button" @click="dismissHint">知道了</button>
      </div>
      <div v-else class="quick-foot">Enter 保存 · Shift+Enter 换行 · Esc 关闭</div>
    </div>
  </div>
</template>

<style scoped>
.quick {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
  color: var(--fm-text);
  overflow: hidden;
}

.quick-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 8px 0 12px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  user-select: none;
}

.quick-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
  flex: 1;
}

.quick-shortcut {
  font-size: 10px;
  color: var(--fm-muted);
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  padding: 0 4px;
}

.quick-close {
  border: none;
  background: transparent;
  color: var(--fm-muted);
  font-size: 16px;
  line-height: 1;
  width: 22px;
  height: 22px;
  border-radius: 4px;
  cursor: pointer;
}

.quick-close:hover {
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
}

.quick-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 12px 10px;
}

.quick-input {
  flex: 1;
  min-height: 0;
  resize: none;
  border: none;
  outline: none;
  background: transparent;
  color: var(--fm-text);
  font-size: 15px;
  line-height: 1.5;
  font-family: inherit;
}

.quick-input::placeholder {
  color: var(--fm-muted);
}

.quick-preview {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  min-height: 20px;
}

.preview-tag {
  font-size: 11px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 4px;
  padding: 1px 6px;
}

.preview-placeholder {
  font-size: 11px;
  color: var(--fm-muted);
}

.quick-foot,
.quick-hint {
  font-size: 11px;
  color: var(--fm-muted);
}

.quick-hint {
  display: flex;
  align-items: center;
  gap: 8px;
}

.quick-hint button {
  border: none;
  background: transparent;
  color: var(--fm-primary);
  font-size: 11px;
  cursor: pointer;
  padding: 0;
}

.quick-error {
  font-size: 11px;
  color: var(--fm-danger);
}
</style>
