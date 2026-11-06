import { defineStore } from 'pinia'

export type HangzaiPhase = 'thinking' | 'streaming' | 'tool' | 'confirm' | 'summarizing'

export interface TaskMirror {
  active: boolean
  runId: string | null
  sessionId: number | null
  sessionTitle: string
  phase: HangzaiPhase
  round: number
  tool: string | null
  startedAt: number
  awaitingConfirm: boolean
  canStop: boolean
  lastError: string
}

const blankTask = (): TaskMirror => ({
  active: false,
  runId: null,
  sessionId: null,
  sessionTitle: '',
  phase: 'thinking',
  round: 0,
  tool: null,
  startedAt: 0,
  awaitingConfirm: false,
  canStop: false,
  lastError: '',
})

let stopHandler: (() => Promise<void>) | null = null

/**
 * 航仔任务的跨页面状态镜像：只保存“正在做什么”和停止入口，
 * 真正的对话、消息与工具循环仍留在 AiAssistant 组件里（keep-alive 保活）。
 */
export const useHangzaiTaskStore = defineStore('hangzaiTask', {
  state: blankTask,
  getters: {
    phaseText: (state): string => {
      if (state.phase === 'confirm') return '等待你确认一个写操作'
      if (state.phase === 'tool') return `正在执行 ${state.tool ?? '工具'}`
      if (state.phase === 'streaming') return '正在生成回复'
      return '正在思考'
    },
  },
  actions: {
    update(patch: Partial<TaskMirror>) {
      Object.assign(this, patch)
    },
    clear() {
      Object.assign(this, blankTask())
    },
    fail(message: string) {
      this.lastError = message
      this.active = false
      this.canStop = false
      this.awaitingConfirm = false
    },
    bindStop(handler: () => Promise<void>) {
      stopHandler = handler
    },
    async requestStop() {
      if (!stopHandler) return
      await stopHandler()
    },
  },
})
