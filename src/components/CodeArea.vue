<script setup lang="ts">
import { computed, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    rows?: number
    readonly?: boolean
    placeholder?: string
    fill?: boolean
  }>(),
  {
    rows: 15,
    readonly: false,
    placeholder: '',
    fill: false,
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const textareaRef = ref<HTMLTextAreaElement | null>(null)
const gutterRef = ref<HTMLElement | null>(null)

const LINE_HEIGHT = 20

const lineCount = computed(() => {
  const text = props.modelValue
  if (!text) return 1
  return text.split('\n').length
})

const lineNumbers = computed(() =>
  Array.from({ length: lineCount.value }, (_, i) => i + 1),
)

const gutterWidth = computed(() => Math.max(2, String(lineCount.value).length) * 8 + 18)

const boxHeight = computed(() => props.rows * LINE_HEIGHT + 20)

function onScroll() {
  if (gutterRef.value && textareaRef.value) {
    gutterRef.value.scrollTop = textareaRef.value.scrollTop
  }
}

function onInput(e: Event) {
  const target = e.target as HTMLTextAreaElement
  emit('update:modelValue', target.value)
}

function onKeydown(e: KeyboardEvent) {
  if (props.readonly) return
  if (e.key !== 'Tab') return
  e.preventDefault()
  const el = e.target as HTMLTextAreaElement
  const start = el.selectionStart
  const end = el.selectionEnd
  const next = `${props.modelValue.slice(0, start)}  ${props.modelValue.slice(end)}`
  emit('update:modelValue', next)
  requestAnimationFrame(() => {
    el.selectionStart = start + 2
    el.selectionEnd = start + 2
  })
}

defineExpose({
  focus: () => textareaRef.value?.focus(),
})
</script>

<template>
  <div class="code-area" :class="{ fill }" :style="fill ? undefined : { height: `${boxHeight}px` }">
    <div
      ref="gutterRef"
      class="gutter"
      :style="{ width: `${gutterWidth}px` }"
      aria-hidden="true"
    >
      <div v-for="n in lineNumbers" :key="n" class="gutter-line">{{ n }}</div>
    </div>
    <textarea
      ref="textareaRef"
      class="editor"
      :value="modelValue"
      :readonly="readonly"
      :placeholder="placeholder"
      spellcheck="false"
      wrap="off"
      @input="onInput"
      @scroll="onScroll"
      @keydown="onKeydown"
    />
  </div>
</template>

<style scoped>
.code-area {
  display: flex;
  overflow: hidden;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.code-area:focus-within {
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 3px rgba(61, 107, 143, 0.1);
}

.code-area.fill {
  flex: 1;
  min-height: 0;
  height: 100%;
}

.gutter {
  flex: 0 0 auto;
  padding: 10px 0;
  overflow: hidden;
  text-align: right;
  user-select: none;
  background: var(--fm-gutter-bg);
  border-right: 1px solid var(--fm-border);
  color: var(--fm-muted);
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  font-size: 12px;
  line-height: 20px;
}

.gutter-line {
  padding: 0 8px 0 10px;
}

.editor {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 10px 12px;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--fm-text);
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  font-size: 12px;
  line-height: 20px;
  white-space: pre;
  overflow: auto;
  tab-size: 2;
}

.editor::placeholder {
  color: var(--fm-muted);
}

.editor[readonly] {
  color: var(--fm-text-2);
  cursor: default;
}

@media (max-width: 640px) {
  .code-area {
    max-height: 60vh;
  }
}
</style>
