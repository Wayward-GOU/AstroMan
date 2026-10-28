<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  ThunderboltOutlined,
  ArrowLeftOutlined,
  StopOutlined,
  PlayCircleOutlined,
  FileTextOutlined,
  InfoCircleOutlined,
  PlusOutlined,
  FolderOpenOutlined,
  SaveOutlined,
  DeleteOutlined,
  RobotOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import { useProjectStore } from '@/stores/project'
import { useAiAssistStore } from '@/stores/aiAssist'
import { stressPlanPrompt } from '@/utils/aiScenarios'
import * as api from '@/api'
import type {
  Environment,
  HttpRequestDetail,
  PlanSummary,
  RequestSearchHit,
  StressDone,
  StressProgress,
  StressStage,
} from '@/types/models'

const route = useRoute()
const router = useRouter()
const projectStore = useProjectStore()

const requestId = computed(() => Number(route.query.requestId || 0))
const projectId = computed(() => Number(route.query.projectId || 0))

const request = ref<HttpRequestDetail | null>(null)
const environments = ref<Environment[]>([])
const envId = ref<number | null>(null)
const loading = ref(true)

/* ---------- 新建压测弹窗 ---------- */
const configOpen = ref(false)
const targetProjectId = ref<number | null>(null)
const targetRequestId = ref<number | null>(null)
const choices = ref<RequestSearchHit[]>([])
const choicesLoading = ref(false)
const preparing = ref(false)
const activeProjectId = ref<number | null>(null)
const modalEnvironments = ref<Environment[]>([])
const modalEnvId = ref<number | null>(null)

const projectOptions = computed(() =>
  projectStore.projects.map((item) => ({ value: item.id, label: item.name })),
)

const requestSelectOptions = computed(() =>
  choices.value.map((item) => ({
    value: item.id,
    label: `${item.method}  ${item.name}  ·  ${item.url}`,
  })),
)

const selectedChoice = computed(
  () => choices.value.find((item) => item.id === targetRequestId.value) ?? null,
)

const currentProject = computed(
  () =>
    projectStore.projects.find(
      (item) => item.id === (targetProjectId.value ?? activeProjectId.value),
    ) ?? null,
)

const aiAssist = useAiAssistStore()

function aiPlan() {
  if (!request.value) {
    message.warning('先选择一个接口再做计划')
    return
  }
  void aiAssist.show({
    title: '压测计划建议',
    prompt: stressPlanPrompt(request.value, settings, stages.value),
    hint: `${request.value.method} ${request.value.url} · 当前 ${settings.concurrent} 并发 × ${settings.durationSec}s`,
  })
}

async function openConfig() {
  if (running.value) {
    message.warning('压测进行中，请先停止当前压测')
    return
  }
  if (!projectStore.projects.length) {
    try {
      await projectStore.loadProjects()
    } catch (e) {
      message.error(String(e))
      return
    }
  }
  targetProjectId.value = projectStore.projects[0]?.id ?? null
  targetRequestId.value = null
  configOpen.value = true
}

watch(targetProjectId, async (id) => {
  targetRequestId.value = null
  choices.value = []
  modalEnvironments.value = []
  modalEnvId.value = null
  plans.value = []
  activePlanId.value = null
  if (!id) return
  choicesLoading.value = true
  try {
    const [list, envs, planList] = await Promise.all([
      api.requestOptions(id),
      api.envList(id),
      api.planList(id),
    ])
    choices.value = list
    modalEnvironments.value = envs
    plans.value = planList
    const active = envs.find((env) => env.isActive) ?? envs[0]
    modalEnvId.value = active?.id ?? null
  } catch (e) {
    message.error(String(e))
  } finally {
    choicesLoading.value = false
  }
})

/* ---------- 压测计划 ---------- */
const plans = ref<PlanSummary[]>([])
const activePlanId = ref<number | null>(null)
const planName = ref('')
const planSaving = ref(false)

function currentPlanConfig() {
  return {
    settings: { ...settings },
    stages: stages.value.map((item) => ({ ...item })),
    envId: modalEnvId.value,
  }
}

async function applyPlan(id: number | null) {
  if (!id) {
    activePlanId.value = null
    planName.value = ''
    return
  }
  try {
    const plan = await api.planGet(id)
    const config = plan.config as {
      settings?: Record<string, unknown>
      stages?: StressStage[]
      envId?: number | null
    } | null
    if (config?.settings) Object.assign(settings, config.settings)
    if (Array.isArray(config?.stages) && config.stages.length) {
      stages.value = config.stages.map((item) => ({
        concurrent: Number(item.concurrent) || 1,
        durationSec: Number(item.durationSec) || 1,
      }))
    }
    if (config?.envId !== undefined) modalEnvId.value = config.envId ?? null
    if (plan.requestId) targetRequestId.value = plan.requestId
    activePlanId.value = plan.id
    planName.value = plan.name
    message.success(`已载入计划「${plan.name}」`)
  } catch (e) {
    message.error(String(e))
  }
}

async function savePlan() {
  if (!targetProjectId.value) return
  const name = planName.value.trim() || `${selectedChoice.value?.name ?? '压测'} 计划`
  planSaving.value = true
  try {
    const saved = await api.planSave({
      id: activePlanId.value,
      projectId: targetProjectId.value,
      requestId: targetRequestId.value,
      name,
      config: currentPlanConfig(),
    })
    activePlanId.value = saved.id
    planName.value = saved.name
    plans.value = await api.planList(targetProjectId.value)
    message.success('计划已保存，可在下次压测时直接载入')
  } catch (e) {
    message.error(String(e))
  } finally {
    planSaving.value = false
  }
}

