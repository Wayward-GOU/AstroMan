import { computed, ref } from 'vue'

export type ThemeId =
  | 'ocean'
  | 'graphite'
  | 'forest'
  | 'midnight'
  | 'amber'
  | 'teal'
  | 'rose'
  | 'dusk'

export interface ThemeDefinition {
  id: ThemeId
  name: string
  desc: string
  dark: boolean
  swatches: string[]
  vars: Record<string, string>
}

const ocean: ThemeDefinition = {
  id: 'ocean',
  name: '深海蓝',
  desc: '默认 · 冷静克制',
  dark: false,
  swatches: ['#f8fafc', '#f4f7fa', '#3d6b8f', '#0f172a'],
  vars: {
    '--fm-primary': '#3d6b8f',
    '--fm-primary-dark': '#305670',
    '--fm-primary-soft': '#edf3f7',
    '--fm-success': '#3f7f5f',
    '--fm-success-soft': '#edf5f0',
    '--fm-warning': '#b07d33',
    '--fm-warning-soft': '#faf4e9',
    '--fm-danger': '#b0524d',
    '--fm-danger-soft': '#f9efee',
    '--fm-purple': '#665a80',
    '--fm-purple-soft': '#f3f1f7',
    '--fm-bg': '#f8fafc',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#f1f5f9',
    '--fm-border': '#e2e8f0',
    '--fm-border-strong': '#cbd5e1',
    '--fm-text': '#0f172a',
    '--fm-text-2': '#334155',
    '--fm-muted': '#64748b',
    '--fm-sidebar': '#f4f7fa',
    '--fm-sidebar-hover': '#e9eff5',
    '--fm-sidebar-active': '#e3ebf2',
    '--fm-sidebar-text': '#64748b',
    '--fm-sidebar-icon': '#7b8899',
    '--fm-sidebar-active-text': '#3d6b8f',
    '--fm-code-bg': '#fbfcfe',
    '--fm-gutter-bg': '#f4f7fa',
    '--fm-json-key': '#1f4e79',
    '--fm-json-string': '#2f7d5b',
    '--fm-json-number': '#b26a1f',
    '--fm-json-bool': '#6a4c93',
    '--fm-json-punct': '#7b8794',
    '--fm-shadow-sm': '0 1px 2px rgba(15, 23, 42, 0.04)',
    '--fm-shadow': '0 1px 2px rgba(15, 23, 42, 0.04), 0 1px 3px rgba(15, 23, 42, 0.03)',
    '--fm-shadow-lg': '0 8px 20px rgba(15, 23, 42, 0.08)',
  },
}

const graphite: ThemeDefinition = {
  id: 'graphite',
  name: '石墨灰',
  desc: '中性 · 专注数据',
  dark: false,
  swatches: ['#f6f7f9', '#f2f4f6', '#56636f', '#141b22'],
  vars: {
    '--fm-primary': '#56636f',
    '--fm-primary-dark': '#46525d',
    '--fm-primary-soft': '#eef1f4',
    '--fm-success': '#4a7a63',
    '--fm-success-soft': '#edf4f0',
    '--fm-warning': '#a97a3c',
    '--fm-warning-soft': '#f8f3e9',
    '--fm-danger': '#a5564f',
    '--fm-danger-soft': '#f8efee',
    '--fm-purple': '#635f7d',
    '--fm-purple-soft': '#f1f0f5',
    '--fm-bg': '#f6f7f9',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#f1f3f5',
    '--fm-border': '#dfe3e7',
    '--fm-border-strong': '#c6ccd2',
    '--fm-text': '#141b22',
    '--fm-text-2': '#39434d',
    '--fm-muted': '#6b757f',
    '--fm-sidebar': '#f2f4f6',
    '--fm-sidebar-hover': '#e7ebee',
    '--fm-sidebar-active': '#dfe5e9',
    '--fm-sidebar-text': '#5f6a75',
    '--fm-sidebar-icon': '#78828d',
    '--fm-sidebar-active-text': '#3f4a55',
    '--fm-code-bg': '#fafbfc',
    '--fm-gutter-bg': '#f3f5f7',
    '--fm-json-key': '#3a4f63',
    '--fm-json-string': '#3f7059',
    '--fm-json-number': '#9c6a2f',
    '--fm-json-bool': '#5f5378',
    '--fm-json-punct': '#7b8289',
    '--fm-shadow-sm': '0 1px 2px rgba(20, 27, 34, 0.05)',
    '--fm-shadow': '0 1px 2px rgba(20, 27, 34, 0.05), 0 1px 3px rgba(20, 27, 34, 0.04)',
    '--fm-shadow-lg': '0 8px 20px rgba(20, 27, 34, 0.1)',
  },
}

