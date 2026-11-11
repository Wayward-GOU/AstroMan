<script setup lang="ts">
//! W2 G-04：项目本地 Mock Server 面板
//!
//! 路由表与运行状态分离：未启动也能编辑 / 生成路由；启动后轮询状态与请求日志。
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { message, Modal } from 'ant-design-vue'
import {
  CaretRightOutlined,
  CopyOutlined,
  DeleteOutlined,
  EditOutlined,
  PauseCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import * as api from '@/api'
import type { MockLog, MockRoute, MockRouteInput, MockStatus } from '@/types/models'

const props = defineProps<{
  open: boolean
  projectId: number
  projectName?: string
}>()

const emit = defineEmits<{ (e: 'update:open', value: boolean): void }>()

const visible = computed({
  get: () => props.open,
  set: (value: boolean) => emit('update:open', value),
})

const routes = ref<MockRoute[]>([])
const status = ref<MockStatus | null>(null)
const logs = ref<MockLog[]>([])
const loading = ref(false)
const busy = ref(false)
const port = ref<number>(0)
const host = ref('127.0.0.1')

const editorOpen = ref(false)
const form = reactive({
  id: 0,
  name: '',
  method: 'GET',
  path: '',
  statusCode: 200,
  bodyType: 'auto',
  body: '',
  delayMs: 0,
  enabled: true,
})

const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const bodyTypes = [
  { value: 'auto', label: 'auto（回放运行记录 / 实体骨架）' },
  { value: 'json', label: 'JSON' },
  { value: 'text', label: '文本' },
  { value: 'html', label: 'HTML' },
  { value: 'xml', label: 'XML' },
  { value: 'raw', label: 'Raw' },
]
const running = computed(() => !!status.value?.running)

let timer: number | undefined

async function refresh() {
  if (!visible.value || !props.projectId) return
  try {
    const [list, current] = await Promise.all([
      api.mockRoutesList(props.projectId),
      api.mockStatus(props.projectId),
    ])
    routes.value = list
    status.value = current
    if (current.running) {
      logs.value = await api.mockLogs(props.projectId)
    }
  } catch (e) {
    message.error(String(e))
  }
}

async function loadAll() {
  loading.value = true
  await refresh()
  loading.value = false
}

async function start() {
  busy.value = true
  try {
    status.value = await api.mockStart({
      projectId: props.projectId,
      port: port.value || 0,
      host: host.value,
    })
    message.success(`Mock Server 已启动：${status.value.url}`)
    await refresh()
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

async function stop() {
  busy.value = true
  try {
    status.value = await api.mockStop(props.projectId)
    message.success('已停止 Mock Server')
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

async function generate() {
  busy.value = true
  try {
    routes.value = await api.mockRoutesGenerate(props.projectId)
    message.success(`已生成 ${routes.value.length} 条路由（不会覆盖手工编辑）`)
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

function clearRoutes() {
  Modal.confirm({
    title: '清空 Mock 路由',
    content: '将删除当前项目的全部 Mock 路由，接口本身不受影响。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        const removed = await api.mockRoutesClear(props.projectId)
        message.success(`已删除 ${removed} 条路由`)
        await refresh()
      } catch (e) {
        message.error(String(e))
      }
    },
  })
}

function openCreate() {
  form.id = 0
  form.name = ''
  form.method = 'GET'
  form.path = ''
  form.statusCode = 200
  form.bodyType = 'auto'
  form.body = ''
  form.delayMs = 0
  form.enabled = true
  editorOpen.value = true
}

function openEdit(route: MockRoute) {
  form.id = route.id
  form.name = route.name
  form.method = route.method
  form.path = route.path
  form.statusCode = route.statusCode
  form.bodyType = route.bodyType
  form.body = route.body
  form.delayMs = route.delayMs
  form.enabled = route.enabled
  editorOpen.value = true
}

async function saveRoute() {
  if (!form.path.trim()) {
    message.warning('请填写路径，例如 /api/users/{id}')
    return
  }
  busy.value = true
  try {
    const existing = routes.value.find((item) => item.id === form.id)
    const payload: MockRouteInput = {
      ...existing,
      id: form.id || undefined,
      projectId: props.projectId,
      requestId: existing?.requestId ?? null,
      name: form.name.trim() || `${form.method} ${form.path.trim()}`,
      method: form.method,
      path: form.path.trim(),
      statusCode: form.statusCode,
      headers: existing?.headers ?? [],
      bodyType: form.bodyType,
      body: form.body,
      delayMs: form.delayMs,
      enabled: form.enabled,
      sortOrder: existing?.sortOrder ?? 0,
      updatedAt: existing?.updatedAt ?? 0,
    }
    await api.mockRouteSave(payload)
    message.success('路由已保存')
    editorOpen.value = false
    await refresh()
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

function removeRoute(route: MockRoute) {
  Modal.confirm({
    title: '删除路由',
    content: `确定删除 ${route.method} ${route.path}？`,
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        await api.mockRouteDelete(route.id)
        await refresh()
      } catch (e) {
        message.error(String(e))
      }
    },
  })
}

async function clearLogs() {
  try {
    await api.mockLogsClear(props.projectId)
    logs.value = []
  } catch (e) {
    message.error(String(e))
  }
}

async function copy(value?: string | null) {
  if (!value) return
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function fmtTime(ts: number) {
  return new Date(ts).toLocaleTimeString()
}

onMounted(() => {
  void loadAll()
  timer = window.setInterval(() => {
    if (visible.value) void refresh()
  }, 2500)
})

onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})
</script>

<template>
  <a-modal
    v-model:open="visible"
    :title="`Mock Server · ${projectName || '当前项目'}`"
    width="980px"
    :footer="null"
    destroy-on-close
  >
    <a-spin :spinning="loading">
      <div class="toolbar">
        <div class="run-box">
          <template v-if="running">
            <span class="dot on" />
            <a class="url mono" @click="copy(status?.url)">{{ status?.url }}</a>
            <span class="muted">{{ status?.routeCount }} 条路由 · {{ status?.requests }} 次请求</span>
            <a-tag v-if="status?.lan" color="orange">局域网</a-tag>
            <a-button size="small" :loading="busy" @click="stop">
              <template #icon><PauseCircleOutlined /></template>
              停止
            </a-button>
          </template>
          <template v-else>
            <span class="dot off" />
            <span class="muted">未启动</span>
            <a-input-number v-model:value="port" size="small" :min="0" :max="65535" style="width: 100px" />
            <span class="muted">0 = 自动端口</span>
            <a-select v-model:value="host" size="small" style="width: 150px">
              <a-select-option value="127.0.0.1">仅本机</a-select-option>
              <a-select-option value="0.0.0.0">局域网可访问</a-select-option>
            </a-select>
            <a-button size="small" type="primary" :loading="busy" @click="start">
              <template #icon><CaretRightOutlined /></template>
              启动
            </a-button>
          </template>
          <a-tooltip v-if="status?.token" title="局域网访问令牌（查询参数 / X-AstroMan-Mock-Token 头）">
            <a-button size="small" @click="copy(status?.token)">
              <template #icon><CopyOutlined /></template>
              令牌
            </a-button>
          </a-tooltip>
        </div>
        <div class="actions">
          <a-button size="small" :loading="busy" @click="generate">
            <template #icon><ThunderboltOutlined /></template>
            从项目生成
          </a-button>
          <a-button size="small" type="primary" ghost @click="openCreate">
            <template #icon><PlusOutlined /></template>
            新建路由
          </a-button>
          <a-button size="small" danger :disabled="!routes.length" @click="clearRoutes">清空</a-button>
          <a-button size="small" :loading="loading" @click="loadAll">
            <template #icon><ReloadOutlined /></template>
          </a-button>
        </div>
      </div>

      <div class="grid">
        <div class="pane">
          <div class="pane-title">路由（{{ routes.length }}）</div>
          <div class="route-list">
            <div v-for="route in routes" :key="route.id" class="route-row" :class="{ off: !route.enabled }">
              <span class="method" :class="`m-${route.method.toLowerCase()}`">{{ route.method }}</span>
              <span class="path mono">{{ route.path }}</span>
              <span class="tag">{{ route.statusCode }}</span>
              <span class="tag auto" :class="{ manual: route.bodyType !== 'auto' }">
                {{ route.bodyType }}
              </span>
              <a-tag v-if="route.conflict" color="red">冲突</a-tag>
              <span class="spacer" />
              <a-button size="small" type="text" @click="openEdit(route)">
                <template #icon><EditOutlined /></template>
              </a-button>
              <a-button size="small" type="text" danger @click="removeRoute(route)">
                <template #icon><DeleteOutlined /></template>
              </a-button>
            </div>
            <div v-if="!routes.length" class="empty">
              还没有路由。点「从项目生成」按现有接口批量补路由，或手工新建。
            </div>
          </div>
        </div>
        <div class="pane">
          <div class="pane-title">
            最近请求
            <a-button size="small" type="link" :disabled="!logs.length" @click="clearLogs">清空</a-button>
          </div>
          <div class="log-list">
            <div v-for="log in logs" :key="log.id" class="log-row">
              <span class="time">{{ fmtTime(log.at) }}</span>
              <span class="method" :class="`m-${log.method.toLowerCase()}`">{{ log.method }}</span>
              <span class="path mono">{{ log.path }}</span>
              <span class="tag" :class="{ bad: log.status >= 400 }">{{ log.status }}</span>
              <span class="muted">{{ log.durationMs }} ms</span>
              <a-tag v-if="log.generated" color="blue">生成</a-tag>
              <a-tag v-else-if="log.source === 'replay'" color="green">回放</a-tag>
              <span v-if="log.error" class="err">{{ log.error }}</span>
            </div>
            <div v-if="!logs.length" class="empty">
              {{ running ? '还没有请求。把客户端指向上面的地址试试。' : '启动后这里会显示最近的请求。' }}
            </div>
          </div>
        </div>
      </div>
    </a-spin>

    <a-modal
      v-model:open="editorOpen"
      :title="form.id ? '编辑 Mock 路由' : '新建 Mock 路由'"
      width="640px"
      :confirm-loading="busy"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveRoute"
    >
      <a-form layout="vertical">
        <div class="form-row">
          <a-form-item label="方法" style="width: 140px">
            <a-select v-model:value="form.method">
              <a-select-option v-for="item in methods" :key="item" :value="item">{{ item }}</a-select-option>
            </a-select>
          </a-form-item>
          <a-form-item label="路径" required style="flex: 1">
            <a-input v-model:value="form.path" placeholder="/api/users/{id}" />
          </a-form-item>
          <a-form-item label="状态码" style="width: 110px">
            <a-input-number v-model:value="form.statusCode" :min="100" :max="599" style="width: 100%" />
          </a-form-item>
        </div>
        <a-form-item label="名称">
          <a-input v-model:value="form.name" placeholder="留空自动用「方法 路径」" />
        </a-form-item>
        <div class="form-row">
          <a-form-item label="响应来源" style="flex: 1">
            <a-select v-model:value="form.bodyType">
              <a-select-option v-for="item in bodyTypes" :key="item.value" :value="item.value">
                {{ item.label }}
              </a-select-option>
            </a-select>
          </a-form-item>
          <a-form-item label="延迟（毫秒）" style="width: 140px">
            <a-input-number v-model:value="form.delayMs" :min="0" :max="60000" style="width: 100%" />
          </a-form-item>
          <a-form-item label="启用" style="width: 80px">
            <a-switch v-model:checked="form.enabled" />
          </a-form-item>
        </div>
        <a-form-item v-if="form.bodyType !== 'auto'" label="响应体">
          <a-textarea v-model:value="form.body" :rows="8" class="mono-input" />
        </a-form-item>
        <div v-else class="auto-hint">
          运行时优先回放该请求最近一次成功运行记录（敏感值已脱敏）；没有记录时按返回实体生成骨架。
        </div>
      </a-form>
    </a-modal>
  </a-modal>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 12px;
}
.run-box {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dot.on {
  background: #16a34a;
}
.dot.off {
  background: #9ca3af;
}
.url {
  color: #0f766e;
  cursor: pointer;
}
.actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.muted {
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
}
.grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}
.pane {
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 2px;
  min-width: 0;
}
.pane-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  font-size: 12px;
  font-weight: 600;
  border-bottom: 1px solid var(--fm-border, #e5e7eb);
  background: #fafafa;
}
.route-list,
.log-list {
  max-height: 52vh;
  overflow-y: auto;
  padding: 6px;
}
.route-row,
.log-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 6px;
  border-bottom: 1px dashed #f0f1f3;
  font-size: 12px;
}
.route-row.off {
  opacity: 0.5;
}
.method {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-weight: 600;
  font-size: 11px;
  padding: 0 4px;
  border-radius: 2px;
  background: #f3f4f6;
  flex-shrink: 0;
}
.m-get { color: #15803d; }
.m-post { color: #1d4ed8; }
.m-put, .m-patch { color: #b45309; }
.m-delete { color: #b91c1c; }
.path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
.tag {
  font-size: 11px;
  padding: 0 5px;
  border: 1px solid #e5e7eb;
  border-radius: 2px;
  color: #6b7280;
  flex-shrink: 0;
}
.tag.auto {
  color: #6b7280;
}
.tag.manual {
  color: #0f766e;
  border-color: #99f6e4;
}
.tag.bad {
  color: #b91c1c;
  border-color: #fecaca;
}
.spacer {
  flex: 1;
}
.empty {
  padding: 24px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
}
.time {
  color: #9ca3af;
  flex-shrink: 0;
}
.err {
  color: #b91c1c;
  font-size: 11px;
}
.form-row {
  display: flex;
  gap: 10px;
}
.form :deep(.ant-form-item),
.form-row :deep(.ant-form-item) {
  margin-bottom: 12px;
}
.mono-input :deep(textarea) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
.auto-hint {
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.6;
  color: #0f766e;
  background: #f0fdfa;
  border: 1px solid #99f6e4;
  border-radius: 2px;
}
</style>
