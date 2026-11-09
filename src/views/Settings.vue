<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save as saveFile } from '@tauri-apps/plugin-dialog'
import {
  DatabaseOutlined,
  SafetyCertificateOutlined,
  ThunderboltOutlined,
  CopyOutlined,
  BgColorsOutlined,
  CheckOutlined,
  FolderOpenOutlined,
  ExportOutlined,
  ImportOutlined,
  ReloadOutlined,
  LoginOutlined,
  SwapOutlined,
  CloudDownloadOutlined,
  GlobalOutlined,
  KeyOutlined,
  RobotOutlined,
  InfoCircleOutlined,
  SafetyOutlined,
  CloudServerOutlined,
  ExperimentOutlined,
  FileProtectOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import RemoteBackupPanel from '@/components/RemoteBackupPanel.vue'
import * as api from '@/api'
import { themes, currentThemeId, setTheme } from '@/stores/theme'
import { isMac, openDirLabel, platformLabel, shortcutText } from '@/utils/platform'
import { checkForUpdates, isAutoUpdateEnabled, setAutoUpdateEnabled } from '@/utils/updater'
import { toolNames } from '@/utils/aiTools'
import { scriptTimeoutMs, setScriptTimeoutMs, SCRIPT_TIMEOUT_RANGE } from '@/utils/scriptHost'
import type {
  AiProvider,
  AppInfo,
  AppSettings,
  BackupItem,
  CheckResult,
  PerformanceProfile,
  PolicySnapshot,
  PythonCheckResult,
  SecretStatus,
  StorageInfo,
  UpdateInfo,
} from '@/types/models'

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
  { key: 'backup', label: '备份与自检', desc: '快照、恢复、数据库体检', icon: CloudServerOutlined },
  { key: 'security', label: '密钥与加密', desc: '主密钥、口令、密钥包', icon: SafetyOutlined },
  { key: 'network', label: '网络与安全', desc: '代理、客户端证书、Python 授权、运行档位与 AI 权限', icon: GlobalOutlined },
  { key: 'policy', label: '企业策略', desc: 'policy.json 下发的只读能力开关', icon: FileProtectOutlined },
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
const workspaceBusy = ref<'export' | 'import' | ''>('')

const network = reactive<AppSettings>({
  proxyEnabled: false,
  proxyUrl: '',
  certPath: '',
  certPassword: '',
  pythonPath: '',
  pythonEnabled: false,
  performanceMode: 'auto',
  aiPermissionMode: 'confirm_write',
  aiFallbackIds: '',
})
const networkLoading = ref(false)

/* 摘要模型下拉：只列已配置的服务商，0 = 跟随主 provider */
const providers = ref<AiProvider[]>([])
const providersLoading = ref(false)
const summaryProviderOptions = computed(() =>
  providers.value
    .filter((item) => typeof item.id === 'number')
    .map((item) => ({ value: item.id as number, label: `${item.name} · ${item.model}` })),
)

/** 失败回退链：存逗号字符串，界面用数组；顺序即尝试顺序（AI-12） */
const fallbackIds = computed<number[]>({
  get: () =>
    String(network.aiFallbackIds || '')
      .split(',')
      .map((item) => Number(item.trim()))
      .filter((id) => Number.isInteger(id) && id > 0),
  set: (value) => {
    network.aiFallbackIds = value.join(',')
  },
})

async function loadProviders() {
  providersLoading.value = true
  try {
    providers.value = await api.aiProviderList()
  } catch {
    providers.value = []
  } finally {
    providersLoading.value = false
  }
}

const pythonChecking = ref(false)
const pythonCheckResult = ref<PythonCheckResult | null>(null)
const performanceProfile = ref<PerformanceProfile | null>(null)
const policy = ref<PolicySnapshot | null>(null)
const policyLoading = ref(false)

/** 策略里是否至少开了一个限制项，用于标签展示 */
const policyActive = computed(() => {
  const value = policy.value?.policy
  if (!value) return false
  return (
    value.disableAi ||
    value.disableCloudAi ||
    value.disableLanServices ||
    value.disableTelemetry ||
    value.forceEncryptBackup ||
    value.allowedImportFormats.length > 0 ||
    value.retentionDays > 0
  )
})

async function loadPolicy() {
  policyLoading.value = true
  try {
    policy.value = await api.policyGet()
  } catch (e) {
    message.error(String(e))
  } finally {
    policyLoading.value = false
  }
}

async function reloadPolicy() {
  try {
    policy.value = await api.policyReload()
    if (policy.value?.error) {
      message.warning('已重新读取，但策略文件格式有误，当前按默认策略运行')
    } else {
      message.success('企业策略已重新加载')
    }
  } catch (e) {
    message.error(String(e))
  }
}

const memoryText = computed(() => {
  const bytes = performanceProfile.value?.totalMemoryBytes
  if (!bytes) return ''
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)}GB`
})

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
    const [app, dir, settings, profile] = await Promise.all([
      api.appInfo(),
      api.storageInfo(),
      api.settingsGet(),
      api.performanceProfile(),
    ])
    info.value = app
    storage.value = dir
    performanceProfile.value = profile
    Object.assign(network, settings)
  } catch (e) {
    message.error(String(e))
  }
  // 备份与密钥状态各自独立拉取：任一失败不阻塞设置页其它内容
  void loadProviders()
  void loadBackups()
  void loadSecrets()
  void loadPolicy()
})

async function saveNetwork() {
  networkLoading.value = true
  try {
    const saved = await api.settingsSet({ ...network })
    Object.assign(network, saved)
    performanceProfile.value = await api.performanceProfile()
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

async function exportWorkspace() {
  let picked: string | null = null
  try {
    const result = await open({
      directory: true,
      multiple: false,
      title: '选择工作区导出目录',
    })
    picked = typeof result === 'string' ? result : null
  } catch (e) {
    message.error(String(e))
    return
  }
  if (!picked) return
  workspaceBusy.value = 'export'
  try {
    const result = await api.workspaceExportDir({ path: picked })
    const { projects, collections, requests, environments } = result.counts
    message.success(
      `已导出 ${projects} 个项目 / ${collections} 个集合 / ${requests} 个请求 / ${environments} 个环境`,
    )
    if (result.warnings.length) {
      Modal.warning({
        title: '导出完成，但有需要注意的项',
        content: result.warnings.join('；'),
      })
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    workspaceBusy.value = ''
  }
}

async function importWorkspace() {
  let picked: string | null = null
  try {
    const result = await open({
      directory: true,
      multiple: false,
      title: '选择工作区目录（含 manifest.json）',
    })
    picked = typeof result === 'string' ? result : null
  } catch (e) {
    message.error(String(e))
    return
  }
  if (!picked) return
  workspaceBusy.value = 'import'
  try {
    const result = await api.workspaceImportDir({ path: picked })
    const created = result.created
    const updated = result.updated
    const summary = `新增 ${created.projects} 个项目 / ${created.requests} 个请求，更新 ${updated.projects} 个项目 / ${updated.requests} 个请求`
    if (result.conflicts.length) {
      const head = result.conflicts
        .slice(0, 20)
        .map((item) => `${item.kind} #${item.id} ${item.name}：${item.reason}`)
        .join('；')
      const tail = result.conflicts.length > 20 ? `；另有 ${result.conflicts.length - 20} 项` : ''
      Modal.warning({
        title: `导入完成，${result.conflicts.length} 项冲突未覆盖`,
        content: `${head}${tail}`,
      })
    } else {
      message.success(summary)
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    workspaceBusy.value = ''
  }
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

