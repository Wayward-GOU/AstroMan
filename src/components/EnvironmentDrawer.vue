<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { CheckCircleFilled, PlusOutlined } from '@ant-design/icons-vue'
import * as api from '@/api'
import type { Environment, EnvVariable } from '@/types/models'

const props = defineProps<{
  open: boolean
  projectId: number | null
  /** 请求页当前选中的环境，打开抽屉时优先定位到它 */
  activeId: number | null
}>()

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'select', id: number | null): void
  (e: 'changed', environments: Environment[]): void
}>()

const environments = ref<Environment[]>([])
const editingId = ref<number | null>(null)
const variables = ref<EnvVariable[]>([])
const envScript = ref('')
const loading = ref(false)
const saving = ref(false)
const scriptSaving = ref(false)
const creating = ref(false)
const newName = ref('')
const showSecret = ref<Record<number, boolean>>({})

const editing = computed(() => environments.value.find((e) => e.id === editingId.value) ?? null)

const envOptions = computed(() =>
  environments.value.map((e) => ({
    value: e.id,
    label: e.isActive ? `${e.name}（当前）` : e.name,
  })),
)

watch(
  () => props.open,
  (open) => {
    if (open) void load()
  },
)

watch(
  () => props.projectId,
  () => {
    if (props.open) void load()
  },
)

