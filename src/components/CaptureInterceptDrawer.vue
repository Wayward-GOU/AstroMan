<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import {
  DeleteOutlined,
  FileOutlined,
  FolderOpenOutlined,
  PlusOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import * as api from '@/api/capture'
import type {
  BreakpointPhase,
  BreakpointRule,
  InterceptView,
  MapRemoteRule,
  MapRule,
  WeakNetwork,
} from '@/types/capture'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'changed', view: InterceptView): void
}>()

type TabKey = 'breakpoint' | 'map' | 'remote' | 'weak'

const activeTab = ref<TabKey>('breakpoint')
const loading = ref(false)
const saving = ref(false)
const breakpointsEnabled = ref(false)
const breakpointRules = ref<BreakpointRule[]>([])
const mapEnabled = ref(false)
const mapRules = ref<MapRule[]>([])
const remoteEnabled = ref(false)
const remoteRules = ref<MapRemoteRule[]>([])
const weak = ref<WeakNetwork>(emptyWeak())
const weakPatternsText = ref('')
const pendingCount = ref(0)
const pausedTotal = ref(0)

const methodOptions = ['ANY', 'GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const phaseOptions: { value: BreakpointPhase; label: string }[] = [
  { value: 'both', label: '请求与响应' },
  { value: 'request', label: '请求（入参）' },
  { value: 'response', label: '响应（返参）' },
]
const weakPresets: { value: string; label: string; weak: Partial<WeakNetwork> }[] = [
  {
    value: '4g',
    label: '4G（100ms / 4Mbps）',
    weak: { latencyMs: 100, jitterMs: 30, bandwidthKbps: 4000, lossPercent: 0 },
  },
  {
    value: '3g',
    label: '3G（300ms / 750kbps）',
    weak: { latencyMs: 300, jitterMs: 100, bandwidthKbps: 750, lossPercent: 1 },
  },
  {
    value: 'wifi',
    label: '弱 Wi-Fi（150ms / 1.5Mbps）',
    weak: { latencyMs: 150, jitterMs: 80, bandwidthKbps: 1500, lossPercent: 2 },
  },
  {
    value: 'edge',
    label: '极差（800ms / 300kbps / 10% 丢包）',
    weak: { latencyMs: 800, jitterMs: 300, bandwidthKbps: 300, lossPercent: 10 },
  },
  { value: 'custom', label: '自定义', weak: {} },
]

function emptyWeak(): WeakNetwork {
  return {
    enabled: false,
    preset: 'custom',
    latencyMs: 200,
    jitterMs: 50,
    bandwidthKbps: 0,
    lossPercent: 0,
    patterns: [],
  }
}

function emptyBreakpointRule(): BreakpointRule {
  return {
    id: 0,
    enabled: true,
    name: '',
    pattern: '',
    method: 'ANY',
    phase: 'both',
  }
}

function emptyMapRule(): MapRule {
  return {
    id: 0,
    enabled: true,
    name: '',
    pattern: '',
    method: 'ANY',
    source: 'inline',
    filePath: '',
    status: 200,
    contentType: '',
    body: '',
    headers: [],
  }
}

function emptyRemoteRule(): MapRemoteRule {
  return {
    id: 0,
    enabled: true,
    name: '',
    pattern: '',
    method: 'ANY',
    target: '',
    keepPath: true,
  }
}

function applyView(view: InterceptView) {
  breakpointsEnabled.value = view.breakpointsEnabled
  breakpointRules.value = (view.breakpointRules ?? []).map((rule) => ({ ...rule }))
  mapEnabled.value = view.mapEnabled
  mapRules.value = (view.mapRules ?? []).map((rule) => ({
    ...rule,
    headers: (rule.headers ?? []).map((header) => ({ ...header })),
  }))
  remoteEnabled.value = view.remoteEnabled
  remoteRules.value = (view.remoteRules ?? []).map((rule) => ({ ...rule }))
  weak.value = { ...emptyWeak(), ...(view.weak ?? {}) }
  weakPatternsText.value = (view.weak?.patterns ?? []).join('\n')
  pendingCount.value = view.pendingCount
  pausedTotal.value = view.pausedTotal
}

async function load() {
  loading.value = true
  try {
    const view = await api.captureInterceptState()
    applyView(view)
  } catch (error) {
    message.error(String(error))
  } finally {
    loading.value = false
  }
}

