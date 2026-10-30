<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open } from '@tauri-apps/plugin-dialog'
import {
  DatabaseOutlined,
  SafetyCertificateOutlined,
  ThunderboltOutlined,
  CopyOutlined,
  BgColorsOutlined,
  CheckOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  LoginOutlined,
  SwapOutlined,
  CloudDownloadOutlined,
  GlobalOutlined,
  KeyOutlined,
  RobotOutlined,
  InfoCircleOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import { themes, currentThemeId, setTheme } from '@/stores/theme'
import { isMac, openDirLabel, platformLabel, shortcutText } from '@/utils/platform'
import { checkForUpdates, isAutoUpdateEnabled, setAutoUpdateEnabled } from '@/utils/updater'
import { toolNames } from '@/utils/aiTools'
import type { AppInfo, AppSettings, PythonCheckResult, StorageInfo, UpdateInfo } from '@/types/models'

const router = useRouter()

/** 航仔 可调用的工具总数，随注册表自动变化 */
const aiToolCount = toolNames().length

/* ---------- 分区导航 ---------- */
interface Section {
  key: string
  label: string
  desc: string
  icon: Component
}

const sections: Section[] = [
  { key: 'appearance', label: '外观', desc: '主题配色与界面风格', icon: BgColorsOutlined },
  { key: 'storage', label: '数据与存储', desc: '数据库位置、备份与隐私', icon: DatabaseOutlined },
  { key: 'network', label: '网络与安全', desc: '代理、客户端证书、Python 授权', icon: GlobalOutlined },
  { key: 'assistant', label: 'AI 助手', desc: '航仔与模型服务', icon: RobotOutlined },
  { key: 'update', label: '版本更新', desc: '检查并安装新版本', icon: CloudDownloadOutlined },
  { key: 'shortcuts', label: '快捷键与技巧', desc: '常用操作一览', icon: ThunderboltOutlined },
  { key: 'about', label: '关于', desc: '版本信息与数据说明', icon: InfoCircleOutlined },
]

const TAB_KEY = 'astroman:settings-tab'

const active = ref<string>(localStorage.getItem(TAB_KEY) || 'appearance')
// 本地缓存的分区名可能是历史版本残留，回落到第一个分区
if (!sections.some((item) => item.key === active.value)) active.value = 'appearance'
const activeSection = computed(
  () => sections.find((item) => item.key === active.value) ?? sections[0],
)

watch(active, (value) => localStorage.setItem(TAB_KEY, value))

function selectSection(key: string) {
  active.value = key
}

/* ---------- 基础信息 ---------- */
const info = ref<AppInfo | null>(null)
const storage = ref<StorageInfo | null>(null)
const storageLoading = ref(false)

const network = reactive<AppSettings>({
  proxyEnabled: false,
  proxyUrl: '',
  certPath: '',
  certPassword: '',
  pythonPath: '',
  pythonEnabled: false,
})
const networkLoading = ref(false)
const pythonChecking = ref(false)
const pythonCheckResult = ref<PythonCheckResult | null>(null)

async function checkPython() {
  pythonChecking.value = true
  try {
    pythonCheckResult.value = await api.pythonCheck(network.pythonPath)
  } catch (e) {
    message.error(String(e))
  } finally {
    pythonChecking.value = false
  }
}

onMounted(async () => {
  try {
    const [app, dir, settings] = await Promise.all([
      api.appInfo(),
      api.storageInfo(),
      api.settingsGet(),
    ])
    info.value = app
    storage.value = dir
    Object.assign(network, settings)
  } catch (e) {
    message.error(String(e))
  }
})

async function saveNetwork() {
  networkLoading.value = true
  try {
    const saved = await api.settingsSet({ ...network })
    Object.assign(network, saved)
    message.success('设置已保存并立即生效')
  } catch (e) {
    message.error(String(e))
  } finally {
    networkLoading.value = false
  }
}

async function pickCert() {
  const picked = await open({
    directory: false,
    multiple: false,
    title: '选择客户端证书（.pfx / .p12）',
    filters: [{ name: 'PKCS#12 证书', extensions: ['pfx', 'p12'] }],
  })
  if (typeof picked === 'string') network.certPath = picked
}

async function copy(text?: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.warning('当前环境不允许访问剪贴板')
  }
}

