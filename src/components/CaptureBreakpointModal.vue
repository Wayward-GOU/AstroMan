<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import {
  CheckOutlined,
  DeleteOutlined,
  PlusOutlined,
  SendOutlined,
  StopOutlined,
} from '@ant-design/icons-vue'
import * as api from '@/api/capture'
import type {
  CaptureHeader,
  InterceptView,
  PendingBreakpoint,
} from '@/types/capture'

const props = defineProps<{
  pending: PendingBreakpoint[]
  timeoutSeconds: number
  maxPending: number
}>()

const emit = defineEmits<{
  (e: 'resolved', view: InterceptView): void
}>()

const selectedId = ref<number | null>(null)
const busy = ref(false)

const method = ref('')
const url = ref('')
const requestHeaders = ref<CaptureHeader[]>([])
const requestBody = ref('')
const requestBodyLocked = ref(false)
const originalRequestBody = ref('')

const responseStatus = ref(200)
const responseHeaders = ref<CaptureHeader[]>([])
const responseBody = ref('')
const responseBodyLocked = ref(false)
const originalResponseBody = ref('')

const methodOptions = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']

const open = computed(() => props.pending.length > 0)
const current = computed(
  () =>
    props.pending.find((item) => item.pauseId === selectedId.value) ??
    props.pending[0] ??
    null,
)
const isRequest = computed(() => current.value?.phase === 'request')

watch(
  () => props.pending.map((item) => item.pauseId).join(','),
  () => {
    if (!props.pending.length) {
      selectedId.value = null
      return
    }
    if (!props.pending.some((item) => item.pauseId === selectedId.value)) {
      selectedId.value = props.pending[0].pauseId
    }
  },
  { immediate: true },
)

watch(
  () => current.value?.pauseId,
  () => {
    const pause = current.value
    if (!pause) return
    method.value = pause.method || 'GET'
    url.value = pause.url
    requestHeaders.value = pause.requestHeaders.map((header) => ({ ...header }))
    requestBody.value = pause.requestBody.text ?? ''
    requestBodyLocked.value = !pause.requestEditable
    originalRequestBody.value = pause.requestBody.text ?? ''

    responseStatus.value = pause.status ?? 200
    responseHeaders.value = pause.responseHeaders.map((header) => ({ ...header }))
    responseBody.value = pause.responseBody.text ?? ''
    responseBodyLocked.value = !pause.responseEditable
    originalResponseBody.value = pause.responseBody.text ?? ''
  },
  { immediate: true },
)

const methodSelectOptions = computed(() => {
  const options = new Set(methodOptions)
  if (method.value) options.add(method.value)
  return Array.from(options)
})

function formatBytes(value: number) {
  if (!value) return '0 B'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
}

function resetRequest() {
  const pause = current.value
  if (!pause) return
  method.value = pause.method
  url.value = pause.url
  requestHeaders.value = pause.requestHeaders.map((header) => ({ ...header }))
  requestBody.value = pause.requestBody.text ?? ''
}

function resetResponse() {
  const pause = current.value
  if (!pause) return
  responseStatus.value = pause.status ?? 200
  responseHeaders.value = pause.responseHeaders.map((header) => ({ ...header }))
  responseBody.value = pause.responseBody.text ?? ''
}

function addHeader(target: 'request' | 'response') {
  if (target === 'request') requestHeaders.value.push({ name: '', value: '' })
  else responseHeaders.value.push({ name: '', value: '' })
}

function removeHeader(target: 'request' | 'response', index: number) {
  if (target === 'request') requestHeaders.value.splice(index, 1)
  else responseHeaders.value.splice(index, 1)
}

function cleanHeaders(headers: CaptureHeader[]) {
  return headers
    .map((header) => ({ name: header.name.trim(), value: header.value }))
    .filter((header) => header.name)
}

async function resolve(action: 'continue' | 'abort' | 'edit') {
  const pause = current.value
  if (!pause || busy.value) return
  busy.value = true
  try {
    let request
    let response
    if (action === 'edit' && pause.phase === 'request') {
      const body =
        !requestBodyLocked.value && requestBody.value !== originalRequestBody.value
          ? requestBody.value
          : undefined
      request = {
        method: method.value.trim().toUpperCase(),
        url: url.value.trim(),
        headers: cleanHeaders(requestHeaders.value),
        body,
      }
    }
    if (action === 'edit' && pause.phase === 'response') {
      const body =
        !responseBodyLocked.value && responseBody.value !== originalResponseBody.value
          ? responseBody.value
          : undefined
      response = {
        status: responseStatus.value,
        headers: cleanHeaders(responseHeaders.value),
        body,
      }
    }
    const view = await api.captureBreakpointResolve({
      pauseId: pause.pauseId,
      action,
      request,
      response,
    })
    emit('resolved', view)
    if (action === 'abort') message.warning('已中止该流量')
  } catch (error) {
    message.error(String(error))
  } finally {
    busy.value = false
  }
}

