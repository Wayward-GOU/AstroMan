<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open } from '@tauri-apps/plugin-dialog'
import {
  DatabaseOutlined,
  SafetyCertificateOutlined,
  ThunderboltOutlined,
  CopyOutlined,
  LaptopOutlined,
  BgColorsOutlined,
  CheckOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  LoginOutlined,
  SwapOutlined,
  CloudDownloadOutlined,
  GlobalOutlined,
  KeyOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import { themes, currentThemeId, setTheme } from '@/stores/theme'
import { isMac, openDirLabel, platformLabel, shortcutText } from '@/utils/platform'
import { checkForUpdates, isAutoUpdateEnabled, setAutoUpdateEnabled } from '@/utils/updater'
import type { AppInfo, AppSettings, StorageInfo, UpdateInfo } from '@/types/models'

const router = useRouter()

const info = ref<AppInfo | null>(null)
const storage = ref<StorageInfo | null>(null)
const storageLoading = ref(false)

const network = reactive<AppSettings>({
  proxyEnabled: false,
  proxyUrl: '',
  certPath: '',
  certPassword: '',
})
const networkLoading = ref(false)

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
    message.success('网络设置已保存并立即生效')
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
  <div class="page">
    <PageHeader
      eyebrow="Settings"
      title="设置"
      subtitle="本地版不收集任何数据，所有内容只保存在这台电脑上"
    />

    <div class="grid">
      <div class="fm-card block theme-block">
        <div class="block-head">
          <span class="block-icon blue"><BgColorsOutlined /></span>
          <div>
            <div class="block-title">外观主题</div>
            <div class="block-desc">选择配色方案，切换后立即生效并保存在本机</div>
          </div>
        </div>
        <div class="theme-list">
          <button
            v-for="item in themes"
            :key="item.id"
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

      <div class="fm-card block storage-block">
        <div class="block-head">
          <span class="block-icon blue"><DatabaseOutlined /></span>
          <div>
            <div class="block-title">数据存储位置</div>
            <div class="block-desc">自定义 SQLite 数据文件存放目录，可迁移现有数据</div>
          </div>
          <span class="storage-mode" :class="{ custom: storage?.isCustom }">
            {{ storage?.isCustom ? '自定义目录' : '默认目录' }}
          </span>
        </div>

        <a-spin :spinning="storageLoading">
          <div class="rows">
            <div class="row">
              <span class="label">数据目录</span>
              <span class="value mono path">{{ storage?.dataDir || '-' }}</span>
              <a-button type="text" size="small" class="copy" title="复制路径" @click="copy(storage?.dataDir)">
                <template #icon><CopyOutlined /></template>
              </a-button>
            </div>
            <div class="row">
              <span class="label">数据库文件</span>
              <span class="value mono path">{{ storage?.dbPath || '-' }}</span>
              <a-button type="text" size="small" class="copy" title="复制路径" @click="copy(storage?.dbPath)">
                <template #icon><CopyOutlined /></template>
              </a-button>
            </div>
            <div class="row">
              <span class="label">占用空间</span>
              <span class="value mono">{{ storageSize }}</span>
            </div>
            <div class="row">
              <span class="label">默认目录</span>
              <span class="value mono path muted">{{ storage?.defaultDir || '-' }}</span>
            </div>
            <div v-if="storage?.pendingDir" class="row">
              <span class="label">待生效</span>
              <span class="value mono path pending">{{ storage.pendingDir }}</span>
              <a-button type="link" size="small" class="copy" @click="restartApp">立即重启</a-button>
            </div>
          </div>

          <div class="storage-actions">
            <a-button type="primary" :loading="storageLoading" @click="changeStorage">
              <template #icon><SwapOutlined /></template>
              更改位置
            </a-button>
            <a-button :disabled="!storage?.dataDir" @click="openStorageDir(storage?.dataDir)">
              <template #icon><FolderOpenOutlined /></template>
              {{ openDirLabel }}
            </a-button>
            <a-button :disabled="!storage?.isCustom && !storage?.pendingDir" @click="resetStorage">
              <template #icon><ReloadOutlined /></template>
              恢复默认
            </a-button>
          </div>

          <div class="storage-tip">
            <LoginOutlined />
            <span>
              更改位置后需要重启应用生效；选择「迁移数据」会把当前数据库完整复制到新目录，原目录文件保留，确认数据无误后可自行删除。
            </span>
          </div>
        </a-spin>
      </div>

      <div class="fm-card block network-block">
        <div class="block-head">
          <span class="block-icon blue"><GlobalOutlined /></span>
          <div>
            <div class="block-title">网络与安全</div>
            <div class="block-desc">代理与客户端证书对全部请求生效，配置仅保存在本机</div>
          </div>
        </div>
        <div class="rows">
          <div class="row net-row">
            <span class="label">启用代理</span>
            <span class="value">
              <a-switch v-model:checked="network.proxyEnabled" size="small" />
            </span>
          </div>
          <div class="row net-row">
            <span class="label">代理地址</span>
            <span class="value">
              <a-input
                v-model:value="network.proxyUrl"
                size="small"
                :disabled="!network.proxyEnabled"
                placeholder="http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080"
              />
            </span>
          </div>
          <div class="row net-row">
            <span class="label">客户端证书</span>
            <span class="value cert-value">
              <a-input
                v-model:value="network.certPath"
                size="small"
                readonly
                placeholder="未选择（仅支持 PKCS#12 的 .pfx / .p12）"
              />
              <a-button size="small" @click="pickCert">
                <template #icon><KeyOutlined /></template>
                选择
              </a-button>
              <a-button v-if="network.certPath" size="small" type="text" danger @click="network.certPath = ''">
                清除
              </a-button>
            </span>
          </div>
          <div class="row net-row">
            <span class="label">证书密码</span>
            <span class="value">
              <a-input-password
                v-model:value="network.certPassword"
                size="small"
                :disabled="!network.certPath"
                placeholder="PKCS#12 密码，没有可留空"
              />
            </span>
          </div>
        </div>
        <div class="storage-actions">
          <a-button type="primary" :loading="networkLoading" @click="saveNetwork">保存并生效</a-button>
        </div>
        <div class="storage-tip">
          <SafetyCertificateOutlined />
          <span>
            证书用于双向 TLS（mTLS）鉴权；代理支持 HTTP / HTTPS / SOCKS5。若填写有误，保存时会直接提示原因且不会覆盖当前生效配置。
          </span>
        </div>
      </div>

      <div class="fm-card block update-block">
        <div class="block-head">
          <span class="block-icon blue"><CloudDownloadOutlined /></span>
          <div>
            <div class="block-title">版本更新</div>
            <div class="block-desc">从 GitHub Release 获取最新版本，下载后按安装向导升级</div>
          </div>
          <span v-if="updateInfo?.hasUpdate" class="update-badge">有新版本</span>
        </div>
        <div class="rows">
          <div class="row">
            <span class="label">当前版本</span>
            <span class="value mono">v{{ info?.version || '-' }}</span>
          </div>
          <div class="row">
            <span class="label">最新版本</span>
            <span class="value mono">
              {{ updateInfo ? `v${updateInfo.latestVersion}` : '尚未检查' }}
              <template v-if="updateInfo && !updateInfo.hasUpdate">（已是最新）</template>
            </span>
          </div>
          <div class="row">
            <span class="label">自动检查</span>
            <span class="value">启动后自动检查更新，有新版本时弹窗提示</span>
            <a-switch :checked="autoUpdate" size="small" @change="(v: unknown) => toggleAutoUpdate(Boolean(v))" />
          </div>
        </div>
        <div class="storage-actions">
          <a-button type="primary" :loading="updateLoading" @click="manualCheck">
            <template #icon><CloudDownloadOutlined /></template>
            检查更新
          </a-button>
          <a-button @click="openReleasePage">打开发布页</a-button>
        </div>
        <div v-if="isMac" class="storage-tip">
          <SafetyCertificateOutlined />
          <span>
            macOS 首次打开或覆盖升级后若提示「Apple 无法验证 AstroMan.app」，是系统对未公证应用的拦截，与 Intel / Apple Silicon 架构无关：到「系统设置 → 隐私与安全性」点击「仍要打开」，或在终端执行
            <code>sudo xattr -dr com.apple.quarantine /Applications/AstroMan.app</code>。
          </span>
        </div>
        <div class="storage-tip">
          <SafetyCertificateOutlined />
          <span>
            安装包直接从 GitHub Release 下载，不经过任何第三方服务器。国内网络访问 GitHub 可能较慢，失败时请检查网络后重试。
          </span>
        </div>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon blue"><LaptopOutlined /></span>
          <div>
            <div class="block-title">应用信息</div>
            <div class="block-desc">当前版本与本地运行环境</div>
          </div>
        </div>
        <div class="rows">
          <div class="row">
            <span class="label">版本</span>
            <span class="value mono">AstroMan {{ info?.version || '-' }}</span>
          </div>
          <div class="row">
            <span class="label">运行模式</span>
            <span class="value">纯本地桌面应用（Tauri）</span>
          </div>
          <div class="row">
            <span class="label">运行系统</span>
            <span class="value">{{ platformLabel }}</span>
          </div>
          <div class="row">
            <span class="label">数据目录</span>
            <span class="value mono path">{{ info?.dataDir || '-' }}</span>
            <a-button type="text" size="small" class="copy" @click="copy(info?.dataDir)">
              <template #icon><CopyOutlined /></template>
            </a-button>
          </div>
          <div class="row">
            <span class="label">数据库文件</span>
            <span class="value mono path">{{ info?.dbPath || '-' }}</span>
            <a-button type="text" size="small" class="copy" @click="copy(info?.dbPath)">
              <template #icon><CopyOutlined /></template>
            </a-button>
          </div>
        </div>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon green"><SafetyCertificateOutlined /></span>
          <div>
            <div class="block-title">数据与隐私</div>
            <div class="block-desc">所有数据都在本机，可随时备份</div>
          </div>
        </div>
        <ul class="list">
          <li>
            项目、集合、请求、环境变量、历史记录和压测报告全部存放在上方的 SQLite 文件中，
            不依赖任何云端后端。
          </li>
          <li>不需要注册账号，没有登录态、权限体系与付费限制。</li>
          <li>备份方式：退出应用后复制数据库文件；换电脑时放回同名路径即可。</li>
          <li>卸载应用不会自动删除数据目录，如需彻底清理请手动删除。</li>
        </ul>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon purple"><ThunderboltOutlined /></span>
          <div>
            <div class="block-title">AI 辅助（航仔）</div>
            <div class="block-desc">用一句话描述需求，直接操作项目、请求与压测</div>
          </div>
          <span class="soon live">已上线</span>
        </div>
        <div class="ai-hint">
          点右侧屏幕中间的机器人即可打开航仔。先在顶部「模型服务」里配置一家服务商（DeepSeek / Kimi / 智谱 / 通义千问 / Claude / Gemini /
          Ollama 等，兼容 OpenAI 协议的服务都能接），之后能建项目、写请求、发请求、生成签名脚本、查压测报告与导出记录。
          敏感操作会先弹确认；密钥仅加密保存在本机数据库，不会上传到任何服务器。
        </div>
        <ul class="list">
          <li>共 39 个工具，覆盖项目、请求、环境变量、历史记录、压测报告与脚本。</li>
          <li>用量统计可按天查看调用次数、token 与预估费用，按模型和工具拆分明细。</li>
        </ul>
        <div class="ai-actions">
          <a-button type="primary" @click="router.push({ name: 'ai' })">打开航仔</a-button>
          <a-button @click="router.push({ name: 'ai-usage' })">用量统计</a-button>
        </div>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon amber"><DatabaseOutlined /></span>
          <div>
            <div class="block-title">快捷键与技巧</div>
            <div class="block-desc">日常使用中最常用的操作</div>
          </div>
        </div>
        <div class="shortcuts">
          <div class="shortcut"><span class="keys"><kbd>Enter</kbd></span><span>在 URL 输入框直接发送请求</span></div>
          <div class="shortcut"><span class="keys"><kbd v-pre>{{base_url}}</kbd></span><span>在 URL / Headers / Body 中引用环境变量</span></div>
          <div class="shortcut"><span class="keys"><kbd v-pre>{{randomInt(1,100)}}</kbd></span><span>生成范围内的随机整数，用于造测试数据</span></div>
          <div class="shortcut"><span class="keys"><kbd>{{ shortcutText('+') }}</kbd></span><span>放大界面（也可使用左下角的缩放控件）</span></div>
          <div class="shortcut"><span class="keys"><kbd>{{ shortcutText('-') }}</kbd></span><span>缩小界面</span></div>
          <div class="shortcut"><span class="keys"><kbd>{{ shortcutText('0') }}</kbd></span><span>恢复界面缩放为 100%</span></div>
          <div class="shortcut"><span class="keys"><kbd>停止</kbd></span><span>压测中可随时停止，数据仍会保存成报告</span></div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 设置页内容较多，占满可用宽度，不沿用全局 1440px 上限 */
.page {
  max-width: none;
}

.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  align-items: start;
  width: 100%;
}

