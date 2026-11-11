<script setup lang="ts">
import { ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import * as api from '@/api'
import type { DocsSiteStatus } from '@/types/docs'
import { formatDateTime } from '@/utils/datetime'

const props = defineProps<{
  open: boolean
  projectId: number
  projectName?: string
  environmentId?: number | null
}>()

const emit = defineEmits<{ (e: 'update:open', value: boolean): void }>()

function emptyStatus(): DocsSiteStatus {
  return {
    running: false,
    url: '',
    host: '',
    port: 0,
    lan: false,
    token: null,
    projectId: null,
    environmentId: null,
    requestCount: 0,
    pageCount: 0,
    startedAt: 0,
    warnings: [],
  }
}

const status = ref<DocsSiteStatus>(emptyStatus())
const loading = ref(false)
const starting = ref(false)
const refreshing = ref(false)
const stopping = ref(false)
const port = ref<number | null>(null)
const lan = ref(false)

watch(
  () => props.open,
  (open) => {
    if (open) void loadStatus()
  },
)

async function loadStatus() {
  loading.value = true
  try {
    status.value = await api.docsSiteStatus()
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

function link() {
  if (!status.value.url) return ''
  if (!status.value.token) return status.value.url
  return `${status.value.url}?token=${encodeURIComponent(status.value.token)}`
}

async function copy(text: string, tip = '已复制') {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success(tip)
  } catch (e) {
    message.error(`复制失败：${String(e)}`)
  }
}

async function start() {
  starting.value = true
  try {
    status.value = await api.docsSiteStart({
      projectId: props.projectId,
      environmentId: props.environmentId ?? null,
      port: port.value ?? null,
      host: lan.value ? '0.0.0.0' : '127.0.0.1',
    })
    message.success(status.value.lan ? '文档站已启动（局域网模式）' : '文档站已启动')
  } catch (e) {
    message.error(String(e))
  } finally {
    starting.value = false
  }
}

async function refresh() {
  refreshing.value = true
  try {
    status.value = await api.docsSiteRefresh()
    message.success('已按最新接口重新渲染')
  } catch (e) {
    message.error(String(e))
  } finally {
    refreshing.value = false
  }
}

async function stop() {
  stopping.value = true
  try {
    status.value = await api.docsSiteStop()
    message.success('文档站已停止')
  } catch (e) {
    message.error(String(e))
  } finally {
    stopping.value = false
  }
}

async function openBrowser() {
  const target = link()
  if (!target) return
  try {
    await api.openExternal(target)
  } catch (e) {
    message.error(String(e))
  }
}

function fmtTime(ts: number) {
  return ts ? formatDateTime(ts) : '—'
}
</script>

<template>
  <a-modal
    :open="open"
    title="只读文档站"
    width="600px"
    :footer="null"
    @cancel="emit('update:open', false)"
  >
    <a-spin :spinning="loading">
      <div v-if="!status.running" class="site-panel">
        <div class="hint">
          把「{{ projectName || '当前项目' }}」的接口文档起成本机 HTTP 服务，同事用浏览器打开链接即可查看，内容只读。
        </div>
        <a-form layout="vertical" class="site-form">
          <a-form-item label="监听端口">
            <a-input-number
              v-model:value="port"
              size="small"
              :min="1"
              :max="65535"
              placeholder="自动分配"
              style="width: 160px"
            />
            <span class="tip">留空由系统挑选空闲端口</span>
          </a-form-item>
          <a-form-item label="局域网访问">
            <a-switch v-model:checked="lan" size="small" />
            <span class="tip">绑定 0.0.0.0 并生成访问令牌，仅限内网使用，用完即关</span>
          </a-form-item>
        </a-form>
        <a-alert
          type="info"
          show-icon
          :message="
            lan
              ? '局域网模式会生成一次性令牌，首次访问带 ?token= 会写入 HttpOnly Cookie'
              : '未开启局域网时仅本机 127.0.0.1 可访问'
          "
        />
        <div class="actions">
          <a-button type="primary" size="small" :loading="starting" @click="start">启动文档站</a-button>
        </div>
      </div>

      <div v-else class="site-panel">
        <a-descriptions :column="2" size="small" bordered>
          <a-descriptions-item label="访问地址" :span="2">
            <div class="url-row">
              <span class="url mono" :title="status.url">{{ status.url }}</span>
              <a-button type="link" size="small" @click="copy(status.url)">复制</a-button>
              <a-button type="link" size="small" @click="openBrowser">浏览器打开</a-button>
            </div>
          </a-descriptions-item>
          <a-descriptions-item label="项目">
            {{ status.projectId === projectId ? projectName || '当前项目' : `项目 #${status.projectId}` }}
          </a-descriptions-item>
          <a-descriptions-item label="访问范围">
            {{ status.lan ? '局域网（需令牌）' : '仅本机' }}
          </a-descriptions-item>
          <a-descriptions-item label="接口数">{{ status.requestCount }}</a-descriptions-item>
          <a-descriptions-item label="页面数">{{ status.pageCount }}</a-descriptions-item>
          <a-descriptions-item label="启动时间" :span="2">{{ fmtTime(status.startedAt) }}</a-descriptions-item>
        </a-descriptions>

        <div v-if="status.token" class="token-box">
          <div class="token-label">访问令牌</div>
          <div class="token-row">
            <code class="token mono">{{ status.token }}</code>
            <a-button type="link" size="small" @click="copy(link(), '已复制带令牌链接')">
              复制带令牌链接
            </a-button>
          </div>
          <div class="tip">令牌仅用于内网联调，会出现在访问日志里，用完即关。</div>
        </div>

        <a-alert
          v-if="status.warnings.length"
          type="warning"
          show-icon
          :message="status.warnings.join('；')"
          class="warn"
        />

        <div class="actions">
          <a-button size="small" :loading="refreshing" @click="refresh">刷新内容</a-button>
          <a-button size="small" :loading="starting" @click="start">重启为当前项目</a-button>
          <a-button size="small" danger :loading="stopping" @click="stop">停止服务</a-button>
        </div>
        <div class="tip foot">
          刷新只重新渲染页面，端口不变；关闭弹窗不会停止服务，退出应用前请手动停止。
        </div>
      </div>
    </a-spin>
  </a-modal>
</template>

<style scoped>
.site-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.hint {
  color: #646a73;
  line-height: 1.6;
}

.site-form {
  margin-top: 4px;
}

.site-form :deep(.ant-form-item) {
  margin-bottom: 12px;
}

.tip {
  margin-left: 8px;
  color: #8a8f99;
  font-size: 12px;
}

.url-row,
.token-row {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.url {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mono {
  font-family: Consolas, 'Courier New', monospace;
}

.token-box {
  border: 1px solid #e5e6eb;
  border-radius: 6px;
  padding: 8px 10px;
  background: #f7f8fa;
}

.token-label {
  font-weight: 600;
  margin-bottom: 4px;
}

.token {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: block;
}

.warn {
  margin-top: 2px;
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.foot {
  margin-left: 0;
}
</style>