const storageSize = computed(() => {
  if (!storage.value) return '-'
  return fmtSize(storage.value.dbSize + storage.value.walSize)
})

function fmtSize(bytes: number) {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}

async function changeStorage() {
  let picked: string | null = null
  try {
    const result = await open({
      directory: true,
      multiple: false,
      title: '选择数据存储目录',
      defaultPath: storage.value?.dataDir,
    })
    picked = typeof result === 'string' ? result : null
  } catch (e) {
    message.error(String(e))
    return
  }
  if (!picked) return

  Modal.confirm({
    title: '如何切换数据目录？',
    content:
      '「迁移数据」会把当前数据库复制到新目录，原目录文件保留；「仅切换」则新目录从空白数据开始。切换后需要重启应用才能生效。',
    okText: '迁移数据并切换',
    cancelText: '仅切换',
    keyboard: false,
    async onOk() {
      await applyStorage(picked as string, true)
    },
    async onCancel() {
      await applyStorage(picked as string, false)
    },
  })
}

async function applyStorage(path: string, migrate: boolean) {
  storageLoading.value = true
  try {
    storage.value = await api.storageSet(path, migrate)
    Modal.confirm({
      title: '存储位置已更新',
      content: '重启应用后生效，重启前仍使用原目录。是否立即重启？',
      okText: '立即重启',
      cancelText: '稍后手动重启',
      async onOk() {
        await api.appRestart()
      },
    })
  } catch (e) {
    message.error(String(e))
  } finally {
    storageLoading.value = false
  }
}

async function resetStorage() {
  Modal.confirm({
    title: '恢复默认存储位置？',
    content: '恢复后将使用应用默认数据目录，自定义目录中的文件不会被删除。需要重启应用生效。',
    okText: '恢复默认',
    okType: 'danger',
    async onOk() {
      storageLoading.value = true
      try {
        storage.value = await api.storageReset()
        Modal.confirm({
          title: '已恢复默认位置',
          content: '是否立即重启应用？',
          okText: '立即重启',
          cancelText: '稍后',
          async onOk() {
            await api.appRestart()
          },
        })
      } catch (e) {
        message.error(String(e))
      } finally {
        storageLoading.value = false
      }
    },
  })
}

async function openStorageDir(path?: string) {
  if (!path) return
  try {
    await api.storageOpen(path)
  } catch (e) {
    message.error(String(e))
  }
}

async function restartApp() {
  try {
    await api.appRestart()
  } catch (e) {
    message.error(String(e))
  }
}

/* ---------- 自动更新 ---------- */
const autoUpdate = ref(isAutoUpdateEnabled())
const updateLoading = ref(false)
const updateInfo = ref<UpdateInfo | null>(null)

function toggleAutoUpdate(value: boolean) {
  autoUpdate.value = value
  setAutoUpdateEnabled(value)
}

async function manualCheck() {
  updateLoading.value = true
  try {
    const result = await checkForUpdates()
    if (result) updateInfo.value = result
  } finally {
    updateLoading.value = false
  }
}

async function openReleasePage() {
  try {
    await api.openExternal(updateInfo.value?.releaseUrl || 'https://github.com/Wayward-GOU/AstroMan/releases')
  } catch (e) {
    message.error(String(e))
  }
}
</script>

