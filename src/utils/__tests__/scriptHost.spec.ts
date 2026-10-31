import { afterEach, describe, expect, it } from 'vitest'
import {
  SCRIPT_TIMEOUT_RANGE,
  runScriptJob,
  scriptHost,
  scriptTimeoutMs,
  setScriptTimeoutMs,
} from '@/utils/scriptHost'
import { executeScriptJob, listRuntimeVars, setRuntimeVars } from '@/utils/scriptRunner'
import type { HttpResult } from '@/types/models'

/**
 * 这一组测试锁的是「脚本沙箱」的行为契约，不是 Worker 本身：
 *   - 环境没有 Worker（node / 老 WebView）时必须降级执行，而不是报错；
 *   - 超时配置要能被读回并且被夹到合法区间；
 *   - executeScriptJob 的返回结构（request / vars / logs / assertions / error）不能变，
 *     因为 RequestEditor 与 Runner 都按这个结构取值。
 */

function emptyResponse(): HttpResult {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 12,
    sizeBytes: 2,
    headers: {},
    body: '{}',
    truncated: false,
    error: null,
    bodyKind: 'json',
    bodyFile: null,
    bodyBytes: 2,
    rawBytes: 2,
    previewBase64: null,
    timings: null,
    httpVersion: 'HTTP/1.1',
    redirects: [],
    retries: [],
    ignoredHeaders: [],
    cancelled: false,
  }
}

const baseRequest = {
  method: 'GET',
  url: 'https://example.com/api?b=2',
  headers: { 'content-type': 'application/json' } as Record<string, string>,
  params: {},
  bodyType: 'none',
  body: '',
}

afterEach(() => {
  scriptHost.dispose()
  setRuntimeVars({})
})

describe('脚本超时设置', () => {
  it('无本地存储时回落到默认 5s', () => {
    expect(scriptTimeoutMs()).toBe(5000)
  })

  it('设置后能读回，并且被夹在 1–30s', () => {
    // node 环境没有 localStorage，先装一个最小的替身
    const bag = new Map<string, string>()
    ;(globalThis as unknown as { localStorage: Storage }).localStorage = {
      getItem: (key: string) => bag.get(key) ?? null,
      setItem: (key: string, value: string) => void bag.set(key, value),
      removeItem: (key: string) => void bag.delete(key),
      clear: () => bag.clear(),
      key: () => null,
      length: 0,
    } as Storage

    expect(setScriptTimeoutMs(8000)).toBe(8000)
    expect(scriptTimeoutMs()).toBe(8000)

    // 小于下限 / 大于上限都要被夹住，避免用户填 0 之后所有脚本秒超时
    expect(setScriptTimeoutMs(1)).toBe(SCRIPT_TIMEOUT_RANGE.min)
    expect(setScriptTimeoutMs(999_999)).toBe(SCRIPT_TIMEOUT_RANGE.max)
    setScriptTimeoutMs(5000)
  })
})

describe('executeScriptJob', () => {
  it('前置脚本能改写请求并写运行时变量', () => {
    const result = executeScriptJob({
      phase: 'pre',
      code: `
        request.headers['x-token'] = 'abc'
        vars.set('ts', '123')
      `,
      request: { ...baseRequest },
      env: {},
      vars: {},
    })

    expect(result.error).toBeUndefined()
    expect(result.request.headers['x-token']).toBe('abc')
    expect(result.vars.ts).toBe('123')
    // 前置阶段不产生断言
    expect(result.assertions).toEqual([])
  })

  it('后置脚本能读到响应并产生断言', () => {
    const result = executeScriptJob({
      phase: 'post',
      // test() 以「是否抛错」判定通过，所以断言要用 assert
      code: `
        test('状态码 200', () => assert(response.status === 200, '应为 200'))
        test('故意失败', () => assert(response.status === 500, '这里必然失败'))
      `,
      request: { ...baseRequest },
      response: emptyResponse(),
      env: {},
      vars: {},
    })

    expect(result.error).toBeUndefined()
    expect(result.assertions).toHaveLength(2)
    expect(result.assertions[0].passed).toBe(true)
    expect(result.assertions[1].passed).toBe(false)
  })

  it('脚本抛错时把错误原文放进 error，且不吞掉已产生的日志', () => {
    const result = executeScriptJob({
      phase: 'pre',
      code: `
        console.log('before')
        throw new Error('boom')
      `,
      request: { ...baseRequest },
      env: {},
      vars: {},
    })

    expect(result.error).toContain('boom')
    expect(result.logs.some((item) => item.text.includes('before'))).toBe(true)
  })

  it('传入的 vars 是初始值，脚本能继续往上加', () => {
    const result = executeScriptJob({
      phase: 'pre',
      code: `vars.set('stage', 'pre')`,
      request: { ...baseRequest },
      env: {},
      vars: { existed: '1' },
    })

    expect(result.vars).toMatchObject({ existed: '1', stage: 'pre' })
  })
})

describe('runScriptJob 降级', () => {
  it('没有 Worker 时同步降级执行，结果结构与 Worker 一致', async () => {
    const result = await runScriptJob({
      phase: 'pre',
      code: `vars.set('k', 'v')`,
      request: { ...baseRequest },
      env: {},
      vars: {},
    })

    expect(result.error).toBeUndefined()
    expect(result.vars.k).toBe('v')
    expect(Array.isArray(result.logs)).toBe(true)
    expect(Array.isArray(result.assertions)).toBe(true)
    expect(typeof result.durationMs).toBe('number')
  })

  it('脚本抛错时不 reject，而是返回带 error 的结果', async () => {
    const result = await runScriptJob({
      phase: 'pre',
      code: `throw new Error('nope')`,
      request: { ...baseRequest },
      env: {},
      vars: {},
    })

    expect(result.error).toContain('nope')
  })

  it('运行时变量通过 setRuntimeVars 在主线程侧保持同步', async () => {
    const result = await runScriptJob({
      phase: 'pre',
      code: `vars.set('token', 't-1')`,
      request: { ...baseRequest },
      env: {},
      vars: {},
    })
    setRuntimeVars(result.vars)
    expect(listRuntimeVars().token).toBe('t-1')
  })
})
