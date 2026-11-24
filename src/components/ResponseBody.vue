<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { CloseOutlined, DownOutlined, SearchOutlined, UpOutlined } from '@ant-design/icons-vue'
import LargeTextViewer from '@/components/LargeTextViewer.vue'
import { findMatches, highlightJson, highlightPlain } from '@/utils/jsonHighlight'
import { formatCharCount } from '@/utils/textChunk'

const props = withDefaults(
  defineProps<{
    text: string
    /** json 会区分字段名 / 字符串 / 数字 / 注释 / {{变量}} */
    language?: 'json' | 'plain'
    /**
     * 超过该字符数改为分块渲染并关闭搜索。
     * 单块 `<pre>` 装几 MB 正文时，浏览器要对全部字符做折行排版，主线程会被占满。
     */
    chunkMax?: number
    /**
     * 超过该字符数关闭语法着色，只保留搜索标记。
     * 着色成本随体积超线性增长（实测 128KB 约 0.13s、512KB 约 1.7s、1MB 约 6.7s），
     * 阈值定在 128KB 既不牺牲常规响应的体验，也避免大响应打开即卡死。
     */
    colorMax?: number
  }>(),
  {
    language: 'json',
    chunkMax: 1024 * 1024,
    colorMax: 128 * 1024,
  },
)

const rootEl = ref<HTMLElement | null>(null)
const preEl = ref<HTMLElement | null>(null)
const largeRef = ref<InstanceType<typeof LargeTextViewer> | null>(null)
const inputRef = ref<{ focus?: () => void } | null>(null)

const searchOpen = ref(false)
/** 输入框即时值，防抖后再参与匹配，避免大正文每敲一个字符都重算 */
const queryInput = ref('')
const query = ref('')
const current = ref(0)
let queryTimer: number | undefined

const chunked = computed(() => props.text.length > props.chunkMax)
const searchable = computed(() => !chunked.value)
const colorized = computed(() => props.text.length <= props.colorMax)

const matches = computed(() =>
  searchable.value ? findMatches(props.text, query.value) : [],
)
const matchCount = computed(() => matches.value.length)

const html = computed(() => {
  if (!searchable.value) return ''
  const options = { query: query.value }
  // 超过着色阈值只包搜索标记：DOM 里只有文本节点加少量 mark，排版成本与体积线性相关
  if (!colorized.value) return highlightPlain(props.text, options)
  return props.language === 'json'
    ? highlightJson(props.text, options)
    : highlightPlain(props.text, options)
})

const countText = computed(() => {
  if (!query.value) return ''
  if (!matchCount.value) return '无匹配'
  return `${current.value + 1}/${matchCount.value}`
})

function cancelQueryTimer() {
  if (queryTimer) {
    window.clearTimeout(queryTimer)
    queryTimer = undefined
  }
}

/** 只切换 mark 上的高亮类，不重新着色整段正文，大响应翻找也不会卡 */
function paintCurrent() {
  nextTick(() => {
    const pre = preEl.value
    if (!pre) return
    pre.querySelectorAll('mark.hl-current').forEach((el) => el.classList.remove('hl-current'))
    if (!matchCount.value) return
    const marks = pre.querySelectorAll<HTMLElement>(
      `mark[data-match-index="${current.value}"]`,
    )
    marks.forEach((el) => el.classList.add('hl-current'))
    const mark = marks[0]
    if (!mark) return
    const preRect = pre.getBoundingClientRect()
    const markRect = mark.getBoundingClientRect()
    pre.scrollTop += markRect.top - preRect.top - preRect.height / 2 + markRect.height / 2
  })
}

function step(delta: number) {
  if (!matchCount.value) return
  current.value = (current.value + delta + matchCount.value) % matchCount.value
}

function closeSearch() {
  searchOpen.value = false
  queryInput.value = ''
  query.value = ''
  current.value = 0
}

function openSearch(initial = '') {
  searchOpen.value = true
  if (initial) {
    queryInput.value = initial
    query.value = initial
    current.value = 0
  }
  nextTick(() => inputRef.value?.focus?.())
}

/** 双击字段名 / 字符串：直接以该内容为关键字查找全部同名字段 */
function onDblClick(event: MouseEvent) {
  const target = (event.target as HTMLElement | null)?.closest<HTMLElement>('[data-json-token]')
  const token = target?.getAttribute('data-json-token')
  if (!token) return
  const isKey = target?.getAttribute('data-json-kind') === 'key'
  openSearch(isKey ? `"${token}"` : token)
}

watch(queryInput, (value) => {
  cancelQueryTimer()
  queryTimer = window.setTimeout(() => {
    queryTimer = undefined
    const next = (value ?? '').trim()
    if (next === query.value) return
    query.value = next
    current.value = 0
  }, 120)
})