<template>
  <div class="page settings-page">
    <PageHeader
      eyebrow="Settings"
      title="设置"
      subtitle="本地版不收集任何数据，所有内容只保存在这台电脑上"
    />

    <div class="settings">
      <!-- 左侧分类 -->
      <aside class="set-nav">
        <button
          v-for="item in sections"
          :key="item.key"
          type="button"
          class="nav-item"
          :class="{ active: active === item.key }"
          @click="selectSection(item.key)"
        >
          <span class="nav-ico"><component :is="item.icon" /></span>
          <span class="nav-text">
            <span class="nav-name">{{ item.label }}</span>
            <span class="nav-desc">{{ item.desc }}</span>
          </span>
        </button>
        <div class="nav-foot">AstroMan v{{ info?.version || '-' }}</div>
      </aside>

      <!-- 右侧内容 -->
      <section class="set-body">
        <header class="panel-head">
          <h2>{{ activeSection.label }}</h2>
          <p>{{ activeSection.desc }}</p>
        </header>

        <!-- ============ 外观 ============ -->
        <div v-if="active === 'appearance'" class="panel">
          <div class="group">
            <div class="group-title">主题配色</div>
            <div class="group-desc">切换后立即生效，选择会保存在本机</div>
            <div class="theme-list">
              <button
                v-for="item in themes"
                :key="item.id"
                type="button"
                class="theme-item"
                :class="{ active: currentThemeId === item.id }"
                @click="setTheme(item.id)"
              >
                <span
                  class="theme-preview"
                  :style="{ background: item.swatches[0], borderColor: item.swatches[2] }"
                >
                  <span class="theme-side" :style="{ background: item.swatches[1] }" />
                  <span class="theme-dot" :style="{ background: item.swatches[2] }" />
                  <span class="theme-line" :style="{ background: item.swatches[3] }" />
                </span>
                <span class="theme-name">{{ item.name }}</span>
                <span class="theme-desc">{{ item.desc }}</span>
                <CheckOutlined v-if="currentThemeId === item.id" class="theme-check" />
              </button>
            </div>
          </div>

          <div class="group">
            <div class="group-title">界面缩放</div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">调整整体缩放</div>
                <div class="row-desc">侧边栏底部的 − / 百分比 / + 控件，或用快捷键切换</div>
              </div>
              <div class="row-control">
                <kbd>{{ shortcutText('+') }}</kbd>
                <kbd>{{ shortcutText('-') }}</kbd>
                <kbd>{{ shortcutText('0') }}</kbd>
              </div>
            </div>
          </div>
        </div>

        <!-- ============ 数据与存储 ============ -->
        <div v-else-if="active === 'storage'" class="panel">
          <a-spin :spinning="storageLoading">
            <div class="group">
              <div class="group-title">
                存储位置
                <span class="tag" :class="{ accent: storage?.isCustom }">
                  {{ storage?.isCustom ? '自定义目录' : '默认目录' }}
                </span>
              </div>
              <div class="group-desc">项目、请求、环境变量与历史记录都存在这里的 SQLite 文件中</div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">数据目录</div>
                  <div class="row-desc mono">{{ storage?.dataDir || '-' }}</div>
                </div>
                <div class="row-control">
                  <a-button size="small" @click="copy(storage?.dataDir)">
                    <template #icon><CopyOutlined /></template>
                    复制
                  </a-button>
                  <a-button size="small" :disabled="!storage?.dataDir" @click="openStorageDir(storage?.dataDir)">
                    <template #icon><FolderOpenOutlined /></template>
                    {{ openDirLabel }}
                  </a-button>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">数据库文件</div>
                  <div class="row-desc mono">{{ storage?.dbPath || '-' }}</div>
                </div>
                <div class="row-control">
                  <a-button size="small" @click="copy(storage?.dbPath)">
                    <template #icon><CopyOutlined /></template>
                    复制
                  </a-button>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">占用空间</div>
                  <div class="row-desc">数据库与预写日志合计</div>
                </div>
                <div class="row-control mono">{{ storageSize }}</div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">默认目录</div>
                  <div class="row-desc mono">{{ storage?.defaultDir || '-' }}</div>
                </div>
              </div>

              <div v-if="storage?.pendingDir" class="row">
                <div class="row-label">
                  <div class="row-title warn">待生效目录</div>
                  <div class="row-desc mono">{{ storage.pendingDir }}</div>
                </div>
                <div class="row-control">
                  <a-button size="small" type="primary" @click="restartApp">立即重启</a-button>
                </div>
              </div>

              <div class="group-actions">
                <a-button type="primary" :loading="storageLoading" @click="changeStorage">
                  <template #icon><SwapOutlined /></template>
                  更改位置
                </a-button>
              </div>
            </div>

            <div class="callout">
              <LoginOutlined />
              <span>
                更改位置后需要重启应用生效；选择「迁移数据」会把当前数据库完整复制到新目录，原目录文件保留，确认数据无误后可自行删除。
              </span>
            </div>

            <div class="group danger-group">
              <div class="group-title">恢复默认</div>
              <div class="group-desc">切回应用默认数据目录，自定义目录里的文件不会被删除</div>
              <div class="row">
                <div class="row-label">
                  <div class="row-title">恢复默认存储位置</div>
                  <div class="row-desc">如果你从没改过目录，这里无需操作</div>
                </div>
                <div class="row-control">
                  <a-button
                    danger
                    size="small"
                    :disabled="!storage?.isCustom && !storage?.pendingDir"
                    @click="resetStorage"
                  >
                    <template #icon><ReloadOutlined /></template>
                    恢复默认
                  </a-button>
                </div>
              </div>
            </div>
          </a-spin>
        </div>

        <!-- ============ 网络与安全 ============ -->
        <div v-else-if="active === 'network'" class="panel">
          <div class="group">
            <div class="group-title">代理</div>
            <div class="group-desc">支持 HTTP / HTTPS / SOCKS5，仅对本应用的请求生效</div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">启用代理</div>
                <div class="row-desc">开启后所有请求都经由下面的地址转发</div>
              </div>
              <div class="row-control">
                <a-switch v-model:checked="network.proxyEnabled" size="small" />
              </div>
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">代理地址</div>
                <div class="row-desc">例：http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080</div>
              </div>
              <div class="row-control wide">
                <a-input
                  v-model:value="network.proxyUrl"
                  size="small"
                  :disabled="!network.proxyEnabled"
                  placeholder="http://127.0.0.1:7890"
                />
              </div>
            </div>
          </div>

          <div class="group">
            <div class="group-title">客户端证书</div>
            <div class="group-desc">用于双向 TLS（mTLS）鉴权，仅支持 PKCS#12 的 .pfx / .p12</div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">证书文件</div>
                <div class="row-desc">留空表示不携带客户端证书</div>
              </div>
              <div class="row-control wide">
                <a-input
                  v-model:value="network.certPath"
                  size="small"
                  readonly
                  placeholder="未选择证书"
                />
                <a-button size="small" @click="pickCert">
                  <template #icon><KeyOutlined /></template>
                  选择
                </a-button>
                <a-button
                  v-if="network.certPath"
                  size="small"
                  type="text"
                  danger
                  @click="network.certPath = ''"
                >
                  清除
                </a-button>
              </div>
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">证书密码</div>
                <div class="row-desc">PKCS#12 密码，没有可留空</div>
              </div>
              <div class="row-control wide">
                <a-input-password
                  v-model:value="network.certPassword"
                  size="small"
                  :disabled="!network.certPath"
                  placeholder="证书密码"
                />
              </div>
            </div>
          </div>

          <div class="group">
            <div class="group-title">Python 脚本</div>
            <div class="group-desc">测试场景的前后置钩子、Python 用例与套件包导入都依赖它</div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">允许执行本地脚本</div>
                <div class="row-desc">
                  {{
                    network.pythonEnabled
                      ? '已允许执行本地 Python 脚本'
                      : '未开启：测试场景的 Python 钩子、Python 用例与套件包导入都会被拒绝'
                  }}
                </div>
              </div>
              <div class="row-control">
                <a-switch v-model:checked="network.pythonEnabled" size="small" />
              </div>
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">解释器路径</div>
                <div class="row-desc">留空自动探测（python3 / python / py -3）</div>
              </div>
              <div class="row-control wide">
                <a-input
                  v-model:value="network.pythonPath"
                  size="small"
                  placeholder="自动探测"
                />
                <a-button size="small" :loading="pythonChecking" @click="checkPython">
                  <template #icon><ReloadOutlined /></template>
                  探测
                </a-button>
              </div>
            </div>

            <div v-if="pythonCheckResult" class="probe">
              <div :class="pythonCheckResult.available ? 'ok' : 'bad'">
                {{ pythonCheckResult.available ? '已找到' : '未找到' }}
                <template v-if="pythonCheckResult.info">
                  Python {{ pythonCheckResult.info.version }}（{{ pythonCheckResult.info.display }}）
                </template>
              </div>
              <div class="probe-msg">{{ pythonCheckResult.message }}</div>
              <div v-if="network.pythonEnabled && !pythonCheckResult.enabled" class="probe-msg">
                开关还没保存生效：点「保存并生效」后授权才会写入本机配置。
              </div>
              <div v-if="pythonCheckResult.tried.length" class="probe-tried mono">
                试过：{{ pythonCheckResult.tried.join(' / ') }}
              </div>
            </div>
          </div>

          <div class="panel-footer">
            <span class="footer-hint">修改后需要点这里保存才会生效</span>
            <a-button type="primary" :loading="networkLoading" @click="saveNetwork">保存并生效</a-button>
          </div>

          <div class="callout">
            <SafetyCertificateOutlined />
            <span>
              证书用于双向 TLS（mTLS）鉴权；代理支持 HTTP / HTTPS / SOCKS5。若填写有误，保存时会直接提示原因且不会覆盖当前生效配置。
              Python 授权只影响「测试场景」里的前后置钩子、Python 用例与导入的套件包；未开启时这些脚本一律不执行，运行会给出明确原因。
            </span>
          </div>
        </div>

        <!-- ============ AI 助手 ============ -->
        <div v-else-if="active === 'assistant'" class="panel">
          <div class="group">
            <div class="group-title">
              航仔
              <span class="tag accent">已上线</span>
            </div>
            <div class="group-desc">用一句话描述需求，直接操作项目、请求、压测与测试场景</div>
            <div class="hint">
              点右侧屏幕中间的机器人即可打开航仔。先在「模型服务」里配置一家服务商（DeepSeek / Kimi /
              智谱 / 通义千问 / Claude / Gemini / Ollama 等，兼容 OpenAI 协议的服务都能接），之后能建项目、写请求、发请求、生成签名脚本、查压测报告与导出记录。
              敏感操作会先弹确认；密钥仅加密保存在本机数据库，不会上传到任何服务器。
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">可用工具</div>
                <div class="row-desc">覆盖项目、请求、环境变量、历史记录、压测报告、测试场景与脚本</div>
              </div>
              <div class="row-control mono">{{ aiToolCount }} 个</div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">用量统计</div>
                <div class="row-desc">按天查看调用次数、token 与预估费用，可按模型和工具拆分</div>
              </div>
              <div class="row-control">
                <a-button size="small" @click="router.push({ name: 'ai-usage' })">查看用量</a-button>
              </div>
            </div>
            <div class="group-actions">
              <a-button type="primary" @click="router.push({ name: 'ai' })">打开航仔</a-button>
            </div>
          </div>
        </div>

        <!-- ============ 版本更新 ============ -->
        <div v-else-if="active === 'update'" class="panel">
          <div class="group">
            <div class="group-title">
              版本
              <span v-if="updateInfo?.hasUpdate" class="tag accent">有新版本</span>
            </div>
            <div class="group-desc">从 GitHub Release 获取最新版本，下载后按安装向导升级</div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">当前版本</div>
                <div class="row-desc">安装包不经过任何第三方服务器</div>
              </div>
              <div class="row-control mono">v{{ info?.version || '-' }}</div>
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">最新版本</div>
                <div class="row-desc">点下方「检查更新」获取 GitHub 上的最新发布</div>
              </div>
              <div class="row-control">
                <span class="mono">{{ updateInfo ? `v${updateInfo.latestVersion}` : '尚未检查' }}</span>
                <span v-if="updateInfo && !updateInfo.hasUpdate" class="tag">已是最新</span>
              </div>
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">自动检查更新</div>
                <div class="row-desc">启动后自动检查，有新版本时弹窗提示</div>
              </div>
              <div class="row-control">
                <a-switch
                  :checked="autoUpdate"
                  size="small"
                  @change="(v: unknown) => toggleAutoUpdate(Boolean(v))"
                />
              </div>
            </div>

            <div class="group-actions">
              <a-button type="primary" :loading="updateLoading" @click="manualCheck">
                <template #icon><CloudDownloadOutlined /></template>
                检查更新
              </a-button>
              <a-button @click="openReleasePage">打开发布页</a-button>
            </div>
          </div>

          <div v-if="isMac" class="callout warn">
            <SafetyCertificateOutlined />
            <span>
              macOS 首次打开或覆盖升级后若提示「Apple 无法验证 AstroMan.app」，是系统对未公证应用的拦截，与 Intel / Apple Silicon
              架构无关：到「系统设置 → 隐私与安全性」点击「仍要打开」，或在终端执行
              <code>sudo xattr -dr com.apple.quarantine /Applications/AstroMan.app</code>。
            </span>
          </div>

          <div class="callout">
            <InfoCircleOutlined />
            <span>
              安装包直接从 GitHub Release 下载。国内网络访问 GitHub 可能较慢，失败时请检查网络后重试。
            </span>
          </div>
        </div>

        <!-- ============ 快捷键与技巧 ============ -->
        <div v-else-if="active === 'shortcuts'" class="panel">
          <div class="group">
            <div class="group-title">快捷键</div>
            <div class="row compact">
              <div class="row-label"><div class="row-title">在 URL 输入框直接发送请求</div></div>
              <div class="row-control"><kbd>Enter</kbd></div>
            </div>
            <div class="row compact">
              <div class="row-label"><div class="row-title">放大界面</div></div>
              <div class="row-control"><kbd>{{ shortcutText('+') }}</kbd></div>
            </div>
            <div class="row compact">
              <div class="row-label"><div class="row-title">缩小界面</div></div>
              <div class="row-control"><kbd>{{ shortcutText('-') }}</kbd></div>
            </div>
            <div class="row compact">
              <div class="row-label"><div class="row-title">恢复 100% 缩放</div></div>
              <div class="row-control"><kbd>{{ shortcutText('0') }}</kbd></div>
            </div>
          </div>

          <div class="group">
            <div class="group-title">技巧</div>
            <div class="row compact">
              <div class="row-label">
                <div class="row-title">引用环境变量</div>
                <div class="row-desc">在 URL / Headers / Body 中都可使用</div>
              </div>
              <div class="row-control"><kbd v-pre>{{base_url}}</kbd></div>
            </div>
            <div class="row compact">
              <div class="row-label">
                <div class="row-title">生成随机整数</div>
                <div class="row-desc">用于批量造测试数据</div>
              </div>
              <div class="row-control"><kbd v-pre>{{randomInt(1,100)}}</kbd></div>
            </div>
            <div class="row compact">
              <div class="row-label">
                <div class="row-title">停止压测</div>
                <div class="row-desc">压测中可随时停止，已产生的数据仍会保存成报告</div>
              </div>
              <div class="row-control"><kbd>停止</kbd></div>
            </div>
          </div>
        </div>

        <!-- ============ 关于 ============ -->
        <div v-else-if="active === 'about'" class="panel">
          <div class="group">
            <div class="group-title">应用信息</div>
            <div class="row">
              <div class="row-label"><div class="row-title">版本</div></div>
              <div class="row-control mono">AstroMan {{ info?.version || '-' }}</div>
            </div>
            <div class="row">
              <div class="row-label"><div class="row-title">运行模式</div></div>
              <div class="row-control">纯本地桌面应用（Tauri）</div>
            </div>
            <div class="row">
              <div class="row-label"><div class="row-title">运行系统</div></div>
              <div class="row-control">{{ platformLabel }}</div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">数据目录</div>
                <div class="row-desc mono">{{ info?.dataDir || '-' }}</div>
              </div>
              <div class="row-control">
                <a-button size="small" @click="copy(info?.dataDir)">
                  <template #icon><CopyOutlined /></template>
                  复制
                </a-button>
              </div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">数据库文件</div>
                <div class="row-desc mono">{{ info?.dbPath || '-' }}</div>
              </div>
              <div class="row-control">
                <a-button size="small" @click="copy(info?.dbPath)">
                  <template #icon><CopyOutlined /></template>
                  复制
                </a-button>
              </div>
            </div>
          </div>

          <div class="group">
            <div class="group-title">数据与隐私</div>
            <div class="group-desc">所有数据都在本机，可随时备份</div>
            <ul class="bullet">
              <li>
                项目、集合、请求、环境变量、历史记录和压测报告全部存放在上方的 SQLite 文件中，不依赖任何云端后端。
              </li>
              <li>不需要注册账号，没有登录态、权限体系与付费限制。</li>
              <li>备份方式：退出应用后复制数据库文件；换电脑时放回同名路径即可。</li>
              <li>卸载应用不会自动删除数据目录，如需彻底清理请手动删除。</li>
            </ul>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