const forest: ThemeDefinition = {
  id: 'forest',
  name: '森林绿',
  desc: '自然 · 低饱和',
  dark: false,
  swatches: ['#f7faf8', '#f0f6f2', '#3f6f5a', '#12211a'],
  vars: {
    '--fm-primary': '#3f6f5a',
    '--fm-primary-dark': '#325948',
    '--fm-primary-soft': '#edf4f0',
    '--fm-success': '#3f7f5f',
    '--fm-success-soft': '#edf5f0',
    '--fm-warning': '#b08033',
    '--fm-warning-soft': '#faf4e9',
    '--fm-danger': '#b0524d',
    '--fm-danger-soft': '#f9efee',
    '--fm-purple': '#615c7e',
    '--fm-purple-soft': '#f2f1f6',
    '--fm-bg': '#f7faf8',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#f0f6f2',
    '--fm-border': '#dfeae4',
    '--fm-border-strong': '#c4d6cc',
    '--fm-text': '#12211a',
    '--fm-text-2': '#334a3f',
    '--fm-muted': '#647a6e',
    '--fm-sidebar': '#f0f6f2',
    '--fm-sidebar-hover': '#e5efe9',
    '--fm-sidebar-active': '#dcebe3',
    '--fm-sidebar-text': '#5c7266',
    '--fm-sidebar-icon': '#77897e',
    '--fm-sidebar-active-text': '#3f6f5a',
    '--fm-code-bg': '#fbfdfc',
    '--fm-gutter-bg': '#f1f7f3',
    '--fm-json-key': '#2c5546',
    '--fm-json-string': '#397a58',
    '--fm-json-number': '#a9742c',
    '--fm-json-bool': '#5c5878',
    '--fm-json-punct': '#7a877f',
    '--fm-shadow-sm': '0 1px 2px rgba(18, 33, 26, 0.04)',
    '--fm-shadow': '0 1px 2px rgba(18, 33, 26, 0.04), 0 1px 3px rgba(18, 33, 26, 0.03)',
    '--fm-shadow-lg': '0 8px 20px rgba(18, 33, 26, 0.08)',
  },
}

