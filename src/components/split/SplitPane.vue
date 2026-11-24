<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

/**
 * 可拖拽分栏容器（flex 版）。
 *
 * 用途：把一个容器拆成两栏，中间分隔条可拖动改变比例；比例写入 localStorage，
 * 双击分隔条或按 Home 复位。适合结构简单的「左输入 / 右输出」场景（工具箱）。
 *
 * 注意：外层必须给定高度，否则比例无处生效。
 */

type Direction = 'horizontal' | 'vertical'

const props = withDefaults(
  defineProps<{
    /** horizontal = 左右分栏，vertical = 上下分栏 */
    direction?: Direction
    /** 初始比例（第一栏占比） */
    defaultRatio?: number
    /** 第一栏最小像素尺寸，防止被拖没 */
    minFirst?: number
    /** 第二栏最小像素尺寸 */
    minSecond?: number
    /** 记住比例用的 key，空则不做持久化 */
    storageKey?: string
  }>(),
  {
    direction: 'horizontal',
    defaultRatio: 0.5,
    minFirst: 220,
    minSecond: 220,
    storageKey: '',
  },
)

const rootEl = ref<HTMLElement | null>(null)
const dragging = ref(false)
const ratio = ref(readRatio())

function readRatio(): number {
  if (!props.storageKey) return props.defaultRatio
  try {
    const raw = localStorage.getItem(props.storageKey)
    const value = raw ? Number(raw) : NaN
    if (Number.isFinite(value) && value > 0.08 && value < 0.92) return value
  } catch {
    // localStorage 不可用（隐私模式 / 配额满）时用默认比例
  }
  return props.defaultRatio
}

function persist() {
  if (!props.storageKey) return
  try {
    localStorage.setItem(props.storageKey, ratio.value.toFixed(4))
  } catch {
    // 存储失败不影响本次拖拽
  }
}

function totalSize(): number {
  const el = rootEl.value
  if (!el) return 0
  const rect = el.getBoundingClientRect()
  return props.direction === 'horizontal' ? rect.width : rect.height
}

/** 容器过小时放弃最小尺寸约束，避免两栏互相顶死 */
function clampRatio(raw: number, total: number): number {
  if (total <= 0) return props.defaultRatio
  const lo = props.minFirst / total
  const hi = 1 - props.minSecond / total
  if (lo >= hi) return props.defaultRatio
  return Math.min(Math.max(raw, lo), hi)
}

function applyPointer(clientPos: number) {
  const el = rootEl.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const total = props.direction === 'horizontal' ? rect.width : rect.height
  if (total <= 0) return
  const offset = props.direction === 'horizontal' ? clientPos - rect.left : clientPos - rect.top
  ratio.value = clampRatio(offset / total, total)
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
  applyPointer(props.direction === 'horizontal' ? event.clientX : event.clientY)
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
  ratio.value = props.defaultRatio
  persist()
}

function nudge(delta: number) {
  ratio.value = clampRatio(ratio.value + delta, totalSize())
  persist()
}

function onKeydown(event: KeyboardEvent) {
  const horizontal = props.direction === 'horizontal'
  const shrink = horizontal ? 'ArrowLeft' : 'ArrowUp'
  const grow = horizontal ? 'ArrowRight' : 'ArrowDown'
  if (event.key === shrink) {
    event.preventDefault()
    nudge(-0.02)
  } else if (event.key === grow) {
    event.preventDefault()
    nudge(0.02)
  } else if (event.key === 'Home') {
    event.preventDefault()
    reset()
  }
}

/** 窗口尺寸变化后按新容器宽度重新套一次约束，防止某栏被压到 0 */
function keepInBounds() {
  const total = totalSize()
  if (total > 0) ratio.value = clampRatio(ratio.value, total)
}

const firstStyle = computed(() => ({ flexBasis: `${(ratio.value * 100).toFixed(2)}%` }))

/**
 * 切换分栏方向时，storageKey 会跟着变（左右与上下各存一份比例）。
 * 组件不重建，所以必须主动重读，否则沿用另一方向的旧比例。
 */
watch(
  () => [props.storageKey, props.direction] as const,
  async () => {
    ratio.value = readRatio()
    await nextTick()
    keepInBounds()
  },
)

onMounted(() => window.addEventListener('resize', keepInBounds))
onBeforeUnmount(() => window.removeEventListener('resize', keepInBounds))
</script>

<template>
  <div ref="rootEl" class="split-pane" :class="[direction, { dragging }]">
    <div class="split-pane__first" :style="firstStyle">
      <slot name="first" />
    </div>
    <div
      class="split-handle"
      role="separator"
      :aria-orientation="direction === 'horizontal' ? 'vertical' : 'horizontal'"
      :aria-label="direction === 'horizontal' ? '拖动调整左右宽度' : '拖动调整上下高度'"
      :aria-valuenow="Math.round(ratio * 100)"
      aria-valuemin="8"
      aria-valuemax="92"
      tabindex="0"
      :title="direction === 'horizontal' ? '拖动调整宽度，双击复位' : '拖动调整高度，双击复位'"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @dblclick="reset"
      @keydown="onKeydown"
    >
      <span class="split-handle__line" />
    </div>
    <div class="split-pane__second">
      <slot name="second" />
    </div>
  </div>
</template>

<style scoped>
.split-pane {
  display: flex;
  min-width: 0;
  min-height: 0;
}

.split-pane.horizontal {
  flex-direction: row;
}

.split-pane.vertical {
  flex-direction: column;
}

.split-pane.dragging {
  user-select: none;
}

.split-pane__first {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.split-pane__second {
  flex: 1 1 0;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.split-pane__first > :slotted(*),
.split-pane__second > :slotted(*) {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}

.split-handle {
  flex: 0 0 9px;
  display: flex;
  align-items: center;
  justify-content: center;
  touch-action: none;
  cursor: col-resize;
  outline: none;
}

.split-pane.vertical > .split-handle {
  cursor: row-resize;
}

.split-handle__line {
  border-radius: 2px;
  background: var(--fm-border);
  transition: background 0.15s ease;
}

.split-pane.horizontal > .split-handle > .split-handle__line {
  width: 3px;
  height: 44px;
}

.split-pane.vertical > .split-handle > .split-handle__line {
  width: 44px;
  height: 3px;
}

.split-handle:hover > .split-handle__line,
.split-handle:focus-visible > .split-handle__line,
.split-pane.dragging > .split-handle > .split-handle__line {
  background: var(--fm-primary);
}
</style>
