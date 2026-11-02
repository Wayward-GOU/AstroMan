import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { TOOL_MAP } from '@/tools/registry'

const FAVORITE_KEY = 'astroman.tool.favorites'
const RECENT_KEY = 'astroman.tool.recent'
const MAX_RECENT = 10

function readIds(key: string): string[] {
  if (typeof localStorage === 'undefined') return []
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed.filter((item): item is string => typeof item === 'string' && TOOL_MAP.has(item))
  } catch {
    return []
  }
}

function writeIds(key: string, ids: string[]) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(key, JSON.stringify(ids))
  } catch {
    /* 存储被禁用时静默降级为会话内状态 */
  }
}

export const useToolPrefsStore = defineStore('toolPrefs', () => {
  const favoriteIds = ref<string[]>(readIds(FAVORITE_KEY))
  const recentIds = ref<string[]>(readIds(RECENT_KEY))

  const favorites = computed(() =>
    favoriteIds.value.map((id) => TOOL_MAP.get(id)).filter((tool): tool is NonNullable<typeof tool> => Boolean(tool)),
  )
  const recent = computed(() =>
    recentIds.value.map((id) => TOOL_MAP.get(id)).filter((tool): tool is NonNullable<typeof tool> => Boolean(tool)),
  )

  function isFavorite(id: string): boolean {
    return favoriteIds.value.includes(id)
  }

  function toggleFavorite(id: string) {
    if (!TOOL_MAP.has(id)) return
    favoriteIds.value = isFavorite(id)
      ? favoriteIds.value.filter((item) => item !== id)
      : [id, ...favoriteIds.value].slice(0, 20)
    writeIds(FAVORITE_KEY, favoriteIds.value)
  }

  function markRecent(id: string) {
    if (!TOOL_MAP.has(id)) return
    recentIds.value = [id, ...recentIds.value.filter((item) => item !== id)].slice(0, MAX_RECENT)
    writeIds(RECENT_KEY, recentIds.value)
  }

  function clearRecent() {
    recentIds.value = []
    writeIds(RECENT_KEY, [])
  }

  return { favoriteIds, recentIds, favorites, recent, isFavorite, toggleFavorite, markRecent, clearRecent }
})
