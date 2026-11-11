#!/usr/bin/env node
/**
 * 硬编码样式门禁：统计 hex / rgba / 字面圆角 / 投影 / :deep(.ant-*)
 * 超出基线即失败，防止改造期间「边改边加」。
 *
 * 用法：node scripts/check-hardcoded-color.mjs [--write-baseline]
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const TARGET_DIRS = ['src/views', 'src/components', 'src/App.vue']

/**
 * 基线（2026-11-10 C3 收尾实测，统计范围含 src/views + src/components + src/App.vue + src/styles）
 * hex 234 / rgba 19 / radius 447 / shadow 51 / deep 110
 * C1 落地表格首末单元格圆角归零（一条声明管四个选择器），radius 净 +1 → 454；
 * C3 结束后按实测值下调（原 425/89/454/73/110），此后只允许更低。
 * 收尾清理 History 回退色与剩余字面圆角，hex 239→234、rgba 20→19。
 *
 * 口径要点：src/styles/tokens.css 是令牌定义处（自带 59 个 hex），必须排除。
 * 否则 C0 一新增这个文件，hex 就变成 484，门禁当场自锁。
 */
const BASELINE = {
  hex: 234,
  rgba: 19,
  radius: 447,
  shadow: 51,
  deep: 110,
}

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

/* 令牌/组件定义处不计入：
   - tokens.css 是色值集中地（59 个 hex）
   - components.css 是共用组件层，只允许 var(--fm-*)，圆角声明本身是设计规格
   两者都是「集中定义」，计入会让门禁在新增文件时自锁。 */
const EXEMPT_FILES = new Set([
  path.join(ROOT, 'src/styles/tokens.css'),
  path.join(ROOT, 'src/styles/components.css'),
])

for (const file of files) {
  if (EXEMPT_FILES.has(file)) continue
  const text = fs.readFileSync(file, 'utf8')
  const c = {
    hex: (text.match(/#[0-9a-fA-F]{3,8}\b/g) || []).length,
    rgba: (text.match(/rgba?\(/g) || []).length,
    radius: (text.match(/border-radius\s*:/g) || []).length,
    shadow: (text.match(/box-shadow\s*:/g) || []).length,
    deep: (text.match(/:deep\(\s*\.ant-/g) || []).length,
  }
  Object.keys(c).forEach((k) => (counts[k] += c[k]))
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

console.log('\n合计：')
let failed = false
Object.entries(BASELINE).forEach(([k, base]) => {
  const now = counts[k]
  const ok = now <= base
  if (!ok) failed = true
  console.log(`  ${k.padEnd(8)} ${String(now).padStart(4)} / 基线 ${base}  ${ok ? '✅' : '❌ 超出'}`)
})

if (failed) {
  console.error('\n✗ 硬编码数量超过基线。本次改造只允许下降。')
  process.exit(1)
}
console.log('\n✓ 硬编码数量在基线内')
