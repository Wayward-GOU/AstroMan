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

applyPlatformAttributes()
initTheme()

const app = createApp(App)
const pinia = createPinia()

// 兜底异常：留下可复现的日志，并把完整错误弹给用户，避免只闪一条提示或直接白屏
let errorTipShown = false
app.config.errorHandler = (err, _instance, info) => {
  console.error('[AstroMan] 界面异常', info, err)
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
})

app.use(pinia)
app.use(router)
app.use(Antd)
app.mount('#app')
