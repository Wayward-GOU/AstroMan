<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { ReloadOutlined } from '@ant-design/icons-vue'
import * as api from '@/api'
import type { HttpRequestDetail, KeyValueItem } from '@/types/models'
import type { RevisionBrief, RevisionDetail, RevisionDiff } from '@/types/contract'
import { formatDateTime } from '@/utils/datetime'

const props = defineProps<{
  open: boolean
  requestId: number | null
  requestName?: string
}>()

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'restored', value: HttpRequestDetail): void
}>()

const loading = ref(false)
const restoring = ref(false)
const revisions = ref<RevisionBrief[]>([])
const selected = ref<number | null>(null)
const diff = ref<RevisionDiff | null>(null)
const diffLoading = ref(false)
const detailOpen = ref(false)
const detailLoading = ref(false)
const detail = ref<RevisionDetail | null>(null)
const detailTab = ref('content')

const selectedIndex = computed(() =>
  revisions.value.findIndex((item) => item.revision === selected.value),
)

watch(
  () => props.open,
  (open) => {
    if (open) void load()
  },
)

watch(
  () => props.requestId,
  () => {
    if (props.open) void load()
  },
)

async function load() {
  const requestId = props.requestId
  if (!requestId) {
    revisions.value = []
    return
  }
  loading.value = true
  diff.value = null
  try {
    revisions.value = await api.requestRevisionList(requestId, 50)
    selected.value = revisions.value[0]?.revision ?? null
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

function sourceLabel(source: string) {
  switch (source) {
    case 'restore':
      return '回滚'
    case 'import':
      return '导入'
    case 'ai':
      return 'AI'
    case 'reorder':
      return '排序'
    default:
      return '保存'
  }
}

function sourceColor(source: string) {
  if (source === 'restore') return 'orange'
  if (source === 'import') return 'green'
  return 'blue'
}

function authLabel(kind: string) {
  switch (kind) {
    case 'bearer':
      return 'Bearer Token'
    case 'basic':
      return 'Basic Auth'
    case 'apikey':
      return 'API Key'
    case 'oauth2':
      return 'OAuth2'
    default:
      return '无'
  }
}

function bodyTypeLabel(type: string) {
  const labels: Record<string, string> = {
    none: '无',
    json: 'JSON',
    text: 'Text',
    form: 'Form',
    multipart: 'Multipart',
    graphql: 'GraphQL',
    binary: 'Binary',
  }
  return labels[type] ?? type
}

function kvText(items: KeyValueItem[] | undefined) {
  return (items ?? [])
    .filter((item) => item.key.trim())
    .map((item) => `${item.enabled ? '' : '[停用] '}${item.key}: ${item.value}`)
    .join('\n')
}

function prettyJson(text?: string | null) {
  if (!text) return ''
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    return text
  }
}

async function viewDetail(item: RevisionBrief) {
  const requestId = props.requestId
  if (!requestId) return
  detailOpen.value = true
  detailLoading.value = true
  detail.value = null
  detailTab.value = 'content'
  try {
    detail.value = await api.requestRevisionGet(requestId, item.revision)
  } catch (e) {
    message.error(String(e))
    detailOpen.value = false
  } finally {
    detailLoading.value = false
  }
}

function closeDetail() {
  detailOpen.value = false
}

function kindLabel(kind: string) {
  switch (kind) {
    case 'added':
      return '新增'
    case 'removed':
      return '删除'
    case 'changed':
      return '修改'
    case 'type_changed':
      return '类型变更'
    case 'required_changed':
      return '必填变更'
    case 'description_changed':
      return '说明变更'
    default:
      return kind
  }
}

/** 对比「上一版本 → 这一版本」；最早的一条没有更旧版本 */
async function compare(item: RevisionBrief) {
  const requestId = props.requestId
  if (!requestId) return
  const index = revisions.value.findIndex((row) => row.revision === item.revision)
  const older = revisions.value[index + 1]
  if (!older) {
    message.info('这是最早的一个版本，没有更旧的版本可对比')
    return
  }
  selected.value = item.revision
  diffLoading.value = true
  try {
    diff.value = await api.requestRevisionDiff(requestId, older.revision, item.revision)
  } catch (e) {
    message.error(String(e))
  } finally {
    diffLoading.value = false
  }
}

function restore(item: RevisionBrief, after?: () => void) {
  const requestId = props.requestId
  if (!requestId) return
  if (item.truncated) {
    message.warning('该版本保存时已超出大小上限被截断，不能回滚')
    return
  }
  Modal.confirm({
    title: `回滚到版本 #${item.revision}`,
    content: `当前未保存的修改会被覆盖，回滚会生成一个新版本（#${item.revision} 之后）。字段说明按合并语义保留。`,
    okText: '回滚',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      restoring.value = true
      try {
        const detail = await api.requestRevisionRestore(requestId, item.revision)
        emit('restored', detail)
        message.success(`已回滚到版本 #${item.revision}`)
        await load()
        after?.()
      } catch (e) {
        message.error(String(e))
      } finally {
        restoring.value = false
      }
    },
  })
}
</script>

