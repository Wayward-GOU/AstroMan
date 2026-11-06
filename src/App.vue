<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { theme as AntTheme } from 'ant-design-vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  ApiOutlined,
  AppstoreOutlined,
  ExperimentOutlined,
  FolderOpenOutlined,
  HistoryOutlined,
  SettingOutlined,
  SafetyCertificateOutlined,
  LineChartOutlined,
  ToolOutlined,
  ThunderboltOutlined,
  PlayCircleOutlined,
  SearchOutlined,
  EnterOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  ArrowLeftOutlined,
  ScheduleOutlined,
} from '@ant-design/icons-vue'
import AppLogo from '@/components/AppLogo.vue'
import AstroBuddy from '@/components/AstroBuddy.vue'
import AstroBuddyIcon from '@/components/AstroBuddyIcon.vue'
import AiAssistDrawer from '@/components/AiAssistDrawer.vue'
import ErrorDialog from '@/components/ErrorDialog.vue'
import NotesReminderHost from '@/components/notes/NotesReminderHost.vue'
import HangzaiTaskDock from '@/components/HangzaiTaskDock.vue'
import { currentTheme, currentThemeId, setTheme, themes } from '@/stores/theme'
import { canGoBack, previousEntry, stepBack } from '@/stores/navHistory'
import { isMac, shortcutText } from '@/utils/platform'
import { checkForUpdates, isAutoUpdateEnabled } from '@/utils/updater'
import { useNotesStore } from '@/stores/notes'
import { useHangzaiTaskStore } from '@/stores/hangzaiTask'

const route = useRoute()
const router = useRouter()
const notesStore = useNotesStore()
const hangzaiTask = useHangzaiTaskStore()
const isBare = computed(() => Boolean(route.meta.bare))

let notesTicker: number | undefined
let unlistenNotesChanged: UnlistenFn | null = null
let unlistenNotesOpen: UnlistenFn | null = null

/* ---------- 全局返回 ---------- */
const backKeyText = isMac ? '⌥ ←' : 'Alt + ←'

function goBackPage() {
  const target = stepBack()
  if (target) router.push(target)
}

const ZOOM_KEY = 'fm-zoom'
const ZOOM_MIN = 0.5
const ZOOM_MAX = 2
const ZOOM_STEP = 0.1

const zoom = ref(1)
const zoomNotice = ref(false)
let noticeTimer: number | undefined

function normalizeZoom(value: number) {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(value * 100) / 100))
}

function showZoomNotice() {
  zoomNotice.value = true
  if (noticeTimer) window.clearTimeout(noticeTimer)
  noticeTimer = window.setTimeout(() => {
    zoomNotice.value = false
  }, 900)
}

async function applyZoom(value: number, persist = true) {
  const next = normalizeZoom(value)
  zoom.value = next
  try {
    await getCurrentWebview().setZoom(next)
  } catch {
    // 浏览器预览环境不支持 WebView 缩放，忽略即可
  }
  if (persist) {
    localStorage.setItem(ZOOM_KEY, String(next))
    showZoomNotice()
  }
}

function zoomIn() {
  applyZoom(zoom.value + ZOOM_STEP)
}

function zoomOut() {
  applyZoom(zoom.value - ZOOM_STEP)
}

function resetZoom() {
  applyZoom(1)
}

function onZoomHotkey(event: KeyboardEvent) {
  if (isBare.value) return
  if (event.altKey) return
  const modifier = isMac ? event.metaKey : event.ctrlKey
  if (!modifier) return
  if (event.key === '=' || event.key === '+') {
    event.preventDefault()
    zoomIn()
  } else if (event.key === '-' || event.key === '_') {
    event.preventDefault()
    zoomOut()
  } else if (event.key === '0') {
    event.preventDefault()
    resetZoom()
  }
}

/* ---------- 侧边栏导航 ---------- */
// 必须声明在命令面板的 computed / watch 之前，否则 watch 首次求值会触发 TDZ
interface NavItem {
  key: string
  label: string
  hint: string
  to: string
  icon: Component
  group: string
}

