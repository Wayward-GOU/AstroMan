import { expect, test } from './fixtures/tauri'

/**
 * v1.1.2 第四批 E2E：集群审批与预算（分配模式）切换。
 *
 * 数据来自 e2e/fixtures/tauri.ts 的假桥：主节点任务处于 pairing，
 * 已有 1 个从节点完成配对、1 个节点待接受，用于验证审批与分配预览。
 */
test.describe('分布式压测', () => {
  test('集群审批：待接受列表可接受节点', async ({ app, page }) => {
    await app.goto('/cluster')

    await expect(page.getByText(/待接受 1 个/)).toBeVisible()
    const row = page.locator('.join-row', { hasText: '压测从机-B' })
    await expect(row).toBeVisible()

    // ant-design 会在两个汉字的按钮文案间插入空格，用正则可兼容
    await row.getByRole('button', { name: /接\s*受/ }).click()

    await expect(page.getByText('已接受节点，正在下发配置')).toBeVisible()
    await expect(page.getByText(/待接受 0 个/)).toBeVisible()

    const calls = await app.calls('cluster_master_approve')
    expect(calls).toHaveLength(1)
    expect(calls[0].args).toMatchObject({ jobId: 7, nodeId: 'slave-2' })
    await expect(page.locator('.node-name', { hasText: '压测从机-B' })).toBeVisible()
  })

  test('预算切换：能力分配改为平均分配后并发预览同步', async ({ page, app }) => {
    await app.goto('/cluster')

    // auto：主节点预算 3000、从节点 1500，总并发 20 → 13 / 7
    await expect(page.getByText(/准备时按「能力分配（按本机预算）」拆分总并发/)).toBeVisible()
    await expect(page.locator('.planned-value')).toHaveText(['≈ 13', '≈ 7'])

    await page.locator('.node-panel .panel-actions .ant-select').first().click()
    await page.locator('.ant-select-dropdown:visible').getByText('平均分配', { exact: true }).click()

    await expect(page.getByText(/准备时按「平均分配（余数依次补齐）」拆分总并发/)).toBeVisible()
    await expect(page.locator('.planned-value')).toHaveText(['≈ 10', '≈ 10'])
  })

  test('节点视图：卡片展示机器配置，可切回表格', async ({ page, app }) => {
    await app.goto('/cluster')

    // 默认卡片视图：每台机器一张卡，配置、资源与分配并发都在卡内
    const cards = page.locator('.node-card')
    await expect(cards).toHaveCount(2)
    await expect(cards.first().locator('.nc-spec .v')).toHaveText(['8 核', '16 GB', '3,000'])
    await expect(cards.first().locator('.nc-meter')).toHaveCount(2)
    await expect(cards.first().locator('.planned-value')).toHaveText('≈ 13')

    // 指标格下方一行灰度小字：本次配对以来的全部峰值（QPS / 并发 / 资源）
    await expect(cards.first().locator('.nc-peak')).toHaveText(
      '峰值 CPU 55% · 内存 69% · 网卡 2.6 MB/s'
    )
    await expect(cards.nth(1).locator('.nc-peak')).toHaveText(
      '峰值 QPS 1,800 · 并发 200 · CPU 86% · 内存 52% · 网卡 6.5 MB/s'
    )

    // 切到表格：13 列密集对照，卡片不再渲染
    await page.getByText('表格', { exact: true }).click()
    await expect(page.locator('.node-card')).toHaveCount(0)
    await expect(page.locator('.node-panel .ant-table')).toBeVisible()

    await page.getByText('卡片', { exact: true }).click()
    await expect(page.locator('.node-card')).toHaveCount(2)
  })
})
