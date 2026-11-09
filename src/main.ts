import { createApp } from 'vue'
import { createPinia } from 'pinia'
import Antd, { message } from 'ant-design-vue'
import 'ant-design-vue/dist/reset.css'
import './styles/main.css'
import App from './App.vue'
import router from './router'
import { initTheme } from './stores/theme'
import { useErrorDialogStore } from './stores/errorDialog'
import { applyPlatformAttributes } from './utils/platform'
import * as api from './api'

applyPlatformAttributes()
initTheme()

// 前端异常转后端日志：同类只报一次、总量封顶，避免异常风暴写爆日志盘
const reportedErrors = new Set<string>()
function reportFrontendError(level: 'error' | 'warn', error: unknown, context?: string) {
  const text = error instanceof Error ? error.stack || error.message : String(error ?? '')
  const key = `${level}:${text.slice(0, 200)}`
  if (!text.trim() || reportedErrors.has(key) || reportedErrors.size >= 50) return
  reportedErrors.add(key)
  void api.frontendLog(level, text, context).catch(() => {
    // 日志通道本身失败时不再打扰用户
  })
}

const app = createApp(App)
const pinia = createPinia()

// 兜底异常：留下可复现的日志，并把完整错误弹给用户，避免只闪一条提示或直接白屏
let errorTipShown = false
app.config.errorHandler = (err, _instance, info) => {
  console.error('[AstroMan] 界面异常', info, err)
  reportFrontendError('error', err, `触发位置=${info}; 页面=${window.location.hash || '/'}`)
  try {
    useErrorDialogStore(pinia).show({
      title: '界面出现异常',
      error: err,
      context: [
        { label: '触发位置', value: info },
        { label: '当前页面', value: window.location.hash || '/' },
      ],
    })
  } catch {
    // store 尚未就绪时退回一次性提示
    if (!errorTipShown) {
      errorTipShown = true
      message.error('界面出现异常，请重试；若持续出现可重启应用')
    }
  }
}
window.addEventListener('unhandledrejection', (event) => {
  console.error('[AstroMan] 未处理的 Promise 异常', event.reason)
  reportFrontendError('warn', event.reason, `未处理的 Promise; 页面=${window.location.hash || '/'}`)
})
window.addEventListener('error', (event) => {
  if (!event.error) return
  reportFrontendError('error', event.error, `全局 error; 页面=${window.location.hash || '/'}`)
})

app.use(pinia)
app.use(router)
app.use(Antd)
app.mount('#app')
