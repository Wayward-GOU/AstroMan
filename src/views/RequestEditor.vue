<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import {
  ArrowLeftOutlined,
  ThunderboltOutlined,
  SendOutlined,
  SaveOutlined,
  DeleteOutlined,
} from '@ant-design/icons-vue'
import KeyValueEditor from '@/components/KeyValueEditor.vue'
import CodeArea from '@/components/CodeArea.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import type { HttpRequestDetail, HttpResult, RequestRun } from '@/types/models'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()

const requestId = computed(() => Number(route.params.id))
const projectId = computed(() => Number(route.query.projectId || 0))

const form = ref<HttpRequestDetail>({
  collectionId: 0,
  name: '',
  method: 'GET',
  url: '',
  headers: [],
  params: [],
  bodyType: 'none',
  body: '',
  preScript: '',
  postScript: '',
  assertions: '',
})

const loading = ref(true)
const saving = ref(false)
const sending = ref(false)
const result = ref<HttpResult | null>(null)
const activeTab = ref('params')
const responseTab = ref('body')
const insecure = ref(false)
const envId = ref<number | null>(null)

const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const bodyTypes = [
  { label: 'JSON', value: 'json' },
  { label: 'Raw', value: 'raw' },
  { label: 'Form', value: 'form' },
  { label: '无', value: 'none' },
]

const enabledParams = computed(() => form.value.params.filter((p) => p.enabled && p.key).length)
const enabledHeaders = computed(() => form.value.headers.filter((h) => h.enabled && h.key).length)

interface HeaderPreset {
  id: number
  group: string
  key: string
  value: string
}

const headerPresets: HeaderPreset[] = [
  { id: 1, group: '常用', key: 'Accept', value: 'application/json' },
  { id: 2, group: '常用', key: 'Content-Type', value: 'application/json' },
  { id: 3, group: '常用', key: 'User-Agent', value: 'FinkMeter/1.0' },
  { id: 4, group: '常用', key: 'Cache-Control', value: 'no-cache' },
  { id: 5, group: '常用', key: 'Connection', value: 'keep-alive' },
  { id: 6, group: '认证', key: 'Authorization', value: 'Bearer {{token}}' },
  { id: 7, group: '认证', key: 'Authorization', value: 'Basic {{credentials}}' },
  { id: 8, group: '认证', key: 'X-Api-Key', value: '{{api_key}}' },
  { id: 9, group: '认证', key: 'X-Auth-Token', value: '{{token}}' },
  { id: 10, group: '内容协商', key: 'Accept', value: 'text/html,application/xhtml+xml' },
  { id: 11, group: '内容协商', key: 'Accept-Encoding', value: 'gzip, deflate, br' },
  { id: 12, group: '内容协商', key: 'Accept-Language', value: 'zh-CN,zh;q=0.9' },
  { id: 13, group: '内容协商', key: 'Content-Type', value: 'application/x-www-form-urlencoded' },
  { id: 14, group: '内容协商', key: 'Content-Type', value: 'multipart/form-data' },
  { id: 15, group: '客户端信息', key: 'Origin', value: '{{base_url}}' },
  { id: 16, group: '客户端信息', key: 'Referer', value: '{{base_url}}/' },
  { id: 17, group: '客户端信息', key: 'Cookie', value: 'session={{session_id}}' },
  { id: 18, group: '客户端信息', key: 'X-Requested-With', value: 'XMLHttpRequest' },
  { id: 19, group: '客户端信息', key: 'X-Request-Id', value: '{{uuid}}' },
  { id: 20, group: '条件请求', key: 'If-None-Match', value: '{{etag}}' },
  { id: 21, group: '条件请求', key: 'If-Modified-Since', value: '{{last_modified}}' },
]

const headerGroups = computed(() => {
  const groups: { label: string; items: HeaderPreset[] }[] = []
  for (const preset of headerPresets) {
    let group = groups.find((item) => item.label === preset.group)
    if (!group) {
      group = { label: preset.group, items: [] }
      groups.push(group)
    }
    group.items.push(preset)
  }
  return groups
})

