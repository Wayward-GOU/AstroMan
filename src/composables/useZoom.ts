import { computed, ref } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { clampZoom, stepZoom, toPercent } from '@/utils/zoom'

/**
 * 界面缩放状态。
 *
 * 放在模块级而不是组件内：MainLayout（侧边栏）与设置页都要读写同一个值，
 * 各自持有副本会立刻不同步——设置页调完滑块，侧边栏仍显示旧百分比。
 */

const STORAGE_KEY = 'fm-zoom'
/**
 * 一次性迁移标记。
 *
 * v1.1.5 之前 Ctrl/⌘+滚轮 也参与缩放，而 macOS 触控板双指捏合产生的正是
 * 带 ctrlKey 的 wheel 事件——一次随手捏合就会连打十几档到底，并把 50% 存下来，
 * 于是每次打开界面都只有一半。滚轮手势已移除，这里再把历史值清一次。
 */
const MIGRATE_KEY = 'fm-zoom-reset-v2'

function readStored(): number {
  try {
    if (localStorage.getItem(MIGRATE_KEY) !== '1') {
      localStorage.setItem(MIGRATE_KEY, '1')
      localStorage.setItem(STORAGE_KEY, '1')
      return 1
    }
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw === null ? 1 : clampZoom(Number(raw))
  } catch {
    return 1
  }
}

const zoom = ref(readStored())
const noticeVisible = ref(false)
let noticeTimer: number | undefined

const zoomPercent = computed(() => toPercent(zoom.value))

function flashNotice() {
  noticeVisible.value = true
  if (noticeTimer) window.clearTimeout(noticeTimer)
  noticeTimer = window.setTimeout(() => {
    noticeVisible.value = false
  }, 900)
}

async function apply(value: number, options: { persist?: boolean; flash?: boolean } = {}) {
  const { persist = true, flash = true } = options
  const next = clampZoom(value)
  zoom.value = next
  try {
    await getCurrentWebview().setZoom(next)
  } catch {
    // 浏览器预览环境没有 WebView 缩放，忽略；状态仍然更新，界面上的百分比是准的
  }
  if (persist) {
    try {
      localStorage.setItem(STORAGE_KEY, String(next))
    } catch {
      // 隐身模式等场景写不进去，不影响本次会话
    }
  }
  if (flash) flashNotice()
}

/** 启动时把上次的缩放应用到 WebView，不重复落库也不弹提示 */
function applyStoredZoom() {
  return apply(zoom.value, { persist: false, flash: false })
}

function zoomIn() {
  return apply(stepZoom(zoom.value, 1))
}

function zoomOut() {
  return apply(stepZoom(zoom.value, -1))
}

function resetZoom() {
  return apply(1)
}

function setZoomPercent(percent: number) {
  return apply(percent / 100, { flash: false })
}

export function useZoom() {
  return {
    zoom,
    zoomPercent,
    noticeVisible,
    zoomIn,
    zoomOut,
    resetZoom,
    setZoomPercent,
    applyStoredZoom,
  }
}
