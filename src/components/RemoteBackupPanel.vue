<script setup lang="ts">
//! W2 G-06：异地加密备份目标管理
//!
//! 口令只随单次上传 / 恢复提交，不落库；列表只显示「已保存密码」。
import { computed, onMounted, reactive, ref } from 'vue'
import { message, Modal } from 'ant-design-vue'
import {
  DeleteOutlined,
  EditOutlined,
  ExperimentOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  UploadOutlined,
  CloudDownloadOutlined,
  PlusOutlined,
} from '@ant-design/icons-vue'
import * as api from '@/api'
import type {
  BackupTarget,
  BackupTargetKind,
  RemoteBackup,
} from '@/types/models'

const targets = ref<BackupTarget[]>([])
const loading = ref(false)
const busy = ref(false)

const editorOpen = ref(false)
const editorForm = reactive({
  id: 0,
  name: '',
  kind: 'local' as BackupTargetKind,
  url: '',
  username: '',
  password: '',
  dir: '',
  enabled: true,
})
const editing = computed(() => editorForm.id > 0)

const passOpen = ref(false)
const passMode = ref<'upload' | 'restore'>('upload')
const passTarget = ref<BackupTarget | null>(null)
const passRemoteName = ref('')
const passphrase = ref('')
const passphraseConfirm = ref('')

const remoteOpen = ref(false)
const remoteTarget = ref<BackupTarget | null>(null)
const remoteItems = ref<RemoteBackup[]>([])
const remoteLoading = ref(false)

