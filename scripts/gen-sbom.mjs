#!/usr/bin/env node
/**
 * 生成 CycloneDX 1.5 SBOM（供应链安全基线，W4）
 *
 * 不依赖第三方包：直接解析 Cargo.lock 与 package-lock.json，
 * 输出到 `dist/sbom.cdx.json`（可用 `--out` 指定路径）。
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const args = process.argv.slice(2)
const outIndex = args.indexOf('--out')
const out = resolve(
  root,
  outIndex >= 0 && args[outIndex + 1] ? args[outIndex + 1] : 'dist/sbom.cdx.json',
)

const components = []
let appVersion = '0.0.0'

try {
  const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'))
  appVersion = pkg.version ?? appVersion
} catch {
  // package.json 缺失时继续生成，只影响元数据
}

function add(name, version, purl, scope, hashes = []) {
  if (!name || !version) return
  components.push({
    type: 'library',
    name,
    version,
    purl,
    scope,
    ...(hashes.length ? { hashes } : {}),
  })
}

/* ---------- 前端依赖 ---------- */
try {
  const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json'), 'utf8'))
  const packages = lock.packages ?? {}
  for (const [path, meta] of Object.entries(packages)) {
    if (!path || !meta?.version) continue
    const name = meta.name ?? path.replace(/^node_modules\//, '').split('node_modules/').pop()
    if (!name) continue
    const scope = meta.dev ? 'optional' : 'required'
    const hashes = []
    if (typeof meta.integrity === 'string') {
      const [algorithm, value] = meta.integrity.split('-', 2)
      if (algorithm && value) {
        hashes.push({ alg: algorithm.toUpperCase().replace('SHA', 'SHA-'), content: value })
      }
    }
    add(name, meta.version, `pkg:npm/${encodeURIComponent(name)}@${meta.version}`, scope, hashes)
  }
} catch {
  console.warn('未找到 package-lock.json，跳过前端依赖')
}

/* ---------- Rust 依赖 ---------- */
try {
  const cargo = readFileSync(resolve(root, 'src-tauri/Cargo.lock'), 'utf8')
  const blocks = cargo.split(/^\[\[package\]\]$/m).slice(1)
  for (const block of blocks) {
    const name = /^name\s*=\s*"([^"]+)"/m.exec(block)?.[1]
    const version = /^version\s*=\s*"([^"]+)"/m.exec(block)?.[1]
    const checksum = /^checksum\s*=\s*"([^"]+)"/m.exec(block)?.[1]
    const source = /^source\s*=\s*"([^"]+)"/m.exec(block)?.[1] ?? ''
    if (!name || !version) continue
    // 只收外部依赖，跳过 workspace 自身的包
    if (!source) continue
    add(
      name,
      version,
      `pkg:cargo/${encodeURIComponent(name)}@${version}`,
      'required',
      checksum ? [{ alg: 'SHA-256', content: checksum }] : [],
    )
  }
} catch {
  console.warn('未找到 src-tauri/Cargo.lock，跳过 Rust 依赖')
}

components.sort((a, b) => a.purl.localeCompare(b.purl))

const bom = {
  bomFormat: 'CycloneDX',
  specVersion: '1.5',
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: [{ vendor: 'AstroMan', name: 'gen-sbom', version: '1' }],
    component: {
      type: 'application',
      name: 'AstroMan',
      version: appVersion,
    },
  },
  components,
}

mkdirSync(dirname(out), { recursive: true })
writeFileSync(out, `${JSON.stringify(bom, null, 2)}\n`, 'utf8')
console.log(`SBOM 已生成：${out}（${components.length} 个组件）`)
