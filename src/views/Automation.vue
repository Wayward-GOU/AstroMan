<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { Modal, message } from 'ant-design-vue'
import {
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  StopOutlined,
  FileTextOutlined,
  DownloadOutlined,
  UploadOutlined,
} from '@ant-design/icons-vue'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useRouter } from 'vue-router'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import type {
  Environment,
  PlanSummary,
  PreflightReport,
  Project,
  RequestSearchHit,
  RunCase,
  RunDetail,
  SuiteDetail,
  TestArtifact,
  TestCase,
  TestDataset,
  TestRun,
  TestSuiteSummary,
} from '@/types/models'
import { formatDateTime } from '@/utils/datetime'

const projects = ref<Project[]>([])
const projectId = ref<number | null>(null)
const suites = ref<TestSuiteSummary[]>([])
const keyword = ref('')
const suite = ref<SuiteDetail | null>(null)
const envs = ref<Environment[]>([])
const requestOptions = ref<RequestSearchHit[]>([])
const tab = ref<'cases' | 'runs' | 'datasets' | 'settings'>('cases')
const loading = ref(false)
const router = useRouter()

function openStressReport(reportId?: number) {
  if (!reportId) return
  router.push(`/reports/${reportId}`)
}

const envName = computed(() => {
  const id = suite.value?.suite.envId
  return envs.value.find((item) => item.id === id)?.name ?? '默认环境'
})

/* ---------- 套件 ---------- */
async function loadProjects() {
  projects.value = await api.projectList()
  if (!projectId.value && projects.value.length) {
    projectId.value = projects.value[0].id
  }
}

async function loadSuites() {
  if (!projectId.value) return
  suites.value = await api.testSuiteList(projectId.value, keyword.value.trim() || undefined)
  if (suite.value && !suites.value.some((item) => item.id === suite.value?.suite.id)) {
    suite.value = null
  }
}

async function loadContext() {
  if (!projectId.value) return
  envs.value = await api.envList(projectId.value)
  requestOptions.value = await api.requestOptions(projectId.value)
}

async function selectSuite(id: number) {
  loading.value = true
  try {
    suite.value = await api.testSuiteGet(id)
    await loadRuns(id)
  } finally {
    loading.value = false
  }
}

const suiteModal = reactive({ open: false, name: '', envId: undefined as number | undefined })
function openSuiteModal() {
  suiteModal.open = true
  suiteModal.name = ''
  suiteModal.envId = undefined
}
async function saveSuiteModal() {
  if (!projectId.value || !suiteModal.name.trim()) {
    message.warning('请填写套件名称')
    return
  }
  const id = await api.testSuiteSave({
    projectId: projectId.value,
    name: suiteModal.name.trim(),
    envId: suiteModal.envId,
  })
  suiteModal.open = false
  await loadSuites()
  await selectSuite(id)
  message.success('套件已创建')
}

async function exportSuitePack() {
  if (!suite.value) return
  const path = await saveDialog({
    title: '导出测试套件包',
    defaultPath: `${suite.value.suite.name}.astromantest.json`,
    filters: [{ name: 'AstroMan 测试套件', extensions: ['json'] }],
  })
  if (!path) return
  try {
    await api.testSuiteExport(suite.value.suite.id, path)
    message.success('套件包已导出')
  } catch (error) {
    message.error(String(error))
  }
}

async function importSuitePack() {
  if (!projectId.value) {
    message.warning('先选择项目')
    return
  }
  const path = await openDialog({
    multiple: false,
    title: '导入测试套件包',
    filters: [{ name: 'AstroMan 测试套件', extensions: ['json'] }],
  })
  if (!path || Array.isArray(path)) return
  try {
    const id = await api.testSuiteImport(path, projectId.value, 'rename')
    await loadSuites()
    await selectSuite(id)
    message.success('套件包已导入')
  } catch (error) {
    message.error(String(error))
  }
}

function deleteSuite(item: TestSuiteSummary) {
  Modal.confirm({
    title: `删除套件「${item.name}」？`,
    content: '套件下的用例、数据集与运行记录会一起删除，且不可恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.testSuiteDelete(item.id)
      if (suite.value?.suite.id === item.id) suite.value = null
      await loadSuites()
      message.success('已删除')
    },
  })
}

/* ---------- 用例 ---------- */
const caseColumns = [
  { title: '用例', dataIndex: 'name', key: 'name', ellipsis: true },
  { title: '类型', dataIndex: 'kind', key: 'kind', width: 90 },
  { title: '标签', dataIndex: 'tags', key: 'tags', width: 120, ellipsis: true },
  { title: '依赖', dataIndex: 'dependsOn', key: 'dependsOn', width: 100, ellipsis: true },
  { title: '状态', dataIndex: 'enabled', key: 'enabled', width: 80 },
  { title: '操作', key: 'actions', width: 120 },
]
const caseEditor = reactive({
  open: false,
  saving: false,
  form: {
    id: undefined as number | undefined,
    name: '',
    kind: 'request',
    requestId: undefined as number | undefined,
    datasetId: undefined as number | undefined,
    dependsOn: '',
    tags: '',
    enabled: true,
    timeoutMs: 30000,
    retries: 0,
    retryOnFailure: false,
    continueOnFailure: false,
    variablesJson: '',
    assertionsJson: '',
    preScript: '',
    postScript: '',
    mainScript: '',
    stressJson: '',
    planId: undefined as number | undefined,
  },
})

