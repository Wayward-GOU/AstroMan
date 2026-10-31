import {
  executeScriptJob,
  SCRIPT_PHASE_LABEL,
  type ScriptJob,
  type ScriptJobResult,
} from './scriptRunner'

/**
 * 脚本宿主：Worker 生命周期 + 超时终止 + 串行队列 + 降级
 *
 * 为什么必须有超时：`new Function(...)()` 是同步的，`while(true)` 会把整个 WebView
 * 主线程占死，用户只能结束进程（方案 4.1 的现状）。放进 Worker 后，主线程只需要
 * 一个 `terminate()` 就能恢复，代价是丢掉这个 Worker，下一次任务重建即可。
 */

const DEFAULT_TIMEOUT_MS = 5000
const MIN_TIMEOUT_MS = 1000
const MAX_TIMEOUT_MS = 30_000
const TIMEOUT_STORAGE_KEY = 'astroman.script.timeoutMs'

/** 读取脚本超时（设置项，1–30s） */
export function scriptTimeoutMs(): number {
  try {
    const raw = Number(localStorage.getItem(TIMEOUT_STORAGE_KEY))
    if (!Number.isFinite(raw) || raw <= 0) return DEFAULT_TIMEOUT_MS
    return Math.min(MAX_TIMEOUT_MS, Math.max(MIN_TIMEOUT_MS, raw))
  } catch {
    return DEFAULT_TIMEOUT_MS
  }
}

export function setScriptTimeoutMs(value: number) {
  const clamped = Math.min(MAX_TIMEOUT_MS, Math.max(MIN_TIMEOUT_MS, Math.round(value)))
  try {
    localStorage.setItem(TIMEOUT_STORAGE_KEY, String(clamped))
  } catch {
    // 隐私模式下 localStorage 可能不可写，忽略即可，超时回落到默认值
  }
  return clamped
}

export const SCRIPT_TIMEOUT_RANGE = { min: MIN_TIMEOUT_MS, max: MAX_TIMEOUT_MS }

interface Pending {
  resolve: (result: ScriptJobResult) => void
  timer: ReturnType<typeof setTimeout>
}

interface WorkerReply {
  id: number
  result?: ScriptJobResult
  error?: string
}

class ScriptHost {
  private worker: Worker | null = null
  /** Worker 创建失败（老 WebView / CSP）后不再重试，直接走主线程降级 */
  private unavailable = false
  private seq = 0
  private pending = new Map<number, Pending>()
  /** 串行队列：Worker 同时只能跑一条脚本，并发调用要排队 */
  private chain: Promise<unknown> = Promise.resolve()

  /** 排队执行一条脚本任务；超时返回带原因的失败结果，不抛异常 */
  run(job: ScriptJob, timeoutOverride?: number): Promise<ScriptJobResult> {
    const exec = () => this.runOnce(job, timeoutOverride)
    const next = this.chain.then(exec, exec)
    // 队列本身不能因为某次失败断掉
    this.chain = next.then(
      () => undefined,
      () => undefined,
    )
    return next
  }

  private runOnce(job: ScriptJob, timeoutOverride?: number): Promise<ScriptJobResult> {
    const worker = this.ensure()
    if (!worker) {
      // 降级：没有 Worker 时同步执行，行为和旧版一致（阻塞主线程，但没有更优解）
      return Promise.resolve(executeScriptJob(job))
    }
    const timeout = Math.min(
      MAX_TIMEOUT_MS,
      Math.max(MIN_TIMEOUT_MS, timeoutOverride ?? scriptTimeoutMs()),
    )
    const started = performance.now()
    return new Promise<ScriptJobResult>((resolve) => {
      const id = ++this.seq
      const timer = setTimeout(() => {
        this.pending.delete(id)
        this.restart()
        resolve({
          request: job.request,
          // 超时：变量回滚到执行前快照，不保留半个脚本写入的值
          vars: job.vars,
          logs: [{ phase: job.phase, level: 'error', text: `脚本执行超时（${timeout}ms），已终止` }],
          assertions: [],
          error: `脚本执行超时（${formatMs(timeout)}），已终止（${SCRIPT_PHASE_LABEL[job.phase]}）；变量改动已回滚`,
          durationMs: Math.round(performance.now() - started),
        })
      }, timeout)
      this.pending.set(id, { resolve, timer })
      worker.postMessage({ id, job })
    })
  }

  private ensure(): Worker | null {
    if (this.unavailable) return null
    if (this.worker) return this.worker
    if (typeof Worker === 'undefined') {
      this.unavailable = true
      return null
    }
    try {
      const worker = new Worker(new URL('./scriptWorker.ts', import.meta.url), {
        type: 'module',
        name: 'astroman-script',
      })
      worker.onmessage = (event: MessageEvent<WorkerReply>) => this.handle(event.data)
      worker.onerror = () => this.failAll('脚本运行环境异常（Worker 启动失败），已重建')
      this.worker = worker
      return worker
    } catch {
      this.unavailable = true
      return null
    }
  }

  private handle(reply: WorkerReply) {
    const pending = this.pending.get(reply.id)
    if (!pending) return
    this.pending.delete(reply.id)
    clearTimeout(pending.timer)
    if (reply.result) {
      pending.resolve(reply.result)
      return
    }
    // Worker 内部抛错（例如注入对象构造失败）：保留错误原文
    pending.resolve({
      request: {} as ScriptJobResult['request'],
      vars: {},
      logs: [],
      assertions: [],
      error: reply.error ?? '脚本执行失败',
      durationMs: 0,
    })
  }

  /** 超时后必须重建：被 terminate 的 Worker 里可能还卡着死循环 */
  private restart() {
    this.destroy()
    this.worker = null
  }

  private failAll(message: string) {
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer)
      pending.resolve({
        request: {} as ScriptJobResult['request'],
        vars: {},
        logs: [],
        assertions: [],
        error: message,
        durationMs: 0,
      })
    }
    this.pending.clear()
    this.restart()
  }

  private destroy() {
    if (!this.worker) return
    try {
      this.worker.terminate()
    } catch {
      // 忽略：终止失败也不影响后续重建
    }
  }

  /** 应用退出 / 测试清理用 */
  dispose() {
    this.destroy()
    this.worker = null
  }
}

function formatMs(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(ms % 1000 === 0 ? 0 : 1)}s` : `${ms}ms`
}

export const scriptHost = new ScriptHost()

/**
 * 执行脚本任务的统一入口：优先 Worker，环境不支持时降级为主线程同步执行。
 */
export function runScriptJob(job: ScriptJob, timeoutOverride?: number): Promise<ScriptJobResult> {
  return scriptHost.run(job, timeoutOverride)
}
