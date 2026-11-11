#!/usr/bin/env node
/**
 * 21 套主题重映射（一次性脚本 + CI 自检）
 *
 * 用法：
 *   node scripts/theme-migrate.mjs            # 生成 theme.generated.ts 并打印对比度表
 *   node scripts/theme-migrate.mjs --check    # 只自检，不写文件（CI 用）
 *
 * 规则：
 *   1. 主色按 PRIMARY_MAP 固定映射（保色相，降饱和，明度达标）
 *   2. 其余 37 键按亮/暗分组套用 tokens.ts 的基准组
 *   3. 每套主题自检最差对比度，< 4.5 退出码 1
 *
 * 说明：脚本只处理 PRIMARY_MAP 里的 21 套旧主题；theme.ts 中新增的
 * paper / night 两套由 toCssVars() 直接生成，不在迁移范围内。
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const SRC = path.join(ROOT, 'src/stores/theme.ts')
const OUT = path.join(ROOT, 'src/stores/theme.generated.ts')
const CHECK_ONLY = process.argv.includes('--check')

/* ---------- 主色映射表：来自设计稿实算，不要改成运行时随机 ---------- */
const PRIMARY_MAP = {
  ocean: '#445FE3', graphite: '#4F6A86', forest: '#327959', midnight: '#90ABE2',
  amber: '#9D5C29', teal: '#1B7870', rose: '#BF3A68', clay: '#9E5950',
  dusk: '#A3B4F2', violet: '#7155D8', sky: '#1F749A', mint: '#1C7A5B',
  coral: '#B94728', cocoa: '#866652', lime: '#50771A', steel: '#4D708C',
  indigo: '#99A6F0', plum: '#C693C7', pine: '#7AB99A', slate: '#9AB4C5',
  aurora: '#5FC1B8',
}

/* ---------- 基准组：与 tokens.ts 的 paperColors / nightColors 必须一致 ---------- */
const PAPER = {
  'primary-soft': '#E4EDEA', 'success': '#2A7253', 'success-soft': '#E7F0EB',
  'warning': '#8F6118', 'warning-soft': '#F6EEE0',
  'danger': '#A83A2C', 'danger-soft': '#F6E7E4',
  'purple': '#6A4A87', 'purple-soft': '#EFEAF4',
  'bg': '#F3F1EC', 'surface': '#FBFAF7', 'surface-alt': '#F0EEE8',
  'border': '#DBD6CB', 'border-strong': '#C9C2B5', 'border-control': '#C9C2B5',
  'text': '#14181B', 'text-2': '#39424A', 'muted': '#616C74',
  'sidebar': '#F7F5F0', 'sidebar-hover': '#EFEDE6', 'sidebar-active': '#E4EDEA',
  'sidebar-text': '#4E585F', 'sidebar-icon': '#6C777E',
  'code-bg': '#FBFAF7', 'gutter-bg': '#F0EEE8',
  'json-key': '#2B5D8C', 'json-string': '#2A7253', 'json-number': '#8F6118',
  'json-bool': '#6A4A87', 'json-punct': '#8B959B',
  'shadow-sm': 'none', 'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(20, 24, 27, 0.16)',
  'brass': '#A97C3F', 'brass-text': '#8A6530',
}

const NIGHT = {
  'primary-soft': '#16302E', 'success': '#7FB79A', 'success-soft': '#16241D',
  'warning': '#C9A063', 'warning-soft': '#2A2317',
  'danger': '#D0705F', 'danger-soft': '#2E1C19',
  'purple': '#B49FD0', 'purple-soft': '#232030',
  'bg': '#0E1215', 'surface': '#12171B', 'surface-alt': '#171D22',
  'border': '#2C353D', 'border-strong': '#35404A', 'border-control': '#35404A',
  'text': '#E9E6E0', 'text-2': '#B9BFC3', 'muted': '#7E888E',
  'sidebar': '#11161A', 'sidebar-hover': '#1A2126', 'sidebar-active': '#1D272D',
  'sidebar-text': '#98A2A8', 'sidebar-icon': '#7E888E',
  'code-bg': '#10151A', 'gutter-bg': '#141A20',
  'json-key': '#7FA8C9', 'json-string': '#7FB79A', 'json-number': '#C9A063',
  'json-bool': '#B49FD0', 'json-punct': '#6F7A80',
  'shadow-sm': 'none', 'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(0, 0, 0, 0.5)',
  'brass': '#C9A063', 'brass-text': '#C9A063',
}

const ORDER = [
  'primary', 'primary-dark', 'primary-soft',
  'success', 'success-soft', 'warning', 'warning-soft', 'danger', 'danger-soft',
  'purple', 'purple-soft',
  'bg', 'surface', 'surface-alt', 'border', 'border-strong', 'border-control',
  'text', 'text-2', 'muted',
  'sidebar', 'sidebar-hover', 'sidebar-active', 'sidebar-text', 'sidebar-icon', 'sidebar-active-text',
  'code-bg', 'gutter-bg',
  'json-key', 'json-string', 'json-number', 'json-bool', 'json-punct',
  'shadow-sm', 'shadow', 'shadow-lg', 'brass', 'brass-text',
]