async function toggleBreakpoints(enabled: boolean) {
  breakpointsEnabled.value = enabled
  try {
    const view = await api.captureBreakpointSetRules({ enabled })
    pendingCount.value = view.pendingCount
    pausedTotal.value = view.pausedTotal
    emit('changed', view)
  } catch (error) {
    breakpointsEnabled.value = !enabled
    message.error(String(error))
  }
}

async function toggleMap(enabled: boolean) {
  mapEnabled.value = enabled
  try {
    const view = await api.captureMapSetRules({ enabled })
    emit('changed', view)
  } catch (error) {
    mapEnabled.value = !enabled
    message.error(String(error))
  }
}

async function toggleRemote(enabled: boolean) {
  remoteEnabled.value = enabled
  try {
    const view = await api.captureRemoteSetRules({ enabled })
    emit('changed', view)
  } catch (error) {
    remoteEnabled.value = !enabled
    message.error(String(error))
  }
}

async function toggleWeak(enabled: boolean) {
  weak.value.enabled = enabled
  try {
    const view = await api.captureWeakSet(currentWeak())
    applyView(view)
    emit('changed', view)
  } catch (error) {
    weak.value.enabled = !enabled
    message.error(String(error))
  }
}

function clampNumber(
  value: number | null | undefined,
  min: number,
  max: number,
  fallback = 0,
) {
  const number = Number(value)
  if (!Number.isFinite(number)) return fallback
  return Math.min(Math.max(Math.round(number), min), max)
}

function currentWeak(): WeakNetwork {
  return {
    ...weak.value,
    latencyMs: clampNumber(weak.value.latencyMs, 0, 60000),
    jitterMs: clampNumber(weak.value.jitterMs, 0, 30000),
    bandwidthKbps: clampNumber(weak.value.bandwidthKbps, 0, 100000000),
    lossPercent: clampNumber(weak.value.lossPercent, 0, 100),
    patterns: weakPatternsText.value
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean),
  }
}

function applyPreset(value: string) {
  weak.value.preset = value
  const preset = weakPresets.find((item) => item.value === value)
  if (!preset || value === 'custom') return
  Object.assign(weak.value, preset.weak)
}

async function save() {
  saving.value = true
  try {
    let view: InterceptView
    if (activeTab.value === 'breakpoint') {
      view = await api.captureBreakpointSetRules({
        enabled: breakpointsEnabled.value,
        rules: breakpointRules.value,
      })
    } else if (activeTab.value === 'map') {
      view = await api.captureMapSetRules({
        enabled: mapEnabled.value,
        rules: mapRules.value.map((rule) => ({
          ...rule,
          status: clampNumber(rule.status, 100, 599, 200),
        })),
      })
    } else if (activeTab.value === 'remote') {
      view = await api.captureRemoteSetRules({
        enabled: remoteEnabled.value,
        rules: remoteRules.value,
      })
    } else {
      weak.value = currentWeak()
      view = await api.captureWeakSet(weak.value)
    }
    applyView(view)
    emit('changed', view)
    message.success('拦截规则已保存并生效')
  } catch (error) {
    message.error(String(error))
  } finally {
    saving.value = false
  }
}

function addBreakpointRule() {
  breakpointRules.value.push(emptyBreakpointRule())
}

function addMapRule() {
  mapRules.value.push(emptyMapRule())
}

function removeBreakpointRule(index: number) {
  breakpointRules.value.splice(index, 1)
}

function removeMapRule(index: number) {
  mapRules.value.splice(index, 1)
}

function addRemoteRule() {
  remoteRules.value.push(emptyRemoteRule())
}

function removeRemoteRule(index: number) {
  remoteRules.value.splice(index, 1)
}

function addMapHeader(rule: MapRule) {
  rule.headers.push({ name: '', value: '' })
}

function removeMapHeader(rule: MapRule, index: number) {
  rule.headers.splice(index, 1)
}

