<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { theme as AntTheme } from 'ant-design-vue'
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

const route = useRoute()

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

const theme = computed(() => ({
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
    borderRadius: 6,
    borderRadiusLG: 10,
    fontSize: 13,
    fontFamily:
      "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', sans-serif",
  },
  components: {
    Button: { controlHeight: 34, fontWeight: 500 },
    Input: { controlHeight: 34 },
    Select: { controlHeight: 34 },
    Table: {
      headerBg: currentTheme.value.vars['--fm-surface-alt'],
      headerColor: currentTheme.value.vars['--fm-muted'],
    },
  },
}))
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