<template>
  <a-drawer
    :open="open"
    placement="right"
    :width="760"
    title="版本历史"
    class="history-drawer"
    @update:open="(value: boolean) => emit('update:open', value)"
  >
    <div class="history-head">
      <span class="history-hint">
        {{ requestName || '当前接口' }}：每次保存 / 自动保存都会留一个版本，最多保留最近 50 个；
        回滚会生成新版本，不删除历史。凭据只记录是否配置，不展示明文。
      </span>
      <a-button size="small" :loading="loading || restoring" @click="load">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
    </div>

    <a-spin :spinning="loading">
      <div v-if="!revisions.length" class="history-empty">
        还没有版本，保存一次后会出现在这里
      </div>
      <div v-else class="history-list">
        <div
          v-for="item in revisions"
          :key="item.id"
          class="history-row"
          :class="{ active: selected === item.revision }"
          @click="compare(item)"
        >
          <span class="history-rev mono">#{{ item.revision }}</span>
          <a-tag :color="sourceColor(item.source)">{{ sourceLabel(item.source) }}</a-tag>
          <span class="history-time mono">{{ fmtTime(item.createdAt) }}</span>
          <a-tag v-if="item.truncated" color="red">已截断</a-tag>
          <span class="history-ops">
            <a @click.stop="viewDetail(item)">详情</a>
            <a
              :class="{ disabled: selectedIndex === revisions.length - 1 }"
              @click.stop="compare(item)"
            >
              改动
            </a>
            <a
              :class="{ disabled: item.truncated || restoring }"
              @click.stop="restore(item)"
            >
              回滚
            </a>
          </span>
        </div>
      </div>
    </a-spin>

    <a-spin :spinning="diffLoading">
      <div v-if="diff" class="diff-block">
        <div class="diff-head">
          <span>
            对比 <span class="mono">#{{ diff.from.revision }}</span> →
            <span class="mono">#{{ diff.to.revision }}</span>
          </span>
          <a @click="diff = null">收起</a>
        </div>
        <div v-if="!diff.changes.length" class="history-empty">
          两个版本内容一致
        </div>
        <div
          v-for="(change, index) in diff.changes"
          :key="`${change.field}-${index}`"
          class="diff-row"
          :class="change.level"
        >
          <span class="diff-field mono" :title="change.field">{{ change.field }}</span>
          <span class="diff-kind">{{ kindLabel(change.kind) }}</span>
          <span class="diff-values mono">
            <span class="diff-before" :title="change.before">{{ change.before || '—' }}</span>
            <span class="diff-arrow">→</span>
            <span class="diff-after" :title="change.after">{{ change.after || '—' }}</span>
          </span>
        </div>
      </div>
    </a-spin>
  </a-drawer>

  <a-modal
    v-model:open="detailOpen"
    title="版本详情"
    :width="760"
    :footer="null"
    class="revision-detail-modal"
  >
    <a-spin :spinning="detailLoading">
      <template v-if="detail">
        <div class="detail-meta">
          <span class="detail-rev mono">#{{ detail.brief.revision }}</span>
          <a-tag :color="sourceColor(detail.brief.source)">
            {{ sourceLabel(detail.brief.source) }}
          </a-tag>
          <span class="detail-time mono">{{ fmtTime(detail.brief.createdAt) }}</span>
          <a-tag v-if="detail.brief.truncated" color="red">已截断</a-tag>
          <span class="detail-spacer" />
          <a-button
            size="small"
            danger
            :disabled="detail.brief.truncated || restoring"
            @click="restore(detail.brief, closeDetail)"
          >
            回滚到该版本
          </a-button>
        </div>
        <div class="detail-line mono">
          {{ detail.snapshot.method.toUpperCase() }} {{ detail.snapshot.url }}
        </div>
        <div class="detail-sub">
          {{ detail.snapshot.name }} · 认证：{{ authLabel(detail.snapshot.authKind) }}
          {{ detail.snapshot.authConfigured ? '（已配置，凭据不回显）' : '（未配置）' }}
        </div>

        <a-tabs v-model:activeKey="detailTab" size="small">
          <a-tab-pane key="content" tab="请求内容">
            <div class="detail-section">
              <div class="detail-section-title">Headers</div>
              <pre v-if="kvText(detail.snapshot.headers)" class="mono detail-code">{{
                kvText(detail.snapshot.headers)
              }}</pre>
              <div v-else class="detail-empty">无</div>
            </div>
            <div class="detail-section">
              <div class="detail-section-title">Query</div>
              <pre v-if="kvText(detail.snapshot.params)" class="mono detail-code">{{
                kvText(detail.snapshot.params)
              }}</pre>
              <div v-else class="detail-empty">无</div>
            </div>
            <div class="detail-section">
              <div class="detail-section-title">
                Body（{{ bodyTypeLabel(detail.snapshot.bodyType) }}）
              </div>
              <pre v-if="detail.snapshot.body" class="mono detail-code detail-code-tall">{{
                detail.snapshot.body
              }}</pre>
              <div v-else class="detail-empty">无</div>
            </div>
            <div
              v-if="
                detail.snapshot.preScript ||
                detail.snapshot.postScript ||
                detail.snapshot.assertions
              "
              class="detail-section"
            >
              <div class="detail-section-title">脚本与断言</div>
              <pre
                v-if="detail.snapshot.preScript"
                class="mono detail-code"
              >前置脚本：{{ detail.snapshot.preScript }}</pre>
              <pre
                v-if="detail.snapshot.postScript"
                class="mono detail-code"
              >后置脚本：{{ detail.snapshot.postScript }}</pre>
              <pre
                v-if="detail.snapshot.assertions"
                class="mono detail-code"
              >断言：{{ detail.snapshot.assertions }}</pre>
            </div>
          </a-tab-pane>
          <a-tab-pane key="request" tab="请求实体">
            <pre
              v-if="detail.snapshot.requestEntity"
              class="mono detail-code detail-code-tall"
            >{{ prettyJson(detail.snapshot.requestEntity) }}</pre>
            <div v-else class="detail-empty">该版本没有请求实体</div>
          </a-tab-pane>
          <a-tab-pane key="response" tab="返回实体">
            <pre
              v-if="detail.snapshot.responseEntity"
              class="mono detail-code detail-code-tall"
            >{{ prettyJson(detail.snapshot.responseEntity) }}</pre>
            <div v-else class="detail-empty">该版本没有返回实体</div>
          </a-tab-pane>
        </a-tabs>
      </template>
    </a-spin>
  </a-modal>
