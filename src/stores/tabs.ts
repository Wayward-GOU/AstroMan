import { defineStore } from 'pinia'

export interface RequestTab {
  id: number
  name: string
  method: string
}

interface ProjectTabState {
  tabs: RequestTab[]
  activeId: number | null
}

const STORAGE_KEY = 'fm-request-tabs'
export const MAX_REQUEST_TABS = 20

function readAll(): Record<string, ProjectTabState> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, ProjectTabState>
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

export const useTabsStore = defineStore('tabs', {
  state: () => ({
    projectId: 0,
    tabs: [] as RequestTab[],
    activeId: null as number | null,
  }),
  getters: {
    full: (state) => state.tabs.length >= MAX_REQUEST_TABS,
  },
  actions: {
    ensureProject(projectId: number) {
      if (this.projectId === projectId) return
      this.projectId = projectId
      const saved = readAll()[String(projectId)]
      this.tabs = Array.isArray(saved?.tabs) ? saved.tabs.slice(0, MAX_REQUEST_TABS) : []
      this.activeId = saved?.activeId ?? null
    },
    persist() {
      const all = readAll()
      all[String(this.projectId)] = { tabs: this.tabs, activeId: this.activeId }
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(all))
      } catch {
        // 存储空间不足时忽略，标签仍保留在内存中
      }
    },
    canOpen(id: number) {
      return this.tabs.some((tab) => tab.id === id) || this.tabs.length < MAX_REQUEST_TABS
    },
    open(tab: RequestTab) {
      const existing = this.tabs.find((item) => item.id === tab.id)
      if (existing) {
        existing.name = tab.name
        existing.method = tab.method
        this.activeId = tab.id
        this.persist()
        return true
      }
      if (this.tabs.length >= MAX_REQUEST_TABS) return false
      this.tabs.push({ ...tab })
      this.activeId = tab.id
      this.persist()
      return true
    },
    touch(id: number, name: string, method: string) {
      const tab = this.tabs.find((item) => item.id === id)
      if (!tab) return
      if (tab.name === name && tab.method === method) return
      tab.name = name
      tab.method = method
      this.persist()
    },
    setActive(id: number | null) {
      if (this.activeId === id) return
      this.activeId = id
      this.persist()
    },
    /** 关闭标签，返回关闭后应激活的标签 id（没有则为 null） */
    close(id: number): number | null {
      const index = this.tabs.findIndex((item) => item.id === id)
      if (index < 0) return this.activeId
      this.tabs.splice(index, 1)
      if (this.activeId === id) {
        this.activeId = this.tabs[index]?.id ?? this.tabs[index - 1]?.id ?? null
      }
      this.persist()
      return this.activeId
    },
    closeOthers(id: number) {
      this.tabs = this.tabs.filter((item) => item.id === id)
      this.activeId = this.tabs[0]?.id ?? null
      this.persist()
    },
    closeAll() {
      this.tabs = []
      this.activeId = null
      this.persist()
    },
  },
})