/* 整页撑满可用高度，面板内部滚动，像桌面软件而不是长卡片流 */
.settings-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding-bottom: 16px;
}

.page {
  max-width: none;
}

.settings {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 0;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-lg);
  background: var(--fm-surface);
  box-shadow: var(--fm-shadow);
  overflow: hidden;
}

/* ---------- 左侧分类 ---------- */
.set-nav {
  width: 216px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 10px;
  background: var(--fm-surface-alt);
  border-right: 1px solid var(--fm-border);
  overflow-y: auto;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 9px 10px;
  border: none;
  border-radius: 9px;
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: var(--fm-text-2);
  transition: background 0.14s, color 0.14s;
}

.nav-item:hover {
  background: var(--fm-surface);
}

.nav-item.active {
  background: var(--fm-primary-soft);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--fm-primary) 22%, transparent);
}

.nav-ico {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  background: var(--fm-bg);
  color: var(--fm-muted);
  transition: background 0.14s, color 0.14s;
}

.nav-item.active .nav-ico {
  background: var(--fm-primary);
  color: #fff;
}

.nav-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.nav-name {
  font-size: 12.5px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-item.active .nav-name {
  color: var(--fm-primary);
  font-weight: 600;
}

.nav-desc {
  font-size: 10px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-foot {
  margin-top: auto;
  padding: 14px 10px 4px;
  font-size: 10px;
  color: var(--fm-muted);
  border-top: 1px solid var(--fm-border);
}

/* ---------- 右侧内容 ---------- */
.set-body {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: 20px 26px 28px;
}

.panel-head {
  margin-bottom: 18px;
}

.panel-head h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.1px;
}

.panel-head p {
  margin: 4px 0 0;
  font-size: 11.5px;
  color: var(--fm-muted);
}

.group {
  margin-bottom: 22px;
}

.group-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
}

