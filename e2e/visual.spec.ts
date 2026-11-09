import { expect, test } from './fixtures/tauri'

/**
 * G-12 视觉回归：项目页、请求编辑器、报告页三个稳定页面。
 *
 * - 阈值 0.2%（maxDiffPixelRatio: 0.002），动效与光标闪烁关闭；
 * - 覆盖 1366×768、1920×1080 两种分辨率与 1366×768 下 200% 缩放；
 * - 基线只在 Windows + Chrome 上维护：Linux/macOS 的字体光栅化不同，
 *   在这些平台跳过，只跑冒烟（见 docker/CI 的 e2e job）。
 */
test.skip(process.platform !== 'win32', '视觉基线在 Windows + Chrome 上生成，其它平台只跑冒烟')

const pages = [
  { name: 'project', path: '/project/1', ready: '用户接口' },
  { name: 'request', path: '/request/11?projectId=1', ready: '获取用户列表' },
  { name: 'reports', path: '/reports', ready: '压测报告' },
]

const SHOT = { maxDiffPixelRatio: 0.002, animations: 'disabled' as const, caret: 'hide' as const }

for (const item of pages) {
  test(`视觉：${item.name} @1366x768`, async ({ app, page }) => {
    await app.goto(item.path)
    await expect(page.getByText(item.ready).first()).toBeVisible()
    await page.evaluate(() => document.fonts.ready)
    await expect(page).toHaveScreenshot(`${item.name}-1366x768.png`, SHOT)
  })

  test(`视觉：${item.name} @1920x1080`, async ({ app, page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 })
    await app.goto(item.path)
    await expect(page.getByText(item.ready).first()).toBeVisible()
    await page.evaluate(() => document.fonts.ready)
    await expect(page).toHaveScreenshot(`${item.name}-1920x1080.png`, SHOT)
  })

  test(`视觉：${item.name} @200% 缩放`, async ({ app, page }) => {
    await app.goto(item.path)
    await expect(page.getByText(item.ready).first()).toBeVisible()
    await page.evaluate(() => {
      document.documentElement.style.zoom = '2'
    })
    await page.evaluate(() => document.fonts.ready)
    await expect(page).toHaveScreenshot(`${item.name}-1366x768-zoom200.png`, SHOT)
  })
}
