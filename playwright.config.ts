import { defineConfig } from '@playwright/test'

/**
 * G-12：桌面端 UI 的端到端冒烟 + 视觉回归。
 *
 * 桌面壳（Tauri / WebView2）无法直接被 Playwright 驱动，所以这里跑的是
 * Vite 起的同一套前端代码，并在页面加载前注入 `__TAURI_INTERNALS__` 假桥。
 * 假桥只服务 UI 行为与渲染，不替代 Rust 侧单测。
 *
 * 浏览器选择：
 * - 本机：默认用系统 Chrome（`PW_CHANNEL=chrome`），无需下载浏览器；
 * - CI：`CI=true` 时改用 `npx playwright install chromium` 安装的 Chromium。
 *
 * 视觉基线只在 Windows + Chrome 上生成（见 e2e/visual.spec.ts 的 skip），
 * 因为字体光栅化会随平台变化；其它平台只跑冒烟。
 */
const CI = !!process.env.CI
const PORT = 5273

export default defineConfig({
  testDir: './e2e',
  outputDir: 'test-results',
  fullyParallel: false,
  workers: 1,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: CI ? [['line'], ['html', { open: 'never' }]] : [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    viewport: { width: 1366, height: 768 },
    locale: 'zh-CN',
    timezoneId: 'Asia/Shanghai',
    colorScheme: 'light',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    channel: CI ? undefined : process.env.PW_CHANNEL || 'chrome',
  },
  webServer: {
    // 显式绑 IPv4：Linux runner 上 localhost 可能先解析到 ::1，
    // 而 baseURL 用的是 127.0.0.1，会出现「服务已起但探测不到」的假超时
    command: `npm run dev -- --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !CI,
    timeout: 120_000,
    stdout: 'pipe',
  },
  projects: [
    { name: 'smoke', testMatch: /smoke\.spec\.ts/ },
    { name: 'visual', testMatch: /visual\.spec\.ts/ },
    { name: 'enterprise', testMatch: /enterprise\.spec\.ts/ },
  ],
})
