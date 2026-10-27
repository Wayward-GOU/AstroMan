<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(defineProps<{ size?: number }>(), { size: 38 })

const sizePx = computed(() => `${props.size}px`)
</script>

<template>
  <img class="app-logo" src="/logo.png" :width="props.size" :height="props.size" alt="FbinHttp" :style="{ width: sizePx, height: sizePx }" />
</template>

<style scoped>
.app-logo {
  display: block;
  flex-shrink: 0;
  object-fit: contain;
  border-radius: 22.9%;
  box-shadow: 0 1px 2px rgba(6, 16, 25, 0.16);
}

/* 深色侧栏下加深轮廓，避免橙色底标与背景融合 */
:global(html[data-theme='midnight']) .app-logo,
:global(html[data-theme='dusk']) .app-logo {
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.16), 0 0 12px rgba(0, 0, 0, 0.35);
}
</style>
