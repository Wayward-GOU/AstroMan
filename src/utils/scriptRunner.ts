import type { HttpResult } from '@/types/models'
import { createCipher, type CipherFacade } from '@/utils/cryptoScript'
import { rsaPublicEncrypt } from '@/utils/rsa'

export type ScriptPhase = 'env' | 'pre' | 'post'
export type ScriptLogLevel = 'log' | 'warn' | 'error'

export interface ScriptLog {
  phase: ScriptPhase
  level: ScriptLogLevel
  text: string
}

export interface ScriptAssertion {
  name: string
  passed: boolean
  message?: string
}

/** 前置脚本可修改的请求对象 */
export interface ScriptRequest {
  method: string
  url: string
  headers: Record<string, string>
  params: Record<string, string>
  bodyType: string
  body: string
}

export interface ScriptResponse {
  status: number
  statusText: string
  durationMs: number
  sizeBytes: number
  truncated: boolean
  error?: string | null
  headers: Record<string, string>
  body: string
  text(): string
  json(): unknown
}

interface VarsFacade {
  get(key: string): string | undefined
  set(key: string, value: unknown): void
  unset(key: string): void
  has(key: string): boolean
  all(): Record<string, string>
}

interface ConsoleFacade {
  log(...args: unknown[]): void
  info(...args: unknown[]): void
  debug(...args: unknown[]): void
  warn(...args: unknown[]): void
  error(...args: unknown[]): void
}

interface RsaFacade {
  /** 公钥加密（PKCS#1 v1.5），返回 Base64，等价于 Java 的 RSAUtils.publicEncrypt */
  publicEncrypt(text: unknown, publicKey: unknown): string
}

function createRsa(): RsaFacade {
  return {
    publicEncrypt(text, publicKey) {
      return rsaPublicEncrypt(String(text ?? ''), String(publicKey ?? ''))
    },
  }
}

export interface PreScriptResult {
  request: ScriptRequest
  logs: ScriptLog[]
  error?: string
  durationMs: number
}

export interface PostScriptResult {
  logs: ScriptLog[]
  assertions: ScriptAssertion[]
  error?: string
  durationMs: number
}

/** 本次运行期间脚本写入的变量，发送请求时会被渲染进 URL / Header / Body */
const runtimeVars = new Map<string, string>()

export function listRuntimeVars(): Record<string, string> {
  return Object.fromEntries(runtimeVars)
}

export function clearRuntimeVars() {
  runtimeVars.clear()
}

/** 只替换脚本写入的变量，环境变量仍交给后端渲染 */
export function renderRuntimeVars(text: string): string {
  if (!text || !text.includes('{{') || !runtimeVars.size) return text
  return text.replace(/\{\{\s*([\w.-]+)\s*\}\}/g, (raw, key: string) =>
    runtimeVars.has(key) ? runtimeVars.get(key)! : raw,
  )
}

function createVars(env: Record<string, string>): VarsFacade {
  return {
    get(key) {
      const name = String(key)
      if (runtimeVars.has(name)) return runtimeVars.get(name)
      return Object.prototype.hasOwnProperty.call(env, name) ? env[name] : undefined
    },
    set(key, value) {
      const name = String(key)
      runtimeVars.set(name, value === undefined || value === null ? '' : String(value))
    },
    unset(key) {
      runtimeVars.delete(String(key))
    },
    has(key) {
      const name = String(key)
      return runtimeVars.has(name) || Object.prototype.hasOwnProperty.call(env, name)
    },
    all() {
      return { ...env, ...Object.fromEntries(runtimeVars) }
    },
  }
}

function formatArg(value: unknown): string {
  if (typeof value === 'string') return value
  if (value instanceof Error) return `${value.name}: ${value.message}`
  if (value === undefined) return 'undefined'
  try {
    const text = JSON.stringify(value, null, 2)
    return text === undefined ? String(value) : text
  } catch {
    return String(value)
  }
}

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message || error.name
  return String(error)
}

function createConsole(phase: ScriptPhase, logs: ScriptLog[]): ConsoleFacade {
  const push = (level: ScriptLogLevel, args: unknown[]) => {
    const text = args.map(formatArg).join(' ')
    logs.push({ phase, level, text: text.slice(0, 4000) })
  }
  return {
    log: (...args) => push('log', args),
    info: (...args) => push('log', args),
    debug: (...args) => push('log', args),
    warn: (...args) => push('warn', args),
    error: (...args) => push('error', args),
  }
}

