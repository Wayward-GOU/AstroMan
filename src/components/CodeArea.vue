<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    rows?: number
    readonly?: boolean
    placeholder?: string
    fill?: boolean
    /** 支持 Ctrl/Cmd + / 切换行注释（// 注释） */
    commentable?: boolean
    /** 语法着色：json 会区分注释、字段名、字符串、数字与 {{变量}} */
    language?: 'plain' | 'json'
  }>(),
  {
    rows: 15,
    readonly: false,
    placeholder: '',
    fill: false,
    commentable: false,
    language: 'plain',
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const textareaRef = ref<HTMLTextAreaElement | null>(null)
const gutterRef = ref<HTMLElement | null>(null)
const highlightRef = ref<HTMLElement | null>(null)
/** 输入法组合期间临时显示原文，避免拼音/候选内容不可见 */
const composing = ref(false)

const LINE_HEIGHT = 20

const lineCount = computed(() => {
  const text = props.modelValue
  if (!text) return 1
  return text.split('\n').length
})

const lineNumbers = computed(() =>
  Array.from({ length: lineCount.value }, (_, i) => i + 1),
)

const gutterWidth = computed(() => Math.max(2, String(lineCount.value).length) * 8 + 18)

const boxHeight = computed(() => props.rows * LINE_HEIGHT + 20)

const highlighted = computed(() =>
  props.language === 'json' ? highlightJson(props.modelValue) : null,
)

function escapeHtml(text: string) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/** 把字符串里的 {{变量}} 单独着色，其余内容原样转义 */
function wrapVars(text: string) {
  const pattern = /\{\{[^{}\n]*\}\}/g
  let out = ''
  let last = 0
  let match: RegExpExecArray | null
  while ((match = pattern.exec(text)) !== null) {
    out += escapeHtml(text.slice(last, match.index))
    out += `<span class="tok-var">${escapeHtml(match[0])}</span>`
    last = match.index + match[0].length
  }
  return out + escapeHtml(text.slice(last))
}

/** JSON 着色：注释 / 字段名 / 字符串 / 数字 / {{变量}} 各一种颜色 */
function highlightJson(text: string) {
  let out = ''
  let index = 0
  while (index < text.length) {
    const current = text[index]

    if (current === '"') {
      let end = index + 1
      while (end < text.length) {
        if (text[end] === '\\') {
          end += 2
          continue
        }
        if (text[end] === '"') {
          end += 1
          break
        }
        end += 1
      }
      const raw = text.slice(index, end)
      const isKey = /^\s*:/.test(text.slice(end))
      out += `<span class="${isKey ? 'tok-key' : 'tok-string'}">${wrapVars(raw)}</span>`
      index = end
      continue
    }

    if (current === '/' && text[index + 1] === '/') {
      let end = text.indexOf('\n', index)
      if (end === -1) end = text.length
      out += `<span class="tok-comment">${escapeHtml(text.slice(index, end))}</span>`
      index = end
      continue
    }

    if (current === '/' && text[index + 1] === '*') {
      const close = text.indexOf('*/', index + 2)
      const end = close === -1 ? text.length : close + 2
      out += `<span class="tok-comment">${escapeHtml(text.slice(index, end))}</span>`
      index = end
      continue
    }

    if (current === '{' && text[index + 1] === '{') {
      const close = text.indexOf('}}', index + 2)
      if (close !== -1) {
        out += `<span class="tok-var">${escapeHtml(text.slice(index, close + 2))}</span>`
        index = close + 2
        continue
      }
    }

    if (current === '-' || (current >= '0' && current <= '9')) {
      const number = /^-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(text.slice(index))
      if (number) {
        out += `<span class="tok-number">${escapeHtml(number[0])}</span>`
        index += number[0].length
        continue
      }
    }

    if ('{}[],:'.includes(current)) {
      out += `<span class="tok-punct">${escapeHtml(current)}</span>`
      index += 1
      continue
    }

    out += escapeHtml(current)
    index += 1
  }
  return out
}

function onScroll() {
  const el = textareaRef.value
  if (!el) return
  if (gutterRef.value) {
    gutterRef.value.scrollTop = el.scrollTop
  }
  if (highlightRef.value) {
    highlightRef.value.scrollTop = el.scrollTop
    highlightRef.value.scrollLeft = el.scrollLeft
  }
}

// 切换接口 / 格式化后内容整体变化时，重新对齐着色层
watch(
  () => props.modelValue,
  () => nextTick(onScroll),
)

function onInput(e: Event) {
  const target = e.target as HTMLTextAreaElement
  emit('update:modelValue', target.value)
}

function onKeydown(e: KeyboardEvent) {
  if (props.readonly) return
  const el = e.target as HTMLTextAreaElement
  if (
    props.commentable &&
    (e.ctrlKey || e.metaKey) &&
    !e.altKey &&
    !e.shiftKey &&
    (e.key === '/' || e.code === 'Slash')
  ) {
    e.preventDefault()
    toggleComment(el)
    return
  }
  if (e.key !== 'Tab') return
  e.preventDefault()
  const start = el.selectionStart
  const end = el.selectionEnd
  const next = `${props.modelValue.slice(0, start)}  ${props.modelValue.slice(end)}`
  emit('update:modelValue', next)
  requestAnimationFrame(() => {
    el.selectionStart = start + 2
    el.selectionEnd = start + 2
  })
}

/** 按行切换 // 注释：全部已注释则取消，否则统一加上 */
function toggleComment(el: HTMLTextAreaElement) {
  const text = props.modelValue
  const start = el.selectionStart
  const end = el.selectionEnd
  const lines = text.split('\n')
  const firstLine = text.slice(0, start).split('\n').length - 1
  let lastLine = text.slice(0, end).split('\n').length - 1
  // 选区正好停在下一行行首时，不把该行算进去
  if (end > start && text[end - 1] === '\n') lastLine -= 1

  const selected = lines.slice(firstLine, lastLine + 1)
  if (!selected.some((line) => line.trim() !== '')) return

  const commented = selected.every((line) => line.trim() === '' || /^\s*\/\//.test(line))
  const nextSelected = selected.map((line) => {
    if (line.trim() === '') return line
    return commented ? line.replace(/^(\s*)\/\/ ?/, '$1') : line.replace(/^(\s*)/, '$1// ')
  })

  const nextLines = [...lines.slice(0, firstLine), ...nextSelected, ...lines.slice(lastLine + 1)]
  emit('update:modelValue', nextLines.join('\n'))

  const lineStart = (index: number) =>
    nextLines.slice(0, index).reduce((total, line) => total + line.length + 1, 0)

  requestAnimationFrame(() => {
    if (start === end) {
      const column = start - (text.lastIndexOf('\n', start - 1) + 1)
      const delta = nextSelected[0].length - selected[0].length
      el.selectionStart = el.selectionEnd = lineStart(firstLine) + Math.max(0, column + delta)
    } else {
      el.selectionStart = lineStart(firstLine)
      el.selectionEnd = lineStart(lastLine + 1) - 1
    }
  })
}

defineExpose({
  focus: () => textareaRef.value?.focus(),
})
</script>

<template>
  <div class="code-area" :class="{ fill }" :style="fill ? undefined : { height: `${boxHeight}px` }">
    <div
      ref="gutterRef"
      class="gutter"
      :style="{ width: `${gutterWidth}px` }"
      aria-hidden="true"
    >
      <div v-for="n in lineNumbers" :key="n" class="gutter-line">{{ n }}</div>
    </div>
    <div class="editor-wrap">
      <pre v-if="highlighted !== null" ref="highlightRef" class="highlight" aria-hidden="true"><code v-html="highlighted" /></pre>
      <textarea
        ref="textareaRef"
        class="editor"
        :class="{ overlay: highlighted !== null && !composing }"
        :value="modelValue"
        :readonly="readonly"
        :placeholder="placeholder"
        spellcheck="false"
        wrap="off"
        @input="onInput"
        @scroll="onScroll"
        @keydown="onKeydown"
        @compositionstart="composing = true"
        @compositionend="composing = false"
      />
    </div>
  </div>
</template>

<style scoped>
.code-area {
  display: flex;
  overflow: hidden;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.code-area:focus-within {
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 3px rgba(61, 107, 143, 0.1);
}

.code-area.fill {
  flex: 1;
  min-height: 0;
  height: 100%;
}

.gutter {
  flex: 0 0 auto;
  padding: 10px 0;
  overflow: hidden;
  text-align: right;
  user-select: none;
  background: var(--fm-gutter-bg);
  border-right: 1px solid var(--fm-border);
  color: var(--fm-muted);
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  font-size: 12px;
  line-height: 20px;
}

.gutter-line {
  padding: 0 8px 0 10px;
}

.editor-wrap {
  position: relative;
  flex: 1;
  min-width: 0;
  height: 100%;
}

.highlight {
  position: absolute;
  inset: 0;
  margin: 0;
  padding: 10px 12px;
  overflow: hidden;
  pointer-events: none;
  color: var(--fm-text);
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  font-size: 12px;
  line-height: 20px;
  white-space: pre;
  tab-size: 2;
}

.highlight code {
  font-family: inherit;
  font-size: inherit;
  line-height: inherit;
}

/* v-html 生成的节点没有 scoped 标记，需要 :deep 才能命中 */
.highlight :deep(.tok-comment) {
  color: var(--fm-muted);
  font-style: italic;
}

.highlight :deep(.tok-key) {
  color: var(--fm-json-key);
  font-weight: 600;
}

.highlight :deep(.tok-string) {
  color: var(--fm-json-string);
}

.highlight :deep(.tok-number) {
  color: var(--fm-json-number);
}

.highlight :deep(.tok-punct) {
  color: var(--fm-json-punct);
}

.highlight :deep(.tok-var) {
  color: var(--fm-json-bool);
  font-weight: 600;
}

.editor {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  padding: 10px 12px;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--fm-text);
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  font-size: 12px;
  line-height: 20px;
  white-space: pre;
  overflow: auto;
  tab-size: 2;
}

/* 着色模式下文字透明，只保留光标，颜色由底层 .highlight 提供 */
.editor.overlay {
  color: transparent;
  caret-color: var(--fm-text);
}

.editor::placeholder {
  color: var(--fm-muted);
}

.editor[readonly] {
  color: var(--fm-text-2);
  cursor: default;
}

@media (max-width: 640px) {
  .code-area {
    max-height: 60vh;
  }
}
</style>