.group-desc {
  margin: 4px 0 10px;
  font-size: 11px;
  color: var(--fm-muted);
  line-height: 1.6;
}

.row {
  display: flex;
  align-items: center;
  gap: 18px;
  min-height: 54px;
  padding: 12px 2px;
  border-bottom: 1px solid var(--fm-border);
}

.row:first-of-type {
  border-top: 1px solid var(--fm-border);
}

.row.compact {
  min-height: 44px;
  padding: 8px 2px;
}

.row:last-child {
  border-bottom: 1px solid var(--fm-border);
}

/* 最后一组操作按钮不重复画分割线 */
.group-actions + .callout {
  margin-top: 4px;
}

.row-label {
  flex: 1;
  min-width: 0;
}

.row-title {
  font-size: 12.5px;
  font-weight: 500;
  color: var(--fm-text);
}

.row-title.warn {
  color: var(--fm-warning);
}

.row-desc {
  margin-top: 3px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-muted);
  word-break: break-all;
}

.row-control {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.row-control.wide {
  flex: 0 0 340px;
  min-width: 220px;
}

.row-control.wide :deep(.ant-input),
.row-control.wide :deep(.ant-input-affix-wrapper) {
  flex: 1;
  min-width: 0;
}

.group-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  padding-top: 14px;
}

