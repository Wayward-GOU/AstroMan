import type { ThemeDefinition } from '@/stores/theme'
import { COLOR_KEYS } from './tokens'

/** 启动自检：每套主题必须包含 COLOR_KEYS 的全部键。只在 dev 生效。 */
export function assertThemesComplete(themes: ThemeDefinition[]) {
  if (!import.meta.env.DEV) return
  const required = COLOR_KEYS.map((k) => `--fm-${k}`)
  let fail = 0

  themes.forEach((t) => {
    const miss = required.filter((k) => !t.vars[k])
    if (miss.length) {
      fail++
      console.error(`[theme] ${t.id}(${t.name}) 缺少 ${miss.length} 个令牌: ${miss.join(', ')}`)
    }
  })

  if (fail === 0) console.info(`[theme] ${themes.length} 套主题令牌完整`)
}
