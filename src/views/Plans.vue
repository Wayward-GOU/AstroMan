<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import {
  CopyOutlined,
  DeleteOutlined,
  DesktopOutlined,
  ExportOutlined,
  EyeOutlined,
  ImportOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  SearchOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import type {
  PlanImportPreview,
  PlanSummary,
  StressReport,
  TestPlan,
} from '@/types/models'

const router = useRouter()
const projectStore = useProjectStore()

const loading = ref(false)
const plans = ref<PlanSummary[]>([])
const projects = ref<{ id: number; name: string }[]>([])
const reports = ref<StressReport[]>([])
const projectFilter = ref<number | null>(null)
const keyword = ref('')
const includeSensitive = ref(false)
const selectedIds = ref<number[]>([])

const detailOpen = ref(false)
const detailLoading = ref(false)
const detail = ref<TestPlan | null>(null)

const importOpen = ref(false)
const importing = ref(false)
const importPayload = ref('')
const importPreview = ref<PlanImportPreview | null>(null)
const importProjectId = ref<number | null>(null)

const filteredPlans = computed(() => {
  const query = keyword.value.trim().toLowerCase()
  if (!query) return plans.value
  return plans.value.filter(
    (item) =>
      item.name.toLowerCase().includes(query) ||
      (item.requestName ?? '').toLowerCase().includes(query) ||
      (item.resolvedUrl ?? item.urlTemplate ?? '').toLowerCase().includes(query),
  )
})

const detailReports = computed(() => {
  if (!detail.value) return []
  return reports.value.filter((item) => item.planId === detail.value?.id).slice(0, 10)
})

function fmtTime(value?: number | null) {
  if (!value) return '—'
  const date = new Date(value)
  return date.toLocaleString('zh-CN', { hour12: false })
}

function fmtNum(value?: number | null, digits = 1) {
  if (value == null || !Number.isFinite(value)) return '—'
  return value >= 1000 ? value.toFixed(0) : value.toFixed(digits)
}

function statusLabel(status?: string | null) {
  switch (status) {
    case 'completed':
      return '完成'
    case 'stopped':
      return '已停止'
    case 'failed':
      return '失败'
    default:
      return status || '—'
  }
}

async function loadAll() {
  loading.value = true
  try {
    if (!projectStore.projects.length) await projectStore.loadProjects()
    projects.value = projectStore.projects.map((item) => ({ id: item.id, name: item.name }))
    const [planList, reportList] = await Promise.all([
      api.planList(projectFilter.value),
      api.reportList(200),
    ])
    plans.value = planList
    reports.value = reportList
    selectedIds.value = []
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

function switchTab(key: string | number) {
  if (key === 'stress') router.push('/stress')
  else if (key === 'reports') router.push('/reports')
}

function runPlan(plan: { id: number }) {
  router.push({ path: '/stress', query: { planId: String(plan.id) } })
}

async function runInWindow(plan: { id: number }) {
  try {
    await api.stressOpenWindow(plan.id, null)
  } catch (e) {
    message.error(String(e))
  }
}

async function duplicatePlan(plan: PlanSummary) {
  try {
    await api.planDuplicate(plan.id)
    message.success(`已复制计划「${plan.name}」`)
    await loadAll()
  } catch (e) {
    message.error(String(e))
  }
}

function deletePlan(plan: PlanSummary) {
  Modal.confirm({
    title: `删除计划「${plan.name}」？`,
    content: '只删除保存的压测配置，已生成的报告不受影响；运行中的计划不能删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      try {
        await api.planDelete(plan.id)
        message.success('计划已删除')
        await loadAll()
      } catch (e) {
        message.error(String(e))
        throw e
      }
    },
  })
}

async function openDetail(plan: PlanSummary) {
  detailOpen.value = true
  detailLoading.value = true
  detail.value = null
  try {
    detail.value = await api.planGet(plan.id)
  } catch (e) {
    message.error(String(e))
    detailOpen.value = false
  } finally {
    detailLoading.value = false
  }
}

async function exportPlans(ids: number[]) {
  if (!ids.length) {
    message.warning('请先选择要导出的计划')
    return
  }
  const doExport = async () => {
    try {
      const text = await api.planExport(ids, includeSensitive.value)
      const path = await saveDialog({
        defaultPath: `astroman-plans-${Date.now()}.json`,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      })
      if (!path) return
      await api.fileWriteText(path, text)
      message.success(`已导出 ${ids.length} 个计划`)
    } catch (e) {
      message.error(String(e))
    }
  }
  if (includeSensitive.value) {
    Modal.confirm({
      title: '导出包含敏感信息？',
      content:
        '将写入认证信息、密钥变量与解析后的完整地址，可能包含 token / 签名，请确认文件存放位置安全。',
      okText: '确认导出',
      okType: 'danger',
      cancelText: '取消',
      onOk: doExport,
    })
    return
  }
  await doExport()
}

async function pickImport() {
  try {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: '计划 JSON', extensions: ['json'] }],
    })
    if (!path || Array.isArray(path)) return
    const text = await api.fileReadText(path)
    importPayload.value = text
    importProjectId.value = projectFilter.value ?? projects.value[0]?.id ?? null
    if (!importProjectId.value) {
      message.warning('请先在顶部选择导入目标项目')
      return
    }
    importPreview.value = await api.planImportPreview(text, importProjectId.value)
    importOpen.value = true
  } catch (e) {
    message.error(String(e))
  }
}

