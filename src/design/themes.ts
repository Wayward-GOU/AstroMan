/**
 * 23 套主题的基色表 · 颜色唯一事实源
 *
 * 每套主题只维护 13 个基色（画布/卡片/次级底色/侧栏/描边/文字/主色），
 * 其余 38 个设计令牌由 buildColorVars 派生，避免再出现「21 套主题只是换主色、
 * 卡片和侧栏全是同一组纸色」的问题。
 *
 * 约定：
 *   - 亮色主题也使用深色侧栏，形成稳定的导航锚点；
 *   - 派生只做混色与明度，不引第三方库；
 *   - 对比度门禁 scripts/check-contrast.ts 直接 import 本文件。
 */
import { mix, type ColorVars } from './tokens'

/** 主题 id 顺序即设置页展示顺序 */
export const THEME_IDS = [
  'paper',
  'night',
  'ocean',
  'graphite',
  'forest',
  'midnight',
  'amber',
  'teal',
  'rose',
  'clay',
  'dusk',
  'violet',
  'sky',
  'mint',
  'coral',
  'cocoa',
  'lime',
  'steel',
  'indigo',
  'plum',
  'pine',
  'slate',
  'aurora',
] as const

export type ThemeId = (typeof THEME_IDS)[number]

export interface BasePalette {
  id: ThemeId
  name: string
  desc: string
  dark: boolean
  /** 页面画布 */
  bg: string
  /** 卡片 / 输入框 / 弹层底色 */
  surface: string
  /** 次级底色：表头、悬停行、代码块 */
  alt: string
  /** 侧栏底色（亮色主题也用深色） */
  sidebar: string
  /** 侧栏正文与图标 */
  sidebarText: string
  /** 发丝线 */
  border: string
  /** 控件描边与强分隔 */
  borderStrong: string
  /** 正文 */
  text: string
  /** 次级文字 */
  text2: string
  /** 弱化文字 */
  muted: string
  primary: string
  /** 亮色主题的金铜文字色；缺省用统一值 */
  brassText?: string
}

