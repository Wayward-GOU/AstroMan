/** JSON 格式化放到 Worker 里执行，避免大文本解析 + 重新序列化阻塞输入 */
interface PendingTask {
  resolve: (value: string) => void
  reject: (error: Error) => void
}

let worker: Worker | null = null
let workerBroken = false
let seq = 0
const pending = new Map<number, PendingTask>()

function failAll(error: Error) {
  for (const task of pending.values()) task.reject(error)
  pending.clear()
  worker?.terminate()
  worker = null
  workerBroken = true
}

function getWorker(): Worker | null {
  if (workerBroken || typeof Worker === 'undefined') return null
  if (worker) return worker
  try {
    worker = new Worker(new URL('./workers/jsonFormat.worker.ts', import.meta.url), {
      type: 'module',
    })
    worker.onmessage = (event: MessageEvent) => {
      const data = event.data as { id?: number; ok?: boolean; result?: string; error?: string }
      if (typeof data?.id !== 'number') return
      const task = pending.get(data.id)
      if (!task) return
      pending.delete(data.id)
      if (data.ok) task.resolve(data.result ?? '')
      else task.reject(new Error(data.error || 'JSON 解析失败'))
    }
    worker.onerror = () => failAll(new Error('格式化线程异常，请重试'))
    return worker
  } catch {
    workerBroken = true
    return null
  }
}

function formatSync(text: string): Promise<string> {
  return new Promise((resolve, reject) => {
    try {
      resolve(JSON.stringify(JSON.parse(text), null, 2))
    } catch (error) {
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}

/** 异步格式化 JSON；Worker 不可用时退回主线程同步实现 */
export function formatJsonAsync(text: string): Promise<string> {
  const target = getWorker()
  if (!target) return formatSync(text)

  const id = ++seq
  return new Promise<string>((resolve, reject) => {
    pending.set(id, { resolve, reject })
    try {
      target.postMessage({ id, text })
    } catch (error) {
      pending.delete(id)
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}