const assertionHint =
  '[{"type":"status","op":"in","value":[200,201]},\n {"type":"jsonPath","path":"$.code","op":"eq","value":0}]'

function openNewCase(kind: 'request' | 'python' | 'stress') {
  Object.assign(caseEditor.form, {
    id: undefined,
    name: '',
    kind,
    requestId: undefined,
    datasetId: undefined,
    dependsOn: '',
    tags: '',
    enabled: true,
    timeoutMs: kind === 'python' ? 60000 : 30000,
    retries: 0,
    retryOnFailure: false,
    continueOnFailure: false,
    variablesJson: '',
    assertionsJson: '',
    preScript: '',
    postScript: '',
    mainScript: kind === 'python' ? 'def main(ctx, result):\n    result.ok()\n' : '',
    stressJson: kind === 'stress' ? stressTemplate() : '',
    planId: undefined,
  })
  caseEditor.open = true
  if (kind === 'stress') void loadPlans()
}

/** 压测用例的内联配置模板：导入计划后会被覆盖 */
function stressTemplate() {
  return JSON.stringify(
    {
      name: '',
      method: 'GET',
      url: '{{baseUrl}}/api/ping',
      headers: [],
      params: [],
      bodyType: 'none',
      concurrent: 10,
      durationSec: 30,
      thinkTimeMs: 0,
      timeoutMs: 30000,
      sla: { p95Ms: 500, p99Ms: 1000, errorRate: 1, minRps: 0 },
      slaOverride: null,
      baselineReportId: null,
    },
    null,
    2,
  )
}

const plans = ref<PlanSummary[]>([])
const planOptions = computed(() => plans.value.map((item) => ({ label: item.name, value: item.id })))

async function loadPlans() {
  if (!projectId.value || plans.value.length) return
  plans.value = await api.planList(projectId.value)
}

async function importPlan(planId: number) {
  try {
    const plan = await api.planGet(planId)
    const config = (plan.config ?? {}) as Record<string, unknown>
    caseEditor.form.stressJson = JSON.stringify(
      { ...config, slaOverride: null, baselineReportId: null },
      null,
      2,
    )
    if (!caseEditor.form.name.trim()) caseEditor.form.name = `${plan.name} 压测`
    message.success('已导入压测计划配置')
  } catch (error) {
    message.error(`导入失败：${String(error)}`)
  }
}

function openEditCase(row: TestCase) {
  Object.assign(caseEditor.form, {
    id: row.id,
    name: row.name,
    kind: row.kind,
    requestId: row.requestId,
    datasetId: row.datasetId,
    dependsOn: row.dependsOn ?? '',
    tags: row.tags ?? '',
    enabled: row.enabled,
    timeoutMs: row.timeoutMs,
    retries: row.retries,
    retryOnFailure: row.retryOnFailure,
    continueOnFailure: row.continueOnFailure,
    variablesJson: row.variablesJson ?? '',
    assertionsJson: row.assertionsJson ?? '',
    preScript: row.preScript ?? '',
    postScript: row.postScript ?? '',
    mainScript: row.mainScript ?? '',
    stressJson: row.stressJson ?? '',
    planId: row.planId,
  })
  caseEditor.open = true
  if (row.kind === 'stress') void loadPlans()
}

function validJson(text: string, label: string) {
  if (!text.trim()) return true
  try {
    JSON.parse(text)
    return true
  } catch {
    message.error(`${label}不是合法 JSON`)
    return false
  }
}

async function saveCase() {
  if (!suite.value) return
  const form = caseEditor.form
  if (!form.name.trim()) {
    message.warning('请填写用例名称')
    return
  }
  if (form.kind === 'request' && !form.requestId) {
    message.warning('HTTP 用例需要绑定一个接口')
    return
  }
  if (form.kind === 'python' && !form.mainScript.trim()) {
    message.warning('Python 用例需要 mainScript')
    return
  }
  if (form.kind === 'stress') {
    if (!form.stressJson.trim()) {
      message.warning('压测用例需要内联压测配置')
      return
    }
    if (!validJson(form.stressJson, '压测配置')) return
  }
  if (!validJson(form.assertionsJson, '断言') || !validJson(form.variablesJson, '变量')) return
  caseEditor.saving = true
  try {
    await api.testCaseSave({
      id: form.id,
      suiteId: suite.value.suite.id,
      name: form.name.trim(),
      kind: form.kind,
      requestId: form.requestId,
      datasetId: form.datasetId,
      dependsOn: form.dependsOn.trim() || undefined,
      enabled: form.enabled,
      timeoutMs: form.timeoutMs,
      retries: form.retries,
      retryOnFailure: form.retryOnFailure,
      continueOnFailure: form.continueOnFailure,
      tags: form.tags.trim() || undefined,
      variablesJson: form.variablesJson.trim() || undefined,
      assertionsJson: form.assertionsJson.trim() || undefined,
      preScript: form.preScript || undefined,
      postScript: form.postScript || undefined,
      mainScript: form.mainScript || undefined,
      stressJson: form.kind === 'stress' ? form.stressJson.trim() : undefined,
      planId: form.planId,
    })
    caseEditor.open = false
    await selectSuite(suite.value.suite.id)
    await loadSuites()
    message.success('用例已保存')
  } finally {
    caseEditor.saving = false
  }
}

