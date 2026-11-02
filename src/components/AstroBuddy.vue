<template>
  <div
    class="buddy"
    :class="{ 'is-dragging': dragging, 'is-pressed': pressed, 'show-intro': showIntro }"
    :style="style"
    role="button"
    tabindex="0"
    aria-label="打开航仔（可拖动位置）"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerCancel"
    @keydown.enter.prevent="emit('open')"
    @keydown.space.prevent="emit('open')"
  >
    <span class="buddy-halo" />

    <span class="buddy-body" :style="{ '--buddy-tilt': `${tilt}deg` }">
      <img class="buddy-art" :src="buddyArt" alt="" draggable="false" />
    </span>

    <span class="buddy-tip">{{ tipText }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import buddyArt from '@/assets/buddy.png'

const emit = defineEmits<{ open: [] }>()

const STORAGE_KEY = 'astroman.buddy.pos'
const TIP_KEY = 'astroman.buddy.tip'
const W = 72
const H = 121
const EDGE = 10

const pos = reactive({ x: 0, y: 0 })
const dragging = ref(false)
const pressed = ref(false)
const tilt = ref(0)
const showIntro = ref(false)

let pointerId = -1
let startX = 0
let startY = 0
let originX = 0
let originY = 0
let lastX = 0
let moved = false
let introTimer = 0

const style = computed(() => ({
  transform: `translate3d(${pos.x}px, ${pos.y}px, 0)`,
}))
const tipText = computed(() => (dragging.value ? '松手就停在这' : '航仔 · 可以拖着我走'))

function clampPos(x: number, y: number) {
  const maxX = Math.max(EDGE, window.innerWidth - W - EDGE)
  const maxY = Math.max(EDGE, window.innerHeight - H - EDGE)
  return { x: Math.min(Math.max(EDGE, x), maxX), y: Math.min(Math.max(EDGE, y), maxY) }
}

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ x: pos.x, y: pos.y }))
  } catch {
    // 存不下就只在本次会话生效
  }
}

function restore() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) {
      const saved = JSON.parse(raw) as { x?: number; y?: number }
      if (typeof saved.x === 'number' && typeof saved.y === 'number') {
        const next = clampPos(saved.x, saved.y)
        pos.x = next.x
        pos.y = next.y
        return
      }
    }
  } catch {
    // 数据坏了就回默认位置
  }
  pos.x = window.innerWidth - W - 16
  pos.y = window.innerHeight / 2 - H / 2
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) return
  pointerId = event.pointerId
  startX = event.clientX
  startY = event.clientY
  lastX = event.clientX
  originX = pos.x
  originY = pos.y
  moved = false
  pressed.value = true
  try {
    // 指针捕获让鼠标滑出角色范围后仍能继续跟手
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  } catch {
    // 合成事件或指针已失效时捕获会抛错，退化为普通拖拽
  }
}

function onPointerMove(event: PointerEvent) {
  if (!pressed.value || event.pointerId !== pointerId) return
  const dx = event.clientX - startX
  const dy = event.clientY - startY
  // 手抖 4px 内仍算点击，避免轻点时误判成拖拽
  if (!moved && Math.abs(dx) + Math.abs(dy) > 4) {
    moved = true
    dragging.value = true
  }
  if (!moved) return
  const next = clampPos(originX + dx, originY + dy)
  pos.x = next.x
  pos.y = next.y
  // 按水平速度给一点倾斜，松手回正，模仿挂件甩动的惯性
  const step = event.clientX - lastX
  lastX = event.clientX
  tilt.value = Math.max(-13, Math.min(13, tilt.value * 0.55 + step * 1.1))
}

function onPointerUp(event: PointerEvent) {
  if (event.pointerId !== pointerId) return
  const wasDragging = moved
  pressed.value = false
  dragging.value = false
  tilt.value = 0
  pointerId = -1
  moved = false
  if (wasDragging) persist()
  else emit('open')
}

function onResize() {
  const next = clampPos(pos.x, pos.y)
  pos.x = next.x
  pos.y = next.y
}

/** 指针被系统抢走（手势、切窗口）时只复位状态，不当成点击 */
function onPointerCancel(event: PointerEvent) {
  if (event.pointerId !== pointerId) return
  pressed.value = false
  dragging.value = false
  tilt.value = 0
  pointerId = -1
  moved = false
}