export const PALETTES: BasePalette[] = [
  {
    id: 'paper',
    name: '宣纸',
    desc: '默认 · 暖纸墨青',
    dark: false,
    bg: '#F3F1EC',
    surface: '#FBFAF7',
    alt: '#F0EEE8',
    sidebar: '#15181A',
    sidebarText: '#A8B0A9',
    border: '#DBD6CB',
    borderStrong: '#C9C2B5',
    text: '#1C1F1B',
    text2: '#3E4742',
    muted: '#636C65',
    primary: '#0F6B64',
    brassText: '#7A5626',
  },
  {
    id: 'night',
    name: '夜航舱',
    desc: '深色 · 长时间读数据',
    dark: true,
    bg: '#0E1214',
    surface: '#141A1D',
    alt: '#191F23',
    sidebar: '#090C0E',
    sidebarText: '#9FADAB',
    border: '#2A3533',
    borderStrong: '#34403E',
    text: '#E6EDEA',
    text2: '#B6C2BE',
    muted: '#889794',
    primary: '#4FC3A6',
  },
  {
    id: 'ocean',
    name: '极光蓝',
    desc: '亮色 · 鲜明现代',
    dark: false,
    bg: '#EEF2F7',
    surface: '#FFFFFF',
    alt: '#F4F7FB',
    sidebar: '#132033',
    sidebarText: '#A3B2C4',
    border: '#D5DEE9',
    borderStrong: '#C6D2E0',
    text: '#172027',
    text2: '#3A4652',
    muted: '#66737D',
    primary: '#2A66C2',
  },
  {
    id: 'graphite',
    name: '石墨灰',
    desc: '中性 · 专注数据',
    dark: false,
    bg: '#F0F2F3',
    surface: '#FFFFFF',
    alt: '#F6F8F8',
    sidebar: '#1C2429',
    sidebarText: '#A5B0B5',
    border: '#DDE2E4',
    borderStrong: '#C6CED1',
    text: '#1A2126',
    text2: '#3D484E',
    muted: '#68747A',
    primary: '#4F6A86',
  },
  {
    id: 'forest',
    name: '森林绿',
    desc: '自然 · 低饱和',
    dark: false,
    bg: '#EFF2EC',
    surface: '#FCFDF9',
    alt: '#F4F7F0',
    sidebar: '#16211A',
    sidebarText: '#A2B2A6',
    border: '#DBE1D6',
    borderStrong: '#C5CEBF',
    text: '#1A221B',
    text2: '#3C473D',
    muted: '#647164',
    primary: '#2F7A55',
  },
  {
    id: 'midnight',
    name: '暗夜',
    desc: '深色 · 夜间调试',
    dark: true,
    bg: '#090C0F',
    surface: '#0F1418',
    alt: '#141A1F',
    sidebar: '#050708',
    sidebarText: '#98A5AD',
    border: '#263037',
    borderStrong: '#303C44',
    text: '#E4EBEF',
    text2: '#B4C0C7',
    muted: '#87949B',
    primary: '#7FB2E8',
  },
  {
    id: 'amber',
    name: '琥珀橙',
    desc: '暖调 · 品牌同色',
    dark: false,
    bg: '#F4EFE7',
    surface: '#FFFAF3',
    alt: '#F8F1E6',
    sidebar: '#241C14',
    sidebarText: '#B4A493',
    border: '#E0D5C5',
    borderStrong: '#CDBFA9',
    text: '#241E16',
    text2: '#4A4033',
    muted: '#776A58',
    primary: '#9C5A16',
    brassText: '#7A5626',
  },
  {
    id: 'teal',
    name: '青碧',
    desc: '清爽 · 冷绿调',
    dark: false,
    bg: '#EDF3F1',
    surface: '#FBFDFC',
    alt: '#F2F8F6',
    sidebar: '#10201D',
    sidebarText: '#9FB4AF',
    border: '#D2DED9',
    borderStrong: '#BFCFC9',
    text: '#17211E',
    text2: '#3A4743',
    muted: '#65756F',
    primary: '#0F7A6A',
  },
  {
    id: 'rose',
    name: '玫瑰',
    desc: '柔和 · 暖粉调',
    dark: false,
    bg: '#F6EEEF',
    surface: '#FFFAFA',
    alt: '#FAF2F3',
    sidebar: '#241A1D',
    sidebarText: '#B4A2A8',
    border: '#E3D6D8',
    borderStrong: '#D0BEC1',
    text: '#241A1D',
    text2: '#4B3B3F',
    muted: '#7A666B',
    primary: '#A33B5C',
  },
  {
    id: 'clay',
    name: '陶土红',
    desc: '克制 · 暖而不刺眼',
    dark: false,
    bg: '#F5EFEA',
    surface: '#FDF9F5',
    alt: '#F8F1EB',
    sidebar: '#251B16',
    sidebarText: '#B4A49A',
    border: '#E2D7CD',
    borderStrong: '#CEBFB1',
    text: '#251C17',
    text2: '#4B3E36',
    muted: '#796A5E',
    primary: '#A24E2E',
  },
  {
    id: 'dusk',
    name: '暮色',
    desc: '深色 · 紫蓝夜空',
    dark: true,
    bg: '#14121A',
    surface: '#1A1721',
    alt: '#211D28',
    sidebar: '#0D0B11',
    sidebarText: '#A79FB8',
    border: '#363040',
    borderStrong: '#413A52',
    text: '#E9E4F1',
    text2: '#BBB3CC',
    muted: '#8C849C',
    primary: '#A08AD8',
  },
  {
    id: 'violet',
    name: '紫罗兰',
    desc: '优雅 · 紫调',
    dark: false,
    bg: '#F3F0F7',
    surface: '#FCFAFF',
    alt: '#F7F3FB',
    sidebar: '#1E1829',
    sidebarText: '#ADA4BF',
    border: '#E0D9EA',
    borderStrong: '#CDC2DC',
    text: '#211A2B',
    text2: '#443A50',
    muted: '#746A80',
    primary: '#7652B0',
  },
  {
    id: 'sky',
    name: '天青',
    desc: '明快 · 高饱和蓝',
    dark: false,
    bg: '#EDF2F5',
    surface: '#FBFDFE',
    alt: '#F2F7FA',
    sidebar: '#142029',
    sidebarText: '#A0AFBA',
    border: '#D7E1E8',
    borderStrong: '#BFCFD9',
    text: '#162029',
    text2: '#394852',
    muted: '#5F707A',
    primary: '#2E6F9E',
  },
  {
    id: 'mint',
    name: '薄荷',
    desc: '清凉 · 青绿',
    dark: false,
    bg: '#EDF4F0',
    surface: '#FBFEFC',
    alt: '#F1F8F4',
    sidebar: '#13221C',
    sidebarText: '#9FB5AB',
    border: '#D6E3DC',
    borderStrong: '#BED1C7',
    text: '#16211C',
    text2: '#3A4A42',
    muted: '#617369',
    primary: '#257A5D',
  },
  {
    id: 'coral',
    name: '珊瑚',
    desc: '活力 · 暖橘红',
    dark: false,
    bg: '#F7EFEC',
    surface: '#FFFBF9',
    alt: '#FBF2EF',
    sidebar: '#261A17',
    sidebarText: '#B6A39D',
    border: '#E4D6D1',
    borderStrong: '#D1BEB7',
    text: '#251C19',
    text2: '#4C3D38',
    muted: '#7B6A64',
    primary: '#B1513C',
  },
  {
    id: 'cocoa',
    name: '可可',
    desc: '温润 · 大地棕',
    dark: false,
    bg: '#F0EBE1',
    surface: '#FCF9F2',
    alt: '#F6F0E5',
    sidebar: '#2A231B',
    sidebarText: '#B8AC9B',
    border: '#DFD5C3',
    borderStrong: '#CCBEA6',
    text: '#2A241B',
    text2: '#4F4639',
    muted: '#756A58',
    primary: '#8F5624',
    brassText: '#7A5626',
  },
  {
    id: 'lime',
    name: '青柠',
    desc: '跳跃 · 黄绿',
    dark: false,
    bg: '#F1F3E9',
    surface: '#FCFDF7',
    alt: '#F5F7EC',
    sidebar: '#1D2114',
    sidebarText: '#AEB49F',
    border: '#D6DCC8',
    borderStrong: '#C9D0B8',
    text: '#1E2316',
    text2: '#424A35',
    muted: '#697257',
    primary: '#56701F',
  },
  {
    id: 'steel',
    name: '钢蓝',
    desc: '克制 · 冷灰蓝',
    dark: false,
    bg: '#EDF0F3',
    surface: '#FAFCFD',
    alt: '#F2F6F8',
    sidebar: '#161E25',
    sidebarText: '#A2AEB8',
    border: '#D2DCE2',
    borderStrong: '#C1CDD5',
    text: '#182129',
    text2: '#3B4852',
    muted: '#62717B',
    primary: '#40657F',
  },
  {
    id: 'indigo',
    name: '靛夜',
    desc: '深色 · 靛蓝',
    dark: true,
    bg: '#0C0F18',
    surface: '#121622',
    alt: '#171C2A',
    sidebar: '#070910',
    sidebarText: '#9CA6C4',
    border: '#293044',
    borderStrong: '#343C55',
    text: '#E5E8F4',
    text2: '#B4BAD2',
    muted: '#878FA9',
    primary: '#7C8FE8',
  },
  {
    id: 'plum',
    name: '梅夜',
    desc: '深色 · 紫红',
    dark: true,
    bg: '#150F16',
    surface: '#1B141D',
    alt: '#221A24',
    sidebar: '#0C080D',
    sidebarText: '#AC9EB2',
    border: '#35293F',
    borderStrong: '#42354F',
    text: '#EFE4F2',
    text2: '#C2B2C9',
    muted: '#95849C',
    primary: '#C08ACB',
  },
  {
    id: 'pine',
    name: '松夜',
    desc: '深色 · 松林',
    dark: true,
    bg: '#0C1311',
    surface: '#111A17',
    alt: '#16211D',
    sidebar: '#070C0A',
    sidebarText: '#9AB0A7',
    border: '#26342F',
    borderStrong: '#303F39',
    text: '#E3EDE8',
    text2: '#B3C3BB',
    muted: '#83968D',
    primary: '#6FC79E',
  },
  {
    id: 'slate',
    name: '岩夜',
    desc: '深色 · 岩板',
    dark: true,
    bg: '#121619',
    surface: '#181E22',
    alt: '#1E262B',
    sidebar: '#0A0D0F',
    sidebarText: '#9EAAB2',
    border: '#29343A',
    borderStrong: '#38464E',
    text: '#E5EBEF',
    text2: '#B6C1C7',
    muted: '#88969E',
    primary: '#7FB8D8',
  },
  {
    id: 'aurora',
    name: '极光青',
    desc: '深色 · 青碧流光',
    dark: true,
    bg: '#0A1215',
    surface: '#101A1E',
    alt: '#152227',
    sidebar: '#060B0D',
    sidebarText: '#97B2B2',
    border: '#233538',
    borderStrong: '#2D4146',
    text: '#E2F0EE',
    text2: '#B1C6C2',
    muted: '#809A96',
    primary: '#38D2C0',
  },
]