function fmtSize(bytes: number): string {
  if (!bytes) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

function fmtTime(ts?: number | null): string {
  if (!ts) return '-'
  return new Date(ts).toLocaleString()
}

async function loadTargets() {
  loading.value = true
  try {
    targets.value = await api.backupTargetList()
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

function resetForm() {
  editorForm.id = 0
  editorForm.name = ''
  editorForm.kind = 'local'
  editorForm.url = ''
  editorForm.username = ''
  editorForm.password = ''
  editorForm.dir = ''
  editorForm.enabled = true
}

function openCreate() {
  resetForm()
  editorOpen.value = true
}

function openEdit(item: BackupTarget) {
  editorForm.id = item.id
  editorForm.name = item.name
  editorForm.kind = (item.kind as BackupTargetKind) || 'local'
  editorForm.url = item.url
  editorForm.username = item.username
  editorForm.password = ''
  editorForm.dir = item.dir
  editorForm.enabled = item.enabled
  editorOpen.value = true
}

async function saveTarget() {
  if (!editorForm.name.trim()) {
    message.warning('请填写备份目标名称')
    return
  }
  busy.value = true
  try {
    await api.backupTargetSave({
      id: editorForm.id || undefined,
      name: editorForm.name.trim(),
      kind: editorForm.kind,
      url: editorForm.url.trim(),
      username: editorForm.username.trim(),
      password: editorForm.password.trim() ? editorForm.password : undefined,
      dir: editorForm.dir.trim(),
      enabled: editorForm.enabled,
    })
    message.success(editing.value ? '已保存备份目标' : '已新建备份目标')
    editorOpen.value = false
    await loadTargets()
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

function removeTarget(item: BackupTarget) {
  Modal.confirm({
    title: '删除备份目标',
    content: `确定删除「${item.name}」？已上传到远端的备份文件不会被删除。`,
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        await api.backupTargetDelete(item.id)
        message.success('已删除备份目标')
        await loadTargets()
      } catch (e) {
        message.error(String(e))
      }
    },
  })
}

async function testTarget(item: BackupTarget) {
  busy.value = true
  try {
    const report = await api.backupTargetTest(item.id)
    if (report.ok) message.success(report.detail)
    else message.error(report.detail)
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

function openUpload(item: BackupTarget) {
  passMode.value = 'upload'
  passTarget.value = item
  passRemoteName.value = ''
  passphrase.value = ''
  passphraseConfirm.value = ''
  passOpen.value = true
}

function openRestore(item: BackupTarget, name: string) {
  passMode.value = 'restore'
  passTarget.value = item
  passRemoteName.value = name
  passphrase.value = ''
  passphraseConfirm.value = ''
  passOpen.value = true
}

async function submitPassphrase() {
  const target = passTarget.value
  if (!target) return
  const secret = passphrase.value.trim()
  if (secret.length < 8) {
    message.warning('口令至少 8 位；口令丢失将无法恢复备份')
    return
  }
  if (passMode.value === 'upload' && secret !== passphraseConfirm.value.trim()) {
    message.warning('两次输入的口令不一致')
    return
  }
  busy.value = true
  try {
    if (passMode.value === 'upload') {
      const result = await api.backupUpload(target.id, secret)
      message.success(`已上传 ${result.remoteName}（${fmtSize(result.encryptedSize)}）`)
      passOpen.value = false
      await loadTargets()
    } else {
      await api.backupRestoreFromTarget(target.id, passRemoteName.value, secret)
      passOpen.value = false
      Modal.confirm({
        title: '恢复已排队',
        content:
          '已从异地备份解密并校验通过。重启应用后会在打开数据库之前完成替换，期间当前数据会先自动备份一份。',
        okText: '立即重启',
        cancelText: '稍后手动重启',
        onOk: async () => {
          try {
            await api.appRestart()
          } catch (e) {
            message.error(String(e))
          }
        },
      })
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    busy.value = false
  }
}

async function openRemote(item: BackupTarget) {
  remoteTarget.value = item
  remoteOpen.value = true
  await loadRemote()
}

async function loadRemote() {
  const target = remoteTarget.value
  if (!target) return
  remoteLoading.value = true
  try {
    remoteItems.value = await api.backupTargetRemoteList(target.id)
  } catch (e) {
    message.error(String(e))
    remoteItems.value = []
  } finally {
    remoteLoading.value = false
  }
}

function restoreFromRemote(item: RemoteBackup) {
  const target = remoteTarget.value
  if (!target) return
  remoteOpen.value = false
  openRestore(target, item.name)
}

onMounted(loadTargets)
</script>

<template>
  <div class="remote-panel">
    <div class="head">
      <div>
        <div class="title">异地加密备份</div>
        <div class="desc">
          上传前用独立口令加密（AES-256-GCM + Argon2id），口令不落库；远端保留最新 7 份或 90 天
        </div>
      </div>
      <div class="actions">
        <a-button size="small" type="primary" @click="openCreate">
          <template #icon><PlusOutlined /></template>
          新建目标
        </a-button>
        <a-button size="small" :loading="loading" @click="loadTargets">
          <template #icon><ReloadOutlined /></template>
          刷新
        </a-button>
      </div>
    </div>

    <a-spin :spinning="loading">
      <div v-if="targets.length" class="list">
        <div v-for="item in targets" :key="item.id" class="row">
          <div class="main">
            <div class="name">
              {{ item.name }}
              <span class="kind">{{ item.kindLabel }}</span>
              <span v-if="!item.enabled" class="off">已停用</span>
            </div>
            <div class="meta">
              <span class="mono">{{ item.kind === 'webdav' ? item.url : item.dir }}</span>
              <span v-if="item.hasPassword">已保存密码</span>
              <span v-if="item.lastRunAt">上次 {{ fmtTime(item.lastRunAt) }}</span>
            </div>
            <div v-if="item.lastStatus" class="status" :class="{ fail: item.lastStatus.startsWith('fail') }">
              {{ item.lastStatus }}
            </div>
          </div>
          <div class="ops">
            <a-button size="small" :loading="busy" @click="testTarget(item)">
              <template #icon><ExperimentOutlined /></template>
              测试
            </a-button>
            <a-button size="small" :disabled="!item.enabled" @click="openUpload(item)">
              <template #icon><UploadOutlined /></template>
              上传
            </a-button>
            <a-button size="small" @click="openRemote(item)">
              <template #icon><CloudDownloadOutlined /></template>
              远端备份
            </a-button>
            <a-button size="small" type="text" @click="openEdit(item)">
              <template #icon><EditOutlined /></template>
            </a-button>
            <a-button size="small" type="text" danger @click="removeTarget(item)">
              <template #icon><DeleteOutlined /></template>
            </a-button>
          </div>
        </div>
      </div>
      <div v-else class="empty">
        还没有异地备份目标。可以选一个本地目录，或填写 WebDAV 地址。
      </div>
    </a-spin>

    <a-modal
      v-model:open="editorOpen"
      :title="editing ? '编辑备份目标' : '新建备份目标'"
      :confirm-loading="busy"
      width="560px"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveTarget"
    >
      <a-form layout="vertical" class="form">
        <a-form-item label="名称" required>
          <a-input v-model:value="editorForm.name" placeholder="例如：家里 NAS / 移动硬盘" />
        </a-form-item>
        <a-form-item label="类型">
          <a-radio-group v-model:value="editorForm.kind">
            <a-radio-button value="local">本地目录</a-radio-button>
            <a-radio-button value="webdav">WebDAV</a-radio-button>
          </a-radio-group>
        </a-form-item>
        <template v-if="editorForm.kind === 'local'">
          <a-form-item label="目录" required>
            <a-input v-model:value="editorForm.dir" placeholder="D:\\AstroMan-backups" />
          </a-form-item>
        </template>
        <template v-else>
          <a-form-item label="WebDAV 地址" required>
            <a-input v-model:value="editorForm.url" placeholder="https://nas.example.com/dav" />
          </a-form-item>
          <a-form-item label="用户名">
            <a-input v-model:value="editorForm.username" autocomplete="off" />
          </a-form-item>
          <a-form-item label="密码">
            <a-input-password
              v-model:value="editorForm.password"
              :placeholder="editing ? '留空表示保留原密码' : '用于 WebDAV 认证'"
              autocomplete="new-password"
            />
          </a-form-item>
        </template>
        <a-form-item label="目录（可选）">
          <a-input v-model:value="editorForm.dir" placeholder="备份子目录，例如 astroman" />
        </a-form-item>
        <a-form-item label="启用">
          <a-switch v-model:checked="editorForm.enabled" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal
      v-model:open="passOpen"
      :title="passMode === 'upload' ? `上传加密备份 · ${passTarget?.name || ''}` : `恢复 · ${passRemoteName}`"
      :confirm-loading="busy"
      width="480px"
      :ok-text="passMode === 'upload' ? '开始上传' : '解密并排队恢复'"
      cancel-text="取消"
      @ok="submitPassphrase"
    >
      <div class="pass-hint">
        备份口令独立于应用主密钥，请妥善保存；<b>口令丢失后备份无法恢复</b>。
      </div>
      <a-form layout="vertical">
        <a-form-item label="备份口令（至少 8 位）" required>
          <a-input-password v-model:value="passphrase" autocomplete="new-password" />
        </a-form-item>
        <a-form-item v-if="passMode === 'upload'" label="再次输入" required>
          <a-input-password v-model:value="passphraseConfirm" autocomplete="new-password" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal
      v-model:open="remoteOpen"
      :title="`远端备份 · ${remoteTarget?.name || ''}`"
      width="720px"
      :footer="null"
    >
      <div class="remote-toolbar">
        <span class="muted">显示最新 7 份 / 90 天内保留的加密包</span>
        <a-button size="small" :loading="remoteLoading" @click="loadRemote">
          <template #icon><ReloadOutlined /></template>
          刷新
        </a-button>
      </div>
      <a-spin :spinning="remoteLoading">
        <div v-if="remoteItems.length" class="remote-list">
          <div v-for="item in remoteItems" :key="item.name" class="remote-row">
            <div class="remote-main">
              <div class="mono">{{ item.name }}</div>
              <div class="muted">{{ fmtSize(item.size) }} · {{ fmtTime(item.modifiedAt) }}</div>
            </div>
            <a-button size="small" type="primary" ghost @click="restoreFromRemote(item)">
              <template #icon><FolderOpenOutlined /></template>
              恢复
            </a-button>
          </div>
        </div>
        <div v-else class="empty">远端还没有加密备份</div>
      </a-spin>
    </a-modal>
  </div>
</template>

<style scoped>
.remote-panel {
  border-top: 1px dashed var(--fm-border, #e5e7eb);
  padding-top: 16px;
  margin-top: 16px;
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}
.title {
  font-size: 14px;
  font-weight: 600;
  color: var(--fm-text, #111827);
}
.desc {
  margin-top: 4px;
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
  line-height: 1.6;
}
.actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 2px;
  background: #fff;
}
.main {
  min-width: 0;
}
.name {
  font-weight: 600;
  color: var(--fm-text, #111827);
  display: flex;
  align-items: center;
  gap: 8px;
}
.kind {
  font-size: 11px;
  font-weight: 500;
  padding: 1px 6px;
  border-radius: 2px;
  background: #f0fdf4;
  color: #15803d;
}
.off {
  font-size: 11px;
  color: #b45309;
}
.meta {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 4px;
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
}
.status {
  margin-top: 4px;
  font-size: 12px;
  color: #15803d;
  word-break: break-all;
}
.status.fail {
  color: #b91c1c;
}
.ops {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.empty {
  padding: 18px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
  border: 1px dashed var(--fm-border, #e5e7eb);
  border-radius: 2px;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  word-break: break-all;
}
.form :deep(.ant-form-item) {
  margin-bottom: 12px;
}
.pass-hint {
  margin-bottom: 12px;
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.6;
  color: #92400e;
  background: #fffbeb;
  border: 1px solid #fde68a;
  border-radius: 2px;
}
.remote-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.muted {
  font-size: 12px;
  color: var(--fm-text-secondary, #6b7280);
}
.remote-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 60vh;
  overflow-y: auto;
}
.remote-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 10px;
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 2px;
}
.remote-main {
  min-width: 0;
}
</style>
