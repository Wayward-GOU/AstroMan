export type AppPlatform = 'macos' | 'windows' | 'other'

function detectPlatform(): AppPlatform {
  const ua = navigator.userAgent
  if (/Macintosh|Mac OS X/i.test(ua)) return 'macos'
  if (/Windows/i.test(ua)) return 'windows'
  return 'other'
}

export const appPlatform: AppPlatform = detectPlatform()
export const isMac = appPlatform === 'macos'

/** 把平台信息挂到 html 上，供全局样式做平台差异化 */
export function applyPlatformAttributes() {
  document.documentElement.dataset.platform = appPlatform
  document.documentElement.classList.toggle('platform-macos', isMac)
  document.documentElement.classList.toggle('platform-windows', appPlatform === 'windows')
}

export const platformLabel =
  appPlatform === 'macos' ? 'macOS' : appPlatform === 'windows' ? 'Windows' : '其他系统'

/** 平台化快捷键文案，例如 ⌘ + / Ctrl + */
export function shortcutText(key: string): string {
  return isMac ? `⌘ ${key}` : `Ctrl + ${key}`
}

export const openDirLabel = isMac ? '在访达中显示' : '打开目录'