function deleteCase(row: TestCase) {
  Modal.confirm({
    title: `删除用例「${row.name}」？`,
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.testCaseDelete(row.id)
      if (suite.value) await selectSuite(suite.value.suite.id)
      await loadSuites()
      message.success('已删除')
    },
  })
}

/* ---------- 数据集 ---------- */
const datasetEditor = reactive({
  open: false,
  saving: false,
  form: {
    id: undefined as number | undefined,
    name: '',
    format: 'csv',
    content: '',
    path: '',
  },
})
function openNewDataset() {
  Object.assign(datasetEditor.form, { id: undefined, name: '', format: 'json', content: '', path: '' })
  datasetEditor.open = true
}
function openEditDataset(row: TestDataset) {
  Object.assign(datasetEditor.form, {
    id: row.id,
    name: row.name,
    format: row.format,
    content: row.content ?? '',
    path: row.path ?? '',
  })
  datasetEditor.open = true
}
async function saveDataset() {
  if (!suite.value) return
  if (!datasetEditor.form.name.trim()) {
    message.warning('请填写数据集名称')
    return
  }
  datasetEditor.saving = true
  try {
    await api.testDatasetSave({
      id: datasetEditor.form.id,
      suiteId: suite.value.suite.id,
      name: datasetEditor.form.name.trim(),
      format: datasetEditor.form.format,
      content: datasetEditor.form.content.trim() || undefined,
      path: datasetEditor.form.path.trim() || undefined,
    })
    datasetEditor.open = false
    await selectSuite(suite.value.suite.id)
    message.success('数据集已保存')
  } finally {
    datasetEditor.saving = false
  }
}
function deleteDataset(row: TestDataset) {
  Modal.confirm({
    title: `删除数据集「${row.name}」？`,
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.testDatasetDelete(row.id)
      if (suite.value) await selectSuite(suite.value.suite.id)
      message.success('已删除')
    },
  })
}

/* ---------- 套件设置 ---------- */
const settingsForm = reactive({
  name: '',
  envId: undefined as number | undefined,
  tags: '',
  pythonPath: '',
  configJson: '',
  setupScript: '',
  teardownScript: '',
  saving: false,
})
function fillSettings() {
  const current = suite.value?.suite
  if (!current) return
  settingsForm.name = current.name
  settingsForm.envId = current.envId
  settingsForm.tags = current.tags ?? ''
  settingsForm.pythonPath = current.pythonPath ?? ''
  settingsForm.configJson = current.configJson ?? ''
  settingsForm.setupScript = current.setupScript ?? ''
  settingsForm.teardownScript = current.teardownScript ?? ''
}
async function saveSettings() {
  if (!suite.value || !projectId.value) return
  if (!validJson(settingsForm.configJson, '套件配置')) return
  settingsForm.saving = true
  try {
    await api.testSuiteSave({
      id: suite.value.suite.id,
      projectId: projectId.value,
      name: settingsForm.name.trim(),
      envId: settingsForm.envId,
      tags: settingsForm.tags.trim() || undefined,
      pythonPath: settingsForm.pythonPath.trim() || undefined,
      configJson: settingsForm.configJson.trim() || undefined,
      setupScript: settingsForm.setupScript || undefined,
      teardownScript: settingsForm.teardownScript || undefined,
    })
    await loadSuites()
    await selectSuite(suite.value.suite.id)
    message.success('设置已保存')
  } finally {
    settingsForm.saving = false
  }
}

/* ---------- 运行 ---------- */
const runEnvId = ref<number | undefined>(undefined)
const preflightOpen = ref(false)
const preflight = ref<PreflightReport | null>(null)
const runDrawer = reactive({ open: false, runId: 0, detail: null as RunDetail | null })
const running = computed(() =>
  ['queued', 'running'].includes(runDrawer.detail?.run.status ?? ''),
)
const progress = computed(() => {
  const run = runDrawer.detail?.run
  if (!run || run.total <= 0) return 0
  const done = run.passed + run.failed + run.skipped + run.errored
  return Math.min(100, Math.round((done / run.total) * 100))
})
const activeCase = ref<RunCase | null>(null)
let pollTimer: number | null = null