.panel-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  padding: 4px 0 14px;
}

.footer-hint {
  font-size: 11px;
  color: var(--fm-muted);
}

.mono {
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11px;
}

/* ---------- 标签 / 提示 ---------- */
.tag {
  font-size: 10px;
  font-weight: 500;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 8px;
  border-radius: 999px;
  flex-shrink: 0;
}

.tag.accent {
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-color: transparent;
}

.callout {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  margin-bottom: 20px;
  padding: 11px 13px;
  border-radius: 10px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  font-size: 11px;
  line-height: 1.75;
  color: var(--fm-muted);
}

.callout.warn {
  border-color: var(--fm-warning);
  background: var(--fm-warning-soft);
  color: var(--fm-text-2);
}

.callout > :first-child {
  margin-top: 2px;
  flex-shrink: 0;
}

.callout code {
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 10.5px;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-bg);
  word-break: break-all;
}

.hint {
  padding: 12px 14px;
  margin-bottom: 14px;
  border-radius: 10px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  font-size: 11.5px;
  line-height: 1.85;
  color: var(--fm-text-2);
}

.bullet {
  margin: 8px 0 0;
  padding-left: 18px;
}

.bullet li {
  font-size: 11.5px;
  line-height: 1.9;
  color: var(--fm-text-2);
}

