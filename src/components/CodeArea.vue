<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

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
/** 小于该字符数：逐键同步着色（成本低，体验最好） */
const LIVE_MAX = 64 * 1024
/** 大于该字符数：默认关闭着色，需用户手动开启 */
const MANUAL_MAX = 256 * 1024
/** 超过该字符数：不再着色，只保留纯文本编辑 */
const HIGHLIGHT_MAX = 2 * 1024 * 1024
/** 大文本停止输入后延迟着色，避免每次按键都重算整段 HTML */
const DEFER_DELAY = 200
/** 行数超过该值时行号改为虚拟滚动，避免几万个 DOM 节点 */
const VIRTUAL_LINES = 300

const textLength = computed(() => props.modelValue.length)

/* ---------- 着色模式：按体量分层，内容再大也不阻塞输入 ---------- */
const manualHighlight = ref(false)

type HighlightMode = 'live' | 'defer' | 'manual' | 'off'

const highlightMode = computed<HighlightMode>(() => {
  if (props.language !== 'json' || !props.modelValue) return 'off'
  const length = textLength.value
  if (length > HIGHLIGHT_MAX) return 'off'
  if (length > MANUAL_MAX) return manualHighlight.value ? 'defer' : 'manual'
  if (length > LIVE_MAX) return 'defer'
  return 'live'
})

const showHighlightToggle = computed(
  () =>
    props.language === 'json' &&
    textLength.value > MANUAL_MAX &&
    textLength.value <= HIGHLIGHT_MAX,
)

const showTooLargeNote = computed(
  () => props.language === 'json' && textLength.value > HIGHLIGHT_MAX,
)

function toggleHighlight() {
  manualHighlight.value = !manualHighlight.value
  refreshHighlight(true)
}

/* ---------- 着色源与原文解耦：输入期间可以先显示纯文本 ---------- */
const highlightSource = ref('')
let highlightTimer: number | undefined

function cancelHighlightTimer() {
  if (highlightTimer) {
    window.clearTimeout(highlightTimer)
    highlightTimer = undefined
  }
}

function refreshHighlight(immediate = false) {
  cancelHighlightTimer()
  const mode = highlightMode.value
  if (mode === 'live') {
    highlightSource.value = props.modelValue
    return
  }
  if (mode === 'defer') {
    if (immediate) {
      highlightSource.value = props.modelValue
      return
    }
    highlightTimer = window.setTimeout(() => {
      highlightTimer = undefined
      highlightSource.value = props.modelValue
    }, DEFER_DELAY)
    return
  }
  // manual / off：清空着色层，输入框直接显示原文
  highlightSource.value = ''
}

// 内容或模式变化时刷新着色源；live 模式同步完成，不会出现闪烁
watch(
  () => [props.modelValue, highlightMode.value],
  () => refreshHighlight(),
)

/* ---------- 行号：大文本延迟统计 + 虚拟滚动 ---------- */
const lineCount = ref(1)
let lineTimer: number | undefined

function countLines(text: string) {
  if (!text) return 1
  let count = 1
  let index = text.indexOf('\n')
  while (index !== -1) {
    count += 1
    index = text.indexOf('\n', index + 1)
  }
  return count
}

function refreshLineCount(immediate: boolean) {
  if (lineTimer) {
    window.clearTimeout(lineTimer)
    lineTimer = undefined
  }
  const apply = () => {
    lineCount.value = countLines(props.modelValue)
  }
  if (immediate || textLength.value <= LIVE_MAX) {
    apply()
    return
  }
  lineTimer = window.setTimeout(() => {
    lineTimer = undefined
    apply()
  }, DEFER_DELAY)
}

const lineNumbers = computed(() => Array.from({ length: lineCount.value }, (_, i) => i + 1))

const virtualGutter = computed(() => lineCount.value > VIRTUAL_LINES)

const gutterWidth = computed(() => Math.max(2, String(lineCount.value).length) * 8 + 18)

const gutterScrollTop = ref(0)
const gutterViewHeight = ref(0)
/** textarea 横向滚动条占用的高度；着色层没有滚动条，需要补同样的底部内边距 */
const scrollbarHeight = ref(0)

const gutterInnerStyle = computed(() =>
  virtualGutter.value
    ? { height: `${lineCount.value * LINE_HEIGHT}px` }
    : { transform: `translateY(${-gutterScrollTop.value}px)` },
)

const highlightStyle = computed(() => ({
  paddingBottom: `${10 + scrollbarHeight.value}px`,
}))

const visibleLineNumbers = computed(() => {
  if (!virtualGutter.value) return lineNumbers.value
  const height = gutterViewHeight.value || 400
  const start = Math.max(0, Math.floor(gutterScrollTop.value / LINE_HEIGHT) - 2)
  const end = Math.min(lineCount.value, start + Math.ceil(height / LINE_HEIGHT) + 6)
  const out: number[] = []
  for (let n = start + 1; n <= end; n += 1) out.push(n)
  return out
})