const navs: NavItem[] = [
  { key: 'home', label: '工作台', hint: '概览与快速入口', to: '/', icon: AppstoreOutlined, group: '调试' },
  { key: 'projects', label: '项目管理', hint: '项目 · 集合 · 环境', to: '/projects', icon: FolderOpenOutlined, group: '调试' },
  { key: 'stress', label: '压测', hint: '选择接口发起压测', to: '/stress', icon: ThunderboltOutlined, group: '测试' },
  { key: 'capture', label: '抓包', hint: 'HTTP/HTTPS 录制 · 改包 · 重发', to: '/capture', icon: ApiOutlined, group: '测试' },
  { key: 'runner', label: '批量运行', hint: '集合 Runner · 数据驱动', to: '/runner', icon: PlayCircleOutlined, group: '测试' },
  { key: 'automation', label: '自动化测试', hint: '套件 · 断言 · Python · 报告', to: '/automation', icon: ExperimentOutlined, group: '测试' },
  { key: 'notes', label: '记事本', hint: '待办 · 提醒 · 截图生成', to: '/notes', icon: ScheduleOutlined, group: '智能' },
  { key: 'ai', label: '航仔', hint: 'AI 助手 · 自动建请求并分析', to: '/ai', icon: AstroBuddyIcon, group: '智能' },
  { key: 'history', label: '历史记录', hint: '数据概览 · 最近发送记录', to: '/history', icon: HistoryOutlined, group: '智能' },
  { key: 'monitor', label: '系统监视', hint: 'CPU · 内存 · 磁盘 · 网络', to: '/monitor', icon: LineChartOutlined, group: '系统' },
  { key: 'tools', label: '工具箱', hint: 'JSON · 加解密 · 时间戳', to: '/tools', icon: ToolOutlined, group: '系统' },
  { key: 'settings', label: '设置', hint: '应用与数据', to: '/settings', icon: SettingOutlined, group: '系统' },
]

const navGroups = computed(() => {
  const groups: { name: string; items: NavItem[] }[] = []
  navs.forEach((item) => {
    let bucket = groups.find((entry) => entry.name === item.group)
    if (!bucket) {
      bucket = { name: item.group, items: [] }
      groups.push(bucket)
    }
    bucket.items.push(item)
  })
  return groups
})

/* ---------- 侧边栏折叠 ---------- */
const SIDEBAR_KEY = 'astroman:sidebar-collapsed'
const NARROW_BREAKPOINT = 1180

const sidebarPinned = ref(false)
try {
  sidebarPinned.value = localStorage.getItem(SIDEBAR_KEY) === '1'
} catch {
  // localStorage 不可用时使用默认展开态
}
const windowNarrow = ref(false)
const sidebarCollapsed = computed(() => windowNarrow.value || sidebarPinned.value)

function syncSidebarWidth() {
  windowNarrow.value = window.innerWidth < NARROW_BREAKPOINT
}

function toggleSidebar() {
  if (windowNarrow.value) {
    // 窄窗口下强制展开会把内容挤没，先让窗口变宽再展开
    windowNarrow.value = false
    sidebarPinned.value = false
    try {
      localStorage.setItem(SIDEBAR_KEY, '0')
    } catch {
      // 忽略存储失败
    }
    return
  }
  sidebarPinned.value = !sidebarPinned.value
  try {
    localStorage.setItem(SIDEBAR_KEY, sidebarPinned.value ? '1' : '0')
  } catch {
    // 忽略存储失败
  }
}

/* ---------- 命令面板 ---------- */
interface PaletteItem {
  key: string
  label: string
  hint: string
  group: string
  run: () => void
}

const paletteOpen = ref(false)
const paletteQuery = ref('')
const paletteIndex = ref(0)
const paletteInput = ref<{ focus: () => void } | null>(null)

function go(path: string) {
  router.push(path)
}

function cycleTheme() {
  const index = themes.findIndex((item) => item.id === currentThemeId.value)
  const next = themes[(index + 1) % themes.length]
  setTheme(next.id)
}

