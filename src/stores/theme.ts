import { computed, ref } from 'vue'
import { PALETTES, buildColorVars, type ThemeId } from '@/design/themes'
import { toCssVars } from '@/design/tokens'

export type { ThemeId }

export interface ThemeDefinition {
  id: ThemeId
  name: string
  desc: string
  dark: boolean
  /** 设置页缩略图取色：[画布, 卡片, 侧栏, 主色, 正文, 发丝线] */
  swatches: string[]
  vars: Record<string, string>
}

/**
 * 23 套主题全部由 src/design/themes.ts 的基色表派生，
 * 每套都会真正改变画布、卡片、侧栏、描边与文字，不再只是换主色。
 */
export const themes: ThemeDefinition[] = PALETTES.map((palette) => ({
  id: palette.id,
  name: palette.name,
  desc: palette.desc,
  dark: palette.dark,
  swatches: [
    palette.bg,
    palette.surface,
    palette.sidebar,
    palette.primary,
    palette.text,
    palette.border,
  ],
  vars: toCssVars(buildColorVars(palette)),
}))

const STORAGE_KEY = 'astroman.theme'
const LEGACY_STORAGE_KEYS = ['fbinhttp.theme', 'finkmeter.theme']

export function findTheme(id: string | null | undefined): ThemeDefinition {
  return themes.find((item) => item.id === id) ?? themes[0]
}

export function applyTheme(theme: ThemeDefinition) {
  const root = document.documentElement
  Object.entries(theme.vars).forEach(([key, value]) => root.style.setProperty(key, value))
  root.dataset.theme = theme.id
  // 明暗标记与主题 id 解耦，供局部深色作用域与方法色使用
  root.dataset.palette = theme.dark ? 'night' : 'paper'
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
  return 'paper'
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
