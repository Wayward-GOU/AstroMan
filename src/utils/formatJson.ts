import { checkJsonBody, parseLooseJson, type JsonBodyIssue, type LooseRepair } from './looseJson'

/** JSON 格式化放到 Worker 里执行，避免大文本解析 + 重新序列化阻塞输入 */
export interface JsonFormatResult {
  text: string
  /** 命中的宽松处理项；非空表示原文不是严格 JSON，已按宽松规则规范化 */
  repairs: LooseRepair[]
}

interface PendingTask {
  resolve: (value: JsonFormatResult) => void
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
      const data = event.data as {
        id?: number
        ok?: boolean
        result?: string
        error?: string
        repairs?: LooseRepair[]
      }
      if (typeof data?.id !== 'number') return
      const task = pending.get(data.id)
      if (!task) return
      pending.delete(data.id)
      if (data.ok) {
        task.resolve({ text: data.result ?? '', repairs: data.repairs ?? [] })
      } else {
        task.reject(new Error(data.error || 'JSON 解析失败'))
      }
    }
    worker.onerror = () => failAll(new Error('格式化线程异常，请重试'))
    return worker
  } catch {
    workerBroken = true
    return null
  }
}

/** 主线程兜底：严格解析失败时按宽松规则再试一次（单引号 / 尾逗号 / 无引号 key / 注释） */
function formatSync(text: string): Promise<JsonFormatResult> {
  return new Promise((resolve, reject) => {
    try {
      resolve({ text: JSON.stringify(JSON.parse(text), null, 2), repairs: [] })
      return
    } catch (strictError) {
      try {
        const loose = parseLooseJson(text)
        resolve({ text: JSON.stringify(loose.value, null, 2), repairs: loose.repairs })
      } catch {
        reject(strictError instanceof Error ? strictError : new Error(String(strictError)))
      }
    }
  })
}

/** 异步格式化 JSON，同时返回命中过的宽松处理项；Worker 不可用时退回主线程同步实现 */
export function formatJsonLooseAsync(text: string): Promise<JsonFormatResult> {
  const target = getWorker()
  if (!target) return formatSync(text)

  const id = ++seq
  return new Promise<JsonFormatResult>((resolve, reject) => {
    pending.set(id, { resolve, reject })
    try {
      target.postMessage({ id, text })
    } catch (error) {
      pending.delete(id)
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}

/** 异步格式化 JSON；Worker 不可用时退回主线程同步实现 */
export async function formatJsonAsync(text: string): Promise<string> {
  const result = await formatJsonLooseAsync(text)
  return result.text
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
 *
 * 单引号字符串也按「字符串内」处理：`{'id':'{{id}}'}` 里的占位符不能补引号，
 * 否则会拼出 `'"__ASTRO_VAR_VALUE_0__"'` 这种嵌套引号。
 */
function maskJsonTemplateVars(text: string): MaskedTemplate {
  const valueVars: string[] = []
  const stringVars: string[] = []
  let out = ''
  let quote: '"' | "'" | null = null
  let escaped = false
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i]
    if (quote) {
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
      if (ch === quote) {
        quote = null
        out += ch
        continue
      }
    } else if (ch === '"' || ch === "'") {
      quote = ch
      out += ch
      continue
    }
    if (ch === '{' && text[i + 1] === '{') {
      const end = text.indexOf('}}', i + 2)
      if (end > i) {
        const variable = text.slice(i, end + 2)
        if (quote) {
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
 * 发送前体检：先掩掉 `{{变量}}` 再判断。
 *
 * 模板里 `"ts": {{timestamp}}` 这种裸值占位本来就不是合法 JSON，直接校验会误报，
 * 所以复用格式化用的同一套掩码逻辑，返回 null 表示可以放心发送。
 */
export function checkJsonTemplate(text: string): JsonBodyIssue | null {
  const masked = maskJsonTemplateVars(text)
  if (!masked.valueVars.length && !masked.stringVars.length) return checkJsonBody(text)
  return checkJsonBody(masked.text)
}

/**
 * 格式化可能含 `{{变量}}` 的 JSON 模板，并返回命中过的宽松处理项。
 *
 * 与 `formatJsonLooseAsync` 的区别只在占位符：裸值占位（如 `"ts": {{timestamp}}`）
 * 也能正常格式化，且格式化后引号保持原样，可直接用于发送。
 */
export async function formatJsonTemplateLooseAsync(text: string): Promise<JsonFormatResult> {
  const masked = maskJsonTemplateVars(text)
  if (!masked.valueVars.length && !masked.stringVars.length) {
    return formatJsonLooseAsync(text)
  }
  const formatted = await formatJsonLooseAsync(masked.text)
  return { text: restoreJsonTemplateVars(formatted.text, masked), repairs: formatted.repairs }
}

/**
 * 格式化可能含 `{{变量}}` 的 JSON 模板。
 *
 * 与 `formatJsonAsync` 的区别只在占位符：裸值占位（如 `"ts": {{timestamp}}`）
 * 也能正常格式化，且格式化后引号保持原样，可直接用于发送。
 */
export async function formatJsonTemplateAsync(text: string): Promise<string> {
  const result = await formatJsonTemplateLooseAsync(text)
  return result.text
}
