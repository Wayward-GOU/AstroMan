<script setup lang="ts">
import type { SplitDirection } from '@/composables/useSplitLayout'

/**
 * 分栏方向切换（左右 / 上下）。
 *
 * 图标是自绘的「方框被切开」，比 antd 的 ColumnWidth / ColumnHeight 直观：
 * 那两个图标长得像字母 H 和 I，容易被当成排版或表格功能。
 */
defineProps<{ modelValue: SplitDirection }>()
const emit = defineEmits<{ 'update:modelValue': [SplitDirection] }>()
</script>

<template>
  <div class="fm-split-switch" role="group" aria-label="分栏方向">
    <a-tooltip title="左右分栏">
      <button
        type="button"
        :class="{ active: modelValue === 'horizontal' }"
        :aria-pressed="modelValue === 'horizontal'"
        aria-label="左右分栏"
        @click="emit('update:modelValue', 'horizontal')"
      >
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
          <rect
            x="1.6"
            y="2.6"
            width="10.8"
            height="8.8"
            rx="1.2"
            stroke="currentColor"
            stroke-width="1.2"
          />
          <line x1="7" y1="2.6" x2="7" y2="11.4" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    </a-tooltip>
    <a-tooltip title="上下分栏，拖动中间横条调整高度">
      <button
        type="button"
        :class="{ active: modelValue === 'vertical' }"
        :aria-pressed="modelValue === 'vertical'"
        aria-label="上下分栏"
        @click="emit('update:modelValue', 'vertical')"
      >
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
          <rect
            x="1.6"
            y="2.6"
            width="10.8"
            height="8.8"
            rx="1.2"
            stroke="currentColor"
            stroke-width="1.2"
          />
          <line x1="1.6" y1="7" x2="12.4" y2="7" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    </a-tooltip>
  </div>
</template>