const midnight: ThemeDefinition = {
  id: 'midnight',
  name: '暗夜',
  desc: '深色 · 夜间调试',
  dark: true,
  swatches: ['#0d1217', '#111821', '#8fb2cf', '#e6edf3'],
  vars: {
    '--fm-primary': '#7fa6c6',
    '--fm-primary-dark': '#a2c2da',
    '--fm-primary-soft': '#1c2a36',
    '--fm-success': '#6aa98a',
    '--fm-success-soft': '#182a22',
    '--fm-warning': '#c9a05f',
    '--fm-warning-soft': '#2b2418',
    '--fm-danger': '#cf8179',
    '--fm-danger-soft': '#2e1f1f',
    '--fm-purple': '#a396c8',
    '--fm-purple-soft': '#262134',
    '--fm-bg': '#0d1217',
    '--fm-surface': '#151b22',
    '--fm-surface-alt': '#1b232c',
    '--fm-border': '#28323d',
    '--fm-border-strong': '#3a4652',
    '--fm-text': '#e6edf3',
    '--fm-text-2': '#c2ccd6',
    '--fm-muted': '#8b98a5',
    '--fm-sidebar': '#111821',
    '--fm-sidebar-hover': '#1b242e',
    '--fm-sidebar-active': '#22303c',
    '--fm-sidebar-text': '#93a2b1',
    '--fm-sidebar-icon': '#7f8d9c',
    '--fm-sidebar-active-text': '#a9c6dd',
    '--fm-code-bg': '#0f151b',
    '--fm-gutter-bg': '#131a21',
    '--fm-json-key': '#8fb8d9',
    '--fm-json-string': '#86c8a1',
    '--fm-json-number': '#d8a466',
    '--fm-json-bool': '#b9a6dd',
    '--fm-json-punct': '#7c8895',
    '--fm-shadow-sm': '0 1px 2px rgba(0, 0, 0, 0.3)',
    '--fm-shadow': '0 1px 2px rgba(0, 0, 0, 0.3), 0 1px 3px rgba(0, 0, 0, 0.25)',
    '--fm-shadow-lg': '0 8px 20px rgba(0, 0, 0, 0.45)',
  },
}

const amber: ThemeDefinition = {
  id: 'amber',
  name: '琥珀橙',
  desc: '暖调 · 品牌同色',
  dark: false,
  swatches: ['#fdfbf7', '#f7f1e8', '#b4692a', '#22190f'],
  vars: {
    '--fm-primary': '#b4692a',
    '--fm-primary-dark': '#96531d',
    '--fm-primary-soft': '#fdf1e6',
    '--fm-success': '#4a7d55',
    '--fm-success-soft': '#eef5ef',
    '--fm-warning': '#a97a1f',
    '--fm-warning-soft': '#faf3e4',
    '--fm-danger': '#b0504a',
    '--fm-danger-soft': '#f9efee',
    '--fm-purple': '#6b5b83',
    '--fm-purple-soft': '#f4f1f7',
    '--fm-bg': '#fdfbf7',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#f7f1e8',
    '--fm-border': '#ece2d5',
    '--fm-border-strong': '#d9cbb8',
    '--fm-text': '#22190f',
    '--fm-text-2': '#4a3a29',
    '--fm-muted': '#7a6a58',
    '--fm-sidebar': '#f8f2e8',
    '--fm-sidebar-hover': '#f1e8da',
    '--fm-sidebar-active': '#ebdfcc',
    '--fm-sidebar-text': '#6d5c48',
    '--fm-sidebar-icon': '#8b7861',
    '--fm-sidebar-active-text': '#a85618',
    '--fm-code-bg': '#fdfbf7',
    '--fm-gutter-bg': '#f7f1e8',
    '--fm-json-key': '#7a3f16',
    '--fm-json-string': '#3f6d4a',
    '--fm-json-number': '#9c6218',
    '--fm-json-bool': '#6a4c7d',
    '--fm-json-punct': '#9a8b78',
    '--fm-shadow-sm': '0 1px 2px rgba(34, 25, 15, 0.05)',
    '--fm-shadow': '0 1px 2px rgba(34, 25, 15, 0.05), 0 1px 3px rgba(34, 25, 15, 0.04)',
    '--fm-shadow-lg': '0 8px 20px rgba(34, 25, 15, 0.1)',
  },
}

