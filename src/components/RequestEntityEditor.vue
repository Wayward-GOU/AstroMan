<script setup lang="ts">
// 接口详情内的「请求实体 / 返回实体」编辑器
//
// 实体只随当前接口保存：说明、必填、增删字段都写回接口详情，
// 类型由发送时自动推断（只读），手工新增字段标记为「手工」。
import { computed, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import type { EntityDoc, EntityNode, EntityTarget } from '@/types/models'
import {
  addEntityField,
  clearStaleFields,
  countEntityFields,
  countStaleFields,
  entityPath,
  entityTypeLabel,
  flattenEntityRows,
  markEntityRequired,
  mergeEntityEdits,
  parseEntityDoc,
  removeEntityField,
  serializeEntityDoc,
  ENTITY_TYPES,
} from '@/utils/entitySchema'
import { requestEntityGenerate } from '@/api'

const props = defineProps<{
  /** 接口 id；未保存的请求只能先本地编辑 */
  requestId?: number
  target: EntityTarget
  /** 当前编辑器里的请求体（生成请求实体时优先使用） */
  body?: string
  bodyType?: string
  modelValue?: string | null
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | null): void
}>()

const doc = ref<EntityDoc | null>(parseEntityDoc(props.modelValue))
const search = ref('')
const collapsed = ref(new Set<string>())
const generating = ref(false)

/** 父组件回写同一份 JSON 时不再重复解析，避免光标 / 输入被重置 */
watch(
  () => props.modelValue,
  (raw) => {
    if (raw && doc.value && serializeEntityDoc(doc.value) === raw) return
    doc.value = parseEntityDoc(raw)
  },
)

function commit() {
  if (!doc.value) return
  doc.value.updatedAt = Date.now()
  emit('update:modelValue', serializeEntityDoc(doc.value))
}

function ensureDoc() {
  if (!doc.value) {
    doc.value = {
      version: 1,
      updatedAt: Date.now(),
      hiddenPaths: [],
      root: {
        name: '$',
        type: 'object',
        itemType: '',
        required: false,
        description: '',
        manual: false,
        stale: false,
        children: [],
      },
    }
  }
  return doc.value
}

const fieldCount = computed(() => (doc.value ? countEntityFields(doc.value) : 0))
const staleCount = computed(() => (doc.value ? countStaleFields(doc.value) : 0))

const containerPaths = computed(() =>
  doc.value
    ? flattenEntityRows(doc.value)
        .filter((row) => row.node.children.length > 0)
        .map((row) => row.path)
    : [],
)

const allCollapsed = computed(
  () =>
    containerPaths.value.length > 0 &&
    containerPaths.value.every((path) => collapsed.value.has(path)),
)

function toggle(path: string) {
  if (collapsed.value.has(path)) collapsed.value.delete(path)
  else collapsed.value.add(path)
}

function toggleAll() {
  if (allCollapsed.value) {
    collapsed.value = new Set()
    return
  }
  collapsed.value = new Set(containerPaths.value)
}

const visibleRows = computed(() => {
  const current = doc.value
  if (!current) return []
  const keyword = search.value.trim().toLowerCase()
  const rows: ReturnType<typeof flattenEntityRows> = []
  const walk = (parent: EntityNode, parentPath: string, depth: number, hidden: boolean) => {
    for (const child of parent.children) {
      const path = entityPath(parentPath, child)
      const typeLabel = entityTypeLabel(child)
      if (keyword) {
        const hit =
          path.toLowerCase().includes(keyword) ||
          child.description.toLowerCase().includes(keyword)
        if (hit) rows.push({ path, node: child, depth, typeLabel })
        walk(child, path, depth + 1, false)
      } else {
        if (!hidden) rows.push({ path, node: child, depth, typeLabel })
        walk(child, path, depth + 1, hidden || collapsed.value.has(path))
      }
    }
  }
  walk(current.root, '', 0, false)
  return rows
})

function canAddChild(node: EntityNode) {
  if (node.type === 'object') return true
  if (node.type !== 'array') return false
  return !['string', 'number', 'boolean', 'null', 'file'].includes(node.itemType)
}

function onRequired(node: EntityNode, event: { target?: { checked?: boolean } }) {
  markEntityRequired(node, Boolean(event.target?.checked))
  commit()
}

function onDescription(node: EntityNode, value: string) {
  if (node.description === value) return
  node.description = value
  commit()
}

function onRemove(node: EntityNode, path: string) {
  if (!doc.value) return
  const remove = () => {
    if (!doc.value) return
    if (removeEntityField(doc.value, path)) commit()
  }
  if (!node.children.length) {
    remove()
    return
  }
  Modal.confirm({
    title: `删除「${node.name}」及其子字段？`,
    content: '只删除实体里的字段结构，不会修改请求体或响应内容。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: remove,
  })
}

/* ---------- 新增字段 ---------- */
const addOpen = ref(false)
const addParent = ref('$')
const addName = ref('')
const addType = ref('string')
const typeOptions = ENTITY_TYPES.map((value) => ({ value, label: value }))