function removePlan() {
  if (!activePlanId.value) return
  const id = activePlanId.value
  Modal.confirm({
    title: '删除该压测计划？',
    content: '只删除保存的压测配置，已生成的压测报告不受影响。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.planDelete(id)
      plans.value = plans.value.filter((item) => item.id !== id)
      activePlanId.value = null
      planName.value = ''
      message.success('计划已删除')
    },
  })
}

async function startFromConfig() {
  if (!targetProjectId.value) {
    message.warning('请先选择项目')
    return
  }
  if (!targetRequestId.value) {
    message.warning('请选择要压测的接口')
    return
  }
  preparing.value = true
  try {
    const detail = await api.requestGet(targetRequestId.value)
    if (!detail.url.trim()) {
      message.warning('该接口还没有填写 URL，请先到请求编辑器补全')
      return
    }
    request.value = detail
    activeProjectId.value = targetProjectId.value
    environments.value = modalEnvironments.value
    envId.value = modalEnvId.value
    configOpen.value = false
    done.value = null
    progress.value = null
    await start()
  } catch (e) {
    message.error(String(e))
  } finally {
    preparing.value = false
  }
}


const settings = reactive({
  mode: 'fixed' as 'fixed' | 'stages',
  concurrent: 20,
  durationSec: 30,
  rampUpSec: 5,
  thinkTimeMs: 0,
  timeoutMs: 30000,
  connectTimeoutMs: 10000,
  followRedirects: true,
  stopOnErrorRate: false,
  stopErrorRate: 5,
  stopMinRequests: 100,
  insecure: false,
  targetRps: 0,
  slaEnabled: false,
  slaP95: 500,
  slaP99: 1000,
  slaErrorRate: 1,
  slaMinRps: 0,
})

const stages = ref<StressStage[]>([
  { concurrent: 10, durationSec: 30 },
  { concurrent: 30, durationSec: 30 },
  { concurrent: 60, durationSec: 30 },
])

function addStage() {
  const last = stages.value[stages.value.length - 1]
  stages.value.push({
    concurrent: Math.min(500, (last?.concurrent ?? 0) + 20),
    durationSec: last?.durationSec ?? 30,
  })
}

function removeStage(index: number) {
  if (stages.value.length <= 1) return
  stages.value.splice(index, 1)
}

const running = ref(false)
const stopping = ref(false)
const progress = ref<StressProgress | null>(null)
const done = ref<StressDone | null>(null)
const resultCard = ref<HTMLElement | null>(null)
const unlisteners: UnlistenFn[] = []

