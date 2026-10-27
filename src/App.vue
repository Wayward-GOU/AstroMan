<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { theme as AntTheme } from 'ant-design-vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  AppstoreOutlined,
  FundProjectionScreenOutlined,
  HistoryOutlined,
  SettingOutlined,
  SafetyCertificateOutlined,
  LineChartOutlined,
  ToolOutlined,
  ThunderboltOutlined,
  PlayCircleOutlined,
  RobotOutlined,
  SearchOutlined,
  EnterOutlined,
} from '@ant-design/icons-vue'
import AppLogo from '@/components/AppLogo.vue'
import { currentTheme, currentThemeId, setTheme, themes } from '@/stores/theme'
import { isMac, shortcutText } from '@/utils/platform'
import { checkForUpdates, isAutoUpdateEnabled } from '@/utils/updater'

const route = useRoute()
const router = useRouter()

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
const navs = [
  { key: 'home', label: '工作台', hint: '项目与请求调试', to: '/', icon: AppstoreOutlined },
  { key: 'stress', label: '压测', hint: '选择接口发起压测', to: '/stress', icon: ThunderboltOutlined },
  { key: 'runner', label: '批量运行', hint: '集合 Runner · 数据驱动', to: '/runner', icon: PlayCircleOutlined },
  { key: 'ai', label: '小HI', hint: 'AI 助手 · 自动建请求并分析', to: '/ai', icon: RobotOutlined },
  { key: 'reports', label: '压测报告', hint: '历史压测结果', to: '/reports', icon: FundProjectionScreenOutlined },
  { key: 'history', label: '请求历史', hint: '最近发送记录', to: '/history', icon: HistoryOutlined },
  { key: 'monitor', label: '系统监视', hint: 'CPU · 内存 · 磁盘 · 网络', to: '/monitor', icon: LineChartOutlined },
  { key: 'tools', label: '工具箱', hint: 'JSON · 加解密 · 时间戳', to: '/tools', icon: ToolOutlined },
  { key: 'settings', label: '设置', hint: '应用与数据', to: '/settings', icon: SettingOutlined },
]

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
  const stored = Number(localStorage.getItem(ZOOM_KEY) ?? '1')
  applyZoom(Number.isFinite(stored) && stored > 0 ? stored : 1, false)
  window.addEventListener('keydown', onZoomHotkey)
  window.addEventListener('keydown', onGlobalHotkey)

  // 启动后静默检查一次 GitHub 最新版本，有更新才提示
  if (isAutoUpdateEnabled()) {
    window.setTimeout(() => {
      checkForUpdates({ silent: true })
    }, 5000)
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onZoomHotkey)
  window.removeEventListener('keydown', onGlobalHotkey)
  if (noticeTimer) window.clearTimeout(noticeTimer)
})