const teal: ThemeDefinition = {
  id: 'teal',
  name: '青碧',
  desc: '清爽 · 冷绿调',
  dark: false,
  swatches: ['#f6fbfa', '#edf6f5', '#2f7a75', '#0c1e1c'],
  vars: {
    '--fm-primary': '#2f7a75',
    '--fm-primary-dark': '#24635f',
    '--fm-primary-soft': '#e9f4f3',
    '--fm-success': '#3f7f5f',
    '--fm-success-soft': '#edf5f0',
    '--fm-warning': '#b07d33',
    '--fm-warning-soft': '#faf4e9',
    '--fm-danger': '#b0524d',
    '--fm-danger-soft': '#f9efee',
    '--fm-purple': '#615c80',
    '--fm-purple-soft': '#f2f1f7',
    '--fm-bg': '#f6fbfa',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#edf6f5',
    '--fm-border': '#d9e9e7',
    '--fm-border-strong': '#bcd6d3',
    '--fm-text': '#0c1e1c',
    '--fm-text-2': '#2f4744',
    '--fm-muted': '#5d7a77',
    '--fm-sidebar': '#eff7f6',
    '--fm-sidebar-hover': '#e4f1ef',
    '--fm-sidebar-active': '#d9ece9',
    '--fm-sidebar-text': '#547370',
    '--fm-sidebar-icon': '#6f8c89',
    '--fm-sidebar-active-text': '#2f7a75',
    '--fm-code-bg': '#fbfdfd',
    '--fm-gutter-bg': '#eff7f6',
    '--fm-json-key': '#1d5450',
    '--fm-json-string': '#2f7d5b',
    '--fm-json-number': '#b26a1f',
    '--fm-json-bool': '#5a4c80',
    '--fm-json-punct': '#77908d',
    '--fm-shadow-sm': '0 1px 2px rgba(12, 30, 28, 0.04)',
    '--fm-shadow': '0 1px 2px rgba(12, 30, 28, 0.04), 0 1px 3px rgba(12, 30, 28, 0.03)',
    '--fm-shadow-lg': '0 8px 20px rgba(12, 30, 28, 0.08)',
  },
}

const rose: ThemeDefinition = {
  id: 'rose',
  name: '玫瑰',
  desc: '柔和 · 暖粉调',
  dark: false,
  swatches: ['#fdf9fa', '#f9eff2', '#a3566b', '#241318'],
  vars: {
    '--fm-primary': '#a3566b',
    '--fm-primary-dark': '#8a4557',
    '--fm-primary-soft': '#fbeef1',
    '--fm-success': '#4a7d63',
    '--fm-success-soft': '#eef5f1',
    '--fm-warning': '#ab7b35',
    '--fm-warning-soft': '#faf4e9',
    '--fm-danger': '#b04a4a',
    '--fm-danger-soft': '#faeceb',
    '--fm-purple': '#6f5480',
    '--fm-purple-soft': '#f5f0f8',
    '--fm-bg': '#fdf9fa',
    '--fm-surface': '#ffffff',
    '--fm-surface-alt': '#f9eff2',
    '--fm-border': '#f0dde2',
    '--fm-border-strong': '#dcbfc8',
    '--fm-text': '#241318',
    '--fm-text-2': '#4c3038',
    '--fm-muted': '#7d5e67',
    '--fm-sidebar': '#fbf1f4',
    '--fm-sidebar-hover': '#f6e6eb',
    '--fm-sidebar-active': '#f1dbe2',
    '--fm-sidebar-text': '#775a63',
    '--fm-sidebar-icon': '#93757f',
    '--fm-sidebar-active-text': '#a3566b',
    '--fm-code-bg': '#fefcfd',
    '--fm-gutter-bg': '#fbf1f4',
    '--fm-json-key': '#7a3247',
    '--fm-json-string': '#3f7a5e',
    '--fm-json-number': '#a96b28',
    '--fm-json-bool': '#62497d',
    '--fm-json-punct': '#9a8188',
    '--fm-shadow-sm': '0 1px 2px rgba(36, 19, 24, 0.04)',
    '--fm-shadow': '0 1px 2px rgba(36, 19, 24, 0.04), 0 1px 3px rgba(36, 19, 24, 0.03)',
    '--fm-shadow-lg': '0 8px 20px rgba(36, 19, 24, 0.08)',
  },
}

