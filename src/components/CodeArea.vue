<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { Compartment, EditorState, RangeSetBuilder, type Extension } from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  placeholder as placeholderExtension,
  type Command,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view'
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentLess,
  indentMore,
} from '@codemirror/commands'
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from '@codemirror/search'
import {
  HighlightStyle,
  StreamLanguage,
  bracketMatching,
  indentUnit,
  syntaxHighlighting,
} from '@codemirror/language'
import { tags } from '@lezer/highlight'

const props = withDefaults(
  defineProps<{
    modelValue: string
    rows?: number
    readonly?: boolean
    placeholder?: string
    fill?: boolean
    /** 语法着色：json 会区分注释、字段名、字符串、数字与 {{变量}} */
    language?: 'plain' | 'json'
    /** 显示查找入口（Ctrl/Cmd + F 始终可用） */
    searchable?: boolean
  }>(),
  {
    rows: 15,
    readonly: false,
    placeholder: '',
    fill: false,
    language: 'plain',
    searchable: false,
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const hostRef = ref<HTMLElement | null>(null)
const viewRef = shallowRef<EditorView | null>(null)

const LINE_HEIGHT = 20
const boxHeight = computed(() => props.rows * LINE_HEIGHT + 20)

/** 最近一次由编辑器主动抛出的值，用来区分外部改值与 v-model 回写 */
let lastEmitted = props.modelValue
let applyingExternal = false

/* ---------- JSON 着色（兼容注释与 {{变量}}，按可视区域增量解析） ---------- */
type JsonState = { blockComment: boolean }

const jsonLike = StreamLanguage.define<JsonState>({
  name: 'json-like',
  startState: () => ({ blockComment: false }),
  token(stream, state) {
    if (state.blockComment) {
      if (stream.match(/[\s\S]*?\*\//)) state.blockComment = false
      else stream.skipToEnd()
      return 'comment'
    }
    if (stream.eatSpace()) return null

    if (stream.match('//')) {
      stream.skipToEnd()
      return 'comment'
    }
    if (stream.match('/*')) {
      if (!stream.match(/[\s\S]*?\*\//)) {
        state.blockComment = true
        stream.skipToEnd()
      }
      return 'comment'
    }
    if (stream.match(/\{\{[^{}\n]*\}\}/)) return 'variable'

    if (stream.match(/"(?:[^"\\]|\\.)*"/)) {
      return /^\s*:/.test(stream.string.slice(stream.pos)) ? 'propertyName' : 'string'
    }

    if (stream.match(/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/)) return 'number'
    if (stream.match('true') || stream.match('false')) return 'bool'
    if (stream.match('null')) return 'null'
    if (stream.match(/[{}[\],:]/)) return 'punct'

    stream.next()
    return null
  },
  languageData: {
    commentTokens: { line: '//', block: { open: '/*', close: '*/' } },
  },
  tokenTable: {
    propertyName: tags.propertyName,
    string: tags.string,
    number: tags.number,
    bool: tags.bool,
    null: tags.null,
    comment: tags.comment,
    punct: tags.punctuation,
    variable: tags.special(tags.string),
  },
})

const highlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: 'var(--fm-muted)', fontStyle: 'italic' },
  { tag: tags.propertyName, color: 'var(--fm-json-key)', fontWeight: '500' },
  { tag: tags.string, color: 'var(--fm-json-string)' },
  { tag: tags.number, color: 'var(--fm-json-number)' },
  { tag: tags.bool, color: 'var(--fm-json-bool)', fontWeight: '500' },
  { tag: tags.null, color: 'var(--fm-json-bool)', fontStyle: 'italic' },
  { tag: tags.punctuation, color: 'var(--fm-muted)' },
  { tag: tags.special(tags.string), color: 'var(--fm-brass-text)', fontWeight: '500' },
])

/** 字符串内部的 {{变量}} 单独加底色，字段名/普通字符串里都能看到 */
const varMark = Decoration.mark({ class: 'cm-template-var' })

function buildVarDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>()
  const from = view.state.doc.lineAt(view.viewport.from).from
  const to = view.state.doc.lineAt(view.viewport.to).to
  const text = view.state.sliceDoc(from, to)
  const pattern = /\{\{[^{}\n]*\}\}/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(text)) !== null) {
    builder.add(from + match.index, from + match.index + match[0].length, varMark)
  }
  return builder.finish()
}

const templateVars = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet

    constructor(view: EditorView) {
      this.decorations = buildVarDecorations(view)
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildVarDecorations(update.view)
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
)

