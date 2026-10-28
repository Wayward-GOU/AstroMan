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
  PlayCircleOutlined,
  BarChartOutlined,
  FileTextOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { formatDateTime } from '@/utils/datetime'
import { useProjectStore } from '@/stores/project'
import { useAiAssistStore } from '@/stores/aiAssist'
import { dashboardInsightPrompt } from '@/utils/aiScenarios'
import * as api from '@/api'
import type {
  DashboardMetrics,
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
const metrics = ref<DashboardMetrics | null>(null)

const aiAssist = useAiAssistStore()

function aiInsight() {
  void aiAssist.show({
    title: '工作台洞察',
    prompt: dashboardInsightPrompt(stats.value, metrics.value),
    hint: stats.value
      ? `${stats.value.projectCount} 个项目 · ${stats.value.requestCount} 个接口 · ${stats.value.historyCount} 条历史`
      : '数据加载中',
  })
}
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
      note: s ? `近 7 天 ${s.recentHistory} 次` : '本机保留最近 100000 条',
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

/* ---------- 数据大屏 ---------- */

const dashDays = computed(() => metrics.value?.days ?? [])
const codeBuckets = computed(() => metrics.value?.codeBuckets ?? [])
const topRequests = computed(() => metrics.value?.topRequests ?? [])
const reportPoints = computed(() => metrics.value?.reportPoints ?? [])
const methodBuckets = computed(() => metrics.value?.methodBuckets ?? [])
const hourBuckets = computed(() => metrics.value?.hourBuckets ?? [])
const latencyBuckets = computed(() => metrics.value?.latencyBuckets ?? [])
const topProjects = computed(() => metrics.value?.topProjects ?? [])

const trendTotal = computed(() => dashDays.value.reduce((sum, d) => sum + d.total, 0))
const trendSuccess = computed(() => dashDays.value.reduce((sum, d) => sum + d.success, 0))
const trendRate = computed(() =>
  trendTotal.value ? (trendSuccess.value / trendTotal.value) * 100 : 0,
)
const trendAvgRt = computed(() => {
  const weighted = dashDays.value.reduce((sum, d) => sum + d.avgRt * d.total, 0)
  return trendTotal.value ? weighted / trendTotal.value : 0
})
const trendPeak = computed(() => Math.max(1, ...dashDays.value.map((d) => d.total)))

const historyTotal = computed(() => codeBuckets.value.reduce((sum, b) => sum + b.count, 0))
const reportPeak = computed(() =>
  Math.max(1, ...reportPoints.value.map((r) => r.p95Rt)),
)
const todayTotal = computed(() => hourBuckets.value.reduce((sum, h) => sum + h.count, 0))
const hourPeak = computed(() => Math.max(1, ...hourBuckets.value.map((h) => h.count)))
const methodPeak = computed(() => Math.max(1, ...methodBuckets.value.map((b) => b.count)))
const latencyTotal = computed(() => latencyBuckets.value.reduce((sum, b) => sum + b.count, 0))
const latencyPeak = computed(() => Math.max(1, ...latencyBuckets.value.map((b) => b.count)))
const projectPeak = computed(() => Math.max(1, ...topProjects.value.map((p) => p.count)))
const todayPeakHour = computed(() => {
  const peak = hourBuckets.value.reduce(
    (best, item) => (item.count > best.count ? item : best),
    hourBuckets.value[0] ?? { hour: 0, count: 0 },
  )
  return peak.count ? `${String(peak.hour).padStart(2, '0')}:00` : '—'
})

function trendBarHeight(point: { total: number }) {
  return point.total ? Math.max(6, (point.total / trendPeak.value) * 100) : 0
}

function trendSegment(point: { total: number }, value: number) {
  return point.total ? (value / point.total) * 100 : 0
}

function monthDay(date: string) {
  return date.slice(5)
}

function trendTitle(point: { date: string; total: number; success: number; avgRt: number }) {
  const rate = point.total ? ((point.success / point.total) * 100).toFixed(0) : 0
  return `${point.date} · ${point.total} 次请求 · 成功率 ${rate}% · 平均 ${fmtMs(point.avgRt)}`
}

function bucketClass(label: string) {
  if (label === '2xx') return 'ok'
  if (label === '3xx') return 'info'
  if (label === '4xx') return 'warn'
  if (label === '5xx') return 'danger'
  return 'other'
}

function bucketPercent(count: number) {
  return historyTotal.value ? `${((count / historyTotal.value) * 100).toFixed(2)}%` : '0%'
}

function bucketText(count: number) {
  if (!historyTotal.value) return '0%'
  return `${((count / historyTotal.value) * 100).toFixed(1)}%`
}

function reportBarWidth(report: { p95Rt: number }) {
  return `${Math.max(6, (report.p95Rt / reportPeak.value) * 100)}%`
}

function reportTone(report: { errorRate: number }) {
  if (report.errorRate >= 1) return 'danger'
  if (report.errorRate >= 0.1) return 'warn'
  return 'ok'
}

function hourHeight(point: { count: number }) {
  return point.count ? Math.max(4, (point.count / hourPeak.value) * 100) : 0
}

function hourTitle(point: { hour: number; count: number }) {
  return `${String(point.hour).padStart(2, '0')}:00 - ${String(point.hour).padStart(2, '0')}:59 · ${point.count} 次`
}

function methodWidth(bucket: { count: number }) {
  return `${bucket.count ? Math.max(6, (bucket.count / methodPeak.value) * 100) : 0}%`
}

function methodTone(label: string) {
  if (label === 'GET') return 'ok'
  if (label === 'POST') return 'info'
  if (label === 'PUT' || label === 'PATCH') return 'warn'
  if (label === 'DELETE') return 'danger'
  return 'other'
}

function latencyWidth(bucket: { count: number }) {
  return `${bucket.count ? Math.max(6, (bucket.count / latencyPeak.value) * 100) : 0}%`
}

function latencyTone(index: number) {
  return ['ok', 'ok', 'info', 'warn', 'danger'][index] ?? 'other'
}

function latencyText(bucket: { count: number }) {
  if (!latencyTotal.value) return '0%'
  return `${((bucket.count / latencyTotal.value) * 100).toFixed(1)}%`
}

function projectWidth(item: { count: number }) {
  return `${Math.max(6, (item.count / projectPeak.value) * 100)}%`
}

const runnerSteps = [
  {
    title: '选择项目与环境',
    desc: '进入批量运行页，选好项目后会自动带出该项目已激活的环境与变量。',
  },
  {
    title: '勾选要跑的接口',
    desc: '支持关键字过滤与多选，第一次跑建议先选 1～2 个接口确认参数。',
  },
  {
    title: '决定数据来源',
    desc: '填迭代次数，或导入 CSV 文件；表头即变量名，每一行驱动一轮请求。',
  },
  {
    title: '运行并导出结果',
    desc: '设置请求间隔后开始，随时可停止；结果表汇总状态码、耗时与断言。',
  },
]

// 示例内容放在常量里，避免模板把 {{变量}} 当成插值
const csvExample = `username,password
alice,Passw0rd!
bob,Hello@2024
carol,Fbin@2026`
const bodyExample = `{
  "username": "{{username}}",
  "password": "{{password}}"
}`
const runnerUrlExample = '{{base_url}}/api/login'
const runnerVarExample = '{{username}}'
const runnerEnvExample = '{{base_url}}'

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
  {
    key: '05',
    title: '签名与加密',
    desc: '脚本页「插入示例」可一键填入 MD5 / HMAC 签名、AES 加密、RSA 加密，生成的值用 {{变量}} 引用。',
    action: '打开项目',
    go: goProject,
  },
  {
    key: '06',
    title: '导入现成接口',
    desc: '项目页可粘贴 cURL 一键解析；工作台可导入项目 JSON，用于同事共享或换机迁移。',
    action: '打开项目',
    go: goProject,
  },
  {
    key: '07',
    title: '用小HI 提效',
    desc: '在「小HI」里说一句话就能建项目、写接口、发请求、写签名脚本、查压测报告，并能导出执行记录。',
    action: '打开小HI',
    go: () => router.push({ name: 'ai' }),
  },
  {
    key: '08',
    title: '数据与备份',
    desc: '数据默认存在本机 SQLite；设置页可查看路径、迁移目录，项目页可导出 JSON 备份。',
    action: '打开设置',
    go: () => router.push('/settings'),
  },
])

