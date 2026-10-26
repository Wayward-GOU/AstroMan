<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import {
  PlusOutlined,
  ArrowRightOutlined,
  ThunderboltOutlined,
  DeploymentUnitOutlined,
  FolderOpenOutlined,
  DatabaseOutlined,
  ImportOutlined,
  DownOutlined,
  ToolOutlined,
  LineChartOutlined,
  FundProjectionScreenOutlined,
  ClockCircleOutlined,
  ReloadOutlined,
  SettingOutlined,
  ApiOutlined,
  SafetyCertificateOutlined,
  RocketOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { formatDateTime } from '@/utils/datetime'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import type {
  HistoryItem,
  MonitorSnapshot,
  StorageInfo,
  StressReport,
  WorkspaceStats,
} from '@/types/models'

const router = useRouter()
const store = useProjectStore()

const loading = ref(true)
const createOpen = ref(false)
const newName = ref('')
const newDesc = ref('')
const submitting = ref(false)

const history = ref<HistoryItem[]>([])
const reports = ref<StressReport[]>([])
const stats = ref<WorkspaceStats | null>(null)
const storage = ref<StorageInfo | null>(null)
const snapshot = ref<MonitorSnapshot | null>(null)
const monitorLoading = ref(false)
const appVersion = ref('')

const recentProject = computed(() => {
  if (!store.projects.length) return null
  return [...store.projects].sort((a, b) => b.updatedAt - a.updatedAt)[0]
})

const headerSubtitle = computed(() =>
  stats.value
    ? `${stats.value.projectCount} 个项目 · ${stats.value.requestCount} 个接口 · 数据全部保存在本机`
    : '本地 API 调试、压测与工具箱，数据不出这台电脑',
)

const overallRate = computed(() => {
  const s = stats.value
  if (!s || s.reportRequestTotal === 0) return null
  return (s.reportSuccessTotal / s.reportRequestTotal) * 100
})

const kpis = computed(() => {
  const s = stats.value
  const project = recentProject.value
  const openProject = () => {
    if (project) router.push(`/project/${project.id}`)
    else createOpen.value = true
  }
  return [
    {
      key: 'project',
      label: '项目',
      value: s?.projectCount ?? store.projects.length,
      note: '按项目组织接口',
      go: openProject,
    },
    {
      key: 'request',
      label: '接口',
      value: s?.requestCount ?? null,
      note: s ? `分布在 ${s.collectionCount} 个集合` : '正在统计',
      go: openProject,
    },
    {
      key: 'collection',
      label: '集合',
      value: s?.collectionCount ?? null,
      note: '支持无限嵌套',
      go: openProject,
    },
    {
      key: 'environment',
      label: '环境',
      value: s?.environmentCount ?? null,
      note: '变量与密钥管理',
      go: goEnvironment,
    },
    {
      key: 'report',
      label: '压测报告',
      value: s?.reportCount ?? null,
      note: s ? `近 7 天新增 ${s.recentReports} 份` : '结束后自动生成',
      go: () => router.push('/reports'),
    },
    {
      key: 'history',
      label: '历史请求',
      value: s?.historyCount ?? null,
      note: s ? `近 7 天 ${s.recentHistory} 次` : '本机保留最近 5000 条',
      go: () => router.push('/history'),
    },
  ]
})

const launches = computed(() => [
  {
    key: 'stress',
    title: '发起压测',
    desc: '选择接口，配置并发、时长与 SLA',
    icon: ThunderboltOutlined,
    go: () => router.push('/stress'),
  },
  {
    key: 'reports',
    title: '压测报告',
    desc: '分位、阶段汇总与逐请求明细',
    icon: FundProjectionScreenOutlined,
    go: () => router.push('/reports'),
  },
  {
    key: 'tools',
    title: '工具箱',
    desc: 'JSON、加解密、时间戳等离线工具',
    icon: ToolOutlined,
    go: () => router.push('/tools'),
  },
  {
    key: 'monitor',
    title: '系统监视',
    desc: 'CPU、内存、磁盘与本机 IP',
    icon: LineChartOutlined,
    go: () => router.push('/monitor'),
  },
  {
    key: 'environment',
    title: '环境变量',
    desc: '统一管理 base_url 与 token',
    icon: DeploymentUnitOutlined,
    go: goEnvironment,
  },
  {
    key: 'settings',
    title: '数据与设置',
    desc: '存储位置、主题与版本信息',
    icon: SettingOutlined,
    go: goSettings,
  },
])

const toolLinks = [
  { id: 'json-workbench', label: 'JSON 格式化' },
  { id: 'json2java', label: 'JSON → Java' },
  { id: 'timestamp', label: '时间戳转换' },
  { id: 'mybatis', label: 'MyBatis SQL' },
  { id: 'regex', label: '正则测试' },
  { id: 'ip-lookup', label: 'IP 查询' },
]

const features = computed(() => [
  {
    key: 'debug',
    icon: ApiOutlined,
    title: '接口调试',
    desc: '类 Postman 的请求编辑器，方法、URL、环境变量与发送按钮集中在一条地址栏，双栏对照查看参数与响应。',
    points: ['Params / Headers / Body', '{{变量}} 与内置函数', '前后置脚本与运行历史'],
    go: goProject,
  },
  {
    key: 'stress',
    icon: ThunderboltOutlined,
    title: '本地压测',
    desc: '参考 JMeter 的参数模型，支持固定并发与阶梯加压、目标 RPS 限速、SLA 阈值与自动停止。',
    points: ['并发 / 时长 / 思考时间', 'P95 · P99 · 错误率', '逐请求明细可导出'],
    go: () => router.push('/stress'),
  },
  {
    key: 'toolbox',
    icon: ToolOutlined,
    title: '离线工具箱',
    desc: '日常开发常用工具集中在一处，全部在本地运行，不发起外部请求，也不写入压测或请求历史。',
    points: ['JSON 转换与格式化', '加密编码 / 时间戳', 'MyBatis 日志填充'],
    go: () => router.push('/tools'),
  },
  {
    key: 'privacy',
    icon: SafetyCertificateOutlined,
    title: '数据留在本机',
    desc: '项目、接口、环境变量、历史与压测报告全部写入本机 SQLite，无需账号，断网也能完整使用。',
    points: ['零云端依赖', '自定义存储位置', 'JSON / HTML / Excel 归档'],
    go: goSettings,
  },
])

const highlights = computed(() => {
  const s = stats.value
  return [
    { label: '累计发送请求', value: s ? fmtInt(s.historyCount) : '—' },
    { label: '压测请求总数', value: s ? fmtInt(s.reportRequestTotal) : '—' },
    { label: '压测报告', value: s ? fmtInt(s.reportCount) : '—' },
    { label: '环境变量组', value: s ? fmtInt(s.environmentCount) : '—' },
  ]
})

const steps = computed(() => [
  {
    key: '01',
    title: '创建项目',
    desc: '一个项目对应一组接口，创建时自带默认集合。',
    action: '新建项目',
    go: () => (createOpen.value = true),
  },
  {
    key: '02',
    title: '配置环境',
    desc: '把 base_url、token 写进环境变量，请求里用 {{变量}} 引用。',
    action: '管理环境',
    go: goEnvironment,
  },
  {
    key: '03',
    title: '调试接口',
    desc: '填写 URL、Headers、Body，发送后查看状态码、耗时与响应。',
    action: '打开项目',
    go: goProject,
  },
  {
    key: '04',
    title: '发起压测',
    desc: '配置并发与时长，结束后自动生成报告与逐请求明细。',
    action: '去压测',
    go: () => router.push('/stress'),
  },
])

const faqs = [
  {
    q: '数据保存在哪里？会上传到云端吗？',
    a: '不会。项目、请求、环境变量、历史与压测报告都保存在本机 SQLite 文件中，设置页可以查看数据库路径，也可以把数据目录迁移到其他盘。',
  },
  {
    q: '请求里的 {{变量}} 是怎么替换的？',
    a: '先选一个环境，再用 {{变量名}} 引用环境变量。还支持内置函数：{{timestamp}}、{{uuid}}、{{randomInt(1,100)}}、{{randomString(8)}}。',
  },
  {
    q: '压测并发数开多少合适？',
    a: '取决于目标服务与本机性能。建议从 10～30 并发、30 秒时长起步，观察错误率与延迟后再逐步提高；压测页提供固定并发与阶梯加压两种模式。',
  },
  {
    q: '压测报告可以导出吗？',
    a: '可以。报告详情页支持导出 JSON（完整归档）、HTML（可浏览器查看或打印为 PDF）与 Excel（多工作表，含逐请求明细），方便归档和分享。',
  },
  {
    q: '压测把接口打挂了怎么办？',
    a: '压测中随时可以点「停止」，已产生的数据仍会保存成报告。建议只在测试环境压测，并提前确认目标服务的限流策略。',
  },
]

onMounted(async () => {
  try {
    await store.loadProjects()
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }

  const [his, reps, st, storageInfo, info] = await Promise.allSettled([
    api.historyList(8),
    api.reportList(4),
    api.workspaceStats(),
    api.storageInfo(),
    api.appInfo(),
  ])
  if (his.status === 'fulfilled') history.value = his.value
  if (reps.status === 'fulfilled') reports.value = reps.value
  if (st.status === 'fulfilled') stats.value = st.value
  if (storageInfo.status === 'fulfilled') storage.value = storageInfo.value
  if (info.status === 'fulfilled') appVersion.value = info.value.version

  await loadMonitor()
})

async function loadMonitor() {
  monitorLoading.value = true
  try {
    snapshot.value = await api.winstatsSnapshot()
  } catch (e) {
    message.error(String(e))
  } finally {
    monitorLoading.value = false
  }
}

const diskWorst = computed(() => {
  const disks = snapshot.value?.disks ?? []
  if (!disks.length) return null
  return disks.reduce((a, b) => (b.percent > a.percent ? b : a))
})

const primaryIp = computed(() => {
  const ips = snapshot.value?.ips ?? []
  return ips.find((item) => item.primary)?.address ?? ips[0]?.address ?? '-'
})

const gpuPrimary = computed(() => snapshot.value?.gpus?.[0] ?? null)

/* ---------- 创建 / 删除 / 导入 ---------- */

async function createProject() {
  if (!newName.value.trim()) {
    message.warning('请填写项目名称')
    return
  }
  submitting.value = true
  try {
    const p = await api.projectCreate(newName.value.trim(), newDesc.value.trim() || undefined)
    createOpen.value = false
    newName.value = ''
    newDesc.value = ''
    await store.loadProjects()
    refreshStats()
    router.push(`/project/${p.id}`)
  } catch (e) {
    message.error(String(e))
  } finally {
    submitting.value = false
  }
}

async function refreshStats() {
  try {
    stats.value = await api.workspaceStats()
  } catch {
    /* 统计失败不影响主流程 */
  }
}

function removeProject(id: number, name: string) {
  Modal.confirm({
    title: `删除项目「${name}」？`,
    content: '项目下的集合、请求与运行记录将一并删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.projectDelete(id)
      await store.loadProjects()
      refreshStats()
      message.success('已删除')
    },
  })
}