async function pickMapFile(rule: MapRule) {
  try {
    const picked = await openDialog({
      multiple: false,
      filters: [
        { name: '全部文件', extensions: ['*'] },
        { name: 'JSON', extensions: ['json'] },
        { name: 'HTML', extensions: ['html', 'htm'] },
        { name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico'] },
      ],
    })
    if (typeof picked === 'string' && picked) {
      rule.filePath = picked
      if (!rule.name) rule.name = picked.split(/[\\/]/).pop() ?? ''
    }
  } catch (error) {
    message.error(String(error))
  }
}


watch(
  () => props.open,
  (open) => {
    if (open) void load()
  },
)

onMounted(() => {
  if (props.open) void load()
})
</script>

<template>
  <a-drawer
    :open="open"
    title="拦截规则"
    placement="right"
    width="min(760px, 96vw)"
    :body-style="{ padding: '0 16px 16px' }"
    @update:open="(value: boolean) => emit('update:open', value)"
  >
    <a-spin :spinning="loading">
      <div class="intercept-body">
        <div class="intercept-summary">
          <span>挂起 {{ pendingCount }} 条</span>
          <span>累计拦截 {{ pausedTotal }}</span>
          <span class="hint">规则保存在本机，重启后仍在；最多 200 条。</span>
        </div>

        <a-tabs v-model:active-key="activeTab" size="small" class="intercept-tabs">
          <a-tab-pane key="breakpoint">
            <template #tab>
              断点
              <a-badge
                v-if="breakpointRules.length"
                :count="breakpointRules.length"
                :number-style="{ backgroundColor: '#bfbfbf' }"
              />
            </template>
            <div class="tab-head">
              <div class="tab-switch">
                <a-switch
                  size="small"
                  :checked="breakpointsEnabled"
                  @change="(value: boolean) => toggleBreakpoints(value)"
                />
                <span>启用断点</span>
                <span class="hint">命中后请求会挂起，等这里放行或改包</span>
              </div>
              <a-button size="small" @click="addBreakpointRule">
                <PlusOutlined />
                添加规则
              </a-button>
            </div>

            <p class="rule-hint">
              URL 匹配支持 <code>*</code> 通配，不含 <code>*</code> 时按「包含」判断；只写
              <code>host/path</code> 也能命中。请求阶段可改方法、URL、请求头与请求体；响应阶段可改状态码、响应头与响应体。
            </p>

            <div v-if="!breakpointRules.length" class="rule-empty">
              还没有断点规则。添加一条后，匹配的请求会在发出前 / 返回前暂停。
            </div>
            <div v-else class="rule-list">
              <div
                v-for="(rule, index) in breakpointRules"
                :key="index"
                class="rule-card"
                :class="{ off: !rule.enabled }"
              >
                <div class="rule-head">
                  <a-switch v-model:checked="rule.enabled" size="small" />
                  <a-input
                    v-model:value="rule.name"
                    size="small"
                    class="rule-name"
                    :placeholder="`规则 ${index + 1}`"
                  />
                  <a-tag class="rule-tag">{{ phaseOptions.find((item) => item.value === rule.phase)?.label }}</a-tag>
                  <a-button type="text" size="small" danger title="删除规则" @click="removeBreakpointRule(index)">
                    <DeleteOutlined />
                  </a-button>
                </div>
                <div class="rule-grid">
                  <label>URL 匹配</label>
                  <a-input
                    v-model:value="rule.pattern"
                    size="small"
                    class="mono"
                    placeholder="*/api/user/* 或 example.com/login"
                  />
                  <label>方法</label>
                  <a-select v-model:value="rule.method" size="small" class="rule-select">
                    <a-select-option v-for="item in methodOptions" :key="item" :value="item">
                      {{ item === 'ANY' ? '不限方法' : item }}
                    </a-select-option>
                  </a-select>
                  <label>阶段</label>
                  <a-select v-model:value="rule.phase" size="small" class="rule-select">
                    <a-select-option v-for="item in phaseOptions" :key="item.value" :value="item.value">
                      {{ item.label }}
                    </a-select-option>
                  </a-select>
                </div>
              </div>
            </div>

            <section class="rule-note">
              <h4>怎么用</h4>
              <ol>
                <li>填一条 URL 规则并启用断点，例如 <code>*/api/order/create</code>。</li>
                <li>在 App / 浏览器触发该请求，流量会暂停并弹窗。</li>
                <li>在弹窗里改完入参 / 返参后点「改包放行」；不动就点「放行」，点「中止」直接断掉。</li>
                <li>120 秒没人处理会自动放行；同时挂起超过 32 条后新请求不再暂停。</li>
              </ol>
            </section>
          </a-tab-pane>

          <a-tab-pane key="map">
            <template #tab>
              本地映射
              <a-badge
                v-if="mapRules.length"
                :count="mapRules.length"
                :number-style="{ backgroundColor: '#bfbfbf' }"
              />
            </template>
            <div class="tab-head">
              <div class="tab-switch">
                <a-switch
                  size="small"
                  :checked="mapEnabled"
                  @change="(value: boolean) => toggleMap(value)"
                />
                <span>启用本地映射</span>
                <span class="hint">命中后不发往服务器，直接返回本地内容</span>
              </div>
              <a-button size="small" @click="addMapRule">
                <PlusOutlined />
                添加规则
              </a-button>
            </div>

            <p class="rule-hint">
              本地映射（Map Local）可以把接口指到本地文件或直接内联的响应。文件每次命中实时读取，改完保存即生效；最大 50 MB。
            </p>

            <div v-if="!mapRules.length" class="rule-empty">
              还没有映射规则。适合把线上接口临时指向本地 mock 数据，或替换静态资源。
            </div>
            <div v-else class="rule-list">
              <div
                v-for="(rule, index) in mapRules"
                :key="index"
                class="rule-card"
                :class="{ off: !rule.enabled }"
              >
                <div class="rule-head">
                  <a-switch v-model:checked="rule.enabled" size="small" />
                  <a-input
                    v-model:value="rule.name"
                    size="small"
                    class="rule-name"
                    :placeholder="`映射 ${index + 1}`"
                  />
                  <a-tag class="rule-tag">{{ rule.source === 'file' ? '本地文件' : '内联内容' }}</a-tag>
                  <a-button type="text" size="small" danger title="删除规则" @click="removeMapRule(index)">
                    <DeleteOutlined />
                  </a-button>
                </div>
                <div class="rule-grid">
                  <label>URL 匹配</label>
                  <a-input
                    v-model:value="rule.pattern"
                    size="small"
                    class="mono"
                    placeholder="*/api/config 或 example.com/logo.png"
                  />
                  <label>方法</label>
                  <a-select v-model:value="rule.method" size="small" class="rule-select">
                    <a-select-option v-for="item in methodOptions" :key="item" :value="item">
                      {{ item === 'ANY' ? '不限方法' : item }}
                    </a-select-option>
                  </a-select>
                  <label>响应来源</label>
                  <a-radio-group v-model:value="rule.source" size="small" button-style="solid">
                    <a-radio-button value="inline">内联内容</a-radio-button>
                    <a-radio-button value="file">本地文件</a-radio-button>
                  </a-radio-group>

                  <template v-if="rule.source === 'file'">
                    <label>本地文件</label>
                    <div class="file-line">
                      <a-input
                        v-model:value="rule.filePath"
                        size="small"
                        class="mono"
                        placeholder="D:\\mock\\config.json"
                      >
                        <template #prefix><FileOutlined /></template>
                      </a-input>
                      <a-button size="small" @click="pickMapFile(rule)">
                        <FolderOpenOutlined />
                        选择
                      </a-button>
                    </div>
                  </template>
                  <template v-else>
                    <label>响应正文</label>
                    <a-textarea
                      v-model:value="rule.body"
                      :rows="5"
                      size="small"
                      class="mono"
                      placeholder='{"ok":true}'
                    />
                  </template>

                  <label>状态码</label>
                  <a-input-number v-model:value="rule.status" :min="100" :max="599" size="small" class="rule-number" />
                  <label>Content-Type</label>
                  <a-input
                    v-model:value="rule.contentType"
                    size="small"
                    class="mono"
                    placeholder="留空按扩展名或内容猜"
                  />
                </div>

                <a-collapse ghost size="small" class="rule-extra">
                  <a-collapse-panel key="headers" :header="`自定义响应头（${rule.headers.length}）`">
                    <div
                      v-for="(header, headerIndex) in rule.headers"
                      :key="headerIndex"
                      class="header-row"
                    >
                      <a-input v-model:value="header.name" size="small" class="mono" placeholder="Header" />
                      <a-input v-model:value="header.value" size="small" class="mono" placeholder="Value" />
                      <a-button
                        type="text"
                        size="small"
                        danger
                        @click="removeMapHeader(rule, headerIndex)"
                      >
                        <DeleteOutlined />
                      </a-button>
                    </div>
                    <a-button size="small" type="dashed" block @click="addMapHeader(rule)">
                      <PlusOutlined />
                      添加响应头
                    </a-button>
                  </a-collapse-panel>
                </a-collapse>
              </div>
            </div>
          </a-tab-pane>

          <a-tab-pane key="remote">
            <template #tab>
              远程映射
              <a-badge
                v-if="remoteRules.length"
                :count="remoteRules.length"
                :number-style="{ backgroundColor: '#bfbfbf' }"
              />
            </template>
            <div class="tab-head">
              <div class="tab-switch">
                <a-switch
                  size="small"
                  :checked="remoteEnabled"
                  @change="(value: boolean) => toggleRemote(value)"
                />
                <span>启用远程映射</span>
                <span class="hint">命中后改发到另一台服务器，客户端与应用无感</span>
              </div>
              <a-button size="small" @click="addRemoteRule">
                <PlusOutlined />
                添加规则
              </a-button>
            </div>

            <p class="rule-hint">
              远程映射（Map Remote）把请求转发到另一个地址，适合「线上域名指到测试环境」或「灰度服务」。
              URL 匹配规则与断点一致；列表里仍显示客户端请求的原地址，详情里能看到实际发往的目标。
              与本地映射同时命中时，以本地映射为准。
            </p>

            <div v-if="!remoteRules.length" class="rule-empty">
              还没有远程映射规则。例如把 <code>api.example.com</code> 整体指到
              <code>localhost:8080</code>。
            </div>
            <div v-else class="rule-list">
              <div
                v-for="(rule, index) in remoteRules"
                :key="index"
                class="rule-card"
                :class="{ off: !rule.enabled }"
              >
                <div class="rule-head">
                  <a-switch v-model:checked="rule.enabled" size="small" />
                  <a-input
                    v-model:value="rule.name"
                    size="small"
                    class="rule-name"
                    :placeholder="`映射 ${index + 1}`"
                  />
                  <a-tag class="rule-tag">{{ rule.keepPath ? '保留原路径' : '固定目标地址' }}</a-tag>
                  <a-button type="text" size="small" danger title="删除规则" @click="removeRemoteRule(index)">
                    <DeleteOutlined />
                  </a-button>
                </div>
                <div class="rule-grid">
                  <label>URL 匹配</label>
                  <a-input
                    v-model:value="rule.pattern"
                    size="small"
                    class="mono"
                    placeholder="example.com/api 或 */api/user/*"
                  />
                  <label>方法</label>
                  <a-select v-model:value="rule.method" size="small" class="rule-select">
                    <a-select-option v-for="item in methodOptions" :key="item" :value="item">
                      {{ item === 'ANY' ? '不限方法' : item }}
                    </a-select-option>
                  </a-select>
                  <label>目标地址</label>
                  <a-input
                    v-model:value="rule.target"
                    size="small"
                    class="mono"
                    placeholder="http://localhost:8080 或 https://test.example.com"
                  />
                  <label>路径处理</label>
                  <a-radio-group v-model:value="rule.keepPath" size="small" button-style="solid">
                    <a-radio-button :value="true">保留原路径与查询串</a-radio-button>
                    <a-radio-button :value="false">只发到目标地址</a-radio-button>
                  </a-radio-group>
                </div>
              </div>
            </div>
          </a-tab-pane>

          <a-tab-pane key="weak">
            <template #tab>
              弱网
              <ThunderboltOutlined v-if="weak.enabled" class="weak-on" />
            </template>
            <div class="tab-head">
              <div class="tab-switch">
                <a-switch
                  size="small"
                  :checked="weak.enabled"
                  @change="(value: boolean) => toggleWeak(value)"
                />
                <span>启用弱网模拟</span>
                <span class="hint">延迟、抖动、带宽限速与丢包</span>
              </div>
            </div>

            <div class="weak-grid">
              <label>预设</label>
              <a-select v-model:value="weak.preset" size="small" class="rule-select" @change="applyPreset">
                <a-select-option v-for="item in weakPresets" :key="item.value" :value="item.value">
                  {{ item.label }}
                </a-select-option>
              </a-select>
              <label>首包延迟</label>
              <div class="unit-line">
                <a-input-number v-model:value="weak.latencyMs" :min="0" :max="60000" size="small" class="rule-number" />
                <span class="unit">ms</span>
              </div>
              <label>抖动</label>
              <div class="unit-line">
                <a-input-number v-model:value="weak.jitterMs" :min="0" :max="30000" size="small" class="rule-number" />
                <span class="unit">ms</span>
              </div>
              <label>下行带宽</label>
              <div class="unit-line">
                <a-input-number
                  v-model:value="weak.bandwidthKbps"
                  :min="0"
                  :max="100000000"
                  size="small"
                  class="rule-number"
                />
                <span class="unit">kbps（0 不限速）</span>
              </div>
              <label>丢包率</label>
              <div class="unit-line">
                <a-input-number v-model:value="weak.lossPercent" :min="0" :max="100" size="small" class="rule-number" />
                <span class="unit">%（命中直接返回 504）</span>
              </div>
              <label>作用范围</label>
              <a-textarea
                v-model:value="weakPatternsText"
                :rows="3"
                size="small"
                class="mono"
                placeholder="每行一条 URL 规则，支持 * 通配；留空表示全部流量"
              />
            </div>

            <p class="rule-hint weak-hint">
              延迟加在请求首包前，带宽按响应体大小折算等待时间（单次最多 120 秒）；丢包会把请求直接判死，
              方便验证 App 的重试与降级逻辑。弱网对本地映射的响应同样生效。
            </p>
          </a-tab-pane>
        </a-tabs>

        <div class="drawer-foot">
          <span class="foot-hint">改完记得保存；开关切换会立即生效。</span>
          <a-button :loading="loading" size="small" @click="load">重新读取</a-button>
          <a-button type="primary" size="small" :loading="saving" @click="save">
            保存当前页
          </a-button>
        </div>
      </div>
    </a-spin>
  </a-drawer>
</template>

<style scoped>
.intercept-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
}