function lineTop(n: number) {
  return (n - 1) * LINE_HEIGHT - gutterScrollTop.value
}

const boxHeight = computed(() => props.rows * LINE_HEIGHT + 20)

/** 只有着色层与当前内容一致时才叠加显示，避免滞后内容重影 */
const freshHighlight = computed(
  () => highlightSource.value !== '' && highlightSource.value === props.modelValue,
)

const highlighted = computed(() =>
  highlightSource.value ? highlightJson(highlightSource.value) : '',
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

function isDigit(text: string, index: number) {
  const code = text.charCodeAt(index)
  return index < text.length && code >= 48 && code <= 57
}

/**
 * JSON 着色：注释 / 字段名 / 字符串 / 数字 / {{变量}} 各一种颜色。
 *
 * 按「普通片段 + 命中片段」收集后一次性 join，避免逐字符 `out +=`；
 * 字段名判断不再对剩余全文 slice，数字扫描也不再对剩余全文跑正则。
 */
function highlightJson(text: string) {
  const parts: string[] = []
  const length = text.length
  let index = 0
  let plainStart = 0

  const flushPlain = (end: number) => {
    if (end > plainStart) parts.push(escapeHtml(text.slice(plainStart, end)))
  }

  const pushToken = (className: string, start: number, end: number, vars = false) => {
    flushPlain(start)
    const raw = text.slice(start, end)
    parts.push(`<span class="${className}">${vars ? wrapVars(raw) : escapeHtml(raw)}</span>`)
    plainStart = end
    index = end
  }

  while (index < length) {
    const code = text.charCodeAt(index)

    // 双引号字符串 / 字段名
    if (code === 34) {
      let end = index + 1
      while (end < length) {
        const next = text.charCodeAt(end)
        if (next === 92) {
          end += 2
          continue
        }
        if (next === 34) {
          end += 1
          break
        }
        end += 1
      }
      // 跳过空白后看是否紧跟冒号，判断是不是字段名
      let probe = end
      while (probe < length) {
        const next = text.charCodeAt(probe)
        if (next === 32 || next === 9 || next === 10 || next === 13) {
          probe += 1
          continue
        }
        break
      }
      const isKey = probe < length && text.charCodeAt(probe) === 58
      pushToken(isKey ? 'tok-key' : 'tok-string', index, end, true)
      continue
    }

    // 行注释
    if (code === 47 && text.charCodeAt(index + 1) === 47) {
      let end = text.indexOf('\n', index)
      if (end === -1) end = length
      pushToken('tok-comment', index, end)
      continue
    }

    // 块注释
    if (code === 47 && text.charCodeAt(index + 1) === 42) {
      const close = text.indexOf('*/', index + 2)
      const end = close === -1 ? length : close + 2
      pushToken('tok-comment', index, end)
      continue
    }

    // {{变量}}
    if (code === 123 && text.charCodeAt(index + 1) === 123) {
      const close = text.indexOf('}}', index + 2)
      if (close !== -1) {
        pushToken('tok-var', index, close + 2)
        continue
      }
    }

    // 数字（含小数与科学计数法）
    if (code === 45 || (code >= 48 && code <= 57)) {
      let end = index
      if (text.charCodeAt(end) === 45) end += 1
      const digitsStart = end
      while (isDigit(text, end)) end += 1
      if (end > digitsStart) {
        if (text.charCodeAt(end) === 46 && isDigit(text, end + 1)) {
          end += 1
          while (isDigit(text, end)) end += 1
        }
        const exponent = text.charCodeAt(end)
        if (exponent === 101 || exponent === 69) {
          let expEnd = end + 1
          const sign = text.charCodeAt(expEnd)
          if (sign === 43 || sign === 45) expEnd += 1
          const expDigits = expEnd
          while (isDigit(text, expEnd)) expEnd += 1
          if (expEnd > expDigits) end = expEnd
        }
        pushToken('tok-number', index, end)
        continue
      }
    }

    // 标点
    if (
      code === 123 ||
      code === 125 ||
      code === 91 ||
      code === 93 ||
      code === 44 ||
      code === 58
    ) {
      pushToken('tok-punct', index, index + 1)
      continue
    }

    index += 1
  }

  flushPlain(length)
  return parts.join('')
}

/* ---------- 滚动同步：滚动事件合并到一帧，避免高频写 DOM ---------- */
let scrollRaf = 0

function applyScroll() {
  const el = textareaRef.value
  if (!el) return
  // 横向滚动条会压缩 textarea 的可视高度；着色层没有滚动条，必须补相同的底部空间，
  // 否则滚到底部时着色层会被提前限制 scrollTop，出现“文字比光标高一行”的错位。
  const hbar = Math.max(0, el.offsetHeight - el.clientHeight)
  if (hbar !== scrollbarHeight.value) {
    scrollbarHeight.value = hbar
    // 补内边距要等下一次 DOM 更新生效，届时再对齐一次滚动位置
    nextTick(applyScroll)
  }
  if (highlightRef.value) {
    highlightRef.value.scrollTop = el.scrollTop
    highlightRef.value.scrollLeft = el.scrollLeft
  }
  gutterScrollTop.value = el.scrollTop
  if (!gutterViewHeight.value && gutterRef.value) {
    gutterViewHeight.value = Math.max(0, gutterRef.value.clientHeight - 20)
  }
}

function onScroll() {
  if (scrollRaf) return
  scrollRaf = requestAnimationFrame(() => {
    scrollRaf = 0
    applyScroll()
  })
}

// 延迟着色完成、着色层重新挂载时，先把滚动位置对齐，避免大文件停在顶部错位
watch(freshHighlight, (fresh) => {
  if (fresh) nextTick(applyScroll)
})

// 切换接口 / 格式化后内容整体变化时，重新对齐着色层与行号
watch(
  () => props.modelValue,
  () => {
    refreshLineCount(false)
    nextTick(applyScroll)
  },
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

let resizeObserver: ResizeObserver | undefined

onMounted(() => {
  refreshHighlight(true)
  refreshLineCount(true)
  applyScroll()
  if (gutterRef.value) {
    gutterViewHeight.value = Math.max(0, gutterRef.value.clientHeight - 20)
    if (typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => {
        if (gutterRef.value) {
          gutterViewHeight.value = Math.max(0, gutterRef.value.clientHeight - 20)
        }
        // 尺寸变化可能让横向滚动条出现 / 消失，需要重新测量着色层补偿高度
        applyScroll()
      })
      resizeObserver.observe(gutterRef.value)
      if (textareaRef.value) resizeObserver.observe(textareaRef.value)
    }
  }
})

onBeforeUnmount(() => {
  cancelHighlightTimer()
  if (lineTimer) window.clearTimeout(lineTimer)
  if (scrollRaf) cancelAnimationFrame(scrollRaf)
  resizeObserver?.disconnect()
})

/** 在光标处插入文本（Schema 浏览器点击字段时用），保留光标位置 */
function insertText(text: string) {
  const el = textareaRef.value
  if (!el) {
    emit('update:modelValue', props.modelValue + text)
    return
  }
  const start = el.selectionStart ?? props.modelValue.length
  const end = el.selectionEnd ?? start
  emit('update:modelValue', props.modelValue.slice(0, start) + text + props.modelValue.slice(end))
  nextTick(() => {
    el.focus()
    const pos = start + text.length
    el.setSelectionRange(pos, pos)
  })
}

defineExpose({
  focus: () => textareaRef.value?.focus(),
  insertText,
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
      <div class="gutter-inner" :style="gutterInnerStyle">
        <template v-if="virtualGutter">
          <div
            v-for="n in visibleLineNumbers"
            :key="n"
            class="gutter-line gutter-line-abs"
            :style="{ top: `${lineTop(n)}px` }"
          >
            {{ n }}
          </div>
        </template>
        <template v-else>
          <div v-for="n in lineNumbers" :key="n" class="gutter-line">{{ n }}</div>
        </template>
      </div>
    </div>
    <div class="editor-wrap">
      <pre
        v-if="freshHighlight"
        ref="highlightRef"
        class="highlight"
        :style="highlightStyle"
        aria-hidden="true"
      ><code v-html="highlighted" /></pre>
      <textarea
        ref="textareaRef"
        class="editor"
        :class="{ overlay: freshHighlight && !composing }"
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
      <div v-if="showHighlightToggle" class="hl-hint">
        <button v-if="highlightMode === 'defer'" class="hl-btn" @click="toggleHighlight">
          关闭着色
        </button>
        <button v-else class="hl-btn" @click="toggleHighlight">开启着色</button>
      </div>
      <div v-else-if="showTooLargeNote" class="hl-hint hl-note">内容过大，已关闭着色</div>
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
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fm-primary) 13%, transparent);
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

.gutter-inner {
  position: relative;
}

.gutter-line {
  padding: 0 8px 0 10px;
}

.gutter-line-abs {
  position: absolute;
  left: 0;
  right: 0;
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

.hl-hint {
  position: absolute;
  right: 10px;
  bottom: 6px;
  z-index: 2;
  font-size: 11px;
  color: var(--fm-muted);
  background: color-mix(in srgb, var(--fm-code-bg) 88%, transparent);
  border: 1px solid var(--fm-border);
  border-radius: 999px;
  padding: 1px 8px;
  pointer-events: auto;
}

.hl-btn {
  border: none;
  background: transparent;
  color: var(--fm-primary);
  font-size: 11px;
  cursor: pointer;
  padding: 0;
}

.hl-note {
  padding: 1px 8px;
}

@media (max-width: 640px) {
  .code-area {
    max-height: 60vh;
  }
}
</style>
