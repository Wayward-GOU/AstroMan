<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(defineProps<{ size?: number }>(), { size: 38 })

const sizePx = computed(() => `${props.size}px`)
</script>

<template>
  <img class="app-logo" src="/logo.png" :width="props.size" :height="props.size" alt="AstroMan" :style="{ width: sizePx, height: sizePx }" />
</template>

<style scoped>
.app-logo {
  display: block;
  flex-shrink: 0;
  object-fit: contain;
  border-radius: 22.9%;
  box-shadow: var(--fm-shadow-badge);
}

/* 深色侧栏下加深轮廓，避免橙色底标与背景融合 */
:global(html[data-theme='midnight']) .app-logo,
:global(html[data-theme='dusk']) .app-logo {
  box-shadow: var(--fm-shadow-badge-dark);
}
</style>
