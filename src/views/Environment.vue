<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { PlusOutlined, CheckCircleFilled } from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { envAuditPrompt } from '@/utils/aiScenarios'
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
const envScript = ref('')
const scriptSaving = ref(false)

const current = computed(() => environments.value.find((e) => e.id === currentId.value) ?? null)

const aiAssist = useAiAssistStore()

function aiAudit() {
  if (!current.value) {
    message.warning('先选择一个环境')
    return
  }
  void aiAssist.show({
    title: '环境变量体检',
    prompt: envAuditPrompt(current.value, variables.value),
    hint: `${current.value.name} · ${variables.value.length} 个变量 · 密钥值不会发送明文`,
  })
}

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
  envScript.value = env?.script ?? ''
}

async function saveEnvScript() {
  if (!currentId.value) return
  scriptSaving.value = true
  try {
    await api.envScriptSave(currentId.value, envScript.value)
    const env = environments.value.find((e) => e.id === currentId.value)
    if (env) env.script = envScript.value
    message.success('环境脚本已保存')
  } catch (e) {
    message.error(String(e))
  } finally {
    scriptSaving.value = false
  }
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
      subtitle="请求里的 {{变量名}} 会被替换成当前环境变量、环境脚本或内置变量的值"
    >
      <a-button :disabled="!current" @click="aiAudit">
        <template #icon><AiSparkIcon /></template>
        AI 体检
      </a-button>
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

    <div class="fm-card script-card">
      <div class="script-head">
        <div>
          <div class="hint-title">环境脚本</div>
          <div class="hint-sub">
            每次发送请求前执行，可用 <span class="mono" v-pre>vars.set('key', value)</span> 赋值，请求里用
            <span class="mono" v-pre>{{key}}</span> 引用；也可读取 <span class="mono" v-pre>request</span>、
            <span class="mono" v-pre>env</span>、<span class="mono" v-pre>rsa.publicEncrypt(明文, 公钥)</span>、
            <span class="mono" v-pre>cipher</span>（aes / des / md5 / sha256 / hmac / base64）与
            <span class="mono" v-pre>console</span>。
          </div>
        </div>
        <a-button size="small" type="primary" :loading="scriptSaving" :disabled="!current" @click="saveEnvScript">
          保存脚本
        </a-button>
      </div>
      <a-textarea
        v-model:value="envScript"
        class="mono script-input"
        :rows="7"
        :disabled="!current"
        placeholder="vars.set('trace_id', 'trace-' + Date.now())&#10;vars.set('nonce', Math.random().toString(36).slice(2, 10))"
      />
    </div>

    <div class="fm-card hint-card">
      <div class="hint-head">
        <div class="hint-title">变量与脚本说明</div>
        <div class="hint-sub">
          变量优先级：运行时变量（脚本 <span class="mono" v-pre>vars.set</span>）&gt; 环境变量 &gt; 内置变量
        </div>
      </div>
      <div class="hint-groups">
        <div class="hint-group">
          <div class="hint-group-title">时间类</div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{timestamp}}</span>
            <span class="hint-desc" v-pre>毫秒时间戳，也支持 Postman 写法 {{$timestamp}}</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{timestampSec}}</span>
            <span class="hint-desc">秒级时间戳</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{isoTime}}</span>
            <span class="hint-desc">ISO 8601，如 2026-10-26T20:30:00.123+08:00</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{datetime}}</span>
            <span class="hint-desc">2026-10-26 20:30:00</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{date}}</span>
            <span class="hint-desc">日期 2026-10-26</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{time}}</span>
            <span class="hint-desc">时间 20:30:00</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{yyyyMMdd}}</span>
            <span class="hint-desc">紧凑日期 20261026</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{yyyyMMddHHmmss}}</span>
            <span class="hint-desc">紧凑时间 20261026203000</span>
          </div>
        </div>

        <div class="hint-group">
          <div class="hint-group-title">随机类</div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{uuid}}</span>
            <span class="hint-desc" v-pre>随机 UUID，{{uuidNoDash}} 为无短横线版本</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomInt(1,100)}}</span>
            <span class="hint-desc">区间随机整数，单参数时为 0 到该值</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomFloat(1,10,2)}}</span>
            <span class="hint-desc">区间随机小数，第三个参数为小数位</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomString(8)}}</span>
            <span class="hint-desc" v-pre>随机字母数字；{{randomString(8, hex)}} 可指定字符集</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomHex(16)}}</span>
            <span class="hint-desc">指定长度十六进制串</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomNumber(6)}}</span>
            <span class="hint-desc">指定长度纯数字串</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomBool}}</span>
            <span class="hint-desc">随机 true / false</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{randomEmail}}</span>
            <span class="hint-desc" v-pre>随机邮箱，另有 {{randomPhone}}、{{randomIPv4}}</span>
          </div>
        </div>

        <div class="hint-group">
          <div class="hint-group-title">自定义与脚本</div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{base_url}}</span>
            <span class="hint-desc">引用当前环境的变量</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{trace_id}}</span>
            <span class="hint-desc">引用环境脚本 vars.set 生成的运行时变量</span>
          </div>
          <div class="hint-item">
            <span class="hint-key mono" v-pre>{{last_title}}</span>
            <span class="hint-desc">引用后置脚本从响应中提取的值</span>
          </div>
          <pre class="mono hint-code" v-pre>// 环境脚本示例
vars.set('trace_id', 'trace-' + Date.now())
vars.set('nonce', Math.random().toString(36).slice(2, 10))</pre>
          <pre class="mono hint-code" v-pre>// 后置脚本提取响应字段
vars.set('last_title', response.json().slideshow.title)</pre>
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

.script-card,
.hint-card {
  margin-top: 16px;
  padding: 16px 20px;
}

.script-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 10px;
}

.script-input {
  font-size: 12px;
  line-height: 1.7;
}

.hint-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  margin-bottom: 12px;
}

.script-head .hint-title {
  margin-bottom: 4px;
}

.hint-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}

.hint-head .hint-title {
  margin-bottom: 0;
}

.hint-sub {
  font-size: 11.5px;
  color: var(--fm-muted);
  line-height: 1.7;
}

.hint-groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 18px 28px;
}

.hint-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.hint-group-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text-2);
  padding-bottom: 6px;
  border-bottom: 1px dashed var(--fm-border);
}

.hint-code {
  margin: 2px 0 0;
  padding: 8px 10px;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  word-break: break-all;
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
