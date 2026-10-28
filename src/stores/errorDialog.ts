import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface ErrorContextItem {
  label: string
  value: string
}

export interface ErrorDialogPayload {
  title: string
  summary: string
  detail: string
  context: ErrorContextItem[]
  at: number
}

export type ErrorContextInput =
  | Record<string, unknown>
  | Array<{ label: string; value: unknown } | [string, unknown]>

function firstLine(text: string): string {
  const line = text.split(/\r?\n/).find((item) => item.trim()) ?? text
  const trimmed = line.trim()
  return trimmed.length > 200 ? `${trimmed.slice(0, 200)}…` : trimmed
}

function toText(value: unknown): string {
  if (value == null || value === '') return ''
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}

/** 把任意异常整理成「一句话摘要 + 可复制的完整信息」 */
export function formatError(error: unknown): { summary: string; detail: string } {
  if (error == null) return { summary: '未知错误', detail: '未知错误' }
  if (typeof error === 'string') {
    const text = error.trim()
    if (!text) return { summary: '未知错误', detail: '未知错误' }
    return { summary: firstLine(text), detail: text }
  }
  if (error instanceof Error) {
    const message = error.message || error.name || '未知错误'
    return { summary: firstLine(message), detail: error.stack || `${error.name}: ${error.message}` }
  }
  const obj = error as Record<string, unknown>
  const message = typeof obj.message === 'string' ? obj.message : ''
  const stack = typeof obj.stack === 'string' ? obj.stack : ''
  let json = ''
  try {
    json = JSON.stringify(error, null, 2)
  } catch {
    json = String(error)
  }
  const summaryText = message || json || String(error)
  const detail = [message, stack, json].filter((item) => item && item !== message).join('\n\n')
  return { summary: firstLine(summaryText), detail: detail || summaryText }
}

function normalizeContext(input?: ErrorContextInput): ErrorContextItem[] {
  if (!input) return []
  const entries: Array<[string, unknown]> = Array.isArray(input)
    ? input.map((item) =>
        Array.isArray(item) ? [String(item[0]), item[1]] : [item.label, item.value],
      )
    : Object.entries(input)
  return entries
    .map(([label, value]) => ({ label, value: toText(value) }))
    .filter((item) => item.label && item.value)
}

export interface ShowErrorOptions {
  /** 出错环节，例如「发送请求」「前置脚本执行失败」 */
  title?: string
  error: unknown
  /** 覆盖默认摘要（默认取异常第一行） */
  summary?: string
  /** 覆盖默认详情 */
  detail?: string
  context?: ErrorContextInput
}

/**
 * 全局错误弹窗：执行过程中出错时把完整错误摊开给用户，可一键复制。
 * 同一个错误正在展示时不会重复弹；渲染异常连续刷屏时 500ms 内只弹一次。
 */
export const useErrorDialogStore = defineStore('errorDialog', () => {
  const open = ref(false)
  const payload = ref<ErrorDialogPayload | null>(null)
  let lastKey = ''
  let lastAt = 0

  function show(options: ShowErrorOptions) {
    const formatted = formatError(options.error)
    const title = options.title?.trim() || '执行出错'
    const summary = options.summary?.trim() || formatted.summary
    const detail = options.detail?.trim() || formatted.detail
    const context = normalizeContext(options.context)
    const key = `${title}::${summary}`
    const now = Date.now()
    // 同一个错误正在展示时不打断；渲染异常连续刷屏时 500ms 内只弹一次
    if (open.value && lastKey === key) return
    if (lastKey === key && now - lastAt < 500) return
    lastKey = key
    lastAt = now
    payload.value = { title, summary, detail, context, at: now }
    open.value = true
  }

  function close() {
    open.value = false
  }

  return { open, payload, show, close }
})