const tips = [
  {
    title: '变量引用与优先级',
    desc: 'URL、Header、Body 都能写变量；优先级：运行时变量（脚本 vars.set）> 环境变量 > 内置变量。',
    code: '{{base_url}}/api/users/{{user_id}}\n{{timestamp}}  {{uuid}}  {{randomInt(1,100)}}',
  },
  {
    title: '前置脚本：签名头',
    desc: '在「脚本 → 前置脚本」里拼接待签名串，算好后写进请求头。',
    code: "var ts = String(Date.now())\nrequest.headers.timestamp = ts\nrequest.headers.sign = cipher.md5(env.APP_ID + ts + request.body + env.APP_SECRET)",
  },
  {
    title: '前置脚本：加密请求体',
    desc: 'AES 默认 CBC + PKCS7，与 Java 的 AES/CBC/PKCS5Padding 结果一致；密钥与 IV 放环境变量。',
    code: "request.body = cipher.aes.encrypt(request.body, env.AES_KEY, { iv: env.AES_IV })",
  },
  {
    title: '前置脚本：RSA 加密参数',
    desc: '公钥加密（PKCS#1 v1.5）后存入变量，Header 里用 {{authCode}} 引用即可。',
    code: "vars.set('authCode', rsa.publicEncrypt(env.APP_ID + '.' + env.USER_CODE + '.' + Date.now(), env.PUBLIC_KEY))",
  },
  {
    title: '后置脚本：断言与取变量',
    desc: '断言结果展示在响应区「测试」页；提取的变量可供后续请求复用。',
    code: "test('HTTP 200', () => assert(response.status === 200))\nvars.set('token', response.json().token)",
  },
  {
    title: 'cURL 导入与代码生成',
    desc: '项目页粘贴 cURL 即可解析成接口；请求栏「代码」按钮反向生成 cURL / 原始报文 / fetch / Python。',
    code: '',
  },
  {
    title: '多标签并行调试',
    desc: '同一项目最多同时打开 20 个接口标签，编辑内容自动保存，切换或关闭都不丢改动。',
    code: '',
  },
  {
    title: '压测参数怎么起步',
    desc: '建议 10～30 并发、30 秒时长起步，观察错误率与 P95 后再加；可配 P95 / P99 / 错误率 SLA 自动判定。',
    code: '',
  },
  {
    title: 'AI 助手：自然语言操作',
    desc: '点右侧屏幕中间的机器人进入小HI。先在小HI 顶部「模型服务」里配置一家服务商（如 DeepSeek / Kimi / 智谱 / Claude），之后直接说需求即可。写入类操作会先给计划、等你确认后再执行。',
    code: '列出本机所有项目，并按请求数量排序\n在第一个项目里新建登录接口，发送并分析返回\n给接口写一个前置签名脚本，再发一次验证',
  },
  {
    title: 'AI 助手：聊天与出方案',
    desc: '小HI 也能当日常助手：解释概念、给压测方案、做错误诊断、写脚本草稿，这类问答不会去翻你的项目数据。',
    code: 'HTTP/2 和 HTTP/1.1 有什么区别\n给我一份下单接口的压测方案，含并发与 SLA\n这个 401 报错一般怎么排查',
  },
  {
    title: 'AI 助手：写脚本与查数据',
    desc: '可让 小HI 直接写前置 / 后置脚本（保存前做语法检查），也能查压测报告、按接口或项目导出执行记录为 CSV / JSON。',
    code: '给「创建订单」接口写 HMAC-SHA256 签名前置脚本\n导出某接口最近 50 条执行记录为 CSV\n最近一次压测报告的关键指标怎么样',
  },
  {
    title: 'AI 用量与费用',
    desc: '每次模型调用都会记一条：服务商、模型、输入 / 输出 token、耗时与估算费用，近 1 年可查；小HI 顶部「用量统计」进入。',
    code: '',
  },
  {
    title: '数据备份与迁移',
    desc: '项目页或卡片菜单导出 *.fbinhttp.json 交给同事；设置页可查看数据库路径并迁移数据目录。',
    code: '',
  },
]

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
    q: '接口要签名或加密，脚本里怎么写？',
    a: '打开接口编辑器的「脚本」页，右上角「插入示例」可直接填入时间戳、MD5 / HMAC-SHA256 签名、AES 加密、RSA 公钥加密等可运行代码，改几个变量名即可。脚本中可用 cipher（aes / des / tripleDes 加解密、md5 / sha1 / sha256、hmac、base64 / hex 编解码）与 rsa.publicEncrypt，生成的值用 vars.set 写入后，在 Header 或 Body 里用 {{变量}} 引用。',
  },
  {
    q: '密钥长度报错是什么原因？',
    a: 'AES 密钥必须是 16 / 24 / 32 字节，DES 是 8 字节，3DES 是 24 字节；AES 的 IV 要 16 字节，DES / 3DES 要 8 字节。密钥本身是十六进制字符串时传 { keyFormat: \'hex\' }，Base64 则用 { keyFormat: \'base64\' }，避免把编码后的文本当成原始字节。',
  },
  {
    q: '怎么快速导入现成的接口？',
    a: '在项目页点「导入接口」，把浏览器 Network 里复制的 cURL 命令粘进去，会自动解析出方法、URL、请求头、查询参数与 Body，再选目标集合即可。反过来，请求栏「代码」按钮可以把当前请求生成 cURL / 原始报文 / fetch / Python 代码。',
  },
  {
    q: '批量运行（Runner）和压测有什么区别？',
    a: '批量运行按顺序发送：一次选中多个接口，配合迭代次数或 CSV 数据逐轮跑，适合回归与数据驱动场景，每条结果都能单独查看；压测是在固定并发下持续施压，关注 QPS、P95/P99 与错误率。需要按数据批量跑接口用「批量运行」，需要并发能力去「压测」。',
  },
  {
    q: '怎么把项目分享给同事？',
    a: '项目页顶部或工作台项目卡片菜单里选「导出项目」，会得到 *.fbinhttp.json（含集合、接口、脚本与断言）。对方在工作台点「导入项目 → 导入项目 JSON」即可复原；重名时会自动加「（导入）」后缀，不影响原有项目。',
  },
  {
    q: '变量没生效怎么排查？',
    a: '先确认请求栏选择的环境是否正确，再检查变量名拼写与大小写。若脚本里用 vars.set 写过同名变量，运行时会覆盖环境变量（优先级：运行时 > 环境 > 内置）；脚本页底部的「运行时变量」列表可以直接看到实际取值。',
  },
  {
    q: '压测报告在哪里看？可以导出吗？',
    a: '压测结束后报告自动入库，侧边栏「压测报告」可查看 SLA 判定、延迟分位、状态码分布与逐请求明细；详情页支持导出 JSON（归档）、HTML（可浏览器打开或打印 PDF）与 Excel（多工作表）。',
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
  {
    q: '小HI 能帮我做什么？',
    a: '两个模式：一是聊天答疑、出方案（压测方案、测试策略、脚本写法、错误诊断），只给建议不动数据；二是操作本机——新建项目 / 集合 / 接口、真实发送请求并解读响应、写前置与后置脚本、批量执行、生成压测计划、查询历史与报告、导出执行记录。凡是要新建、修改、删除、导入导出或发起压测，它都会先把完整计划列出来，你确认后才弹执行框。',
  },
  {
    q: 'AI 服务怎么配置？密钥安全吗？',
    a: '进入小HI，点顶部「模型服务」→「添加模型服务」，选一个厂商预设（自动填好协议、Base URL 和模型下拉），再填自己的 API Key 即可。支持 OpenAI 兼容、Claude、Gemini 与本地 Ollama；密钥用 AES-256-GCM 加密后只保存在本机数据库，不会上传到任何第三方。',
  },
  {
    q: 'AI 用了多少 token、花了多少钱？',
    a: '每次模型调用都会记录服务商、模型、输入 / 输出 token、耗时与估算费用，保留近 1 年。在小HI 顶部点「用量统计」可看指标卡、每日趋势、逐条调用记录（含本轮提问）与各工具准确率，费用按厂商公开单价估算并折算人民币。',
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

  const [his, reps, st, storageInfo, info, dash] = await Promise.allSettled([
    api.historyList(8),
    api.reportList(4),
    api.workspaceStats(),
    api.storageInfo(),
    api.appInfo(),
    api.dashboardMetrics(),
  ])
  if (his.status === 'fulfilled') history.value = his.value
  if (reps.status === 'fulfilled') reports.value = reps.value
  if (st.status === 'fulfilled') stats.value = st.value
  if (storageInfo.status === 'fulfilled') storage.value = storageInfo.value
  if (info.status === 'fulfilled') appVersion.value = info.value.version
  if (dash.status === 'fulfilled') metrics.value = dash.value

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
  const [s, m] = await Promise.allSettled([api.workspaceStats(), api.dashboardMetrics()])
  if (s.status === 'fulfilled') stats.value = s.value
  if (m.status === 'fulfilled') metrics.value = m.value
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
      title: '导入 FbinHttp 项目',
      multiple: false,
      filters: [{ name: 'FbinHttp 项目文件', extensions: ['json'] }],
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
      defaultPath: `${safeName}.fbinhttp.json`,
      filters: [{ name: 'FbinHttp 项目文件', extensions: ['json'] }],
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
  if (info.key === 'fbinhttp') importProject()
  else if (info.key === 'api') importApiProject()
}

function clearHistory() {
  Modal.confirm({
    title: '清空全部请求历史？',
    content: '只删除本机记录的请求历史，不会影响项目、接口与压测报告。按时间范围清理请前往「请求历史」页。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.dataCleanup({ history: true, vacuum: true })
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
            <a-menu-item key="fbinhttp">导入项目 JSON</a-menu-item>
            <a-menu-item key="api">导入 Postman / Apifox 数据</a-menu-item>
          </a-menu>
        </template>
      </a-dropdown>
      <a-button @click="aiInsight">
        <template #icon><AiSparkIcon /></template>
        AI 洞察
      </a-button>
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
          <span>FbinHttp v{{ appVersion || '1.0.0' }}</span>
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

    <!-- 批量运行介绍 -->
    <section class="runner-band fm-card">
      <div class="runner-copy">
        <span class="runner-eyebrow"><PlayCircleOutlined /> 批量运行 · Runner</span>
        <h2 class="runner-title">一次跑完一组接口，用 CSV 给每一轮换数据</h2>
        <p class="runner-desc">
          批量运行把「一次发一个请求」变成「一次跑一批」：勾选项目里任意多个接口，设定迭代次数，
          或导入一份 CSV 让每一行驱动一轮请求。每个请求的状态码、耗时、断言结果与错误都会汇总到结果表，
          跑完还能导出留档。环境变量、前置/后置脚本与断言照常生效。
        </p>
        <div class="runner-tags">
          <span>多接口勾选</span>
          <span>迭代次数 / CSV 参数化</span>
          <span>环境与脚本生效</span>
          <span>逐条结果可导出</span>
        </div>
        <div class="runner-actions">
          <a-button type="primary" @click="router.push('/runner')">
            <template #icon><PlayCircleOutlined /></template>
            进入批量运行
          </a-button>
          <a-button :disabled="!recentProject" @click="goProject">先准备接口</a-button>
        </div>
      </div>

      <div class="runner-steps">
        <div v-for="(s, i) in runnerSteps" :key="s.title" class="runner-step">
          <span class="runner-step-index mono">0{{ i + 1 }}</span>
          <div class="runner-step-body">
            <div class="runner-step-title">{{ s.title }}</div>
            <div class="runner-step-desc">{{ s.desc }}</div>
          </div>
        </div>
      </div>
    </section>

    <!-- 批量运行示例 -->
    <div class="fm-card runner-example">
      <div class="example-head">
        <span class="example-title"><FileTextOutlined /> 示例：用 CSV 批量登录 3 个账号</span>
        <span class="example-note">CSV 表头就是变量名，每行跑一轮</span>
      </div>
      <div class="example-grid">
        <div class="example-col">
          <div class="example-label">users.csv</div>
          <pre class="example-code mono">{{ csvExample }}</pre>
        </div>
        <div class="example-col">
          <div class="example-label">
            POST <span class="mono">{{ runnerUrlExample }}</span>（Body 为 JSON）
          </div>
          <pre class="example-code mono">{{ bodyExample }}</pre>
        </div>
        <div class="example-col">
          <div class="example-label">运行结果（3 轮）</div>
          <div class="example-results">
            <div><b class="mono">第 1 轮</b> alice → 200 · 断言通过</div>
            <div><b class="mono">第 2 轮</b> bob → 200 · 断言通过</div>
            <div class="fail"><b class="mono">第 3 轮</b> carol → 401 · 断言失败</div>
          </div>
        </div>
      </div>
      <div class="example-foot">
        在 URL、Header、Body 里用 <code class="mono">{{ runnerVarExample }}</code> 引用 CSV 列，
        <code class="mono">{{ runnerEnvExample }}</code> 等环境变量仍会照常替换。单次任务最多 5000 个请求
        （接口数 × 迭代轮数），超过会被拦截；需要高并发施压请改用「压测」。
      </div>
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
          FbinHttp 面向需要频繁调试 HTTP 接口的开发者：界面参考 Postman，压测参数参考 JMeter，
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
        <h2 class="section-title">FbinHttp 能做什么</h2>
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
            <div class="empty-desc">创建一个项目开始调试，或者导入 FbinHttp 项目、Postman 集合或 Apifox 数据。</div>
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

    <!-- 数据大屏 -->
    <div class="section-head dash-head">
      <div>
        <h2 class="section-title">数据大屏</h2>
        <div class="section-desc">根据本机请求历史与压测报告实时汇总，清空历史后同步归零</div>
      </div>
      <div class="dash-legend">
        <span class="legend-dot ok" />成功
        <span class="legend-dot err" />失败
      </div>
    </div>

    <div class="dash-grid">
      <!-- 请求趋势 -->
      <div class="fm-card dash-card trend-card">
        <div class="dash-card-head">
          <div>
            <div class="dash-title">近 14 天请求趋势</div>
            <div class="dash-sub">按发送时间统计，包含单接口调试与批量运行</div>
          </div>
          <div class="dash-kpis">
            <span><b class="mono">{{ fmtInt(trendTotal) }}</b> 次请求</span>
            <span><b class="mono">{{ trendRate.toFixed(1) }}%</b> 成功率</span>
            <span><b class="mono">{{ fmtMs(trendAvgRt) }}</b> 平均耗时</span>
          </div>
        </div>
        <div v-if="trendTotal" class="trend-plot">
          <div v-for="d in dashDays" :key="d.date" class="trend-col" :title="trendTitle(d)">
            <div class="trend-slot">
              <div class="trend-bar" :style="{ height: `${trendBarHeight(d)}%` }">
                <span class="trend-seg ok" :style="{ height: `${trendSegment(d, d.success)}%` }" />
                <span
                  class="trend-seg err"
                  :style="{ height: `${trendSegment(d, d.total - d.success)}%` }"
                />
              </div>
            </div>
            <span class="trend-day mono">{{ monthDay(d.date) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">
          还没有请求记录。发送一次请求或跑一轮批量运行后，这里会自动生成趋势图。
        </div>
      </div>

      <!-- 状态码分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">状态码分布</div>
          <span class="dash-total mono">{{ fmtInt(historyTotal) }} 次</span>
        </div>
        <div v-if="historyTotal" class="code-stack">
          <span
            v-for="b in codeBuckets"
            :key="b.label"
            class="code-seg"
            :class="bucketClass(b.label)"
            :style="{ width: bucketPercent(b.count) }"
          />
        </div>
        <div class="code-legend">
          <div v-for="b in codeBuckets" :key="b.label" class="code-row">
            <span class="legend-dot" :class="bucketClass(b.label)" />
            <span class="code-label">{{ b.label }}</span>
            <span class="code-count mono">{{ fmtInt(b.count) }}</span>
            <span class="code-pct mono">{{ bucketText(b.count) }}</span>
          </div>
        </div>
        <div v-if="!historyTotal" class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 方法分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">请求方法分布</div>
          <span class="dash-total mono">{{ fmtInt(historyTotal) }} 次</span>
        </div>
        <div v-if="historyTotal" class="bar-list">
          <div v-for="b in methodBuckets" :key="b.label" class="bar-row">
            <span class="bar-label method-label" :class="`tone-text-${methodTone(b.label)}`">
              {{ b.label }}
            </span>
            <span class="bar-track">
              <span
                class="bar-fill"
                :class="methodTone(b.label)"
                :style="{ width: methodWidth(b) }"
              />
            </span>
            <span class="bar-value mono">{{ fmtInt(b.count) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 今日时段分布 -->
      <div class="fm-card dash-card hour-card">
        <div class="dash-card-head">
          <div>
            <div class="dash-title">今日各时段请求量</div>
            <div class="dash-sub">按本地时间 0-23 点，峰值时段 {{ todayPeakHour }}</div>
          </div>
          <div class="dash-kpis">
            <span><b class="mono">{{ fmtInt(todayTotal) }}</b> 次</span>
          </div>
        </div>
        <div v-if="todayTotal" class="hour-plot">
          <div v-for="h in hourBuckets" :key="h.hour" class="hour-col" :title="hourTitle(h)">
            <div class="hour-slot">
              <div class="hour-bar" :style="{ height: `${hourHeight(h)}%` }" />
            </div>
            <span v-if="h.hour % 3 === 0" class="hour-tick mono">{{ h.hour }}</span>
            <span v-else class="hour-tick mono" />
          </div>
        </div>
        <div v-else class="dash-empty">今天还没有请求，发一次就会出现在这里。</div>
      </div>

      <!-- 耗时分布 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">响应耗时分布</div>
          <span class="dash-total mono">共 {{ fmtInt(latencyTotal) }} 次</span>
        </div>
        <div v-if="latencyTotal" class="bar-list">
          <div v-for="(b, i) in latencyBuckets" :key="b.label" class="bar-row">
            <span class="bar-label">{{ b.label }}</span>
            <span class="bar-track">
              <span
                class="bar-fill"
                :class="latencyTone(i)"
                :style="{ width: latencyWidth(b) }"
              />
            </span>
            <span class="bar-value mono">{{ latencyText(b) }}</span>
          </div>
        </div>
        <div v-else class="dash-empty">暂无请求记录。</div>
      </div>

      <!-- 项目调用量 -->
      <div class="fm-card dash-card">
        <div class="dash-card-head">
          <div class="dash-title">项目调用量 Top 5</div>
          <a class="dash-link" @click="router.push('/projects')">我的项目</a>
        </div>
        <div v-if="topProjects.length" class="bar-list">
          <div v-for="p in topProjects" :key="p.name" class="bar-row">
            <span class="bar-label project-label" :title="p.name">{{ p.name }}</span>
            <span class="bar-track">
              <span class="bar-fill ok" :style="{ width: projectWidth(p) }" />
            </span>
            <span class="bar-value mono">{{ fmtInt(p.count) }}</span>
            <span class="bar-value mono" :class="{ 'tone-danger': p.successRate < 95 }">
              {{ p.successRate.toFixed(0) }}%
            </span>
          </div>
        </div>
        <div v-else class="dash-empty">还没有调用记录。</div>
      </div>

      <!-- 高频接口 -->
      <div class="fm-card dash-card top-card">
        <div class="dash-card-head">
          <div class="dash-title">高频接口 Top 5</div>
          <a class="dash-link" @click="router.push('/history')">全部历史</a>
        </div>
        <div v-if="topRequests.length" class="top-list">
          <div v-for="(t, i) in topRequests" :key="`${t.method}-${t.url}`" class="top-row">
            <span class="top-rank mono">{{ i + 1 }}</span>
            <span class="method-tag" :class="`method-${t.method}`">{{ t.method }}</span>
            <span class="top-name" :title="t.url">{{ t.name }}</span>
            <span class="top-meta mono">{{ fmtInt(t.count) }} 次</span>
            <span class="top-meta mono">{{ fmtMs(t.avgRt) }}</span>
            <span class="top-rate mono" :class="{ low: t.successRate < 95 }">
              {{ t.successRate.toFixed(0) }}%
            </span>
          </div>
        </div>
        <div v-else class="dash-empty">还没有调用记录。</div>
      </div>

      <!-- 压测走势 -->
      <div class="fm-card dash-card report-card">
        <div class="dash-card-head">
          <div class="dash-title">最近压测 P95 走势</div>
          <a class="dash-link" @click="router.push('/reports')">全部报告</a>
        </div>
        <div v-if="reportPoints.length" class="report-chart">
          <div
            v-for="r in reportPoints"
            :key="r.id"
            class="report-chart-row"
            @click="router.push(`/reports/${r.id}`)"
          >
            <span class="report-chart-name" :title="r.name">{{ r.name }}</span>
            <span class="report-chart-track">
              <span
                class="report-chart-fill"
                :class="reportTone(r)"
                :style="{ width: reportBarWidth(r) }"
              />
            </span>
            <span class="report-chart-value mono">{{ fmtMs(r.p95Rt) }}</span>
            <span class="report-chart-value mono" :class="`tone-${reportTone(r)}`">
              {{ r.errorRate.toFixed(1) }}%
            </span>
          </div>
          <div class="report-chart-foot">柱长为 P95 延迟，右侧为错误率；红色表示错误率 ≥ 1%</div>
        </div>
        <div v-else class="dash-empty">
          还没有压测报告。<a class="dash-link" @click="router.push('/stress')">去发起压测</a>
        </div>
      </div>
    </div>

    <!-- 最近请求 -->
    <div id="recent-history" class="section-head">
      <div>
        <h2 class="section-title">最近请求</h2>
        <div class="section-desc">每次发送自动记录，最多保留最近 100000 条</div>
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
        <div class="section-desc">从建项目到签名加密、压测与备份，右侧是常见问题</div>
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

    <div class="section-head tips-head">
      <div>
        <h2 class="section-title">常用操作</h2>
        <div class="section-desc">
          高频用法速查；脚本示例也可在接口编辑器「脚本」页用「插入示例」一键填入
        </div>
      </div>
    </div>

    <div class="tips-grid">
      <div v-for="tip in tips" :key="tip.title" class="fm-card tip-card">
        <div class="tip-title">{{ tip.title }}</div>
        <div class="tip-desc">{{ tip.desc }}</div>
        <pre v-if="tip.code" class="tip-code mono">{{ tip.code }}</pre>
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

.tips-head {
  margin-top: 22px;
}

.tips-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(258px, 1fr));
  gap: 14px;
}

.tip-card {
  padding: 14px 16px;
}

.tip-title {
  font-size: 12.5px;
  font-weight: 600;
  margin-bottom: 4px;
}

.tip-desc {
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.tip-code {
  margin: 8px 0 0;
  padding: 8px 10px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
  color: var(--fm-text);
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
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

/* ---------- 数据大屏 ---------- */
.dash-head {
  margin-top: 20px;
  margin-bottom: 12px;
}

.dash-legend {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--fm-muted);
}

.legend-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--fm-border-strong);
}

.legend-dot.ok {
  background: var(--fm-success);
}

.legend-dot.err {
  background: var(--fm-danger);
}

.legend-dot.info {
  background: var(--fm-primary);
}

.legend-dot.warn {
  background: var(--fm-warning);
}

.legend-dot.danger {
  background: var(--fm-danger);
}

.legend-dot.other {
  background: var(--fm-border-strong);
}

.dash-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

/* 宽图占两列，其余卡片一列，整体更紧凑 */
.trend-card,
.hour-card,
.top-card,
.report-card {
  grid-column: span 2;
}

.dash-card {
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 13px 15px;
}

.dash-card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.dash-title {
  font-size: 12.5px;
  font-weight: 600;
}

.dash-sub {
  margin-top: 2px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.dash-kpis {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.dash-kpis b {
  color: var(--fm-text);
  font-weight: 600;
}

.dash-total {
  font-size: 11px;
  color: var(--fm-muted);
}

.dash-link {
  font-size: 11px;
  color: var(--fm-primary);
  cursor: pointer;
}

.dash-empty {
  padding: 28px 0 14px;
  font-size: 11.5px;
  line-height: 1.7;
  color: var(--fm-muted);
  text-align: center;
}

/* 通用横向条形图：方法 / 耗时 / 项目共用 */
.bar-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin-top: 12px;
}

.bar-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.bar-label {
  flex: 0 0 62px;
  overflow: hidden;
  font-size: 10.5px;
  color: var(--fm-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.method-label {
  font-weight: 600;
  color: var(--fm-text);
}

.project-label {
  flex: 0 0 78px;
  color: var(--fm-text);
}

.bar-track {
  position: relative;
  flex: 1 1 auto;
  height: 7px;
  overflow: hidden;
  border-radius: 4px;
  background: var(--fm-surface-alt);
}

.bar-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--fm-border-strong);
  transition: width 0.35s ease;
}

.bar-fill.ok {
  background: var(--fm-success);
}

.bar-fill.info {
  background: var(--fm-primary);
}

.bar-fill.warn {
  background: var(--fm-warning);
}

.bar-fill.danger {
  background: var(--fm-danger);
}

.bar-value {
  flex: 0 0 42px;
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.tone-text-ok {
  color: var(--fm-success);
}

.tone-text-info {
  color: var(--fm-primary);
}

.tone-text-warn {
  color: var(--fm-warning);
}

.tone-text-danger {
  color: var(--fm-danger);
}

.tone-danger {
  color: var(--fm-danger);
}

/* 今日各时段柱状图 */
.hour-plot {
  display: flex;
  gap: 2px;
  height: 96px;
  margin-top: 12px;
}

.hour-col {
  display: flex;
  flex: 1 1 0;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}

.hour-slot {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.hour-bar {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  border-radius: 3px 3px 1px 1px;
  background: var(--fm-primary);
  opacity: 0.85;
  transition: height 0.35s ease;
}

.hour-tick {
  height: 12px;
  font-size: 9px;
  color: var(--fm-muted);
  text-align: center;
}

/* 请求趋势 */
.trend-plot {
  display: flex;
  gap: 6px;
  height: 128px;
  margin-top: 12px;
}

.trend-col {
  display: flex;
  flex: 1 1 0;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.trend-slot {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.trend-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  width: 100%;
  max-width: 24px;
  margin: 0 auto;
  display: flex;
  flex-direction: column-reverse;
  overflow: hidden;
  border-radius: 4px 4px 2px 2px;
  background: var(--fm-surface-alt);
  transition: height 0.35s ease;
}

.trend-seg {
  display: block;
  width: 100%;
  flex-shrink: 0;
}

.trend-seg.ok {
  background: var(--fm-primary);
}

.trend-seg.err {
  background: var(--fm-danger);
}

.trend-day {
  font-size: 9.5px;
  color: var(--fm-muted);
  text-align: center;
  white-space: nowrap;
}

/* 状态码分布 */
.code-stack {
  display: flex;
  height: 8px;
  margin: 14px 0 12px;
  border-radius: 4px;
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.code-seg {
  height: 100%;
  transition: width 0.3s;
}

.code-seg.ok {
  background: var(--fm-success);
}

.code-seg.info {
  background: var(--fm-primary);
}

.code-seg.warn {
  background: var(--fm-warning);
}

.code-seg.danger {
  background: var(--fm-danger);
}

.code-seg.other {
  background: var(--fm-border-strong);
}

.code-legend {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.code-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}

.code-label {
  width: 34px;
  color: var(--fm-text-2);
}

.code-count {
  flex: 1;
  text-align: right;
  color: var(--fm-text-2);
}

.code-pct {
  width: 48px;
  text-align: right;
  color: var(--fm-muted);
}

/* 高频接口 */
.top-list {
  display: flex;
  flex-direction: column;
  margin-top: 10px;
}

.top-row {
  display: grid;
  grid-template-columns: 16px 46px minmax(0, 1fr) 54px 58px 40px;
  align-items: center;
  gap: 8px;
  padding: 7px 0;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11px;
}

.top-row:last-child {
  border-bottom: none;
}

.top-rank {
  font-size: 10px;
  color: var(--fm-muted);
}

.top-row .method-tag {
  justify-self: start;
}

.top-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text);
}

.top-meta {
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.top-rate {
  font-size: 10.5px;
  color: var(--fm-success);
  text-align: right;
}

.top-rate.low {
  color: var(--fm-danger);
}

/* 压测走势 */
.report-chart {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.report-chart-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 86px 66px 46px;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

.report-chart-name {
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.report-chart-row:hover .report-chart-name {
  color: var(--fm-primary);
}

.report-chart-track {
  height: 6px;
  border-radius: 4px;
  overflow: hidden;
  background: var(--fm-surface-alt);
}

.report-chart-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--fm-primary);
}

.report-chart-fill.warn {
  background: var(--fm-warning);
}

.report-chart-fill.danger {
  background: var(--fm-danger);
}

.report-chart-value {
  font-size: 10.5px;
  color: var(--fm-muted);
  text-align: right;
}

.report-chart-value.tone-ok {
  color: var(--fm-success);
}

.report-chart-value.tone-warn {
  color: var(--fm-warning);
}

.report-chart-value.tone-danger {
  color: var(--fm-danger);
}

.report-chart-foot {
  margin-top: 2px;
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 批量运行 ---------- */
.runner-band {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  gap: 24px;
  margin-top: 16px;
  padding: 20px 22px;
  background:
    radial-gradient(120% 160% at 0% 0%, var(--fm-primary-soft) 0%, transparent 60%),
    var(--fm-surface);
}

.runner-eyebrow {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.4px;
  color: var(--fm-primary);
}

.runner-title {
  margin: 9px 0 8px;
  font-size: 16.5px;
  font-weight: 650;
  letter-spacing: -0.2px;
  line-height: 1.4;
}

.runner-desc {
  margin: 0;
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-muted);
}

.runner-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 12px;
}

.runner-tags span {
  padding: 2px 9px;
  border-radius: 20px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-size: 10.5px;
  color: var(--fm-text-2);
}

.runner-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 16px;
}

.runner-steps {
  display: flex;
  flex-direction: column;
  justify-content: center;
}

.runner-step {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 0;
  border-bottom: 1px dashed var(--fm-border);
}

.runner-step:last-child {
  border-bottom: none;
}

.runner-step-index {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  border-radius: 6px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 10px;
  font-weight: 700;
}

.runner-step-title {
  font-size: 12px;
  font-weight: 600;
}

.runner-step-desc {
  margin-top: 2px;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.runner-example {
  margin-top: 12px;
  padding: 15px 17px;
}

.example-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.example-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  font-weight: 600;
}

.example-note {
  font-size: 10.5px;
  color: var(--fm-muted);
}

.example-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-top: 12px;
}

.example-label {
  margin-bottom: 6px;
  font-size: 10.5px;
  color: var(--fm-muted);
}

.example-code {
  margin: 0;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
  color: var(--fm-text);
  font-size: 11px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-all;
}

.example-results {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: 6px;
  background: var(--fm-surface-alt);
  font-size: 11px;
  color: var(--fm-text-2);
}

.example-results .fail {
  color: var(--fm-danger);
}

.example-foot {
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.example-foot code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
  font-size: 10.5px;
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

  .dash-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 980px) {
  .dash-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .trend-card,
  .hour-card,
  .top-card,
  .report-card {
    grid-column: auto;
  }

  .intro-band {
    grid-template-columns: minmax(0, 1fr);
    gap: 18px;
  }

  .runner-band {
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
    padding: 18px;
  }

  .example-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .intro-stats {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .history-head,
  .history-row {
    grid-template-columns: 60px minmax(0, 1fr) 58px 78px 96px;
  }

  .help-grid {
    grid-template-columns: minmax(0, 1fr);
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