async function refreshImportPreview() {
  if (!importPayload.value || !importProjectId.value) return
  try {
    importPreview.value = await api.planImportPreview(
      importPayload.value,
      importProjectId.value,
    )
  } catch (e) {
    message.error(String(e))
  }
}

async function applyImport() {
  if (!importPayload.value || !importProjectId.value || !importPreview.value?.formatOk) {
    message.warning('文件格式不正确，无法导入')
    return
  }
  importing.value = true
  try {
    const ids = await api.planImportApply(importPayload.value, importProjectId.value)
    message.success(`已导入 ${ids.length} 个计划`)
    importOpen.value = false
    await loadAll()
  } catch (e) {
    message.error(String(e))
  } finally {
    importing.value = false
  }
}

const columns = [
  { title: '计划名', key: 'name', dataIndex: 'name', width: 190, fixed: 'left' as const },
  { title: '项目', key: 'project', dataIndex: 'projectName', width: 130 },
  { title: '目标接口', key: 'request', dataIndex: 'requestName', width: 170 },
  { title: '方法', key: 'method', dataIndex: 'method', width: 76 },
  { title: '完整地址', key: 'url', dataIndex: 'resolvedUrl', width: 320 },
  { title: '环境', key: 'env', dataIndex: 'envName', width: 110 },
  { title: '压测方式', key: 'mode', dataIndex: 'stagesSummary', width: 190 },
  { title: '最近运行', key: 'lastRunAt', dataIndex: 'lastRunAt', width: 160 },
  { title: '最近状态', key: 'lastStatus', dataIndex: 'lastStatus', width: 96 },
  { title: '报告数', key: 'reportCount', dataIndex: 'reportCount', width: 82 },
  { title: '操作', key: 'actions', width: 210, fixed: 'right' as const },
]

const rowSelection = computed(() => ({
  selectedRowKeys: selectedIds.value,
  onChange: (keys: (string | number)[]) => {
    selectedIds.value = keys.map((key) => Number(key))
  },
}))