async function releaseAll() {
  if (busy.value) return
  busy.value = true
  try {
    const view = await api.captureBreakpointReleaseAll()
    emit('resolved', view)
    message.success('已放行全部挂起流量')
  } catch (error) {
    message.error(String(error))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <a-modal
    :open="open"
    :width="900"
    :closable="false"
    :mask-closable="false"
    :keyboard="false"
    wrap-class-name="breakpoint-modal"
    :footer="null"
    destroy-on-close
  >
    <template #title>
      <div class="bp-title">
        <span class="bp-dot" />
        <span>断点已暂停</span>
        <a-tag color="orange">{{ pending.length }} 条挂起</a-tag>
        <span class="bp-sub">超过 {{ timeoutSeconds }} 秒未处理自动放行</span>
      </div>
    </template>

    <div v-if="current" class="bp-body">
      <div class="bp-queue">
        <button
          v-for="item in pending"
          :key="item.pauseId"
          type="button"
          class="bp-chip"
          :class="{ active: item.pauseId === current.pauseId }"
          :title="item.url"
          @click="selectedId = item.pauseId"
        >
          <span class="chip-phase" :class="item.phase">
            {{ item.phase === 'request' ? '请求' : '响应' }}
          </span>
          <span class="chip-method">{{ item.method }}</span>
          <span class="chip-url mono">{{ item.url.replace(/^https?:\/\//, '') }}</span>
        </button>
      </div>

      <div class="bp-meta">
        <a-tag :color="isRequest ? 'blue' : 'green'">
          {{ isRequest ? '请求阶段 · 改入参' : '响应阶段 · 改返参' }}
        </a-tag>
        <span class="bp-rule">{{ current.rule || '未命名规则' }}</span>
        <span class="bp-clock mono">#{{ current.pauseId }}</span>
      </div>

      <template v-if="isRequest">
        <div class="bp-line">
          <a-select
            v-model:value="method"
            size="small"
            class="bp-method"
            :options="methodSelectOptions.map((item) => ({ value: item, label: item }))"
          />
          <a-input v-model:value="url" size="small" class="mono bp-url" placeholder="https://host/path" />
        </div>

        <div class="bp-sect-head">
          <span>请求头 <em>{{ requestHeaders.length }}</em></span>
          <a-button size="small" type="link" @click="addHeader('request')">
            <PlusOutlined />
            添加
          </a-button>
        </div>
        <div class="bp-headers">
          <div v-for="(header, index) in requestHeaders" :key="index" class="bp-header-row">
            <a-input v-model:value="header.name" size="small" class="mono" placeholder="Header" />
            <a-input v-model:value="header.value" size="small" class="mono" placeholder="Value" />
            <a-button type="text" size="small" danger @click="removeHeader('request', index)">
              <DeleteOutlined />
            </a-button>
          </div>
          <p v-if="!requestHeaders.length" class="bp-empty">没有请求头</p>
        </div>

        <div class="bp-sect-head">
          <span>请求体</span>
          <em v-if="current.requestBody.size">{{ formatBytes(current.requestBody.size) }}</em>
          <span class="spacer" />
          <a-button size="small" type="text" @click="resetRequest">恢复原值</a-button>
        </div>
        <a-textarea
          v-model:value="requestBody"
          :rows="9"
          class="mono"
          :disabled="requestBodyLocked"
          placeholder="无请求体"
        />
        <p v-if="requestBodyLocked" class="bp-locked">
          原始请求体为二进制或超过缓冲上限（{{ formatBytes(current.requestBody.size) }}），
          这里只能改方法、URL 与请求头，请求体按原样发送。
        </p>
        <p v-else class="bp-hint">
          Content-Length、Connection 等逐跳头由代理重算；改完点「改包放行」才会生效。
        </p>
      </template>

      <template v-else>
        <div class="bp-line">
          <span class="bp-label">响应状态</span>
          <a-input-number
            v-model:value="responseStatus"
            :min="100"
            :max="599"
            size="small"
            class="bp-status"
          />
          <span class="mono bp-url">{{ current.url }}</span>
        </div>

        <div class="bp-sect-head">
          <span>响应头 <em>{{ responseHeaders.length }}</em></span>
          <a-button size="small" type="link" @click="addHeader('response')">
            <PlusOutlined />
            添加
          </a-button>
        </div>
        <div class="bp-headers">
          <div v-for="(header, index) in responseHeaders" :key="index" class="bp-header-row">
            <a-input v-model:value="header.name" size="small" class="mono" placeholder="Header" />
            <a-input v-model:value="header.value" size="small" class="mono" placeholder="Value" />
            <a-button type="text" size="small" danger @click="removeHeader('response', index)">
              <DeleteOutlined />
            </a-button>
          </div>
          <p v-if="!responseHeaders.length" class="bp-empty">没有响应头</p>
        </div>

        <div class="bp-sect-head">
          <span>响应体</span>
          <em v-if="current.responseBody.size">{{ formatBytes(current.responseBody.size) }}</em>
          <span class="spacer" />
          <a-button size="small" type="text" @click="resetResponse">恢复原值</a-button>
        </div>
        <a-textarea
          v-model:value="responseBody"
          :rows="9"
          class="mono"
          :disabled="responseBodyLocked"
          placeholder="无响应体"
        />
        <p v-if="responseBodyLocked" class="bp-locked">
          该响应是流式（SSE / 下载）或超过缓冲上限，只能改状态码与响应头，响应体会按原样透传。
        </p>
        <p v-else class="bp-hint">
          改完点「改包放行」才会替换响应；Content-Length 会按新正文重新计算。
        </p>
      </template>

      <div class="bp-foot">
        <a-button size="small" :loading="busy" @click="releaseAll">全部放行</a-button>
        <span class="bp-foot-hint">同时挂起上限 {{ maxPending }} 条</span>
        <span class="spacer" />
        <a-button size="small" danger :disabled="busy" @click="resolve('abort')">
          <StopOutlined />
          中止
        </a-button>
        <a-button size="small" :disabled="busy" @click="resolve('continue')">
          <CheckOutlined />
          放行
        </a-button>
        <a-button type="primary" size="small" :loading="busy" @click="resolve('edit')">
          <SendOutlined />
          改包放行
        </a-button>
      </div>
    </div>
  </a-modal>
</template>

<style scoped>
.bp-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.bp-dot {
  width: 8px;
  height: 8px;
  background: #fa8c16;
  border-radius: 50%;
  box-shadow: 0 0 0 3px rgba(250, 140, 22, 0.18);
}

.bp-sub {
  font-size: 12px;
  font-weight: 400;
  color: var(--fm-muted);
}

.bp-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.bp-queue {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 84px;
  overflow: auto;
}

.bp-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  padding: 3px 8px;
  font-size: 12px;
  background: var(--fm-bg-soft, #f6f7f9);
  border: 1px solid transparent;
  border-radius: 6px;
  cursor: pointer;
}

.bp-chip.active {
  background: #fff7e6;
  border-color: #fa8c16;
}

.chip-phase {
  padding: 0 5px;
  color: #fff;
  background: #1677ff;
  border-radius: 3px;
}

.chip-phase.response {
  background: #52c41a;
}

.chip-method {
  font-weight: 600;
}

.chip-url {
  max-width: 260px;
  overflow: hidden;
  color: var(--fm-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bp-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.bp-rule {
  color: var(--fm-text, #1f2329);
}

.bp-clock {
  margin-left: auto;
}

.bp-line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.bp-method {
  width: 104px;
}

.bp-url {
  flex: 1;
  min-width: 0;
}

.bp-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.bp-status {
  width: 110px;
}

.bp-sect-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 4px;
  font-size: 12px;
  color: var(--fm-muted);
}

.bp-sect-head em {
  font-style: normal;
  color: var(--fm-text, #1f2329);
}

.spacer {
  margin-left: auto;
}

.bp-headers {
  max-height: 140px;
  overflow: auto;
}

.bp-header-row {
  display: flex;
  gap: 8px;
  margin-bottom: 6px;
}

.bp-empty {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.bp-hint,
.bp-locked {
  margin: 0;
  padding: 6px 8px;
  font-size: 12px;
  line-height: 1.5;
  border-radius: 4px;
}

.bp-hint {
  color: var(--fm-muted);
  background: var(--fm-bg-soft, #f6f7f9);
}

.bp-locked {
  color: #ad6800;
  background: #fffbe6;
}

.bp-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 10px;
  margin-top: 4px;
  border-top: 1px solid var(--fm-border, #e5e7eb);
}

.bp-foot-hint {
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
