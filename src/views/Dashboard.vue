<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  ArrowRightOutlined,
  ThunderboltOutlined,
  FolderOpenOutlined,
  ImportOutlined,
  DownOutlined,
  ToolOutlined,
  ApiOutlined,
  GlobalOutlined,
  SafetyCertificateOutlined,
  RocketOutlined,
  PlayCircleOutlined,
  FileTextOutlined,
  ExperimentOutlined,
  PlusOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { notesCreate, notesCounts, notesList, notesToggle } from '@/api/notes'
import { captureStatus } from '@/api/capture'
import { formatDueLabel, parseQuickNote } from '@/utils/notesQuickParse'
import type { NoteCounts, NoteSummary } from '@/types/notes'
import { useProjectStore } from '@/stores/project'
import { useAiAssistStore } from '@/stores/aiAssist'
import { useHangzaiTaskStore } from '@/stores/hangzaiTask'
import { dashboardInsightPrompt } from '@/utils/aiScenarios'
import * as api from '@/api'
import type { DashboardMetrics, RequestSearchHit, StorageInfo, WorkspaceStats } from '@/types/models'

const router = useRouter()
const store = useProjectStore()
const hangzai = useHangzaiTaskStore()

const loading = ref(true)
const createOpen = ref(false)
const newName = ref('')
const newDesc = ref('')
const submitting = ref(false)

const stats = ref<WorkspaceStats | null>(null)
const metrics = ref<DashboardMetrics | null>(null)

const aiAssist = useAiAssistStore()

/* ---------- 今日待办 ---------- */
const noteCounts = ref<NoteCounts>({ open: 0, dueToday: 0, overdue: 0, done: 0 })
const notePreview = ref<NoteSummary[]>([])
const noteQuick = ref('')
const noteSaving = ref(false)
const noteNow = ref(Date.now())
let unlistenNotes: UnlistenFn | null = null

const noteParsed = computed(() => parseQuickNote(noteQuick.value, new Date(noteNow.value)))

async function loadNotes() {
  noteNow.value = Date.now()
  try {
    const [nextCounts, result] = await Promise.all([
      notesCounts(),
      notesList({ scope: 'today', limit: 5 }),
    ])
    noteCounts.value = nextCounts
    notePreview.value = result.items
  } catch {
    // 后端未就绪或浏览器预览时保持空态
  }
}

async function saveNoteQuick() {
  if (!noteQuick.value.trim() || noteSaving.value) return
  if (!noteParsed.value.title.trim()) {
    message.warning('再写点内容')
    return
  }
  noteSaving.value = true
  try {
    await notesCreate({
      title: noteParsed.value.title,
      dueAt: noteParsed.value.dueAt,
      repeat: noteParsed.value.repeat,
      tags: noteParsed.value.tags,
      priority: noteParsed.value.priority,
    })
    noteQuick.value = ''
    await loadNotes()
  } catch (error) {
    message.error(String(error))
  } finally {
    noteSaving.value = false
  }
}

function onNoteQuickKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.isComposing) {
    event.preventDefault()
    void saveNoteQuick()
  }
}

async function toggleNoteDone(item: NoteSummary, done: boolean) {
  try {
    await notesToggle(item.id, done)
    await loadNotes()
  } catch (error) {
    message.error(String(error))
  }
}

function onNotesVisible() {
  if (document.visibilityState === 'visible') void loadNotes()
}


function aiInsight() {
  void aiAssist.show({
    title: '工作台洞察',
    prompt: dashboardInsightPrompt(stats.value, metrics.value),
    source: 'workspace',
    hint: stats.value
      ? `${stats.value.projectCount} 个项目 · ${stats.value.requestCount} 个接口 · ${stats.value.historyCount} 条历史`
      : '数据加载中',
  })
}
const storage = ref<StorageInfo | null>(null)
const appVersion = ref('')
const proxyOn = ref<boolean | null>(null)
const captureRunning = ref(false)
const modelLabel = ref('未配置')
const guideOpen = ref(false)

const recentProject = computed(() => {
  if (!store.projects.length) return null
  return [...store.projects].sort((a, b) => b.updatedAt - a.updatedAt)[0]
})

const activeProjects = computed(() => {
  const weekAgo = Date.now() - 7 * 86_400_000
  return store.projects.filter((project) => project.updatedAt >= weekAgo).length
})

interface RecentProjectRow {
  id: number
  name: string
  description: string
  count: number | null
  last: RequestSearchHit | null
  updatedAt: number
}

const recentProjects = ref<RecentProjectRow[]>([])

async function loadRecentProjects() {
  const list = [...store.projects]
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, 5)
  recentProjects.value = list.map((project) => ({
    id: project.id,
    name: project.name,
    description: project.description ?? '',
    count: null,
    last: null,
    updatedAt: project.updatedAt,
  }))
  await Promise.all(
    list.map(async (project) => {
      try {
        const requests = await api.requestOptions(project.id)
        const row = recentProjects.value.find((item) => item.id === project.id)
        if (!row) return
        row.count = requests.length
        row.last = requests.reduce<RequestSearchHit | null>(
          (latest, item) => (!latest || item.updatedAt > latest.updatedAt ? item : latest),
          null,
        )
      } catch {
        // 浏览器预览或后端未就绪：保留空值，表格显示「—」
      }
    }),
  )
}

const todayRequests = computed(() => {
  const days = metrics.value?.days ?? []
  return days.length ? days[days.length - 1] : null
})

const headerSubtitle = computed(() => {
  const s = stats.value
  if (!s) return '本地 API 调试、抓包、压测与工具箱，数据不出这台电脑'
  const today = todayRequests.value?.total ?? 0
  return `${s.projectCount} 个项目 · ${s.requestCount} 个接口 · 今天已发送 ${fmtInt(today)} 次请求`
})

const kpis = computed(() => {
  const s = stats.value
  const project = recentProject.value
  const today = todayRequests.value
  const openProject = () => {
    if (project) router.push(`/project/${project.id}`)
    else createOpen.value = true
  }
  const todayNote = !today || today.total === 0
    ? '今天还没有请求'
    : `成功率 ${((today.success / today.total) * 100).toFixed(1)}%`
  return [
    {
      key: 'project',
      label: '项目',
      value: s?.projectCount ?? store.projects.length,
      note: `${activeProjects.value} 个本周活跃`,
      up: false,
      go: () => router.push('/projects'),
    },
    {
      key: 'request',
      label: '接口',
      value: s?.requestCount ?? null,
      note: s ? `分布在 ${s.collectionCount} 个集合` : '正在统计',
      up: false,
      go: openProject,
    },
    {
      key: 'today',
      label: '今日请求',
      value: today?.total ?? null,
      note: todayNote,
      up: Boolean(today && today.total > 0),
      go: () => router.push('/history'),
    },
    {
      key: 'todo',
      label: '待办',
      value: noteCounts.value.open,
      note: noteCounts.value.overdue > 0
        ? `${noteCounts.value.overdue} 项已逾期`
        : `今天到期 ${noteCounts.value.dueToday} 项`,
      up: false,
      go: () => router.push('/notes'),
    },
  ]
})