const dusk: ThemeDefinition = {
  id: 'dusk',
  name: '暮色',
  desc: '深色 · 紫蓝夜空',
  dark: true,
  swatches: ['#0f1218', '#161b24', '#a3b8dd', '#e8edf5'],
  vars: {
    '--fm-primary': '#a3b8dd',
    '--fm-primary-dark': '#c3d2ef',
    '--fm-primary-soft': '#232a3a',
    '--fm-success': '#7fb79a',
    '--fm-success-soft': '#1a2a24',
    '--fm-warning': '#d3a866',
    '--fm-warning-soft': '#2b2519',
    '--fm-danger': '#d1847c',
    '--fm-danger-soft': '#2e2021',
    '--fm-purple': '#b8a6df',
    '--fm-purple-soft': '#262239',
    '--fm-bg': '#0f1218',
    '--fm-surface': '#161b24',
    '--fm-surface-alt': '#1d232e',
    '--fm-border': '#2b3340',
    '--fm-border-strong': '#3d4655',
    '--fm-text': '#e8edf5',
    '--fm-text-2': '#c3cad6',
    '--fm-muted': '#8b93a3',
    '--fm-sidebar': '#131821',
    '--fm-sidebar-hover': '#1d2632',
    '--fm-sidebar-active': '#253040',
    '--fm-sidebar-text': '#96a0b0',
    '--fm-sidebar-icon': '#828c9c',
    '--fm-sidebar-active-text': '#c3d2ef',
    '--fm-code-bg': '#10141b',
    '--fm-gutter-bg': '#141a22',
    '--fm-json-key': '#93b3d8',
    '--fm-json-string': '#8fc9a6',
    '--fm-json-number': '#d9ad72',
    '--fm-json-bool': '#bfa9e6',
    '--fm-json-punct': '#7f8794',
    '--fm-shadow-sm': '0 1px 2px rgba(0, 0, 0, 0.32)',
    '--fm-shadow': '0 1px 2px rgba(0, 0, 0, 0.32), 0 1px 3px rgba(0, 0, 0, 0.26)',
    '--fm-shadow-lg': '0 8px 20px rgba(0, 0, 0, 0.46)',
  },
}

export const themes: ThemeDefinition[] = [
  ocean,
  graphite,
  forest,
  midnight,
  amber,
  teal,
  rose,
  dusk,
]

const STORAGE_KEY = 'astroman.theme'
const LEGACY_STORAGE_KEYS = ['fbinhttp.theme', 'finkmeter.theme']

export function findTheme(id: string | null | undefined): ThemeDefinition {
  return themes.find((item) => item.id === id) ?? ocean
}

export function applyTheme(theme: ThemeDefinition) {
  const root = document.documentElement
  Object.entries(theme.vars).forEach(([key, value]) => root.style.setProperty(key, value))
  root.dataset.theme = theme.id
  root.style.colorScheme = theme.dark ? 'dark' : 'light'
}

export function loadThemeId(): ThemeId {
  try {
    const stored =
      localStorage.getItem(STORAGE_KEY) ??
      LEGACY_STORAGE_KEYS.map((key) => localStorage.getItem(key)).find((value) => value)
    if (stored && themes.some((item) => item.id === stored)) return stored as ThemeId
  } catch {
    // localStorage 不可用时回退默认主题
  }
  return 'ocean'
}

export function saveThemeId(id: ThemeId) {
  try {
    localStorage.setItem(STORAGE_KEY, id)
    LEGACY_STORAGE_KEYS.forEach((key) => localStorage.removeItem(key))
  } catch {
    // 忽略存储失败
  }
}

/** 全应用共享的当前主题，修改后所有使用方同步更新 */
export const currentThemeId = ref<ThemeId>(loadThemeId())
export const currentTheme = computed<ThemeDefinition>(() => findTheme(currentThemeId.value))

/** 启动时把已保存的主题写入 CSS 变量，避免首屏闪烁 */
export function initTheme() {
  applyTheme(currentTheme.value)
}

export function setTheme(id: ThemeId) {
  currentThemeId.value = id
  applyTheme(findTheme(id))
  saveThemeId(id)
}
