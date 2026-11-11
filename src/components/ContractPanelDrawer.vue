<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { DownOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons-vue'
import { save } from '@tauri-apps/plugin-dialog'
import * as api from '@/api'
import type {
  ContractChange,
  ContractCheckResult,
  ContractSnapshot,
} from '@/types/contract'
import { formatDateTime } from '@/utils/datetime'

const props = defineProps<{
  open: boolean
  projectId: number
  projectName?: string
}>()

const emit = defineEmits<{ (e: 'update:open', value: boolean): void }>()

const loading = ref(false)
const creating = ref(false)
const checking = ref(false)
const exporting = ref(false)
const snapshots = ref<ContractSnapshot[]>([])
const label = ref('')
const activeId = ref<number | null>(null)
const result = ref<ContractCheckResult | null>(null)

watch(
  () => props.open,
  (open) => {
    if (open) void load()
  },
)

watch(
  () => props.projectId,
  () => {
    if (props.open) void load()
  },
)

async function load() {
  if (!props.projectId) return
  loading.value = true
  result.value = null
  try {
    snapshots.value = await api.contractSnapshotList(props.projectId)
    // 默认基线：最近一次手工创建的快照；快照被删或切换项目时回落到最新一条
    if (!snapshots.value.some((item) => item.id === activeId.value)) {
      activeId.value = snapshots.value[0]?.id ?? null
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

async function createSnapshot() {
  if (!props.projectId) return
  creating.value = true
  try {
    const created = await api.contractSnapshotCreate(props.projectId, label.value.trim())
    label.value = ''
    message.success(`已生成契约快照 #${created.id}（${created.requestCount} 个请求）`)
    await load()
    activeId.value = created.id
  } catch (e) {
    message.error(String(e))
  } finally {
    creating.value = false
  }
}

async function check(item: ContractSnapshot) {
  activeId.value = item.id
  checking.value = true
  result.value = null
  try {
    result.value = await api.contractCheck(props.projectId, item.id)
    const diff = result.value.diff
    if (diff.breaking.length) {
      message.warning(`发现 ${diff.breaking.length} 处破坏性变更`)
    } else {
      message.success('未发现破坏性变更')
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    checking.value = false
  }
}

async function checkLatest() {
  const latest = snapshots.value[0]
  if (!latest) {
    message.info('还没有契约快照，先生成一个基线')
    return
  }
  await check(latest)
}

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

function safeName() {
  return (props.projectName || 'astroman-contract').replace(/[\\/:*?"<>|]/g, '_')
}

async function exportReport(item: ContractSnapshot, format: 'markdown' | 'html') {
  exporting.value = true
  try {
    const extension = format === 'markdown' ? 'md' : 'html'
    const path = await save({
      title: '导出契约变更报告',
      defaultPath: `${safeName()}-contract-${item.id}.${extension}`,
      filters: [{ name: format === 'markdown' ? 'Markdown' : 'HTML', extensions: [extension] }],
    })
    if (!path) return
    await api.contractReport(props.projectId, item.id, format, path)
    message.success('契约报告已导出')
  } catch (e) {
    message.error(String(e))
  } finally {
    exporting.value = false
  }
}

const groups = computed(() => {
  const diff = result.value?.diff
  if (!diff) return []
  return [
    { key: 'breaking', label: '破坏性变更', items: diff.breaking },
    { key: 'compatible', label: '兼容性变更', items: diff.compatible },
    { key: 'warning', label: '提醒', items: diff.warnings },
  ].filter((group) => group.items.length)
})

function opText(change: ContractChange) {
  return `${change.method.toUpperCase()} ${change.path}`
}
</script>

<template>
  <a-drawer
    :open="open"
    placement="right"
    :width="820"
    title="契约基线"
    class="contract-drawer"
    @update:open="(value: boolean) => emit('update:open', value)"
  >
    <div class="contract-head">
      <div class="contract-hint">
        快照由项目当前的接口与实体树生成；检查只会比对结构（路径、方法、参数、实体字段），
        不比较示例值。无返回实体的接口会标记为「无实体」，不参与破坏性判定。
      </div>
      <div class="contract-create">
        <a-input
          v-model:value="label"
          size="small"
          placeholder="快照标签，如 v1.2.0（可留空）"
          class="label-input"
          @press-enter="createSnapshot"
        />
        <a-button size="small" type="primary" :loading="creating" @click="createSnapshot">
          <template #icon><PlusOutlined /></template>
          生成快照
        </a-button>
        <a-button
          size="small"
          :loading="checking"
          :disabled="!snapshots.length || loading"
          @click="checkLatest"
        >
          检查最新基线
        </a-button>
        <a-button size="small" :loading="loading || checking" @click="load">
          <template #icon><ReloadOutlined /></template>
        </a-button>
      </div>
    </div>

    <a-spin :spinning="loading || checking">
      <div v-if="!snapshots.length" class="contract-empty">
        还没有契约快照。发布前生成一个基线，之后每次改动都可以对照检查。
      </div>
      <div v-else class="snapshot-list">
        <div
          v-for="(item, index) in snapshots"
          :key="item.id"
          class="snapshot-row"
          :class="{ active: activeId === item.id }"
        >
          <span class="snapshot-id mono">#{{ item.id }}</span>
          <span class="snapshot-label" :title="item.label">
            {{ item.label }}
            <a-tag v-if="index === 0" color="blue" class="latest-tag">最新基线</a-tag>
          </span>
          <span class="snapshot-count mono">{{ item.requestCount }} 请求</span>
          <span class="snapshot-time mono">{{ fmtTime(item.createdAt) }}</span>
          <span class="snapshot-digest mono" :title="item.digest">
            {{ item.digest.slice(0, 8) || '—' }}
          </span>
          <span class="snapshot-ops">
            <a @click="check(item)">检查</a>
            <a-dropdown :trigger="['click']" :disabled="exporting">
              <a-button size="small" type="link" class="report-link">
                报告
                <DownOutlined class="report-caret" />
              </a-button>
              <template #overlay>
                <a-menu
                  @click="(info: any) =>
                    exportReport(item, String(info.key) === 'html' ? 'html' : 'markdown')"
                >
                  <a-menu-item key="markdown">Markdown（.md）</a-menu-item>
                  <a-menu-item key="html">HTML（.html）</a-menu-item>
                </a-menu>
              </template>
            </a-dropdown>
          </span>
        </div>
      </div>
    </a-spin>

    <div v-if="result" class="check-block">
      <div class="check-head">
        <span class="check-title">
          基线 <span class="mono">#{{ result.baseline.id }}</span>
          {{ result.baseline.label }} ·
          {{ result.diff.summary.paths }} 条路径 / {{ result.diff.summary.operations }} 个操作
        </span>
        <a @click="result = null">收起</a>
      </div>
      <div class="check-summary">
        <span class="pill breaking">破坏 {{ result.diff.summary.breaking }}</span>
        <span class="pill compatible">兼容 {{ result.diff.summary.compatible }}</span>
        <span class="pill warning">提醒 {{ result.diff.summary.warnings }}</span>
      </div>

      <div v-if="!groups.length" class="contract-empty">
        与基线一致，未发现任何结构变化
      </div>
      <div v-for="group in groups" :key="group.key" class="change-group">
        <div class="change-group-title">{{ group.label }}（{{ group.items.length }}）</div>
        <div
          v-for="(change, index) in group.items"
          :key="`${change.path}-${change.method}-${change.field}-${index}`"
          class="change-row"
          :class="group.key"
        >
          <span class="change-op mono">{{ opText(change) }}</span>
          <span class="change-field mono" :title="change.field">{{ change.field || '—' }}</span>
          <span class="change-msg">{{ change.message }}</span>
        </div>
      </div>

      <div v-if="result.diff.schemaMissing.length" class="schema-missing">
        <div class="schema-missing-title">无返回实体（响应侧不参与判定）</div>
        <div v-for="item in result.diff.schemaMissing" :key="item" class="mono schema-item">
          {{ item }}
        </div>
      </div>
    </div>
  </a-drawer>
</template>

<style scoped>
.contract-head {
  margin-bottom: 12px;
}
.contract-hint {
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-3, #8a8f99);
  margin-bottom: 10px;
}
.contract-create {
  display: flex;
  gap: 8px;
}
.label-input {
  flex: 1;
}
.contract-empty {
  padding: 24px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-text-3, #8a8f99);
}
.snapshot-list {
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 6px;
  overflow: hidden;
}
.snapshot-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  font-size: 12px;
  border-bottom: 1px solid var(--fm-border, #eef0f3);
}
.snapshot-row:last-child {
  border-bottom: none;
}
.snapshot-row.active {
  background: var(--fm-fill-1, #f5f6f8);
}
.latest-tag {
  margin-left: 4px;
}
.snapshot-id {
  width: 48px;
  font-weight: 600;
}
.snapshot-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.snapshot-count {
  width: 70px;
  color: var(--fm-text-3, #8a8f99);
}
.snapshot-time {
  width: 150px;
  color: var(--fm-text-3, #8a8f99);
}
.snapshot-digest {
  width: 70px;
  color: var(--fm-text-4, #c0c4cc);
}
.snapshot-ops {
  display: flex;
  gap: 12px;
}
.report-caret {
  font-size: 10px;
}
.check-block {
  margin-top: 16px;
}
.check-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  margin-bottom: 8px;
}
.check-summary {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.pill {
  padding: 1px 8px;
  font-size: 12px;
  border-radius: 10px;
  background: var(--fm-fill-1, #f5f6f8);
}
.pill.breaking {
  color: #cf1322;
  background: #fff1f0;
}
.pill.compatible {
  color: #237804;
  background: #f6ffed;
}
.pill.warning {
  color: #ad6800;
  background: #fffbe6;
}
.change-group {
  margin-bottom: 10px;
}
.change-group-title {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 4px;
}
.change-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 6px 10px;
  font-size: 12px;
  border-left: 3px solid var(--fm-border, #e5e7eb);
  background: var(--fm-fill-1, #fafafb);
  margin-bottom: 4px;
}
.change-row.breaking {
  border-left-color: #cf1322;
}
.change-row.compatible {
  border-left-color: #52c41a;
}
.change-row.warning {
  border-left-color: #faad14;
}
.change-op {
  width: 190px;
  flex-shrink: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.change-field {
  width: 220px;
  flex-shrink: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-3, #8a8f99);
}
.change-msg {
  flex: 1;
  min-width: 0;
}
.schema-missing {
  margin-top: 10px;
  padding: 8px 10px;
  font-size: 12px;
  background: #fffbe6;
  border-radius: 4px;
}
.schema-missing-title {
  font-weight: 600;
  margin-bottom: 4px;
}
.schema-item {
  color: var(--fm-text-3, #8a8f99);
}
</style>