/* ---------- 备份与自检（v1.0.8） ---------- */
const backups = ref<BackupItem[]>([])
const backupLoading = ref(false)
const backupBusy = ref(false)
const backupDir = ref('')
const restoreTarget = ref<BackupItem | null>(null)
const checkResult = ref<CheckResult | null>(null)
const checking = ref(false)

const backupTotalSize = computed(() => backups.value.reduce((sum, item) => sum + item.size, 0))

function fmtBackupSize(bytes: number): string {
  if (!bytes) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

function fmtBackupTime(ts: number): string {
  if (!ts) return '-'
  const d = new Date(ts * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

async function loadBackups() {
  backupLoading.value = true
  try {
    const [list, dir] = await Promise.all([api.backupList(), api.backupDirPath()])
    backups.value = list
    backupDir.value = dir
  } catch (e) {
    message.error(String(e))
  } finally {
    backupLoading.value = false
  }
}

async function createBackup() {
  backupBusy.value = true
  try {
    const item = await api.backupCreate()
    message.success(`已创建备份 ${item.name}`)
    await loadBackups()
  } catch (e) {
    message.error(String(e))
  } finally {
    backupBusy.value = false
  }
}

function askRestore(item: BackupItem) {
  restoreTarget.value = item
}

async function confirmRestore() {
  const item = restoreTarget.value
  if (!item) return
  backupBusy.value = true
  try {
    await api.backupRestore(item.name)
    restoreTarget.value = null
    Modal.confirm({
      title: '恢复已排队',
      content: '已把备份标记为待恢复。重启应用后会在打开数据库之前完成替换，期间你的当前数据会先自动备份一份。',
      okText: '立即重启',
      cancelText: '稍后手动重启',
      onOk: () => restartApp(),
    })
  } catch (e) {
    message.error(String(e))
  } finally {
    backupBusy.value = false
  }
}

async function cancelRestore() {
  try {
    await api.backupCancelRestore()
    message.success('已取消待恢复标记')
  } catch (e) {
    message.error(String(e))
  }
}

async function deleteBackup(item: BackupItem) {
  backupBusy.value = true
  try {
    await api.backupDelete(item.name)
    message.success('已删除备份')
    await loadBackups()
  } catch (e) {
    message.error(String(e))
  } finally {
    backupBusy.value = false
  }
}

async function runCheck(deep: boolean) {
  checking.value = true
  try {
    checkResult.value = await api.dbCheck(deep)
    if (checkResult.value.ok) {
      message.success(`数据库检查通过（${checkResult.value.durationMs} ms）`)
    } else {
      message.error('数据库检查发现问题，详见下方结果')
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    checking.value = false
  }
}

async function exportDiagnostics() {
  try {
    const target = await saveFile({ defaultPath: 'astroman-diagnostics.zip' })
    if (!target) return
    const path = await api.diagnosticsExport(target)
    message.success(`诊断包已导出到 ${path}`)
  } catch (e) {
    message.error(String(e))
  }
}

async function openBackupDir() {
  try {
    const dir = backupDir.value || (await api.backupDirPath())
    await api.storageOpen(dir)
  } catch (e) {
    message.error(String(e))
  }
}

/* ---------- 密钥与加密（v1.0.8） ---------- */
const secrets = ref<SecretStatus | null>(null)
const secretsLoading = ref(false)
const passOpen = ref(false)
const passBusy = ref(false)
const passForm = reactive({ password: '', confirm: '' })
const unlockOpen = ref(false)
const unlockBusy = ref(false)
const unlockPassword = ref('')
const keypackOpen = ref(false)
const keypackBusy = ref(false)
const keypackMode = ref<'export' | 'import'>('export')
const keypackForm = reactive({ password: '', confirm: '' })

/** 口令模式未解锁时，所有敏感值都读不出来 —— 界面上要先提示这件事 */
const secretsLocked = computed(() => secrets.value?.locked === true)

/** 密钥能读到但解不开：多半是钥匙串条目被别的实例覆盖过 */
const mismatchText = computed(() => {
  const count = secrets.value?.brokenSecretCount ?? 0
  const head = count > 0 ? `已保存的 ${count} 条密钥值都解不出来。` : ''
  return `${head}这些 Key 不会自动恢复，需要在「设置 → AI」或环境变量里重新填写一次；如果你导出过密钥包，导入它可以直接解开原有数据。`
})

async function loadSecrets() {
  secretsLoading.value = true
  try {
    secrets.value = await api.secretsStatus()
  } catch (e) {
    message.error(String(e))
  } finally {
    secretsLoading.value = false
  }
}

function openPassModal() {
  passForm.password = ''
  passForm.confirm = ''
  passOpen.value = true
}

async function submitPassphrase() {
  if (passForm.password.length < 8) {
    message.warning('口令至少 8 位')
    return
  }
  if (passForm.password !== passForm.confirm) {
    message.warning('两次输入的口令不一致')
    return
  }
  passBusy.value = true
  try {
    secrets.value = await api.secretsSetPassphrase(passForm.password, passForm.confirm)
    passOpen.value = false
    passForm.password = ''
    passForm.confirm = ''
    message.success('已切换到口令模式，下次启动需要输入这个口令')
  } catch (e) {
    message.error(String(e))
  } finally {
    passBusy.value = false
  }
}

function openUnlock() {
  unlockPassword.value = ''
  unlockOpen.value = true
}

async function submitUnlock() {
  if (!unlockPassword.value) {
    message.warning('请输入口令')
    return
  }
  unlockBusy.value = true
  try {
    secrets.value = await api.secretsUnlock(unlockPassword.value)
    unlockOpen.value = false
    unlockPassword.value = ''
    message.success('已解锁')
  } catch (e) {
    message.error(String(e))
  } finally {
    unlockBusy.value = false
  }
}

function openKeypack(mode: 'export' | 'import') {
  keypackMode.value = mode
  keypackForm.password = ''
  keypackForm.confirm = ''
  keypackOpen.value = true
}

async function submitKeypack() {
  if (!keypackForm.password) {
    message.warning('请设置密钥包口令')
    return
  }
  if (keypackMode.value === 'export' && keypackForm.password !== keypackForm.confirm) {
    message.warning('两次输入的口令不一致')
    return
  }
  keypackBusy.value = true
  try {
    if (keypackMode.value === 'export') {
      const target = await saveFile({ defaultPath: 'astroman-keypack.json' })
      if (!target) return
      const path = await api.secretsExport(keypackForm.password, target)
      keypackOpen.value = false
      message.success(`密钥包已导出到 ${path}`)
    } else {
      const pickedRaw = await open({
        multiple: false,
        filters: [{ name: '密钥包', extensions: ['json'] }],
      })
      const picked = typeof pickedRaw === 'string' ? pickedRaw : ''
      if (!picked) return
      // 先只读预检，避免选错文件把密钥覆盖成垃圾
      const pack = await api.secretsInspectPack(picked)
      Modal.confirm({
        title: '确认导入这个密钥包？',
        content: `来源：${pack.source || '未知'} · 创建于 ${fmtBackupTime(pack.createdAt)}。导入后当前主密钥会被替换，之前用旧密钥加密的值需要重新填写。`,
        okText: '导入',
        cancelText: '取消',
        onOk: async () => {
          secrets.value = await api.secretsImport(keypackForm.password, picked)
          keypackOpen.value = false
          message.success('密钥包已导入')
        },
      })
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    keypackBusy.value = false
  }
}

function askResetSecrets() {
  Modal.confirm({
    title: '重置全部密钥值？',
    content: '会把所有敏感环境变量与 AI 接口 Key 清空，且不可恢复。如果还要继续用这些密钥，请先导出密钥包。',
    okText: '确认清空',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        const count = await api.secretsReset(true)
        secrets.value = await api.secretsStatus()
        message.success(`已清空 ${count} 个密钥值`)
      } catch (e) {
        message.error(String(e))
      }
    },
  })
}

/* ---------- 脚本沙箱超时（v1.0.8） ---------- */
const scriptTimeout = ref(scriptTimeoutMs())

function changeScriptTimeout(value: number | null) {
  if (typeof value !== 'number') return
  setScriptTimeoutMs(value)
  scriptTimeout.value = scriptTimeoutMs()
  message.success(`脚本超时已设为 ${scriptTimeout.value} ms`)
}

/* ---------- 动效偏好（无障碍） ---------- */
const motionOff = ref(document.documentElement.dataset.motion === 'off')

function toggleMotion(value: boolean) {
  motionOff.value = value
  try {
    localStorage.setItem('fm-motion', value ? 'off' : 'on')
  } catch {
    // 存储不可用时仅本次生效
  }
  document.documentElement.dataset.motion = value ? 'off' : 'on'
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

          <div class="group">
            <div class="group-title">动效</div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">关闭界面动画</div>
                <div class="row-desc">关闭过渡与入场动画，低端机或晕动敏感场景使用；系统开启「减少动态效果」时也会自动生效</div>
              </div>
              <div class="row-control">
                <a-switch :checked="motionOff" @change="toggleMotion" />
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

            <div class="group">
              <div class="group-title">Git 友好工作区</div>
              <div class="group-desc">
                把项目、集合、请求与环境导出为可 diff 的 JSON 目录；敏感环境变量只写
                <code v-pre>{{secret:NAME}}</code>
                占位，导入后需要重新补值
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">导出工作区</div>
                  <div class="row-desc">选择目标目录，按 manifest + 项目结构写出，JSON 键顺序固定</div>
                </div>
                <div class="row-control">
                  <a-button
                    size="small"
                    :loading="workspaceBusy === 'export'"
                    :disabled="workspaceBusy !== ''"
                    @click="exportWorkspace"
                  >
                    <template #icon><ExportOutlined /></template>
                    导出
                  </a-button>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">导入工作区</div>
                  <div class="row-desc">选择包含 manifest.json 的目录，按 id 合并；内容冲突只报告、不覆盖</div>
                </div>
                <div class="row-control">
                  <a-button
                    size="small"
                    :loading="workspaceBusy === 'import'"
                    :disabled="workspaceBusy !== ''"
                    @click="importWorkspace"
                  >
                    <template #icon><ImportOutlined /></template>
                    导入
                  </a-button>
                </div>
              </div>
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

        <!-- ============ 备份与自检 ============ -->
        <div v-else-if="active === 'backup'" class="panel">
          <div class="group">
            <div class="group-title">
              数据库快照
              <span class="tag">{{ backups.length }} 份 · {{ fmtBackupSize(backupTotalSize) }}</span>
            </div>
            <div class="group-desc">
              每天启动后自动做一次快照；恢复前会先给你当前的数据再做一份，不会直接覆盖
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">备份目录</div>
                <div class="row-desc mono">{{ backupDir || '-' }}</div>
              </div>
              <div class="row-control">
                <a-button size="small" :disabled="!backupDir" @click="openBackupDir">
                  <template #icon><FolderOpenOutlined /></template>
                  {{ openDirLabel }}
                </a-button>
              </div>
            </div>

            <div class="group-actions">
              <a-button type="primary" :loading="backupBusy" @click="createBackup">
                <template #icon><CloudServerOutlined /></template>
                立即备份
              </a-button>
              <a-button :loading="backupLoading" @click="loadBackups">
                <template #icon><ReloadOutlined /></template>
                刷新列表
              </a-button>
              <a-button :disabled="!backups.length" @click="cancelRestore">
                取消待恢复
              </a-button>
            </div>
          </div>

          <div class="group">
            <div class="group-title">快照列表</div>
            <a-spin :spinning="backupLoading">
              <div v-if="backups.length" class="backup-list">
                <div v-for="item in backups" :key="item.name" class="backup-row">
                  <div class="backup-main">
                    <div class="backup-name mono">{{ item.name }}</div>
                    <div class="backup-meta">
                      <span class="backup-kind" :class="`kind-${item.kind}`">{{ item.kindLabel }}</span>
                      <span>{{ fmtBackupTime(item.createdAt) }}</span>
                      <span>{{ fmtBackupSize(item.size) }}</span>
                      <span>schema v{{ item.schemaVersion }} · {{ item.tables }} 张表</span>
                    </div>
                  </div>
                  <div class="backup-actions">
                    <a-button size="small" @click="askRestore(item)">恢复</a-button>
                    <a-button size="small" type="text" danger @click="deleteBackup(item)">删除</a-button>
                  </div>
                </div>
              </div>
              <div v-else class="empty-line">
                还没有备份。点「立即备份」现在做一份，或者在下次启动时等自动快照。
              </div>
            </a-spin>
          </div>

          <RemoteBackupPanel />

          <div class="group">
            <div class="group-title">数据库体检</div>
            <div class="group-desc">
              快速检查秒级完成（每次启动自动跑）；完整性检查会逐页校验，数据量大时需要几分钟
            </div>
            <div class="group-actions">
              <a-button size="small" :loading="checking" @click="runCheck(false)">快速检查</a-button>
              <a-button size="small" :loading="checking" danger @click="runCheck(true)">
                <template #icon><ExperimentOutlined /></template>
                完整性检查
              </a-button>
            </div>
            <div v-if="checkResult" class="check-result" :class="{ bad: !checkResult.ok }">
              <div class="check-line">
                <span class="check-dot" />
                {{ checkResult.ok ? '检查通过' : '发现问题' }}
                <span class="mono">· {{ checkResult.durationMs }} ms · schema v{{ checkResult.schemaVersion }} · {{ checkResult.tables }} 张表</span>
              </div>
              <pre v-if="!checkResult.ok" class="check-detail mono">{{ checkResult.detail }}</pre>
            </div>
          </div>

          <div class="group">
            <div class="group-title">诊断包</div>
            <div class="group-desc">
              导出日志片段、环境信息与表统计，用于排查问题；不含请求正文与密钥
            </div>
            <div class="group-actions">
              <a-button size="small" @click="exportDiagnostics">
                <template #icon><CopyOutlined /></template>
                导出诊断包
              </a-button>
            </div>
          </div>
        </div>

        <!-- ============ 密钥与加密 ============ -->
        <div v-else-if="active === 'security'" class="panel">
          <a-spin :spinning="secretsLoading">
            <div class="group">
              <div class="group-title">
                主密钥
                <span class="tag" :class="{ accent: secrets && !secrets.degraded, warn: secrets?.degraded }">
                  {{ secrets?.sourceLabel || '读取中' }}
                </span>
              </div>
              <div class="group-desc">
                敏感环境变量与 AI 接口 Key 都用主密钥加密后再入库，库里的密文搬到别的列会解密失败
              </div>

              <a-alert
                v-if="secretsLocked"
                type="warning"
                show-icon
                class="sec-alert"
                message="主密钥未解锁"
                :description="secrets?.lockedReason || '口令模式下需要先输入口令，敏感值才能读取；解锁前相关请求会因为变量为空而失败。'"
                action
              >
                <template #action>
                  <a-button size="small" type="primary" @click="openUnlock">解锁</a-button>
                </template>
              </a-alert>

              <a-alert
                v-if="secrets?.degraded"
                type="warning"
                show-icon
                class="sec-alert"
                message="当前处于降级模式，等同于没有加密"
                description="系统钥匙串不可用，主密钥落在数据目录的 keys/master.key 里。任何能读这个目录的程序都能拿到你的密钥。"
              />

              <a-alert
                v-if="secrets?.keyMismatch"
                type="error"
                show-icon
                class="sec-alert"
                message="主密钥与已保存的密钥值不匹配"
                :description="mismatchText"
                action
              >
                <template #action>
                  <a-button size="small" @click="openKeypack('import')">导入密钥包</a-button>
                </template>
              </a-alert>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">加密覆盖</div>
                  <div class="row-desc">
                    环境变量 {{ secrets?.encryptedEnvCount ?? 0 }} 条 · AI 接口 Key {{ secrets?.encryptedAiCount ?? 0 }} 条
                  </div>
                </div>
                <div class="row-control">
                  <a-button size="small" :loading="secretsLoading" @click="loadSecrets">
                    <template #icon><ReloadOutlined /></template>
                    刷新
                  </a-button>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">系统钥匙串</div>
                  <div class="row-desc">
                    {{ secrets?.keyringAvailable ? '可用（推荐）' : '不可用，已降级到文件模式' }}
                  </div>
                </div>
              </div>
            </div>

            <div class="group">
              <div class="group-title">口令模式</div>
              <div class="group-desc">
                用你自己的口令派生主密钥，密钥不落盘。适合把数据目录放在移动硬盘或同步盘的情况。
                <br />
                注意：忘记口令无法找回，只能重置后重新填写所有密钥。
              </div>
              <div class="group-actions">
                <a-button
                  size="small"
                  type="primary"
                  :disabled="secrets?.source === 'passphrase'"
                  @click="openPassModal"
                >
                  <template #icon><KeyOutlined /></template>
                  {{ secrets?.source === 'passphrase' ? '已启用口令模式' : '切换到口令模式' }}
                </a-button>
                <a-button v-if="secretsLocked" size="small" @click="openUnlock">解锁</a-button>
              </div>
            </div>

            <div class="group">
              <div class="group-title">密钥包</div>
              <div class="group-desc">
                换电脑或重装前先导出密钥包，否则之前加密的密钥值在新机器上解不开
              </div>
              <div class="group-actions">
                <a-button size="small" @click="openKeypack('export')">导出密钥包</a-button>
                <a-button size="small" @click="openKeypack('import')">导入密钥包</a-button>
              </div>
            </div>

            <div class="group">
              <div class="group-title">脚本沙箱</div>
              <div class="group-desc">
                前置 / 后置脚本在独立的 Worker 里执行，超时会被强制终止，本次脚本写入的变量会回滚
              </div>
              <div class="row">
                <div class="row-label">
                  <div class="row-title">脚本超时</div>
                  <div class="row-desc">超过这个时间直接终止本次脚本，避免死循环卡住界面</div>
                </div>
                <div class="row-control">
                  <a-input-number
                    :value="scriptTimeout"
                    :min="SCRIPT_TIMEOUT_RANGE.min"
                    :max="SCRIPT_TIMEOUT_RANGE.max"
                    :step="500"
                    size="small"
                    style="width: 120px"
                    @change="changeScriptTimeout"
                  />
                  <span class="mono unit">ms</span>
                </div>
              </div>
            </div>

            <div class="group danger-group">
              <div class="group-title">重置</div>
              <div class="group-desc">清空全部密钥值。此操作不可撤销</div>
              <div class="row">
                <div class="row-label">
                  <div class="row-title">清空所有密钥值</div>
                  <div class="row-desc">环境变量里的 secret 与 AI 接口 Key 会被置空</div>
                </div>
                <div class="row-control">
                  <a-button danger size="small" @click="askResetSecrets">重置密钥</a-button>
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

          <div class="group">
            <div class="group-title">运行档位</div>
            <div class="group-desc">控制抓包保留条数与响应体内存预算；低内存档适合 4GB 办公机</div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">性能模式</div>
                <div class="row-desc">
                  自动：≤6GB 用低内存，≥16GB 用高档，其余标准；手动选择后不再随内存变化
                </div>
              </div>
              <div class="row-control wide">
                <a-select v-model:value="network.performanceMode" size="small" style="width: 200px">
                  <a-select-option value="auto">自动（按内存）</a-select-option>
                  <a-select-option value="low">低内存（4GB 档）</a-select-option>
                  <a-select-option value="standard">标准</a-select-option>
                </a-select>
              </div>
            </div>

            <div v-if="performanceProfile" class="row">
              <div class="row-label">
                <div class="row-title">当前生效</div>
                <div class="row-desc">
                  档位 {{ performanceProfile.tier }}<template v-if="memoryText"> · 本机 {{ memoryText }}</template>
                </div>
              </div>
              <div class="row-control wide">
                <span class="mono">{{ performanceProfile.description }}</span>
              </div>
            </div>
          </div>

          <div class="group">
            <div class="group-title">AI 权限</div>
            <div class="group-desc">
              控制航仔能把操作执行到哪一步；生产环境无论哪种模式都会二次确认，无法跳过
            </div>

            <div class="row">
              <div class="row-label">
                <div class="row-title">AI 权限模式</div>
                <div class="row-desc">
                  只读：拒绝全部写工具与非只读请求；确认写：写操作与 POST / PUT / DELETE 需确认（默认）；
                  受信：自建测试环境不再逐次确认，但写工具仍需确认
                </div>
              </div>
              <div class="row-control wide">
                <a-select v-model:value="network.aiPermissionMode" size="small" style="width: 220px">
                  <a-select-option value="readonly">只读</a-select-option>
                  <a-select-option value="confirm_write">确认写（默认）</a-select-option>
                  <a-select-option value="trusted">受信（仅自建环境）</a-select-option>
                </a-select>
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

        <!-- ============ 企业策略 ============ -->
        <div v-else-if="active === 'policy'" class="panel">
          <a-spin :spinning="policyLoading">
            <div class="group">
              <div class="group-title">
                企业策略
                <span class="tag" :class="{ accent: policyActive }">
                  {{ policyActive ? '已启用限制' : '默认（不限制）' }}
                </span>
              </div>
              <div class="group-desc">
                由管理员在数据目录下放置 policy.json 下发；这里只读展示。改完文件后点「重新读取」，无需重启应用。
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">策略文件</div>
                  <div class="row-desc mono">{{ policy?.path || '-' }}</div>
                </div>
                <div class="row-control">
                  <a-button size="small" @click="copy(policy?.path)">
                    <template #icon><CopyOutlined /></template>
                    复制路径
                  </a-button>
                  <a-button size="small" @click="reloadPolicy">
                    <template #icon><ReloadOutlined /></template>
                    重新读取
                  </a-button>
                </div>
              </div>

              <div v-if="policy?.error" class="callout" style="color: var(--fm-danger, #d4380d)">
                <SafetyCertificateOutlined />
                <span>{{ policy.error }}</span>
              </div>
            </div>

            <div class="group">
              <div class="group-title">能力开关</div>
              <div class="row">
                <div class="row-label">
                  <div class="row-title">AI 助手</div>
                  <div class="row-desc">disableAi：关闭后航仔、模型列表与连通性测试都不再可用</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.disableAi }">
                    {{ policy?.policy.disableAi ? '已禁用' : '可用' }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">云端模型</div>
                  <div class="row-desc">disableCloudAi：只允许 127.0.0.1 / localhost 上的本地模型</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.disableCloudAi }">
                    {{ policy?.policy.disableCloudAi ? '仅本地模型' : '允许云端' }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">局域网服务</div>
                  <div class="row-desc">disableLanServices：关闭后 Mock / 文档站只能监听 127.0.0.1</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.disableLanServices }">
                    {{ policy?.policy.disableLanServices ? '仅本机' : '允许局域网' }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">对外遥测</div>
                  <div class="row-desc">disableTelemetry：当前版本没有遥测，保留给后续版本</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.disableTelemetry }">
                    {{ policy?.policy.disableTelemetry ? '已禁用' : '未限制' }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">异地备份加密</div>
                  <div class="row-desc">forceEncryptBackup：上传备份时必须设置口令，远端始终保存加密包</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.forceEncryptBackup }">
                    {{ policy?.policy.forceEncryptBackup ? '强制加密' : '按用户选择' }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">允许的导入格式</div>
                  <div class="row-desc">allowedImportFormats：空表示不限制；可选 curl / postman / insomnia / openapi / har / bruno</div>
                </div>
                <div class="row-control">
                  <span v-if="!policy?.policy.allowedImportFormats.length" class="tag">不限制</span>
                  <span
                    v-for="format in policy?.policy.allowedImportFormats ?? []"
                    :key="format"
                    class="tag accent"
                  >
                    {{ format }}
                  </span>
                </div>
              </div>

              <div class="row">
                <div class="row-label">
                  <div class="row-title">请求历史保留</div>
                  <div class="row-desc">retentionDays：0 表示只按 10 万条上限保留</div>
                </div>
                <div class="row-control">
                  <span class="tag" :class="{ accent: policy?.policy.retentionDays }">
                    {{ policy?.policy.retentionDays ? `${policy.policy.retentionDays} 天` : '不按时间清理' }}
                  </span>
                </div>
              </div>
            </div>
          </a-spin>
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
            <div class="row">
              <div class="row-label">
                <div class="row-title">摘要模型</div>
                <div class="row-desc">
                  历史变长后异步压缩旧对话；未选择时跟随主模型，摘要失败自动回退主模型
                </div>
              </div>
              <div class="row-control wide">
                <a-select
                  v-model:value="network.summaryProviderId"
                  size="small"
                  style="width: 260px"
                  :loading="providersLoading"
                  placeholder="跟随主模型"
                >
                  <a-select-option :value="0">跟随主模型</a-select-option>
                  <a-select-option
                    v-for="item in summaryProviderOptions"
                    :key="item.value"
                    :value="item.value"
                  >
                    {{ item.label }}
                  </a-select-option>
                </a-select>
              </div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">备用模型</div>
                <div class="row-desc">
                  主模型在首字之前失败（限流 / 服务异常 / 网络错误）时按选择顺序切换，默认关闭；
                  一旦已开始输出就不会中途换模型
                </div>
              </div>
              <div class="row-control wide">
                <a-select
                  v-model:value="fallbackIds"
                  mode="multiple"
                  size="small"
                  style="width: 320px"
                  :loading="providersLoading"
                  :max-tag-count="3"
                  placeholder="不启用回退"
                  allow-clear
                >
                  <a-select-option
                    v-for="item in summaryProviderOptions"
                    :key="item.value"
                    :value="item.value"
                  >
                    {{ item.label }}
                  </a-select-option>
                </a-select>
              </div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">月度预算</div>
                <div class="row-desc">
                  按自然月累计模型费用，0 表示不限制；金额以美元计（约 ¥7.2 / 美元）
                </div>
              </div>
              <div class="row-control wide">
                <a-input-number
                  v-model:value="network.monthlyBudget"
                  :min="0"
                  :precision="2"
                  :step="1"
                  size="small"
                  style="width: 160px"
                />
              </div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">预算提醒</div>
                <div class="row-desc">用量达到预算 80% 时提示一次，不打断当前对话</div>
              </div>
              <div class="row-control">
                <a-switch
                  v-model:checked="network.budgetAlert"
                  size="small"
                  :disabled="!network.monthlyBudget"
                />
              </div>
            </div>
            <div class="row">
              <div class="row-label">
                <div class="row-title">用尽停止</div>
                <div class="row-desc">关闭时只提醒；开启后预算用完会拒绝新的对话轮次</div>
              </div>
              <div class="row-control">
                <a-switch
                  v-model:checked="network.budgetHardStop"
                  size="small"
                  :disabled="!network.monthlyBudget"
                />
              </div>
            </div>
            <div class="group-actions">
              <a-button type="primary" @click="router.push({ name: 'ai' })">打开航仔</a-button>
              <a-button :loading="networkLoading" @click="saveNetwork">保存设置</a-button>
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
            <div v-if="info?.webviewVersion" class="row">
              <div class="row-label">
                <div class="row-title">WebView2 / WebKit</div>
                <div class="row-desc">渲染内核版本，排查兼容性问题时提供给支持人员</div>
              </div>
              <div class="row-control mono">{{ info.webviewVersion }}</div>
            </div>
            <div v-if="info?.dataDirRisks?.length" class="callout">
              <SafetyCertificateOutlined />
              <span>
                数据目录风险：{{ info.dataDirRisks.join('；') }}
              </span>
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

    <!-- ============ v1.0.8 弹窗：恢复确认 / 口令 / 解锁 / 密钥包 ============ -->
    <a-modal
      :open="!!restoreTarget"
      title="从备份恢复"
      ok-text="排队恢复"
      cancel-text="取消"
      :confirm-loading="backupBusy"
      @ok="confirmRestore"
      @cancel="restoreTarget = null"
    >
      <div class="modal-body">
        <p>
          即将用 <span class="mono">{{ restoreTarget?.name }}</span>
          （{{ fmtBackupTime(restoreTarget?.createdAt || 0) }} 的快照）替换当前数据库。
        </p>
        <p class="modal-note">
          恢复会在重启应用时、打开数据库之前完成；替换前会先把当前数据备份成一份 pre-restore 快照，
          所以这一步本身是可回退的。
        </p>
      </div>
    </a-modal>

    <a-modal
      v-model:open="passOpen"
      title="切换到口令模式"
      ok-text="启用"
      cancel-text="取消"
      :confirm-loading="passBusy"
      @ok="submitPassphrase"
    >
      <div class="modal-body">
        <p class="modal-note">
          口令用来派生主密钥，不会保存在任何地方。忘记口令就无法解开已加密的密钥值，
          只能用「重置密钥」重新填写，或提前导出的密钥包恢复。
        </p>
        <a-input-password
          v-model:value="passForm.password"
          placeholder="口令（至少 8 位）"
          autocomplete="new-password"
        />
        <a-input-password
          v-model:value="passForm.confirm"
          placeholder="再次输入口令"
          autocomplete="new-password"
        />
      </div>
    </a-modal>

    <a-modal
      v-model:open="unlockOpen"
      title="解锁主密钥"
      ok-text="解锁"
      cancel-text="取消"
      :confirm-loading="unlockBusy"
      @ok="submitUnlock"
    >
      <div class="modal-body">
        <p class="modal-note">输入启动时设置的口令，解锁后本次运行期间敏感值可用。</p>
        <a-input-password
          v-model:value="unlockPassword"
          placeholder="口令"
          autocomplete="current-password"
          @press-enter="submitUnlock"
        />
      </div>
    </a-modal>

    <a-modal
      v-model:open="keypackOpen"
      :title="keypackMode === 'export' ? '导出密钥包' : '导入密钥包'"
      :ok-text="keypackMode === 'export' ? '导出' : '选择文件并导入'"
      cancel-text="取消"
      :confirm-loading="keypackBusy"
      @ok="submitKeypack"
    >
      <div class="modal-body">
        <p class="modal-note">
          <template v-if="keypackMode === 'export'">
            密钥包用你设置的口令加密，导出后请和数据库分开保存 —— 两者放在一起就失去了保护意义。
          </template>
          <template v-else>
            导入会用密钥包里的主密钥替换当前主密钥，随后自动用新密钥重新加密已有密钥值。
          </template>
        </p>
        <a-input-password
          v-model:value="keypackForm.password"
          :placeholder="keypackMode === 'export' ? '设置密钥包口令' : '密钥包口令'"
          autocomplete="off"
        />
        <a-input-password
          v-if="keypackMode === 'export'"
          v-model:value="keypackForm.confirm"
          placeholder="再次输入口令"
          autocomplete="off"
        />
      </div>
    </a-modal>
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

/* ---------- v1.0.8：备份与自检 ---------- */
.backup-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.backup-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 11px;
  border-radius: 7px;
  border: 1px solid var(--fm-border, rgba(0, 0, 0, 0.07));
  background: var(--fm-surface-2, rgba(0, 0, 0, 0.02));
}

.backup-main {
  flex: 1 1 auto;
  min-width: 0;
}

.backup-name {
  font-size: 12px;
  word-break: break-all;
}

.backup-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 4px;
  font-size: 11px;
  color: var(--fm-text-3, #999);
}

.backup-kind {
  padding: 0 6px;
  border-radius: 4px;
  font-size: 10px;
  line-height: 16px;
  background: rgba(128, 128, 128, 0.14);
  color: var(--fm-text-2, #666);
}

.backup-kind.kind-daily {
  background: rgba(14, 165, 233, 0.14);
  color: #0284c7;
}

.backup-kind.kind-weekly {
  background: rgba(139, 92, 246, 0.14);
  color: #7c3aed;
}

.backup-kind.kind-manual {
  background: rgba(16, 185, 129, 0.14);
  color: #059669;
}

.backup-kind.kind-pre-restore,
.backup-kind.kind-pre-migrate {
  background: rgba(217, 119, 6, 0.14);
  color: #b45309;
}

.backup-actions {
  display: flex;
  gap: 4px;
  flex: 0 0 auto;
}

.empty-line {
  padding: 14px 0;
  font-size: 12px;
  color: var(--fm-text-3, #999);
}

.check-result {
  margin-top: 10px;
  padding: 9px 11px;
  border-radius: 7px;
  background: rgba(16, 185, 129, 0.1);
  border: 1px solid rgba(16, 185, 129, 0.28);
}

.check-result.bad {
  background: rgba(220, 38, 38, 0.08);
  border-color: rgba(220, 38, 38, 0.28);
}

.check-line {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  color: #047857;
}

.check-result.bad .check-line {
  color: #b91c1c;
}

.check-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
}

.check-detail {
  margin: 6px 0 0;
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: #b91c1c;
  max-height: 160px;
  overflow: auto;
}

/* ---------- v1.0.8：密钥与加密 ---------- */
.sec-alert {
  margin: 8px 0;
}

.tag.warn {
  color: #b45309;
  background: rgba(217, 119, 6, 0.12);
  border-color: rgba(217, 119, 6, 0.3);
}

.unit {
  font-size: 11px;
  color: var(--fm-text-3, #999);
  margin-left: 6px;
}

/* ---------- v1.0.8：设置页弹窗 ---------- */
.modal-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  font-size: 13px;
  line-height: 1.7;
}

.modal-note {
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text-3, #999);
  margin: 0;
}
</style>