onMounted(async () => {
  try {
    unlisteners.push(
      await listen<StressProgress>('stress:progress', (event) => {
        progress.value = event.payload
      }),
    )
    unlisteners.push(
      await listen<StressDone>('stress:done', (event) => {
        done.value = event.payload
        running.value = false
        stopping.value = false
        if (event.payload.reportId) {
          message.success('压测已结束，报告已生成')
        } else if (event.payload.message) {
          message.error(event.payload.message)
        }
        nextTick(() => resultCard.value?.scrollIntoView({ behavior: 'smooth', block: 'center' }))
      }),
    )
  } catch (e) {
    console.error('压测事件监听失败', e)
  }

  try {
    if (!requestId.value) {
      loading.value = false
      return
    }
    if (projectId.value) {
      activeProjectId.value = projectId.value
      environments.value = await api.envList(projectId.value)
      const active = environments.value.find((env) => env.isActive) ?? environments.value[0]
      envId.value = active?.id ?? null
    }
    request.value = await api.requestGet(requestId.value)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  for (const off of unlisteners) off()
  if (running.value) {
    api.stressStop().catch(() => undefined)
  }
})

const samples = computed(() => progress.value?.samples ?? [])

const qpsSeries = computed(() => [
  {
    name: 'QPS',
    color: '#3d6b8f',
    values: samples.value.map((s) => s.qps),
  },
])

const latencySeries = computed(() => [
  { name: '平均', color: '#3d6b8f', values: samples.value.map((s) => s.avgRt) },
  { name: 'P95', color: '#b07d33', values: samples.value.map((s) => s.p95Rt) },
  { name: 'P99', color: '#b0524d', values: samples.value.map((s) => s.p99Rt) },
])

const chartLabels = computed(() => samples.value.map((s) => `${s.elapsedSec}s`))

const elapsed = computed(() => progress.value?.elapsedSec ?? done.value?.elapsedSec ?? 0)

const totalSeconds = computed(() => {
  if (settings.mode === 'stages') {
    return stages.value.reduce((sum, item) => sum + Math.max(1, item.durationSec || 0), 0)
  }
  return Math.max(1, settings.durationSec)
})

const peakConcurrent = computed(() => {
  if (settings.mode === 'stages') {
    return stages.value.reduce((peak, item) => Math.max(peak, item.concurrent || 0), 0)
  }
  return settings.concurrent
})

const percent = computed(() =>
  Math.min(100, Math.round((elapsed.value / Math.max(1, totalSeconds.value)) * 100)),
)

const stageLabel = computed(() => {
  if (settings.mode !== 'stages') return ''
  const index = progress.value?.stageIndex ?? 0
  return `阶段 ${index + 1}/${stages.value.length}`
})

const statusCodes = computed(() => progress.value?.statusCodes ?? [])
const statusCodeTotal = computed(() =>
  statusCodes.value.reduce((sum, item) => sum + item.count, 0),
)

function codeClass(code: number) {
  if (code >= 500) return 'bad'
  if (code >= 400) return 'warn'
  if (code >= 300) return 'info'
  return 'ok'
}

const errorRate = computed(() => {
  const total = progress.value?.total ?? done.value?.total ?? 0
  const errors = progress.value?.errors ?? done.value?.errors ?? 0
  return total > 0 ? (errors / total) * 100 : 0
})

const currentRps = computed(() => {
  if (progress.value) return progress.value.rps
  if (done.value && done.value.elapsedSec > 0) return done.value.total / done.value.elapsedSec
  return 0
})

function fmt(v: number | undefined | null, digits = 1) {
  if (v === undefined || v === null || Number.isNaN(v)) return '-'
  return v >= 1000 ? v.toFixed(0) : v.toFixed(digits)
}

async function start() {
  if (!request.value) return
  if (!request.value.url.trim()) {
    message.warning('请先回到请求编辑器填写 URL')
    return
  }

  if (settings.mode === 'stages') {
    if (stages.value.length < 1) {
      message.warning('至少需要一个加压阶段')
      return
    }
    if (stages.value.some((item) => item.concurrent < 1 || item.concurrent > 500)) {
      message.warning('阶段并发数需在 1 到 500 之间')
      return
    }
    if (stages.value.some((item) => item.durationSec < 1)) {
      message.warning('每个阶段至少持续 1 秒')
      return
    }
  } else {
    if (settings.concurrent < 1 || settings.concurrent > 500) {
      message.warning('并发数需在 1 到 500 之间')
      return
    }
    if (settings.rampUpSec >= settings.durationSec) {
      message.warning('预热时长需要小于压测时长')
      return
    }
  }

  progress.value = null
  done.value = null
  stopping.value = false
  running.value = true

  try {
    await api.stressStart({
      requestId: request.value.id,
      name: request.value.name,
      method: request.value.method,
      url: request.value.url,
      headers: request.value.headers,
      params: request.value.params,
      bodyType: request.value.bodyType,
      body: request.value.body,
      envId: envId.value,
      insecure: settings.insecure,
      timeoutMs: settings.timeoutMs,
      connectTimeoutMs: settings.connectTimeoutMs,
      followRedirects: settings.followRedirects,
      stopErrorRate: settings.stopOnErrorRate ? settings.stopErrorRate : null,
      stopMinRequests: settings.stopOnErrorRate ? settings.stopMinRequests : null,
      concurrent: settings.mode === 'fixed' ? settings.concurrent : peakConcurrent.value,
      durationSec: totalSeconds.value,
      rampUpSec: settings.mode === 'fixed' ? settings.rampUpSec : 0,
      thinkTimeMs: settings.thinkTimeMs,
      stages: settings.mode === 'stages' ? stages.value.map((item) => ({ ...item })) : [],
      targetRps: settings.targetRps > 0 ? settings.targetRps : null,
      sla: settings.slaEnabled
        ? {
            p95Ms: settings.slaP95 > 0 ? settings.slaP95 : null,
            p99Ms: settings.slaP99 > 0 ? settings.slaP99 : null,
            errorRate: settings.slaErrorRate > 0 ? settings.slaErrorRate : null,
            minRps: settings.slaMinRps > 0 ? settings.slaMinRps : null,
          }
        : null,
    })
  } catch (e) {
    running.value = false
    message.error(String(e))
  }
}

async function stop() {
  stopping.value = true
  try {
    await api.stressStop()
  } catch (e) {
    message.error(String(e))
  }
}

function backToRequest() {
  if (!request.value?.id) {
    router.push('/')
    return
  }
  router.push({
    name: 'request',
    params: { id: request.value.id },
    query: { projectId: activeProjectId.value || undefined },
  })
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Stress Test"
      title="接口压测"
      subtitle="选择项目与接口、配置压测参数，在本地发起真实并发请求"
    >
      <a-button :disabled="!request" @click="aiPlan">
        <template #icon><RobotOutlined /></template>
        AI 生成计划
      </a-button>
      <a-button type="primary" :disabled="running" @click="openConfig">
        <template #icon><PlusOutlined /></template>
        新建压测
      </a-button>
      <a-button @click="backToRequest">
        <template #icon><ArrowLeftOutlined /></template>
        返回请求
      </a-button>
      <a-button @click="router.push('/reports')">
        <template #icon><FileTextOutlined /></template>
        压测报告
      </a-button>
    </PageHeader>

    <a-spin :spinning="loading">
      <div v-if="!request && !requestId" class="fm-card guide-empty">
        <div class="empty-icon-wrap"><ThunderboltOutlined /></div>
        <div class="empty-title">选择一个接口开始压测</div>
        <div class="empty-desc">
          点击「新建压测」，先选择项目与接口，再填写并发、时长、限速与 SLA 阈值，
          确认后立即在本地发起真实压测并自动生成报告。
        </div>
        <a-button type="primary" size="large" @click="openConfig">
          <template #icon><PlusOutlined /></template>
          新建压测
        </a-button>
      </div>

      <div v-else-if="request" class="layout">
        <section class="fm-card config">
          <div class="pane-head">
            <span>压测配置</span>
            <span class="pane-tag">单接口</span>
          </div>

          <div class="target">
            <div class="target-top">
              <span class="method-tag" :class="`method-${request.method}`">{{ request.method }}</span>
              <span class="target-name">{{ request.name }}</span>
            </div>
            <div class="target-url mono">{{ request.url }}</div>
            <div class="target-meta">
              Headers {{ request.headers.filter((h) => h.enabled && h.key).length }} 项 ·
              Params {{ request.params.filter((p) => p.enabled && p.key).length }} 项 ·
              Body {{ request.bodyType === 'none' ? '无' : request.bodyType }}
            </div>
          </div>

          <div class="form">
            <div class="field">
              <div class="label">加压模式</div>
              <a-segmented
                v-model:value="settings.mode"
                :options="[
                  { label: '固定并发', value: 'fixed' },
                  { label: '阶梯加压', value: 'stages' },
                ]"
                block
                :disabled="running"
              />
              <div class="hint">
                {{ settings.mode === 'fixed'
                  ? '固定并发持续压测，适合验证稳定吞吐'
                  : '按阶段逐步提升并发，用于寻找性能拐点' }}
              </div>
            </div>

            <template v-if="settings.mode === 'fixed'">
              <div class="field">
                <div class="label">并发数</div>
                <a-input-number v-model:value="settings.concurrent" :min="1" :max="500" style="width: 100%" />
                <div class="hint">同一时刻同时执行的请求数量，建议从 10～30 起步</div>
              </div>

              <div class="field-row">
                <div class="field">
                  <div class="label">压测时长（秒）</div>
                  <a-input-number v-model:value="settings.durationSec" :min="1" :max="3600" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">预热时长（秒）</div>
                  <a-input-number v-model:value="settings.rampUpSec" :min="0" :max="600" style="width: 100%" />
                </div>
              </div>
            </template>

            <template v-else>
              <div class="field">
                <div class="label stage-label">
                  <span>加压阶段</span>
                  <a-button size="small" type="link" :disabled="running" @click="addStage">+ 添加阶段</a-button>
                </div>
                <div class="stage-list">
                  <div class="stage-row stage-header">
                    <span />
                    <span>并发数</span>
                    <span>时长（秒）</span>
                    <span />
                  </div>
                  <div v-for="(stage, index) in stages" :key="index" class="stage-row">
                    <span class="stage-index">{{ index + 1 }}</span>
                    <a-input-number
                      v-model:value="stage.concurrent"
                      :min="1"
                      :max="500"
                      size="small"
                      :disabled="running"
                    />
                    <a-input-number
                      v-model:value="stage.durationSec"
                      :min="1"
                      :max="3600"
                      size="small"
                      :disabled="running"
                    />
                    <a-button
                      size="small"
                      type="text"
                      danger
                      :disabled="running || stages.length <= 1"
                      @click="removeStage(index)"
                    >
                      移除
                    </a-button>
                  </div>
                </div>
                <div class="hint">
                  共 {{ totalSeconds }} 秒，峰值并发 {{ peakConcurrent }}；阶段切换时新并发立即生效
                </div>
              </div>
            </template>

            <div class="field-row">
              <div class="field">
                <div class="label">思考时间（毫秒）</div>
                <a-input-number v-model:value="settings.thinkTimeMs" :min="0" :max="60000" :step="100" style="width: 100%" />
              </div>
              <div class="field">
                <div class="label">超时（毫秒）</div>
                <a-input-number v-model:value="settings.timeoutMs" :min="500" :max="120000" :step="1000" style="width: 100%" />
              </div>
            </div>

            <div class="field">
              <div class="label">目标 RPS 限速</div>
              <a-input-number v-model:value="settings.targetRps" :min="0" :max="100000" :step="10" style="width: 100%" />
              <div class="hint">0 表示不限速；设置后每秒最多发出该数量的请求</div>
            </div>

            <div class="field">
              <div class="label">环境</div>
              <a-select v-model:value="envId" allow-clear placeholder="不使用环境变量" style="width: 100%">
                <a-select-option v-for="env in environments" :key="env.id" :value="env.id">
                  {{ env.name }}
                </a-select-option>
              </a-select>
              <div class="hint" v-pre>选择环境后会替换 URL 与 Headers 中的 {{变量}}</div>
            </div>

            <a-checkbox v-model:checked="settings.insecure">忽略证书校验（自签证书）</a-checkbox>

            <div class="sla-block">
              <div class="sla-head">
                <a-checkbox v-model:checked="settings.slaEnabled">设置 SLA 验收阈值</a-checkbox>
              </div>
              <div v-if="settings.slaEnabled" class="sla-grid">
                <div class="field">
                  <div class="label">P95 ≤（ms）</div>
                  <a-input-number v-model:value="settings.slaP95" :min="0" :max="120000" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">P99 ≤（ms）</div>
                  <a-input-number v-model:value="settings.slaP99" :min="0" :max="120000" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">错误率 ≤（%）</div>
                  <a-input-number v-model:value="settings.slaErrorRate" :min="0" :max="100" :step="0.5" size="small" style="width: 100%" />
                </div>
                <div class="field">
                  <div class="label">平均 RPS ≥</div>
                  <a-input-number v-model:value="settings.slaMinRps" :min="0" :max="100000" size="small" style="width: 100%" />
                </div>
              </div>
              <div v-if="settings.slaEnabled" class="hint">报告生成时会逐项判定并给出是否达标</div>
            </div>
          </div>

          <div class="config-foot">
            <a-button
              v-if="!running"
              type="primary"
              size="large"
              block
              :disabled="!request"
              @click="start"
            >
              <template #icon><PlayCircleOutlined /></template>
              开始压测
            </a-button>
            <a-button v-else type="primary" danger size="large" block :loading="stopping" @click="stop">
              <template #icon><StopOutlined /></template>
              {{ stopping ? '正在停止…' : '停止压测' }}
            </a-button>
            <div class="leave-hint">离开本页会自动停止压测，已产生的数据仍会保存</div>
          </div>
        </section>

        <section class="live">
          <div class="fm-card status-card">
            <div class="status-head">
              <div class="status-left">
                <span class="status-dot" :class="{ live: running, done: !running && done }" />
                <span class="status-text">
                  {{ running ? '压测进行中' : done ? '压测已结束' : '等待开始' }}
                </span>
                <span v-if="running" class="status-elapsed mono">{{ elapsed }}s / {{ totalSeconds }}s</span>
                <span v-if="running && stageLabel" class="status-stage">{{ stageLabel }}</span>
              </div>
              <span class="status-name">{{ request.name }}</span>
            </div>
            <a-progress
              :percent="running || done ? percent : 0"
              :show-info="false"
              :stroke-color="running ? '#3d6b8f' : done ? '#3f7f5f' : '#cbd5e1'"
              size="small"
            />
            <div class="stats">
              <div class="stat">
                <span class="stat-label">总请求</span>
                <span class="stat-value mono">{{ progress?.total ?? done?.total ?? 0 }}</span>
              </div>
              <div class="stat">
                <span class="stat-label">当前 RPS</span>
                <span class="stat-value mono">{{ fmt(currentRps) }}</span>
              </div>
              <div class="stat">
                <span class="stat-label">平均延迟</span>
                <span class="stat-value mono">{{ fmt(progress?.avgRt) }} <i>ms</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">P95</span>
                <span class="stat-value mono warn">{{ fmt(progress?.p95Rt) }} <i>ms</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">错误率</span>
                <span class="stat-value mono" :class="{ bad: errorRate > 0 }">{{ errorRate.toFixed(1) }} <i>%</i></span>
              </div>
              <div class="stat">
                <span class="stat-label">并发</span>
                <span class="stat-value mono">{{ progress?.concurrent ?? 0 }} <i>/ {{ peakConcurrent }}</i></span>
              </div>
            </div>

            <div v-if="statusCodes.length > 0" class="status-codes">
              <span class="codes-title">状态码分布</span>
              <div class="status-code-item" v-for="item in statusCodes" :key="item.code">
                <span class="code" :class="`code-${codeClass(item.code)}`">{{ item.code }}</span>
                <span class="count">{{ item.count }}</span>
                <span class="ratio">{{ statusCodeTotal > 0 ? ((item.count / statusCodeTotal) * 100).toFixed(1) : '0.0' }}%</span>
              </div>
            </div>
          </div>

          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">吞吐量 QPS</span>
              <span class="chart-sub">按秒统计</span>
            </div>
            <LineChart :series="qpsSeries" :labels="chartLabels" unit="" />
          </div>

          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">响应延迟</span>
              <span class="chart-sub">单位 ms</span>
            </div>
            <LineChart :series="latencySeries" :labels="chartLabels" unit="ms" :area="false" />
          </div>

          <div v-if="done" ref="resultCard" class="fm-card result-card">
            <div class="result-icon"><FileTextOutlined /></div>
            <div class="result-main">
              <div class="result-title">
                {{ done.status === 'stopped' ? '压测已手动停止' : '压测完成' }}
              </div>
              <div class="result-desc">
                共 {{ done.total }} 次请求，成功 {{ done.success }} 次，失败 {{ done.errors }} 次，
                耗时 {{ done.elapsedSec }} 秒{{ done.message ? ` · ${done.message}` : '' }}
              </div>
            </div>
            <a-button v-if="done.reportId" type="primary" @click="router.push(`/reports/${done.reportId}`)">
              查看报告
            </a-button>
          </div>

          <div v-if="!progress && !done" class="fm-card tips-card">
            <div class="tips-title"><InfoCircleOutlined /> 压测前请确认</div>
            <ul>
              <li>只对你有权限的测试环境压测，避免影响线上流量。</li>
              <li>并发数不要一次拉满，先小并发观察错误率与 P95 延迟。</li>
              <li>配置了限流或防刷策略的接口，压测结果会包含大量 429/403。</li>
            </ul>
          </div>
        </section>
      </div>

      <div v-else-if="!loading" class="fm-card guide-empty">
        <div class="empty-title">没有找到这个请求</div>
        <div class="empty-desc">请求可能已经被删除，请回到项目重新选择。</div>
        <a-button type="primary" @click="openConfig">重新选择接口</a-button>
      </div>
    </a-spin>

    <a-modal
      v-model:open="configOpen"
      title="新建压测"
      :width="1040"
      :mask-closable="false"
      class="stress-config-modal"
      ok-text="开始压测"
      cancel-text="取消"
      :confirm-loading="preparing"
      @ok="startFromConfig"
    >
      <div class="modal-body">
        <div class="modal-section plan-section">
          <div class="section-title">
            <span class="step">0</span>压测计划
            <span class="section-sub">Plan</span>
          </div>
          <div class="plan-row">
            <a-select
              :value="activePlanId"
              size="large"
              allow-clear
              placeholder="载入已保存的压测计划"
              class="plan-select"
              :options="plans.map((item) => ({ value: item.id, label: `${item.name}${item.requestName ? ` · ${item.requestName}` : ''}` }))"
              @change="applyPlan"
            />
            <a-input
              v-model:value="planName"
              size="large"
              class="plan-name"
              placeholder="计划名称（留空自动命名）"
            />
            <a-button size="large" :loading="planSaving" :disabled="!targetProjectId" @click="savePlan">
              <template #icon><SaveOutlined /></template>
              保存计划
            </a-button>
            <a-button
              v-if="activePlanId"
              size="large"
              danger
              type="text"
              @click="removePlan"
            >
              <template #icon><DeleteOutlined /></template>
              删除
            </a-button>
          </div>
        </div>

        <div class="modal-section">
          <div class="section-title">
            <span class="step">1</span>选择目标接口
            <span class="section-sub">Target</span>
          </div>
          <div class="target-grid">
            <div class="field">
              <div class="label">项目</div>
              <a-select
                v-model:value="targetProjectId"
                :options="projectOptions"
                placeholder="选择项目"
                size="large"
                show-search
                option-filter-prop="label"
                popup-class-name="stress-select-dropdown"
              />
            </div>
            <div class="field">
              <div class="label">环境变量</div>
              <a-select
                v-model:value="modalEnvId"
                size="large"
                allow-clear
                placeholder="不使用环境变量"
                popup-class-name="stress-select-dropdown"
              >
                <a-select-option v-for="env in modalEnvironments" :key="env.id" :value="env.id">
                  {{ env.name }}
                </a-select-option>
              </a-select>
            </div>
          </div>
          <div class="field">
            <div class="label">
              接口
              <span class="label-hint">选中后自动带入 Headers、Params、Body 与变量</span>
            </div>
            <a-select
              v-model:value="targetRequestId"
              :options="requestSelectOptions"
              :loading="choicesLoading"
              placeholder="选择要压测的接口"
              size="large"
              show-search
              option-filter-prop="label"
              popup-class-name="stress-select-dropdown"
              :not-found-content="choicesLoading ? '加载中…' : '该项目下暂无接口'"
            />
          </div>
          <div v-if="selectedChoice" class="target-preview">
            <div class="target-preview-top">
              <span class="method-tag" :class="`method-${selectedChoice.method}`">{{ selectedChoice.method }}</span>
              <span class="target-preview-name">{{ selectedChoice.name }}</span>
              <span class="target-preview-collection">{{ selectedChoice.collectionName }}</span>
            </div>
            <div class="target-url mono">{{ selectedChoice.url }}</div>
          </div>
          <div v-else-if="currentProject" class="hint">
            {{ currentProject.name }} · 共 {{ choices.length }} 个接口
          </div>
        </div>

        <div class="modal-section">
          <div class="section-title">
            <span class="step">2</span>配置压测参数
            <span class="section-sub">JMeter 风格</span>
          </div>
          <div class="modal-fields">
            <!-- 线程组 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">线程组</span>
                <span class="param-sub">Thread Group</span>
              </div>
              <div class="param-body">
                <div class="field">
                  <div class="label">加压模式</div>
                  <a-segmented
                    v-model:value="settings.mode"
                    :options="[
                      { label: '固定并发', value: 'fixed' },
                      { label: '阶梯加压', value: 'stages' },
                    ]"
                    block
                  />
                  <div class="hint">
                    {{ settings.mode === 'fixed'
                      ? '固定线程数持续压测，适合验证稳定吞吐'
                      : '按阶段逐步提升并发，用于寻找性能拐点' }}
                  </div>
                </div>

                <template v-if="settings.mode === 'fixed'">
                  <div class="field-row">
                    <div class="field">
                      <div class="label">线程数（并发）</div>
                      <a-input-number
                        v-model:value="settings.concurrent"
                        :min="1"
                        :max="500"
                        size="large"
                        style="width: 100%"
                      />
                      <div class="hint">建议从 10～30 起步，观察错误率后再提高</div>
                    </div>
                    <div class="field">
                      <div class="label">持续时间（秒）</div>
                      <a-input-number
                        v-model:value="settings.durationSec"
                        :min="1"
                        :max="3600"
                        size="large"
                        style="width: 100%"
                      />
                    </div>
                  </div>
                  <div class="field">
                    <div class="label">Ramp-Up 时间（秒）</div>
                    <a-input-number
                      v-model:value="settings.rampUpSec"
                      :min="0"
                      :max="600"
                      size="large"
                      style="width: 100%"
                    />
                    <div class="hint">并发从 0 逐步升到目标值的时间，需小于持续时间</div>
                  </div>
                </template>

                <template v-else>
                  <div class="field">
                    <div class="label stage-label">
                      <span>加压阶段</span>
                      <a-button size="small" type="link" @click="addStage">+ 添加阶段</a-button>
                    </div>
                    <div class="stage-list">
                      <div class="stage-row stage-header">
                        <span />
                        <span>并发数</span>
                        <span>时长（秒）</span>
                        <span />
                      </div>
                      <div v-for="(stage, index) in stages" :key="index" class="stage-row">
                        <span class="stage-index">{{ index + 1 }}</span>
                        <a-input-number v-model:value="stage.concurrent" :min="1" :max="500" />
                        <a-input-number v-model:value="stage.durationSec" :min="1" :max="3600" />
                        <a-button size="small" type="text" danger :disabled="stages.length <= 1" @click="removeStage(index)">
                          移除
                        </a-button>
                      </div>
                    </div>
                    <div class="hint">共 {{ totalSeconds }} 秒，峰值并发 {{ peakConcurrent }}</div>
                  </div>
                </template>
              </div>
            </div>

            <!-- HTTP 请求默认值 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">HTTP 请求默认值</span>
                <span class="param-sub">HTTP Request Defaults</span>
              </div>
              <div class="param-body">
                <div class="field-row">
                  <div class="field">
                    <div class="label">连接超时（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.connectTimeoutMs"
                      :min="500"
                      :max="120000"
                      :step="1000"
                      size="large"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">请求超时（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.timeoutMs"
                      :min="500"
                      :max="120000"
                      :step="1000"
                      size="large"
                      style="width: 100%"
                    />
                  </div>
                </div>
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.followRedirects">跟随 3xx 重定向</a-checkbox>
                  <a-checkbox v-model:checked="settings.insecure">忽略证书校验（自签证书）</a-checkbox>
                </div>
              </div>
            </div>

            <!-- 流量与节奏 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">流量与节奏</span>
                <span class="param-sub">Timer &amp; Throughput</span>
              </div>
              <div class="param-body">
                <div class="field-row">
                  <div class="field">
                    <div class="label">思考时间（毫秒）</div>
                    <a-input-number
                      v-model:value="settings.thinkTimeMs"
                      :min="0"
                      :max="60000"
                      :step="100"
                      size="large"
                      style="width: 100%"
                    />
                    <div class="hint">每个线程两次请求之间的固定间隔</div>
                  </div>
                  <div class="field">
                    <div class="label">目标 RPS 限速</div>
                    <a-input-number
                      v-model:value="settings.targetRps"
                      :min="0"
                      :max="100000"
                      :step="10"
                      size="large"
                      style="width: 100%"
                    />
                    <div class="hint">0 表示不限速；每秒最多发出该数量的请求</div>
                  </div>
                </div>
              </div>
            </div>

            <!-- 自动停止 -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">自动停止</span>
                <span class="param-sub">Auto Stop</span>
              </div>
              <div class="param-body">
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.stopOnErrorRate">错误率超过阈值时提前停止</a-checkbox>
                </div>
                <div v-if="settings.stopOnErrorRate" class="field-row">
                  <div class="field">
                    <div class="label">错误率阈值（%）</div>
                    <a-input-number
                      v-model:value="settings.stopErrorRate"
                      :min="0.1"
                      :max="100"
                      :step="0.5"
                      size="large"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">最少请求数</div>
                    <a-input-number
                      v-model:value="settings.stopMinRequests"
                      :min="1"
                      :max="1000000"
                      :step="100"
                      size="large"
                      style="width: 100%"
                    />
                    <div class="hint">达到该请求数后才开始判定，避免起步阶段误停</div>
                  </div>
                </div>
              </div>
            </div>

            <!-- SLA -->
            <div class="param-card">
              <div class="param-head">
                <span class="param-title">SLA 验收</span>
                <span class="param-sub">Service Level</span>
              </div>
              <div class="param-body">
                <div class="check-row">
                  <a-checkbox v-model:checked="settings.slaEnabled">设置 SLA 验收阈值</a-checkbox>
                </div>
                <div v-if="settings.slaEnabled" class="sla-grid">
                  <div class="field">
                    <div class="label">P95 ≤（ms）</div>
                    <a-input-number v-model:value="settings.slaP95" :min="0" :max="120000" size="large" style="width: 100%" />
                  </div>
                  <div class="field">
                    <div class="label">P99 ≤（ms）</div>
                    <a-input-number v-model:value="settings.slaP99" :min="0" :max="120000" size="large" style="width: 100%" />
                  </div>
                  <div class="field">
                    <div class="label">错误率 ≤（%）</div>
                    <a-input-number
                      v-model:value="settings.slaErrorRate"
                      :min="0"
                      :max="100"
                      :step="0.5"
                      size="large"
                      style="width: 100%"
                    />
                  </div>
                  <div class="field">
                    <div class="label">平均 RPS ≥</div>
                    <a-input-number v-model:value="settings.slaMinRps" :min="0" :max="100000" size="large" style="width: 100%" />
                  </div>
                </div>
                <div v-if="settings.slaEnabled" class="hint">报告生成时会逐项判定并给出是否达标</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <div class="modal-foot">
          <span class="modal-foot-hint">
            <InfoCircleOutlined /> 开始后立即进入实时监控，离开本页会自动停止压测
          </span>
          <div class="modal-foot-actions">
            <a-button @click="configOpen = false">取消</a-button>
            <a-button type="primary" :loading="preparing" @click="startFromConfig">
              <template #icon><PlayCircleOutlined /></template>
              开始压测
            </a-button>
          </div>
        </div>
      </template>
    </a-modal>
  </div>
