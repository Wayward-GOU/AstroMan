<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { PlusOutlined, CheckCircleFilled } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import type { Environment, EnvVariable } from '@/types/models'

const route = useRoute()
const router = useRouter()
const projectId = computed(() => Number(route.params.projectId))

const environments = ref<Environment[]>([])
const currentId = ref<number | null>(null)
const variables = ref<EnvVariable[]>([])
const saving = ref(false)
const createOpen = ref(false)
const newName = ref('')
const showSecret = ref<Record<number, boolean>>({})

const current = computed(() => environments.value.find((e) => e.id === currentId.value) ?? null)

onMounted(load)

async function load() {
  try {
    environments.value = await api.envList(projectId.value)
    if (environments.value.length) {
      select(environments.value.find((e) => e.isActive)?.id ?? environments.value[0].id)
    }
  } catch (e) {
    message.error(String(e))
  }
}

function select(id: number) {
  currentId.value = id
  const env = environments.value.find((e) => e.id === id)
  variables.value = (env?.variables ?? []).map((v) => ({ ...v }))
}

function addVariable() {
  variables.value.push({ key: '', value: '', isSecret: false, sortOrder: variables.value.length })
}

function removeVariable(index: number) {
  variables.value.splice(index, 1)
}

async function saveVariables() {
  if (!currentId.value) return
  saving.value = true
  try {
    await api.envVarSave(
      currentId.value,
      variables.value.map((v, i) => ({ ...v, sortOrder: i })),
    )
    await load()
    message.success('变量已保存')
  } catch (e) {
    message.error(String(e))
  } finally {
    saving.value = false
  }
}

async function createEnv() {
  const name = newName.value.trim()
  if (!name) {
    message.warning('请填写环境名称')
    return
  }
  try {
    await api.envCreate(projectId.value, name)
    createOpen.value = false
    newName.value = ''
    await load()
    message.success('已创建')
  } catch (e) {
    message.error(String(e))
  }
}

async function activate(id: number) {
  try {
    await api.envActivate(projectId.value, id)
    await load()
    select(id)
    message.success('已切换当前环境')
  } catch (e) {
    message.error(String(e))
  }
}

function removeEnv(env: Environment) {
  Modal.confirm({
    title: `删除环境「${env.name}」？`,
    content: '该环境下的变量会一并删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.envDelete(env.id)
      await load()
    },
  })
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Environment"
      title="环境变量"
      subtitle="请求里的 {{变量名}} 会被替换成当前环境下的值"
    >
      <a-button @click="router.push(`/project/${projectId}`)">返回项目</a-button>
      <a-button type="primary" @click="createOpen = true">
        <template #icon><PlusOutlined /></template>
        新建环境
      </a-button>
    </PageHeader>

    <div class="layout">
      <div class="env-list fm-card">
        <div class="pane-head">环境</div>
        <div
          v-for="env in environments"
          :key="env.id"
          class="env-item"
          :class="{ active: env.id === currentId }"
          @click="select(env.id)"
        >
          <div class="env-name">
            <span class="env-dot" :class="{ on: env.isActive }" />
            <span class="env-text">{{ env.name }}</span>
            <CheckCircleFilled v-if="env.isActive" class="active-icon" />
          </div>
          <div class="env-op-row">
            <a v-if="!env.isActive" @click.stop="activate(env.id)">设为当前</a>
            <a class="danger" @click.stop="removeEnv(env)">删除</a>
          </div>
        </div>
        <div v-if="!environments.length" class="env-empty">
          <div class="env-empty-desc">还没有环境</div>
          <a-button size="small" type="primary" ghost @click="createOpen = true">新建环境</a-button>
        </div>
      </div>

      <div class="var-pane fm-card">
        <div class="pane-head">
          <div class="pane-title">
            {{ current ? `${current.name} 的变量` : '请选择环境' }}
            <span v-if="current?.isActive" class="active-pill">当前环境</span>
          </div>
          <div class="pane-actions">
            <a-button size="small" :disabled="!current" @click="addVariable">
              <template #icon><PlusOutlined /></template>
              添加变量
            </a-button>
            <a-button size="small" type="primary" :loading="saving" :disabled="!current" @click="saveVariables">
              保存
            </a-button>
          </div>
        </div>
        <div class="var-body">
          <div v-if="variables.length" class="var-head">
            <span class="c-key">变量名</span>
            <span class="c-value">值</span>
            <span class="c-secret">密文</span>
            <span class="c-op" />
          </div>
          <div v-for="(v, i) in variables" :key="i" class="var-row">
            <a-input v-model:value="v.key" class="c-key mono" size="small" placeholder="例如 base_url" />
            <div class="c-value value-wrap">
              <a-input
                v-model:value="v.value"
                class="mono"
                size="small"
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
            <div class="var-empty-icon">｛｝</div>
            <div class="var-empty-title">该环境还没有变量</div>
            <div class="var-empty-desc">
              建议先添加 <span class="mono tag" v-pre>{{base_url}}</span>，然后在请求 URL 里用
              <span class="mono tag" v-pre>{{base_url}}</span> 引用
            </div>
            <a-button type="primary" ghost size="small" @click="addVariable">添加第一个变量</a-button>
          </div>
        </div>
      </div>
    </div>

    <div class="fm-card hint-card">
      <div class="hint-title">变量使用说明</div>
      <div class="hint-grid">
        <div class="hint-item">
          <span class="hint-key mono" v-pre>{{base_url}}</span>
          <span class="hint-desc">引用当前环境的变量</span>
        </div>
        <div class="hint-item">
          <span class="hint-key mono" v-pre>{{timestamp}}</span>
          <span class="hint-desc">当前毫秒时间戳</span>
        </div>
        <div class="hint-item">
          <span class="hint-key mono" v-pre>{{uuid}}</span>
          <span class="hint-desc">随机 UUID</span>
        </div>
        <div class="hint-item">
          <span class="hint-key mono" v-pre>{{randomInt(1,100)}}</span>
          <span class="hint-desc">区间随机整数</span>
        </div>
        <div class="hint-item">
          <span class="hint-key mono" v-pre>{{randomString(8)}}</span>
          <span class="hint-desc">指定长度随机字符串</span>
        </div>
      </div>
    </div>

    <a-modal v-model:open="createOpen" title="新建环境" ok-text="创建" cancel-text="取消" @ok="createEnv">
      <a-input
        v-model:value="newName"
        placeholder="例如：开发 / 测试 / 生产"
        style="margin-top: 12px"
        @press-enter="createEnv"
      />
    </a-modal>
  </div>