const launches = computed(() => [
  {
    key: 'stress',
    title: '发起压测',
    desc: '固定 / 阶梯并发，SLA 判定',
    icon: ThunderboltOutlined,
    go: () => router.push('/stress'),
  },
  {
    key: 'capture',
    title: '开始抓包',
    desc: '本机 MITM · 断点 · 弱网',
    icon: GlobalOutlined,
    go: () => router.push('/capture'),
  },
  {
    key: 'automation',
    title: '跑测试套件',
    desc: '断言 · Python 钩子 · 报告',
    icon: ExperimentOutlined,
    go: () => router.push('/automation'),
  },
  {
    key: 'ai',
    title: '问航仔',
    desc: '写接口、发请求、查报告',
    icon: AiSparkIcon,
    go: () => router.push({ name: 'ai' }),
  },
])

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
    key: 'capture',
    icon: GlobalOutlined,
    title: '抓包分析',
    desc: '本机 MITM 代理录制 HTTP / HTTPS，按域名分组查看请求与响应；一键信任根证书、接管系统代理，手机同 Wi-Fi 也能抓。',
    points: ['HTTPS 解密与证书', '域名分组 / HAR / ZIP 导出', '改参重发与手机代理'],
    go: () => router.push('/capture'),
  },
  {
    key: 'automation',
    icon: ExperimentOutlined,
    title: '自动化测试',
    desc: '把接口组合保存成测试套件，配好断言与变量链后可反复运行；支持 Python 钩子与压测用例，结果产出 HTML / JSON / JUnit。',
    points: ['HTTP / Python / 压测用例', '运行前预检与依赖跳过', 'CLI 同源执行与 CI 门禁'],
    go: () => router.push('/automation'),
  },
  {
    key: 'stress',
    icon: ThunderboltOutlined,
    title: '本地压测',
    desc: '参考 JMeter 的参数模型，支持固定并发与阶梯加压、目标 QPS 限速、SLA 阈值与自动停止。',
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

/* ---------- 抓包介绍 ---------- */
const captureSteps = [
  {
    title: '开始录制',
    desc: '点「录制」启动本机代理，默认 127.0.0.1:8888；端口被占用会自动向后找。',
  },
  {
    title: '装并信任根证书',
    desc: '点「CA 未安装」→「一键安装」，HTTPS 才能解密；重新生成证书后要重新安装。',
  },
  {
    title: '让流量走代理',
    desc: '本机点「接管系统代理」；手机把监听切 0.0.0.0，同一 Wi-Fi 填电脑 IP:端口并安装证书。',
  },
  {
    title: '查看、导出与重发',
    desc: '按域名分组看请求与响应，顶部导出全部 HAR / ZIP，右键只导出该分组；任意请求可改方法 / URL / 头 / Body 重发。',
  },
]

const captureCurlExample = `# 本机命令行走代理
curl -x http://127.0.0.1:8888 https://example.com`

const capturePhoneChecklist = [
  { name: '同一 Wi-Fi', detail: '电脑监听 0.0.0.0，手机代理填 电脑IP:8888' },
  { name: '安装根证书', detail: '设置抽屉导出后装到手机；iOS 还要打开完全信任' },
  { name: '先验代理', detail: '手机浏览器访问一个 HTTP 页面，再看列表是否有流量' },
]

const captureBoundaries = [
  { name: '微信小程序', detail: '可能绕过 Wi-Fi 代理；自己的小程序走开发者工具，其他优先 iPhone' },
  { name: 'pinning / HTTP/3', detail: '证书固定与 QUIC 无法解密，必要时关闭浏览器 QUIC' },
  { name: '只存内存', detail: '5000 条 / 128 MB body，重启清空；及时导出 HAR / ZIP' },
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

/* ---------- 测试场景示例 ---------- */
const suiteSteps = [
  {
    title: '建一个测试套件',
    desc: '在「自动化测试」里新建套件；也可以在批量运行页点「另存为测试套件」，把勾选好的接口与数据文件直接带过来。',
  },
  {
    title: '编排用例并写断言',
    desc: 'HTTP 用例绑定接口，用状态码 / JSONPath / Header / 正则做声明式断言；复杂逻辑用 Python 前后置钩子，容量门禁用压测用例。',
  },
  {
    title: '运行前自动预检',
    desc: '点「运行」会先检查解释器、数据集、依赖环与缺失 secret；接口自带的 JS 钩子与环境脚本会被列出并跳过，需要时改写为 Python。',
  },
  {
    title: '看报告或接 CI',
    desc: '每次运行产出 HTML / JSON / JUnit 与日志；桌面端与 astroman-cli 使用同一内核，可直接作为流水线门禁。',
  },
]

const suiteAssertionExample = `[
  { "type": "status", "op": "eq", "value": 200 },
  { "type": "jsonPath", "path": "$.args.keyword",
    "op": "eq", "value": "astroman" }
]`
const suiteCliExample =
  'astroman-cli test run --suite "示例回归（httpbin）" \\\n  --report junit,html --out ./artifacts'

const suiteSampleCases = [
  {
    name: '查询参数回显',
    detail: 'HTTP 用例 · 绑定「GET 查询参数」· 断言 200 与 $.args.keyword = astroman',
  },
  {
    name: '创建 JSON 并回显',
    detail: 'HTTP 用例 · 绑定「POST 创建 JSON」· 断言 200 与 $.json.name = AstroMan、耗时 ≤ 10s',
  },
  {
    name: 'Bearer 认证通过',
    detail: 'HTTP 用例 · 绑定「Bearer Token 认证」· 断言 200 与 $.authenticated = true',
  },
]

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
    title: '抓一次真实流量',
    desc: '开始录制、信任根证书并接管系统代理，浏览器 / App 的请求会按域名分组出现；手机同 Wi-Fi 也能抓。',
    action: '打开抓包',
    go: () => router.push('/capture'),
  },
  {
    key: '05',
    title: '发起压测',
    desc: '配置并发与时长，结束后自动生成报告与逐请求明细。',
    action: '去压测',
    go: () => router.push('/stress'),
  },
  {
    key: '06',
    title: '沉淀回归套件',
    desc: '把常用接口保存成自动化测试套件，配好断言、变量链与失败策略后反复运行；示例项目已内置「示例回归（httpbin）」可直接试跑。',
    action: '打开自动化测试',
    go: () => router.push('/automation'),
  },
  {
    key: '07',
    title: '签名与加密',
    desc: '脚本页「插入示例」可一键填入 MD5 / HMAC 签名、AES 加密、RSA 加密，生成的值用 {{变量}} 引用。',
    action: '打开项目',
    go: goProject,
  },
  {
    key: '08',
    title: '导入现成接口',
    desc: '项目页可粘贴 cURL 一键解析；工作台可导入项目 JSON，用于同事共享或换机迁移。',
    action: '打开项目',
    go: goProject,
  },
  {
    key: '09',
    title: '用航仔 提效',
    desc: '在「航仔」里说一句话就能建项目、写接口、发请求、写签名脚本、查压测报告，并能导出执行记录。',
    action: '打开航仔',
    go: () => router.push({ name: 'ai' }),
  },
  {
    key: '10',
    title: '数据与备份',
    desc: '数据默认存在本机 SQLite；设置页可查看路径、迁移目录，项目页可导出 JSON 备份。',
    action: '打开设置',
    go: () => router.push('/settings'),
  },
])