async function startRun() {
  if (!suite.value) return
  const envId = runEnvId.value ?? suite.value.suite.envId
  try {
    const report = await api.testPreflight(suite.value.suite.id, envId)
    preflight.value = report
    if (!report.runnable) {
      preflightOpen.value = true
      return
    }
  } catch (error) {
    message.error(String(error))
    return
  }
  try {
    const runId = await api.testRunStart(suite.value.suite.id, envId)
    runDrawer.open = true
    runDrawer.runId = runId
    activeCase.value = null
    await refreshRun()
    startPolling()
    await loadRuns(suite.value.suite.id)
  } catch (error) {
    message.error(String(error))
  }
}

async function refreshRun() {
  if (!runDrawer.runId) return
  try {
    runDrawer.detail = await api.testRunGet(runDrawer.runId)
    if (!running.value) {
      stopPolling()
      if (suite.value) await loadRuns(suite.value.suite.id)
    }
  } catch {
    stopPolling()
  }
}

function startPolling() {
  stopPolling()
  pollTimer = window.setInterval(refreshRun, 800)
}
function stopPolling() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer)
    pollTimer = null
  }
}

async function stopRun() {
  if (!runDrawer.runId) return
  try {
    await api.testRunStop(runDrawer.runId)
    message.success('已发送停止信号')
    await refreshRun()
  } catch (error) {
    message.error(String(error))
  }
}

function openRun(runId: number) {
  runDrawer.open = true
  runDrawer.runId = runId
  activeCase.value = null
  refreshRun().then(() => {
    if (running.value) startPolling()
  })
}

const runColumns = [
  { title: '运行', dataIndex: 'id', key: 'id', width: 90 },
  { title: '状态', dataIndex: 'status', key: 'status', width: 100 },
  { title: '结果', key: 'counts', width: 200 },
  { title: '耗时', dataIndex: 'durationMs', key: 'durationMs', width: 100 },
  { title: '开始时间', dataIndex: 'startedAt', key: 'startedAt', width: 170 },
  { title: '操作', key: 'actions', width: 150 },
]
const runs = ref<TestRun[]>([])
const runsLoading = ref(false)
async function loadRuns(suiteId: number) {
  runsLoading.value = true
  try {
    runs.value = await api.testRunList(suiteId, 1, 50)
  } finally {
    runsLoading.value = false
  }
}

const statusColor: Record<string, string> = {
  passed: 'green',
  failed: 'red',
  error: 'orange',
  skipped: 'default',
  cancelled: 'default',
  running: 'blue',
  queued: 'blue',
}
const statusText: Record<string, string> = {
  passed: '通过',
  failed: '失败',
  error: '错误',
  skipped: '跳过',
  cancelled: '已取消',
  running: '运行中',
  queued: '排队中',
}

/* ---------- 报告与附件 ---------- */
const artifacts = ref<TestArtifact[]>([])
const reportDir = ref('')

async function generateReport() {
  if (!runDrawer.runId) return
  try {
    const report = await api.testReportGenerate(runDrawer.runId)
    reportDir.value = report.dir
    artifacts.value = await api.testArtifactList(runDrawer.runId)
    message.success('报告已生成')
  } catch (error) {
    message.error(String(error))
  }
}
function openReport() {
  if (!reportDir.value) return
  api.openExternal(`${reportDir.value}/report.html`).catch((error) => message.error(String(error)))
}
async function downloadArtifact(item: TestArtifact) {
  const path = await saveDialog({ title: '保存附件', defaultPath: item.name })
  if (!path) return
  await api.testArtifactExport(item.id, path)
  message.success('附件已保存')
}

/* ---------- 生命周期 ---------- */
watch(projectId, async () => {
  suite.value = null
  runs.value = []
  await Promise.all([loadSuites(), loadContext()])
})
watch(keyword, loadSuites)
watch(() => suite.value?.suite.id, fillSettings)

onMounted(async () => {
  try {
    await loadProjects()
    await Promise.all([loadSuites(), loadContext()])
  } catch (error) {
    message.error(String(error))
  }
})
onBeforeUnmount(stopPolling)
</script>

