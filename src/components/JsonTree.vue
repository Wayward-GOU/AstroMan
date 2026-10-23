<script setup lang="ts">
import { computed, ref, watch } from 'vue'

type ExpandMode = 'default' | 'all' | 'none'

const props = withDefaults(
  defineProps<{
    value: unknown
    nodeKey?: string | number | null
    depth?: number
    mode?: ExpandMode
    comma?: boolean
  }>(),
  {
    nodeKey: null,
    depth: 0,
    mode: 'default',
    comma: false,
  },
)

defineOptions({ name: 'JsonTree' })

const type = computed(() => {
  const value = props.value
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  return typeof value
})

const isContainer = computed(() => type.value === 'object' || type.value === 'array')

const entries = computed<Array<{ key: string | number; value: unknown }>>(() => {
  if (type.value === 'array') {
    return (props.value as unknown[]).map((item, index) => ({ key: index, value: item }))
  }
  if (type.value === 'object') {
    return Object.entries(props.value as Record<string, unknown>).map(([key, value]) => ({
      key,
      value,
    }))
  }
  return []
})

const size = computed(() => entries.value.length)

const openBrace = computed(() => (type.value === 'array' ? '[' : '{'))
const closeBrace = computed(() => (type.value === 'array' ? ']' : '}'))

function initialExpanded(): boolean {
  if (!isContainer.value) return false
  if (props.mode === 'all') return true
  if (props.mode === 'none') return false
  return props.depth < 2 && size.value > 0 && size.value <= 200
}

const expanded = ref(initialExpanded())

watch(
  () => props.mode,
  (mode) => {
    if (!isContainer.value) return
    if (mode === 'all') expanded.value = true
    else if (mode === 'none') expanded.value = false
    else expanded.value = initialExpanded()
  },
)

const keyText = computed(() => {
  if (props.nodeKey === null) return ''
  return typeof props.nodeKey === 'number' ? `[${props.nodeKey}]` : `"${props.nodeKey}"`
})

const scalarText = computed(() => {
  const value = props.value
  if (value === null) return 'null'
  if (typeof value === 'string') return JSON.stringify(value)
  return String(value)
})

function toggle() {
  expanded.value = !expanded.value
}
</script>

<template>
  <div class="jt-node">
    <div v-if="isContainer" class="jt-line">
      <button class="jt-toggle" :class="{ leaf: false }" @click="toggle">
        {{ expanded ? '▾' : '▸' }}
      </button>
      <span v-if="nodeKey !== null" class="jt-key">{{ keyText }}</span>
      <span v-if="nodeKey !== null" class="jt-punct">: </span>
      <span class="jt-punct">{{ openBrace }}</span>
      <template v-if="!expanded">
        <span class="jt-summary">… {{ size }} 项</span>
        <span class="jt-punct">{{ closeBrace }}</span>
        <span v-if="comma" class="jt-punct">,</span>
      </template>
      <span v-else class="jt-count">{{ size }} 项</span>
    </div>

    <template v-if="isContainer && expanded">
      <div class="jt-children">
        <JsonTree
          v-for="(item, index) in entries"
          :key="`${String(item.key)}`"
          :value="item.value"
          :node-key="item.key"
          :depth="depth + 1"
          :mode="mode"
          :comma="index < entries.length - 1"
        />
      </div>
      <div class="jt-line jt-close">
        <span class="jt-punct">{{ closeBrace }}</span>
        <span v-if="comma" class="jt-punct">,</span>
      </div>
    </template>

    <div v-if="!isContainer" class="jt-line">
      <span class="jt-toggle-placeholder" />
      <span v-if="nodeKey !== null" class="jt-key">{{ keyText }}</span>
      <span v-if="nodeKey !== null" class="jt-punct">: </span>
      <span class="jt-value" :class="`is-${type}`">{{ scalarText }}</span>
      <span v-if="comma" class="jt-punct">,</span>
    </div>
  </div>
</template>

<style scoped>
.jt-node {
  font-family: 'JetBrains Mono', 'Cascadia Code', 'SFMono-Regular', Consolas, monospace;
  font-size: 12px;
  line-height: 1.75;
}

.jt-line {
  display: flex;
  align-items: flex-start;
  white-space: pre-wrap;
  word-break: break-word;
}

.jt-toggle {
  width: 14px;
  flex-shrink: 0;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--fm-muted);
  font-size: 10px;
  line-height: 21px;
  cursor: pointer;
  text-align: left;
}

.jt-toggle:hover {
  color: var(--fm-primary);
}

.jt-toggle-placeholder {
  width: 14px;
  flex-shrink: 0;
}

.jt-key {
  color: var(--fm-json-key);
  font-weight: 500;
}

.jt-punct {
  color: var(--fm-json-punct);
}

.jt-summary {
  color: var(--fm-muted);
  font-style: italic;
  padding: 0 3px;
}

.jt-count {
  margin-left: 6px;
  color: var(--fm-muted);
  font-size: 10px;
}

.jt-children {
  margin-left: 7px;
  padding-left: 7px;
  border-left: 1px dashed var(--fm-border);
}

.jt-close {
  padding-left: 14px;
}

.jt-value.is-string {
  color: var(--fm-json-string);
}

.jt-value.is-number {
  color: var(--fm-json-number);
}

.jt-value.is-boolean {
  color: var(--fm-json-bool);
}

.jt-value.is-null,
.jt-value.is-undefined {
  color: var(--fm-muted);
  font-style: italic;
}

.jt-value.is-object,
.jt-value.is-array {
  color: var(--fm-json-punct);
}
</style>