const paletteCommands = computed<PaletteItem[]>(() => [
  ...navs.map((item) => ({
    key: `nav-${item.key}`,
    label: item.label,
    hint: item.hint,
    group: '页面',
    run: () => go(item.to),
  })),
  ...(canGoBack.value
    ? [
        {
          key: 'nav-back',
          label: '返回上一页',
          hint: previousEntry.value ? `回到 ${previousEntry.value.label}` : '回到上一次停留的页面',
          group: '页面',
          run: goBackPage,
        },
      ]
    : []),
  {
    key: 'theme',
    label: '切换主题',
    hint: `当前：${currentTheme.value.name}`,
    group: '外观',
    run: cycleTheme,
  },
  { key: 'zoom-in', label: '放大界面', hint: shortcutText('+'), group: '外观', run: zoomIn },
  { key: 'zoom-out', label: '缩小界面', hint: shortcutText('-'), group: '外观', run: zoomOut },
  { key: 'zoom-reset', label: '恢复 100%', hint: shortcutText('0'), group: '外观', run: resetZoom },
  {
    key: 'update',
    label: '检查更新',
    hint: '从 GitHub 获取最新版本',
    group: '应用',
    run: () => {
      checkForUpdates({ silent: false })
    },
  },
])

const paletteFiltered = computed(() => {
  const query = paletteQuery.value.trim().toLowerCase()
  if (!query) return paletteCommands.value
  return paletteCommands.value.filter(
    (item) => item.label.toLowerCase().includes(query) || item.hint.toLowerCase().includes(query),
  )
})

watch(paletteFiltered, () => {
  paletteIndex.value = 0
})

function openPalette() {
  paletteQuery.value = ''
  paletteIndex.value = 0
  paletteOpen.value = true
  nextTick(() => paletteInput.value?.focus())
}

function runPalette(index: number) {
  const item = paletteFiltered.value[index]
  if (!item) return
  paletteOpen.value = false
  item.run()
}

function onPaletteKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    if (!paletteFiltered.value.length) return
    paletteIndex.value = (paletteIndex.value + 1) % paletteFiltered.value.length
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    if (!paletteFiltered.value.length) return
    paletteIndex.value =
      (paletteIndex.value - 1 + paletteFiltered.value.length) % paletteFiltered.value.length
  } else if (event.key === 'Enter') {
    event.preventDefault()
    runPalette(paletteIndex.value)
  }
}

function onGlobalHotkey(event: KeyboardEvent) {
  if (isBare.value) return
  // Alt + ← 返回上一次停留的页面（桌面软件通用习惯）
  if (event.altKey && !event.ctrlKey && !event.metaKey && event.key === 'ArrowLeft') {
    if (canGoBack.value) {
      event.preventDefault()
      goBackPage()
    }
    return
  }
  const modifier = isMac ? event.metaKey : event.ctrlKey
  if (!modifier || event.altKey) return
  const key = event.key.toLowerCase()
  if (key === 'k') {
    event.preventDefault()
    if (paletteOpen.value) {
      paletteOpen.value = false
    } else {
      openPalette()
    }
    return
  }
  // Ctrl / ⌘ + 1..8 直达对应页面
  if (/^[1-9]$/.test(key)) {
    const target = navs[Number(key) - 1]
    if (!target) return
    event.preventDefault()
    go(target.to)
  }
}

function groupPalette(items: PaletteItem[]) {
  const groups: { group: string; items: { item: PaletteItem; index: number }[] }[] = []
  items.forEach((item, index) => {
    let bucket = groups.find((entry) => entry.group === item.group)
    if (!bucket) {
      bucket = { group: item.group, items: [] }
      groups.push(bucket)
    }
    bucket.items.push({ item, index })
  })
  return groups
}

