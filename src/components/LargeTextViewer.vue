<script setup lang="ts">
/**
 * 超大文本查看器。
 *
 * 只负责一件事：把几十万到几 MB 的纯文本分块渲染，让打开和滚动都不卡。
 * 不做语法着色、不做搜索——超过着色阈值后这些能力本来就关掉了（见 ResponseBody）。
 *
 * 设计要点：
 * - 每块是独立的 `<pre>`，追加时已有块不会重新排版，代价只和单块大小相关
 * - 滚动接近底部自动追加下一块，并保证先填满视口再停，避免"滚不动所以不再加载"
 * - 追加用 setTimeout 让出一次渲染，连续触底时不会同步堆积
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  DEFAULT_CHUNK_OPTIONS,
  formatCharCount,
  nextChunkEnd,
} from '@/utils/textChunk'

const props = withDefaults(
  defineProps<{
    text: string
    /** 首块字符数 */
    initialChars?: number
    /** 后续每块字符数 */
    stepChars?: number
    /** 距底部多少像素开始追加下一块 */
    threshold?: number
  }>(),
  {
    initialChars: DEFAULT_CHUNK_OPTIONS.initialChars,
    stepChars: DEFAULT_CHUNK_OPTIONS.stepChars,
    threshold: 480,
  },
)

const viewEl = ref<HTMLElement | null>(null)
const chunks = ref<string[]>([])
/** 是否已渲染完整正文 */
const done = ref(true)
const loading = ref(false)
let timer: number | undefined

const total = computed(() => props.text.length)
const loaded = computed(() => chunks.value.reduce((sum, item) => sum + item.length, 0))
const moreText = computed(
  () => `已展开 ${formatCharCount(loaded.value)} / ${formatCharCount(total.value)}，继续向下滚动加载更多`,
)

function cancel() {
  if (timer !== undefined) {
    window.clearTimeout(timer)
    timer = undefined
  }
}

function append() {
  if (done.value || loading.value) return
  loading.value = true
  cancel()
  // 让出一次渲染：先让用户看到上一块，再做这一块的排版与插入
  timer = window.setTimeout(() => {
    timer = undefined
    const start = loaded.value
    if (start >= total.value) {
      done.value = true
      loading.value = false
      return
    }
    const end = nextChunkEnd(props.text, start, props.stepChars)
    chunks.value = chunks.value.concat(props.text.slice(start, end))
    if (end >= total.value) done.value = true
    loading.value = false
    nextTick(fill)
  }, 0)
}

/** 内容不足以撑出滚动条时继续加载，否则用户没法用滚动触发后续块 */
function fill() {
  const el = viewEl.value
  if (!el || done.value || loading.value) return
  if (el.scrollHeight - el.clientHeight <= props.threshold) append()
}

function onScroll() {
  const el = viewEl.value
  if (!el || done.value || loading.value) return
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - props.threshold) append()
}

function reset() {
  cancel()
  loading.value = false
  const text = props.text
  if (!text) {
    chunks.value = []
    done.value = true
    return
  }
  const end = nextChunkEnd(text, 0, props.initialChars)
  chunks.value = [text.slice(0, end)]
  done.value = end >= text.length
  nextTick(() => {
    if (viewEl.value) viewEl.value.scrollTop = 0
    fill()
  })
}

watch(() => props.text, reset, { immediate: true })

let observer: ResizeObserver | undefined

onMounted(() => {
  // 窗口变大后已渲染内容可能不足以撑满视口，此时没有滚动条也就没有 scroll 事件，
  // 需要主动补一块，否则会停在一屏空白上
  if (typeof ResizeObserver === 'undefined') return
  const el = viewEl.value
  if (!el) return
  observer = new ResizeObserver(() => fill())
  observer.observe(el)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  observer = undefined
  cancel()
})

defineExpose({
  scrollToTop: () => viewEl.value?.scrollTo({ top: 0 }),
  /** 供测试与调试：已渲染块数 */
  chunkCount: () => chunks.value.length,
  isDone: () => done.value,
})
</script>

<template>
  <div ref="viewEl" class="vt-view" @scroll.passive="onScroll">
    <pre v-for="(chunk, index) in chunks" :key="index" class="vt-chunk">{{ chunk }}</pre>
    <div v-if="!done" class="vt-more">
      {{ loading ? '正在展开后续内容…' : moreText }}
    </div>
  </div>
</template>

<style scoped>
.vt-view {
  flex: 1 1 auto;
  height: 100%;
  min-height: 0;
  box-sizing: border-box;
  padding: 14px 16px;
  overflow: auto;
  font-family: var(--fm-font-mono);
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--fm-text-2);
}

/*
 * 块间不留边距：每个块都是独立的排版上下文，边距会让相邻块之间多出空白，
 * 视觉上出现"断开"。行高与字号继承自容器，保证跨块行距一致。
 */
.vt-chunk {
  margin: 0;
  padding: 0;
  font: inherit;
  color: inherit;
  background: none;
  white-space: pre-wrap;
  word-break: break-all;
}

.vt-more {
  padding: 12px 0 24px;
  font-family: var(--fm-font-sans);
  font-size: 11.5px;
  color: var(--fm-muted);
}
</style>
