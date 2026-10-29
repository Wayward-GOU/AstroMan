<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import {
  CaretRightOutlined,
  DeleteOutlined,
  DownloadOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  StopOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { runnerAnalyzePrompt } from '@/utils/aiScenarios'
import * as api from '@/api'
import { useProjectStore } from '@/stores/project'
import { useErrorDialogStore } from '@/stores/errorDialog'
import { formatDateTime } from '@/utils/datetime'
import {
  clearRuntimeVars,
  renderRuntimeVars,
  runEnvScript,
  runPostScript,
  runPreScript,
  type ScriptRequest,
} from '@/utils/scriptRunner'
import type { HttpRequestDetail, HttpResult, RequestSearchHit } from '@/types/models'

const router = useRouter()
const store = useProjectStore()
const errorDialog = useErrorDialogStore()

const projectId = ref<number | null>(null)
const envId = ref<number | null>(null)
const candidates = ref<RequestSearchHit[]>([])
const keyword = ref('')
const selected = ref<number[]>([])
const iterations = ref(1)
const delayMs = ref(0)
const insecure = ref(false)
const loading = ref(false)
const running = ref(false)
const stopFlag = ref(false)

interface DataRow {
  [key: string]: string
}

const dataPath = ref('')
const dataRows = ref<DataRow[]>([])
const dataError = ref('')

interface RunItem {
  index: number
  iteration: number
  requestId: number
  name: string
  method: string
  url: string
  status: number
  durationMs: number
  sizeBytes: number
  passed: number
  failed: number
  error?: string
  assertions: { name: string; passed: boolean; message?: string }[]
  startedAt: number
}

const results = ref<RunItem[]>([])
const currentIndex = ref(0)
const totalTasks = ref(0)

onMounted(async () => {
  await store.loadProjects()
  if (store.projects.length) {
    projectId.value = store.projects[0].id
  }
})

watch(projectId, async (id) => {
  selected.value = []
  candidates.value = []
  envId.value = null
  if (!id) return
  loading.value = true
  try {
    await store.openProject(id)
    candidates.value = await api.requestOptions(id)
    const active = store.environments.find((env) => env.isActive)
    envId.value = active?.id ?? store.environments[0]?.id ?? null
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

const filtered = computed(() => {
  const key = keyword.value.trim().toLowerCase()
  if (!key) return candidates.value
  return candidates.value.filter(
    (item) =>
      item.name.toLowerCase().includes(key) ||
      item.url.toLowerCase().includes(key) ||
      item.collectionName.toLowerCase().includes(key),
  )
})

const selectedRequests = computed(() =>
  selected.value
    .map((id) => candidates.value.find((item) => item.id === id))
    .filter((item): item is RequestSearchHit => !!item),
)

const envVars = computed<Record<string, string>>(() => {
  const env = store.environments.find((item) => item.id === envId.value)
  if (!env) return {}
  return Object.fromEntries(env.variables.map((item) => [item.key, item.value ?? '']))
})

const envScript = computed(() => store.environments.find((item) => item.id === envId.value)?.script ?? '')

const summary = computed(() => {
  const list = results.value
  const total = list.length
  const failed = list.filter((item) => item.error || item.status >= 400 || item.failed > 0).length
  const passed = total - failed
  const assertions = list.reduce((sum, item) => sum + item.passed + item.failed, 0)
  const assertionFailed = list.reduce((sum, item) => sum + item.failed, 0)
  const avg = total ? list.reduce((sum, item) => sum + item.durationMs, 0) / total : 0
  return { total, passed, failed, assertions, assertionFailed, avg }
})

const aiAssist = useAiAssistStore()

function aiAnalyzeResults() {
  if (!results.value.length) {
    message.warning('还没有可分析的执行结果')
    return
  }
  void aiAssist.show({
    title: '批量执行结果分析',
    prompt: runnerAnalyzePrompt({ ...summary.value }, results.value),
    hint: `共 ${summary.value.total} 次执行，失败 ${summary.value.failed} 次，断言失败 ${summary.value.assertionFailed} 条`,
  })
}

const plannedTasks = computed(() => {
  const rows = dataRows.value.length
  const rounds = rows || iterations.value
  return selected.value.length * rounds
})

function toggle(id: number) {
  const index = selected.value.indexOf(id)
  if (index >= 0) selected.value.splice(index, 1)
  else selected.value.push(id)
}

function move(index: number, delta: number) {
  const target = index + delta
  if (target < 0 || target >= selected.value.length) return
  const list = [...selected.value]
  const [item] = list.splice(index, 1)
  list.splice(target, 0, item)
  selected.value = list
}

function selectAll() {
  selected.value = filtered.value.map((item) => item.id)
}

function clearAll() {
  selected.value = []
}

/* ---------- 数据文件 ---------- */
async function pickDataFile() {
  const picked = await open({
    multiple: false,
    title: '选择数据文件（CSV 或 JSON）',
    filters: [
      { name: '数据文件', extensions: ['csv', 'json'] },
    ],
  })
  if (typeof picked !== 'string') return
  try {
    const text = await api.fileReadText(picked)
    const isJson = picked.toLowerCase().endsWith('.json')
    if (isJson) {
      const parsed = JSON.parse(text)
      if (!Array.isArray(parsed) || !parsed.every((row) => row && typeof row === 'object')) {
        throw new Error('JSON 需为对象数组，例如 [{"username":"a"},{"username":"b"}]')
      }
      dataRows.value = parsed.map((row) =>
        Object.fromEntries(Object.entries(row).map(([key, value]) => [key, String(value ?? '')])),
      )
    } else {
      dataRows.value = parseCsv(text)
    }
    if (!dataRows.value.length) throw new Error('文件中没有可用数据行')
    dataPath.value = picked
    dataError.value = ''
    message.success(`已加载 ${dataRows.value.length} 行数据`)
  } catch (e) {
    dataRows.value = []
    dataPath.value = ''
    dataError.value = e instanceof Error ? e.message : String(e)
    message.error(dataError.value)
  }
}

function clearDataFile() {
  dataRows.value = []
  dataPath.value = ''
  dataError.value = ''
}

/** 解析 CSV（支持双引号包裹与转义，首行为表头） */
function parseCsv(text: string): DataRow[] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let quoted = false
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i]
    if (quoted) {
      if (char === '"') {
        if (text[i + 1] === '"') {
          cell += '"'
          i += 1
        } else {
          quoted = false
        }
      } else {
        cell += char
      }
      continue
    }
    if (char === '"') {
      quoted = true
    } else if (char === ',') {
      row.push(cell)
      cell = ''
    } else if (char === '\n') {
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
    } else if (char !== '\r') {
      cell += char
    }
  }
  if (cell || row.length) {
    row.push(cell)
    rows.push(row)
  }
  const cleaned = rows.filter((item) => item.some((value) => value.trim() !== ''))
  if (cleaned.length < 2) return []
  const header = cleaned[0].map((item) => item.trim())
  return cleaned.slice(1).map((values) => {
    const record: DataRow = {}
    header.forEach((key, index) => {
      record[key] = (values[index] ?? '').trim()
    })
    return record
  })
}

/* ---------- 执行 ---------- */
function applyData(text: string, row: DataRow | null): string {
  if (!row) return text
  return text.replace(/\{\{\s*([\w.-]+)\s*\}\}/g, (raw, key: string) =>
    Object.prototype.hasOwnProperty.call(row, key) ? row[key] : raw,
  )
}

function toScriptRequest(detail: HttpRequestDetail): ScriptRequest {
  const headers: Record<string, string> = {}
  detail.headers.filter((item) => item.enabled && item.key).forEach((item) => {
    headers[item.key] = item.value
  })
  const params: Record<string, string> = {}
  detail.params.filter((item) => item.enabled && item.key).forEach((item) => {
    params[item.key] = item.value
  })
  return {
    method: detail.method,
    url: detail.url,
    headers,
    params,
    bodyType: detail.bodyType,
    body: detail.body,
  }
}

async function start() {
  if (!selected.value.length) {
    message.warning('请先选择要执行的接口')
    return
  }
  if (plannedTasks.value > 5000) {
    message.warning('单次执行超过 5000 次，请减少接口数量或迭代次数')
    return
  }
  results.value = []
  stopFlag.value = false
  running.value = true
  currentIndex.value = 0
  totalTasks.value = plannedTasks.value
  clearRuntimeVars()

  const rounds = dataRows.value.length || iterations.value
  try {
    for (let round = 1; round <= rounds && !stopFlag.value; round += 1) {
      const row = dataRows.value.length ? dataRows.value[round - 1] : null
      for (const requestId of selected.value) {
        if (stopFlag.value) break
        currentIndex.value += 1
        await runOne(requestId, round, row)
        if (delayMs.value > 0) {
          await new Promise((resolve) => setTimeout(resolve, delayMs.value))
        }
      }
    }
    if (stopFlag.value) message.info('已停止执行')
    else message.success('批量执行完成')
  } catch (e) {
    const env = store.environments.find((item) => item.id === envId.value)
    errorDialog.show({
      title: '批量执行失败',
      error: e,
      context: [
        { label: '项目', value: store.currentProject?.name ?? '—' },
        { label: '接口数', value: `${selected.value.length} 个` },
        { label: '轮次', value: `${dataRows.value.length || iterations.value} 轮` },
        { label: '环境', value: env?.name ?? '未选择' },
        { label: '数据文件', value: dataPath.value || '未使用' },
      ],
    })
  } finally {
    running.value = false
  }
}

function stop() {
  stopFlag.value = true
}

async function runOne(requestId: number, round: number, row: DataRow | null) {
  const detail = await api.requestGet(requestId)
  if (!detail.auth) {
    detail.auth = { kind: 'none', token: '', username: '', password: '', key: '', value: '', addTo: 'header' }
  }
  let outgoing = toScriptRequest(detail)

  if (envScript.value.trim()) {
    const envResult = runEnvScript(envScript.value, outgoing, envVars.value)
    if (envResult.error) {
      pushFailure(detail, round, `环境脚本执行失败：${envResult.error}`)
      return
    }
    outgoing = envResult.request
  }
  if (detail.preScript?.trim()) {
    const preResult = runPreScript(detail.preScript, outgoing, envVars.value)
    if (preResult.error) {
      pushFailure(detail, round, `前置脚本执行失败：${preResult.error}`)
      return
    }
    outgoing = preResult.request
  }

  const render = (text: string) => applyData(renderRuntimeVars(text), row)
  let result: HttpResult
  try {
    result = await api.sendRequest({
      requestId: detail.id ?? null,
      name: detail.name,
      method: outgoing.method,
      url: render(outgoing.url),
      headers: Object.entries(outgoing.headers).map(([key, value]) => ({
        key,
        value: render(value),
        enabled: true,
      })),
      params: Object.entries(outgoing.params).map(([key, value]) => ({
        key,
        value: render(value),
        enabled: true,
      })),
      bodyType: outgoing.bodyType,
      body: render(outgoing.body),
      envId: envId.value,
      insecure: insecure.value,
      timeoutMs: 30000,
      auth: detail.auth,
      cookieEnabled: true,
    })
  } catch (e) {
    pushFailure(detail, round, String(e))
    return
  }

  const item: RunItem = {
    index: results.value.length + 1,
    iteration: round,
    requestId: detail.id ?? 0,
    name: detail.name,
    method: outgoing.method,
    url: render(outgoing.url),
    status: result.status,
    durationMs: result.durationMs,
    sizeBytes: result.sizeBytes,
    passed: 0,
    failed: 0,
    error: result.error ?? undefined,
    assertions: [],
    startedAt: Date.now(),
  }

  if (detail.postScript?.trim() && !result.error) {
    const postResult = runPostScript(detail.postScript, outgoing, result, envVars.value)
    item.assertions = postResult.assertions
    item.passed = postResult.assertions.filter((a) => a.passed).length
    item.failed = postResult.assertions.filter((a) => !a.passed).length
    if (postResult.error) item.error = `后置脚本执行失败：${postResult.error}`
  }

  results.value.push(item)
}

function pushFailure(detail: HttpRequestDetail, round: number, error: string) {
  results.value.push({
    index: results.value.length + 1,
    iteration: round,
    requestId: detail.id ?? 0,
    name: detail.name,
    method: detail.method,
    url: detail.url,
    status: 0,
    durationMs: 0,
    sizeBytes: 0,
    passed: 0,
    failed: 0,
    error,
    assertions: [],
    startedAt: Date.now(),
  })
}

async function exportResult() {
  if (!results.value.length) return
  const target = await save({
    title: '导出批量执行结果',
    defaultPath: `runner-${formatDateTime(Date.now()).replace(/[: ]/g, '-')}.json`,
    filters: [{ name: 'JSON', extensions: ['json'] }],
  })
  if (!target) return
  const payload = {
    exportedAt: formatDateTime(Date.now()),
    project: store.projects.find((item) => item.id === projectId.value)?.name ?? '',
    environment: store.environments.find((item) => item.id === envId.value)?.name ?? '',
    iterations: dataRows.value.length || iterations.value,
    dataRows: dataRows.value.length,
    summary: summary.value,
    items: results.value,
  }
  try {
    await api.fileWriteText(target, JSON.stringify(payload, null, 2))
    message.success('已导出执行结果')
  } catch (e) {
    message.error(String(e))
  }
}

function statusClass(item: RunItem) {
  if (item.error || item.status === 0 || item.status >= 400 || item.failed > 0) return 'bad'
  return 'ok'
}

function failureText(item: RunItem) {
  const names = item.assertions.filter((a) => !a.passed).map((a) => a.name)
  return names.length ? names.join('、') : '-'
}

function openRequest(id: number) {
  if (!id) return
  void router.push(`/request/${id}`)
}
</script>

<template>
  <div class="runner-page">
    <PageHeader
      eyebrow="Runner"
      title="批量运行"
      subtitle="按顺序批量执行接口，支持 CSV / JSON 数据驱动与脚本断言汇总"
    >
      <a-button :disabled="!results.length" @click="aiAnalyzeResults">
        <template #icon><AiSparkIcon /></template>
        AI 分析结果
      </a-button>
      <a-button :disabled="running" @click="router.push('/history')">查看请求历史</a-button>
    </PageHeader>

    <div class="layout">
      <div class="fm-card pane config">
        <div class="pane-head">
          <span class="pane-title">执行配置</span>
        </div>

        <div class="field">
          <span class="label">项目</span>
          <a-select v-model:value="projectId" :options="store.projects.map((p) => ({ label: p.name, value: p.id }))" placeholder="选择项目" />
        </div>
        <div class="field">
          <span class="label">环境</span>
          <a-select
            v-model:value="envId"
            allow-clear
            placeholder="不使用环境"
            :options="store.environments.map((e) => ({ label: e.name, value: e.id }))"
          />
        </div>
        <div class="field">
          <span class="label">迭代次数</span>
          <a-input-number v-model:value="iterations" :min="1" :max="1000" class="num" :disabled="dataRows.length > 0" />
          <span v-if="dataRows.length" class="hint">数据文件驱动，共 {{ dataRows.length }} 轮</span>
        </div>
        <div class="field">
          <span class="label">请求间隔</span>
          <a-input-number v-model:value="delayMs" :min="0" :max="60000" :step="100" class="num" />
          <span class="hint">毫秒</span>
        </div>
        <div class="field">
          <span class="label">数据文件</span>
          <a-button size="small" :disabled="running" @click="pickDataFile">
            <template #icon><FolderOpenOutlined /></template>
            选择 CSV / JSON
          </a-button>
          <a-button v-if="dataRows.length" size="small" type="text" danger @click="clearDataFile">清除</a-button>
        </div>
        <div v-if="dataPath" class="data-path mono">{{ dataPath }}</div>
        <div v-if="dataError" class="data-error">{{ dataError }}</div>
        <div v-else-if="dataRows.length" class="data-preview">
          已加载 {{ dataRows.length }} 行 · 字段：{{ Object.keys(dataRows[0]).join(', ') }}
        </div>
        <div class="field">
          <a-checkbox v-model:checked="insecure">忽略证书校验</a-checkbox>
        </div>

        <div class="actions">
          <a-button v-if="!running" type="primary" :disabled="!selected.length" @click="start">
            <template #icon><CaretRightOutlined /></template>
            开始执行
          </a-button>
          <a-button v-else danger @click="stop">
            <template #icon><StopOutlined /></template>
            停止
          </a-button>
          <span v-if="running" class="progress mono">{{ currentIndex }} / {{ totalTasks }}</span>
        </div>
        <div class="hint total">计划执行 {{ plannedTasks }} 次（{{ selected.length }} 个接口 × {{ dataRows.length || iterations }} 轮）</div>
      </div>

      <div class="fm-card pane picker">
        <div class="pane-head">
          <span class="pane-title">选择接口</span>
          <a-button size="small" type="link" @click="selectAll">全选</a-button>
          <a-button size="small" type="link" @click="clearAll">清空</a-button>
        </div>
        <a-input v-model:value="keyword" allow-clear placeholder="搜索接口名称 / URL / 集合" size="small" />
        <a-spin :spinning="loading">
          <div class="list">
            <div
              v-for="item in filtered"
              :key="item.id"
              class="list-item"
              :class="{ active: selected.includes(item.id) }"
              @click="toggle(item.id)"
            >
              <a-checkbox :checked="selected.includes(item.id)" @click.stop="toggle(item.id)" />
              <span class="method mono" :class="`m-${item.method.toLowerCase()}`">{{ item.method }}</span>
              <span class="name">{{ item.name }}</span>
              <span class="path mono">{{ item.collectionName }}</span>
            </div>
            <div v-if="!filtered.length" class="empty-hint">没有匹配的接口</div>
          </div>
        </a-spin>
      </div>

      <div class="fm-card pane order">
        <div class="pane-head">
          <span class="pane-title">执行顺序</span>
          <span class="hint">{{ selected.length }} 个</span>
        </div>
        <div class="order-list">
          <div v-for="(item, index) in selectedRequests" :key="item.id" class="order-item">
            <span class="index mono">{{ index + 1 }}</span>
            <span class="method mono" :class="`m-${item.method.toLowerCase()}`">{{ item.method }}</span>
            <span class="name">{{ item.name }}</span>
            <a-button size="small" type="text" @click="move(index, -1)">↑</a-button>
            <a-button size="small" type="text" @click="move(index, 1)">↓</a-button>
            <a-button size="small" type="text" danger @click="toggle(item.id)">
              <template #icon><DeleteOutlined /></template>
            </a-button>
          </div>
          <div v-if="!selectedRequests.length" class="empty-hint">从左侧选择接口后在此排序</div>
        </div>
      </div>
    </div>

    <div class="fm-card result">
      <div class="pane-head">
        <span class="pane-title">执行结果</span>
        <a-button size="small" :disabled="!results.length" @click="exportResult">
          <template #icon><DownloadOutlined /></template>
          导出 JSON
        </a-button>
        <a-button size="small" :disabled="running || !results.length" @click="results = []">
          <template #icon><ReloadOutlined /></template>
          清空
        </a-button>
      </div>

      <div class="summary">
        <div class="sum-item">
          <span class="sum-label">总次数</span>
          <span class="sum-value mono">{{ summary.total }}</span>
        </div>
        <div class="sum-item">
          <span class="sum-label">通过</span>
          <span class="sum-value mono ok">{{ summary.passed }}</span>
        </div>
        <div class="sum-item">
          <span class="sum-label">失败</span>
          <span class="sum-value mono bad">{{ summary.failed }}</span>
        </div>
        <div class="sum-item">
          <span class="sum-label">断言通过 / 失败</span>
          <span class="sum-value mono">{{ summary.assertions - summary.assertionFailed }} / {{ summary.assertionFailed }}</span>
        </div>
        <div class="sum-item">
          <span class="sum-label">平均耗时</span>
          <span class="sum-value mono">{{ summary.avg.toFixed(0) }} ms</span>
        </div>
      </div>

      <a-table
        :data-source="results"
        :pagination="{ pageSize: 20, size: 'small' }"
        size="small"
        row-key="index"
        :scroll="{ y: 320 }"
        :locale="{ emptyText: '还没有执行结果' }"
      >
        <a-table-column title="#" data-index="index" key="index" width="60" />
        <a-table-column title="轮次" data-index="iteration" key="iteration" width="70" />
        <a-table-column title="接口" key="name" ellipsis>
          <template #default="{ record }">
            <a class="link" @click="openRequest(record.requestId)">{{ record.name }}</a>
          </template>
        </a-table-column>
        <a-table-column title="方法" key="method" width="80">
          <template #default="{ record }">
            <span class="mono">{{ record.method }}</span>
          </template>
        </a-table-column>
        <a-table-column title="状态" key="status" width="90">
          <template #default="{ record }">
            <span class="status" :class="statusClass(record)">{{ record.status || 'ERR' }}</span>
          </template>
        </a-table-column>
        <a-table-column title="耗时" key="durationMs" width="100">
          <template #default="{ record }">
            <span class="mono">{{ record.durationMs }} ms</span>
          </template>
        </a-table-column>
        <a-table-column title="断言" key="assertions" width="110">
          <template #default="{ record }">
            <span class="mono">{{ record.passed }} / {{ record.passed + record.failed }}</span>
          </template>
        </a-table-column>
        <a-table-column title="说明" key="error" ellipsis>
          <template #default="{ record }">
            <span :class="{ err: record.error || failureText(record) !== '-' }">{{ record.error || failureText(record) }}</span>
          </template>
        </a-table-column>
      </a-table>
    </div>
  </div>
</template>

<style scoped>
.runner-page {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.layout {
  display: grid;
  grid-template-columns: 300px 1fr 300px;
  gap: 14px;
}

@media (max-width: 1180px) {
  .layout {
    grid-template-columns: 1fr;
  }
}

.pane {
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
}

.pane-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
}

.pane-title {
  flex: 1;
}

.field {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.field .label {
  width: 62px;
  flex-shrink: 0;
  color: var(--fm-muted);
  font-size: 11px;
}

.num {
  width: 110px;
}

.hint {
  font-size: 11px;
  color: var(--fm-muted);
}

.hint.total {
  padding-top: 2px;
}

.data-path {
  font-size: 10.5px;
  color: var(--fm-muted);
  word-break: break-all;
}

.data-preview {
  font-size: 11px;
  color: var(--fm-muted);
}

.data-error {
  font-size: 11px;
  color: var(--fm-danger);
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}

.progress {
  font-size: 11px;
  color: var(--fm-muted);
}

.list {
  margin-top: 8px;
  max-height: 360px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.list-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 6px;
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
}

.list-item:hover {
  background: var(--fm-hover, rgba(127, 127, 127, 0.08));
}

.list-item.active {
  background: var(--fm-primary-soft, rgba(64, 120, 210, 0.12));
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path {
  font-size: 10.5px;
  color: var(--fm-muted);
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.method {
  font-size: 10px;
  font-weight: 600;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-surface-2, rgba(127, 127, 127, 0.1));
}

.m-get { color: var(--fm-success); }
.m-post { color: var(--fm-warning); }
.m-put { color: #4a7bd0; }
.m-delete { color: var(--fm-danger); }

.order-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 420px;
  overflow-y: auto;
}

.order-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  padding: 4px 6px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
}

.index {
  width: 18px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.result {
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.summary {
  display: flex;
  flex-wrap: wrap;
  gap: 20px;
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--fm-surface-2, rgba(127, 127, 127, 0.06));
}

.sum-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sum-label {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.sum-value {
  font-size: 15px;
  font-weight: 600;
}

.sum-value.ok { color: var(--fm-success); }
.sum-value.bad { color: var(--fm-danger); }

.status {
  font-size: 11px;
  font-weight: 600;
}

.status.ok { color: var(--fm-success); }
.status.bad { color: var(--fm-danger); }

.link {
  color: var(--fm-primary);
}

.err {
  color: var(--fm-danger);
}

.empty-hint {
  font-size: 11.5px;
  color: var(--fm-muted);
  padding: 8px 0;
}
</style>