onMounted(() => {
  if (isBare.value) return
  const stored = Number(localStorage.getItem(ZOOM_KEY) ?? '1')
  applyZoom(Number.isFinite(stored) && stored > 0 ? stored : 1, false)
  syncSidebarWidth()
  window.addEventListener('resize', syncSidebarWidth)
  window.addEventListener('keydown', onZoomHotkey)
  window.addEventListener('keydown', onGlobalHotkey)

  // 记事本角标：启动加载一次，之后每分钟与窗口重新可见时刷新
  void notesStore.loadSettings()
  void notesStore.refreshCounts()
  notesTicker = window.setInterval(() => {
    notesStore.touchNow()
    void notesStore.refreshCounts()
  }, 60_000)
  document.addEventListener('visibilitychange', onNotesVisible)
  void (async () => {
    try {
      unlistenNotesChanged = await listen<{ reason?: string }>('notes:changed', (event) => {
        void notesStore.refreshCounts()
        if (event.payload?.reason === 'settings') void notesStore.loadSettings()
      })
    } catch {
      // 浏览器预览没有 Tauri 事件，跳过即可
    }
    try {
      unlistenNotesOpen = await listen('notes:open', () => {
        go('/notes')
      })
    } catch {
      // 同上
    }
  })()

  // 启动后静默检查一次 GitHub 最新版本，有更新才提示
  if (isAutoUpdateEnabled()) {
    window.setTimeout(() => {
      checkForUpdates({ silent: true })
    }, 5000)
  }
})

function onNotesVisible() {
  if (document.visibilityState !== 'visible') return
  notesStore.touchNow()
  void notesStore.refreshCounts()
}

onBeforeUnmount(() => {
  if (notesTicker) window.clearInterval(notesTicker)
  document.removeEventListener('visibilitychange', onNotesVisible)
  unlistenNotesChanged?.()
  unlistenNotesOpen?.()
  window.removeEventListener('resize', syncSidebarWidth)
  window.removeEventListener('keydown', onZoomHotkey)
  window.removeEventListener('keydown', onGlobalHotkey)
  if (noticeTimer) window.clearTimeout(noticeTimer)
})

const activeKey = computed(() => {
  const path = route.path
  if (path.startsWith('/stress')) return 'stress'
  if (path.startsWith('/capture')) return 'capture'
  if (path.startsWith('/runner')) return 'runner'
  if (path.startsWith('/projects')) return 'projects'
  if (path.startsWith('/project/')) return 'projects'
  if (path.startsWith('/automation')) return 'automation'
  if (path.startsWith('/notes')) return 'notes'
  if (path.startsWith('/ai')) return 'ai'
  if (path.startsWith('/history')) return 'history'
  if (path.startsWith('/monitor')) return 'monitor'
  if (path.startsWith('/tools')) return 'tools'
  if (path === '/settings') return 'settings'
  return 'home'
})

const theme = computed(() => {
  const controlHeight = isMac ? 32 : 34
  return {
    algorithm: AntTheme[currentTheme.value.dark ? 'darkAlgorithm' : 'defaultAlgorithm'],
    token: {
      colorPrimary: currentTheme.value.vars['--fm-primary'],
      colorInfo: currentTheme.value.vars['--fm-primary'],
      colorSuccess: currentTheme.value.vars['--fm-success'],
      colorWarning: currentTheme.value.vars['--fm-warning'],
      colorError: currentTheme.value.vars['--fm-danger'],
      colorText: currentTheme.value.vars['--fm-text'],
      colorTextSecondary: currentTheme.value.vars['--fm-text-2'],
      colorBorder: currentTheme.value.vars['--fm-border'],
      colorBgLayout: currentTheme.value.vars['--fm-bg'],
      colorBgContainer: currentTheme.value.vars['--fm-surface'],
      colorBgElevated: currentTheme.value.vars['--fm-surface'],
      borderRadius: isMac ? 7 : 6,
      borderRadiusLG: isMac ? 11 : 10,
      fontSize: 13,
      fontFamily: isMac
        ? "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'PingFang SC', 'Hiragino Sans GB', sans-serif"
        : "'Segoe UI Variable Text', 'Segoe UI', 'Microsoft YaHei', 'PingFang SC', sans-serif",
    },
    components: {
      Button: { controlHeight, fontWeight: 500 },
      Input: { controlHeight },
      Select: { controlHeight },
      Table: {
        headerBg: currentTheme.value.vars['--fm-surface-alt'],
        headerColor: currentTheme.value.vars['--fm-muted'],
      },
    },
  }
})
</script>

