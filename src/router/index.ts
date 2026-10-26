import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'dashboard', component: () => import('@/views/Dashboard.vue') },
    { path: '/project/:id', name: 'project', component: () => import('@/views/Project.vue') },
    { path: '/request/:id', name: 'request', component: () => import('@/views/RequestEditor.vue') },
    { path: '/environment/:projectId', name: 'environment', component: () => import('@/views/Environment.vue') },
    { path: '/stress', name: 'stress', component: () => import('@/views/StressTest.vue') },
    { path: '/reports', name: 'reports', component: () => import('@/views/Reports.vue') },
    { path: '/history', name: 'history', component: () => import('@/views/History.vue') },
    { path: '/reports/:id', name: 'report-detail', component: () => import('@/views/ReportDetail.vue') },
    { path: '/tools', name: 'tools', component: () => import('@/views/Tools.vue') },
    { path: '/monitor', name: 'monitor', component: () => import('@/views/SystemMonitor.vue') },
    { path: '/settings', name: 'settings', component: () => import('@/views/Settings.vue') },
  ],
})

export default router
