#!/usr/bin/env node
/**
 * 教程内容守护：断言 10 步 / 18 速查 / 22 问，且 Dashboard 与 Manual 同源。
 * 用法：node scripts/check-manual.mjs
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const DATA = path.join(ROOT, 'src/data/manual.ts')
const DASH = path.join(ROOT, 'src/views/Dashboard.vue')
const MANUAL = path.join(ROOT, 'src/views/Manual.vue')

const EXPECT = { steps: 10, tips: 18, faqs: 22 }
let fail = 0

if (!fs.existsSync(DATA)) {
  console.error('✗ 缺少 src/data/manual.ts，先运行 scripts/extract-manual.mjs')
  process.exit(1)
}

const data = fs.readFileSync(DATA, 'utf8')
// 用 [ \t]* 而非 \s*：\s 会吞换行，导致跨行重复计数；subtitle 里也含 title:，必须锚行首
// 按数组分段计数：整文件数 title 会把 manualSteps 的 10 条也算进去
function sliceExport(src, name) {
  const start = src.indexOf(`export const ${name}`)
  if (start < 0) return ''
  // 类型注解 ManualStep[] 里也有 []，必须从 = 之后再找数组起点
  const eq = src.indexOf('=', start)
  const open = src.indexOf('[', eq)
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '[') depth++
    else if (src[i] === ']') {
      depth--
      if (depth === 0) return src.slice(open, i + 1)
    }
  }
  return ''
}

const stepsBlock = sliceExport(data, 'manualSteps')
const tipsBlock = sliceExport(data, 'manualTips')
const faqsBlock = sliceExport(data, 'manualFaqs')

const counts = {
  steps: (stepsBlock.match(/^[ \t]*key:[ \t]*'/gm) || []).length,
  tips: (tipsBlock.match(/^[ \t]*title:[ \t]*'/gm) || []).length,
  faqs: (faqsBlock.match(/^[ \t]*q:[ \t]*'/gm) || []).length,
}

Object.entries(EXPECT).forEach(([k, want]) => {
  const got = counts[k]
  const ok = got === want
  if (!ok) fail++
  console.log(`${ok ? '✅' : '❌'} ${k}: ${got} / ${want}`)
})

// 数据层不得出现动作：go/action 依赖组件内部状态，留在 Dashboard
if (/(^|\n)\s*(go|action):/.test(stepsBlock)) {
  console.log('❌ manualSteps 里出现了 go/action 字段（函数不能进数据层）')
  fail++
} else {
  console.log('✅ manualSteps 只含内容字段')
}

// 分组元信息必须与 tips 等长，否则筛选 chip 的计数会对不上
const metaCount = (data.match(/group: '[^']+'/g) || []).length
if (metaCount !== counts.tips) {
  console.log(`❌ TIP_META 有 ${metaCount} 项，与 tips ${counts.tips} 条不一致`)
  fail++
} else {
  console.log('✅ TIP_META 与 tips 等长')
}

// 同源检查：两处都不能再内联定义数据
const dash = fs.existsSync(DASH) ? fs.readFileSync(DASH, 'utf8') : ''
const manual = fs.existsSync(MANUAL) ? fs.readFileSync(MANUAL, 'utf8') : ''

const dashInline = /const (tips|faqs)\s*=\s*\[/.test(dash)
if (dashInline) {
  console.log('❌ Dashboard.vue 仍内联定义 tips/faqs，必须改为从 @/data/manual 导入')
  fail++
} else {
  console.log('✅ Dashboard.vue 已改为同源导入')
}

if (manual && !/from '@\/data\/manual'/.test(manual)) {
  console.log('❌ Manual.vue 未从 @/data/manual 导入')
  fail++
}

process.exit(fail ? 1 : 0)