<template>
  <a-config-provider :theme="theme">
    <router-view v-if="isBare" />
    <div v-else class="shell">
      <aside class="sider" :class="{ collapsed: sidebarCollapsed }">
        <div class="brand">
          <AppLogo :size="34" />
          <div class="brand-text">
            <span class="brand-name">AstroMan</span>
            <span class="brand-sub">API 调试 · 压测</span>
          </div>
        </div>

        <a-tooltip
          :title="
            canGoBack
              ? `返回「${previousEntry?.label ?? '上一页'}」 · ${backKeyText}`
              : '还没有可返回的页面'
          "
          placement="right"
        >
          <button
            class="nav-back"
            :class="{ disabled: !canGoBack }"
            type="button"
            :aria-disabled="!canGoBack"
            @click="canGoBack && goBackPage()"
          >
            <span class="nav-back-badge">
              <ArrowLeftOutlined class="nav-back-icon" />
            </span>
            <span class="nav-back-text">
              <span class="nav-back-label">返回</span>
              <span class="nav-back-target">{{ previousEntry?.label }}</span>
            </span>
          </button>
        </a-tooltip>

        <nav class="nav">
          <div v-for="group in navGroups" :key="group.name" class="nav-section">
            <div class="nav-label">{{ group.name }}</div>
            <router-link
              v-for="item in group.items"
              :key="item.key"
              :to="item.to"
              class="nav-item"
              :class="{ active: activeKey === item.key }"
              :title="`${item.label} · ${item.hint}`"
            >
              <span class="nav-icon">
                <component :is="item.icon" />
              </span>
              <span class="nav-text">
                <span class="nav-name">{{ item.label }}</span>
              </span>
              <span v-if="item.key === 'notes' && notesStore.badge" class="nav-badge">
                {{ notesStore.badge }}
              </span>
              <span
                v-if="item.key === 'ai' && hangzaiTask.active"
                class="nav-live"
                :title="hangzaiTask.phaseText"
              />
              <span class="nav-indicator" />
            </router-link>
          </div>
        </nav>

        <div class="zoom-box">
          <a-tooltip :title="`缩小 · ${shortcutText('-')}`" placement="top">
            <button class="zoom-btn" type="button" @click="zoomOut">−</button>
          </a-tooltip>
          <a-tooltip :title="`恢复 100% · ${shortcutText('0')}`" placement="top">
            <button
              class="zoom-value"
              :class="{ active: zoom !== 1 }"
              type="button"
              @click="resetZoom"
            >
              {{ Math.round(zoom * 100) }}%
            </button>
          </a-tooltip>
          <a-tooltip :title="`放大 · ${shortcutText('+')}`" placement="top">
            <button class="zoom-btn" type="button" @click="zoomIn">+</button>
          </a-tooltip>
        </div>

        <div class="sider-foot">
          <a-tooltip title="展开侧边栏" placement="right" :disabled="!sidebarCollapsed">
            <button class="sider-toggle" type="button" @click="toggleSidebar">
              <MenuUnfoldOutlined v-if="sidebarCollapsed" />
              <MenuFoldOutlined v-else />
              <span v-if="!sidebarCollapsed" class="toggle-label">收起</span>
            </button>
          </a-tooltip>
          <div class="local-card">
            <SafetyCertificateOutlined class="local-icon" />
            <div>
              <div class="local-title">纯本地运行</div>
              <div class="local-desc">数据只保存在这台电脑</div>
            </div>
          </div>
          <div class="version">AstroMan v1.0 · Desktop</div>
        </div>
      </aside>

      <main class="main">
        <router-view v-slot="{ Component }">
          <keep-alive :include="['AiAssistant']">
            <component :is="Component" />
          </keep-alive>
        </router-view>
      </main>

      <transition name="zoom-fade">
        <div v-if="zoomNotice" class="zoom-notice">{{ Math.round(zoom * 100) }}%</div>
      </transition>

      <AstroBuddy v-if="!route.path.startsWith('/ai')" @open="go('/ai')" />
      <HangzaiTaskDock v-if="!route.path.startsWith('/ai')" @open="go('/ai')" />

      <a-modal
        v-model:open="paletteOpen"
        :footer="null"
        :closable="false"
        :width="560"
        wrap-class-name="palette-wrap"
      >
        <div class="palette">
          <div class="palette-search">
            <SearchOutlined class="palette-icon" />
            <input
              ref="paletteInput"
              v-model="paletteQuery"
              class="palette-input"
              placeholder="搜索页面或操作…"
              @keydown="onPaletteKeydown"
            />
            <span class="palette-kbd">{{ shortcutText('K') }}</span>
          </div>
          <div class="palette-list">
            <template v-for="group in groupPalette(paletteFiltered)" :key="group.group">
              <div class="palette-group">{{ group.group }}</div>
              <button
                v-for="entry in group.items"
                :key="entry.item.key"
                type="button"
                class="palette-item"
                :class="{ active: entry.index === paletteIndex }"
                @mouseenter="paletteIndex = entry.index"
                @click="runPalette(entry.index)"
              >
                <span class="palette-label">{{ entry.item.label }}</span>
                <span class="palette-hint">{{ entry.item.hint }}</span>
                <EnterOutlined v-if="entry.index === paletteIndex" class="palette-enter" />
              </button>
            </template>
            <div v-if="!paletteFiltered.length" class="palette-empty">没有匹配的命令</div>
          </div>
        </div>
      </a-modal>

      <NotesReminderHost />
      <AiAssistDrawer />
      <ErrorDialog />
    </div>
  </a-config-provider>
