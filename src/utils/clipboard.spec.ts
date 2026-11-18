import { afterEach, describe, expect, it, vi } from 'vitest'
import { copyToClipboard } from './clipboard'

function stubDocument(execResult = true) {
  const textarea = {
    value: '',
    style: { cssText: '' },
    setAttribute: vi.fn(),
    select: vi.fn(),
    setSelectionRange: vi.fn(),
    remove: vi.fn(),
  }
  const execCommand = vi.fn(() => execResult)
  vi.stubGlobal('document', {
    createElement: vi.fn(() => textarea),
    body: { appendChild: vi.fn() },
    getSelection: vi.fn(() => null),
    execCommand,
  })
  return { textarea, execCommand }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('copyToClipboard', () => {
  it('优先使用 Clipboard API', async () => {
    const writeText = vi.fn(() => Promise.resolve())
    vi.stubGlobal('navigator', { clipboard: { writeText } })

    await copyToClipboard('hello')

    expect(writeText).toHaveBeenCalledWith('hello')
  })

  it('Clipboard API 被拒绝时回退到 execCommand', async () => {
    vi.stubGlobal('navigator', {
      clipboard: { writeText: vi.fn(() => Promise.reject(new Error('NotAllowedError'))) },
    })
    const { textarea, execCommand } = stubDocument(true)

    await copyToClipboard('curl https://example.com')

    expect(execCommand).toHaveBeenCalledWith('copy')
    expect(textarea.value).toBe('curl https://example.com')
    expect(textarea.remove).toHaveBeenCalled()
  })

  it('两条路径都失败时抛出可读错误', async () => {
    vi.stubGlobal('navigator', {
      clipboard: { writeText: vi.fn(() => Promise.reject(new Error('NotAllowedError'))) },
    })
    stubDocument(false)

    await expect(copyToClipboard('hello')).rejects.toThrow('当前环境不允许写入剪贴板')
  })

  it('空内容直接拒绝，不触碰剪贴板', async () => {
    const writeText = vi.fn()
    vi.stubGlobal('navigator', { clipboard: { writeText } })

    await expect(copyToClipboard('')).rejects.toThrow('没有可复制的内容')
    expect(writeText).not.toHaveBeenCalled()
  })
})