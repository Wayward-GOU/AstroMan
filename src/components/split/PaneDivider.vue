<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'

/**
 * 网格容器里的可拖拽分隔条（绝对定位版）。
 *
 * 用途：宿主容器已经是 `display: grid` 的双列布局（如请求编辑器的 Request / Response），
 * 不方便改成 slot 分栏时，插一条分隔条进来改第一列宽度即可，不改动原有 DOM 结构。
 *
 * 工作方式：把第一列宽度写成宿主容器上的 CSS 变量（默认 `--split-first`），
 * 宿主样式用 `grid-template-columns: var(--split-first, calc(50% - 6px)) minmax(0, 1fr)` 接收。
 * 拖拽过程只改 CSS 变量、不触发上层组件重渲染，避免拖一下卡一下。
 *
 * 宿主必须 `position: relative`；容器过窄（单列断点）时由宿主隐藏本组件。
 */

const props = withDefaults(
  defineProps<{
    /** 写第一列宽度用的 CSS 变量名 */
    cssVar?: string
    defaultRatio?: number
    minFirst?: number
    minSecond?: number
    storageKey?: string
  }>(),
  {
    cssVar: '--split-first',
    defaultRatio: 0.5,
    minFirst: 280,
    minSecond: 280,
    storageKey: '',
  },
)

const DEFAULT_DIVIDER_VAR = '--split-divider-left'

const rootEl = ref<HTMLElement | null>(null)
const dragging = ref(false)
let ratio = readRatio()
let observer: ResizeObserver | null = null

function host(): HTMLElement | null {
  return rootEl.value?.parentElement ?? null
}

function readRatio(): number {
  if (!props.storageKey) return props.defaultRatio
  try {
    const raw = localStorage.getItem(props.storageKey)
    const value = raw ? Number(raw) : NaN
    if (Number.isFinite(value) && value > 0.08 && value < 0.92) return value
  } catch {
    // localStorage 不可用时用默认比例
  }
  return props.defaultRatio
}

function persist() {
  if (!props.storageKey) return
  try {
    localStorage.setItem(props.storageKey, ratio.toFixed(4))
  } catch {
    // 存储失败不影响本次拖拽
  }
}

function clampRatio(raw: number, content: number): number {
  if (content <= 0) return props.defaultRatio
  const lo = props.minFirst / content
  const hi = 1 - props.minSecond / content
  if (lo >= hi) return props.defaultRatio
  return Math.min(Math.max(raw, lo), hi)
}

/** 宿主内容盒宽度：clientWidth 含 padding，栅格百分比按内容盒算 */
function contentWidth(el: HTMLElement, style: CSSStyleDeclaration): number {
  const padLeft = parseFloat(style.paddingLeft) || 0
  const padRight = parseFloat(style.paddingRight) || 0
  return el.clientWidth - padLeft - padRight
}

/** 把比例写回宿主：第一列宽度 + 分隔条中心位置 */
function syncToHost() {
  const el = host()
  if (!el) return
  const style = getComputedStyle(el)
  const content = contentWidth(el, style)
  if (content <= 0) return
  const padLeft = parseFloat(style.paddingLeft) || 0
  const gap = parseFloat(style.columnGap) || 0
  const first = Math.round(ratio * content)
  el.style.setProperty(props.cssVar, `${first}px`)
  el.style.setProperty(DEFAULT_DIVIDER_VAR, `${padLeft + first + gap / 2}px`)
}

function applyPointer(clientX: number) {
  const el = host()
  if (!el) return
  const style = getComputedStyle(el)
  const content = contentWidth(el, style)
  if (content <= 0) return
  const rect = el.getBoundingClientRect()
  const border = parseFloat(style.borderLeftWidth) || 0
  const padLeft = parseFloat(style.paddingLeft) || 0
  ratio = clampRatio((clientX - rect.left - border - padLeft) / content, content)
  syncToHost()
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) return
  const handle = event.currentTarget as HTMLElement
  handle.setPointerCapture(event.pointerId)
  dragging.value = true
  event.preventDefault()
}

function onPointerMove(event: PointerEvent) {
  if (!dragging.value) return
  applyPointer(event.clientX)
}

function onPointerUp(event: PointerEvent) {
  if (!dragging.value) return
  dragging.value = false
  const handle = event.currentTarget as HTMLElement
  try {
    handle.releasePointerCapture(event.pointerId)
  } catch {
    // 指针已释放时忽略
  }
  persist()
}

function reset() {
  ratio = props.defaultRatio
  syncToHost()
  persist()
}

function nudge(delta: number) {
  const el = host()
  if (!el) return
  const content = contentWidth(el, getComputedStyle(el))
  ratio = clampRatio(ratio + delta, content)
  syncToHost()
  persist()
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowLeft') {
    event.preventDefault()
    nudge(-0.02)
  } else if (event.key === 'ArrowRight') {
    event.preventDefault()
    nudge(0.02)
  } else if (event.key === 'Home') {
    event.preventDefault()
    reset()
  }
}

onMounted(() => {
  syncToHost()
  const el = host()
  if (el && typeof ResizeObserver !== 'undefined') {
    // 窗口变宽变窄时按原比例重算像素宽度，避免第一列卡在旧尺寸
    observer = new ResizeObserver(() => syncToHost())
    observer.observe(el)
  }
})

onBeforeUnmount(() => {
  observer?.disconnect()
  observer = null
})
</script>

<template>
  <div
    ref="rootEl"
    class="pane-divider"
    :class="{ dragging }"
    role="separator"
    aria-orientation="vertical"
    aria-label="拖动调整请求区与响应区宽度"
    :aria-valuenow="Math.round(ratio * 100)"
    aria-valuemin="8"
    aria-valuemax="92"
    tabindex="0"
    title="拖动调整宽度，双击复位"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @dblclick="reset"
    @keydown="onKeydown"
  >
    <span class="pane-divider__line" />
  </div>
</template>

<style scoped>
.pane-divider {
  position: absolute;
  top: 0;
  bottom: 0;
  left: var(--split-divider-left, calc(50% - 6px));
  width: 12px;
  margin-left: -6px;
  display: flex;
  align-items: center;
  justify-content: center;
  touch-action: none;
  cursor: col-resize;
  outline: none;
  z-index: 4;
}

.pane-divider__line {
  width: 3px;
  height: 44px;
  border-radius: 2px;
  background: var(--fm-border);
  transition: background 0.15s ease;
}

.pane-divider:hover .pane-divider__line,
.pane-divider:focus-visible .pane-divider__line,
.pane-divider.dragging .pane-divider__line {
  background: var(--fm-primary);
}
</style>