<template>
  <div class="automation">
    <PageHeader
      eyebrow="Automation"
      title="自动化测试"
      subtitle="套件编排 · Python 钩子 · 断言报告 · CLI / CI 同源执行"
    >
      <a-select
        v-model:value="projectId"
        :options="projects.map((item) => ({ label: item.name, value: item.id }))"
        style="width: 180px"
        placeholder="选择项目"
      />
      <a-button @click="loadSuites"><ReloadOutlined /> 刷新</a-button>
    </PageHeader>

    <div class="layout">
      <aside class="suite-pane">
        <div class="pane-head">
          <a-input v-model:value="keyword" allow-clear placeholder="搜索套件" size="small" />
          <a-button size="small" title="导入套件包" @click="importSuitePack"><UploadOutlined /></a-button>
          <a-button type="primary" size="small" @click="openSuiteModal"><PlusOutlined /></a-button>
        </div>
        <div class="suite-list">
          <div
            v-for="item in suites"
            :key="item.id"
            class="suite-item"
            :class="{ active: suite?.suite.id === item.id }"
            @click="selectSuite(item.id)"
          >
            <div class="suite-main">
              <span class="suite-name">{{ item.name }}</span>
              <span class="suite-meta">{{ item.caseCount }} 个用例 · {{ item.envName ?? '未指定环境' }}</span>
            </div>
            <a-button type="text" size="small" danger @click.stop="deleteSuite(item)">
              <DeleteOutlined />
            </a-button>
          </div>
          <a-empty v-if="!suites.length" description="还没有套件" :image-style="{ height: '40px' }" />
        </div>
      </aside>

      <main class="main-pane">
        <template v-if="suite">
          <div class="suite-bar">
            <div class="suite-title">
              <h2>{{ suite.suite.name }}</h2>
              <span class="suite-sub">{{ envName }} · {{ suite.cases.length }} 个用例</span>
            </div>
            <div class="suite-actions">
              <a-button size="small" @click="exportSuitePack"><DownloadOutlined /> 导出套件</a-button>
              <a-select
                v-model:value="runEnvId"
                allow-clear
                placeholder="选择环境"
                style="width: 160px"
                :options="envs.map((item) => ({ label: item.name, value: item.id }))"
              />
              <a-button type="primary" :loading="loading" @click="startRun">
                <PlayCircleOutlined /> 运行
              </a-button>
            </div>
          </div>

          <a-tabs v-model:activeKey="tab" size="small">
            <a-tab-pane key="cases" tab="用例">
              <div class="tab-tools">
                <a-button size="small" @click="openNewCase('request')"><PlusOutlined /> HTTP 用例</a-button>
                <a-button size="small" @click="openNewCase('python')"><PlusOutlined /> Python 用例</a-button>
                <a-button size="small" @click="openNewCase('stress')"><PlusOutlined /> 压测用例</a-button>
              </div>
              <a-table
                :columns="caseColumns"
                :data-source="suite.cases"
                row-key="id"
                size="small"
                :pagination="false"
                :scroll="{ y: 420 }"
              >
                <template #bodyCell="{ column, record }">
                  <template v-if="column.key === 'kind'">
                    <a-tag :color="record.kind === 'python' ? 'purple' : record.kind === 'stress' ? 'orange' : 'blue'">
                      {{ record.kind }}
                    </a-tag>
                  </template>
                  <template v-else-if="column.key === 'enabled'">
                    <a-tag :color="record.enabled ? 'green' : 'default'">{{ record.enabled ? '启用' : '禁用' }}</a-tag>
                  </template>
                  <template v-else-if="column.key === 'actions'">
                    <a-button type="link" size="small" @click="openEditCase(record)"><EditOutlined /></a-button>
                    <a-button type="link" size="small" danger @click="deleteCase(record)"><DeleteOutlined /></a-button>
                  </template>
                </template>
              </a-table>
            </a-tab-pane>

            <a-tab-pane key="runs" tab="运行记录">
              <a-table
                :columns="runColumns"
                :data-source="runs"
                row-key="id"
                size="small"
                :loading="runsLoading"
                :pagination="{ pageSize: 12, size: 'small' }"
              >
                <template #bodyCell="{ column, record }">
                  <template v-if="column.key === 'status'">
                    <a-tag :color="statusColor[record.status]">{{ statusText[record.status] ?? record.status }}</a-tag>
                  </template>
                  <template v-else-if="column.key === 'counts'">
                    通过 {{ record.passed }} · 失败 {{ record.failed }} · 跳过 {{ record.skipped }} · 错误 {{ record.errored }}
                  </template>
                  <template v-else-if="column.key === 'durationMs'">
                    {{ record.durationMs > 1000 ? `${(record.durationMs / 1000).toFixed(1)} s` : `${record.durationMs} ms` }}
                  </template>
                  <template v-else-if="column.key === 'startedAt'">
                    {{ formatDateTime(record.startedAt ?? record.createdAt) }}
                  </template>
                  <template v-else-if="column.key === 'actions'">
                    <a-button type="link" size="small" @click="openRun(record.id)"><FileTextOutlined /> 详情</a-button>
                  </template>
                </template>
              </a-table>
            </a-tab-pane>

            <a-tab-pane key="datasets" tab="数据集">
              <div class="tab-tools">
                <a-button size="small" @click="openNewDataset"><PlusOutlined /> 新建数据集</a-button>
              </div>
              <a-table
                :data-source="suite.datasets"
                row-key="id"
                size="small"
                :pagination="false"
                :columns="[
                  { title: '名称', dataIndex: 'name', key: 'name' },
                  { title: '格式', dataIndex: 'format', key: 'format', width: 80 },
                  { title: '行数', dataIndex: 'rowCount', key: 'rowCount', width: 80 },
                  { title: '内容', dataIndex: 'content', key: 'content', ellipsis: true },
                  { title: '操作', key: 'actions', width: 120 },
                ]"
              >
                <template #bodyCell="{ column, record }">
                  <template v-if="column.key === 'content'">
                    {{ record.content ? record.content.slice(0, 80) : record.path ?? '（点击编辑查看）' }}
                  </template>
                  <template v-else-if="column.key === 'actions'">
                    <a-button type="link" size="small" @click="openEditDataset(record)"><EditOutlined /></a-button>
                    <a-button type="link" size="small" danger @click="deleteDataset(record)"><DeleteOutlined /></a-button>
                  </template>
                </template>
              </a-table>
            </a-tab-pane>

            <a-tab-pane key="settings" tab="套件设置">
              <div class="settings-grid">
                <label>名称 <a-input v-model:value="settingsForm.name" /></label>
                <label>
                  默认环境
                  <a-select
                    v-model:value="settingsForm.envId"
                    allow-clear
                    :options="envs.map((item) => ({ label: item.name, value: item.id }))"
                  />
                </label>
                <label>标签 <a-input v-model:value="settingsForm.tags" placeholder="smoke,order" /></label>
                <label>解释器路径 <a-input v-model:value="settingsForm.pythonPath" placeholder="留空自动探测" /></label>
                <label class="wide">
                  套件配置（JSON）
                  <a-textarea v-model:value="settingsForm.configJson" :rows="4" placeholder='{"failFast":true,"maxWorkers":1,"cookieMode":"shared"}' />
                </label>
                <label class="wide">
                  setupScript（Python）
                  <a-textarea v-model:value="settingsForm.setupScript" :rows="4" />
                </label>
                <label class="wide">
                  teardownScript（Python）
                  <a-textarea v-model:value="settingsForm.teardownScript" :rows="4" />
                </label>
              </div>
              <a-button type="primary" :loading="settingsForm.saving" @click="saveSettings">保存设置</a-button>
            </a-tab-pane>
          </a-tabs>
        </template>
        <a-empty v-else description="选择左侧套件，或新建一个套件开始" class="empty" />
      </main>
    </div>

    <!-- 新建套件 -->
    <a-modal v-model:open="suiteModal.open" title="新建套件" :width="420" @ok="saveSuiteModal">
      <a-form layout="vertical">
        <a-form-item label="套件名称" required>
          <a-input v-model:value="suiteModal.name" placeholder="例如：订单回归" />
        </a-form-item>
        <a-form-item label="默认环境">
          <a-select
            v-model:value="suiteModal.envId"
            allow-clear
            :options="envs.map((item) => ({ label: item.name, value: item.id }))"
          />
        </a-form-item>
      </a-form>
    </a-modal>

    <!-- 用例编辑 -->
    <a-drawer v-model:open="caseEditor.open" :width="720" :title="caseEditor.form.id ? '编辑用例' : '新建用例'">
      <div class="editor-grid">
        <label>名称 <a-input v-model:value="caseEditor.form.name" /></label>
        <label>
          类型
          <a-select
            v-model:value="caseEditor.form.kind"
            :options="[
              { label: 'HTTP 请求', value: 'request' },
              { label: 'Python 脚本', value: 'python' },
              { label: '压测执行', value: 'stress' },
            ]"
          />
        </label>
        <div v-if="caseEditor.form.kind === 'stress'" class="wide stress-config">
          <div class="stress-config-head">
            <span>压测配置（StressConfig JSON，含 sla / slaOverride / baselineReportId）</span>
            <a-select
              :value="caseEditor.form.planId"
              allow-clear
              placeholder="从压测计划导入"
              size="small"
              style="width: 200px"
              :options="planOptions"
              @change="(value: number) => { caseEditor.form.planId = value; if (value) importPlan(value) }"
            />
          </div>
          <a-textarea
            v-model:value="caseEditor.form.stressJson"
            :rows="12"
            class="mono"
            placeholder="压测配置 JSON"
          />
          <div class="stress-hint">
            SLA 达标判定由后端完成：P95 / P99 / 错误率 / 最小 RPS 任一超标即用例失败；填
            baselineReportId 可做基线回归（P95/P99 超基线 1.2 倍、RPS 低于基线 80%、错误率高于基线 0.5% 即失败）
          </div>
        </div>
        <label v-if="caseEditor.form.kind === 'request'" class="wide">
          接口
          <a-select
            v-model:value="caseEditor.form.requestId"
            show-search
            option-filter-prop="label"
            :options="requestOptions.map((item) => ({ label: `${item.method} ${item.name}`, value: item.id }))"
          />
        </label>
        <label>
          数据集
          <a-select
            v-model:value="caseEditor.form.datasetId"
            allow-clear
            :options="(suite?.datasets ?? []).map((item) => ({ label: item.name, value: item.id }))"
          />
        </label>
        <label>依赖用例 ID <a-input v-model:value="caseEditor.form.dependsOn" placeholder="逗号分隔" /></label>
        <label>标签 <a-input v-model:value="caseEditor.form.tags" placeholder="smoke" /></label>
        <label>超时（ms） <a-input-number v-model:value="caseEditor.form.timeoutMs" :min="500" :max="600000" style="width: 100%" /></label>
        <label>重试次数 <a-input-number v-model:value="caseEditor.form.retries" :min="0" :max="5" style="width: 100%" /></label>
        <label class="row">
          <a-checkbox v-model:checked="caseEditor.form.enabled">启用</a-checkbox>
          <a-checkbox v-model:checked="caseEditor.form.retryOnFailure">失败重试</a-checkbox>
          <a-checkbox v-model:checked="caseEditor.form.continueOnFailure">失败继续</a-checkbox>
        </label>
        <label class="wide">
          断言（JSON）
          <a-textarea v-model:value="caseEditor.form.assertionsJson" :rows="4" :placeholder="assertionHint" />
        </label>
        <label class="wide">
          变量（JSON）
          <a-textarea v-model:value="caseEditor.form.variablesJson" :rows="3" placeholder='{"orderId": 42}' />
        </label>
        <label class="wide">
          前置脚本（Python）
          <a-textarea v-model:value="caseEditor.form.preScript" :rows="4" />
        </label>
        <label class="wide">
          后置脚本（Python）
          <a-textarea v-model:value="caseEditor.form.postScript" :rows="4" />
        </label>
        <label v-if="caseEditor.form.kind === 'python'" class="wide">
          mainScript（Python）
          <a-textarea v-model:value="caseEditor.form.mainScript" :rows="8" class="mono" />
        </label>
      </div>
      <div class="drawer-foot">
        <a-button @click="caseEditor.open = false">取消</a-button>
        <a-button type="primary" :loading="caseEditor.saving" @click="saveCase">保存</a-button>
      </div>
    </a-drawer>

    <!-- 数据集编辑 -->
    <a-modal v-model:open="datasetEditor.open" :width="560" :title="datasetEditor.form.id ? '编辑数据集' : '新建数据集'" @ok="saveDataset">
      <a-form layout="vertical">
        <a-form-item label="名称" required><a-input v-model:value="datasetEditor.form.name" /></a-form-item>
        <a-form-item label="格式">
          <a-select
            v-model:value="datasetEditor.form.format"
            :options="[
              { label: 'JSON', value: 'json' },
              { label: 'CSV', value: 'csv' },
            ]"
          />
        </a-form-item>
        <a-form-item label="内联内容（JSON 数组或 CSV 文本）">
          <a-textarea v-model:value="datasetEditor.form.content" :rows="8" class="mono" />
        </a-form-item>
        <a-form-item label="或文件路径（相对数据目录）">
          <a-input v-model:value="datasetEditor.form.path" />
        </a-form-item>
      </a-form>
    </a-modal>

    <!-- 预检 -->
    <a-modal v-model:open="preflightOpen" title="运行前预检未通过" :footer="null" :width="640">
      <a-alert type="error" :message="`${preflight?.errors ?? 0} 个错误，${preflight?.warnings ?? 0} 个提醒`" show-icon />
      <div class="preflight-list">
        <div v-for="(item, index) in preflight?.items ?? []" :key="index" class="preflight-item" :class="item.level">
          <a-tag :color="item.level === 'error' ? 'red' : 'orange'">{{ item.code }}</a-tag>
          <span class="preflight-case">{{ item.caseName ? `${item.caseName}：` : '' }}</span>
          <span>{{ item.message }}</span>
          <div v-if="item.hint" class="preflight-hint">{{ item.hint }}</div>
        </div>
      </div>
    </a-modal>

    <!-- 运行详情 -->
    <a-drawer v-model:open="runDrawer.open" :width="820" :title="`运行 #${runDrawer.runId}`" @close="stopPolling">
      <template v-if="runDrawer.detail">
        <div class="run-head">
          <a-tag :color="statusColor[runDrawer.detail.run.status]">
            {{ statusText[runDrawer.detail.run.status] ?? runDrawer.detail.run.status }}
          </a-tag>
          <span class="run-meta">
            通过 {{ runDrawer.detail.run.passed }} · 失败 {{ runDrawer.detail.run.failed }} · 跳过
            {{ runDrawer.detail.run.skipped }} · 错误 {{ runDrawer.detail.run.errored }} / 共
            {{ runDrawer.detail.run.total }}
          </span>
          <a-button v-if="running" danger size="small" @click="stopRun"><StopOutlined /> 停止</a-button>
          <a-button size="small" :disabled="running" @click="generateReport"><FileTextOutlined /> 生成报告</a-button>
          <a-button v-if="reportDir" size="small" @click="openReport">打开报告</a-button>
        </div>
        <a-progress :percent="progress" size="small" :status="running ? 'active' : 'normal'" />

        <a-table
          :data-source="runDrawer.detail.cases"
          row-key="id"
          size="small"
          :pagination="false"
          :scroll="{ y: 260 }"
          :columns="[
            { title: '用例', dataIndex: 'name', key: 'name', ellipsis: true },
            { title: '状态', dataIndex: 'status', key: 'status', width: 90 },
            { title: '断言', key: 'assertions', width: 90 },
            { title: '耗时', dataIndex: 'durationMs', key: 'durationMs', width: 90 },
            { title: '操作', key: 'actions', width: 80 },
          ]"
          class="run-table"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'status'">
              <a-tag :color="statusColor[record.status]">{{ statusText[record.status] ?? record.status }}</a-tag>
            </template>
            <template v-else-if="column.key === 'assertions'">
              {{ record.assertionsTotal - record.assertionsFailed }}/{{ record.assertionsTotal }}
            </template>
            <template v-else-if="column.key === 'durationMs'">{{ record.durationMs }} ms</template>
            <template v-else-if="column.key === 'actions'">
              <a-button type="link" size="small" @click="activeCase = record">日志</a-button>
            </template>
          </template>
        </a-table>

        <div v-if="activeCase" class="case-detail">
          <div class="detail-title">
            {{ activeCase.name }}
            <a-button
              v-if="activeCase.reportId"
              type="link"
              size="small"
              @click="openStressReport(activeCase.reportId)"
            >
              查看压测报告 #{{ activeCase.reportId }}
            </a-button>
          </div>
          <a-alert v-if="activeCase.error" type="error" :message="activeCase.error" show-icon />
          <div v-if="activeCase.logs" class="detail-block">
            <div class="detail-label">日志</div>
            <pre>{{ activeCase.logs }}</pre>
          </div>
          <div v-if="activeCase.responseSnapshot" class="detail-block">
            <div class="detail-label">响应</div>
            <pre>{{ activeCase.responseSnapshot }}</pre>
          </div>
          <div v-if="activeCase.requestSnapshot" class="detail-block">
            <div class="detail-label">请求</div>
            <pre>{{ activeCase.requestSnapshot }}</pre>
          </div>
        </div>

        <div v-if="artifacts.length" class="detail-block">
          <div class="detail-label">附件</div>
          <div v-for="item in artifacts" :key="item.id" class="artifact-row">
            <span>{{ item.name }}</span>
            <span class="artifact-size">{{ item.sizeBytes }} B</span>
            <a-button size="small" :disabled="!item.exists" @click="downloadArtifact(item)">
              <DownloadOutlined /> 保存
            </a-button>
          </div>
        </div>
      </template>
    </a-drawer>
  </div>