/* ---------- 主题：沿用应用的 --fm-* 令牌，亮暗主题自动跟随 ---------- */
const editorTheme = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--fm-code-bg)',
    color: 'var(--fm-text)',
    fontSize: '12px',
  },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': {
    fontFamily:
      "'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
    fontSize: '12px',
    lineHeight: `${LINE_HEIGHT}px`,
    overflow: 'auto',
  },
  '.cm-content': {
    padding: '10px 0',
    caretColor: 'var(--fm-text)',
  },
  '.cm-line': { padding: '0 12px' },
  '.cm-gutters': {
    backgroundColor: 'var(--fm-gutter-bg)',
    color: 'var(--fm-muted)',
    borderRight: '1px solid var(--fm-border)',
    lineHeight: `${LINE_HEIGHT}px`,
  },
  '.cm-lineNumbers .cm-gutterElement': {
    padding: '0 8px 0 10px',
    minWidth: '32px',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent',
    color: 'var(--fm-text-2)',
  },
  '.cm-activeLine': {
    backgroundColor: 'color-mix(in srgb, var(--fm-primary) 4%, transparent)',
  },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--fm-text)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, .cm-content ::selection': {
    backgroundColor: 'color-mix(in srgb, var(--fm-primary) 24%, transparent)',
  },
  '.cm-selectionMatch': {
    backgroundColor: 'color-mix(in srgb, var(--fm-primary) 12%, transparent)',
  },
  '.cm-searchMatch': {
    backgroundColor: 'color-mix(in srgb, var(--fm-warning) 28%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--fm-warning) 55%, transparent)',
  },
  '.cm-searchMatch.cm-searchMatch-selected': {
    backgroundColor: 'color-mix(in srgb, var(--fm-primary) 30%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--fm-primary) 60%, transparent)',
  },
  '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': {
    backgroundColor: 'color-mix(in srgb, var(--fm-primary) 16%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--fm-primary) 42%, transparent)',
  },
  '.cm-nonmatchingBracket': { color: 'var(--fm-danger)' },
  '.cm-template-var': {
    backgroundColor: 'color-mix(in srgb, var(--fm-brass) 15%, transparent)',
    borderRadius: '2px',
  },
  '.cm-panels': {
    backgroundColor: 'var(--fm-surface-alt)',
    color: 'var(--fm-text)',
  },
  '.cm-panels.cm-panels-top': { borderBottom: '1px solid var(--fm-border)' },
  '.cm-panel.cm-search': { padding: '6px 8px', fontSize: '12px' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button': { fontSize: '12px' },
  '.cm-textfield': {
    backgroundColor: 'var(--fm-surface)',
    border: '1px solid var(--fm-border)',
    color: 'var(--fm-text)',
    borderRadius: '3px',
  },
  '.cm-button': {
    backgroundImage: 'none',
    backgroundColor: 'var(--fm-surface)',
    border: '1px solid var(--fm-border)',
    color: 'var(--fm-text)',
    borderRadius: '3px',
  },
  '.cm-panel.cm-search label': { color: 'var(--fm-muted)' },
  '.cm-placeholder': { color: 'var(--fm-muted)' },
})

const phrases = EditorState.phrases.of({
  Find: '查找',
  Replace: '替换',
  replace: '替换',
  'replace all': '全部替换',
  next: '下一个',
  previous: '上一个',
  all: '全部',
  'match case': '区分大小写',
  regexp: '正则',
  'by word': '全词匹配',
  close: '关闭',
  'Go to line': '跳转到行',
  go: '跳转',
  'current match': '当前匹配',
  'on line': '位于行',
  'replaced $ matches': '已替换 $ 处',
  'replaced match on line $': '已替换第 $ 行的匹配',
})

const languageCompartment = new Compartment()
const readonlyCompartment = new Compartment()
const placeholderCompartment = new Compartment()

function languageExtension(language: 'plain' | 'json'): Extension {
  if (language !== 'json') return []
  return [jsonLike, syntaxHighlighting(highlightStyle), bracketMatching()]
}

function placeholderExtensionFor(value: string): Extension {
  return value ? placeholderExtension(value) : []
}

/** 单光标 Tab 与旧版一致插入两个空格；有选区时仍按行缩进 */
const insertTwoSpaces: Command = (view) => {
  if (view.state.selection.ranges.some((range) => !range.empty)) return indentMore(view)
  view.dispatch({
    ...view.state.replaceSelection('  '),
    scrollIntoView: true,
    userEvent: 'input',
  })
  return true
}

