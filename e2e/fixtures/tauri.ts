import { test as base, expect, type Page } from '@playwright/test'

/**
 * G-12：给前端注入一个最小的 Tauri 桥。
 *
 * 真实桌面端由 Rust 命令提供数据，Playwright 直接跑 Vite 页面时没有桥，
 * 所有 `invoke` 都会失败。这里在页面脚本执行前注入：
 * - `window.__TAURI_INTERNALS__`（invoke / transformCallback / metadata ...）
 * - `window.__TAURI_EVENT_PLUGIN_INTERNALS__`（unregisterListener）
 * - `window.__E2E__`（calls / reset / emit / state），供测试断言与推送事件
 *
 * 夹具只覆盖 UI 冒烟与视觉回归需要的命令；未列出的命令返回 null，
 * 并要求调用方在测试里显式断言自己关心的命令，避免静默通过。
 */
export interface E2ECall {
  cmd: string
  args: Record<string, unknown>
}

export interface AppMock {
  goto(path: string): Promise<void>
  calls(cmd?: string): Promise<E2ECall[]>
  callCount(cmd: string): Promise<number>
  emit(event: string, payload: unknown): Promise<void>
  state(): Promise<Record<string, unknown>>
}

type Fixture = {
  app: AppMock
}

export const test = base.extend<Fixture>({
  app: async ({ page }, use) => {
    await page.addInitScript(() => {
      try {
        // 启动 5 秒后的静默更新检查会调用 GitHub，与 UI 冒烟无关
        localStorage.setItem('fm-auto-update', 'false')
      } catch {
        /* 隐私模式下忽略 */
      }
    })
    await page.addInitScript(installBridge)

    const app: AppMock = {
      async goto(path) {
        await page.goto(`/#${path}`)
        await page.waitForFunction(() => !!(window as any).__E2E__)
      },
      calls(cmd) {
        return page.evaluate(
          (target) => {
            const list = (window as any).__E2E__.calls as E2ECall[]
            return target ? list.filter((item) => item.cmd === target) : list.slice()
          },
          cmd,
        )
      },
      callCount(cmd) {
        return page.evaluate((target) => {
          const list = (window as any).__E2E__.calls as E2ECall[]
          return list.filter((item) => item.cmd === target).length
        }, cmd)
      },
      emit(event, payload) {
        return page.evaluate(
          ([name, value]) => (window as any).__E2E__.emit(name, value),
          [event, payload] as const,
        )
      },
      state() {
        return page.evaluate(() => JSON.parse(JSON.stringify((window as any).__E2E__.state)))
      },
    }

    await use(app)
  },
})

export { expect }

/**
 * 这段函数会被 Playwright 序列化后注入页面，所有依赖都必须在函数内部定义。
 * 数据留在页面里（而不是 Node 侧），页面内点击引起的状态变更才能反映到后续断言。
 */
