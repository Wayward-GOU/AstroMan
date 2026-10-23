<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(defineProps<{ size?: number }>(), { size: 38 })

const uid = `fm-${Math.random().toString(36).slice(2, 8)}`
const tileId = `${uid}-tile`
const fId = `${uid}-f`
const needleId = `${uid}-needle`
const clipId = `${uid}-clip`

const sizePx = computed(() => `${props.size}px`)
</script>

<template>
  <svg
    class="app-logo"
    :width="sizePx"
    :height="sizePx"
    viewBox="0 0 96 96"
    role="img"
    aria-label="FinkMeter"
  >
    <defs>
      <linearGradient :id="tileId" x1="0" y1="0" x2="0.65" y2="1">
        <stop offset="0" stop-color="#183a59" />
        <stop offset="0.55" stop-color="#0c2035" />
        <stop offset="1" stop-color="#061019" />
      </linearGradient>
      <linearGradient :id="fId" gradientUnits="userSpaceOnUse" x1="0" y1="24" x2="0" y2="74">
        <stop offset="0" stop-color="#66a6ea" />
        <stop offset="0.5" stop-color="#2f74c4" />
        <stop offset="1" stop-color="#17457c" />
      </linearGradient>
      <linearGradient :id="needleId" x1="0" y1="1" x2="1" y2="0">
        <stop offset="0" stop-color="#ff5a24" />
        <stop offset="1" stop-color="#ff9a55" />
      </linearGradient>
      <clipPath :id="clipId">
        <circle cx="64" cy="54" r="17" />
      </clipPath>
    </defs>

    <rect width="96" height="96" rx="22" :fill="`url(#${tileId})`" />
    <rect
      x="0.8"
      y="0.8"
      width="94.4"
      height="94.4"
      rx="21.2"
      fill="none"
      stroke="#ffffff"
      stroke-opacity="0.1"
      stroke-width="1.6"
    />

    <!-- 表盘圈 -->
    <circle cx="64" cy="54" r="18" fill="none" stroke="#26496c" stroke-width="4" />

    <!-- 表盘网格 -->
    <g :clip-path="`url(#${clipId})`" stroke="#1c3a56" stroke-width="1.1">
      <path d="M 57 37 V 71 M 64 37 V 71 M 71 37 V 71" />
      <path d="M 47 47 H 81 M 47 54 H 81 M 47 61 H 81" />
    </g>

    <!-- 刻度 -->
    <path
      d="M 52.7 47.5 L 49.7 45.8 M 57.5 42.7 L 55.8 39.7 M 64 41 L 64 37.5 M 70.5 42.7 L 72.2 39.7 M 75.3 47.5 L 78.3 45.8"
      stroke="#4c7ea9"
      stroke-width="1.7"
      stroke-linecap="round"
    />

    <!-- F 字母 -->
    <g
      fill="none"
      :stroke="`url(#${fId})`"
      stroke-width="9"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d="M 30 28 H 60" />
      <path d="M 30 47 H 51" />
      <path d="M 30 28 V 58 Q 30 67 23 71" />
    </g>

    <!-- 罗盘指针 -->
    <path d="M 70 33 L 53.6 53.5 L 38 67 L 50.4 50.5 Z" :fill="`url(#${needleId})`" />

    <!-- 轴心 -->
    <circle cx="52" cy="52" r="5.4" fill="#0a1c2c" stroke="#ff6a2f" stroke-width="2" />
    <circle cx="52" cy="52" r="1.8" fill="#e9eff6" />
  </svg>
</template>

<style scoped>
.app-logo {
  display: block;
  flex-shrink: 0;
  border-radius: 22.9%;
  box-shadow: 0 1px 2px rgba(6, 16, 25, 0.16);
}

/* 深色侧栏下加深轮廓，避免深蓝底标与背景融合 */
:global(html[data-theme='midnight']) .app-logo {
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.2), 0 0 14px rgba(110, 160, 205, 0.22);
}
</style>