/* ---------- 色彩工具 ---------- */
const hex2rgb = (h) => {
  const s = h.replace('#', '')
  const f = s.length === 3 ? s.split('').map((c) => c + c).join('') : s
  return [parseInt(f.slice(0, 2), 16), parseInt(f.slice(2, 4), 16), parseInt(f.slice(4, 6), 16)]
}
const rgb2hex = (r, g, b) =>
  '#' + [r, g, b].map((n) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')).join('').toUpperCase()
const mix = (a, b, t) => {
  const [r1, g1, b1] = hex2rgb(a); const [r2, g2, b2] = hex2rgb(b)
  return rgb2hex(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t)
}
const shade = (h, amt) => (amt >= 0 ? mix(h, '#FFFFFF', amt) : mix(h, '#000000', -amt))

const lum = (hex) => {
  const [r, g, b] = hex2rgb(hex).map((v) => v / 255)
  const f = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}
const contrast = (a, b) => {
  const la = lum(a); const lb = lum(b); const hi = Math.max(la, lb); const lo = Math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
}

/* ---------- 解析旧 theme.ts（只取 PRIMARY_MAP 里的 21 套） ---------- */
const source = fs.readFileSync(SRC, 'utf8')
const blocks = []
const re = /const (\w+): ThemeDefinition = \{([\s\S]*?)\r?\n\}\r?\n/g
let m
while ((m = re.exec(source))) {
  const body = m[2]
  const pick = (k) => (body.match(new RegExp(`\\b${k}: '([^']*)'`)) || [])[1]
  const id = pick('id')
  if (!Object.prototype.hasOwnProperty.call(PRIMARY_MAP, id)) continue
  const vars = {}
  const vre = /'(--fm-[\w-]+)':\s*'([^']*)'/g
  let v
  while ((v = vre.exec(body))) vars[v[1]] = v[2]
  blocks.push({
    varName: m[1],
    id,
    name: pick('name'),
    desc: pick('desc'),
    dark: /dark: true/.test(body),
    oldPrimary: vars['--fm-primary'],
  })
}

if (blocks.length !== 21) {
  console.error(`✗ 解析到 ${blocks.length} 套旧主题，预期 21 套。请检查 theme.ts 结构是否被改动。`)
  process.exit(1)
}

/* ---------- 生成新 vars + 自检 ---------- */
const rows = []
const out = []

for (const b of blocks) {
  const primary = PRIMARY_MAP[b.id]
  const base = b.dark ? NIGHT : PAPER
  const vars = {
    ...base,
    'primary': primary,
    'primary-dark': b.dark ? shade(primary, 0.12) : shade(primary, -0.14),
    'primary-soft': b.dark ? mix(primary, '#000000', 0.82) : mix(primary, base.surface, 0.88),
    'sidebar-active-text': primary,
  }

  const missing = ORDER.filter((k) => !vars[k])
  if (missing.length) {
    console.error(`✗ ${b.id} 生成后缺键: ${missing.join(', ')}`)
    process.exit(1)
  }

  const surface = vars['surface']
  const alt = vars['surface-alt']
  const checks = [
    ['text', vars['text'], surface, 4.5],
    ['text-2', vars['text-2'], surface, 4.5],
    ['muted', vars['muted'], alt, 4.5],
    ['primary', vars['primary'], surface, 4.5],
    ['primary/bg', vars['primary'], vars['bg'], 4.5],
    ['success', vars['success'], alt, 4.5],
    ['warning', vars['warning'], alt, 4.5],
    ['danger', vars['danger'], alt, 4.5],
    ['brass-text', vars['brass-text'], vars['bg'], 4.5],
    ['border-control', vars['border-control'], surface, 1.5],
    ['border', vars['border'], surface, 1.3],
  ]
  // 最差余量 = 各对对比度 / 该对阈值，<1 即不达标
  const margin = Math.min(...checks.map(([, f, bg, min]) => contrast(f, bg) / min))
  const worst = Math.min(...checks.filter((c) => c[3] >= 4.5).map(([, f, bg]) => contrast(f, bg)))
  const primaryCr = contrast(primary, surface)

  rows.push({
    id: b.id, name: b.name, dark: b.dark,
    old: b.oldPrimary, next: primary,
    primaryCr: primaryCr.toFixed(2),
    worst: worst.toFixed(2),
    pass: margin >= 1,
  })

  const varLines = ORDER.map((k) => `    '--fm-${k}': '${vars[k]}',`).join('\n')
  out.push(
`const ${b.varName}: ThemeDefinition = {
  id: '${b.id}',
  name: '${b.name}',
  desc: '${b.desc}',
  dark: ${b.dark},
  swatches: ['${vars['bg']}', '${vars['sidebar']}', '${primary}', '${vars['text']}'],
  vars: {
${varLines}
  },
}`)
}

/* ---------- 打印对比度表 ---------- */
console.log('')
console.log('| # | 主题 | 旧主色 | 新主色 | 主色/底色 | 全套最差 | 判定 |')
console.log('|---|---|---|---|---|---|---|')
rows.forEach((r, i) => {
  console.log(`| ${String(i + 1).padStart(2, '0')} | ${r.name} \`${r.id}\` | \`${r.old}\` | \`${r.next}\` | ${r.primaryCr} | ${r.worst} | ${r.pass ? '✅' : '❌'} |`)
})

const failed = rows.filter((r) => !r.pass)
if (failed.length) {
  console.error(`\n✗ ${failed.length} 套主题对比度未达 4.5：${failed.map((r) => r.id).join(', ')}`)
  process.exit(1)
}
console.log(`\n✓ ${rows.length} 套主题全部达标`)

if (CHECK_ONLY) process.exit(0)

/* ---------- 输出 ---------- */
const header = `/**
 * 本文件由 scripts/theme-migrate.mjs 生成，请勿手工编辑。
 * 生成时间：${new Date().toISOString()}
 * 来源：src/stores/theme.ts + src/design/tokens.ts
 */
import type { ThemeDefinition } from './theme'

`
fs.writeFileSync(OUT, header + out.join('\n\n') + '\n', 'utf8')
console.log(`✓ 已写入 ${path.relative(ROOT, OUT)}`)
