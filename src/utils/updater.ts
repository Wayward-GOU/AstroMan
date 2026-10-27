import { h } from 'vue'
import { message, Modal } from 'ant-design-vue'
import * as api from '@/api'
import { isMac } from '@/utils/platform'
import type { UpdateInfo } from '@/types/models'

export const AUTO_UPDATE_KEY = 'fm-auto-update'

export function isAutoUpdateEnabled(): boolean {
  return localStorage.getItem(AUTO_UPDATE_KEY) !== 'false'
}

export function setAutoUpdateEnabled(enabled: boolean) {
  localStorage.setItem(AUTO_UPDATE_KEY, enabled ? 'true' : 'false')
}

let checking = false

async function downloadAndInstall(info: UpdateInfo) {
  const urls = [info.assetApiUrl, info.assetUrl].filter(
    (url): url is string => typeof url === 'string' && url.startsWith('https://'),
  )
  if (!urls.length || !info.assetName) {
    try {
      await api.openExternal(info.releaseUrl)
    } catch (e) {
      message.error(String(e))
    }
    return
  }
  const hide = message.loading('正在从 GitHub 下载安装包…', 0)
  try {
    const path = await api.updateDownload(urls, info.assetName)
    hide()
    await api.openExternal(path)
    message.success(
      isMac
        ? '安装包已下载并打开，将 FbinHttp 拖入「应用程序」即可完成升级'
        : '安装包已下载，按安装向导完成升级即可',
    )
  } catch (e) {
    hide()
    // 下载失败时直接打开发布页，避免用户卡在报错上
    Modal.confirm({
      title: '自动下载失败',
      content: h('div', { style: 'font-size:12px;line-height:1.8' }, [
        h('div', `当前网络无法直连 GitHub 下载域名：${String(e)}`),
        h('div', '可改用浏览器打开发布页手动下载安装包，或配置代理后重试。'),
      ]),
      okText: '打开发布页',
      cancelText: '关闭',
      onOk: () => api.openExternal(info.releaseUrl),
    })
  }
}

function promptUpdate(info: UpdateInfo) {
  const notes = (info.notes || '').trim()
  const preview = notes.length > 320 ? `${notes.slice(0, 320)}…` : notes
  Modal.confirm({
    title: `发现新版本 v${info.latestVersion}`,
    width: 520,
    content: h('div', { style: 'font-size:12px;line-height:1.8;color:var(--fm-text-2)' }, [
      h('div', `当前版本 v${info.currentVersion}，最新版本 v${info.latestVersion}。`),
      preview ? h('pre', { class: 'update-notes' }, preview) : null,
      h(
        'a',
        {
          style: 'font-size:12px',
          onClick: () => api.openExternal(info.releaseUrl).catch((e) => message.error(String(e))),
        },
        '在浏览器中查看发布说明',
      ),
    ]),
    okText: isMac ? '下载安装包' : '下载并安装',
    cancelText: '稍后',
    onOk: () => downloadAndInstall(info),
  })
}

/** 检查 GitHub 最新 Release；silent 模式下仅在有更新时提示 */
export async function checkForUpdates(options: { silent?: boolean } = {}): Promise<UpdateInfo | null> {
  if (checking) return null
  checking = true
  try {
    const info = await api.updateCheck()
    if (info.hasUpdate) {
      promptUpdate(info)
    } else if (!options.silent) {
      message.success(`已是最新版本 v${info.currentVersion}`)
    }
    return info
  } catch (e) {
    if (!options.silent) message.error(String(e))
    return null
  } finally {
    checking = false
  }
}