const SEMANTIC_LIGHT = {
  success: '#2A7253',
  warning: '#8F6118',
  danger: '#A83A2C',
  purple: '#6A4A87',
}

const SEMANTIC_DARK = {
  success: '#7FB79A',
  warning: '#C9A063',
  danger: '#D0705F',
  purple: '#B49FD0',
}

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '')
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ]
}

export function relativeLum(hex: string): number {
  const [r, g, b] = hexToRgb(hex).map((v) => v / 255)
  const f = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

export function contrastRatio(a: string, b: string): number {
  const la = relativeLum(a)
  const lb = relativeLum(b)
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

/** 在不达标时朝黑/白方向逐步混色，返回第一个达标的颜色 */
function readableOn(color: string, bg: string, min = 4.5): string {
  if (contrastRatio(color, bg) >= min) return color
  const target = relativeLum(bg) < 0.35 ? '#FFFFFF' : '#000000'
  for (let t = 0.08; t <= 1; t += 0.08) {
    const next = mix(color, target, t)
    if (contrastRatio(next, bg) >= min) return next
  }
  return target
}

/** 基色 → 38 个设计令牌 */
export function buildColorVars(p: BasePalette): ColorVars {
  const sem = p.dark ? SEMANTIC_DARK : SEMANTIC_LIGHT
  const soft = (c: string) =>
    p.dark ? mix(c, '#000000', 0.82) : mix(c, p.surface, 0.88)
  const sidebarDark = relativeLum(p.sidebar) < 0.35
  const sidebarStep = (t: number) =>
    sidebarDark ? mix(p.sidebar, '#FFFFFF', t) : mix(p.sidebar, '#000000', t)
  // 选中行底色是侧栏与主色的混色，选中文案要按它而不是按侧栏底色校验
  const sidebarActive = mix(p.sidebar, p.primary, 0.16)

  return {
    primary: p.primary,
    'primary-dark': p.dark ? mix(p.primary, '#FFFFFF', 0.12) : mix(p.primary, '#000000', 0.14),
    'primary-soft': p.dark ? mix(p.primary, '#000000', 0.8) : mix(p.primary, p.surface, 0.88),

    success: sem.success,
    'success-soft': soft(sem.success),
    warning: sem.warning,
    'warning-soft': soft(sem.warning),
    danger: sem.danger,
    'danger-soft': soft(sem.danger),
    purple: sem.purple,
    'purple-soft': soft(sem.purple),

    bg: p.bg,
    surface: p.surface,
    'surface-alt': p.alt,

    border: p.border,
    'border-strong': p.borderStrong,
    'border-control': p.borderStrong,

    text: p.text,
    'text-2': p.text2,
    muted: p.muted,

    sidebar: p.sidebar,
    'sidebar-hover': sidebarStep(0.06),
    'sidebar-active': sidebarActive,
    'sidebar-text': p.sidebarText,
    'sidebar-icon': mix(p.sidebarText, p.sidebar, 0.3),
    'sidebar-active-text': readableOn(p.primary, sidebarActive, 5.5),

    'code-bg': p.dark ? mix(p.surface, '#000000', 0.3) : mix(p.surface, '#000000', 0.035),
    'gutter-bg': p.dark ? mix(p.surface, '#000000', 0.18) : mix(p.surface, '#000000', 0.06),
    'json-key': p.dark ? '#7FA8C9' : '#2B5D8C',
    'json-string': sem.success,
    'json-number': sem.warning,
    'json-bool': sem.purple,
    'json-punct': p.dark ? '#6F7A80' : '#8B959B',

    'shadow-sm': 'none',
    shadow: 'none',
    'shadow-lg': p.dark
      ? '0 18px 48px rgba(0, 0, 0, 0.5)'
      : '0 18px 48px rgba(20, 24, 27, 0.16)',

    brass: p.dark ? '#C9A063' : '#A97C3F',
    'brass-text': p.brassText ?? (p.dark ? '#C9A063' : '#7A5626'),
  }
}

export function findPalette(id: string): BasePalette {
  return PALETTES.find((item) => item.id === id) ?? PALETTES[0]
}

/** 供 App.vue / ReportDetail.vue 等老入口使用的两套基准色 */
export const paperColors: ColorVars = buildColorVars(findPalette('paper'))
export const nightColors: ColorVars = buildColorVars(findPalette('night'))
