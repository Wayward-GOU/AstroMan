#!/usr/bin/env node
/**
 * 版本号同步脚本
 *
 * 版本号在三个文件里各存一份，发版时最容易漏改其中一个：
 *   - package.json            （前端与 package-lock）
 *   - src-tauri/tauri.conf.json（安装包 / 关于页显示的版本）
 *   - src-tauri/Cargo.toml     （Rust 侧版本）
 *
 * 用法：
 *   node scripts/sync-version.mjs            # 只校验，不一致就退出码 1（CI 用这个）
 *   node scripts/sync-version.mjs 2.0.0      # 把三处统一写成 2.0.0
 *
 * 注意：Cargo.lock 里的版本由 cargo 自己维护，这里不动它 —— 跑一次
 * `cargo check` 就会同步，强行改容易和依赖树里的版本记录冲突。
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** 每个目标文件用各自的匹配规则，避免正则误伤依赖里的 version */
const targets = [
  {
    file: 'package.json',
    read: (text) => JSON.parse(text).version,
    write: (text, version) =>
      text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
  },
  {
    file: 'src-tauri/tauri.conf.json',
    read: (text) => JSON.parse(text).version,
    write: (text, version) =>
      text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
  },
  {
    file: 'src-tauri/Cargo.toml',
    // 只取第一个顶层的 version，避免命中 [dependencies.x] 里的 version
    read: (text) => {
      const match = /^\s*version\s*=\s*"([^"]+)"/m.exec(text)
      if (!match) throw new Error('Cargo.toml 里找不到顶层 version')
      return match[1]
    },
    write: (text, version) =>
      text.replace(/^(\s*version\s*=\s*")[^"]+(")/m, `$1${version}$2`),
  },
]

const SEMVER = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/

function collect() {
  return targets.map((target) => {
    const path = join(root, target.file)
    const text = readFileSync(path, 'utf8')
    return { ...target, path, text, version: target.read(text) }
  })
}

function main() {
  const wanted = process.argv[2]?.trim()
  const entries = collect()

  if (!wanted) {
    const versions = new Set(entries.map((item) => item.version))
    if (versions.size === 1) {
      console.log(`版本一致：${entries[0].version}`)
      return
    }
    console.error('版本不一致：')
    for (const item of entries) console.error(`  ${item.file}: ${item.version}`)
    console.error('\n执行 `node scripts/sync-version.mjs <版本号>` 统一，或手动修正。')
    process.exit(1)
  }

  if (!SEMVER.test(wanted)) {
    console.error(`版本号格式不正确：${wanted}（期望 1.2.3 这种形式）`)
    process.exit(1)
  }

  for (const item of entries) {
    if (item.version === wanted) {
      console.log(`跳过 ${item.file}（已是 ${wanted}）`)
      continue
    }
    writeFileSync(item.path, item.write(item.text, wanted), 'utf8')
    console.log(`${item.file}: ${item.version} → ${wanted}`)
  }
  console.log('\n完成。如果改了 Cargo.toml，记得跑一次 cargo check 让 Cargo.lock 跟上。')
}

main()