</template>

<style scoped>
.history-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 12px;
}
.history-hint {
  flex: 1;
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-3, #8a8f99);
}
.history-empty {
  padding: 24px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-text-3, #8a8f99);
}
.history-list {
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 6px;
  overflow: hidden;
}
.history-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  font-size: 12px;
  cursor: pointer;
  border-bottom: 1px solid var(--fm-border, #eef0f3);
}
.history-row:last-child {
  border-bottom: none;
}
.history-row:hover,
.history-row.active {
  background: var(--fm-fill-1, #f5f6f8);
}
.history-rev {
  width: 56px;
  font-weight: 600;
}
.history-time {
  flex: 1;
  color: var(--fm-text-3, #8a8f99);
}
.history-ops {
  display: flex;
  gap: 12px;
}
.history-ops a.disabled {
  color: var(--fm-text-4, #c0c4cc);
  pointer-events: none;
}
.diff-block {
  margin-top: 14px;
}
.diff-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  margin-bottom: 8px;
}
.diff-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 6px 10px;
  font-size: 12px;
  border-left: 3px solid var(--fm-border, #e5e7eb);
  background: var(--fm-fill-1, #fafafb);
  margin-bottom: 4px;
}
.diff-row.warning {
  border-left-color: #faad14;
}
.diff-field {
  width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.diff-kind {
  width: 72px;
  flex-shrink: 0;
  color: var(--fm-text-3, #8a8f99);
}
.diff-values {
  flex: 1;
  min-width: 0;
  display: flex;
  gap: 6px;
  overflow: hidden;
}
.diff-before,
.diff-after {
  max-width: 46%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.diff-before {
  color: var(--fm-text-3, #8a8f99);
  text-decoration: line-through;
}
.diff-arrow {
  color: var(--fm-text-4, #c0c4cc);
}

.detail-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  font-size: 12px;
}
.detail-rev {
  font-weight: 600;
}
.detail-time {
  color: var(--fm-text-3, #8a8f99);
}
.detail-spacer {
  flex: 1;
}
.detail-line {
  font-size: 13px;
  margin-bottom: 4px;
  word-break: break-all;
}
.detail-sub {
  font-size: 12px;
  color: var(--fm-text-3, #8a8f99);
  margin-bottom: 12px;
}
.detail-section {
  margin-bottom: 12px;
}
.detail-section-title {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 4px;
}
.detail-code {
  margin: 0;
  padding: 8px 10px;
  max-height: 220px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: var(--fm-fill-1, #fafafb);
  border: 1px solid var(--fm-border, #eef0f3);
  border-radius: 4px;
}
.detail-code + .detail-code {
  margin-top: 6px;
}
.detail-code-tall {
  max-height: 420px;
}
.detail-empty {
  font-size: 12px;
  color: var(--fm-text-3, #8a8f99);
}
</style>