const activeKey = computed(() => {
  const path = route.path
  if (path.startsWith('/stress')) return 'stress'
  if (path.startsWith('/runner')) return 'runner'
  if (path.startsWith('/ai')) return 'ai'
  if (path.startsWith('/reports')) return 'reports'
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
    <div class="shell">
      <aside class="sider">
        <div class="brand">
          <AppLogo :size="38" />
          <div class="brand-text">
            <span class="brand-name">FbinHttp</span>
            <span class="brand-sub">API 调试 · 压测</span>
          </div>
        </div>

        <nav class="nav">
          <div class="nav-label">导航</div>
          <router-link
            v-for="item in navs"
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
              <span class="nav-hint">{{ item.hint }}</span>
            </span>
            <span class="nav-indicator" />
          </router-link>
        </nav>

        <div class="zoom-box">
          <a-tooltip :title="`放大 · ${shortcutText('+')}`" placement="right">
            <button class="zoom-btn" type="button" @click="zoomIn">+</button>
          </a-tooltip>
          <a-tooltip :title="`恢复 100% · ${shortcutText('0')}`" placement="right">
            <button
              class="zoom-value"
              :class="{ active: zoom !== 1 }"
              type="button"
              @click="resetZoom"
            >
              {{ Math.round(zoom * 100) }}%
            </button>
          </a-tooltip>
          <a-tooltip :title="`缩小 · ${shortcutText('-')}`" placement="right">
            <button class="zoom-btn" type="button" @click="zoomOut">−</button>
          </a-tooltip>
        </div>

        <div class="sider-foot">
          <div class="local-card">
            <SafetyCertificateOutlined class="local-icon" />
            <div>
              <div class="local-title">纯本地运行</div>
              <div class="local-desc">数据只保存在这台电脑</div>
            </div>
          </div>
          <div class="version">FbinHttp v1.0 · Desktop</div>
        </div>
      </aside>

      <main class="main" :class="{ 'main-dock': !route.path.startsWith('/ai') }">
        <router-view />
      </main>

      <transition name="zoom-fade">
        <div v-if="zoomNotice" class="zoom-notice">{{ Math.round(zoom * 100) }}%</div>
      </transition>

      <transition name="zoom-fade">
        <button
          v-if="!route.path.startsWith('/ai')"
          class="hi-float"
          type="button"
          aria-label="打开小HI"
          @click="go('/ai')"
        >
          <span class="hi-float-halo" />
          <svg class="hi-bot" viewBox="0 0 64 64" aria-hidden="true">
            <defs>
              <linearGradient id="hiShell" x1="0" y1="0" x2="0.4" y2="1">
                <stop offset="0" stop-color="#ffffff" />
                <stop offset="1" stop-color="#dce6f2" />
              </linearGradient>
              <linearGradient id="hiFace" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stop-color="#2c3646" />
                <stop offset="1" stop-color="#111722" />
              </linearGradient>
            </defs>

            <path d="M32 9 L32 14" stroke="#a8b6c8" stroke-width="1.8" stroke-linecap="round" />
            <circle cx="32" cy="6.4" r="2.4" class="hi-antenna" />

            <rect x="8.5" y="27" width="4" height="9" rx="2" fill="#cbd8e6" />
            <rect x="51.5" y="27" width="4" height="9" rx="2" fill="#cbd8e6" />

            <rect x="13.5" y="14.5" width="39" height="32" rx="10" fill="#c2cfdf" />
            <rect
              x="11.5"
              y="12.5"
              width="41"
              height="32"
              rx="10"
              fill="url(#hiShell)"
              stroke="#c9d6e5"
              stroke-width="1"
            />
            <rect x="16.5" y="17.5" width="32" height="22" rx="6.5" fill="url(#hiFace)" />

            <g class="hi-eyes">
              <path d="M25.5 24.8 L29 28.5 L25.5 32.2 L22 28.5 Z" fill="#7ee7ff" />
              <path d="M38.5 24.8 L42 28.5 L38.5 32.2 L35 28.5 Z" fill="#7ee7ff" />
            </g>

            <rect x="19" y="47" width="26" height="9" rx="4.5" fill="#c2cfdf" />
            <rect
              x="18"
              y="45.5"
              width="26"
              height="9"
              rx="4.5"
              fill="url(#hiShell)"
              stroke="#c9d6e5"
              stroke-width="1"
            />

            <circle cx="15.5" cy="50" r="3.2" fill="#dbe5f1" stroke="#c9d6e5" stroke-width="1" />
            <circle cx="48.5" cy="50" r="3.2" fill="#dbe5f1" stroke="#c9d6e5" stroke-width="1" />
          </svg>
          <span class="hi-float-tip">小HI · AI 助手</span>
          <span class="hi-float-dot" />
        </button>
      </transition>

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
  width: 64px;
  flex-shrink: 0;
  background: var(--fm-sidebar);
  color: var(--fm-sidebar-text);
  display: flex;
  flex-direction: column;
  padding: 14px 8px 12px;
  border-right: 1px solid var(--fm-border);
  transition: background 0.2s ease, border-color 0.2s ease;
}

.brand {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 2px 0 16px;
  user-select: none;
}

.brand-text {
  display: none;
}

.brand-name {
  color: var(--fm-text);
  font-weight: 600;
  font-size: 14px;
  letter-spacing: 0.2px;
}

.brand-sub {
  font-size: 10px;
  color: var(--fm-muted);
  letter-spacing: 0.3px;
}

.nav {
  flex: 1;
}

.nav-label {
  display: none;
}

.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 9px 0;
  margin-bottom: 4px;
  border-radius: 6px;
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
  left: -8px;
  top: 8px;
  bottom: 8px;
  width: 2px;
  border-radius: 2px;
  background: var(--fm-primary);
}

.nav-icon {
  width: 32px;
  height: 30px;
  border-radius: 5px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
  background: transparent;
  color: var(--fm-sidebar-icon);
}

.nav-item.active .nav-icon {
  background: transparent;
  color: var(--fm-primary);
}

.nav-text {
  display: none;
}

.nav-name {
  font-size: 13px;
  font-weight: 500;
}

.nav-hint {
  font-size: 10px;
  color: #64748b;
}

.nav-item.active .nav-hint {
  color: #94a3b8;
}

.nav-indicator {
  display: none;
}

.zoom-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  padding: 8px 0 6px;
}

.zoom-btn {
  width: 34px;
  height: 24px;
  border-radius: 6px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  color: var(--fm-sidebar-text);
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
  transition: color 0.15s ease, border-color 0.15s ease, background 0.15s ease;
}

.zoom-btn:hover {
  color: var(--fm-primary);
  border-color: var(--fm-border-strong);
}

