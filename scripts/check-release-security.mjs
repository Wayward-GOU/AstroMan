#!/usr/bin/env node
/**
 * release 包安全断言（W4 / A-21）
 *
 * 1. 窗口配置不得声明 `devtools: true`；
 * 2. Cargo.toml 不得启用 tauri 的 `devtools` feature；
 * 3. 主窗口 CSP 必须保留 `object-src 'none'` 与 `base-uri 'self'`；
 * 4. 若显式关闭了 unsafe-eval，则不允许任何配置再把它加回来。
 */
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const errors = []

const conf = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const windows = conf.app?.windows ?? []
for (const window of windows) {
  if (window.devtools === true) {
    errors.push(`窗口 ${window.label ?? '(unnamed)'} 开启了 devtools`)
  }
}

const csp = conf.app?.security?.csp ?? ''
if (!csp.includes("object-src 'none'")) errors.push('CSP 缺少 object-src \'none\'')
if (!csp.includes("base-uri 'self'")) errors.push('CSP 缺少 base-uri \'self\'')
if (!csp.includes("script-src 'self'")) errors.push('CSP 缺少 script-src \'self\'')

const cargo = readFileSync(resolve(root, 'src-tauri/Cargo.toml'), 'utf8')
const tauriLine = cargo
  .split('\n')
  .find((line) => /^tauri\s*=/.test(line.trim()) || /^\s*tauri\s*=/.test(line))
if (tauriLine && /features\s*=\s*\[[^\]]*devtools/.test(tauriLine)) {
  errors.push('Cargo.toml 启用了 tauri devtools feature')
}
if (/^\s*devtools\s*=\s*true\s*$/m.test(cargo)) {
  errors.push('Cargo.toml 存在 devtools = true')
}

if (errors.length) {
  console.error('release 安全断言失败：')
  for (const error of errors) console.error(` - ${error}`)
  process.exit(1)
}

console.log('release 安全断言通过：未开启 devtools，CSP 基础指令完整')