function openAdd(parentPath: string) {
  ensureDoc()
  addParent.value = parentPath
  addName.value = ''
  addType.value = 'string'
  addOpen.value = true
}

function confirmAdd() {
  if (!addName.value.trim()) {
    message.warning('请填写字段名')
    return
  }
  const current = ensureDoc()
  const added = addEntityField(current, addParent.value, addName.value, addType.value)
  if (!added) {
    message.warning('同一层级下已存在同名字段')
    return
  }
  commit()
  addOpen.value = false
}

/* ---------- 生成 ---------- */
const generateLabel = computed(() => {
  if (fieldCount.value > 0) return '重新生成'
  return props.target === 'request' ? '从当前请求体生成' : '从最近响应生成'
})

function readableError(error: unknown) {
  return String(error).replace(/^E_[A-Z_]+:\s*/, '')
}

function notifyWarnings(warnings: string[]) {
  if (!warnings.length) return
  const text =
    warnings.slice(0, 3).join('；') +
    (warnings.length > 3 ? ` 等 ${warnings.length} 条` : '')
  message.warning(text)
}

async function generate() {
  if (!props.requestId) return
  generating.value = true
  try {
    const result = await requestEntityGenerate({
      requestId: props.requestId,
      target: props.target,
      body: props.target === 'request' ? (props.body ?? null) : null,
      bodyType: props.target === 'request' ? (props.bodyType ?? null) : null,
    })
    const remote =
      props.target === 'request' ? result.requestEntity : result.responseEntity
    // 重新生成时后端已清空隐藏路径，本地未保存的说明与手工字段仍然保留
    const merged = mergeEntityEdits(remote ?? null, doc.value, false)
    if (merged) {
      doc.value = merged
      commit()
    }
    notifyWarnings(result.warnings)
  } catch (error) {
    message.warning(readableError(error))
  } finally {
    generating.value = false
  }
}

function onGenerate() {
  if (!props.requestId || generating.value) return
  if (fieldCount.value === 0) {
    void generate()
    return
  }
  Modal.confirm({
    title: '重新生成实体？',
    content: '将按最新请求体 / 响应重新推断类型与必填，字段说明和手工新增字段会保留，用户删除过的字段可能回归。',
    okText: '重新生成',
    cancelText: '取消',
    onOk: generate,
  })
}

function onClearStale() {
  if (!doc.value) return
  Modal.confirm({
    title: `清理 ${staleCount.value} 个过期字段？`,
    content: '这些字段在上一次发送中已不存在，删除后自动同步不会再加回。',
    okText: '清理',
    okType: 'danger',
    cancelText: '取消',
    onOk() {
      if (!doc.value) return
      const removed = clearStaleFields(doc.value)
      if (removed) commit()
    },
  })
}
</script>