</template>

<style scoped>
.shell {
  display: flex;
  height: 100%;
  background: var(--fm-bg);
}

.sider {
  width: 186px;
  flex-shrink: 0;
  background: var(--fm-sidebar);
  color: var(--fm-sidebar-text);
  display: flex;
  flex-direction: column;
  padding: 14px 10px 12px;
  border-right: 1px solid var(--fm-border);
  transition: width 0.18s ease, padding 0.18s ease, background 0.2s ease, border-color 0.2s ease;
  overflow: hidden;
}

.sider.collapsed {
  width: 64px;
  padding: 14px 8px 12px;
}

.brand {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 2px 6px 16px;
  user-select: none;
}

.sider.collapsed .brand {
  justify-content: center;
  padding: 2px 0 14px;
}

.sider.collapsed .brand-text {
  display: none;
}

.brand-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.brand-name {
  color: var(--fm-text);
  font-weight: 700;
  font-size: 14.5px;
  letter-spacing: 0.2px;
  background: linear-gradient(120deg, var(--fm-text) 30%, var(--fm-primary) 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
}

.brand-sub {
  font-size: 10px;
  color: var(--fm-muted);
  letter-spacing: 0.4px;
  margin-top: 1px;
}

.nav {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}

/* ---------- 全局返回 ---------- */
.nav-back {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 34px;
  margin: 0 0 14px;
  padding: 0 10px 0 7px;
  border: 1px solid color-mix(in srgb, var(--fm-primary) 26%, transparent);
  border-radius: 10px;
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--fm-primary) 14%, var(--fm-surface)),
    color-mix(in srgb, var(--fm-primary) 6%, var(--fm-surface))
  );
  color: var(--fm-text);
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
  box-shadow: 0 1px 2px color-mix(in srgb, var(--fm-primary) 12%, transparent);
  transition: background 0.16s ease, border-color 0.16s ease, color 0.16s ease,
    box-shadow 0.16s ease;
}

/* 实心主色箭头块：侧栏里唯一的实心色块，一眼能看到 */
.nav-back-badge {
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  flex: none;
  border-radius: 7px;
  color: #fff;
  background: linear-gradient(135deg, var(--fm-primary), var(--fm-primary-dark));
  box-shadow: 0 1px 3px color-mix(in srgb, var(--fm-primary) 42%, transparent);
  transition: transform 0.16s ease, box-shadow 0.16s ease;
}

.nav-back:hover:not(.disabled) {
  border-color: color-mix(in srgb, var(--fm-primary) 55%, transparent);
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--fm-primary) 21%, var(--fm-surface)),
    color-mix(in srgb, var(--fm-primary) 11%, var(--fm-surface))
  );
  box-shadow: 0 2px 6px color-mix(in srgb, var(--fm-primary) 18%, transparent);
}

.nav-back:hover:not(.disabled) .nav-back-badge {
  transform: translateX(-2px);
  box-shadow: 0 2px 6px color-mix(in srgb, var(--fm-primary) 55%, transparent);
}

.nav-back:active:not(.disabled) {
  transform: translateY(1px);
}

