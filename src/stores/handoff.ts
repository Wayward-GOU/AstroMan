import { ref } from 'vue'

export type HandoffSource =
  | 'request'
  | 'capture'
  | 'report'
  | 'test-run'
  | 'history'
  | 'workspace'
  | 'environment'
  | 'project'

export interface HangzaiHandoff {
  source: HandoffSource
  title: string
  /** 页面快照，进入模型前会包 <untrusted_data> */
  prompt: string
  projectId?: number
  requestId?: number
  createdAt: number
}

const current = ref<HangzaiHandoff | null>(null)
/** 页面现场只在短时间内有效，避免过一会儿进航仔还带着旧数据 */
const TTL = 5 * 60_000

export function setHandoff(payload: Omit<HangzaiHandoff, 'createdAt'>) {
  current.value = { ...payload, createdAt: Date.now() }
}

export function peekHandoff(): HangzaiHandoff | null {
  const value = current.value
  if (!value) return null
  if (Date.now() - value.createdAt > TTL) {
    current.value = null
    return null
  }
  return value
}

/** 取走即消费：同一份页面现场不会被重复带进多轮对话 */
export function takeHandoff(): HangzaiHandoff | null {
  const value = peekHandoff()
  current.value = null
  return value
}

export function clearHandoff() {
  current.value = null
}

export const handoffLabel: Record<HandoffSource, string> = {
  request: '接口',
  capture: '抓包',
  report: '压测报告',
  'test-run': '自动化运行',
  history: '历史记录',
  workspace: '工作台',
  environment: '环境变量',
  project: '项目',
}
