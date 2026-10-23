<script setup lang="ts">
import type { KeyValueItem } from '@/types/models'

const props = defineProps<{
  modelValue: KeyValueItem[]
  keyPlaceholder?: string
  valuePlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: KeyValueItem[]): void
}>()

function update(next: KeyValueItem[]) {
  emit('update:modelValue', next)
}

function change(index: number, field: keyof KeyValueItem, value: string | boolean) {
  const next = props.modelValue.map((item, i) => (i === index ? { ...item, [field]: value } : item))
  update(next)
}

function onCheck(index: number, e: { target?: { checked?: boolean } }) {
  change(index, 'enabled', Boolean(e.target?.checked))
}

function addRow() {
  update([...props.modelValue, { key: '', value: '', enabled: true }])
}

function removeRow(index: number) {
  update(props.modelValue.filter((_, i) => i !== index))
}
</script>

<template>
  <div class="kv-editor">
    <div v-if="modelValue.length" class="kv-head">
      <span class="col-check" />
      <span class="col-key">Key</span>
      <span class="col-value">Value</span>
      <span class="col-op" />
    </div>
    <div v-for="(item, index) in modelValue" :key="index" class="kv-row">
      <a-checkbox
        class="col-check"
        :checked="item.enabled"
        @change="onCheck(index, $event)"
      />
      <a-input
        class="col-key mono"
        size="small"
        :value="item.key"
        :placeholder="keyPlaceholder || 'key'"
        @update:value="(v: string) => change(index, 'key', v)"
      />
      <a-input
        class="col-value mono"
        size="small"
        :value="item.value"
        :placeholder="valuePlaceholder || 'value'"
        @update:value="(v: string) => change(index, 'value', v)"
      />
      <a class="col-op" @click="removeRow(index)">删除</a>
    </div>
    <a-button type="dashed" size="small" block style="margin-top: 8px" @click="addRow">添加一行</a-button>
  </div>
</template>

<style scoped>
.kv-editor {
  width: 100%;
}

.kv-head,
.kv-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.kv-head {
  font-size: 12px;
  color: var(--fm-muted);
  padding: 0 2px 6px;
}

.kv-row {
  margin-bottom: 6px;
}

.col-check {
  width: 20px;
  flex-shrink: 0;
}

.col-key {
  width: 34%;
}

.col-value {
  flex: 1;
}

.col-op {
  width: 36px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-danger);
  text-align: center;
  cursor: pointer;
}
</style>
