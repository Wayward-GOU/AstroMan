<script setup lang="ts">
withDefaults(
  defineProps<{
    title: string
    subtitle?: string
    /** 脚注：口径说明、降级原因等 */
    footnote?: string
    /** 占两列（宽卡片） */
    span?: 1 | 2
  }>(),
  { span: 1, subtitle: '', footnote: '' },
)
</script>

<template>
  <section class="tcard" :class="`span-${span}`">
    <header class="tcard-head">
      <div class="tcard-text">
        <h2>{{ title }}</h2>
        <p v-if="subtitle">{{ subtitle }}</p>
      </div>
      <div v-if="$slots.extra" class="tcard-extra">
        <slot name="extra" />
      </div>
    </header>
    <div class="tcard-body">
      <slot />
    </div>
    <p v-if="footnote" class="tcard-foot">{{ footnote }}</p>
  </section>
</template>

<style scoped>
.tcard {
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-card);
  box-shadow: var(--fm-shadow);
  padding: 14px 16px 10px;
  min-width: 0;
}

.tcard.span-2 {
  grid-column: span 2;
}

.tcard-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}

.tcard-text {
  min-width: 0;
}

.tcard-head h2 {
  margin: 0;
  font-size: 12.5px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--fm-text);
}

.tcard-head p {
  margin: 3px 0 0;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.tcard-extra {
  flex: none;
}

.tcard-foot {
  margin: 8px 0 2px;
  padding-top: 8px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}
</style>