</template>

<style scoped>
.layout {
  display: flex;
  gap: 16px;
  min-height: 420px;
  align-items: stretch;
}

.env-list {
  width: 268px;
  flex-shrink: 0;
  overflow: hidden;
}

.pane-head {
  height: 48px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--fm-border);
  font-size: 13px;
  font-weight: 600;
}

.env-item {
  padding: 11px 14px;
  border-bottom: 1px solid #f2f5fa;
  cursor: pointer;
  transition: background 0.14s;
}

.env-item:hover {
  background: var(--fm-surface-alt);
}

.env-item.active {
  background: var(--fm-primary-soft);
}

.env-name {
  display: flex;
  align-items: center;
  gap: 7px;
}

.env-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--fm-border-strong);
  flex-shrink: 0;
}

.env-dot.on {
  background: var(--fm-success);
}

.env-text {
  font-size: 13px;
  font-weight: 500;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.active-icon {
  color: var(--fm-success);
  font-size: 12px;
}

.env-op-row {
  display: flex;
  gap: 12px;
  margin-top: 5px;
  padding-left: 14px;
  font-size: 11px;
  opacity: 0;
  transition: opacity 0.14s;
}

.env-item:hover .env-op-row {
  opacity: 1;
}

.danger {
  color: var(--fm-danger);
}

.env-empty {
  padding: 34px 16px;
  text-align: center;
}

.env-empty-desc {
  font-size: 12px;
  color: var(--fm-muted);
  margin-bottom: 10px;
}

.var-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.pane-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.active-pill {
  font-size: 9px;
  font-weight: 500;
  color: var(--fm-success);
  background: var(--fm-success-soft);
  padding: 1px 8px;
  border-radius: 999px;
}

.pane-actions {
  display: flex;
  gap: 8px;
}

.var-body {
  padding: 16px;
  flex: 1;
}

.var-head,
.var-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.var-head {
  font-size: 11px;
  color: var(--fm-muted);
  padding: 0 2px 7px;
}

.var-row {
  margin-bottom: 8px;
}

.c-key {
  width: 30%;
}

.c-value {
  flex: 1;
}

.value-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}

.eye {
  font-size: 11px;
  white-space: nowrap;
  color: var(--fm-primary);
}

.c-secret {
  width: 48px;
  text-align: center;
}

.c-op {
  width: 32px;
  text-align: center;
  font-size: 11px;
  cursor: pointer;
}

.var-empty {
  padding: 60px 20px;
  text-align: center;
}

.var-empty-icon {
  width: 48px;
  height: 48px;
  margin: 0 auto 14px;
  border-radius: 14px;
  background: var(--fm-surface-alt);
  color: #c2ccdb;
  font-size: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.var-empty-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.var-empty-desc {
  max-width: 380px;
  margin: 6px auto 16px;
  font-size: 11px;
  line-height: 1.8;
  color: var(--fm-muted);
}

.tag {
  padding: 1px 6px;
  border-radius: 5px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 10px;
}

.hint-card {
  margin-top: 16px;
  padding: 16px 20px;
}

.hint-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  margin-bottom: 12px;
}

.hint-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: 10px 20px;
}

.hint-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.hint-key {
  font-size: 11px;
  color: var(--fm-primary);
}

.hint-desc {
  font-size: 11px;
  color: var(--fm-muted);
}
</style>