onMounted(loadAll)
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Stress Plans"
      title="压测计划"
      subtitle="跨项目统一管理压测配置：直接运行、新窗口运行、复制、导入导出"
    >
      <a-button @click="pickImport">
        <template #icon><ImportOutlined /></template>
        导入计划
      </a-button>
      <a-button type="primary" @click="router.push('/stress')">
        <template #icon><PlusOutlined /></template>
        新建计划
      </a-button>
    </PageHeader>

    <a-tabs :active-key="'plans'" size="small" class="stress-top-tabs" @change="switchTab">
      <a-tab-pane key="stress" tab="压测" />
      <a-tab-pane key="plans" tab="计划" />
      <a-tab-pane key="reports" tab="报告" />
    </a-tabs>

    <div class="fm-card toolbar">
      <a-select
        v-model:value="projectFilter"
        allow-clear
        placeholder="全部项目"
        style="width: 188px"
        :options="projects.map((item) => ({ value: item.id, label: item.name }))"
        @change="loadAll"
      />
      <a-input v-model:value="keyword" allow-clear placeholder="搜索计划名 / 接口 / 地址" class="search">
        <template #prefix><SearchOutlined /></template>
      </a-input>
      <a-checkbox v-model:checked="includeSensitive">导出包含敏感信息</a-checkbox>
      <div class="toolbar-right">
        <a-button :disabled="!selectedIds.length" @click="exportPlans(selectedIds)">
          <template #icon><ExportOutlined /></template>
          导出所选（{{ selectedIds.length }}）
        </a-button>
        <a-button :loading="loading" @click="loadAll">
          <template #icon><ReloadOutlined /></template>
          刷新
        </a-button>
      </div>
    </div>

    <a-spin :spinning="loading">
      <div class="fm-card table-card">
        <a-table
          :columns="columns"
          :data-source="filteredPlans"
          :row-key="(record: PlanSummary) => record.id"
          :row-selection="rowSelection"
          :pagination="{ pageSize: 20, showSizeChanger: true, showTotal: (total: number) => `共 ${total} 个计划` }"
          :scroll="{ x: 1750, y: 'calc(100vh - 360px)' }"
          size="middle"
          class="plans-table"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'name'">
              <a-button type="link" size="small" class="plan-name-btn" @click="openDetail(record)">
                {{ record.name }}
              </a-button>
            </template>
            <template v-else-if="column.key === 'project'">
              {{ record.projectName || '—' }}
            </template>
            <template v-else-if="column.key === 'request'">
              <div class="request-cell">
                <span class="request-name" :title="record.requestName || ''">
                  {{ record.requestName || '未绑定接口' }}
                </span>
                <span v-if="record.requestMissing" class="missing-tag">接口已删除</span>
              </div>
            </template>
            <template v-else-if="column.key === 'method'">
              <span class="method-chip">{{ record.method || 'GET' }}</span>
            </template>
            <template v-else-if="column.key === 'url'">
              <span class="url-cell mono" :title="record.resolvedUrl || record.urlTemplate || ''">
                {{ record.resolvedUrl || record.urlTemplate || '—' }}
              </span>
            </template>
            <template v-else-if="column.key === 'env'">
              {{ record.envName || '—' }}
            </template>
            <template v-else-if="column.key === 'mode'">
              {{ record.stagesSummary }}
            </template>
            <template v-else-if="column.key === 'lastRunAt'">
              {{ fmtTime(record.lastRunAt) }}
            </template>
            <template v-else-if="column.key === 'lastStatus'">
              <a-tag :color="record.lastStatus === 'completed' ? 'green' : record.lastStatus === 'failed' ? 'red' : 'default'">
                {{ statusLabel(record.lastStatus) }}
              </a-tag>
            </template>
            <template v-else-if="column.key === 'reportCount'">
              <a-button v-if="record.reportCount" type="link" size="small" @click="router.push('/reports')">
                {{ record.reportCount }}
              </a-button>
              <span v-else>0</span>
            </template>
            <template v-else-if="column.key === 'actions'">
              <div class="row-actions">
                <a-button type="link" size="small" @click="runPlan(record)">
                  <template #icon><PlayCircleOutlined /></template>
                  运行
                </a-button>
                <a-tooltip title="在独立窗口运行，主窗口可继续做别的">
                  <a-button type="link" size="small" @click="runInWindow(record)">
                    <template #icon><DesktopOutlined /></template>
                    新窗口
                  </a-button>
                </a-tooltip>
                <a-dropdown>
                  <a-button type="link" size="small">更多</a-button>
                  <template #overlay>
                    <a-menu>
                      <a-menu-item key="detail" @click="openDetail(record)">
                        <EyeOutlined /> 查看详情
                      </a-menu-item>
                      <a-menu-item key="duplicate" @click="duplicatePlan(record)">
                        <CopyOutlined /> 复制计划
                      </a-menu-item>
                      <a-menu-item key="export" @click="exportPlans([record.id])">
                        <ExportOutlined /> 导出 JSON
                      </a-menu-item>
                      <a-menu-divider />
                      <a-menu-item key="delete" danger @click="deletePlan(record)">
                        <DeleteOutlined /> 删除
                      </a-menu-item>
                    </a-menu>
                  </template>
                </a-dropdown>
              </div>
            </template>
          </template>
          <template #emptyText>
            <div class="empty-plans">
              <ThunderboltOutlined />
              <div>还没有压测计划</div>
              <div class="empty-desc">在压测页配置好参数后保存为计划，即可在这里统一管理</div>
              <a-button type="primary" size="small" @click="router.push('/stress')">去创建</a-button>
            </div>
          </template>
        </a-table>
      </div>
    </a-spin>

    <!-- 计划详情 -->
    <a-modal v-model:open="detailOpen" title="计划详情" :width="820" :footer="null">
      <a-spin :spinning="detailLoading">
        <div v-if="detail" class="detail-body">
          <div class="detail-head">
            <div>
              <div class="detail-name">{{ detail.name }}</div>
              <div class="detail-sub">
                <span>项目 ID {{ detail.projectId }}</span>
                <span v-if="detail.requestId"> · 接口 ID {{ detail.requestId }}</span>
              </div>
            </div>
            <div class="detail-actions">
              <a-button @click="runPlan({ id: detail.id })">
                <template #icon><PlayCircleOutlined /></template>
                运行
              </a-button>
              <a-button type="primary" @click="runInWindow({ id: detail.id })">
                <template #icon><DesktopOutlined /></template>
                新窗口运行
              </a-button>
            </div>
          </div>

          <div class="detail-grid">
            <div class="detail-item">
              <span class="detail-label">压测方式</span>
              <span>{{ filteredPlans.find((item) => item.id === detail?.id)?.stagesSummary || '—' }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">环境</span>
              <span>{{ filteredPlans.find((item) => item.id === detail?.id)?.envName || '—' }}</span>
            </div>
            <div class="detail-item span-2">
              <span class="detail-label">目标地址</span>
              <span class="mono break">
                {{ filteredPlans.find((item) => item.id === detail?.id)?.resolvedUrl
                  || filteredPlans.find((item) => item.id === detail?.id)?.urlTemplate || '—' }}
              </span>
            </div>
          </div>

          <div class="detail-section-title">最近报告</div>
          <div v-if="detailReports.length" class="detail-reports">
            <div v-for="report in detailReports" :key="report.id" class="detail-report">
              <a-button type="link" size="small" @click="router.push(`/reports/${report.id}`)">
                R-{{ report.id }} · {{ statusLabel(report.status) }}
              </a-button>
              <span class="mono">{{ fmtTime(report.createdAt) }}</span>
              <span>P95 {{ fmtNum(report.p95Rt) }} ms</span>
              <span>{{ fmtNum(report.avgQps) }} QPS</span>
              <span>错误率 {{ fmtNum(report.errorRate) }}%</span>
            </div>
          </div>
          <div v-else class="detail-empty">该计划还没有生成报告</div>

          <div class="detail-section-title">配置快照</div>
          <pre class="config-json">{{ JSON.stringify(detail.config, null, 2) }}</pre>
        </div>
      </a-spin>
    </a-modal>

    <!-- 导入预览 -->
    <a-modal
      v-model:open="importOpen"
      title="导入压测计划"
      :width="760"
      ok-text="确认导入"
      cancel-text="取消"
      :ok-button-props="{ disabled: !importPreview?.formatOk }"
      :confirm-loading="importing"
      @ok="applyImport"
    >
      <div class="import-body">
        <div class="import-row">
          <span class="import-label">目标项目</span>
          <a-select
            v-model:value="importProjectId"
            style="width: 240px"
            :options="projects.map((item) => ({ value: item.id, label: item.name }))"
            @change="refreshImportPreview"
          />
          <span v-if="importPreview?.includesSensitive" class="import-warn">
            文件包含敏感信息，导入后请检查变量值
          </span>
        </div>

        <div v-if="importPreview && !importPreview.formatOk" class="import-invalid">
          文件格式不正确，需要 astroman.stress-plan/v1 格式
        </div>

        <div v-else-if="importPreview" class="import-list">
          <div v-for="(item, index) in importPreview.plans" :key="index" class="import-item">
            <div class="import-item-head">
              <span class="import-item-name">{{ item.planName }}</span>
              <span class="method-chip">{{ item.requestMethod || 'GET' }}</span>
              <span class="url-cell mono" :title="item.requestUrl">{{ item.requestUrl }}</span>
            </div>
            <div class="import-item-meta">
              <span>环境：{{ item.envName || '未绑定' }}</span>
              <span v-if="item.missingVars.length" class="import-warn">
                缺少变量：{{ item.missingVars.join('、') }}
              </span>
              <span v-if="item.hasSensitive" class="import-warn">包含敏感字段</span>
              <span v-for="warning in item.warnings" :key="warning" class="import-warn">{{ warning }}</span>
            </div>
          </div>
        </div>
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.stress-top-tabs {
  margin: -8px 0 14px;
}

.stress-top-tabs :deep(.ant-tabs-nav) {
  margin: 0;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 10px 12px;
  margin-bottom: 12px;
}

.toolbar .search {
  width: 280px;
}

.toolbar-right {
  margin-left: auto;
  display: flex;
  gap: 8px;
}

.table-card {
  padding: 4px 0 0;
}

.plan-name-btn {
  padding: 0;
  font-weight: 500;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.request-cell {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.request-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.missing-tag {
  flex: none;
  padding: 0 5px;
  border: 1px solid var(--fm-warning);
  border-radius: 2px;
  color: var(--fm-warning);
  font-size: 10px;
}

.method-chip {
  display: inline-block;
  padding: 0 6px;
  border-radius: 2px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 10.5px;
  font-weight: 600;
  font-family: var(--fm-font-mono);
  line-height: 18px;
}

.url-cell {
  display: block;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--fm-text-2);
}

.row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.empty-plans {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 42px 0;
  color: var(--fm-muted);
}

.empty-plans :deep(.anticon) {
  font-size: 26px;
  color: var(--fm-primary);
}

.empty-desc {
  font-size: 11.5px;
  margin-bottom: 6px;
}

.detail-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.detail-name {
  font-size: 16px;
  font-weight: 600;
}

.detail-sub {
  margin-top: 4px;
  font-size: 11px;
  color: var(--fm-muted);
}

.detail-actions {
  display: flex;
  gap: 8px;
}

.detail-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  background: var(--fm-surface-alt);
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 12px;
}

.detail-item.span-2 {
  grid-column: span 2;
}

.detail-label {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.detail-section-title {
  font-size: 12px;
  font-weight: 600;
  padding-top: 4px;
}

.detail-reports {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
}

.detail-report {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 6px 10px;
  font-size: 11.5px;
  border-bottom: 1px solid var(--fm-border);
}

.detail-report:last-child {
  border-bottom: none;
}

.detail-empty {
  padding: 14px;
  text-align: center;
  font-size: 11.5px;
  color: var(--fm-muted);
  border: 1px dashed var(--fm-border);
  border-radius: 4px;
}

.config-json {
  margin: 0;
  max-height: 240px;
  overflow: auto;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 11px;
  line-height: 1.55;
}

.import-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.import-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.import-label {
  font-size: 12px;
  color: var(--fm-text-2);
}

.import-warn {
  color: var(--fm-warning);
  font-size: 11px;
}

.import-invalid {
  padding: 16px;
  border: 1px dashed var(--fm-danger);
  border-radius: 4px;
  color: var(--fm-danger);
  text-align: center;
}

.import-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 380px;
  overflow: auto;
}

.import-item {
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
}

.import-item-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.import-item-name {
  font-weight: 600;
  font-size: 12.5px;
  flex: none;
}

.import-item-head .url-cell {
  flex: 1;
  min-width: 0;
}

.import-item-meta {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  margin-top: 5px;
  font-size: 11px;
  color: var(--fm-muted);
}

.break {
  word-break: break-all;
  white-space: normal;
}
</style>