.nav-back:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--fm-primary) 45%, transparent);
  outline-offset: 1px;
}

.nav-back.disabled {
  opacity: 0.62;
  background: transparent;
  border-style: dashed;
  box-shadow: none;
  cursor: default;
}

.nav-back.disabled .nav-back-badge {
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  box-shadow: none;
}

.nav-back-icon {
  font-size: 11px;
  flex-shrink: 0;
}

.nav-back-text {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  overflow: hidden;
}

.nav-back-label {
  flex-shrink: 0;
  font-weight: 600;
  color: var(--fm-primary);
}

.nav-back-target {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--fm-muted);
}

.nav-back:hover:not(.disabled) .nav-back-target {
  color: color-mix(in srgb, var(--fm-primary) 78%, var(--fm-muted));
}

.sider.collapsed .nav-back {
  width: 34px;
  height: 34px;
  margin: 0 auto 12px;
  padding: 0;
  justify-content: center;
  gap: 0;
}

.sider.collapsed .nav-back-badge {
  width: 22px;
  height: 22px;
  border-radius: 8px;
}

.sider.collapsed .nav-back-text {
  display: none;
}

.sider.collapsed .nav-back-icon {
  font-size: 12px;
}

.nav-section {
  margin-bottom: 10px;
}

.nav-label {
  padding: 0 8px 5px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--fm-muted);
  opacity: 0.75;
}

.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 6px 8px;
  margin-bottom: 2px;
  border-radius: 8px;
  color: var(--fm-sidebar-text);
  text-decoration: none;
  transition: background 0.16s, color 0.16s;
}

.nav-item:hover {
  background: var(--fm-sidebar-hover);
  color: var(--fm-text-2);
}

.nav-item.active {
  background: var(--fm-sidebar-active);
  color: var(--fm-sidebar-active-text);
}

.nav-item.active::before {
  content: '';
  position: absolute;
  left: -10px;
  top: 7px;
  bottom: 7px;
  width: 3px;
  border-radius: 3px;
  background: linear-gradient(180deg, var(--fm-primary), color-mix(in srgb, var(--fm-primary) 55%, #ffffff));
}

.nav-icon {
  width: 27px;
  height: 27px;
  flex-shrink: 0;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: var(--fm-sidebar-text);
  background: transparent;
  transition: color 0.16s ease, background 0.16s ease;
}

.nav-item:hover .nav-icon {
  color: var(--fm-text);
}

.nav-item.active .nav-icon {
  color: var(--fm-primary);
  background: color-mix(in srgb, var(--fm-primary) 9%, transparent);
}

.nav-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.nav-name {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-item.active .nav-name {
  font-weight: 600;
}

.nav-hint {
  font-size: 10px;
  color: #64748b;
}

.nav-badge {
  margin-left: auto;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--fm-danger);
  color: #fff;
  font-size: 9px;
  line-height: 16px;
  text-align: center;
  flex-shrink: 0;
}

.sider.collapsed .nav-badge {
  display: none;
}

.nav-live {
  margin-left: auto;
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--fm-primary);
  animation: nav-live-pulse 1.4s ease-in-out infinite;
}

.sider.collapsed .nav-live {
  position: absolute;
  top: 6px;
  right: 6px;
  margin-left: 0;
}

@keyframes nav-live-pulse {
  0%,
  100% {
    opacity: 0.4;
    transform: scale(0.85);
  }
  50% {
    opacity: 1;
    transform: scale(1.1);
  }
}

.nav-item.active .nav-hint {
  color: #94a3b8;
}

.nav-indicator {
  display: none;
}

/* ---------- 折叠态 ---------- */
.sider.collapsed .nav-label {
  display: none;
}

.sider.collapsed .nav-section {
  margin-bottom: 4px;
}

.sider.collapsed .nav-item {
  justify-content: center;
  gap: 0;
  padding: 5px 0;
  margin-bottom: 3px;
}

.sider.collapsed .nav-item.active::before {
  left: -8px;
}

.sider.collapsed .nav-text {
  display: none;
}

.sider.collapsed .nav-icon {
  width: 30px;
  height: 28px;
  font-size: 15px;
}

/* ---------- 缩放控制：一条横向胶囊 ---------- */
.zoom-box {
  display: flex;
  align-items: center;
  gap: 2px;
  margin: 2px 2px 8px;
  padding: 3px;
  border: 1px solid var(--fm-border);
  border-radius: 9px;
  background: var(--fm-surface);
}

.zoom-btn {
  flex: 1;
  height: 22px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fm-sidebar-text);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  transition: color 0.15s ease, background 0.15s ease;
}