function installBridge() {
  const now = 1_725_000_000_000
  const project = {
    id: 1,
    name: 'E2E 示例项目',
    description: 'Playwright 夹具项目',
    color: '#009688',
    sortOrder: 0,
    createdAt: now,
    updatedAt: now,
  }

  const state: Record<string, any> = {
    projects: [project],
    collections: [
      {
        id: 1,
        projectId: 1,
        parentId: null,
        name: '用户接口',
        sortOrder: 0,
        children: [
          {
            id: 2,
            projectId: 1,
            parentId: 1,
            name: '账号',
            sortOrder: 0,
            children: [],
          },
        ],
      },
    ],
    requests: [
      {
        id: 11,
        collectionId: 1,
        name: '获取用户列表',
        method: 'GET',
        url: 'https://api.example.com/users',
        sortOrder: 0,
        updatedAt: now,
      },
      {
        id: 12,
        collectionId: 1,
        name: '创建用户',
        method: 'POST',
        url: 'https://api.example.com/users',
        sortOrder: 1,
        updatedAt: now,
      },
    ],
    environments: [
      {
        id: 1,
        projectId: 1,
        name: '开发环境',
        isActive: true,
        isProd: false,
        script: '',
        variables: [
          { key: 'base_url', value: 'https://api.example.com', isSecret: false, sortOrder: 0, hasValue: true },
          { key: 'token', value: '••••••', isSecret: true, sortOrder: 1, hasValue: true },
        ],
      },
      {
        id: 2,
        projectId: 1,
        name: '预发环境',
        isActive: false,
        isProd: false,
        script: '',
        variables: [{ key: 'base_url', value: 'https://staging.example.com', isSecret: false, sortOrder: 0, hasValue: true }],
      },
    ],
    requestDetails: {
      11: {
        id: 11,
        collectionId: 1,
        name: '获取用户列表',
        method: 'GET',
        protocol: 'http',
        url: 'https://api.example.com/users',
        headers: [{ key: 'Accept', value: 'application/json', enabled: true }],
        params: [{ key: 'page', value: '1', enabled: true }],
        bodyType: 'none',
        body: '',
        bodyMeta: '',
        preScript: '',
        postScript: '',
        assertions: '',
        auth: {
          kind: 'none',
          token: '',
          username: '',
          password: '',
          key: '',
          value: '',
          addTo: 'header',
        },
        requestEntity: null,
        responseEntity: null,
      },
    },
    capture: {
      status: {
        state: 'idle',
        port: 0,
        bindAddr: '127.0.0.1:8899',
        sessionId: null,
        startedAt: 0,
        flowCount: 0,
        dropped: 0,
        caInstalled: true,
        sysproxyOn: false,
        upstreamProxy: null,
        lastError: null,
      },
      ca: {
        exists: true,
        fingerprint: 'AA:BB:CC:DD',
        thumbprint: '11:22:33:44',
        createdAt: now,
        expiresAt: now + 365 * 24 * 3600 * 1000,
        installed: true,
      },
      intercept: {
        breakpointsEnabled: false,
        breakpointsActive: false,
        mapEnabled: false,
        mapActive: false,
        remoteEnabled: false,
        remoteActive: false,
        weak: {
          enabled: false,
          preset: 'none',
          latencyMs: 0,
          jitterMs: 0,
          bandwidthKbps: 0,
          lossPercent: 0,
          patterns: [],
        },
        breakpointRules: [],
        mapRules: [],
        remoteRules: [],
        pausedTotal: 0,
        pendingCount: 0,
        maxPending: 200,
        timeoutSeconds: 30,
        pending: [],
      },
    },
    mock: {
      running: false,
      projectId: 1,
      url: '',
      host: '127.0.0.1',
      port: 0,
      lan: false,
      token: null,
      routeCount: 0,
      startedAt: 0,
      requests: 0,
    },
    suiteDetail: {
      suite: {
        id: 21,
        projectId: 1,
        name: '冒烟套件',
        description: 'Playwright 夹具套件',
        envId: 1,
        createdAt: now,
        updatedAt: now,
      },
      cases: [
        {
          id: 211,
          suiteId: 21,
          name: '获取用户列表',
          kind: 'http',
          requestId: 11,
          sortOrder: 0,
          enabled: true,
          timeoutMs: 30_000,
          retries: 0,
          retryOnFailure: false,
          continueOnFailure: false,
          createdAt: now,
          updatedAt: now,
        },
      ],
      datasets: [],
    },
    suiteSummary: {
      id: 21,
      projectId: 1,
      name: '冒烟套件',
      description: 'Playwright 夹具套件',
      envId: 1,
      envName: '开发环境',
      caseCount: 1,
      createdAt: now,
      updatedAt: now,
    },
    run: {
      id: 900,
      suiteId: 21,
      envId: 1,
      envName: '开发环境',
      trigger: 'manual',
      status: 'passed',
      total: 1,
      passed: 1,
      failed: 0,
      skipped: 0,
      errored: 0,
      durationMs: 1234,
      createdAt: now,
      startedAt: now,
      finishedAt: now + 1234,
    },
    report: {
      id: 1,
      planName: 'E2E 压测计划',
      requestId: 11,
      totalRequests: 100,
      successCount: 99,
      errorCount: 1,
      avgRt: 42,
      minRt: 18,
      maxRt: 320,
      p50Rt: 38,
      p75Rt: 48,
      p90Rt: 61,
      p95Rt: 77,
      p99Rt: 120,
      p999Rt: 280,
      stddevRt: 12,
      ttfbAvg: 30,
      ttfbP95: 55,
      apdex: 0.97,
      bytesTotal: 51_200,
      maxQps: 80,
      avgQps: 52,
      targetRps: 60,
      errorRate: 1,
      concurrency: 8,
      status: 'finished',
      startTime: now,
      endTime: now + 60_000,
      createdAt: now,
    },
    exportPath: 'C:/Temp/astroman-e2e.md',
  }

  const calls: E2ECall[] = []
  const callbacks = new Map<number, (data: any) => void>()
  const listeners = new Map<string, number[]>()
  let nextCallback = 1

  const clone = (value: any) => (value === undefined ? value : JSON.parse(JSON.stringify(value)))

  function httpResult() {
    const body = '{"data":[{"id":1,"name":"Ada"}],"total":1}'
    return {
      status: 200,
      statusText: 'OK',
      durationMs: 42,
      sizeBytes: body.length,
      headers: { 'content-type': 'application/json; charset=utf-8' },
      body,
      truncated: false,
      error: null,
      bodyKind: 'json',
      bodyFile: null,
      bodyBytes: body.length,
      rawBytes: body.length,
      previewBase64: null,
      timings: {
        dnsMs: 1,
        connectMs: 2,
        ttfbMs: 20,
        downloadMs: 3,
        totalMs: 42,
        reusedConnection: false,
        proxy: false,
      },
      httpVersion: 'HTTP/1.1',
      redirects: [],
      retries: [],
      ignoredHeaders: [],
      cancelled: false,
    }
  }

  function preflight() {
    return {
      suiteId: 21,
      suiteName: '冒烟套件',
      envId: 1,
      envName: '开发环境',
      cookieMode: 'auto',
      strictScripts: false,
      totalCases: 1,
      enabledCases: 1,
      errors: 0,
      warnings: 0,
      runnable: true,
      items: [],
    }
  }

  function docsOutline() {
    return {
      groups: [
        {
          key: 'g1',
          name: '用户接口',
          path: ['用户接口'],
          requests: [
            {
              key: 'r11',
              name: '获取用户列表',
              method: 'GET',
              url: 'https://api.example.com/users',
              description: '',
              included: true,
              params: [],
              warnings: [],
            },
          ],
          children: [],
        },
      ],
    }
  }

  function runDetail() {
    return { run: state.run, cases: [], assertions: [] }
  }

  const handlers: Record<string, (args: any) => any> = {
    /* ---------- 启动与全局 ---------- */
    notes_counts: () => ({ open: 0, dueToday: 0, overdue: 0, done: 0 }),
    notes_settings_get: () => ({
      notifyEnabled: true,
      quietHoursEnabled: false,
      quietStart: '22:00',
      quietEnd: '08:00',
      pauseUntil: 0,
      globalShortcut: true,
      visionProviderId: null,
    }),
    update_check: () => ({
      hasUpdate: false,
      currentVersion: '1.1.0',
      latestVersion: '1.1.0',
      releaseUrl: '',
      assetUrl: '',
      assetApiUrl: '',
      assetName: '',
      notes: '',
    }),
    app_info: () => ({ version: '1.1.0', name: 'AstroMan' }),
    settings_get: () => ({}),

    /* ---------- 项目 / 集合 / 请求 ---------- */
    project_list: () => state.projects,
    collection_tree: () => state.collections,
    request_list: (args) => state.requests.filter((item: any) => item.collectionId === args.collectionId),
    request_search: () => state.requests,
    request_options: () =>
      state.requests.map((item: any) => ({
        ...item,
        collectionName: '用户接口',
      })),
    request_get: (args) => clone(state.requestDetails[args.id]),
    request_entities_get: () => ({ requestEntity: null, responseEntity: null, warnings: [] }),
    run_list: () => [],
    run_get: () => null,
    send_http_request: () => httpResult(),

    /* ---------- 环境 ---------- */
    env_list: (args) => state.environments.filter((item: any) => item.projectId === args.projectId),
    env_activate: (args) => {
      for (const env of state.environments) env.isActive = env.id === args.id
      return null
    },

    /* ---------- 抓包 ---------- */
    capture_status: () => clone(state.capture.status),
    capture_flows: () => [],
    capture_flow_detail: () => null,
    capture_ca_info: () => clone(state.capture.ca),
    capture_lan_ips: () => [],
    capture_intercept_state: () => clone(state.capture.intercept),
    capture_start: (args) => {
      state.capture.status = {
        ...state.capture.status,
        state: 'running',
        port: args?.port || 8899,
        bindAddr: args?.bindAddr || '127.0.0.1:8899',
        sessionId: 1,
        startedAt: Date.now(),
      }
      return clone(state.capture.status)
    },
    capture_stop: () => {
      state.capture.status = {
        ...state.capture.status,
        state: 'idle',
        sessionId: null,
        startedAt: 0,
      }
      return clone(state.capture.status)
    },

    /* ---------- Mock ---------- */
    mock_routes_list: () => [],
    mock_status: () => clone(state.mock),
    mock_logs: () => [],
    mock_start: () => {
      state.mock = {
        ...state.mock,
        running: true,
        url: 'http://127.0.0.1:9101',
        host: '127.0.0.1',
        port: 9101,
        routeCount: 2,
        startedAt: Date.now(),
      }
      return clone(state.mock)
    },
    mock_stop: () => {
      state.mock = {
        ...state.mock,
        running: false,
        url: '',
        port: 0,
        routeCount: 0,
        startedAt: 0,
        requests: 0,
      }
      return clone(state.mock)
    },

    /* ---------- 测试套件 ---------- */
    test_suite_list: () => [state.suiteSummary],
    test_suite_get: () => clone(state.suiteDetail),
    test_preflight: () => preflight(),
    test_run_start: () => state.run.id,
    test_run_get: () => clone(runDetail()),
    test_run_list: () => [state.run],

    /* ---------- 报告 ---------- */
    report_list: () => [state.report],
    report_get: () => null,

    /* ---------- 接口文档 ---------- */
    docs_prepare: () => ({
      draftId: 'draft-e2e',
      revision: 'rev-1',
      meta: { title: 'E2E 接口文档', subtitle: 'Playwright 夹具' },
      outline: docsOutline(),
      warnings: [],
    }),
    docs_render: (args) => {
      const markdown = '# E2E 接口文档\n\n## 用户接口\n\n### 获取用户列表\n'
      const html =
        '<!doctype html><html><body><h1>E2E 接口文档</h1><h2>用户接口</h2><h3>获取用户列表</h3></body></html>'
      const content = args?.payload?.format === 'markdown' ? markdown : html
      return { content, bytes: content.length, warnings: [] }
    },
    docs_export: (args) => ({
      path: args?.payload?.path || state.exportPath,
      format: args?.payload?.format || 'markdown',
      scope: 'project',
      requestCount: 1,
      collectionCount: 1,
      bytes: 512,
      warnings: [],
      pdfEngine: null,
      fallbackHtmlPath: null,
    }),

    /* ---------- Tauri 插件 ---------- */
    'plugin:dialog|save': () => state.exportPath,
    'plugin:dialog|open': () => null,
    'plugin:event|listen': (args) => {
      const list = listeners.get(args.event) ?? []
      list.push(args.handler)
      listeners.set(args.event, list)
      // 与真实事件插件一致：返回的 eventId 就是回调 id，unlisten 时原样带回
      return args.handler
    },
    'plugin:event|unlisten': (args) => {
      const list = listeners.get(args.event) ?? []
      listeners.set(
        args.event,
        list.filter((handler) => handler !== args.eventId),
      )
      return null
    },
    'plugin:webview|set_webview_zoom': () => null,
  }

  const internals = {
    callbacks,
    invoke: async (cmd: string, args: any) => {
      calls.push({ cmd, args: clone(args ?? {}) })
      const handler = handlers[cmd]
      if (handler) return handler(args ?? {})
      return null
    },
    transformCallback: (callback: (data: any) => void) => {
      const id = nextCallback++
      callbacks.set(id, callback)
      return id
    },
    unregisterCallback: (id: number) => {
      callbacks.delete(id)
    },
    runCallback: (id: number, data: any) => {
      const callback = callbacks.get(id)
      if (callback) callback(data)
    },
    convertFileSrc: (path: string) => path,
    metadata: {
      currentWebview: { label: 'main' },
      currentWindow: { label: 'main' },
    },
    version: '2.0.0',
  }

  const api = {
    calls,
    state,
    reset() {
      calls.length = 0
    },
    emit(event: string, payload: any) {
      const body = { event, id: 1, payload }
      for (const handler of listeners.get(event) ?? []) {
        internals.runCallback(handler, body)
      }
    },
  }

  ;(window as any).__TAURI_INTERNALS__ = internals
  ;(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event: string, eventId: number) => {
      const list = listeners.get(event) ?? []
      listeners.set(
        event,
        list.filter((handler) => handler !== eventId),
      )
    },
  }
  ;(window as any).__E2E__ = api
}
