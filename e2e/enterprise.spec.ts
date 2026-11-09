import { expect, test } from './fixtures/tauri'
import type { Page } from '@playwright/test'

/**
 * W4 / A-23：企业交付验收里的键盘流程与低端适配。
 *
 * 这里的断言只覆盖前端可验证的部分：
 * - 纯键盘可以聚焦地址栏、按 Enter 发送并看到响应；
 * - 键盘焦点有可见描边（`:focus-visible`）；
 * - 最小窗口 / 200% 缩放等效宽度下不出现整页横向滚动。
 */
async function tabUntil(page: Page, selector: string, max = 120): Promise<boolean> {
  for (let index = 0; index < max; index += 1) {
    await page.keyboard.press('Tab')
    const reached = await page.evaluate((target) => {
      const element = document.querySelector(target)
      return Boolean(element) && document.activeElement === element
    }, selector)
    if (reached) return true
  }
  return false
}

test.describe('键盘与低端适配', () => {
  test('纯键盘发送请求并查看响应', async ({ app, page }) => {
    await app.goto('/request/11?projectId=1')

    expect(await tabUntil(page, 'input.url-input')).toBe(true)

    const ring = await page.evaluate(() => {
      const active = document.activeElement as HTMLElement | null
      if (!active) return null
      const style = getComputedStyle(active)
      return { width: style.outlineWidth, style: style.outlineStyle }
    })
    expect(ring).not.toBeNull()
    expect(ring?.style).not.toBe('none')
    expect(ring?.width).not.toBe('0px')

    await page.keyboard.press('Enter')
    await expect(page.locator('.status-pill')).toContainText('200 OK')
    await expect(page.getByText('Ada').first()).toBeVisible()

    expect(await app.callCount('send_http_request')).toBe(1)
  })

  test('最小窗口下不出现整页横向滚动', async ({ app, page }) => {
    await page.setViewportSize({ width: 900, height: 560 })
    await app.goto('/request/11?projectId=1')
    await expect(page.locator('input.url-input')).toBeVisible()

    const metrics = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      innerWidth: window.innerWidth,
    }))
    expect(metrics.scrollWidth).toBeLessThanOrEqual(metrics.innerWidth + 1)
  })
})