function createResponse(result: HttpResult): ScriptResponse {
  let parsed = false
  let value: unknown
  return {
    status: result.status,
    statusText: result.statusText,
    durationMs: result.durationMs,
    sizeBytes: result.sizeBytes,
    truncated: result.truncated,
    error: result.error ?? null,
    headers: { ...result.headers },
    body: result.body,
    text() {
      return result.body
    },
    json() {
      if (!parsed) {
        parsed = true
        try {
          value = JSON.parse(result.body)
        } catch (e) {
          throw new Error(`响应体不是合法 JSON：${describeError(e)}`)
        }
      }
      return value
    },
  }
}

/** 执行前置脚本：可修改 request.method / url / headers / params / body */
export function runPreScript(
  code: string,
  request: ScriptRequest,
  env: Record<string, string>,
  phase: ScriptPhase = 'pre',
): PreScriptResult {
  const logs: ScriptLog[] = []
  const started = performance.now()
  const target: ScriptRequest = {
    ...request,
    headers: { ...request.headers },
    params: { ...request.params },
  }
  try {
    const runner = new Function('request', 'vars', 'env', 'console', 'rsa', 'cipher', `${code}\n`) as (
      r: ScriptRequest,
      v: VarsFacade,
      e: Record<string, string>,
      c: ConsoleFacade,
      rsa: RsaFacade,
      cipher: CipherFacade,
    ) => void
    runner(target, createVars(env), { ...env }, createConsole(phase, logs), createRsa(), createCipher())
    return { request: target, logs, durationMs: Math.round(performance.now() - started) }
  } catch (e) {
    return {
      request: target,
      logs,
      error: describeError(e),
      durationMs: Math.round(performance.now() - started),
    }
  }
}

/** 执行环境脚本：每次发送请求前运行，可用 vars.set() 为环境生成运行时变量 */
export function runEnvScript(
  code: string,
  request: ScriptRequest,
  env: Record<string, string>,
): PreScriptResult {
  return runPreScript(code, request, env, 'env')
}

/** 执行后置脚本：读取 response，用 test() / assert() 断言，并可用 vars.set() 提取变量 */
export function runPostScript(
  code: string,
  request: ScriptRequest,
  result: HttpResult,
  env: Record<string, string>,
): PostScriptResult {
  const logs: ScriptLog[] = []
  const assertions: ScriptAssertion[] = []
  const started = performance.now()
  const response = createResponse(result)

  const test = (name: unknown, fn: unknown) => {
    const label = String(name ?? '未命名断言')
    if (typeof fn !== 'function') {
      assertions.push({ name: label, passed: false, message: '测试函数无效' })
      return
    }
    try {
      ;(fn as () => void)()
      assertions.push({ name: label, passed: true })
    } catch (e) {
      assertions.push({ name: label, passed: false, message: describeError(e) })
    }
  }

  const assert = (condition: unknown, message?: unknown) => {
    if (!condition) throw new Error(message ? String(message) : '断言失败')
  }

  try {
    const runner = new Function(
      'request',
      'response',
      'vars',
      'env',
      'console',
      'rsa',
      'cipher',
      'test',
      'assert',
      `${code}\n`,
    ) as (
      r: ScriptRequest,
      res: ScriptResponse,
      v: VarsFacade,
      e: Record<string, string>,
      c: ConsoleFacade,
      rsa: RsaFacade,
      cipher: CipherFacade,
      t: (name: unknown, fn: unknown) => void,
      a: (condition: unknown, message?: unknown) => void,
    ) => void
    runner(
      { ...request, headers: { ...request.headers }, params: { ...request.params } },
      response,
      createVars(env),
      { ...env },
      createConsole('post', logs),
      createRsa(),
      createCipher(),
      test,
      assert,
    )
    return {
      logs,
      assertions,
      durationMs: Math.round(performance.now() - started),
    }
  } catch (e) {
    return {
      logs,
      assertions,
      error: describeError(e),
      durationMs: Math.round(performance.now() - started),
    }
  }
}