</template>

<style scoped>
.automation {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.layout {
  display: flex;
  gap: 12px;
  flex: 1;
  min-height: 0;
}
.suite-pane {
  width: 260px;
  flex: 0 0 260px;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: 0;
  min-height: 0;
}
.pane-head {
  display: flex;
  gap: 8px;
  padding: 8px;
  border-bottom: 1px solid var(--fm-border);
}
.suite-list {
  flex: 1;
  overflow: auto;
  padding: 4px;
}
.suite-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  cursor: pointer;
  border-left: 2px solid transparent;
}
.suite-item:hover {
  background: var(--fm-surface-alt);
}
.suite-item.active {
  background: var(--fm-surface-alt);
  border-left-color: var(--fm-primary);
}
.suite-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.suite-name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.suite-meta {
  font-size: 11px;
  color: var(--fm-muted);
}
.main-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  padding: 10px 12px;
  min-height: 0;
}
.suite-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--fm-border);
}
.suite-title h2 {
  margin: 0;
  font-size: 15px;
}
.suite-sub {
  font-size: 11px;
  color: var(--fm-muted);
}
.suite-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}
.tab-tools {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}
.settings-grid,
.editor-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px 14px;
  margin-bottom: 12px;
}
.settings-grid label,
.editor-grid label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--fm-muted);
}
.settings-grid label.wide,
.editor-grid label.wide {
  grid-column: 1 / -1;
}
.editor-grid label.row {
  flex-direction: row;
  align-items: center;
  gap: 12px;
  color: var(--fm-text);
}
.editor-grid .stress-config {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.stress-config-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 12px;
  color: var(--fm-muted);
}
.stress-hint {
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
}
.drawer-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  border-top: 1px solid var(--fm-border);
  padding-top: 10px;
}
.mono textarea,
textarea.mono {
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 12px;
}
.preflight-list {
  margin-top: 10px;
  max-height: 360px;
  overflow: auto;
}
.preflight-item {
  padding: 6px 0;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 12px;
}
.preflight-item.error {
  color: var(--fm-danger);
}
.preflight-hint {
  color: var(--fm-muted);
  font-size: 11px;
  padding-left: 4px;
}
.run-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}
.run-meta {
  flex: 1;
  font-size: 12px;
  color: var(--fm-muted);
}
.run-table {
  margin-top: 10px;
}
.case-detail {
  margin-top: 12px;
  border-top: 1px solid var(--fm-border);
  padding-top: 10px;
}
.detail-title {
  font-weight: 600;
  margin-bottom: 6px;
}
.detail-label {
  font-size: 12px;
  color: var(--fm-muted);
  margin: 8px 0 4px;
}
.detail-block pre {
  max-height: 220px;
  overflow: auto;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 8px;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
  margin: 0;
}
.artifact-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
  border-bottom: 1px dashed var(--fm-border);
}
.artifact-row span:first-child {
  flex: 1;
}
.artifact-size {
  color: var(--fm-muted);
  font-size: 11px;
}
.empty {
  margin: 80px auto;
}
</style>