</template>

<style scoped>
.layout {
  display: grid;
  grid-template-columns: 330px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}

/* ---------- 配置 ---------- */
.config {
  position: sticky;
  top: 20px;
  max-height: calc(100vh - 40px);
  overflow-y: auto;
}

.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 46px;
  padding: 0 16px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 13px;
  font-weight: 600;
}

.pane-tag {
  font-size: 10px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  padding: 2px 8px;
  border-radius: 999px;
  font-weight: 500;
}

.target {
  padding: 14px 16px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.target-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.target-name {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.target-url {
  margin-top: 8px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-2);
  word-break: break-all;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: 7px;
  padding: 7px 9px;
}

.target-meta {
  margin-top: 8px;
  font-size: 11px;
  color: var(--fm-muted);
}

.form {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.field-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.label {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
  margin-bottom: 6px;
}

.hint {
  margin-top: 5px;
  font-size: 10px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.config-foot {
  padding: 0 16px 16px;
}

.leave-hint {
  margin-top: 9px;
  text-align: center;
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 实时面板 ---------- */
.live {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-width: 0;
}

.status-card {
  padding: 16px 18px 18px;
}

.status-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.status-left {
  display: flex;
  align-items: center;
  gap: 9px;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #cbd5e1;
}

.status-dot.live {
  background: var(--fm-success);
  box-shadow: 0 0 0 4px rgba(18, 183, 106, 0.14);
  animation: pulse 1.6s infinite;
}

.status-dot.done {
  background: var(--fm-primary);
}

@keyframes pulse {
  0% {
    box-shadow: 0 0 0 0 rgba(18, 183, 106, 0.32);
  }
  70% {
    box-shadow: 0 0 0 7px rgba(18, 183, 106, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(18, 183, 106, 0);
  }
}

.status-text {
  font-size: 13px;
  font-weight: 600;
}

.status-elapsed {
  font-size: 11px;
  color: var(--fm-muted);
}

.status-name {
  font-size: 11px;
  color: var(--fm-muted);
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-stage {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 999px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

/* ---------- 阶梯加压阶段 ---------- */
.stage-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.stage-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.stage-row {
  display: grid;
  grid-template-columns: 20px 1fr 1fr auto;
  align-items: center;
  gap: 8px;
}

.stage-header {
  font-size: 10px;
  color: var(--fm-muted);
}

.stage-header span:not(:first-child):not(:last-child) {
  padding-left: 2px;
}

.stage-index {
  width: 20px;
  height: 20px;
  border-radius: 6px;
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* ---------- SLA ---------- */
.sla-block {
  padding: 12px;
  border: 1px dashed var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface-alt);
}

.sla-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 10px;
}

/* ---------- 状态码分布 ---------- */
.status-codes {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed var(--fm-border);
}

.codes-title {
  font-size: 10px;
  color: var(--fm-muted);
}

.status-code-item {
  display: flex;
  align-items: baseline;
  gap: 5px;
  padding: 3px 9px;
  border-radius: 7px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.status-code-item .code {
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.status-code-item .code-ok {
  color: var(--fm-success);
}

.status-code-item .code-info {
  color: var(--fm-primary);
}

.status-code-item .code-warn {
  color: var(--fm-warning);
}

.status-code-item .code-bad {
  color: var(--fm-danger);
}

.status-code-item .count {
  font-size: 11px;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.status-code-item .ratio {
  font-size: 9px;
  color: var(--fm-muted);
}

.stats {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 10px;
  margin-top: 16px;
}

.stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 10px;
  border-radius: 9px;
  background: var(--fm-surface-alt);
  border: 1px solid #f0f3f9;
}

.stat-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.stat-value {
  font-size: 15px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
}

.stat-value i {
  font-style: normal;
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
}

.stat-value.warn {
  color: var(--fm-warning);
}

.stat-value.bad {
  color: var(--fm-danger);
}

.chart-card {
  padding: 14px 18px 6px;
}

.chart-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 2px;
}

.chart-title {
  font-size: 13px;
  font-weight: 600;
}

.chart-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

.result-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px 18px;
  border-color: var(--fm-success);
  background: var(--fm-success-soft);
}

.result-icon {
  width: 38px;
  height: 38px;
  flex-shrink: 0;
  border-radius: 10px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
}

.result-main {
  flex: 1;
  min-width: 0;
}

.result-title {
  font-size: 13px;
  font-weight: 600;
}

.result-desc {
  margin-top: 3px;
  font-size: 11px;
  color: var(--fm-muted);
  line-height: 1.6;
}

.tips-card {
  padding: 16px 18px;
}

.tips-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  margin-bottom: 8px;
}

.tips-card ul {
  margin: 0;
  padding-left: 18px;
}

.tips-card li {
  font-size: 11px;
  line-height: 1.9;
  color: var(--fm-muted);
}

.guide-empty {
  padding: 64px 24px;
  text-align: center;
}

.empty-icon-wrap {
  width: 56px;
  height: 56px;
  margin: 0 auto 16px;
  border-radius: 16px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.empty-desc {
  max-width: 420px;
  margin: 0 auto 18px;
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-muted);
}

/* ---------- 新建压测弹窗 ---------- */
.modal-body {
  display: flex;
  flex-direction: column;
  gap: 20px;
  max-height: 74vh;
  overflow-y: auto;
  padding: 4px 8px 4px 2px;
}

.modal-body .label {
  margin-bottom: 7px;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.modal-body .hint {
  margin-top: 6px;
  font-size: 11px;
}

.label-hint {
  margin-left: 8px;
  font-size: 10.5px;
  font-weight: 400;
  color: var(--fm-muted);
}

.modal-body :deep(.ant-select-lg .ant-select-selector) {
  min-height: 44px;
  padding: 6px 14px;
  font-size: 13.5px;
  align-items: center;
}

.modal-body :deep(.ant-input-number-lg) {
  min-height: 44px;
  font-size: 13.5px;
}

.modal-body :deep(.ant-input-number-lg .ant-input-number-input) {
  height: 42px;
}

.modal-section {
  display: flex;
  flex-direction: column;
  gap: 13px;
}

.section-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
}

.section-sub {
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.4px;
  color: var(--fm-muted);
  text-transform: uppercase;
}

.step {
  width: 20px;
  height: 20px;
  border-radius: 7px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.target-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}

.plan-row {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}

.plan-row .plan-select {
  flex: 1 1 240px;
  min-width: 200px;
}

.plan-row .plan-name {
  flex: 1 1 180px;
  min-width: 160px;
}

.plan-section {
  padding-bottom: 18px;
  border-bottom: 1px dashed var(--fm-border);
}

.target-grid .field,
.modal-section > .field {
  min-width: 0;
}

:global(.stress-select-dropdown) {
  min-width: 260px;
}

:global(.stress-select-dropdown .ant-select-item) {
  min-height: 38px;
  padding: 9px 12px;
  font-size: 13px;
}

:global(.stress-select-dropdown .ant-select-item-option-content) {
  white-space: normal;
  word-break: break-all;
  line-height: 1.5;
}

.target-preview {
  padding: 13px 15px;
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface-alt);
}

.target-preview-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.target-preview-name {
  font-size: 12.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.target-preview-collection {
  margin-left: auto;
  flex-shrink: 0;
  font-size: 10px;
  color: var(--fm-muted);
}

.modal-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 参数分组卡片 */
.param-card {
  border: 1px solid var(--fm-border);
  border-radius: 11px;
  background: var(--fm-surface);
  overflow: hidden;
}

.param-head {
  display: flex;
  align-items: baseline;
  gap: 9px;
  padding: 10px 15px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
}

.param-title {
  font-size: 12.5px;
  font-weight: 600;
}

.param-sub {
  font-size: 9.5px;
  letter-spacing: 0.4px;
  color: var(--fm-muted);
  text-transform: uppercase;
}

.param-body {
  display: flex;
  flex-direction: column;
  gap: 13px;
  padding: 15px;
}

.check-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 24px;
}

.check-row :deep(.ant-checkbox-wrapper) {
  font-size: 12.5px;
}

.modal-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.modal-foot-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--fm-muted);
}

.modal-foot-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

@media (max-width: 720px) {
  .target-grid,
  .field-row,
  .sla-grid {
    grid-template-columns: 1fr;
  }
  .modal-foot {
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
  }
}

@media (max-width: 1080px) {
  .layout {
    grid-template-columns: 1fr;
  }
  .config {
    position: static;
  }
  .stats {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}
</style>
