import { expect, test } from './fixtures/tauri'

/**
 * v1.1.2 第四批 E2E：报告峰值与旧报告降级。
 *
 * 报告详情只依赖 report_get / report_requests 两个命令，
 * 夹具里准备了一份带集群峰值的新报告和一份没有配置快照的旧报告。
 */
test.describe('压测报告', () => {
  test('集群报告展示峰值专题与节点贡献', async ({ app, page }) => {
    await app.goto('/reports/1')

    await expect(page.locator('.cluster-card .card-title')).toHaveText('分布式压测')
    await expect(page.getByText('集群总 QPS')).toBeVisible()
    await expect(page.getByText('2,400').first()).toBeVisible()
    // 峰值出现时刻 / 平台期 / 贡献最大的节点
    await expect(page.getByText(/第 30 秒 · 持续 8 秒/).first()).toBeVisible()
    await expect(page.locator('.cluster-card')).toContainText('主节点')
    await expect(page.locator('.cluster-card')).toContainText('压测从机-A')

    expect(await app.callCount('report_get')).toBe(1)
  })

  test('旧报告没有配置快照时降级展示', async ({ app, page }) => {
    await app.goto('/reports/2')

    await expect(page.getByText('历史报告未保存配置快照')).toBeVisible()
    await expect(page.locator('.cluster-card')).toHaveCount(0)
    expect(await app.callCount('report_get')).toBe(1)
  })
})
