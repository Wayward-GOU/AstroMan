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
  align-items: flex-end;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 20px;
}

.text {
  min-width: 0;
}

.eyebrow {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
}

.page-title {
  margin: 8px 0 0;
  font-family: var(--fm-font-serif);
  font-size: 28px;
  line-height: 1.25;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: var(--fm-text);
}

.page-subtitle {
  margin: 7px 0 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
}

@media (max-width: 760px) {
  .page-header {
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
  }

  .actions {
    flex-wrap: wrap;
  }
}
</style>
