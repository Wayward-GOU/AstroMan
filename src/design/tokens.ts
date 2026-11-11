/**
 * AstroMan 设计令牌 · 单一事实源
 *
 * 分层：
 *   L0 primitive —— 原始色值，不直接使用
 *   L1 semantic  —— 语义色，按亮/暗两组定义，写入 theme.vars
 *   L1s static   —— 形态/间距/字体，写入 tokens.css，不随主题变
 *
 * 规则：颜色只在 theme.vars（运行时内联）生效；形态只在 tokens.css 生效。
 */

/* ---------- L0 原始值 ---------- */
export const primitive = {
  ink: '#14181B',
  ink2: '#39424A',
  paper: '#F3F1EC',
  paperSurface: '#FBFAF7',
  paperAlt: '#F0EEE8',
  hairline: '#DBD6CB',
  hairlineStrong: '#C9C2B5',
  teal: '#0F6B64',
  tealDark: '#0B524D',
  tealSoft: '#E4EDEA',
  jade: '#2A7253',
  brass: '#A97C3F',
  brassText: '#8A6530',
  cinnabar: '#A83A2C',
  amberDeep: '#8F6118',
  violetDeep: '#6A4A87',
  nightBg: '#0E1215',
  nightSurface: '#12171B',
  nightAlt: '#171D22',
  nightLine: '#2C353D',
  nightLineStrong: '#35404A',
  nightInk: '#E9E6E0',
  nightInk2: '#B9BFC3',
  nightMuted: '#7E888E',
  nightTeal: '#56B9AC',
  nightJade: '#7FB79A',
  nightBrass: '#C9A063',
  nightCinnabar: '#D0705F',
  nightViolet: '#B49FD0',
} as const

/* ---------- 色彩键集合（38 个，含 3 个 shadow 与 brass 两键） ---------- */
export const COLOR_KEYS = [
  'primary', 'primary-dark', 'primary-soft',
  'success', 'success-soft',
  'warning', 'warning-soft',
  'danger', 'danger-soft',
  'purple', 'purple-soft',
  'bg', 'surface', 'surface-alt',
  'border', 'border-strong', 'border-control',
  'text', 'text-2', 'muted',
  'sidebar', 'sidebar-hover', 'sidebar-active',
  'sidebar-text', 'sidebar-icon', 'sidebar-active-text',
  'code-bg', 'gutter-bg',
  'json-key', 'json-string', 'json-number', 'json-bool', 'json-punct',
  'shadow-sm', 'shadow', 'shadow-lg',
  'brass', 'brass-text',
] as const

export type ColorKey = (typeof COLOR_KEYS)[number]
export type ColorVars = Record<ColorKey, string>

/* ---------- 色彩工具：只做混色与明度，不引第三方库 ---------- */
function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '')
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ]
}

function rgbToHex(r: number, g: number, b: number): string {
  const to = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')
  return `#${to(r)}${to(g)}${to(b)}`.toUpperCase()
}

/** 以 t 的比例把 b 混入 a（t=0 得 a，t=1 得 b） */
export function mix(a: string, b: string, t: number): string {
  const [r1, g1, b1] = hexToRgb(a)
  const [r2, g2, b2] = hexToRgb(b)
  return rgbToHex(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t)
}

/** amount > 0 提亮，< 0 压暗 */
export function shade(hex: string, amount: number): string {
  return amount >= 0 ? mix(hex, '#FFFFFF', amount) : mix(hex, '#000000', -amount)
}

/* ---------- 颜色派生入口在 themes.ts ---------- */
/* 23 套主题的基色表与 38 令牌派生统一放在 src/design/themes.ts，
   这里只保留与主题无关的键集合、色值工具和静态令牌。 */

/** 原始色组 → 带 --fm- 前缀的对象 */
export function toCssVars(colors: ColorVars): Record<string, string> {
  const out: Record<string, string> = {}
  COLOR_KEYS.forEach((k) => {
    out[`--fm-${k}`] = colors[k]
  })
  return out
}

/* ---------- L1s 静态令牌（形态/间距/字体/动效） ---------- */
export const staticTokens = {
  'radius-sm': '2px',
  'radius': '3px',
  'radius-lg': '3px',
  'gap-1': '4px',
  'gap-2': '8px',
  'gap-3': '12px',
  'gap-4': '16px',
  'gap-6': '24px',
  'gap-8': '32px',
  'font-serif': "'Source Han Serif SC', 'Noto Serif SC', 'Songti SC', SimSun, serif",
  /* 前置 Apple 字体，让 macOS 上的 AntD 原生控件与正文同用 SF Pro；Windows 自动跳过 */
  'font-sans': "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Inter', 'Segoe UI Variable Text', 'Microsoft YaHei UI', 'PingFang SC', sans-serif",
  'font-mono': "'JetBrains Mono', 'Cascadia Mono', 'Consolas', monospace",
  'dur-fast': '120ms',
  'dur-mid': '180ms',
  'dur-slow': '240ms',
  'ease': 'cubic-bezier(0.2, 0.8, 0.2, 1)',
} as const
