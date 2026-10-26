<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { theme as AntTheme } from 'ant-design-vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  AppstoreOutlined,
  FundProjectionScreenOutlined,
  SettingOutlined,
  SafetyCertificateOutlined,
  LineChartOutlined,
  ToolOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import AppLogo from '@/components/AppLogo.vue'
import { currentTheme } from '@/stores/theme'
import { isMac, shortcutText } from '@/utils/platform'

const route = useRoute()

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

onMounted(() => {
  const stored = Number(localStorage.getItem(ZOOM_KEY) ?? '1')
  applyZoom(Number.isFinite(stored) && stored > 0 ? stored : 1, false)
  window.addEventListener('keydown', onZoomHotkey)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onZoomHotkey)
  if (noticeTimer) window.clearTimeout(noticeTimer)
})

const navs = [
  { key: 'home', label: '工作台', hint: '项目与请求调试', to: '/', icon: AppstoreOutlined },
  { key: 'stress', label: '压测', hint: '选择接口发起压测', to: '/stress', icon: ThunderboltOutlined },
  { key: 'reports', label: '压测报告', hint: '历史压测结果', to: '/reports', icon: FundProjectionScreenOutlined },
  { key: 'monitor', label: '系统监视', hint: 'CPU · 内存 · 磁盘 · 网络', to: '/monitor', icon: LineChartOutlined },
  { key: 'tools', label: '工具箱', hint: 'JSON · 加解密 · 时间戳', to: '/tools', icon: ToolOutlined },
  { key: 'settings', label: '设置', hint: '应用与数据', to: '/settings', icon: SettingOutlined },
]

const activeKey = computed(() => {
  const path = route.path
  if (path.startsWith('/stress')) return 'stress'
  if (path.startsWith('/reports')) return 'reports'
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
            <span class="brand-name">FinkMeter</span>
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
          <div class="version">FinkMeter v1.0 · Desktop</div>
        </div>
      </aside>

      <main class="main">
        <router-view />
      </main>

      <transition name="zoom-fade">
        <div v-if="zoomNotice" class="zoom-notice">{{ Math.round(zoom * 100) }}%</div>
      </transition>
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
  right: 24px;
  bottom: 24px;
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
</style>
