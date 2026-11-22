import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * 池预览门禁单测（v1.1.2 第四批）
 *
 * 只验证「什么情况下放行 / 拦截」，弹窗外观交给 E2E 与人工验收；
 * 因此这里把 ant-design-vue 的 Modal 与 api.poolPreview 全部替换成桩。
 */
const mocks = vi.hoisted(() => ({
  poolPreview: vi.fn(),
  error: vi.fn(),
  confirm: vi.fn(),
}))

vi.mock('@/api', () => ({
  poolPreview: mocks.poolPreview,
}))

vi.mock('ant-design-vue', () => ({
  Modal: {
    error: mocks.error,
    confirm: mocks.confirm,
  },
}))

import { ensurePoolPreview } from './poolGate'

function bindings() {
  return [{ pool: 'token', target: 'Authorization', mode: 'per_vu' as const }]
}

function preview(overrides: Record<string, unknown> = {}) {
  return {
    vuCount: 10,
    bindings: [{ pool: 'token', target: 'Authorization', itemCount: 100, vusCovered: 10 }],
    problems: [],
    ...overrides,
  }
}

/** 触发确认框的 onOk / onCancel 并返回 Promise 结果 */
async function answerConfirm(ok: boolean) {
  const options = mocks.confirm.mock.calls.at(-1)?.[0] as {
    onOk?: () => void
    onCancel?: () => void
  }
  ok ? options.onOk?.() : options.onCancel?.()
}

describe('ensurePoolPreview', () => {
  beforeEach(() => {
    mocks.poolPreview.mockReset()
    mocks.error.mockReset()
    mocks.confirm.mockReset()
  })

  it('没有绑定池时直接放行，不请求预览', async () => {
    await expect(ensurePoolPreview(undefined, 10)).resolves.toBe(true)
    await expect(ensurePoolPreview([], 10)).resolves.toBe(true)
    await expect(
      ensurePoolPreview([{ pool: ' ', target: '', mode: 'per_vu' }], 10),
    ).resolves.toBe(true)
    expect(mocks.poolPreview).not.toHaveBeenCalled()
    expect(mocks.error).not.toHaveBeenCalled()
  })

  it('绑定不完整时阻止启动且不请求预览', async () => {
    await expect(
      ensurePoolPreview([{ pool: 'token', target: '', mode: 'per_vu' }], 10),
    ).resolves.toBe(false)
    expect(mocks.poolPreview).not.toHaveBeenCalled()
    expect(mocks.error).toHaveBeenCalledWith(
      expect.objectContaining({ title: '池绑定不完整，已阻止启动' }),
    )
  })

  it('预览请求失败时阻止启动', async () => {
    mocks.poolPreview.mockRejectedValue(new Error('数据库忙'))
    await expect(ensurePoolPreview(bindings(), 10)).resolves.toBe(false)
    expect(mocks.error).toHaveBeenCalledWith(
      expect.objectContaining({ title: '池预览失败，已阻止启动' }),
    )
  })

  it('池没有可用条目时阻止启动并列出池名', async () => {
    mocks.poolPreview.mockResolvedValue(
      preview({
        bindings: [
          { pool: 'token', target: 'Authorization', itemCount: 0, vusCovered: 0 },
          { pool: 'token', target: 'X-Token', itemCount: 0, vusCovered: 0 },
        ],
      }),
    )
    await expect(ensurePoolPreview(bindings(), 10)).resolves.toBe(false)
    expect(mocks.error).toHaveBeenCalledWith(
      expect.objectContaining({ content: '以下池没有可用条目：token' }),
    )
  })

  it('预览没有返回任何绑定时阻止启动', async () => {
    mocks.poolPreview.mockResolvedValue(preview({ bindings: [] }))
    await expect(ensurePoolPreview(bindings(), 10)).resolves.toBe(false)
    expect(mocks.error).toHaveBeenCalledWith(
      expect.objectContaining({ content: '没有可用的池绑定。' }),
    )
  })

  it('VU 数取整且不为负，用户确认后才放行', async () => {
    mocks.poolPreview.mockResolvedValue(preview())
    const pending = ensurePoolPreview(bindings(), 12.9)
    await vi.waitFor(() => expect(mocks.confirm).toHaveBeenCalled())
    expect(mocks.poolPreview).toHaveBeenCalledWith(bindings(), 12, false)
    await answerConfirm(true)
    await expect(pending).resolves.toBe(true)

    mocks.poolPreview.mockResolvedValue(preview())
    const negative = ensurePoolPreview(bindings(), -5)
    await vi.waitFor(() => expect(mocks.poolPreview).toHaveBeenCalledTimes(2))
    expect(mocks.poolPreview).toHaveBeenLastCalledWith(bindings(), 0, false)
    await answerConfirm(false)
    await expect(negative).resolves.toBe(false)
  })
})
