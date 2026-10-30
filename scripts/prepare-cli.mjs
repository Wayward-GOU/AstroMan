#!/usr/bin/env node
/**
 * 为 Tauri 打包准备 astroman-cli（T-36）。
 *
 * Tauri 的 bundle.externalBin 要求文件名为 `<name>-<target-triple>[.exe]`，
 * 且必须在 tauri build 之前就存在。这里统一完成：
 *   1. 按目标平台编译 astroman-cli（release）
 *   2. 复制到 src-tauri/binaries/ 并补上 target triple 后缀
 *   3. macOS universal 用 lipo 合并两个架构
 *
 * 用法：
 *   node scripts/prepare-cli.mjs                    # 当前平台默认架构
 *   node scripts/prepare-cli.mjs --target i686-pc-windows-msvc
 *   node scripts/prepare-cli.mjs --target universal-apple-darwin
 */
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, rmSync, statSync, chmodSync, writeFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')
const tauriDir = join(root, 'src-tauri')
const binDir = join(tauriDir, 'binaries')
const isWindows = process.platform === 'win32'
const exeSuffix = isWindows ? '.exe' : ''

function argValue(name) {
  const index = process.argv.indexOf(name)
  return index >= 0 ? process.argv[index + 1] : ''
}

function run(command, args, cwd = tauriDir) {
  console.log(`> ${command} ${args.join(' ')}`)
  execFileSync(command, args, { cwd, stdio: 'inherit' })
}

function placeholder(target) {
  // Tauri 的构建脚本在编译 lib 时就会校验 externalBin 是否存在，
  // 而 CLI 本身也依赖同一 lib，所以先放一个空占位文件打破循环。
  const dest = join(binDir, `astroman-cli-${target}${exeSuffix}`)
  if (!existsSync(dest) || statSync(dest).size === 0) {
    writeFileSync(dest, '')
  }
}

function build(target) {
  placeholder(target)
  const args = ['build', '--release', '--bin', 'astroman-cli']
  if (target) args.push('--target', target)
  run('cargo', args)
  const base = target ? join(tauriDir, 'target', target, 'release') : join(tauriDir, 'target', 'release')
  const binary = join(base, `astroman-cli${exeSuffix}`)
  try {
    statSync(binary)
  } catch {
    throw new Error(`未找到 CLI 产物：${binary}`)
  }
  return binary
}

function install(source, target) {
  const dest = join(binDir, `astroman-cli-${target}${exeSuffix}`)
  copyFileSync(source, dest)
  if (!isWindows) chmodSync(dest, 0o755)
  console.log(`CLI 已就绪：${dest}`)
}

function hostTriple() {
  if (isWindows) {
    return process.arch === 'ia32' ? 'i686-pc-windows-msvc' : 'x86_64-pc-windows-msvc'
  }
  if (process.platform === 'darwin') {
    return process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin'
  }
  return ''
}

mkdirSync(binDir, { recursive: true })
const requested = argValue('--target')

// 开发模式只需要占位文件让 Tauri 编译通过，真正打包前再编译 CLI
if (process.argv.includes('--placeholder')) {
  const triple = requested || hostTriple()
  if (!triple) process.exit(0)
  placeholder(triple)
  console.log(`CLI 占位文件已就绪：astroman-cli-${triple}${exeSuffix}`)
  process.exit(0)
}

if (requested === 'universal-apple-darwin') {
  if (process.platform !== 'darwin') {
    throw new Error('universal-apple-darwin 只能在 macOS 上构建')
  }
  placeholder('universal-apple-darwin')
  const arm = build('aarch64-apple-darwin')
  const intel = build('x86_64-apple-darwin')
  const universal = join(binDir, 'astroman-cli-universal-apple-darwin')
  rmSync(universal, { force: true })
  run('lipo', ['-create', arm, intel, '-output', universal])
  chmodSync(universal, 0o755)
  console.log(`CLI 已就绪：${universal}`)
} else {
  const triple = requested || hostTriple()
  if (!triple) {
    console.log('当前平台不需要 externalBin 后缀处理，跳过 CLI 准备')
    process.exit(0)
  }
  install(build(triple), triple)
}