.block {
  padding: 20px 22px;
  min-width: 0;
}

/* 主题、存储、网络、更新需要整行宽度 */
.theme-block,
.storage-block,
.network-block,
.update-block {
  grid-column: 1 / -1;
}

@media (max-width: 900px) {
  .grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.theme-list {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

@media (max-width: 1180px) {
  .theme-list {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
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

.block-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 16px;
}

.block-icon {
  width: 34px;
  height: 34px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  flex-shrink: 0;
}

.block-icon.blue {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.block-icon.green {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.block-icon.purple {
  background: var(--fm-purple-soft);
  color: var(--fm-purple);
}

.block-icon.amber {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.update-block {
  grid-column: 1 / -1;
}

.update-badge {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-success);
  background: var(--fm-success-soft);
  padding: 2px 10px;
  border-radius: 999px;
  flex-shrink: 0;
}

.block-title {
  font-size: 14px;
  font-weight: 600;
}

.block-desc {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-muted);
}

.soon {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 8px;
  border-radius: 999px;
}

.soon.live {
  color: var(--fm-success);
  background: var(--fm-success-soft);
  border-color: transparent;
}

.ai-actions {
  display: flex;
  gap: 10px;
  margin-top: 14px;
}

.rows {
  display: flex;
  flex-direction: column;
}

.row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 0;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 12px;
}

.row:last-child {
  border-bottom: none;
}

.label {
  width: 84px;
  flex-shrink: 0;
  color: var(--fm-muted);
  font-size: 11px;
}

.net-row .value {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
}

.cert-value {
  gap: 8px;
}

.value {
  flex: 1;
  min-width: 0;
  word-break: break-all;
  color: var(--fm-text-2);
}

.path {
  font-size: 11px;
  line-height: 1.6;
}

.copy {
  color: var(--fm-muted);
  flex-shrink: 0;
}

.list {
  margin: 0;
  padding-left: 18px;
}

.list li {
  font-size: 12px;
  line-height: 1.9;
  color: var(--fm-text-2);
}

.ai-hint {
  font-size: 12px;
  line-height: 1.9;
  color: var(--fm-text-2);
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  margin-bottom: 14px;
}

.shortcuts {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.shortcut {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.keys {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 128px;
  flex-shrink: 0;
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

/* ---------- 数据存储位置 ---------- */
.storage-block {
  grid-column: 1 / -1;
}

.storage-mode {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 2px 10px;
  border-radius: 999px;
  flex-shrink: 0;
}

.storage-mode.custom {
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-color: transparent;
}

.value.muted {
  color: var(--fm-muted);
}

.value.pending {
  color: var(--fm-warning);
}

.storage-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 14px;
}

.storage-tip {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: 14px;
  padding: 10px 12px;
  border-radius: 9px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.storage-tip > :first-child {
  margin-top: 2px;
  flex-shrink: 0;
}
</style>
