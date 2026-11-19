import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriWrite = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: tauriWrite }))

const { copyToClipboard } = await import('./clipboard')

describe('copyToClipboard in Tauri', () => {
  beforeEach(() => {
    tauriWrite.mockReset()
  })

  it('writes through the Tauri clipboard plugin first', async () => {
    tauriWrite.mockResolvedValue(undefined)
    const webWrite = vi.fn()
    vi.stubGlobal('navigator', { clipboard: { writeText: webWrite } })

    await copyToClipboard('curl https://example.com')

    expect(tauriWrite).toHaveBeenCalledWith('curl https://example.com')
    expect(webWrite).not.toHaveBeenCalled()
  })

  it('falls back to the Web API when the plugin fails', async () => {
    tauriWrite.mockRejectedValue(new Error('plugin unavailable'))
    const webWrite = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { clipboard: { writeText: webWrite } })

    await copyToClipboard('hello')

    expect(webWrite).toHaveBeenCalledWith('hello')
  })

  it('rejects empty text without touching the clipboard', async () => {
    await expect(copyToClipboard('')).rejects.toThrow('没有可复制的内容')
    expect(tauriWrite).not.toHaveBeenCalled()
  })
})