<script setup lang="ts">
defineProps<{
  title: string
  subtitle?: string
  eyebrow?: string
}>()
</script>

<template>
  <div class="page-header">
    <div class="text">
      <div v-if="eyebrow" class="eyebrow">{{ eyebrow }}</div>
      <h1 class="page-title">{{ title }}</h1>
      <p v-if="subtitle" class="page-subtitle">{{ subtitle }}</p>
    </div>
    <div v-if="$slots.default" class="actions">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding-bottom: 12px;
  margin-bottom: 14px;
  border-bottom: 1px solid var(--fm-border);
}

.text {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 10px;
  min-width: 0;
}

.eyebrow {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.8px;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.page-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.1px;
  margin: 0;
}

.page-subtitle {
  font-size: 11px;
  margin: 0;
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
</style>