async function load() {
  if (!props.projectId) return
  loading.value = true
  try {
    const list = await api.envList(props.projectId)
    environments.value = list
    const target =
      (props.activeId && list.some((e) => e.id === props.activeId) ? props.activeId : null) ??
      (editingId.value && list.some((e) => e.id === editingId.value) ? editingId.value : null) ??
      list.find((e) => e.isActive)?.id ??
      list[0]?.id ??
      null
    pick(target)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

/** 重新拉取环境，并把列表同步给请求页 */
async function reload(keepId: number | null) {
  if (!props.projectId) return
  const list = await api.envList(props.projectId)
  environments.value = list
  const next =
    (keepId && list.some((e) => e.id === keepId) ? keepId : null) ??
    list.find((e) => e.isActive)?.id ??
    list[0]?.id ??
    null
  pick(next)
  emit('changed', list)
}

function pick(id: number | null | undefined) {
  const env = environments.value.find((e) => e.id === id) ?? null
  editingId.value = env?.id ?? null
  variables.value = (env?.variables ?? []).map((v) => ({ ...v }))
  envScript.value = env?.script ?? ''
  showSecret.value = {}
}

function close() {
  emit('update:open', false)
}

function addVariable() {
  variables.value.push({ key: '', value: '', isSecret: false, sortOrder: variables.value.length })
}

function removeVariable(index: number) {
  variables.value.splice(index, 1)
}

async function saveVariables() {
  if (!editingId.value) return
  saving.value = true
  try {
    await api.envVarSave(
      editingId.value,
      variables.value.map((v, i) => ({ ...v, sortOrder: i })),
    )
    await reload(editingId.value)
    message.success('变量已保存')
  } catch (e) {
    message.error(String(e))
  } finally {
    saving.value = false
  }
}

async function saveScript() {
  if (!editingId.value) return
  scriptSaving.value = true
  try {
    await api.envScriptSave(editingId.value, envScript.value)
    const env = environments.value.find((e) => e.id === editingId.value)
    if (env) env.script = envScript.value
    const list = environments.value.map((e) => ({ ...e }))
    emit('changed', list)
    message.success('环境脚本已保存')
  } catch (e) {
    message.error(String(e))
  } finally {
    scriptSaving.value = false
  }
}

async function createEnv() {
  const name = newName.value.trim()
  if (!props.projectId) return
  if (!name) {
    message.warning('请填写环境名称')
    return
  }
  try {
    const created = await api.envCreate(props.projectId, name)
    creating.value = false
    newName.value = ''
    await reload(created.id)
    message.success('已创建环境')
  } catch (e) {
    message.error(String(e))
  }
}

function cancelCreate() {
  creating.value = false
  newName.value = ''
}

async function activate() {
  if (!props.projectId || !editing.value) return
  const id = editing.value.id
  try {
    await api.envActivate(props.projectId, id)
    await reload(id)
    emit('select', id)
    message.success('已切换当前环境')
  } catch (e) {
    message.error(String(e))
  }
}

function removeEnv() {
  if (!editing.value) return
  const target = editing.value
  Modal.confirm({
    title: `删除环境「${target.name}」？`,
    content: '该环境下的变量与环境脚本会一并删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.envDelete(target.id)
      await reload(null)
      if (props.activeId === target.id) {
        emit('select', environments.value.find((e) => e.isActive)?.id ?? environments.value[0]?.id ?? null)
      }
    },
  })
}
</script>

<template>
  <a-drawer
    :open="open"
    title="环境变量"
    placement="right"
    width="min(680px, 94vw)"
    class="env-drawer"
    :body-style="{ padding: '18px 20px 24px' }"
    @close="close"
  >
    <a-spin :spinning="loading">
      <div v-if="!projectId" class="env-empty">
        <div class="env-empty-title">当前接口未关联项目</div>
        <div class="env-empty-desc">从项目页打开的接口才能在这里管理环境变量。</div>
      </div>

      <div v-else-if="!environments.length" class="env-empty">
        <div class="env-empty-title">还没有环境</div>
        <div class="env-empty-desc">
          先建一个环境，把 base_url、token 这类公共值放进去，请求里用
          <span class="mono tag" v-pre>{{变量名}}</span>
          引用。
        </div>
        <a-button type="primary" size="small" @click="creating = true">
          <template #icon><PlusOutlined /></template>
          新建环境
        </a-button>
      </div>

      <template v-else>
        <div class="env-bar">
          <a-select
            v-model:value="editingId"
            size="small"
            class="env-pick"
            :options="envOptions"
            @change="(value: number) => pick(value)"
          />
          <a-button size="small" @click="creating = true">
            <template #icon><PlusOutlined /></template>
            新建
          </a-button>
        </div>

        <div v-if="editing" class="env-state">
          <span class="env-name">{{ editing.name }}</span>
          <span v-if="editing.isActive" class="active-pill">
            <CheckCircleFilled /> 项目当前环境
          </span>
          <span class="env-actions">
            <a v-if="!editing.isActive" @click="activate">设为当前</a>
            <a class="danger" @click="removeEnv">删除环境</a>
          </span>
        </div>

        <div v-if="creating" class="create-row">
          <a-input
            v-model:value="newName"
            size="small"
            placeholder="例如：测试环境"
            @press-enter="createEnv"
          />
          <a-button size="small" type="primary" @click="createEnv">创建</a-button>
          <a-button size="small" @click="cancelCreate">取消</a-button>
        </div>

        <div class="var-head">
          <span class="c-key">变量名</span>
          <span class="c-value">值</span>
          <span class="c-secret">密钥</span>
          <span class="c-op" />
        </div>
        <div v-for="(v, i) in variables" :key="i" class="var-row">
          <a-input v-model:value="v.key" size="small" class="c-key mono" placeholder="base_url" />
          <div class="c-value value-wrap">
            <a-input
              v-model:value="v.value"
              size="small"
              class="mono"
              :type="v.isSecret && !showSecret[i] ? 'password' : 'text'"
              placeholder="变量值"
            />
            <a v-if="v.isSecret" class="eye" @click="showSecret[i] = !showSecret[i]">
              {{ showSecret[i] ? '隐藏' : '显示' }}
            </a>
          </div>
          <a-checkbox v-model:checked="v.isSecret" class="c-secret" />
          <a class="c-op danger" @click="removeVariable(i)">删除</a>
        </div>
        <div v-if="!variables.length" class="var-empty">
          该环境还没有变量，先添加
          <span class="mono tag" v-pre>{{base_url}}</span>
          试试
        </div>

        <a-button size="small" block class="add-var" @click="addVariable">
          <template #icon><PlusOutlined /></template>
          添加变量
        </a-button>

        <div class="actions">
          <a-button size="small" :disabled="!editing" @click="pick(editingId)">撤销修改</a-button>
          <a-button size="small" type="primary" :loading="saving" :disabled="!editing" @click="saveVariables">
            保存变量
          </a-button>
        </div>

        <div class="script-card">
          <div class="script-head">
            <span class="script-title">环境脚本</span>
            <a-button
              size="small"
              type="primary"
              ghost
              :loading="scriptSaving"
              :disabled="!editing"
              @click="saveScript"
            >
              保存脚本
            </a-button>
          </div>
          <div class="script-hint">
            每次发送前执行，可用 <span class="mono" v-pre>vars.set('key', value)</span> 生成运行时变量
          </div>
          <a-textarea
            v-model:value="envScript"
            class="mono script-input"
            :rows="8"
            :disabled="!editing"
            placeholder="vars.set('trace_id', 'trace-' + Date.now())&#10;vars.set('nonce', Math.random().toString(36).slice(2, 10))"
          />
        </div>

        <div class="tip">
          在 URL / Header / Body 里用
          <span class="mono tag" v-pre>{{base_url}}</span>
          这样的写法引用；优先级：运行时变量 &gt; 环境变量 &gt; 内置变量。
        </div>
      </template>
    </a-spin>
  </a-drawer>
</template>

<style scoped>
.env-empty {
  padding: 48px 12px;
  text-align: center;
}

.env-empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 6px;
}