// query 变化会让 html 重算；等 DOM 更新后再标当前命中并滚动
watch([current, html], paintCurrent)

watch(matchCount, (count) => {
  if (current.value >= count) current.value = Math.max(0, count - 1)
})

// 内容整体变化（新响应 / Pretty 与 Raw 切换）时回到顶部
watch(
  () => props.text,
  () => {
    current.value = 0
    nextTick(() => {
      if (chunked.value) {
        largeRef.value?.scrollToTop()
        return
      }
      if (preEl.value) preEl.value.scrollTop = 0
    })
  },
)

onBeforeUnmount(cancelQueryTimer)

function scrollToTop() {
  if (chunked.value) largeRef.value?.scrollToTop()
  else preEl.value?.scrollTo({ top: 0 })
}

defineExpose({ openSearch, scrollToTop })
</script>

<template>
  <div ref="rootEl" class="resp-body">
    <div v-if="searchOpen && searchable" class="resp-toolbar">
      <a-input
        ref="inputRef"
        v-model:value="queryInput"
        class="resp-search"
        size="small"
        allow-clear
        placeholder="在响应中搜索；双击字段名可直接查同名字段"
        aria-label="在响应中搜索"
        @keydown.enter.exact.prevent="step(1)"
        @keydown.shift.enter.prevent="step(-1)"
        @keydown.esc.prevent="closeSearch"
      >
        <template #prefix><SearchOutlined /></template>
      </a-input>
      <span class="resp-count mono">{{ countText }}</span>
      <a-button size="small" type="text" :disabled="!matchCount" title="上一个" aria-label="上一个匹配" @click="step(-1)">
        <template #icon><UpOutlined /></template>
      </a-button>
      <a-button size="small" type="text" :disabled="!matchCount" title="下一个" aria-label="下一个匹配" @click="step(1)">
        <template #icon><DownOutlined /></template>
      </a-button>
      <a-button size="small" type="text" title="关闭搜索" aria-label="关闭搜索" @click="closeSearch">
        <template #icon><CloseOutlined /></template>
      </a-button>
    </div>

    <div v-if="chunked" class="resp-large-note">
      正文 {{ formatCharCount(text.length) }}，已关闭语法着色与搜索；内容分块展开，向下滚动继续加载
    </div>
    <div v-else-if="!colorized" class="resp-large-note">
      正文 {{ formatCharCount(text.length) }}，已关闭语法着色以保持流畅；搜索仍可用
    </div>

    <div v-if="chunked" class="resp-large-wrap">
      <LargeTextViewer ref="largeRef" :text="text" />
    </div>
    <pre v-else ref="preEl" class="resp-pre" v-html="html" @dblclick="onDblClick" />
  </div>
</template>

<style scoped>
.resp-body {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.resp-toolbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-bottom: 1px dashed var(--fm-border);
  background: var(--fm-surface-alt);
}

.resp-search {
  max-width: 360px;
}

.resp-count {
  min-width: 56px;
  text-align: right;
  font-size: 11px;
  color: var(--fm-muted);
}

.resp-large-note {
  flex-shrink: 0;
  padding: 5px 14px;
  font-size: 11px;
  color: var(--fm-warning);
}

/* 大文本模式下滚动交给 LargeTextViewer，这里只负责把它撑满剩余高度 */
.resp-large-wrap {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.resp-pre {
  position: relative;
  flex: 1;
  min-height: 0;
  box-sizing: border-box;
  margin: 0;
  padding: 14px 16px;
  overflow: auto;
  font-family: var(--fm-font-mono);
  font-size: 12.5px;
  line-height: 1.75;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

/* v-html 生成的节点没有 scoped 标记，需要用 :deep 命中 */
.resp-pre :deep(.tok-comment) {
  color: var(--fm-muted);
  font-style: italic;
}

.resp-pre :deep(.tok-key) {
  color: var(--fm-json-key);
  font-weight: 600;
}

.resp-pre :deep(.tok-string) {
  color: var(--fm-json-string);
}

.resp-pre :deep(.tok-number) {
  color: var(--fm-json-number);
}

.resp-pre :deep(.tok-punct) {
  color: var(--fm-json-punct);
}

.resp-pre :deep(.tok-var) {
  color: var(--fm-json-bool);
  font-weight: 600;
}

.resp-pre :deep(mark.hl-match) {
  background: color-mix(in srgb, var(--fm-warning) 34%, transparent);
  color: inherit;
  border-radius: 2px;
}

.resp-pre :deep(mark.hl-current) {
  background: var(--fm-primary);
  color: var(--fm-surface);
}
</style>