.intercept-summary {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 10px;
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-bg-soft, #f6f7f9);
  border-radius: 6px;
}

.intercept-summary .hint {
  margin-left: auto;
}

.tab-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 4px 0 8px;
}

.tab-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.hint {
  color: var(--fm-muted);
  font-size: 12px;
}

.rule-hint {
  margin: 0 0 10px;
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-muted);
  background: var(--fm-bg-soft, #f6f7f9);
  border-radius: 6px;
}

.rule-hint code,
.rule-note code {
  padding: 0 4px;
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
  background: rgba(0, 0, 0, 0.05);
  border-radius: 3px;
}

.rule-empty {
  padding: 28px 12px;
  font-size: 13px;
  color: var(--fm-muted);
  text-align: center;
  background: var(--fm-bg-soft, #f6f7f9);
  border: 1px dashed var(--fm-border, #e5e7eb);
  border-radius: 8px;
}

.rule-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.rule-card {
  padding: 10px 12px;
  background: #fff;
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 8px;
}

.rule-card.off {
  opacity: 0.62;
}

.rule-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.rule-name {
  max-width: 220px;
}

.rule-tag {
  margin-left: auto;
  color: var(--fm-muted);
}

.rule-grid,
.weak-grid {
  display: grid;
  grid-template-columns: 88px 1fr;
  align-items: center;
  gap: 8px 10px;
  margin-top: 10px;
}

.rule-grid label,
.weak-grid label {
  font-size: 12px;
  color: var(--fm-muted);
}

.rule-select {
  max-width: 220px;
}

.rule-number {
  width: 140px;
}

.file-line {
  display: flex;
  gap: 8px;
}

.file-line .mono {
  flex: 1;
}

.header-row {
  display: flex;
  gap: 8px;
  margin-bottom: 6px;
}

.rule-extra {
  margin-top: 8px;
}

.rule-note {
  margin-top: 14px;
  padding: 10px 12px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-muted);
  border: 1px solid var(--fm-border, #e5e7eb);
  border-radius: 8px;
}

.rule-note h4 {
  margin: 0 0 4px;
  font-size: 12px;
  color: var(--fm-text, #1f2329);
}

.rule-note ol {
  margin: 0;
  padding-left: 18px;
}

.unit-line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.unit {
  font-size: 12px;
  color: var(--fm-muted);
}

.weak-hint {
  margin-top: 12px;
}

.weak-on {
  color: #faad14;
}

.drawer-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 10px;
  margin-top: auto;
  border-top: 1px solid var(--fm-border, #e5e7eb);
}

.foot-hint {
  margin-right: auto;
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