.zoom-btn:hover {
  color: var(--fm-primary);
  background: var(--fm-surface-alt);
}

.zoom-btn:active {
  transform: scale(0.94);
}

.zoom-value {
  flex: 1.5;
  height: 22px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fm-muted);
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  padding: 0;
  transition: color 0.15s ease, background 0.15s ease;
}

.zoom-value:hover {
  background: var(--fm-surface-alt);
}

.zoom-value.active {
  color: var(--fm-primary);
  font-weight: 600;
}

.sider.collapsed .zoom-box {
  justify-content: center;
  margin: 2px 0 6px;
  padding: 2px;
}

.sider.collapsed .zoom-box .zoom-btn {
  display: none;
}

.sider.collapsed .zoom-value {
  flex: 1;
}

.sider.collapsed .local-card,
.sider.collapsed .version {
  display: none;
}

/* ---------- 收起 / 展开按钮 ---------- */
.sider-toggle {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  height: 28px;
  padding: 0 8px;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--fm-sidebar-text);
  font-size: 11.5px;
  cursor: pointer;
  transition: color 0.15s ease, background 0.15s ease;
}

.sider-toggle:hover {
  background: var(--fm-sidebar-hover);
  color: var(--fm-primary);
}

.sider.collapsed .sider-toggle {
  justify-content: center;
  width: 30px;
  padding: 0;
  margin: 0 auto;
}

.zoom-notice {
  position: fixed;
  right: 26px;
  bottom: 100px;
  z-index: 2000;
  padding: 8px 16px;
  border-radius: 10px;
  background: rgba(15, 23, 42, 0.88);
  color: #fff;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.4px;
  font-variant-numeric: tabular-nums;
  pointer-events: none;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.28);
}

.zoom-fade-enter-active,
.zoom-fade-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.zoom-fade-enter-from,
.zoom-fade-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

.sider-foot {
  flex-shrink: 0;
  padding-top: 8px;
  border-top: 1px solid var(--fm-border);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.local-card {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  border-radius: 8px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  overflow: hidden;
}

.sider.collapsed .local-card {
  justify-content: center;
  padding: 8px;
}

.sider.collapsed .local-title,
.sider.collapsed .local-desc,
.sider.collapsed .version {
  display: none;
}

.local-icon {
  color: var(--fm-success);
  font-size: 15px;
  flex-shrink: 0;
}

.local-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  line-height: 1.3;
}

.local-desc {
  font-size: 10px;
  color: var(--fm-muted);
  line-height: 1.3;
}

.version {
  font-size: 10px;
  color: var(--fm-muted);
  text-align: center;
  letter-spacing: 0.3px;
}

.main {
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: auto;
  background: var(--fm-bg);
}

/* ---------- 命令面板 ---------- */
.palette {
  margin: -8px -8px 0;
}

.palette-search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--fm-border);
}

.palette-icon {
  color: var(--fm-muted);
  font-size: 14px;
}

.palette-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--fm-text);
  font-size: 14px;
}

.palette-kbd {
  padding: 2px 6px;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  font-size: 10px;
  color: var(--fm-muted);
}

.palette-list {
  max-height: 380px;
  overflow: auto;
  padding: 6px;
}

.palette-group {
  padding: 8px 10px 4px;
  font-size: 10px;
  letter-spacing: 0.4px;
  color: var(--fm-muted);
}

.palette-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fm-text-2);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.palette-item.active {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.palette-label {
  flex-shrink: 0;
  font-weight: 500;
}

.palette-hint {
  flex: 1;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 11px;
  color: var(--fm-muted);
}

.palette-enter {
  font-size: 11px;
  color: var(--fm-primary);
}

.palette-empty {
  padding: 22px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
