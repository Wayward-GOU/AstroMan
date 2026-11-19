import { isTauri } from '@tauri-apps/api/core'
import { writeText as writeTauriClipboard } from '@tauri-apps/plugin-clipboard-manager'

/**
 * 写入剪贴板。
 *
 * Tauri（WKWebView）会拦截 Clipboard API 和 execCommand，必须走官方插件；
 * 浏览器环境仍优先 Clipboard API，失败再回退到临时 textarea + execCommand。
 * 所有路径都失败才抛错，由调用方提示用户手动复制。
 */
export async function copyToClipboard(text: string): Promise<void> {
  if (!text) throw new Error('没有可复制的内容')

  if (isTauri()) {
    try {
      await writeTauriClipboard(text)
      return
    } catch {
      // 插件不可用时继续尝试 Web API
    }
  }

  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return
    } catch {
      // 继续走 execCommand 兜底
    }
  }

  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.setAttribute('aria-hidden', 'true')
  textarea.style.cssText = 'position:fixed;top:0;left:-9999px;width:1px;height:1px;opacity:0;'
  document.body.appendChild(textarea)

  const selection = document.getSelection()
  const previous = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null
  textarea.select()
  textarea.setSelectionRange(0, text.length)

  let ok = false
  try {
    ok = document.execCommand('copy')
  } finally {
    textarea.remove()
    if (previous && selection) {
      selection.removeAllRanges()
      selection.addRange(previous)
    }
  }

  if (!ok) throw new Error('当前环境不允许写入剪贴板，请手动选中复制')
}