function applyHeaderPreset(id: number) {
  const preset = headerPresets.find((item) => item.id === id)
  if (!preset) return
  const existing = form.value.headers.find(
    (item) => item.key.trim().toLowerCase() === preset.key.toLowerCase(),
  )
  if (existing) {
    existing.value = preset.value
    existing.enabled = true
  } else {
    form.value.headers.push({ key: preset.key, value: preset.value, enabled: true })
  }
  if (preset.key.toLowerCase() === 'content-type' && form.value.bodyType === 'none') {
    if (preset.value.includes('json')) form.value.bodyType = 'json'
    else if (preset.value.includes('urlencoded')) form.value.bodyType = 'form'
  }
  message.success(`已加入 ${preset.key}`)
}

onMounted(async () => {
  try {
    if (projectId.value && !store.environments.length) {
      store.environments = await api.envList(projectId.value)
    }
    const active = store.environments.find((e) => e.isActive) ?? store.environments[0]
    envId.value = active?.id ?? null
    form.value = await api.requestGet(requestId.value)
    await loadRuns()
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

async function save(showToast = true): Promise<boolean> {
  saving.value = true
  try {
    const saved = await api.requestSave(form.value)
    form.value = saved
    if (showToast) message.success('已保存')
    return true
  } catch (e) {
    message.error(String(e))
    return false
  } finally {
    saving.value = false
  }
}

function deleteRequest() {
  const id = form.value.id
  if (!id) return
  Modal.confirm({
    title: `删除请求「${form.value.name || '未命名请求'}」？`,
    content: '该请求及其参数、请求头、Body 将被永久删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.requestDelete(id)
      message.success('已删除请求')
      router.push(projectId.value ? `/project/${projectId.value}` : '/')
    },
  })
}

async function send() {
  if (!form.value.url.trim()) {
    message.warning('请填写 URL')
    return
  }
  sending.value = true
  result.value = null
  try {
    result.value = await api.sendRequest({
      requestId: form.value.id ?? null,
      name: form.value.name,
      method: form.value.method,
      url: form.value.url,
      headers: form.value.headers,
      params: form.value.params,
      bodyType: form.value.bodyType,
      body: form.value.body,
      envId: envId.value,
      insecure: insecure.value,
      timeoutMs: 30000,
    })
  } catch (e) {
    message.error(String(e))
  } finally {
    sending.value = false
  }
  await loadRuns()
}

const runs = ref<RequestRun[]>([])
const runsOpen = ref(true)
const runsLoading = ref(false)
const expandedRunId = ref<number | null>(null)

async function loadRuns() {
  if (!form.value.id) return
  runsLoading.value = true
  try {
    runs.value = await api.runList(form.value.id, 50)
  } catch (e) {
    message.error(String(e))
  } finally {
    runsLoading.value = false
  }
}

function toggleRun(run: RequestRun) {
  expandedRunId.value = expandedRunId.value === run.id ? null : run.id
}

function runStatusColor(status: number) {
  if (!status) return 'var(--fm-danger)'
  if (status < 300) return 'var(--fm-success)'
  if (status < 400) return 'var(--fm-warning)'
  return 'var(--fm-danger)'
}

function fmtRunTime(ts: number) {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

function runHeaders(run: RequestRun) {
  return Object.entries(run.responseHeaders ?? {})
}

function prettyRunBody(text: string) {
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    return text
  }
}

function clearRuns() {
  const id = form.value.id
  if (!id) return
  Modal.confirm({
    title: '清空该接口的运行历史？',
    content: '只删除这个接口的运行记录，不影响接口本身与压测报告。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.runClear(id)
      runs.value = []
      expandedRunId.value = null
      message.success('已清空')
    },
  })
}

async function openStress() {
  if (!form.value.url.trim()) {
    message.warning('请先填写 URL')
    return
  }
  const ok = await save(false)
  if (!ok) return
  router.push({
    name: 'stress',
    query: {
      requestId: String(form.value.id ?? requestId.value),
      projectId: projectId.value ? String(projectId.value) : undefined,
    },
  })
}

const prettyBody = computed(() => {
  const body = result.value?.body ?? ''
  try {
    return JSON.stringify(JSON.parse(body), null, 2)
  } catch {
    return body
  }
})

const statusColor = computed(() => {
  const code = result.value?.status ?? 0
  if (!code) return '#64748b'
  if (code < 300) return 'var(--fm-success)'
  if (code < 400) return 'var(--fm-warning)'
  return 'var(--fm-danger)'
})

function fmtSize(n: number) {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}

const responseHeaders = computed(() => {
  if (!result.value) return []
  return Object.entries(result.value.headers).map(([key, value]) => ({ key, value }))
})
</script>

<template>
  <a-spin :spinning="loading">
    <div class="editor-page">
      <div class="topbar">
        <div class="left">
          <a-button type="text" size="small" class="back-btn" @click="router.push(`/project/${projectId}`)">
            <template #icon><ArrowLeftOutlined /></template>
            返回
          </a-button>
          <span class="divider" />
          <a-input v-model:value="form.name" class="name-input" placeholder="请求名称" />
          <span class="saved-hint">{{ form.id ? `#${form.id}` : '未保存' }}</span>
        </div>
        <div class="right">
          <a-checkbox v-model:checked="insecure" class="insecure">忽略证书校验</a-checkbox>
          <a-button v-if="form.id" type="text" size="small" danger class="del-btn" @click="deleteRequest">
            <template #icon><DeleteOutlined /></template>
            删除
          </a-button>
          <a-button :loading="saving" @click="save()">
            <template #icon><SaveOutlined /></template>
            保存
          </a-button>
          <a-button class="stress-btn" @click="openStress">
            <template #icon><ThunderboltOutlined /></template>
            压测
          </a-button>
          <a-button type="primary" :loading="sending" @click="send">
            <template #icon><SendOutlined /></template>
            发送
          </a-button>
        </div>
      </div>

      <div class="url-bar">
        <a-select v-model:value="form.method" class="method-select" :class="`m-${form.method}`">
          <a-select-option v-for="m in methods" :key="m" :value="m">{{ m }}</a-select-option>
        </a-select>
        <a-input
          v-model:value="form.url"
          class="url-input mono"
          placeholder="https://api.example.com/users 或 {{base_url}}/users"
          @press-enter="send"
        />
        <a-select v-model:value="envId" class="env-select" placeholder="不使用环境" allow-clear>
          <a-select-option v-for="env in store.environments" :key="env.id" :value="env.id">
            {{ env.name }}
          </a-select-option>
        </a-select>
      </div>

      <div class="panes">
        <div class="req-pane fm-card">
          <div class="pane-head">
            <span class="pane-title">请求配置</span>
            <span class="pane-meta">{{ enabledParams }} Params · {{ enabledHeaders }} Headers</span>
          </div>
          <a-tabs v-model:active-key="activeTab" size="small" class="tabs">
            <a-tab-pane key="params" tab="Params">
              <div class="tab-body">
                <KeyValueEditor v-model="form.params" key-placeholder="参数名" value-placeholder="参数值" />
              </div>
            </a-tab-pane>
            <a-tab-pane key="headers" tab="Headers">
              <div class="tab-body">
                <div class="header-tools">
                  <a-select
                    class="preset-select"
                    size="small"
                    placeholder="插入常用请求头"
                    :value="undefined"
                    @change="(v: number) => applyHeaderPreset(v)"
                  >
                    <a-select-opt-group
                      v-for="group in headerGroups"
                      :key="group.label"
                      :label="group.label"
                    >
                      <a-select-option
                        v-for="preset in group.items"
                        :key="preset.id"
                        :value="preset.id"
                      >
                        {{ preset.key }}: {{ preset.value }}
                      </a-select-option>
                    </a-select-opt-group>
                  </a-select>
                  <span class="header-hint">选择后自动追加，已存在的同名头会覆盖值</span>
                </div>
                <KeyValueEditor v-model="form.headers" key-placeholder="Header 名" value-placeholder="Header 值" />
              </div>
            </a-tab-pane>
            <a-tab-pane key="body" tab="Body">
              <div class="tab-body">
                <div class="body-head">
                  <a-radio-group v-model:value="form.bodyType" size="small" button-style="solid">
                    <a-radio-button v-for="t in bodyTypes" :key="t.value" :value="t.value">
                      {{ t.label }}
                    </a-radio-button>
                  </a-radio-group>
                </div>
                <CodeArea
                  v-if="form.bodyType !== 'none'"
                  v-model="form.body"
                  :rows="10"
                  placeholder='{"name":"fink"}'
                />
                <div v-else class="empty-hint">该请求没有 Body</div>
              </div>
            </a-tab-pane>
            <a-tab-pane key="script" tab="脚本">
              <div class="tab-body">
                <div class="script-label">前置脚本（JavaScript）</div>
                <a-textarea v-model:value="form.preScript" class="mono" :rows="8" placeholder="// 修改 request.headers / request.body" />
                <div class="script-label">后置脚本</div>
                <a-textarea v-model:value="form.postScript" class="mono" :rows="8" placeholder="// vars.set('token', response.json().data.token)" />
              </div>
            </a-tab-pane>
          </a-tabs>
        </div>

        <div class="res-pane fm-card">
          <div class="res-head">
            <span class="pane-title">响应</span>
            <template v-if="result">
              <span class="status-text mono" :style="{ color: statusColor }">
                {{ result.status || 'ERR' }} {{ result.statusText }}
              </span>
              <span class="meta-divider" />
              <span class="meta mono">{{ result.durationMs }} ms</span>
              <span class="meta-divider" />
              <span class="meta mono">{{ fmtSize(result.sizeBytes) }}</span>
            </template>
          </div>

          <div v-if="!result" class="res-empty">
            <div class="res-empty-icon">⇥</div>
            <div class="res-empty-title">准备就绪</div>
            <div class="res-empty-desc">点「发送」或按 Enter 查看响应结果</div>
          </div>
          <template v-else>
            <a-alert v-if="result.error" type="error" :message="result.error" show-icon style="margin: 12px 14px" />
            <a-tabs v-model:active-key="responseTab" size="small" class="tabs res-tabs">
              <a-tab-pane key="body" tab="Body">
                <div v-if="result.truncated" class="truncate-hint">响应体超过 5MB，已截断显示</div>
                <pre class="mono body-pre">{{ prettyBody }}</pre>
              </a-tab-pane>
              <a-tab-pane key="headers" tab="Headers">
                <div class="res-header-list">
                  <div v-for="h in responseHeaders" :key="h.key" class="res-header-row">
                    <span class="mono hk">{{ h.key }}</span>
                    <span class="mono hv">{{ h.value }}</span>
                  </div>
                </div>
              </a-tab-pane>
            </a-tabs>
          </template>
        </div>
      </div>

      <section class="runs-panel" :class="{ collapsed: !runsOpen }">
        <div class="runs-head">
          <button class="runs-toggle" @click="runsOpen = !runsOpen">
            <span class="run-caret" :class="{ open: runsOpen }">›</span>
            运行历史
            <span class="runs-count">{{ runs.length }}</span>
          </button>
          <span class="runs-hint">每次发送自动记录请求参数、响应与耗时，该接口保留最近 50 条</span>
          <div class="runs-actions">
            <a-spin v-if="runsLoading" size="small" />
            <a v-if="runs.length" class="runs-clear" @click="clearRuns">清空</a>
          </div>
        </div>

        <div v-if="runsOpen" class="runs-body">
          <div v-if="!runs.length" class="runs-empty">
            还没有运行记录，发送一次请求后会自动记录在这里
          </div>
          <template v-else>
            <div class="run-row run-row-head">
              <span>时间</span>
              <span>方法</span>
              <span>状态</span>
              <span>耗时</span>
              <span>大小</span>
              <span>URL</span>
              <span />
            </div>
            <template v-for="run in runs" :key="run.id">
              <div
                class="run-row"
                :class="{ active: expandedRunId === run.id }"
                @click="toggleRun(run)"
              >
                <span class="run-time mono">{{ fmtRunTime(run.createdAt) }}</span>
                <span class="run-method mono">{{ run.method }}</span>
                <span class="run-status mono" :style="{ color: runStatusColor(run.responseStatus) }">
                  {{ run.responseStatus || 'ERR' }}
                </span>
                <span class="run-dur mono">{{ run.durationMs }} ms</span>
                <span class="run-size mono">{{ fmtSize(run.sizeBytes) }}</span>
                <span class="run-url mono" :title="run.url">{{ run.url }}</span>
                <span class="run-caret" :class="{ open: expandedRunId === run.id }">›</span>
              </div>

              <div v-if="expandedRunId === run.id" class="run-detail">
                <div class="run-col">
                  <div class="run-col-title">请求参数</div>
                  <div class="run-line">
                    <span class="rk">URL</span>
                    <span class="rv mono">{{ run.url }}</span>
                  </div>
                  <div v-for="p in run.params.filter((i) => i.enabled)" :key="`p-${p.key}-${p.value}`" class="run-line">
                    <span class="rk">{{ p.key }}</span>
                    <span class="rv mono">{{ p.value }}</span>
                  </div>
                  <div v-for="h in run.headers" :key="`h-${h.key}-${h.value}`" class="run-line">
                    <span class="rk">{{ h.key }}</span>
                    <span class="rv mono">{{ h.value }}</span>
                  </div>
                  <div v-if="run.body" class="run-block">
                    <div class="run-col-title">请求 Body · {{ run.bodyType }}</div>
                    <pre class="mono run-pre">{{ prettyRunBody(run.body) }}</pre>
                  </div>
                  <div v-if="!run.params.length && !run.headers.length && !run.body" class="runs-empty">
                    该次请求没有额外参数
                  </div>
                </div>

                <div class="run-col">
                  <div class="run-col-title">
                    响应
                    <span class="mono run-col-status" :style="{ color: runStatusColor(run.responseStatus) }">
                      {{ run.responseStatus || 'ERR' }} {{ run.responseStatusText }}
                    </span>
                    <span class="run-col-meta mono">{{ run.durationMs }} ms · {{ fmtSize(run.sizeBytes) }}</span>
                  </div>
                  <div v-if="run.error" class="run-error">{{ run.error }}</div>
                  <template v-else>
                    <div v-for="[k, v] in runHeaders(run)" :key="k" class="run-line">
                      <span class="rk">{{ k }}</span>
                      <span class="rv mono">{{ v }}</span>
                    </div>
                    <pre v-if="run.responseBody" class="mono run-pre">{{ prettyRunBody(run.responseBody) }}</pre>
                    <div v-else class="runs-empty">响应体为空</div>
                  </template>
                </div>
              </div>
            </template>
          </template>
        </div>
      </section>
    </div>
  </a-spin>
</template>

<style scoped>
.editor-page {
  height: 100%;
  display: flex;
  flex-direction: column;
}

:deep(.ant-spin-nested-loading),
:deep(.ant-spin-container) {
  height: 100%;
}

.topbar {
  height: 46px;
  padding: 0 14px;
  background: var(--fm-surface);
  border-bottom: 1px solid var(--fm-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.left,
.right {
  display: flex;
  align-items: center;
  gap: 10px;
}

.back-btn {
  color: var(--fm-text-2);
}

.divider {
  width: 1px;
  height: 16px;
  background: var(--fm-border-strong);
}

.name-input {
  width: 250px;
  font-weight: 600;
  border-color: transparent;
  background: transparent;
}

.name-input:hover {
  border-color: var(--fm-border);
}

.saved-hint {
  font-size: 10px;
  color: var(--fm-muted);
}

.insecure {
  font-size: 12px;
  color: var(--fm-muted);
}

.stress-btn {
  color: var(--fm-warning);
  border-color: var(--fm-border-strong);
  background: var(--fm-warning-soft);
}

.stress-btn:hover {
  color: var(--fm-warning);
  border-color: var(--fm-warning);
  background: var(--fm-warning-soft);
}

.url-bar {
  display: flex;
  gap: 8px;
  padding: 8px 14px;
  background: var(--fm-surface);
  border-bottom: 1px solid var(--fm-border);
}

.method-select {
  width: 108px;
  flex-shrink: 0;
  font-weight: 700;
}

.method-select.m-GET :deep(.ant-select-selection-item) {
  color: var(--fm-success);
}
.method-select.m-POST :deep(.ant-select-selection-item) {
  color: var(--fm-warning);
}
.method-select.m-PUT :deep(.ant-select-selection-item) {
  color: var(--fm-primary);
}
.method-select.m-PATCH :deep(.ant-select-selection-item) {
  color: var(--fm-purple);
}
.method-select.m-DELETE :deep(.ant-select-selection-item) {
  color: var(--fm-danger);
}

.url-input {
  flex: 1;
  font-size: 13px;
}

.env-select {
  width: 170px;
  flex-shrink: 0;
}

.panes {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px 10px;
  overflow: hidden;
}

.req-pane,
.res-pane {
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 6px;
  box-shadow: none;
}

.req-pane {
  flex: 0 0 auto;
  height: 42%;
  min-height: 222px;
}

.res-pane {
  flex: 1;
  min-height: 190px;
}

.pane-head {
  height: 36px;
  padding: 0 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.pane-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.pane-meta {
  font-size: 10px;
  color: var(--fm-muted);
}

.tabs {
  padding: 0 12px;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tabs :deep(.ant-tabs-nav) {
  margin-bottom: 6px;
}

.tabs :deep(.ant-tabs-content-holder) {
  overflow: auto;
}

.header-tools {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.preset-select {
  width: 280px;
}

.header-hint {
  font-size: 10px;
  color: var(--fm-muted);
}

.tab-body {
  padding: 4px 0 8px;
}

.body-head {
  margin: 6px 0 10px;
}

.body-input {
  font-size: 12px;
  line-height: 1.7;
}

.script-label {
  font-size: 11px;
  color: var(--fm-muted);
  margin: 10px 0 6px;
}

.res-head {
  height: 36px;
  padding: 0 12px;
  display: flex;
  align-items: center;
  gap: 10px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.status-text {
  font-size: 12px;
  font-weight: 700;
}

.meta-divider {
  width: 1px;
  height: 12px;
  background: var(--fm-border-strong);
}

.meta {
  font-size: 11px;
  color: var(--fm-muted);
}

.res-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 20px;
}

.res-empty-icon {
  width: 48px;
  height: 48px;
  border-radius: 14px;
  background: var(--fm-surface-alt);
  color: #c2ccdb;
  font-size: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 12px;
}

.res-empty-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.res-empty-desc {
  margin-top: 4px;
  font-size: 11px;
  color: var(--fm-muted);
}

.res-tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.body-pre {
  margin: 0;
  padding: 10px 14px;
  font-size: 12px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-all;
  overflow: auto;
  color: var(--fm-text-2);
}

.truncate-hint {
  padding: 6px 14px;
  font-size: 11px;
  color: var(--fm-warning);
}

.res-header-list {
  padding: 4px 0;
  overflow: auto;
}

.res-header-row {
  display: flex;
  gap: 14px;
  padding: 6px 14px;
  border-bottom: 1px dashed #f1f4f9;
  font-size: 11px;
}

.hk {
  width: 200px;
  flex-shrink: 0;
  color: var(--fm-text-2);
}

.hv {
  flex: 1;
  word-break: break-all;
  color: var(--fm-muted);
}

/* ---------- 运行历史 ---------- */
.runs-panel {
  flex: 0 0 auto;
  max-height: 42%;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
  border-top: 1px solid var(--fm-border);
}

.runs-panel.collapsed {
  max-height: none;
}

.runs-head {
  height: 34px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.runs-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: none;
  background: transparent;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  cursor: pointer;
}

.runs-count {
  min-width: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: var(--fm-border);
  color: var(--fm-text-2);
  font-size: 10px;
  line-height: 16px;
  text-align: center;
}

.runs-hint {
  flex: 1;
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.runs-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.runs-clear {
  font-size: 11px;
  color: var(--fm-danger);
}

.runs-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.runs-empty {
  padding: 14px;
  font-size: 11px;
  color: var(--fm-muted);
}

.run-row {
  display: grid;
  grid-template-columns: 132px 58px 52px 66px 64px minmax(0, 1fr) 18px;
  align-items: center;
  gap: 8px;
  padding: 5px 14px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11px;
  color: var(--fm-text-2);
  cursor: pointer;
}

.run-row:hover {
  background: var(--fm-surface-alt);
}

.run-row.active {
  background: var(--fm-primary-soft);
}

.run-row-head {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--fm-surface);
  color: var(--fm-muted);
  font-size: 10px;
  cursor: default;
  border-bottom: 1px solid var(--fm-border);
}

.run-row-head:hover {
  background: var(--fm-surface);
}

.run-time,
.run-dur,
.run-size {
  color: var(--fm-muted);
}

.run-method {
  font-weight: 700;
  color: var(--fm-primary);
}

.run-status {
  font-weight: 700;
}

.run-url {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-2);
}

.run-caret {
  color: var(--fm-muted);
  transition: transform 0.15s ease;
  display: inline-block;
}

.run-caret.open {
  transform: rotate(90deg);
}

.run-detail {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 14px;
  padding: 10px 14px 14px;
  background: var(--fm-code-bg);
  border-bottom: 1px solid var(--fm-border);
}

.run-col {
  min-width: 0;
}

.run-col-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.run-col-status {
  font-weight: 700;
}

.run-col-meta {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  font-weight: 400;
}

.run-line {
  display: flex;
  gap: 8px;
  padding: 3px 0;
  font-size: 11px;
}

.rk {
  width: 132px;
  flex-shrink: 0;
  color: var(--fm-muted);
  word-break: break-all;
}

.rv {
  flex: 1;
  color: var(--fm-text-2);
  word-break: break-all;
}

.run-block {
  margin-top: 6px;
}

.run-pre {
  margin: 0;
  padding: 8px 10px;
  max-height: 200px;
  overflow: auto;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

.run-error {
  padding: 6px 8px;
  background: var(--fm-danger-soft);
  border-radius: 4px;
  font-size: 11px;
  color: var(--fm-danger);
  word-break: break-all;
}

@media (max-width: 1100px) {
  .runs-panel {
    max-height: 50%;
  }

  .run-row {
    grid-template-columns: 120px 54px 46px 58px minmax(0, 1fr) 16px;
  }

  .run-row > :nth-child(5) {
    display: none;
  }
}

@media (max-width: 860px) {
  .topbar {
    height: auto;
    min-height: 48px;
    padding: 6px 12px;
    flex-wrap: wrap;
    gap: 8px;
  }

  .left {
    flex: 1 1 100%;
    min-width: 0;
  }

  .name-input {
    width: 100%;
    flex: 1;
  }

  .right {
    flex-wrap: wrap;
  }

  .url-bar {
    flex-wrap: wrap;
    padding: 8px 12px;
    gap: 8px;
  }

  .method-select {
    width: 92px;
  }

  .url-input {
    flex: 1 1 100%;
    order: -1;
  }

  .env-select {
    width: 148px;
  }

  .panes {
    padding: 8px 10px;
  }

  .tabs {
    padding: 0 10px;
  }

  .preset-select {
    width: 100%;
  }

  .header-tools {
    flex-wrap: wrap;
  }

  .runs-hint {
    display: none;
  }

  .run-detail {
    grid-template-columns: minmax(0, 1fr);
  }

  .run-row {
    grid-template-columns: 108px 50px 42px minmax(0, 1fr) 16px;
    gap: 6px;
  }

  .run-row > :nth-child(4),
  .run-row > :nth-child(5) {
    display: none;
  }
}
</style>