async function importProject() {
  try {
    const path = await open({
      title: '导入 FinkMeter 项目',
      multiple: false,
      filters: [{ name: 'FinkMeter 项目文件', extensions: ['json'] }],
    })
    if (!path) return
    const project = await api.projectImport(path)
    await store.loadProjects()
    refreshStats()
    message.success(`已导入项目「${project.name}」`)
    router.push(`/project/${project.id}`)
  } catch (e) {
    message.error(String(e))
  }
}

async function exportProjectJson(id: number, name: string) {
  const safeName = name.replace(/[\\/:*?"<>|]/g, '_')
  try {
    const path = await save({
      title: '导出项目 JSON',
      defaultPath: `${safeName}.finkmeter.json`,
      filters: [{ name: 'FinkMeter 项目文件', extensions: ['json'] }],
    })
    if (!path) return
    await api.projectExport(id, path)
    message.success(`项目「${name}」已导出，可发给他人用「导入项目 JSON」恢复`)
  } catch (e) {
    message.error(String(e))
  }
}

const importingApi = ref(false)

async function importApiProject() {
  try {
    const path = await open({
      title: '导入 Postman / Apifox 数据',
      multiple: false,
      filters: [{ name: 'JSON 文件', extensions: ['json'] }],
    })
    if (!path) return
    importingApi.value = true
    const project = await api.apiImport(path, null)
    await store.loadProjects()
    refreshStats()
    message.success(`已导入「${project.name}」`)
    router.push(`/project/${project.id}`)
  } catch (e) {
    message.error(String(e))
  } finally {
    importingApi.value = false
  }
}

function onImportMenu(info: { key: string | number }) {
  if (info.key === 'finkmeter') importProject()
  else if (info.key === 'api') importApiProject()
}

function clearHistory() {
  Modal.confirm({
    title: '清空全部请求历史？',
    content: '只删除本机记录的请求历史，不会影响项目、接口与压测报告。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.historyClear()
      history.value = []
      refreshStats()
      message.success('历史已清空')
    },
  })
}

/* ---------- 跳转 ---------- */

function goProject() {
  if (recentProject.value) router.push(`/project/${recentProject.value.id}`)
  else createOpen.value = true
}

function goEnvironment() {
  if (recentProject.value) router.push(`/environment/${recentProject.value.id}`)
  else createOpen.value = true
}

function goSettings() {
  router.push('/settings')
}

function openHistory(row: HistoryItem) {
  if (row.requestId) router.push(`/request/${row.requestId}`)
}

function openReport(report: StressReport) {
  router.push(`/reports/${report.id}`)
}

function openTool(id: string) {
  router.push({ path: '/tools', query: { tool: id } })
}

/* ---------- 格式化 ---------- */

function fmtInt(value: number) {
  return Math.round(value).toLocaleString()
}

function fmtBytes(value: number) {
  if (!value) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let size = value
  let index = 0
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024
    index += 1
  }
  return `${size >= 100 || index === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[index]}`
}

