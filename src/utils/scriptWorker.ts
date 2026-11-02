import { executeScriptJob, type ScriptJob } from './scriptRunner'

/**
 * 脚本 Worker：常驻 1 个，同步执行脚本，结果通过消息回传。
 *
 * 这里不做超时控制——主线程 `terminate()` 是唯一能打断 `while(true)` 的手段，
 * Worker 自己的定时器在校验同一线程里，脚本一卡死就再也不会触发。
 */

interface WorkerRequest {
  id: number
  job: ScriptJob
}

interface WorkerReply {
  id: number
  result?: ReturnType<typeof executeScriptJob>
  error?: string
}

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null
  postMessage: (message: WorkerReply) => void
}

scope.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const { id, job } = event.data
  try {
    scope.postMessage({ id, result: executeScriptJob(job) })
  } catch (error) {
    // 连注入对象构造都失败（极少见）：把原因回传，主线程按脚本错误处理
    scope.postMessage({ id, error: error instanceof Error ? error.message : String(error) })
  }
}
