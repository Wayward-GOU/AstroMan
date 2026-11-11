#!/usr/bin/env -S vite-node
/**
 * 对比度门禁：直接 import 23 套主题的基色表，逐套派生后审计。
 *
 * 覆盖：
 *   - 正文 / 次级 / 弱化文字与底色的可读性
 *   - 主色与画布、主按钮文字与主色
 *   - 语义色（成功/警告/危险/紫）在次级底色上的可读性
 *   - 侧栏文字、侧栏选中文字与侧栏底色
 *   - 控件描边、发丝线（非文字，阈值更低）
 *
 * 用法：npm run ui:check / npx vite-node scripts/check-contrast.ts
 */
import { PALETTES, buildColorVars, contrastRatio } from '../src/design/themes'

type Pair = [label: string, fg: string, bg: string, min: number]

const TEXT = 4.5

function audit(label: string, vars: ReturnType<typeof buildColorVars>): string[] {
  const pairs: Pair[] = [
    ['正文 / 卡片', vars.text, vars.surface, TEXT],
    ['正文 / 画布', vars.text, vars.bg, TEXT],
    ['次级文字 / 卡片', vars['text-2'], vars.surface, TEXT],
    ['弱化文字 / 卡片', vars.muted, vars.surface, TEXT],
    ['弱化文字 / 次级底色', vars.muted, vars['surface-alt'], TEXT],
    ['主色 / 卡片', vars.primary, vars.surface, TEXT],
    ['主色 / 画布', vars.primary, vars.bg, TEXT],
    ['卡片 / 主色（主按钮文字）', vars.surface, vars.primary, TEXT],
    ['危险 / 次级底色', vars.danger, vars['surface-alt'], TEXT],
    ['成功 / 次级底色', vars.success, vars['surface-alt'], TEXT],
    ['警告 / 次级底色', vars.warning, vars['surface-alt'], TEXT],
    ['紫 / 次级底色', vars.purple, vars['surface-alt'], TEXT],
    ['金铜文字 / 画布', vars['brass-text'], vars.bg, TEXT],
    ['侧栏文字 / 侧栏', vars['sidebar-text'], vars.sidebar, TEXT],
    ['侧栏文字 / 侧栏悬停', vars['sidebar-text'], vars['sidebar-hover'], TEXT],
    ['侧栏选中文字 / 侧栏选中', vars['sidebar-active-text'], vars['sidebar-active'], TEXT],
    ['控件描边 / 卡片（非文字）', vars['border-control'], vars.surface, 1.5],
    ['发丝线 / 卡片（非文字）', vars.border, vars.surface, 1.3],
  ]

  const bad: string[] = []
  pairs.forEach(([name, fg, bg, min]) => {
    const c = contrastRatio(fg, bg)
    if (c < min) bad.push(`${label} · ${name} = ${c.toFixed(2)}（要求 ≥${min}）`)
  })
  return bad
}

const failures: string[] = []
for (const palette of PALETTES) {
  failures.push(...audit(`${palette.name}(${palette.id})`, buildColorVars(palette)))
}

if (failures.length) {
  console.error(`\n✗ ${failures.length} 项对比度未达标：`)
  failures.forEach((line) => console.error(`  - ${line}`))
  process.exit(1)
}

console.log(`\n✓ ${PALETTES.length} 套主题对比度全部达标`)
