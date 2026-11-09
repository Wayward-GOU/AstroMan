<script setup lang="ts">
import { computed, ref } from 'vue'
import type {
  DocEditOverlay,
  DocOutlineField,
  DocOutlineGroup,
  DocOutlineRequest,
} from '@/types/docs'

const props = defineProps<{
  group: DocOutlineGroup
  overlay: DocEditOverlay
  depth: number
}>()

const emit = defineEmits<{
  (e: 'toggle-request', key: string): void
  (e: 'toggle-group', key: string): void
  (e: 'description', key: string, value: string): void
  (e: 'field', requestKey: string, fieldKey: string, value: string): void
  (e: 'move-request', groupKey: string, from: number, to: number): void
}>()

const dragFrom = ref(-1)
const groupExcluded = computed(() => props.overlay.excluded.includes(props.group.key))

const orderedRequests = computed(() => {
  const order = props.overlay.order[props.group.key]
  if (!order?.length) return props.group.requests
  const rank = new Map(order.map((key, index) => [key, index]))
  return [...props.group.requests].sort(
    (left, right) => (rank.get(left.key) ?? Number.MAX_SAFE_INTEGER) - (rank.get(right.key) ?? Number.MAX_SAFE_INTEGER),
  )
})

function isExcluded(key: string): boolean {
  return props.overlay.excluded.includes(key)
}

function descriptionOf(request: DocOutlineRequest): string {
  return props.overlay.nodes[request.key]?.description ?? request.description
}

function fieldValue(request: DocOutlineRequest, field: DocOutlineField): string {
  return props.overlay.nodes[request.key]?.exampleValues?.[field.key] ?? field.value
}

function onDrop(index: number) {
  const from = dragFrom.value
  dragFrom.value = -1
  if (from >= 0 && from !== index) {
    emit('move-request', props.group.key, from, index)
  }
}
</script>

<template>
  <div class="outline-group" :style="{ paddingLeft: `${depth * 14}px` }">
    <div class="group-row">
      <a-checkbox :checked="!groupExcluded" @change="emit('toggle-group', group.key)" />
      <span class="group-name" :title="group.name">{{ group.name }}</span>
      <span v-if="group.path.length" class="group-path">{{ group.path.join(' / ') }}</span>
    </div>

    <div
      v-for="(request, index) in orderedRequests"
      :key="request.key"
      class="request-row"
      :class="{ excluded: isExcluded(request.key) || groupExcluded }"
      draggable="true"
      @dragstart="dragFrom = index"
      @dragend="dragFrom = -1"
      @dragover.prevent
      @drop="onDrop(index)"
    >
      <a-checkbox :checked="!isExcluded(request.key)" @change="emit('toggle-request', request.key)" />
      <span class="method">{{ request.method }}</span>
      <span class="request-name" :title="request.url">{{ request.name }}</span>
      <a-popover trigger="click" placement="right" :overlay-style="{ width: '360px' }">
        <template #content>
          <div class="edit-panel">
            <a-textarea
              :value="descriptionOf(request)"
              :rows="3"
              placeholder="接口说明"
              @change="(event: any) => emit('description', request.key, event.target.value)"
            />
            <div v-for="field in request.params" :key="field.key" class="edit-field">
              <span class="edit-label">
                {{ field.location }} · {{ field.name }}
                <span v-if="field.sensitive" class="locked">（已脱敏，不可改）</span>
              </span>
              <a-input
                size="small"
                :value="fieldValue(request, field)"
                :disabled="field.sensitive"
                :title="field.sensitive ? '敏感字段由真实请求转发，示例值不会被写入' : field.value"
                @change="(event: any) => emit('field', request.key, field.key, event.target.value)"
              />
            </div>
          </div>
        </template>
        <a-button size="small" type="text">编辑</a-button>
      </a-popover>
    </div>

    <DocsOutlineNode
      v-for="child in group.children"
      :key="child.key"
      :group="child"
      :overlay="overlay"
      :depth="depth + 1"
      @toggle-request="(key: string) => emit('toggle-request', key)"
      @toggle-group="(key: string) => emit('toggle-group', key)"
      @description="(key: string, value: string) => emit('description', key, value)"
      @field="(requestKey: string, fieldKey: string, value: string) => emit('field', requestKey, fieldKey, value)"
      @move-request="(groupKey: string, from: number, to: number) => emit('move-request', groupKey, from, to)"
    />
  </div>
</template>

<style scoped>
.outline-group {
  border-left: 1px solid #f0f0f0;
  margin-left: 2px;
}
.group-row,
.request-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 28px;
  padding: 2px 4px;
}
.group-row {
  font-weight: 600;
}
.group-name {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.group-path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #8c8c8c;
  font-size: 12px;
  font-weight: 400;
}
.request-row {
  cursor: move;
}
.request-row:hover {
  background: #f5f6f7;
}
.request-row.excluded {
  opacity: 0.45;
}
.method {
  min-width: 42px;
  color: #009688;
  font-size: 12px;
  font-weight: 600;
}
.request-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.edit-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 420px;
  overflow: auto;
}
.edit-field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.edit-label {
  color: #8c8c8c;
  font-size: 12px;
}
.locked {
  color: #d46b08;
}
</style>
