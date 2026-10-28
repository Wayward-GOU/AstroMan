import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'dashboard', component: () => import('@/views/Dashboard.vue') },
    { path: '/project/:id', name: 'project', component: () => import('@/views/Project.vue') },
    { path: '/request/:id', name: 'request', component: () => import('@/views/RequestEditor.vue') },
    { path: '/environment/:projectId', name: 'environment', component: () => import('@/views/Environment.vue') },
    { path: '/stress', name: 'stress', component: () => import('@/views/StressTest.vue') },
    { path: '/runner', name: 'runner', component: () => import('@/views/Runner.vue') },
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

export default router
