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

/* ---------- 含 {{变量}} 的 JSON 模板 ---------- */

/** 裸值占位符的哨兵前缀：还原时连引号一起替换回 {{变量}} */
const VAR_VALUE_PREFIX = '__ASTRO_VAR_VALUE_'
/** 字符串内占位符的哨兵前缀：保留原有引号，只替换文本 */
const VAR_STRING_PREFIX = '__ASTRO_VAR_STRING_'

interface MaskedTemplate {
  text: string
  valueVars: string[]
  stringVars: string[]
}

/**
 * 把 `{{变量}}` 换成合法 JSON 里的哨兵字符串。
 *
 * 请求模板里常见的写法是 `"ts": {{timestamp}}`（数值占位，不带引号），严格
 * `JSON.parse` 会直接失败。这里区分「字符串内」与「裸值」两种位置：
 * 裸值补上引号，字符串内只换文本，格式化后再按编号还原，语义与引号都不变。
 */
function maskJsonTemplateVars(text: string): MaskedTemplate {
  const valueVars: string[] = []
  const stringVars: string[] = []
  let out = ''
  let inString = false
  let escaped = false
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i]
    if (inString) {
      if (escaped) {
        escaped = false
        out += ch
        continue
      }
      if (ch === '\\') {
        escaped = true
        out += ch
        continue
      }
      if (ch === '"') {
        inString = false
        out += ch
        continue
      }
    } else if (ch === '"') {
      inString = true
      out += ch
      continue
    }
    if (ch === '{' && text[i + 1] === '{') {
      const end = text.indexOf('}}', i + 2)
      if (end > i) {
        const variable = text.slice(i, end + 2)
        if (inString) {
          out += `${VAR_STRING_PREFIX}${stringVars.length}__`
          stringVars.push(variable)
        } else {
          out += `"${VAR_VALUE_PREFIX}${valueVars.length}__"`
          valueVars.push(variable)
        }
        i = end + 1
        continue
      }
    }
    out += ch
  }
  return { text: out, valueVars, stringVars }
}

function restoreJsonTemplateVars(formatted: string, masked: MaskedTemplate): string {
  let out = formatted
  masked.valueVars.forEach((variable, index) => {
    out = out.split(`"${VAR_VALUE_PREFIX}${index}__"`).join(variable)
  })
  masked.stringVars.forEach((variable, index) => {
    out = out.split(`${VAR_STRING_PREFIX}${index}__`).join(variable)
  })
  return out
}

/**
 * 格式化可能含 `{{变量}}` 的 JSON 模板。
 *
 * 与 `formatJsonAsync` 的区别只在占位符：裸值占位（如 `"ts": {{timestamp}}`）
 * 也能正常格式化，且格式化后引号保持原样，可直接用于发送。
 */
export async function formatJsonTemplateAsync(text: string): Promise<string> {
  const masked = maskJsonTemplateVars(text)
  if (!masked.valueVars.length && !masked.stringVars.length) {
    return formatJsonAsync(text)
  }
  const formatted = await formatJsonAsync(masked.text)
  return restoreJsonTemplateVars(formatted, masked)
}