const pathDone = computed(() => {
  const s = stats.value
  if (!s) return 0
  const flags = [
    s.projectCount > 0,
    s.environmentCount > 0,
    s.requestCount > 0,
    s.historyCount > 0,
    s.reportCount > 0,
  ]
  let done = 0
  for (const flag of flags) {
    if (!flag) break
    done += 1
  }
  return done
})

const pathNext = computed(() => steps.value[Math.min(pathDone.value, steps.value.length - 1)])

function openGuide() {
  guideOpen.value = true
  void nextTick(() => {
    document.getElementById('guide')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  })
}

function closeGuide() {
  guideOpen.value = false
}

function pad2(value: number) {
  return String(value).padStart(2, '0')
}

function methodClass(method: string) {
  const value = method.toLowerCase()
  if (value === 'get' || value === 'post' || value === 'put' || value === 'patch') return value
  if (value === 'delete' || value === 'del') return 'delete'
  return 'other'
}

function shortPath(url: string) {
  try {
    const parsed = new URL(url)
    return `${parsed.pathname}${parsed.search}` || '/'
  } catch {
    return url.replace(/^[a-z]+:\/\/[^/]+/i, '') || url
  }
}

function formatRelative(ts: number) {
  const diff = Date.now() - ts
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  if (diff < 172_800_000) return '昨天'
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`
  return new Date(ts).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' })
}

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
    title: 'HTTPS 抓包三步',
    desc: '先录制，再装并信任根证书，最后接管系统代理，顺序别反。证书没信任时，HTTPS 只会显示 CONNECT、看不到明文。',
    code: '录制 → CA 一键安装 → 接管系统代理\ncurl -x http://127.0.0.1:8888 https://example.com',
  },
  {
    title: '手机抓包与微信边界',
    desc: '监听切 0.0.0.0，手机同 Wi-Fi 填电脑 IP:端口并安装证书；iOS 还要打开完全信任。微信可能绕过 Wi-Fi 代理，自己的小程序走开发者工具，其他优先 iPhone。',
    code: '',
  },
  {
    title: '重发改参与未录制模式',
    desc: '详情页「重发」可改方法、URL、请求头和 Body。录制中会新增一条流量；未录制时直接发送，响应只在弹窗展示，不影响录制状态。',
    code: '',
  },
  {
    title: '压测参数怎么起步',
    desc: '建议 10～30 并发、30 秒时长起步，观察错误率与 P95 后再加；可配 P95 / P99 / 错误率 SLA 自动判定。',
    code: '',
  },
  {
    title: '把批量运行固化成套件',
    desc: '在批量运行页勾选好接口跑通后，点「另存为测试套件」，接口、断言与 CSV 数据文件会一起带进自动化测试页，之后一键复跑。',
    code: '批量运行 → 另存为测试套件 → 自动化测试页运行（自动预检）',
  },
  {
    title: '压测用例做容量门禁',
    desc: '套件里选「压测用例」，粘贴压测配置 JSON，可加 slaOverride 设阈值、baselineReportId 与历史报告对比；回归时延迟或错误率恶化会自动判失败。',
    code: '{\n  "slaOverride": { "p95Ms": 800, "errorRate": 1.0 },\n  "baselineReportId": 12\n}',
  },
  {
    title: 'AI 助手：自然语言操作',
    desc: '点右侧屏幕中间的机器人进入航仔。先在航仔 顶部「模型服务」里配置一家服务商（如 DeepSeek / Kimi / 智谱 / Claude），之后直接说需求即可。写入类操作会先给计划、等你确认后再执行。',
    code: '列出本机所有项目，并按请求数量排序\n在第一个项目里新建登录接口，发送并分析返回\n给接口写一个前置签名脚本，再发一次验证',
  },
  {
    title: 'AI 助手：聊天与出方案',
    desc: '航仔 也能当日常助手：解释概念、给压测方案、做错误诊断、写脚本草稿，这类问答不会去翻你的项目数据。',
    code: 'HTTP/2 和 HTTP/1.1 有什么区别\n给我一份下单接口的压测方案，含并发与 SLA\n这个 401 报错一般怎么排查',
  },
  {
    title: 'AI 助手：写脚本与查数据',
    desc: '可让 航仔 直接写前置 / 后置脚本（保存前做语法检查），也能查压测报告、按接口或项目导出执行记录为 CSV / JSON。',
    code: '给「创建订单」接口写 HMAC-SHA256 签名前置脚本\n导出某接口最近 50 条执行记录为 CSV\n最近一次压测报告的关键指标怎么样',
  },
  {
    title: 'AI 用量与费用',
    desc: '每次模型调用都会记一条：服务商、模型、输入 / 输出 token、耗时与估算费用，近 1 年可查；航仔 顶部「用量统计」进入。',
    code: '',
  },
  {
    title: '数据备份与迁移',
    desc: '项目页或卡片菜单导出 *.astroman.json 交给同事；设置页可查看数据库路径并迁移数据目录。',
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
    q: '测试场景（自动化套件）和批量运行、压测有什么区别？',
    a: '三者共用同一套请求执行内核，侧重不同。批量运行是临时任务：现场勾选接口、跑完看结果，不留长期资产；测试场景把「跑哪些接口、按什么顺序、断言什么」固化成可命名、可反复运行的套件，支持用例间依赖（前置用例失败时后续自动跳过）、setup/teardown、数据集与 Python 钩子，每次运行落库并产出 HTML / JSON / JUnit 报告；压测用例是套件里的一种特殊用例，用 SLA 阈值或与基线报告对比来做容量门禁。一句话：临时验一批用批量运行，要长期回归用测试场景，要看并发能力去压测。',
  },
  {
    q: '测试场景里的 Python 脚本怎么保证安全？',
    a: 'Python 默认不执行：只有在「设置 → 网络与安全」里打开 Python 脚本开关后才会真正跑，导入含 Python 脚本的套件包时同样要求本机已授权，否则直接拒绝导入。脚本在本机子进程里运行，带超时、输出与附件大小上限；解释器可在同一处指定，留空则按 python3 / python / py -3 自动探测，点「探测」能先看到版本。每次点「运行」都会自动先预检，解释器不可用、缺数据集、依赖成环、缺 secret 都会提前列出并阻止运行。',
  },
  {
    q: '接口自带的 JS 脚本能在测试场景里跑吗？',
    a: '不能直接跑，预检会把这些接口列出来提醒你。测试场景的前后置钩子用 Python；需要保留原逻辑时，把 JS 里的签名或取值改写为 Python 即可，运行时提供环境变量、变量表、响应对象、断言与加解密（MD5 / HMAC / AES / RSA）等常用能力。',
  },
  {
    q: '怎么在 CI 里跑测试套件？',
    a: '用随安装包提供的 astroman-cli：astroman-cli test run --suite "套件名" --report junit,html --out ./artifacts，还可加 --project / --env 定位套件、--fail-fast 遇错即停。它与桌面端使用同一个 Rust 执行内核，退出码 0 表示全部通过，非 0 分别对应用例失败、预检不通过、参数错误等情况，可直接作为流水线门禁；JUnit 结果还能被 CI 原生展示。',
  },
  {
    q: '怎么把项目分享给同事？',
    a: '项目页顶部或工作台项目卡片菜单里选「导出项目」，会得到 *.astroman.json（含集合、接口、脚本与断言）。对方在工作台点「导入项目 → 导入项目 JSON」即可复原；重名时会自动加「（导入）」后缀，不影响原有项目。',
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
    q: '抓包能解密 HTTPS，但列表里看不到明文？',
    a: '看顶部「CA 未安装 / CA 已信任」。HTTPS 解密要求目标程序信任 AstroMan 根证书：点「CA 未安装」→「一键安装」（Windows 装进当前用户受信任根，无需管理员），装完重启目标程序；如果重新生成过证书，要再点一次「重新安装」。Firefox 使用独立证书库，需要单独导入；部分 App 做了证书固定（pinning），这类流量无法解密。',
  },
  {
    q: '手机开代理后断网，或者抓不到微信？',
    a: '手机开了代理像断网，通常是根证书没装或没信任：先导出证书到手机安装，iOS 还要到「设置 → 通用 → 关于本机 → 证书信任设置」打开完全信任。代理地址要填电脑局域网 IP:端口，填 127.0.0.1 会指向手机自己。微信可能绕过 Wi-Fi 代理、Android 7+ 的 App 也默认不信任用户证书：自己的小程序用微信开发者工具设置代理并勾选「不校验合法域名…」，别人的小程序优先用 iPhone，Android 通常需要 root。AstroMan 不做 VPN / TUN 全局劫持。',
  },
  {
    q: '抓包流量保存在哪里？会占多少内存？',
    a: '抓包流量不写数据库，只存在内存：最多 5000 条、body 合计 128 MB，超出淘汰最旧一条，重启应用即清空。单条 body 最多保留 2 MB 前缀，超过 8 MB 不缓冲，图片只预览 1 MB 内。需要留档请及时「导出 HAR / ZIP」，或在详情里把请求 / 响应 Body 另存为文件。',
  },
  {
    q: '航仔 能帮我做什么？',
    a: '两个模式：一是聊天答疑、出方案（压测方案、测试策略、脚本写法、错误诊断），只给建议不动数据；二是操作本机——新建项目 / 集合 / 接口、真实发送请求并解读响应、写前置与后置脚本、批量执行、生成压测计划、查询历史与报告、导出执行记录。凡是要新建、修改、删除、导入导出或发起压测，它都会先把完整计划列出来，你确认后才弹执行框。',
  },
  {
    q: 'AI 服务怎么配置？密钥安全吗？',
    a: '进入航仔，点顶部「模型服务」→「添加模型服务」，选一个厂商预设（自动填好协议、Base URL 和模型下拉），再填自己的 API Key 即可。支持 OpenAI 兼容、Claude、Gemini 与本地 Ollama；密钥用 AES-256-GCM 加密后只保存在本机数据库，不会上传到任何第三方。',
  },
  {
    q: 'AI 用了多少 token、花了多少钱？',
    a: '每次模型调用都会记录服务商、模型、输入 / 输出 token、耗时与估算费用，保留近 1 年。在航仔 顶部点「用量统计」可看指标卡、每日趋势、逐条调用记录（含本轮提问）与各工具准确率，费用按厂商公开单价估算并折算人民币。',
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

  const [st, storageInfo, info, dash, cap, providers] = await Promise.allSettled([
    api.workspaceStats(),
    api.storageInfo(),
    api.appInfo(),
    api.dashboardMetrics(),
    captureStatus(),
    api.aiProviderList(),
  ])
  if (st.status === 'fulfilled') stats.value = st.value
  if (storageInfo.status === 'fulfilled') storage.value = storageInfo.value
  if (info.status === 'fulfilled') appVersion.value = info.value.version
  if (dash.status === 'fulfilled') metrics.value = dash.value
  if (cap.status === 'fulfilled') {
    proxyOn.value = cap.value.sysproxyOn
    captureRunning.value = cap.value.state === 'running'
  }
  if (providers.status === 'fulfilled') {
    const list = providers.value
    const target = list.find((item) => item.isDefault) ?? list[0]
    if (target) {
      modelLabel.value = `${target.model || target.name}${target.hasKey === false ? ' · 未配置密钥' : ' · 已配置'}`
    }
  }

  await loadRecentProjects()
  await loadNotes()
  document.addEventListener('visibilitychange', onNotesVisible)
  try {
    unlistenNotes = await listen('notes:changed', () => {
      void loadNotes()
    })
  } catch {
    unlistenNotes = null
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('visibilitychange', onNotesVisible)
  unlistenNotes?.()
  unlistenNotes = null
})

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
  await loadRecentProjects()
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
      title: '导入 AstroMan 项目',
      multiple: false,
      filters: [{ name: 'AstroMan 项目文件', extensions: ['json'] }],
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
      defaultPath: `${safeName}.astroman.json`,
      filters: [{ name: 'AstroMan 项目文件', extensions: ['json'] }],
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
      title: '导入 Postman / Apifox / HAR 数据',
      multiple: false,
      filters: [{ name: 'JSON / HAR 文件', extensions: ['json', 'har'] }],
    })
    if (!path) return
    importingApi.value = true
    const project = await api.apiImport(path, null)
    await store.loadProjects()
    refreshStats()
    message.success(`已导入「${project.name}」`)
    if (path.toLowerCase().endsWith('.har')) {
      message.info('HAR 中 Authorization / Cookie 已默认停用，上传文件字段与 base64 请求体未导入', 6)
    }
    router.push(`/project/${project.id}`)
  } catch (e) {
    message.error(String(e))
  } finally {
    importingApi.value = false
  }
}

function onImportMenu(info: { key: string | number }) {
  if (info.key === 'astroman') importProject()
  else if (info.key === 'api') importApiProject()
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
            <a-menu-item key="astroman">导入项目 JSON</a-menu-item>
            <a-menu-item key="api">导入 Postman / Apifox / HAR 数据</a-menu-item>
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
      <a-button type="primary" @click="router.push('/projects')">
        <template #icon><FolderOpenOutlined /></template>
        项目管理
      </a-button>
    </PageHeader>

    <!-- 总览 -->
    <section class="console fm-card">
      <div class="grid-bg" aria-hidden="true" />
      <div class="console-left">
        <div class="console-status">
          <span class="dot" />
          <span>本地工作区 · 已就绪</span>
          <span class="sep" />
          <span>{{ storage ? fmtBytes(storage.dbSize + storage.walSize) : '—' }}</span>
          <span class="sep" />
          <span>{{ storage?.isCustom ? '自定义数据目录' : '默认数据目录' }}</span>
        </div>
        <h2 class="console-title">本地工作区已就绪</h2>
        <p class="console-desc">
          项目、接口、环境变量与压测报告全部保存在本机 SQLite，无需登录，断网可用。
        </p>
        <div class="console-actions">
          <a-button type="primary" @click="router.push('/projects')">
            <template #icon><FolderOpenOutlined /></template>
            进入项目管理
          </a-button>
          <a-button :disabled="!recentProject" @click="goEnvironment">环境变量</a-button>
          <a-button @click="router.push('/stress')">查看报告</a-button>
        </div>
      </div>

      <div class="kpis">
        <button v-for="item in kpis" :key="item.key" type="button" class="kpi" @click="item.go()">
          <span class="kpi-label">{{ item.label }}</span>
          <b class="kpi-value">
            {{ item.value == null ? '—' : item.key === 'todo' ? pad2(item.value) : fmtInt(item.value) }}
          </b>
          <em class="kpi-note" :class="{ up: item.up }">{{ item.note }}</em>
        </button>
      </div>
    </section>

    <!-- 最近项目 + 今日待办 -->
    <div class="cols">
      <section class="panel fm-card">
        <header>
          <h3>最近项目<span>Recent / 5</span></h3>
          <a @click="router.push('/projects')">全部 →</a>
        </header>
        <table class="tbl fm-table">
          <thead>
            <tr>
              <th>项目</th>
              <th>最近接口</th>
              <th class="num">接口数</th>
              <th class="num">更新</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in recentProjects"
              :key="row.id"
              @click="router.push(`/project/${row.id}`)"
            >
              <td class="nm">{{ row.name }}</td>
              <td>
                <template v-if="row.last">
                  <span class="fm-mt" :class="methodClass(row.last.method)">{{ row.last.method }}</span>
                  <span class="path" :title="row.last.url">{{ shortPath(row.last.url) }}</span>
                </template>
                <span v-else class="muted">—</span>
              </td>
              <td class="num">{{ row.count == null ? '—' : fmtInt(row.count) }}</td>
              <td class="num">{{ formatRelative(row.updatedAt) }}</td>
            </tr>
            <tr v-if="!recentProjects.length">
              <td colspan="4" class="empty">还没有项目，先新建一个。</td>
            </tr>
          </tbody>
        </table>
      </section>

      <section class="panel fm-card">
        <header>
          <h3>今日待办<span>Notes</span></h3>
          <a @click="router.push('/notes')">记事本 →</a>
        </header>
        <div class="todo">
          <div
            v-for="item in notePreview"
            :key="item.id"
            class="row"
            :class="{ done: item.status === 'done' }"
            @click="router.push('/notes')"
          >
            <button
              type="button"
              class="box"
              :class="{ on: item.status === 'done' }"
              :aria-label="item.status === 'done' ? '标记未完成' : '标记完成'"
              @click.stop="toggleNoteDone(item, item.status !== 'done')"
            />
            <span class="todo-title">{{ item.title || '未命名' }}</span>
            <span
              v-if="item.dueAt"
              class="time"
              :class="{ late: item.dueAt < noteNow && item.status !== 'done' }"
            >
              {{ formatDueLabel(item.dueAt, noteNow) }}
            </span>
          </div>
          <div v-if="!notePreview.length" class="todo-empty">今天还没有待办，下面记一笔。</div>
        </div>
        <div class="quick">
          <PlusOutlined class="quick-icon" />
          <input
            v-model="noteQuick"
            class="quick-input"
            placeholder="记一笔… 明天10点 交周报 #工作"
            @keydown="onNoteQuickKeydown"
          />
          <a-spin v-if="noteSaving" size="small" />
          <button
            v-else
            type="button"
            class="quick-save"
            :disabled="!noteQuick.trim()"
            @click="saveNoteQuick"
          >
            记一笔
          </button>
        </div>
      </section>
    </div>

    <!-- 快捷入口 -->
    <div class="launch-grid">
      <button v-for="item in launches" :key="item.key" type="button" class="launch fm-card" @click="item.go()">
        <span class="launch-ic"><component :is="item.icon" /></span>
        <span class="launch-body">
          <b>{{ item.title }}</b>
          <span>{{ item.desc }}</span>
        </span>
      </button>
    </div>

    <!-- 上手路径 -->
    <div class="pathband">
      <span class="pl">上手路径</span>
      <span class="pn">
        10 节 · 已完成 <b>{{ pad2(pathDone) }}</b> 节 · 下一步「{{ pathNext.title }}」
      </span>
      <span class="gauge" aria-hidden="true">
        <i
          v-for="(step, index) in steps"
          :key="step.key"
          :class="{ on: index < pathDone, cur: index === pathDone }"
        />
      </span>
      <span class="pnum">{{ pad2(pathDone) }} / 10</span>
      <a class="plink" @click="guideOpen ? closeGuide() : openGuide()">
        {{ guideOpen ? '收起使用说明 ↑' : '打开使用说明 →' }}
      </a>
    </div>

    <div v-show="guideOpen" id="guide" class="guide">

    <!-- 测试场景介绍 -->
    <section class="runner-band suite-band fm-card">
      <div class="runner-copy">
        <span class="runner-eyebrow"><ExperimentOutlined /> 自动化测试 · 测试场景</span>
        <h2 class="runner-title">把回归沉淀成套件，一键跑完再出报告</h2>
        <p class="runner-desc">
          测试场景把「这次跑哪些接口」变成可保存、可反复运行的套件：用例绑定接口并声明断言，
          前后置逻辑用 Python 钩子，容量门禁用压测用例（SLA 与基线对比自动判定）。
          点「运行」会先自动预检，缺数据集、依赖成环、脚本未授权都会提前列出；结果落库并可导出
          HTML / JSON / JUnit，方便归档或接进流水线。
        </p>
        <div class="runner-tags">
          <span>套件可保存</span>
          <span>声明式断言</span>
          <span>Python 前后置</span>
          <span>压测 SLA 门禁</span>
          <span>预检与失败策略</span>
          <span>CLI / CI 同源</span>
        </div>
        <div class="runner-actions">
          <a-button type="primary" @click="router.push('/automation')">
            <template #icon><ExperimentOutlined /></template>
            打开自动化测试
          </a-button>
          <a-button :disabled="!recentProject" @click="router.push('/runner')">
            从批量运行迁移
          </a-button>
        </div>
      </div>

      <div class="runner-steps">
        <div v-for="(s, i) in suiteSteps" :key="s.title" class="runner-step">
          <span class="runner-step-index mono">0{{ i + 1 }}</span>
          <div class="runner-step-body">
            <div class="runner-step-title">{{ s.title }}</div>
            <div class="runner-step-desc">{{ s.desc }}</div>
          </div>
        </div>
      </div>
    </section>

    <!-- 测试场景示例 -->
    <div class="fm-card runner-example">
      <div class="example-head">
        <span class="example-title">
          <FileTextOutlined /> 示例：内置的「示例回归（httpbin）」
        </span>
        <span class="example-note">示例项目自带 3 个用例，可直接运行</span>
      </div>
      <div class="example-grid">
        <div class="example-col">
          <div class="example-label">用例编排</div>
          <div class="example-results">
            <div v-for="c in suiteSampleCases" :key="c.name">
              <b class="mono">{{ c.name }}</b> {{ c.detail }}
            </div>
          </div>
        </div>
        <div class="example-col">
          <div class="example-label">断言写法（JSON）</div>
          <pre class="example-code mono">{{ suiteAssertionExample }}</pre>
        </div>
        <div class="example-col">
          <div class="example-label">运行产物</div>
          <div class="example-results">
            <div><b class="mono">预检</b> 解释器 / 数据集 / 依赖 / secret</div>
            <div><b class="mono">结果</b> 通过 · 失败 · 跳过（依赖未满足）</div>
            <div><b class="mono">报告</b> HTML / JSON / JUnit + 运行日志</div>
          </div>
        </div>
      </div>
      <div class="example-foot">
        没有接口？先在示例项目里试跑，或到批量运行勾选好接口后点「另存为测试套件」直接带过来。
        命令行与桌面端使用同一执行内核，可接进流水线做门禁：
        <pre class="example-code mono">{{ suiteCliExample }}</pre>
      </div>
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

    <!-- 抓包介绍 -->
    <section class="runner-band capture-band fm-card">
      <div class="runner-copy">
        <span class="runner-eyebrow"><GlobalOutlined /> 抓包分析 · Capture</span>
        <h2 class="runner-title">看清 App 真正发了什么，再改参数重发一次</h2>
        <p class="runner-desc">
          抓包用本机 MITM 代理录制 HTTP / HTTPS：开始录制、信任根证书、让流量走代理，
          请求就会按域名分组出现在左侧。点开可看请求头 / Body、响应头 / Body 与原始报文，
          文本能格式化、复制、另存，图片直接预览。
          顶部可导出全部 HAR / ZIP（ZIP 解压后打开 index.html 即可阅读），域名分组右键可只导出该分组；任意请求还能改方法、URL、头与 Body 重发。
        </p>
        <div class="runner-tags">
          <span>HTTP / HTTPS 解密</span>
          <span>域名分组</span>
          <span>根证书一键安装</span>
          <span>系统代理接管</span>
          <span>手机同 Wi-Fi</span>
          <span>HAR / ZIP / 改参重发</span>
        </div>
        <div class="runner-actions">
          <a-button type="primary" @click="router.push('/capture')">
            <template #icon><GlobalOutlined /></template>
            打开抓包
          </a-button>
          <a-button @click="router.push('/history')">对照历史请求</a-button>
        </div>
      </div>

      <div class="runner-steps">
        <div v-for="(s, i) in captureSteps" :key="s.title" class="runner-step">
          <span class="runner-step-index mono">0{{ i + 1 }}</span>
          <div class="runner-step-body">
            <div class="runner-step-title">{{ s.title }}</div>
            <div class="runner-step-desc">{{ s.desc }}</div>
          </div>
        </div>
      </div>
    </section>

    <!-- 抓包示例与排错 -->
    <div class="fm-card runner-example">
      <div class="example-head">
        <span class="example-title"><FileTextOutlined /> 示例：让本机 / 手机流量进入抓包列表</span>
        <span class="example-note">证书没信任时，HTTPS 只会显示 CONNECT</span>
      </div>
      <div class="example-grid">
        <div class="example-col">
          <div class="example-label">本机命令行验证</div>
          <pre class="example-code mono">{{ captureCurlExample }}</pre>
          <div class="example-results example-gap">
            <div><b class="mono">浏览器</b> 点「接管系统代理」即可</div>
            <div><b class="mono">命令行</b> 显式加 -x，否则不走系统代理</div>
          </div>
        </div>
        <div class="example-col">
          <div class="example-label">手机抓包清单</div>
          <div class="example-results">
            <div v-for="item in capturePhoneChecklist" :key="item.name">
              <b class="mono">{{ item.name }}</b> {{ item.detail }}
            </div>
          </div>
        </div>
        <div class="example-col">
          <div class="example-label">边界与排错</div>
          <div class="example-results">
            <div v-for="item in captureBoundaries" :key="item.name">
              <b class="mono">{{ item.name }}</b> {{ item.detail }}
            </div>
          </div>
        </div>
      </div>
      <div class="example-foot">
        流量只存内存（5000 条 / 128 MB body），重启清空；需要留档及时「导出 HAR / ZIP」或在详情里保存 Body。
        更完整的手机、微信与证书排查见 <code class="mono">docs/capture.md</code>。
      </div>
    </div>

    <!-- 产品介绍 -->
    <section class="intro-band fm-card">
      <div class="intro-copy">
        <span class="intro-eyebrow">
          <RocketOutlined />
          产品概览
        </span>
        <h2 class="intro-title">调试、抓包、压测与工具箱，一个桌面应用全部搞定</h2>
        <p class="intro-desc">
          AstroMan 面向需要频繁调试 HTTP 接口的开发者：界面参考 Postman，压测参数参考 JMeter，
          抓包体验接近 Charles，日常格式化与转换工具集中在一起。不用在浏览器插件和在线站点之间来回切换，
          也不用把接口数据交给第三方服务。
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
        <h2 class="section-title">AstroMan 能做什么</h2>
        <div class="section-desc">从接口调试、抓包到压力测试，再到日常开发小工具，全部离线可用</div>
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

    <!-- 上手引导 -->
    <div class="section-head">
      <div>
        <h2 class="section-title">使用说明</h2>
        <div class="section-desc">从建项目到调试、抓包、批量运行、回归套件、压测与备份，右侧是常见问题</div>
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

      <div class="guide-foot">
        <a class="plink" @click="closeGuide">收起使用说明 ↑</a>
      </div>
    </div>

    <!-- 底部状态条 -->
    <div class="statusbar">
      <span>SQLite · {{ storage ? fmtBytes(storage.dbSize + storage.walSize) : '—' }}</span>
      <span class="d">│</span>
      <span>
        {{ captureRunning ? '抓包录制中' : proxyOn ? '系统代理 已接管' : '系统代理 未接管' }}
      </span>
      <span class="d">│</span>
      <span>模型 {{ modelLabel }}</span>
      <span class="d">│</span>
      <span>航仔任务 {{ hangzai.active ? hangzai.phaseText : '空闲' }}</span>
      <span class="right">AstroMan v{{ appVersion || '1.0.0' }} · 本地工作区</span>
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
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) 520px;
  overflow: hidden;
  background: var(--fm-surface);
}

.grid-bg {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background-image:
    linear-gradient(to right, var(--fm-border) 1px, transparent 1px),
    linear-gradient(to bottom, var(--fm-border) 1px, transparent 1px);
  background-size: 26px 26px;
  opacity: 0.4;
  mask-image: linear-gradient(105deg, #000, transparent 62%);
  -webkit-mask-image: linear-gradient(105deg, #000, transparent 62%);
}

.console-left {
  position: relative;
  padding: 24px 26px 22px;
  min-width: 0;
}

.console-status {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 9px;
  font-family: var(--fm-font-mono);
  font-size: 11px;
  letter-spacing: 0.02em;
  color: var(--fm-muted);
}

.console-status .dot {
  width: 5px;
  height: 5px;
  flex: none;
  background: var(--fm-success);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fm-success) 14%, transparent);
}

.console-status .sep {
  width: 1px;
  height: 9px;
  background: var(--fm-border);
}

.console-title {
  margin: 16px 0 8px;
  font-family: var(--fm-font-serif);
  font-size: 23px;
  font-weight: 600;
  letter-spacing: 0.04em;
  line-height: 1.3;
  color: var(--fm-text);
}

.console-desc {
  margin: 0 0 18px;
  max-width: 400px;
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--fm-muted);
}

.console-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.kpis {
  position: relative;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  border-left: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
}

.kpi {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  padding: 22px 20px;
  border: 0;
  border-right: 1px solid var(--fm-border);
  border-bottom: 1px solid var(--fm-border);
  background: transparent;
  text-align: left;
  cursor: pointer;
  transition: background var(--fm-dur-fast);
}

.kpi:nth-child(2n) {
  border-right: 0;
}

.kpi:nth-child(n + 3) {
  border-bottom: 0;
}

.kpi:hover {
  background: var(--fm-surface);
}

.kpi-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--fm-muted);
}

.kpi-label::before {
  content: '';
  width: 3px;
  height: 3px;
  background: var(--fm-brass);
}

.kpi-value {
  margin-top: 9px;
  font-family: var(--fm-font-mono);
  font-size: 30px;
  font-weight: 500;
  line-height: 1.15;
  letter-spacing: -0.01em;
  color: var(--fm-text);
  font-variant-numeric: tabular-nums;
}

.kpi-note {
  margin-top: 2px;
  max-width: 100%;
  font-family: var(--fm-font-mono);
  font-style: normal;
  font-size: 10.5px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kpi-note.up {
  color: var(--fm-success);
}

/* ---------- 最近项目 + 今日待办 ---------- */
.cols {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 372px;
  gap: 16px;
  align-items: start;
  margin-top: 16px;
}

.panel {
  overflow: hidden;
}

.panel > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--fm-border);
}

.panel > header h3 {
  margin: 0;
  font-size: 12.5px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--fm-text);
}

.panel > header h3 span {
  margin-left: 8px;
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  font-weight: 400;
  letter-spacing: 0.14em;
  color: var(--fm-muted);
}

.panel > header a {
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  color: var(--fm-primary);
  cursor: pointer;
}

.tbl th {
  padding: 8px 16px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.tbl td {
  padding: 9px 16px;
}

.tbl th.num {
  text-align: right;
}

.tbl tbody tr {
  cursor: pointer;
}

.tbl .path {
  display: inline-block;
  max-width: 240px;
  margin-left: 7px;
  font-family: var(--fm-font-mono);
  font-size: 11px;
  color: var(--fm-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: bottom;
}

.tbl .muted {
  color: var(--fm-muted);
}

.tbl .empty {
  padding: 22px 16px;
  text-align: center;
  font-size: 11.5px;
  color: var(--fm-muted);
}

.todo {
  display: flex;
  flex-direction: column;
}

.todo .row {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 12px;
  cursor: pointer;
}

.todo .row:last-child {
  border-bottom: 0;
}

.todo .row:hover {
  background: var(--fm-surface-alt);
}

.todo .box {
  width: 12px;
  height: 12px;
  flex: none;
  padding: 0;
  border: 1px solid var(--fm-border-strong);
  background: var(--fm-surface);
  cursor: pointer;
}

.todo .box.on {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
}

.todo .todo-title {
  flex: 1;
  min-width: 0;
  color: var(--fm-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.todo .row.done .todo-title {
  color: var(--fm-muted);
  text-decoration: line-through;
}

.todo .time {
  margin-left: auto;
  flex: none;
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
}

.todo .time.late {
  color: var(--fm-danger);
}

.todo-empty {
  padding: 18px 16px;
  font-size: 11.5px;
  color: var(--fm-muted);
}

.quick {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 12px 16px 14px;
  padding: 6px 9px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  transition: border-color var(--fm-dur-fast);
}

.quick:focus-within {
  border-color: var(--fm-primary);
}

.quick-icon {
  flex: none;
  color: var(--fm-primary);
  font-size: 12px;
}

.quick-input {
  flex: 1;
  min-width: 0;
  height: 20px;
  border: 0;
  background: transparent;
  outline: none;
  color: var(--fm-text);
  font-size: 11.5px;
}

.quick-input::placeholder {
  color: var(--fm-muted);
}

.quick-save {
  flex: none;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--fm-primary);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  cursor: pointer;
}

.quick-save:disabled {
  color: var(--fm-muted);
  cursor: default;
}

/* ---------- 快捷入口 ---------- */
.launch-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  margin-top: 16px;
}

.launch {
  display: flex;
  align-items: flex-start;
  gap: 11px;
  padding: 14px;
  border: 1px solid var(--fm-border);
  text-align: left;
  cursor: pointer;
  transition: border-color var(--fm-dur-fast), background var(--fm-dur-fast);
}

.launch:hover {
  border-color: var(--fm-border-strong);
  background: var(--fm-surface-alt);
}

.launch-ic {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  flex: none;
  border: 1px solid var(--fm-border);
  color: var(--fm-primary);
  font-size: 14px;
}

.launch-body {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.launch-body b {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
}

.launch-body span {
  margin-top: 3px;
  font-size: 10.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

/* ---------- 上手路径 / 状态条 ---------- */
.pathband {
  display: flex;
  align-items: center;
  gap: 14px;
  height: 48px;
  margin-top: 16px;
  padding: 0 16px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
}

.pathband .pl {
  display: flex;
  align-items: center;
  height: 20px;
  padding-right: 14px;
  border-right: 1px solid var(--fm-border);
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
  white-space: nowrap;
}

.pathband .pn {
  font-size: 12px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.pathband .pn b {
  font-weight: 600;
  color: var(--fm-text);
}

.gauge {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  margin-left: auto;
}

.gauge i {
  display: block;
  width: 21px;
  height: 5px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.gauge i.on {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
}

.gauge i.cur {
  height: 9px;
  background: var(--fm-primary-soft);
  border-color: var(--fm-primary);
}

.pnum {
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  color: var(--fm-text-2);
  white-space: nowrap;
}

.plink {
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  color: var(--fm-primary);
  cursor: pointer;
  white-space: nowrap;
}

.statusbar {
  position: sticky;
  bottom: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 9px;
  height: 28px;
  margin: 16px -24px -30px;
  padding: 0 16px;
  border-top: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.02em;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
}

.statusbar .d {
  color: var(--fm-border-strong);
  font-size: 9px;
}

.statusbar .right {
  margin-left: auto;
  overflow: hidden;
  text-overflow: ellipsis;
}

.guide {
  margin-top: 22px;
}

.guide-foot {
  display: flex;
  justify-content: center;
  padding: 16px 0 4px;
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
  border-radius: 999px;
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
  border-radius: var(--fm-radius);
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
  border-radius: var(--fm-radius);
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
  border-radius: var(--fm-radius-sm);
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
  border-radius: var(--fm-radius);
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
  border-radius: var(--fm-radius);
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

/* ---------- 批量运行 ---------- */
.runner-band {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  gap: 24px;
  margin-top: 16px;
  padding: 20px 22px;
}

.suite-band,
.capture-band {
  /* 平铺纸面，不再用径向光斑做装饰 */
  background: var(--fm-surface);
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
  border-radius: 999px;
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
  border-radius: var(--fm-radius);
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
  border-radius: var(--fm-radius);
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
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  font-size: 11px;
  color: var(--fm-text-2);
}

.example-results .fail {
  color: var(--fm-danger);
}

.example-gap {
  margin-top: 8px;
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
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
  font-size: 10.5px;
}

.example-foot .example-code {
  margin-top: 8px;
  font-size: 10.5px;
}

/* ---------- 响应式 ---------- */
@media (max-width: 1280px) {
  .cols {
    grid-template-columns: minmax(0, 1fr) 330px;
  }
}

@media (max-width: 1180px) {
  .console {
    grid-template-columns: minmax(0, 1fr);
  }

  .kpis {
    border-left: none;
    border-top: 1px solid var(--fm-border);
  }
}

@media (max-width: 1080px) {
  .cols {
    grid-template-columns: minmax(0, 1fr);
  }

  .launch-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .pathband {
    height: auto;
    flex-wrap: wrap;
    gap: 8px 14px;
    padding: 10px 16px;
  }

  .gauge {
    margin-left: 0;
  }
}

@media (max-width: 980px) {
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

  .help-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

@media (max-width: 760px) {
  .console-left {
    padding: 18px;
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

  .launch-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .kpi {
    padding: 16px 14px;
  }

  .kpi-value {
    font-size: 24px;
  }

  .help-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .statusbar {
    margin-left: -24px;
    margin-right: -24px;
    white-space: normal;
    height: auto;
    padding: 6px 16px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .launch,
  .project-card,
  .kpi {
    transition: none;
  }
}
</style>