.probe {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 10px;
  padding: 9px 11px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-alt);
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-text-2);
}

.probe .ok {
  color: var(--fm-success);
  font-weight: 600;
}

.probe .bad {
  color: var(--fm-danger);
  font-weight: 600;
}

.probe-msg {
  color: var(--fm-muted);
}

.probe-tried {
  font-size: 10.5px;
  color: var(--fm-muted);
  word-break: break-all;
}

.danger-group .group-title {
  color: var(--fm-danger);
}

kbd {
  display: inline-block;
  padding: 2px 7px;
  border-radius: 5px;
  border: 1px solid var(--fm-border-strong);
  border-bottom-width: 2px;
  background: var(--fm-surface-alt);
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 10px;
  color: var(--fm-text-2);
}

/* ---------- 主题选择 ---------- */
.theme-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(148px, 1fr));
  gap: 12px;
}

.theme-item {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 10px;
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.16s, box-shadow 0.16s;
}

.theme-item:hover {
  border-color: var(--fm-border-strong);
}

.theme-item.active {
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 2px var(--fm-primary-soft);
}

.theme-preview {
  position: relative;
  display: block;
  height: 46px;
  margin-bottom: 5px;
  border: 1px solid;
  border-radius: 6px;
  overflow: hidden;
}

.theme-side {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  width: 14px;
}

