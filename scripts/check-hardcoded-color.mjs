#!/usr/bin/env node
/**
 * 硬编码样式门禁：统计 hex / rgba / 字面圆角 / 字面投影 / :deep(.ant-*)
 * 超出基线即失败，防止改造期间「边改边加」。
 *
 * 用法：node scripts/check-hardcoded-color.mjs [--write-baseline]
 *
 * 口径（2026-11-22 修正；旧口径把「已令牌化」的声明也计入，基线因此失真）：
 *   - hex     ：颜色字面量。排除 HTML 实体——`&#123;` 这类 `{` 的转义会被误判成 3 位 hex，
 *               旧口径由此虚增了 20 余处
 *   - rgba    ：`rgb(` / `rgba(` 函数式色值
 *   - radius  ：`border-radius` 的字面值；`var(--fm-radius*)` 不计（已令牌化）
 *   - shadow  ：含字面颜色的 `box-shadow`；纯 `var(--fm-shadow*)`、`none`、
 *               颜色走 token 或 color-mix 的不计（已令牌化）
 *   - deep    ：`:deep(.ant-*)` 仅作参考项，不进失败判定。
 *               覆盖 Ant Design 内部样式只能靠它，没有令牌化的替代路径；
 *               计入「硬编码债务」只会逼后来人为了过门禁去写选择器 hack。
 *
 * 口径要点：src/styles/tokens.css 是令牌定义处（自带 59 个 hex），必须排除。
 * 否则 C0 一新增这个文件，hex 就变成 484，门禁当场自锁。
 * src/styles/components.css 是共用组件层，同样豁免。
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const TARGET_DIRS = ['src/views', 'src/components', 'src/App.vue']

/**
 * 基线（2026-11-22 按修正口径实测后收紧，此后只允许更低）。
 * 旧值（错口径）：hex 234 / rgba 19 / radius 447 / shadow 51 / deep 110。
 * 本轮：修正口径（排除实体、排除已令牌化声明）+ 清理 114 处 `var(--fm-x, #hex)` 兜底
 *      + 浮层投影收进 `--fm-shadow-*` + 深色底白字收进 `--fm-on-accent`。
 */
const BASELINE = {
  hex: 129,
  rgba: 12,
  radius: 175,
  shadow: 0,
}

/** 仅展示、不判定 */
const REFERENCE = ['deep']

const EXT = new Set(['.vue', '.ts', '.css'])

function walk(dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name)
    const st = fs.statSync(full)
    if (st.isDirectory()) {
      if (name === 'node_modules' || name.startsWith('.')) continue
      walk(full, out)
    } else if (EXT.has(path.extname(name))) {
      out.push(full)
    }
  }
  return out
}

const files = []
TARGET_DIRS.forEach((d) => {
  const full = path.join(ROOT, d)
  if (fs.statSync(full, { throwIfNoEntry: false })?.isDirectory()) walk(full, files)
  else if (fs.existsSync(full)) files.push(full)
})
// 样式目录单独统计
walk(path.join(ROOT, 'src/styles'), files)

const counts = { hex: 0, rgba: 0, radius: 0, shadow: 0, deep: 0 }
const perFile = {}

const EXEMPT_FILES = new Set([
  path.join(ROOT, 'src/styles/tokens.css'),
  path.join(ROOT, 'src/styles/components.css'),
])

/** 去掉 HTML 实体：`&#123;` 会被 `#[0-9a-f]{3}` 误判成 hex */
const stripEntities = (text) => text.replace(/&#\d+;/g, '')

function measure(text) {
  const body = stripEntities(text)
  return {
    hex: (body.match(/#[0-9a-fA-F]{3,8}\b/g) || []).length,
    rgba: (body.match(/rgba?\(/g) || []).length,
    // 字面圆角：border-radius 后不是 var（用 (?!\s*var\() 而非消费空白，
    // 否则 \s* 的回溯会让否定断言失效，把所有 token 写法也计进来）
    radius: (body.match(/border-radius\s*:(?!\s*var\()/g) || []).length,
    // 字面投影：同一条声明内出现函数式色值才算硬编码颜色
    shadow: (body.match(/box-shadow\s*:[^;}\n]*rgba?\(/g) || []).length,
    deep: (body.match(/:deep\(\s*\.ant-/g) || []).length,
  }
}

for (const file of files) {
  if (EXEMPT_FILES.has(file)) continue
  const c = measure(fs.readFileSync(file, 'utf8'))
  Object.keys(counts).forEach((k) => (counts[k] += c[k]))
  if (c.hex + c.rgba + c.radius + c.shadow + c.deep > 0) {
    perFile[path.relative(ROOT, file)] = c
  }
}

const rows = Object.entries(perFile)
  .sort((a, b) => b[1].hex + b[1].radius - (a[1].hex + a[1].radius))
  .slice(0, 15)

console.log('\n文件                              hex  rgba  radius  shadow  deep')
console.log('─'.repeat(70))
rows.forEach(([f, c]) => {
  console.log(
    `${f.padEnd(32)} ${String(c.hex).padStart(4)} ${String(c.rgba).padStart(5)} ${String(c.radius).padStart(7)} ${String(c.shadow).padStart(7)} ${String(c.deep).padStart(5)}`,
  )
})

if (process.argv.includes('--write-baseline')) {
  console.log('\n实测：', JSON.stringify(counts))
  console.log('请把 BASELINE 改为：', JSON.stringify(counts))
  process.exit(0)
}

console.log('\n硬编码合计：')
let failed = false
Object.entries(BASELINE).forEach(([k, base]) => {
  const now = counts[k]
  const ok = now <= base
  if (!ok) failed = true
  console.log(`  ${k.padEnd(8)} ${String(now).padStart(4)} / 基线 ${base}  ${ok ? '✅' : '❌ 超出'}`)
})

REFERENCE.forEach((k) => {
  console.log(
    `  ${k.padEnd(8)} ${String(counts[k]).padStart(4)}（参考项，不判定：覆盖 antd 内部样式无令牌化替代）`,
  )
})

if (failed) {
  console.error('\n✗ 硬编码数量超过基线。本次改造只允许下降。')
  process.exit(1)
}
console.log('\n✓ 硬编码数量在基线内')
