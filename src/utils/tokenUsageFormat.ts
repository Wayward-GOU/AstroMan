/** Token 看板的数字与时间格式化：只做展示，不参与计算 */
import type { TokenSourceId } from '@/types/tokenUsage'

/** 大数字缩写：≥1 亿用「亿」，≥1 万用「万」，其余千分位 */
export function fmtTokens(n: number): string {
  if (!Number.isFinite(n)) return '—'
  if (n >= 1e8) return `${(n / 1e8).toFixed(2)} 亿`
  if (n >= 1e4) return `${(n / 1e4).toFixed(1)} 万`
  return fmtInt(n)
}

export function fmtInt(n: number): string {
  if (!Number.isFinite(n)) return '—'
  return Math.round(n).toLocaleString('zh-CN')
}

export function fmtPct(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—'
  return `${n.toFixed(digits)}%`
}

export function fmtDelta(v: number | null): { text: string; dir: 'up' | 'down' | 'flat' } {
  if (v === null || !Number.isFinite(v)) return { text: '无对比', dir: 'flat' }
  if (Math.abs(v) < 0.05) return { text: '持平', dir: 'flat' }
  return { text: `${v > 0 ? '+' : ''}${v.toFixed(1)}%`, dir: v > 0 ? 'up' : 'down' }
}

export function fmtTime(ts: number): string {
  if (!ts) return '—'
  const d = new Date(ts)
  const p = (n: number): string => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

/** `2026-11-27` → `11/27` */
export function fmtDateShort(d: string): string {
  return d.slice(5).replace('-', '/')
}

export function fmtCredit(n: number): string {
  if (!n) return '0'
  return n >= 1000 ? fmtInt(n) : n.toFixed(n < 10 ? 2 : 0)
}

/** 体积 MB：一位小数，未知显示「—」 */
export function fmtMB(n: number | null): string {
  if (n === null || !Number.isFinite(n)) return '—'
  return n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${n.toFixed(1)} MB`
}

/**
 * 固定汇率：只用于把 CC Switch 的美元账面折算成人民币展示，**不是实时行情**。
 * 页面必须同时标注「估算 / 固定汇率」，不要让用户当成账单。
 */
export const USD_TO_CNY = 7.1

/** 美元金额：≥1 两位小数，小额保留有效位，避免 0.00 */
export function fmtUsd(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '$0'
  if (n >= 1) return `$${n.toFixed(2)}`
  if (n >= 0.01) return `$${n.toFixed(3)}`
  return `$${n.toFixed(4)}`
}

/** 折算人民币：¥ 为固定汇率，非实时 */
export function fmtCnyFromUsd(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '¥0'
  const cny = n * USD_TO_CNY
  return cny >= 1 ? `¥${cny.toFixed(2)}` : `¥${cny.toFixed(3)}`
}

export const SOURCE_SHORT: Record<TokenSourceId, string> = {
  workbuddy: 'WorkBuddy',
  codex: 'Codex',
  claude: 'Claude',
  chatgpt: 'ChatGPT',
  ccswitch: 'CC Switch',
  capture: '抓包',
}