function buildExtensions(): Extension[] {
  return [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    history(),
    drawSelection(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    indentUnit.of('  '),
    search({ top: true }),
    highlightSelectionMatches(),
    templateVars,
    keymap.of([
      ...defaultKeymap,
      ...searchKeymap,
      ...historyKeymap,
      { key: 'Tab', run: insertTwoSpaces, shift: indentLess },
    ]),
    EditorView.contentAttributes.of({
      spellcheck: 'false',
      autocapitalize: 'off',
      autocomplete: 'off',
    }),
    languageCompartment.of(languageExtension(props.language)),
    readonlyCompartment.of(EditorState.readOnly.of(props.readonly)),
    placeholderCompartment.of(placeholderExtensionFor(props.placeholder)),
    phrases,
    editorTheme,
    EditorView.updateListener.of((update) => {
      if (!update.docChanged || applyingExternal) return
      const value = update.state.doc.toString()
      lastEmitted = value
      emit('update:modelValue', value)
    }),
  ]
}

onMounted(() => {
  const host = hostRef.value
  if (!host) return
  lastEmitted = props.modelValue
  viewRef.value = new EditorView({
    state: EditorState.create({ doc: props.modelValue, extensions: buildExtensions() }),
    parent: host,
  })
})

onBeforeUnmount(() => {
  viewRef.value?.destroy()
  viewRef.value = null
})

/** 外部（格式化、切换接口、导入）改值：整段替换，不回抛一次 update */
watch(
  () => props.modelValue,
  (value) => {
    const view = viewRef.value
    if (!view || value === lastEmitted) return
    const current = view.state.doc.toString()
    if (value === current) return
    lastEmitted = value
    applyingExternal = true
    view.dispatch({ changes: { from: 0, to: current.length, insert: value } })
    applyingExternal = false
  },
)

watch(
  () => props.language,
  (language) => {
    viewRef.value?.dispatch({
      effects: languageCompartment.reconfigure(languageExtension(language)),
    })
  },
)

watch(
  () => props.readonly,
  (readonly) => {
    viewRef.value?.dispatch({
      effects: readonlyCompartment.reconfigure(EditorState.readOnly.of(readonly)),
    })
  },
)

watch(
  () => props.placeholder,
  (placeholder) => {
    viewRef.value?.dispatch({
      effects: placeholderCompartment.reconfigure(placeholderExtensionFor(placeholder)),
    })
  },
)

function openSearch() {
  const view = viewRef.value
  if (!view) return
  view.focus()
  openSearchPanel(view)
}

/** 在光标处插入文本（Schema 浏览器点击字段时用），保留光标位置 */
function insertText(text: string) {
  const view = viewRef.value
  if (!view) {
    emit('update:modelValue', props.modelValue + text)
    return
  }
  const { from, to } = view.state.selection.main
  view.dispatch({
    changes: { from, to, insert: text },
    selection: { anchor: from + text.length },
  })
  view.focus()
}

defineExpose({
  focus: () => viewRef.value?.focus(),
  insertText,
})
</script>

<template>
  <div
    class="code-area"
    :class="{ fill, readonly }"
    :style="fill ? undefined : { height: `${boxHeight}px` }"
  >
    <div ref="hostRef" class="editor-host" />
    <button
      v-if="searchable"
      type="button"
      class="search-btn"
      title="查找（Ctrl/Cmd + F）"
      @click.stop="openSearch"
    >
      查找
    </button>
  </div>
</template>

<style scoped>
.code-area {
  position: relative;
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

.editor-host {
  flex: 1;
  min-width: 0;
  height: 100%;
}

.code-area.readonly :deep(.cm-content) {
  color: var(--fm-text-2);
}

.code-area.readonly :deep(.cm-activeLine) {
  background: transparent;
}

/* 容器已经有 focus-within 描边，编辑器内部不再叠加全局焦点框 */
.code-area :deep(.cm-content:focus-visible) {
  outline: none !important;
}

.search-btn {
  position: absolute;
  right: 10px;
  bottom: 8px;
  z-index: 5;
  padding: 1px 8px;
  font-size: 11px;
  line-height: 16px;
  color: var(--fm-primary);
  background: color-mix(in srgb, var(--fm-code-bg) 88%, transparent);
  border: 1px solid var(--fm-border);
  border-radius: 999px;
  cursor: pointer;
}

.search-btn:hover {
  border-color: var(--fm-primary);
}

@media (max-width: 640px) {
  .code-area {
    max-height: 60vh;
  }
}
</style>