<template>
  <div class="entity-editor">
    <div class="entity-toolbar">
      <a-tooltip :title="requestId ? '' : '先保存接口后才能生成实体'">
        <a-button
          size="small"
          type="primary"
          :loading="generating"
          :disabled="!requestId"
          @click="onGenerate"
        >
          {{ generateLabel }}
        </a-button>
      </a-tooltip>
      <a-button size="small" :disabled="!containerPaths.length" @click="toggleAll">
        {{ allCollapsed ? '展开全部' : '折叠全部' }}
      </a-button>
      <a-input
        v-model:value="search"
        class="entity-search"
        size="small"
        allow-clear
        placeholder="搜索字段"
      />
      <a-button size="small" @click="openAdd('$')">新增字段</a-button>
      <a-button v-if="staleCount" size="small" danger @click="onClearStale">
        清理过期字段（{{ staleCount }}）
      </a-button>
      <span class="entity-count">{{ fieldCount }} 个字段</span>
    </div>

    <div v-if="!requestId" class="entity-hint">
      接口尚未保存：可以先手动新增字段，保存后即可从请求体 / 响应自动生成。
    </div>

    <div v-if="!visibleRows.length" class="entity-empty">
      <template v-if="search.trim()">没有匹配「{{ search.trim() }}」的字段</template>
      <template v-else-if="!fieldCount">
        <div class="empty-title">还没有实体字段</div>
        <div class="empty-desc">
          发送一次请求即可自动生成，或点上方「新增字段」手工维护。
        </div>
        <div v-if="target === 'request'" class="empty-desc">
          自动生成仅支持 JSON / 表单请求体。
        </div>
      </template>
      <template v-else>所有字段都已折叠，点「展开全部」查看</template>
    </div>

    <div v-else class="entity-table">
      <div class="entity-row entity-head">
        <span class="col-name">字段</span>
        <span class="col-type">类型</span>
        <span class="col-required">必填</span>
        <span class="col-desc">说明</span>
        <span class="col-op">操作</span>
      </div>

      <div
        v-for="row in visibleRows"
        :key="row.path"
        class="entity-row"
        :class="{ stale: row.node.stale }"
      >
        <div class="col-name">
          <span class="indent" :style="{ width: `${row.depth * 16}px` }" />
          <button
            v-if="row.node.children.length"
            class="expander"
            type="button"
            :title="collapsed.has(row.path) ? '展开子字段' : '折叠子字段'"
            @click="toggle(row.path)"
          >
            {{ collapsed.has(row.path) ? '›' : '⌄' }}
          </button>
          <span v-else class="expander placeholder" />
          <span class="field-name mono" :title="row.path">{{ row.node.name }}</span>
          <span v-if="row.node.type === 'array'" class="field-suffix">[]</span>
          <span v-if="row.node.manual" class="field-tag manual">手工</span>
          <span v-if="row.node.stale" class="field-tag stale">已消失</span>
        </div>

        <span class="col-type">
          <a-tooltip
            v-if="row.node.type === 'unknown'"
            title="不同位置 / 数组元素中的类型不一致，已记为 unknown 并保留首个类型作为提示"
          >
            <span class="type-unknown">{{ row.typeLabel }}</span>
          </a-tooltip>
          <span v-else class="mono">{{ row.typeLabel }}</span>
        </span>

        <span class="col-required">
          <a-checkbox
            :checked="row.node.required"
            :title="row.node.manual ? '已手工调整，自动同步不再覆盖' : '按最近一次发送观测到的必填状态'"
            @change="onRequired(row.node, $event)"
          />
        </span>

        <span class="col-desc">
          <a-textarea
            :value="row.node.description"
            class="entity-desc"
            size="small"
            :auto-size="{ minRows: 1, maxRows: 3 }"
            placeholder="填写字段说明"
            @update:value="(value: string) => onDescription(row.node, value)"
          />
        </span>

        <span class="col-op">
          <a-tooltip :title="canAddChild(row.node) ? '添加子字段' : '标量字段不能添加子字段'">
            <a
              class="entity-op"
              :class="{ disabled: !canAddChild(row.node) }"
              @click="canAddChild(row.node) && openAdd(row.path)"
            >
              +
            </a>
          </a-tooltip>
          <a-tooltip title="删除字段">
            <a class="entity-op del" @click="onRemove(row.node, row.path)">✕</a>
          </a-tooltip>
        </span>
      </div>
    </div>

    <a-modal
      v-model:open="addOpen"
      title="新增字段"
      :ok-button-props="{ disabled: !addName.trim() }"
      ok-text="添加"
      cancel-text="取消"
      @ok="confirmAdd"
    >
      <div class="add-form">
        <a-input
          v-model:value="addName"
          placeholder="字段名，如 traceId"
          allow-clear
          @press-enter="confirmAdd"
        />
        <a-select v-model:value="addType" :options="typeOptions" />
        <div class="add-hint">
          添加到
          <span class="mono">{{ addParent === '$' ? '根节点' : addParent }}</span>
          ，同一层级下字段名不能重复。
        </div>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.entity-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
}

.entity-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.entity-search {
  width: 180px;
}

.entity-count {
  margin-left: auto;
  font-size: 12px;
  color: var(--fm-muted);
}

.entity-hint {
  font-size: 12px;
  color: var(--fm-warning);
}

.entity-empty {
  padding: 28px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.empty-title {
  font-size: 13px;
  color: var(--fm-text-2);
  margin-bottom: 6px;
}

.empty-desc {
  line-height: 1.8;
}

.entity-table {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
}

.entity-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border-bottom: 1px solid var(--fm-border);
}

.entity-row:last-child {
  border-bottom: none;
}

.entity-row.stale {
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
}

.entity-head {
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
}

.col-name {
  flex: 1.4;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 3px;
}

.col-type {
  width: 120px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-text-2);
}

.col-required {
  width: 46px;
  flex-shrink: 0;
  text-align: center;
}

.col-desc {
  flex: 2;
  min-width: 0;
}

.col-op {
  width: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.indent {
  flex-shrink: 0;
}

.expander {
  width: 16px;
  flex-shrink: 0;
  border: none;
  background: none;
  padding: 0;
  color: var(--fm-muted);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
}

.expander.placeholder {
  cursor: default;
}

.field-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.field-suffix {
  font-size: 11px;
  color: var(--fm-purple);
}

.field-tag {
  font-size: 10px;
  line-height: 16px;
  padding: 0 4px;
  border-radius: 3px;
  flex-shrink: 0;
}

.field-tag.manual {
  color: var(--fm-primary-dark);
  background: var(--fm-primary-soft);
}

.field-tag.stale {
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
}

.type-unknown {
  color: var(--fm-warning);
  border-bottom: 1px dashed var(--fm-warning);
  cursor: help;
}

.entity-desc {
  font-size: 12px;
}

.entity-op {
  font-size: 13px;
  color: var(--fm-primary);
  cursor: pointer;
  padding: 0 2px;
}

.entity-op.del {
  color: var(--fm-danger);
}

.entity-op.disabled {
  color: var(--fm-border-strong);
  cursor: not-allowed;
}

.add-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.add-hint {
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.7;
}
</style>
