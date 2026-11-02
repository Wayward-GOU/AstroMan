import { computed, ref } from 'vue'
import type { RouteLocationNormalized } from 'vue-router'

/**
 * 页面访问历史（会话内）。
 *
 * 目标：全局「返回」按钮始终回到**上一次停留的页面**，
 * 而不是写死某个上级路由。返回后再次跳转新页面会截断前进分支，
 * 与浏览器历史行为一致。
 */
interface NavEntry {
  path: string
  label: string
}

const MAX_ENTRIES = 60

const ROUTE_LABELS: Record<string, string> = {
  dashboard: '工作台',
  projects: '项目管理',
  project: '项目详情',
  request: '接口调试',
  environment: '环境变量',
  stress: '接口压测',
  runner: '批量运行',
  automation: '自动化测试',
  reports: '压测报告',
  'report-detail': '报告详情',
  history: '历史记录',
  ai: '航仔',
  'ai-logs': '工具调用记录',
  'ai-usage': '用量统计',
  tools: '工具箱',
  monitor: '系统监视',
  settings: '设置',
}

const stack = ref<NavEntry[]>([])
const cursor = ref(-1)
// 点了「返回」但还没落地的目标路径
let pendingBack: string | null = null

function labelFor(to: RouteLocationNormalized) {
  const name = typeof to.name === 'string' ? to.name : ''
  return ROUTE_LABELS[name] || '上一页'
}

function labelForPath(path: string) {
  if (path === '/') return ROUTE_LABELS.dashboard
  if (path.startsWith('/projects')) return ROUTE_LABELS.projects
  if (path.startsWith('/project/')) return ROUTE_LABELS.project
  if (path.startsWith('/reports')) return ROUTE_LABELS.reports
  if (path.startsWith('/ai')) return ROUTE_LABELS.ai
  return '上一页'
}

/**
 * 冷启动直接落在深层页面（如上次退出时停在接口调试）时，
 * 给栈底补一个逻辑上级，保证返回按钮不会一进来就是死路。
 */
function parentOf(to: RouteLocationNormalized): string | null {
  switch (to.name) {
    case 'project':
      return '/projects'
    case 'request': {
      const pid = to.query.projectId
      return typeof pid === 'string' && pid ? `/project/${pid}` : '/projects'
    }
    case 'environment': {
      const pid = String(to.params.projectId ?? '')
      return pid ? `/project/${pid}` : '/projects'
    }
    case 'report-detail':
      return '/reports'
    case 'ai-logs':
    case 'ai-usage':
      return '/ai'
    case 'dashboard':
      return null
    default:
      return '/'
  }
}

/** 记录一次页面切换（由 router.afterEach 调用） */
export function recordVisit(to: RouteLocationNormalized) {
  const path = to.fullPath
  const current = stack.value[cursor.value]
  if (current && current.path === path) return

  if (cursor.value < 0) {
    const parent = parentOf(to)
    if (parent && parent !== path) {
      stack.value = [
        { path: parent, label: labelForPath(parent) },
        { path, label: labelFor(to) },
      ]
      cursor.value = 1
      return
    }
  }

  // 返回上一页：目标命中则游标回退，不新增记录
  if (pendingBack) {
    const landed = pendingBack === path
    pendingBack = null
    if (landed) {
      cursor.value -= 1
      return
    }
  }

  // 从历史中间跳到新页面时丢掉未走的前进分支
  if (cursor.value < stack.value.length - 1) {
    stack.value = stack.value.slice(0, cursor.value + 1)
  }
  stack.value = [...stack.value, { path, label: labelFor(to) }]
  if (stack.value.length > MAX_ENTRIES) {
    stack.value = stack.value.slice(stack.value.length - MAX_ENTRIES)
  }
  cursor.value = stack.value.length - 1
}

/** 是否有可返回的上一个页面 */
export const canGoBack = computed(() => cursor.value > 0)

/** 上一个页面（用于按钮文案 / 悬浮提示） */
export const previousEntry = computed<NavEntry | null>(() =>
  cursor.value > 0 ? stack.value[cursor.value - 1] ?? null : null,
)

/**
 * 返回上一个页面，返回目标路径（无历史时为 null）。
 * 游标不在调用时回退，而是等路由真的落到目标页后由 recordVisit 回退，
 * 这样跳转被拦截 / 失败时不会让历史错位。
 */
export function stepBack(): string | null {
  if (cursor.value <= 0) return null
  const target = stack.value[cursor.value - 1]?.path ?? null
  pendingBack = target
  return target
}

/** 返回上一个页面；没有历史时退回 fallback */
export function backOr(fallback: string): string {
  return stepBack() ?? fallback
}

/** 供调试 / 测试使用 */
export function navStack() {
  return stack.value.slice()
}