onMounted(() => {
  restore()
  window.addEventListener('resize', onResize)
  try {
    if (!localStorage.getItem(TIP_KEY)) {
      showIntro.value = true
      localStorage.setItem(TIP_KEY, '1')
      introTimer = window.setTimeout(() => {
        showIntro.value = false
      }, 4200)
    }
  } catch {
    // localStorage 不可用时不展示首次提示
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  if (introTimer) window.clearTimeout(introTimer)
})
</script>

<style scoped>
.buddy {
  position: fixed;
  left: 0;
  top: 0;
  z-index: 900;
  width: 72px;
  height: 121px;
  padding: 0;
  border: none;
  background: transparent;
  cursor: grab;
  touch-action: none;
  user-select: none;
  -webkit-user-select: none;
  will-change: transform;
  transition: filter 0.2s ease;
}

.buddy:focus-visible {
  outline: 2px solid var(--fm-primary);
  outline-offset: 4px;
  border-radius: 50%;
}

.buddy.is-dragging {
  cursor: grabbing;
}

.buddy-halo {
  position: absolute;
  inset: -12px -10px -6px;
  border-radius: 50%;
  background: radial-gradient(
    circle at 50% 42%,
    color-mix(in srgb, var(--fm-primary) 40%, transparent) 0%,
    transparent 68%
  );
  filter: blur(4px);
  opacity: 0.42;
  animation: buddy-halo 4.4s ease-in-out infinite;
  pointer-events: none;
  transition: opacity 0.2s ease;
}

.buddy:hover .buddy-halo,
.buddy.is-dragging .buddy-halo {
  opacity: 0.75;
}

.buddy-body {
  position: absolute;
  inset: 0;
  display: block;
  transform: rotate(var(--buddy-tilt, 0deg)) scale(1);
  transform-origin: 50% 82%;
  transition: transform 0.24s cubic-bezier(0.22, 0.9, 0.3, 1.3);
}

.buddy.is-dragging .buddy-body {
  transform: rotate(var(--buddy-tilt, 0deg)) scale(1.07);
  transition: transform 0.08s linear;
}

.buddy-art {
  width: 100%;
  height: 100%;
  display: block;
  object-fit: contain;
  user-select: none;
  -webkit-user-drag: none;
  filter: drop-shadow(0 8px 14px rgba(46, 74, 108, 0.28)) drop-shadow(0 2px 3px rgba(46, 74, 108, 0.18));
  animation: buddy-bob 4.6s ease-in-out infinite;
  will-change: transform;
}

.buddy:hover .buddy-art {
  animation-play-state: paused;
}

.buddy:hover .buddy-body {
  transform: rotate(var(--buddy-tilt, 0deg)) scale(1.04);
}

.buddy.is-dragging .buddy-art {
  animation-play-state: paused;
}

.buddy.is-pressed:not(.is-dragging) .buddy-body {
  transform: scale(0.94, 0.9);
}

.buddy-tip {
  position: absolute;
  left: 50%;
  bottom: calc(100% + 6px);
  transform: translate(-50%, 4px);
  padding: 5px 10px;
  border-radius: 8px;
  background: rgba(17, 23, 34, 0.92);
  color: #fff;
  font-size: 12px;
  line-height: 1.2;
  white-space: nowrap;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.18s ease, transform 0.18s ease;
  box-shadow: 0 6px 16px rgba(15, 23, 42, 0.24);
}

.buddy:hover .buddy-tip,
.buddy.is-dragging .buddy-tip,
.buddy.show-intro .buddy-tip {
  opacity: 1;
  transform: translate(-50%, 0);
}

@keyframes buddy-bob {
  0%,
  100% {
    transform: translateY(0) rotate(-1.2deg);
  }
  50% {
    transform: translateY(-4.5px) rotate(1.2deg);
  }
}

@keyframes buddy-halo {
  0%,
  100% {
    transform: scale(0.94);
    opacity: 0.34;
  }
  50% {
    transform: scale(1.06);
    opacity: 0.62;
  }
}

@media (prefers-reduced-motion: reduce) {
  .buddy-art,
  .buddy-halo {
    animation: none !important;
  }
}
</style>