.env-empty-desc {
  font-size: 12.5px;
  color: var(--fm-muted);
  line-height: 1.7;
  margin-bottom: 14px;
}

.env-bar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.env-pick {
  flex: 1;
  min-width: 0;
}

.env-state {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 12px 0 14px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--fm-border);
}

.env-name {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.active-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--fm-success-soft);
  color: var(--fm-success);
  font-size: 11.5px;
}

.env-actions {
  margin-left: auto;
  display: flex;
  gap: 14px;
  font-size: 12.5px;
}

.env-actions .danger {
  color: var(--fm-danger);
}

.create-row {
  display: flex;
  gap: 8px;
  margin-bottom: 14px;
}

.var-head,
.var-row {
  display: grid;
  grid-template-columns: 1fr 1.9fr 56px 44px;
  gap: 10px;
  align-items: center;
}

.var-head {
  padding: 0 2px 8px;
  font-size: 12px;
  color: var(--fm-muted);
}

.var-row {
  margin-bottom: 10px;
}

.value-wrap {
  position: relative;
  display: flex;
  align-items: center;
  min-width: 0;
}

.value-wrap .eye {
  position: absolute;
  right: 9px;
  font-size: 11.5px;
  color: var(--fm-primary);
}

.c-secret {
  display: flex;
  justify-content: center;
}

.c-op {
  text-align: right;
  font-size: 12.5px;
}

.c-op.danger {
  color: var(--fm-danger);
}

.var-empty {
  padding: 22px 12px;
  text-align: center;
  font-size: 12.5px;
  color: var(--fm-muted);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
}

.tag {
  display: inline-block;
  padding: 0 4px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  color: var(--fm-primary);
  font-size: 12px;
}

.add-var {
  margin-top: 4px;
  border-style: dashed;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin: 14px 0 20px;
}

.script-card {
  padding: 14px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
}

.script-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}

.script-title {
  font-size: 14px;
  font-weight: 600;
}

.script-hint {
  font-size: 12px;
  color: var(--fm-muted);
  margin-bottom: 10px;
  line-height: 1.6;
}

.script-input {
  font-size: 13px;
}

.tip {
  margin-top: 16px;
  padding: 12px 14px;
  border-radius: var(--fm-radius);
  background: var(--fm-primary-soft);
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text-2);
}

.mono {
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
}
</style>
