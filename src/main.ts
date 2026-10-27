import { createApp } from 'vue'
import { createPinia } from 'pinia'
import Antd, { message } from 'ant-design-vue'
import 'ant-design-vue/dist/reset.css'
import './styles/main.css'
import App from './App.vue'
import router from './router'
import { initTheme } from './stores/theme'
import { applyPlatformAttributes } from './utils/platform'

applyPlatformAttributes()
initTheme()

const app = createApp(App)

// 兜底异常：至少留下可复现的日志，并提示一次，避免直接白屏
let errorTipShown = false
app.config.errorHandler = (err, _instance, info) => {
  console.error('[FbinHttp] 界面异常', info, err)
  if (!errorTipShown) {
    errorTipShown = true
    message.error('界面出现异常，请重试；若持续出现可重启应用')
  }
}
window.addEventListener('unhandledrejection', (event) => {
  console.error('[FbinHttp] 未处理的 Promise 异常', event.reason)
})

app.use(createPinia())
app.use(router)
app.use(Antd)
app.mount('#app')
