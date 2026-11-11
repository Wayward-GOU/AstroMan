import { expect, test } from './fixtures/tauri'

/**
 * G-12 冒烟六条：新建请求并发送、环境切换、抓包启停 + Map Remote、
 * 测试套件运行、文档预览 / 导出、Mock 启停。
 *
 * 所有数据来自 e2e/fixtures/tauri.ts 注入的假桥，不访问网络、不依赖本机数据库。
 */
test.describe('桌面端冒烟', () => {
  test('打开请求并发送，渲染响应', async ({ app, page }) => {
    await app.goto('/request/11?projectId=1')

    const url = page.locator('input.url-input')
    await expect(url).toHaveValue('https://api.example.com/users')

    await page.locator('.send-btn').click()

    await expect(page.locator('.status-pill')).toContainText('200 OK')
    await expect(page.getByText('Ada').first()).toBeVisible()

    const calls = await app.calls('send_http_request')
    expect(calls).toHaveLength(1)
    const payload = calls[0].args.payload as Record<string, unknown>
    expect(payload.method).toBe('GET')
    expect(payload.url).toBe('https://api.example.com/users')
    expect(payload.envId).toBe(1)
  })

  test('切换当前环境', async ({ app, page }) => {
    await app.goto('/environment/1')

    const target = page.locator('.env-item', { hasText: '预发环境' })
    await expect(target).toBeVisible()
    await target.getByText('设为当前').click()

    await expect(page.getByText('已切换当前环境')).toBeVisible()

    const calls = await app.calls('env_activate')
    expect(calls).toHaveLength(1)
    expect(calls[0].args).toMatchObject({ projectId: 1, id: 2 })
    await expect(target.locator('.active-icon')).toBeVisible()
  })

  test('抓包启停并查看 Map Remote 规则', async ({ app, page }) => {
    await app.goto('/capture')

    await page.locator('.record-btn', { hasText: '录制' }).click()
    await expect(page.locator('.record-btn', { hasText: '停止' })).toBeVisible()
    expect(await app.callCount('capture_start')).toBe(1)

    await page.getByRole('button', { name: /拦截/ }).click()
    await expect(page.getByText('挂起 0 条')).toBeVisible()
    await page.getByRole('tab', { name: /远程映射/ }).click()
    await expect(page.getByText('远程映射（Map Remote）把请求转发到另一个地址')).toBeVisible()

    await page.keyboard.press('Escape')
    await page.locator('.record-btn', { hasText: '停止' }).click()
    await expect(page.locator('.record-btn', { hasText: '录制' })).toBeVisible()
    expect(await app.callCount('capture_stop')).toBe(1)
  })

  test('运行测试套件并打开运行详情', async ({ app, page }) => {
    await app.goto('/automation')

    await page.locator('.suite-item', { hasText: '冒烟套件' }).click()
    await expect(page.locator('.suite-title h2')).toHaveText('冒烟套件')

    await page.locator('.suite-actions').getByRole('button', { name: '运行' }).click()

    await expect(page.getByText('运行 #900')).toBeVisible()
    await expect(page.locator('.run-head .ant-tag')).toHaveText('通过')
    expect(await app.callCount('test_preflight')).toBe(1)
    expect(await app.callCount('test_run_start')).toBe(1)
  })

  test('预览并导出接口文档', async ({ app, page }) => {
    await app.goto('/project/1')
    await expect(page.getByText('E2E 示例项目')).toBeVisible()

    await page.getByRole('button', { name: /导出/ }).first().click()
    await page.getByText('接口文档（预览后导出）').click()

    await expect(page.getByText('导出接口文档')).toBeVisible()
    await expect(page.locator('.outline-tree')).toContainText('用户接口')
    await expect(page.locator('.preview-frame')).toHaveAttribute('srcdoc', /E2E 接口文档/)
    await page.locator('.preview-tabs .ant-radio-button-wrapper', { hasText: 'Markdown' }).click()
    await expect(page.locator('.markdown-view h1')).toHaveText('E2E 接口文档')

    await page.locator('.docs-head').getByRole('button', { name: /导出/ }).click()
    await page.getByText('Markdown（.md）').click()

    await expect(page.getByText('已导出 1 个接口')).toBeVisible()
    const calls = await app.calls('docs_export')
    expect(calls).toHaveLength(1)
    const payload = calls[0].args.payload as Record<string, unknown>
    expect(payload.format).toBe('markdown')
    expect(payload.path).toBe('C:/Temp/astroman-e2e.md')
  })

  test('启动并停止 Mock Server', async ({ app, page }) => {
    await app.goto('/project/1')
    await expect(page.getByText('E2E 示例项目')).toBeVisible()

    await page.getByRole('button', { name: 'Mock' }).click()
    await expect(page.getByText('Mock Server · E2E 示例项目')).toBeVisible()

    await page.getByRole('button', { name: '启动' }).click()
    await expect(page.locator('.run-box .url')).toHaveText('http://127.0.0.1:9101')
    expect(await app.callCount('mock_start')).toBe(1)

    await page.locator('.run-box').getByRole('button', { name: '停止' }).click()
    await expect(page.locator('.run-box')).toContainText('未启动')
    expect(await app.callCount('mock_stop')).toBe(1)
  })
})