.theme-dot {
  position: absolute;
  top: 10px;
  left: 22px;
  width: 20px;
  height: 6px;
  border-radius: 3px;
}

.theme-line {
  position: absolute;
  top: 24px;
  left: 22px;
  right: 10px;
  height: 4px;
  border-radius: 2px;
  opacity: 0.32;
}

.theme-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
}

.theme-desc {
  font-size: 10px;
  color: var(--fm-muted);
}

.theme-check {
  position: absolute;
  top: 12px;
  right: 12px;
  font-size: 12px;
  color: var(--fm-primary);
}

/* ---------- 窄窗口：导航转为横向标签 ---------- */
@media (max-width: 900px) {
  .settings {
    flex-direction: column;
  }

  .set-nav {
    width: 100%;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    padding: 8px;
    border-right: none;
    border-bottom: 1px solid var(--fm-border);
    overflow-x: auto;
    overflow-y: hidden;
  }

  .nav-item {
    width: auto;
    flex-shrink: 0;
    padding: 6px 10px;
  }

  .nav-desc,
  .nav-foot {
    display: none;
  }

  .set-body {
    padding: 16px;
  }

  .row {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }

  .row-control.wide {
    flex: 1 1 auto;
    min-width: 0;
  }

  .panel-footer {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }

  .panel-footer :deep(.ant-btn) {
    width: 100%;
  }
}
</style>
