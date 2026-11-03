import { createRouter, createWebHashHistory } from 'vue-router'
import { recordVisit } from '@/stores/navHistory'

const LAST_ROUTE_KEY = 'astroman:last-route'
// 只在上次停留页恢复一次，避免 catch-all 重定向后反复跳转
let routeRestored = false

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'dashboard', component: () => import('@/views/Dashboard.vue') },
    { path: '/projects', name: 'projects', component: () => import('@/views/Projects.vue') },
    { path: '/project/:id', name: 'project', component: () => import('@/views/Project.vue') },
    { path: '/request/:id', name: 'request', component: () => import('@/views/RequestEditor.vue') },
    { path: '/environment/:projectId', name: 'environment', component: () => import('@/views/Environment.vue') },
    { path: '/stress', name: 'stress', component: () => import('@/views/StressTest.vue') },
    { path: '/capture', name: 'capture', component: () => import('@/views/Capture.vue') },
    { path: '/runner', name: 'runner', component: () => import('@/views/Runner.vue') },
    { path: '/automation', name: 'automation', component: () => import('@/views/Automation.vue') },
    { path: '/reports', name: 'reports', component: () => import('@/views/Reports.vue') },
    { path: '/history', name: 'history', component: () => import('@/views/History.vue') },
    { path: '/ai', name: 'ai', component: () => import('@/views/AiAssistant.vue') },
    { path: '/ai/logs', name: 'ai-logs', component: () => import('@/views/AiToolLogs.vue') },
    { path: '/ai/usage', name: 'ai-usage', component: () => import('@/views/AiUsage.vue') },
    { path: '/reports/:id', name: 'report-detail', component: () => import('@/views/ReportDetail.vue') },
    { path: '/tools', name: 'tools', component: () => import('@/views/Tools.vue') },
    { path: '/monitor', name: 'monitor', component: () => import('@/views/SystemMonitor.vue') },
    { path: '/settings', name: 'settings', component: () => import('@/views/Settings.vue') },
    // 未知路由（旧链接、手改 hash）统一回工作台，避免空白页
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

router.beforeEach((to) => {
  if (routeRestored) return true
  routeRestored = true
  // 直接带 hash 进入时以用户输入为准；只有从默认首页启动才恢复上次页面
  if (to.path !== '/') return true
  try {
    const saved = localStorage.getItem(LAST_ROUTE_KEY)
    if (saved && saved.startsWith('/') && !saved.startsWith('//') && saved !== '/') {
      return saved
    }
  } catch {
    // localStorage 不可用时按默认首页打开
  }
  return true
})

router.afterEach((to) => {
  if (!to.matched.length) return
  // 维护「上一次停留页面」栈，供全局返回按钮使用
  recordVisit(to)
  try {
    localStorage.setItem(LAST_ROUTE_KEY, to.fullPath)
  } catch {
    // 忽略存储失败
  }
})

export default router
