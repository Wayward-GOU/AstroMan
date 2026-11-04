import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { notesCounts, notesSettingsGet, notesSettingsSet } from '@/api/notes'
import type { NoteCounts, NotesSettings } from '@/types/notes'

const DEFAULT_SETTINGS: NotesSettings = {
  notifyEnabled: true,
  quietHoursEnabled: false,
  quietStart: '22:00',
  quietEnd: '08:00',
  pauseUntil: 0,
  globalShortcut: true,
  visionProviderId: null,
}

export const useNotesStore = defineStore('notes', () => {
  const counts = ref<NoteCounts>({ open: 0, dueToday: 0, overdue: 0, done: 0 })
  const settings = ref<NotesSettings>({ ...DEFAULT_SETTINGS })
  const loaded = ref(false)
  const now = ref(Date.now())

  /** 侧边栏角标：逾期 + 今天到期，超过 99 显示 99+ */
  const badge = computed(() => {
    const value = counts.value.overdue + counts.value.dueToday
    if (value <= 0) return ''
    return value > 99 ? '99+' : String(value)
  })

  const paused = computed(() => settings.value.pauseUntil > now.value)

  const quietText = computed(() =>
    settings.value.quietHoursEnabled
      ? `${settings.value.quietStart}–${settings.value.quietEnd}`
      : '',
  )

  function touchNow() {
    now.value = Date.now()
  }

  async function refreshCounts() {
    try {
      counts.value = await notesCounts()
      loaded.value = true
    } catch {
      // 浏览器预览或后端未就绪时保持默认值
    }
  }

  async function loadSettings() {
    try {
      settings.value = await notesSettingsGet()
    } catch {
      settings.value = { ...DEFAULT_SETTINGS }
    }
    touchNow()
  }

  async function saveSettings(patch: Partial<NotesSettings>) {
    const next = { ...settings.value, ...patch }
    settings.value = await notesSettingsSet(next)
    touchNow()
    return settings.value
  }

  return {
    counts,
    settings,
    loaded,
    now,
    badge,
    paused,
    quietText,
    touchNow,
    refreshCounts,
    loadSettings,
    saveSettings,
  }
})