.zoom-btn:active {
  transform: scale(0.96);
}

.zoom-value {
  width: 44px;
  height: 20px;
  border: none;
  background: transparent;
  color: var(--fm-muted);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  padding: 0;
}

.zoom-value.active {
  color: var(--fm-primary);
  font-weight: 600;
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

/* 全局悬浮入口：小HI 小机器人，贴右下角，尽量不压内容 */
.hi-float {
  position: fixed;
  right: 14px;
  bottom: 14px;
  z-index: 900;
  width: 54px;
  height: 54px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0.78;
  transition: transform 0.22s cubic-bezier(0.2, 0.8, 0.3, 1.2), opacity 0.2s ease;
}

.hi-float:hover,
.hi-float:focus-visible {
  opacity: 1;
}

.hi-float::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 34%, #ffffff 0%, #eff5fc 60%, #e1ebf7 100%);
  border: 1px solid rgba(148, 170, 196, 0.34);
  box-shadow: 0 10px 24px rgba(52, 84, 120, 0.22), 0 2px 6px rgba(52, 84, 120, 0.12);
  transition: box-shadow 0.22s ease;
}

.hi-float-halo {
  position: absolute;
  inset: -11px;
  border-radius: 50%;
  background: radial-gradient(
    circle,
    color-mix(in srgb, var(--fm-primary) 46%, transparent) 0%,
    transparent 68%
  );
  filter: blur(3px);
  animation: hi-halo 4.2s ease-in-out infinite;
  pointer-events: none;
}

.hi-bot {
  position: relative;
  width: 40px;
  height: 40px;
  animation: hi-bob 4.2s ease-in-out infinite;
  will-change: transform;
}

.hi-eyes {
  transform-box: fill-box;
  transform-origin: center;
  animation: hi-blink 5.4s steps(1) infinite;
}

.hi-eyes path {
  filter: drop-shadow(0 0 2.5px rgba(126, 231, 255, 0.85));
}

.hi-antenna {
  fill: #38d9a9;
  animation: hi-antenna 2.8s ease-in-out infinite;
}

.hi-float:hover {
  transform: translateY(-2px) scale(1.06);
}

.hi-float:hover::before {
  box-shadow: 0 14px 30px rgba(52, 84, 120, 0.28), 0 3px 8px rgba(52, 84, 120, 0.14);
}

.hi-float:active {
  transform: scale(0.96);
}

.hi-float:hover .hi-bot,
.hi-float:hover .hi-float-halo {
  animation-play-state: paused;
}

.hi-float-tip {
  position: absolute;
  right: 64px;
  top: 50%;
  padding: 5px 10px;
  border-radius: 8px;
  background: rgba(17, 23, 34, 0.92);
  color: #fff;
  font-size: 12px;
  white-space: nowrap;
  opacity: 0;
  transform: translateY(-50%) translateX(6px);
  transition: opacity 0.18s ease, transform 0.18s ease;
  pointer-events: none;
}

.hi-float:hover .hi-float-tip {
  opacity: 1;
  transform: translateY(-50%) translateX(0);
}

.hi-float-dot {
  position: absolute;
  right: 2px;
  bottom: 4px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #38d9a9;
  border: 2px solid #fff;
  animation: hi-dot 2.6s ease-out infinite;
}

@keyframes hi-bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-3.5px);
  }
}

@keyframes hi-halo {
  0%,
  100% {
    opacity: 0.36;
    transform: scale(0.96);
  }
  50% {
    opacity: 0.68;
    transform: scale(1.04);
  }
}

@keyframes hi-blink {
  0%,
  92%,
  100% {
    transform: scaleY(1);
  }
  94%,
  97% {
    transform: scaleY(0.12);
  }
}

@keyframes hi-antenna {
  0%,
  100% {
    fill: #38d9a9;
    opacity: 1;
  }
  50% {
    fill: #5ce8bd;
    opacity: 0.55;
  }
}

@keyframes hi-dot {
  0% {
    box-shadow: 0 0 0 0 rgba(56, 217, 169, 0.55);
  }
  70% {
    box-shadow: 0 0 0 7px rgba(56, 217, 169, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(56, 217, 169, 0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .hi-bot,
  .hi-float-halo,
  .hi-eyes,
  .hi-antenna,
  .hi-float-dot {
    animation: none;
  }
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
  padding-top: 8px;
  display: flex;
  justify-content: center;
}

.local-card {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 8px;
  border-radius: 6px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
}

.local-icon {
  color: var(--fm-success);
  font-size: 14px;
}

.local-title,
.local-desc,
.version {
  display: none;
}

.main {
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: auto;
  background: var(--fm-bg);
}

/* 为右下角悬浮机器人预留空间，保证内容能滚动到按钮上方 */
.main-dock {
  padding-bottom: 62px;
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