function fmtMs(value?: number | null) {
  if (value == null) return '-'
  return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${Math.round(value)} ms`
}

function fmtPercent(value: number) {
  return `${value.toFixed(1)}%`
}

function fmtWhen(value?: number | null) {
  return formatDateTime(value)
}

function reportRate(report: StressReport) {
  return report.totalRequests > 0 ? (report.successCount / report.totalRequests) * 100 : 0
}

function tone(value: number) {
  if (value >= 85) return 'danger'
  if (value >= 65) return 'warn'
  return 'ok'
}

function statusColor(code?: number | null) {
  if (!code) return 'var(--fm-muted)'
  if (code < 300) return 'var(--fm-success)'
  if (code < 400) return 'var(--fm-warning)'
  return 'var(--fm-danger)'
}
</script>

<template>
  <div class="page dashboard">
    <PageHeader eyebrow="Workspace" title="工作台" :subtitle="headerSubtitle">
      <a-dropdown trigger="click">
        <a-button :loading="importingApi">
          <template #icon><ImportOutlined /></template>
          导入项目
          <DownOutlined style="font-size: 10px; margin-left: 4px" />
        </a-button>
        <template #overlay>
          <a-menu @click="onImportMenu">
            <a-menu-item key="finkmeter">导入项目 JSON</a-menu-item>
            <a-menu-item key="api">导入 Postman / Apifox 数据</a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-button @click="router.push('/stress')">
        <template #icon><ThunderboltOutlined /></template>
        发起压测
      </a-button>
      <a-button type="primary" @click="createOpen = true">
        <template #icon><PlusOutlined /></template>
        新建项目
      </a-button>
    </PageHeader>

    <!-- 总览 -->
    <section class="console fm-card">
      <div class="console-main">
        <div class="console-eyebrow">
          <span class="pulse-dot" />
          <span>本地工作区</span>
          <span class="meta-dot" />
          <span class="mono">{{ storage ? fmtBytes(storage.dbSize + storage.walSize) : '—' }}</span>
          <span class="meta-dot" />
          <span>{{ storage?.isCustom ? '自定义数据目录' : '默认数据目录' }}</span>
        </div>
        <h2 class="console-title">
          {{ recentProject ? `继续「${recentProject.name}」` : '创建第一个项目，开始调试' }}
        </h2>
        <p class="console-desc">
          {{
            recentProject
              ? recentProject.description || '项目已就绪，打开后即可编辑接口、发送请求或直接发起压测。'
              : '项目、请求、环境变量与压测报告全部保存在本机 SQLite，无需登录，断网可用。'
          }}
        </p>
        <div class="console-actions">
          <a-button type="primary" @click="goProject">
            <template #icon><FolderOpenOutlined /></template>
            {{ recentProject ? '进入项目' : '新建项目' }}
          </a-button>
          <a-button :disabled="!recentProject" @click="goEnvironment">环境变量</a-button>
          <a-button @click="router.push('/reports')">查看报告</a-button>
        </div>
        <div class="console-meta">
          <DatabaseOutlined />
          <span>FinkMeter v{{ appVersion || '1.0.0' }}</span>
          <span class="meta-dot" />
          <a @click="goSettings">数据位置与设置</a>
        </div>
      </div>

      <div class="kpi-grid">
        <button v-for="item in kpis" :key="item.key" class="kpi" @click="item.go()">
          <span class="kpi-label">{{ item.label }}</span>
          <span class="kpi-value mono">{{ item.value == null ? '—' : fmtInt(item.value) }}</span>
          <span class="kpi-note">{{ item.note }}</span>
        </button>
      </div>
    </section>

    <!-- 快捷入口 -->
    <div class="launch-grid">
      <button v-for="item in launches" :key="item.key" class="launch fm-card" @click="item.go()">
        <span class="launch-icon"><component :is="item.icon" /></span>
        <span class="launch-text">
          <span class="launch-title">{{ item.title }}</span>
          <span class="launch-desc">{{ item.desc }}</span>
        </span>
        <ArrowRightOutlined class="launch-arrow" />
      </button>
    </div>

    <!-- 产品介绍 -->
    <section class="intro-band fm-card">
      <div class="intro-copy">
        <span class="intro-eyebrow">
          <RocketOutlined />
          产品概览
        </span>
        <h2 class="intro-title">调试、压测与工具箱，一个桌面应用全部搞定</h2>
        <p class="intro-desc">
          FinkMeter 面向需要频繁调试 HTTP 接口的开发者：界面参考 Postman，压测参数参考 JMeter，
          日常格式化与转换工具集中在一起。不用在浏览器插件和在线站点之间来回切换，也不用把接口数据交给第三方服务。
        </p>
        <div class="intro-tags">
          <span>本地优先</span>
          <span>无需登录</span>
          <span>断网可用</span>
          <span>Tauri 2 + Rust</span>
          <span>SQLite 存储</span>
        </div>
      </div>
      <div class="intro-stats">
        <div v-for="item in highlights" :key="item.label" class="intro-stat">
          <span class="intro-stat-value mono">{{ item.value }}</span>
          <span class="intro-stat-label">{{ item.label }}</span>
        </div>
      </div>
    </section>

    <!-- 功能能力 -->
    <div class="section-head feature-head">
      <div>
        <h2 class="section-title">FinkMeter 能做什么</h2>
        <div class="section-desc">从接口调试到压力测试，再到日常开发小工具，全部离线可用</div>
      </div>
    </div>
    <div class="feature-grid">
      <div v-for="f in features" :key="f.key" class="fm-card feature" @click="f.go()">
        <div class="feature-top">
          <span class="feature-icon"><component :is="f.icon" /></span>
          <span class="feature-title">{{ f.title }}</span>
        </div>
        <p class="feature-desc">{{ f.desc }}</p>
        <div class="feature-points">
          <span v-for="p in f.points" :key="p">{{ p }}</span>
        </div>
        <span class="feature-go">前往 <ArrowRightOutlined /></span>
      </div>
    </div>

    <!-- 主体 -->
    <div class="main-grid">
      <div class="col-main">
        <div class="section-head">
          <div>
            <h2 class="section-title">我的项目</h2>
            <div class="section-desc">
              {{ store.projects.length }} 个项目<template v-if="stats"> · {{ fmtInt(stats.collectionCount) }} 个集合 · {{ fmtInt(stats.requestCount) }} 个接口</template>
            </div>
          </div>
          <a-button type="link" size="small" @click="createOpen = true">
            <template #icon><PlusOutlined /></template>
            新建项目
          </a-button>
        </div>

        <a-spin :spinning="loading">
          <div v-if="store.projects.length" class="project-grid">
            <div
              v-for="p in store.projects"
              :key="p.id"
              class="fm-card project-card"
              @click="router.push(`/project/${p.id}`)"
            >
              <div class="project-top">
                <span class="dot" :style="{ background: p.color || 'var(--fm-primary)' }" />
                <span class="project-name">{{ p.name }}</span>
                <a-dropdown :trigger="['click']">
                  <a class="more" @click.stop>···</a>
                  <template #overlay>
                    <a-menu>
                      <a-menu-item key="export" @click="exportProjectJson(p.id, p.name)">
                        导出项目 JSON
                      </a-menu-item>
                      <a-menu-item key="delete" danger @click="removeProject(p.id, p.name)">
                        删除项目
                      </a-menu-item>
                    </a-menu>
                  </template>
                </a-dropdown>
              </div>
              <div class="project-desc">{{ p.description || '暂无描述' }}</div>
              <div class="project-foot">
                <span class="project-meta">更新于 {{ fmtWhen(p.updatedAt) }}</span>
                <span class="project-open">进入 <ArrowRightOutlined /></span>
              </div>
            </div>

            <button class="new-project-card" @click="createOpen = true">
              <PlusOutlined />
              <span>新建项目</span>
            </button>
          </div>
          <div v-else class="fm-card panel-empty">
            <div class="empty-mark">📁</div>
            <div class="empty-title">还没有项目</div>
            <div class="empty-desc">创建一个项目开始调试，或者导入 FinkMeter 项目、Postman 集合或 Apifox 数据。</div>
            <div class="empty-actions">
              <a-button type="primary" @click="createOpen = true">新建项目</a-button>
              <a-button @click="importProject">导入项目 JSON</a-button>
              <a-button @click="importApiProject">导入 Postman/Apifox</a-button>
            </div>
          </div>
        </a-spin>
      </div>

      <aside class="col-side">
        <!-- 最近压测 -->
        <div class="side-card fm-card">
          <div class="side-head">
            <span class="side-title">最近压测</span>
            <span v-if="overallRate != null" class="side-note mono">{{ overallRate.toFixed(1) }}% 成功率</span>
            <a class="side-link" @click="router.push('/reports')">全部</a>
          </div>
          <div v-if="reports.length" class="report-list">
            <div v-for="r in reports" :key="r.id" class="report-row" @click="openReport(r)">
              <div class="report-row-top">
                <span class="status-dot" :class="{ stopped: r.status === 'stopped' }" />
                <span class="report-name">{{ r.planName }}</span>
                <span class="report-when">{{ fmtWhen(r.createdAt) }}</span>
              </div>
              <div class="report-row-metrics">
                <span>请求 <b class="mono">{{ fmtInt(r.totalRequests) }}</b></span>
                <span>P95 <b class="mono">{{ fmtMs(r.p95Rt) }}</b></span>
                <span>错误率 <b class="mono">{{ fmtPercent(r.errorRate) }}</b></span>
              </div>
            </div>
          </div>
          <div v-else class="side-empty">
            还没有压测报告，选择接口发起一次压测即可自动生成。
            <a class="side-link" @click="router.push('/stress')">去压测</a>
          </div>
        </div>

        <!-- 本机状态 -->
        <div class="side-card fm-card">
          <div class="side-head">
            <span class="side-title">本机状态</span>
            <a-button type="text" size="small" class="icon-btn" :loading="monitorLoading" @click="loadMonitor">
              <template #icon><ReloadOutlined /></template>
            </a-button>
          </div>
          <template v-if="snapshot">
            <div class="metric">
              <div class="metric-head">
                <span>CPU</span>
                <span class="mono">{{ fmtPercent(snapshot.cpu.total) }}</span>
              </div>
              <div class="metric-bar">
                <div class="metric-fill" :class="tone(snapshot.cpu.total)" :style="{ width: `${snapshot.cpu.total}%` }" />
              </div>
            </div>
            <div class="metric">
              <div class="metric-head">
                <span>内存</span>
                <span class="mono">{{ fmtPercent(snapshot.memory.percent) }}</span>
              </div>
              <div class="metric-bar">
                <div class="metric-fill" :class="tone(snapshot.memory.percent)" :style="{ width: `${snapshot.memory.percent}%` }" />
              </div>
            </div>
            <div v-if="diskWorst" class="metric">
              <div class="metric-head">
                <span>磁盘 · {{ diskWorst.name || diskWorst.mount }}</span>
                <span class="mono">{{ fmtPercent(diskWorst.percent) }}</span>
              </div>
              <div class="metric-bar">
                <div class="metric-fill" :class="tone(diskWorst.percent)" :style="{ width: `${diskWorst.percent}%` }" />
              </div>
            </div>
            <div v-if="gpuPrimary" class="metric">
              <div class="metric-head">
                <span>GPU</span>
                <span class="mono">{{ fmtPercent(gpuPrimary.usage) }}</span>
              </div>
              <div class="metric-bar">
                <div class="metric-fill" :class="tone(gpuPrimary.usage)" :style="{ width: `${gpuPrimary.usage}%` }" />
              </div>
            </div>
            <div class="monitor-foot">
              <span class="mono">{{ primaryIp }}</span>
              <a class="side-link" @click="router.push('/monitor')">详细监视</a>
            </div>
          </template>
          <div v-else class="side-empty">正在读取本机状态…</div>
        </div>

        <!-- 常用工具 -->
        <div class="side-card fm-card">
          <div class="side-head">
            <span class="side-title">常用工具</span>
            <a class="side-link" @click="router.push('/tools')">工具箱</a>
          </div>
          <div class="tool-chips">
            <button v-for="tool in toolLinks" :key="tool.id" class="tool-chip" @click="openTool(tool.id)">
              {{ tool.label }}
            </button>
          </div>
        </div>
      </aside>
    </div>

    <!-- 最近请求 -->
    <div id="recent-history" class="section-head">
      <div>
        <h2 class="section-title">最近请求</h2>
        <div class="section-desc">每次发送自动记录，最多保留最近 5000 条</div>
      </div>
      <div v-if="history.length" class="head-actions">
        <a-button size="small" @click="clearHistory">清空历史</a-button>
        <a-button size="small" @click="router.push('/history')">查看全部</a-button>
        <a-button type="link" size="small" @click="goProject">去发送请求</a-button>
      </div>
    </div>

    <div v-if="history.length" class="fm-card history-card">
      <div class="history-head">
        <span>方法</span>
        <span>接口</span>
        <span>URL</span>
        <span class="right">状态</span>
        <span class="right">耗时</span>
        <span class="right">时间</span>
      </div>
      <div
        v-for="h in history"
        :key="h.id"
        class="history-row"
        :class="{ clickable: h.requestId }"
        @click="openHistory(h)"
      >
        <span class="method-tag" :class="`method-${h.method}`">{{ h.method }}</span>
        <span class="history-name" :title="h.requestName ?? ''">{{ h.requestName || '—' }}</span>
        <span class="history-url mono" :title="h.url">{{ h.url }}</span>
        <span class="history-status mono right" :style="{ color: statusColor(h.statusCode) }">
          {{ h.statusCode ?? 'ERR' }}
        </span>
        <span class="history-duration mono right">{{ fmtMs(h.durationMs) }}</span>
        <span class="history-when right">{{ fmtWhen(h.createdAt) }}</span>
      </div>
    </div>
    <div v-else class="fm-card history-empty">
      <ClockCircleOutlined class="empty-clock" />
      <div>
        <div class="empty-title">还没有发送过请求</div>
        <div class="empty-desc">进入项目后点「发送」，这里就会记录每次请求的时间、状态与耗时。</div>
      </div>
      <a-button size="small" class="history-empty-btn" @click="goProject">打开项目</a-button>
    </div>

    <!-- 上手引导 -->
    <div class="section-head">
      <div>
        <h2 class="section-title">使用说明</h2>
        <div class="section-desc">四步走完整流程，常见问题也在下方</div>
      </div>
    </div>

    <div class="help-grid">
      <div class="fm-card steps-card">
        <div v-for="step in steps" :key="step.key" class="step-row">
          <span class="step-index mono">{{ step.key }}</span>
          <div class="step-body">
            <div class="step-title">{{ step.title }}</div>
            <div class="step-desc">{{ step.desc }}</div>
          </div>
          <a class="step-link" @click="step.go()">{{ step.action }}<ArrowRightOutlined /></a>
        </div>
      </div>

      <div class="fm-card faq-card">
        <a-collapse :bordered="false" ghost>
          <a-collapse-panel v-for="(f, i) in faqs" :key="i" :header="f.q">
            <p class="faq-answer">{{ f.a }}</p>
          </a-collapse-panel>
        </a-collapse>
      </div>
    </div>

    <a-modal
      v-model:open="createOpen"
      title="新建项目"
      :confirm-loading="submitting"
      ok-text="创建"
      cancel-text="取消"
      @ok="createProject"
    >
      <a-form layout="vertical" style="margin-top: 12px">
        <a-form-item label="项目名称" required>
          <a-input v-model:value="newName" placeholder="例如：订单服务" allow-clear @press-enter="createProject" />
        </a-form-item>
        <a-form-item label="描述">
          <a-textarea v-model:value="newDesc" :rows="3" placeholder="可选，写一句项目用途" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<style scoped>
/* ---------- 总览 ---------- */
.console {
  display: grid;
  grid-template-columns: minmax(0, 1.15fr) minmax(0, 1fr);
  overflow: hidden;
  background:
    radial-gradient(120% 150% at 0% 0%, var(--fm-primary-soft) 0%, transparent 58%),
    var(--fm-surface);
}

.console-main {
  padding: 22px 24px;
  min-width: 0;
}

.console-eyebrow {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  color: var(--fm-muted);
}

.pulse-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--fm-success);
  box-shadow: 0 0 0 0 rgba(63, 127, 95, 0.35);
  animation: fm-pulse 2.6s ease-out infinite;
}

@keyframes fm-pulse {
  0% {
    box-shadow: 0 0 0 0 rgba(63, 127, 95, 0.3);
  }
  70% {
    box-shadow: 0 0 0 7px rgba(63, 127, 95, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(63, 127, 95, 0);
  }
}

.meta-dot {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.console-title {
  margin: 14px 0 8px;
  font-size: 20px;
  font-weight: 650;
  letter-spacing: -0.2px;
  line-height: 1.35;
}

.console-desc {
  margin: 0;
  max-width: 520px;
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--fm-muted);
}

.console-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 18px;
}

.console-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 16px;
  font-size: 11px;
  color: var(--fm-muted);
}

.console-meta a {
  color: var(--fm-primary);
}

.kpi-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  border-left: 1px solid var(--fm-border);
}

.kpi {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  padding: 16px 18px;
  border: none;
  border-bottom: 1px solid var(--fm-border);
  border-right: 1px solid var(--fm-border);
  background: transparent;
  text-align: left;
  cursor: pointer;
  transition: background 0.16s;
}

.kpi:nth-child(3n) {
  border-right: none;
}

.kpi:nth-child(n + 4) {
  border-bottom: none;
}

.kpi:hover {
  background: var(--fm-surface-alt);
}

.kpi-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.kpi-value {
  font-size: 23px;
  font-weight: 600;
  line-height: 1.25;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.kpi-note {
  font-size: 10.5px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

/* ---------- 快捷入口 ---------- */
.launch-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(206px, 1fr));
  gap: 12px;
  margin-top: 14px;
}

.launch {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 13px 15px;
  text-align: left;
  cursor: pointer;
  transition: border-color 0.16s, box-shadow 0.16s, transform 0.16s;
}

.launch:hover {
  border-color: var(--fm-border-strong);
  box-shadow: var(--fm-shadow-lg);
  transform: translateY(-1px);
}

.launch-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  flex-shrink: 0;
  border-radius: 9px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 16px;
}

.launch-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.launch-title {
  font-size: 13px;
  font-weight: 600;
}

.launch-desc {
  font-size: 10.5px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.launch-arrow {
  color: var(--fm-border-strong);
  font-size: 11px;
  transition: color 0.16s, transform 0.16s;
}

.launch:hover .launch-arrow {
  color: var(--fm-primary);
  transform: translateX(1px);
}

/* ---------- 产品介绍 ---------- */
.intro-band {
  display: grid;
  grid-template-columns: minmax(0, 1.55fr) minmax(0, 1fr);
  gap: 24px;
  align-items: center;
  margin-top: 16px;
  padding: 22px 24px;
  background:
    radial-gradient(120% 160% at 100% 0%, var(--fm-primary-soft) 0%, transparent 62%),
    var(--fm-surface);
}

.intro-eyebrow {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.4px;
  color: var(--fm-primary);
}

.intro-title {
  margin: 9px 0 8px;
  font-size: 17px;
  font-weight: 650;
  letter-spacing: -0.2px;
  line-height: 1.4;
}

.intro-desc {
  margin: 0;
  max-width: 640px;
  font-size: 12.5px;
  line-height: 1.8;
  color: var(--fm-muted);
}

.intro-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 14px;
}

.intro-tags span {
  padding: 2px 9px;
  border-radius: 20px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-size: 10.5px;
  color: var(--fm-text-2);
}

.intro-stats {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.intro-stat {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 14px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface);
}

.intro-stat-value {
  font-size: 19px;
  font-weight: 600;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.intro-stat-label {
  font-size: 10.5px;
  color: var(--fm-muted);
}

/* ---------- 功能卡片 ---------- */
.feature-head {
  margin-bottom: 12px;
}

.feature-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(238px, 1fr));
  gap: 12px;
}

.feature {
  display: flex;
  flex-direction: column;
  padding: 16px 17px 14px;
  cursor: pointer;
  transition: border-color 0.16s, box-shadow 0.18s, transform 0.18s;
}

.feature:hover {
  border-color: var(--fm-border-strong);
  box-shadow: var(--fm-shadow-lg);
  transform: translateY(-2px);
}

.feature-top {
  display: flex;
  align-items: center;
  gap: 9px;
}

.feature-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  border-radius: 8px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 15px;
}

.feature-title {
  font-size: 13px;
  font-weight: 600;
}

.feature-desc {
  margin: 10px 0 0;
  flex: 1;
  font-size: 11.5px;
  line-height: 1.75;
  color: var(--fm-muted);
}

.feature-points {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-top: 12px;
}

.feature-points span {
  padding: 2px 7px;
  border-radius: 5px;
  background: var(--fm-surface-alt);
  font-size: 10px;
  color: var(--fm-text-2);
}

.feature-go {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-top: 12px;
  font-size: 10.5px;
  color: var(--fm-primary);
  opacity: 0.55;
  transition: opacity 0.16s;
}

.feature:hover .feature-go {
  opacity: 1;
}

/* ---------- 主体双栏 ---------- */
.main-grid {
  display: grid;
  grid-template-columns: minmax(0, 2.15fr) minmax(280px, 1fr);
  gap: 18px;
  align-items: start;
  margin-top: 2px;
}

.col-main {
  min-width: 0;
}

.col-side {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}

/* ---------- 项目 ---------- */
.project-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(228px, 1fr));
  gap: 12px;
}

.project-card {
  padding: 15px 16px;
  cursor: pointer;
  transition: box-shadow 0.18s, transform 0.18s, border-color 0.18s;
}

.project-card:hover {
  box-shadow: var(--fm-shadow-lg);
  border-color: var(--fm-border-strong);
  transform: translateY(-2px);
}

.project-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.project-name {
  flex: 1;
  font-size: 13.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.more {
  color: var(--fm-muted);
  letter-spacing: 2px;
  padding: 0 2px;
}

.project-desc {
  margin-top: 9px;
  min-height: 36px;
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--fm-muted);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.project-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
}

.project-meta {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.project-open {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 10.5px;
  color: var(--fm-primary);
  opacity: 0;
  transition: opacity 0.18s;
}

.project-card:hover .project-open {
  opacity: 1;
}

.new-project-card {
  min-height: 134px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border-radius: var(--fm-radius);
  border: 1.5px dashed var(--fm-border-strong);
  background: transparent;
  color: var(--fm-muted);
  font-size: 12px;
  cursor: pointer;
  transition: border-color 0.16s, color 0.16s, background 0.16s;
}

.new-project-card:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.panel-empty {
  padding: 42px 24px;
  text-align: center;
}

.empty-mark {
  font-size: 30px;
  margin-bottom: 10px;
}

.empty-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 5px;
}

.empty-desc {
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.empty-actions {
  display: flex;
  justify-content: center;
  gap: 8px;
  margin-top: 16px;
}

/* ---------- 侧栏卡片 ---------- */
.side-card {
  padding: 14px 16px;
}

.side-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.side-title {
  font-size: 12.5px;
  font-weight: 600;
  flex: 1;
}

.side-note {
  font-size: 10.5px;
  color: var(--fm-success);
}

.side-link {
  font-size: 11px;
  color: var(--fm-primary);
  cursor: pointer;
}

.side-empty {
  padding: 12px 0 4px;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.report-row {
  padding: 9px 0;
  border-bottom: 1px solid var(--fm-border);
  cursor: pointer;
}

.report-row:last-child {
  border-bottom: none;
  padding-bottom: 2px;
}

.report-row-top {
  display: flex;
  align-items: center;
  gap: 7px;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--fm-success);
  flex-shrink: 0;
}

.status-dot.stopped {
  background: var(--fm-warning);
}

.report-name {
  flex: 1;
  font-size: 12px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.report-row:hover .report-name {
  color: var(--fm-primary);
}

.report-when {
  font-size: 10px;
  color: var(--fm-muted);
  flex-shrink: 0;
}

.report-row-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin: 5px 0 0 13px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.report-row-metrics b {
  color: var(--fm-text-2);
  font-weight: 600;
}

/* ---------- 本机状态 ---------- */
.icon-btn {
  width: 22px;
  height: 22px;
  padding: 0;
  color: var(--fm-muted);
}

.metric {
  margin-top: 9px;
}

.metric-head {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--fm-muted);
  margin-bottom: 4px;
}

.metric-head .mono {
  color: var(--fm-text-2);
}

.metric-bar {
  height: 4px;
  border-radius: 3px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.metric-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--fm-primary);
  transition: width 0.4s ease;
}

.metric-fill.ok {
  background: var(--fm-primary);
}

.metric-fill.warn {
  background: var(--fm-warning);
}

.metric-fill.danger {
  background: var(--fm-danger);
}

.monitor-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
  font-size: 11px;
  color: var(--fm-text-2);
}

/* ---------- 常用工具 ---------- */
.tool-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 4px 0 2px;
}

.tool-chip {
  padding: 3px 9px;
  border-radius: 6px;
  border: 1px solid var(--fm-border);
  background: transparent;
  color: var(--fm-text-2);
  font-size: 11px;
  cursor: pointer;
  transition: border-color 0.16s, color 0.16s, background 0.16s;
}

.tool-chip:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

/* ---------- 最近请求 ---------- */
.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.history-card {
  overflow: hidden;
}

.history-head {
  display: grid;
  grid-template-columns: 64px 140px minmax(0, 1fr) 64px 80px 108px;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.3px;
  color: var(--fm-muted);
}

.history-row {
  display: grid;
  grid-template-columns: 64px 140px minmax(0, 1fr) 64px 80px 108px;
  align-items: center;
  gap: 12px;
  padding: 9px 16px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11.5px;
  transition: background 0.14s;
}

.history-row:last-child {
  border-bottom: none;
}

.history-row.clickable {
  cursor: pointer;
}

.history-row:hover {
  background: var(--fm-surface-alt);
}

.right {
  text-align: right;
}

.history-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-2);
}

.history-url {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-muted);
}

.history-status,
.history-duration {
  font-weight: 600;
}

.history-when {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.history-empty {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 20px 22px;
}

.empty-clock {
  font-size: 22px;
  color: var(--fm-border-strong);
}

.history-empty .empty-desc {
  margin-top: 3px;
}

.history-empty .empty-title {
  margin-bottom: 0;
}

.history-empty .history-empty-btn {
  margin-left: auto;
}

/* ---------- 使用说明 ---------- */
.help-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 14px;
  align-items: start;
}

.steps-card {
  padding: 6px 18px;
}

.step-row {
  display: flex;
  align-items: center;
  gap: 13px;
  padding: 12px 0;
  border-bottom: 1px solid var(--fm-border);
}

.step-row:last-child {
  border-bottom: none;
}

.step-index {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border-radius: 7px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 10.5px;
  font-weight: 700;
}

.step-body {
  flex: 1;
  min-width: 0;
}

.step-title {
  font-size: 12.5px;
  font-weight: 600;
  margin-bottom: 2px;
}

.step-desc {
  font-size: 11px;
  line-height: 1.65;
  color: var(--fm-muted);
}

.step-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  font-size: 11px;
  color: var(--fm-primary);
}

.faq-card {
  padding: 2px 18px;
}

.faq-card :deep(.ant-collapse-item) {
  border-bottom: 1px solid var(--fm-border);
}

.faq-card :deep(.ant-collapse-item:last-child) {
  border-bottom: none;
}

.faq-card :deep(.ant-collapse-header) {
  padding: 11px 0 !important;
  font-size: 12px;
  font-weight: 500;
}

.faq-card :deep(.ant-collapse-content-box) {
  padding: 0 0 12px !important;
}

.faq-answer {
  margin: 0;
  font-size: 11.5px;
  line-height: 1.8;
  color: var(--fm-text-2);
}

/* ---------- 响应式 ---------- */
@media (max-width: 1180px) {
  .console {
    grid-template-columns: minmax(0, 1fr);
  }

  .kpi-grid {
    border-left: none;
    border-top: 1px solid var(--fm-border);
  }

  .main-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .col-side {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(258px, 1fr));
    align-items: start;
  }
}

@media (max-width: 980px) {
  .intro-band {
    grid-template-columns: minmax(0, 1fr);
    gap: 18px;
  }

  .intro-stats {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .history-head,
  .history-row {
    grid-template-columns: 60px minmax(0, 1fr) 58px 78px 96px;
  }

  .history-head span:nth-child(2),
  .history-name {
    display: none;
  }
}

@media (max-width: 760px) {
  .console-main {
    padding: 18px 18px;
  }

  .intro-band {
    padding: 18px;
  }

  .intro-title {
    font-size: 15.5px;
  }

  .intro-stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .feature-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .kpi {
    padding: 12px 14px;
  }

  .kpi-value {
    font-size: 20px;
  }

  .help-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .history-head,
  .history-row {
    grid-template-columns: 56px minmax(0, 1fr) 52px 88px;
  }

  .history-head span:nth-child(5),
  .history-duration {
    display: none;
  }

  .history-head,
  .history-row {
    padding-left: 12px;
    padding-right: 12px;
  }
}

@media (max-width: 620px) {
  .history-head,
  .history-row {
    grid-template-columns: 54px minmax(0, 1fr) 48px;
  }

  .history-head span:nth-child(6),
  .history-when {
    display: none;
  }

  .history-empty {
    flex-wrap: wrap;
  }
}

@media (prefers-reduced-motion: reduce) {
  .pulse-dot {
    animation: none;
  }

  .launch,
  .project-card,
  .metric-fill {
    transition: none;
  }
}
</style>
