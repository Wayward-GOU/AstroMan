<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { DownOutlined } from '@ant-design/icons-vue'
import { save } from '@tauri-apps/plugin-dialog'
import MarkdownIt from 'markdown-it'
import * as api from '@/api'
import type { Environment } from '@/types/models'
import type {
  DocEditOverlay,
  DocFormat,
  DocOutline,
  DocOutlineGroup,
  DocScope,
  DocsOptions,
  DocsPrepareResult,
} from '@/types/docs'
import { defaultDocsOptions } from '@/types/docs'
import {
  clearOverlay,
  countEdits,
  debounce,
  emptyOverlay,
  extensionFor,
  loadOverlay,
  sanitizeFileName,
  saveOverlay,
} from '@/utils/docsExport'
import DocsOutlineNode from './DocsOutlineNode.vue'

const props = defineProps<{
  open: boolean
  scope: DocScope
  targetId: number
  projectId: number
  projectName?: string
  defaultEnvironmentId?: number | null
}>()

const emit = defineEmits<{ (e: 'update:open', value: boolean): void }>()

const md = new MarkdownIt({ html: false, linkify: true, breaks: false })

const loading = ref(false)
const exporting = ref(false)
const environments = ref<Environment[]>([])
const environmentTouched = ref(false)
const environmentId = ref<number | null>(props.defaultEnvironmentId ?? null)
const options = reactive<DocsOptions>(defaultDocsOptions())
const outline = ref<DocOutline | null>(null)
const overlay = ref<DocEditOverlay>(emptyOverlay())
const draftId = ref('')
const warnings = ref<string[]>([])
const title = ref('')
const subtitle = ref('')
const tab = ref<'html' | 'markdown' | 'print'>('html')
const html = ref('')
const markdown = ref('')

const edited = computed(() => countEdits(overlay.value))
const markdownHtml = computed(() => md.render(markdown.value))
/** 单接口文档在侧栏展示接口信息，而不是集合树 */
const singleRequest = computed(() => {
  if (props.scope !== 'request') return null
  for (const group of outline.value?.groups ?? []) {
    if (group.requests[0]) return group.requests[0]
  }
  return null
})
const singleDescription = computed(() => {
  const request = singleRequest.value
  if (!request) return ''
  return overlay.value.nodes[request.key]?.description ?? request.description
})
const scopeLabel = computed(() => {
  if (props.scope === 'request') return '单个接口'
  if (props.scope === 'collection') return '集合'
  return '项目'
})

/** 每种格式是否与当前覆盖层一致；切换标签页时按需重渲染 */
const stale = ref<Record<DocFormat, boolean>>({ html: true, markdown: true })

let prepareSeq = 0
let renderSeq = 0

let persistTimer: ReturnType<typeof setTimeout> | null = null

function persistNow() {
  saveOverlay(props.scope, props.targetId, environmentId.value, overlay.value)
}

function persistSoon() {
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(() => {
    persistTimer = null
    persistNow()
  }, 300)
}

function flushPersist() {
  if (persistTimer) {
    clearTimeout(persistTimer)
    persistTimer = null
  }
  persistNow()
}

async function loadEnvironments() {
  if (!props.projectId) return
  try {
    const list = await api.envList(props.projectId)
    environments.value = list
    if (!environmentTouched.value) {
      const preferred =
        props.defaultEnvironmentId ?? list.find((item) => item.isActive)?.id ?? list[0]?.id ?? null
      environmentId.value =
        preferred != null && list.some((item) => item.id === preferred)
          ? preferred
          : (list[0]?.id ?? null)
    }
  } catch (error) {
    message.error(String(error))
  }
}

async function prepare(keepOverlay = true) {
  const seq = ++prepareSeq
  draftId.value = ''
  loading.value = true
  try {
    const result: DocsPrepareResult = await api.docsPrepare({
      scope: props.scope,
      targetId: props.targetId,
      environmentId: environmentId.value,
      options: { ...options },
    })
    if (seq !== prepareSeq) return
    draftId.value = result.draftId
    outline.value = result.outline
    warnings.value = result.warnings
    if (!keepOverlay) {
      overlay.value = emptyOverlay(result.revision)
    } else {
      if (overlay.value.revision && overlay.value.revision !== result.revision && edited.value > 0) {
        message.warning('源数据已更新，你的编辑仍保留；请在预览中确认差异')
      }
      overlay.value.revision = result.revision
    }
    if (overlay.value.meta.title === undefined) title.value = result.meta.title
    if (overlay.value.meta.subtitle === undefined) subtitle.value = result.meta.subtitle
    persistNow()
    stale.value = { html: true, markdown: true }
    await renderNow()
  } catch (error) {
    if (seq === prepareSeq) message.error(String(error))
  } finally {
    if (seq === prepareSeq) loading.value = false
  }
}

async function renderNow(format?: DocFormat) {
  if (!draftId.value) return
  const target: DocFormat = format ?? (tab.value === 'markdown' ? 'markdown' : 'html')
  const seq = ++renderSeq
  try {
    const result = await api.docsRender({
      draftId: draftId.value,
      overlay: overlay.value,
      format: target,
    })
    if (seq !== renderSeq) return
    if (target === 'markdown') markdown.value = result.content
    else html.value = result.content
    stale.value = { ...stale.value, [target]: false }
  } catch (error) {
    if (seq !== renderSeq) return
    if (String(error).includes('E_DRAFT_EXPIRED')) {
      await prepare(true)
      return
    }
    message.error(String(error))
  }
}

const renderSoon = debounce(() => void renderNow(), 300)

function touch() {
  persistSoon()
  stale.value = { html: true, markdown: true }
  renderSoon()
}

function setTitle(value: string) {
  title.value = value
  overlay.value.meta = { ...overlay.value.meta, title: value }
  touch()
}

function setSubtitle(value: string) {
  subtitle.value = value
  overlay.value.meta = { ...overlay.value.meta, subtitle: value }
  touch()
}

function toggleExcluded(key: string) {
  const set = new Set(overlay.value.excluded)
  if (set.has(key)) set.delete(key)
  else set.add(key)
  overlay.value.excluded = [...set]
  touch()
}

function setDescription(key: string, value: string) {
  const node = overlay.value.nodes[key] ?? { exampleValues: {} }
  overlay.value.nodes = { ...overlay.value.nodes, [key]: { ...node, description: value } }
  touch()
}

function setSingleDescription(value: string) {
  const request = singleRequest.value
  if (request) setDescription(request.key, value)
}

function setField(requestKey: string, fieldKey: string, value: string) {
  const node = overlay.value.nodes[requestKey] ?? { exampleValues: {} }
  overlay.value.nodes = {
    ...overlay.value.nodes,
    [requestKey]: { ...node, exampleValues: { ...(node.exampleValues ?? {}), [fieldKey]: value } },
  }
  touch()
}

function findGroup(list: DocOutlineGroup[], key: string): DocOutlineGroup | null {
  for (const group of list) {
    if (group.key === key) return group
    const child = findGroup(group.children, key)
    if (child) return child
  }
  return null
}

function moveRequest(groupKey: string, from: number, to: number) {
  const group = outline.value ? findGroup(outline.value.groups, groupKey) : null
  if (!group) return
  const base = group.requests.map((item) => item.key)
  const saved = overlay.value.order[groupKey] ?? []
  // 与编辑树里的展示顺序保持一致：已保存顺序在前，未记录的按原始顺序补在末尾
  const order = [
    ...saved.filter((key) => base.includes(key)),
    ...base.filter((key) => !saved.includes(key)),
  ]
  if (from < 0 || from >= order.length || to < 0 || to >= order.length) return
  const [moved] = order.splice(from, 1)
  if (!moved) return
  order.splice(to, 0, moved)
  overlay.value.order = { ...overlay.value.order, [groupKey]: order }
  touch()
}

function confirmDiscard(text: string): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: '确认放弃编辑？',
      content: text,
      okText: '继续',
      cancelText: '取消',
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
}

async function onEnvironmentChange(value: number | null | undefined) {
  const next = value ?? null
  if (next === environmentId.value) return
  if (edited.value > 0 && !(await confirmDiscard('切换环境会重新生成示例值，已编辑内容将丢失'))) {
    // 受控 select 需要一次显式的值变化才能把显示回退到原环境
    const current = environmentId.value
    environmentId.value = null
    await nextTick()
    environmentId.value = current
    return
  }
  environmentTouched.value = true
  environmentId.value = next
  clearOverlay(props.scope, props.targetId, next)
  overlay.value = emptyOverlay()
  title.value = ''
  subtitle.value = ''
  await prepare(false)
}

async function regenerate() {
  if (edited.value > 0 && !(await confirmDiscard('将丢弃手动编辑并重新生成示例值'))) return
  clearOverlay(props.scope, props.targetId, environmentId.value)
  overlay.value = emptyOverlay()
  title.value = ''
  subtitle.value = ''
  await prepare(false)
}

function closeDrawer() {
  flushPersist()
  emit('update:open', false)
}

async function exportDoc(format: DocFormat) {
  if (!draftId.value) return
  const path = await save({
    title: '导出接口文档',
    defaultPath: `${sanitizeFileName(title.value || props.projectName || '接口文档')}${extensionFor(format)}`,
    filters: [
      {
        name: format === 'markdown' ? 'Markdown' : 'HTML',
        extensions: [format === 'markdown' ? 'md' : 'html'],
      },
    ],
  })
  if (!path) return
  exporting.value = true
  try {
    const call = async () =>
      api.docsExport({
        format,
        path,
        source: { kind: 'draft', draftId: draftId.value, overlay: overlay.value },
      })
    let result
    try {
      result = await call()
    } catch (error) {
      if (!String(error).includes('E_DRAFT_EXPIRED')) throw error
      await prepare(true)
      result = await call()
    }
    message.success(`已导出 ${result.requestCount} 个接口到 ${path}`)
    if (result.warnings.length) {
      const list = result.warnings.slice(0, 3)
      message.warning(
        `${list.join('；')}${result.warnings.length > 3 ? ` 等 ${result.warnings.length} 条` : ''}`,
      )
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    exporting.value = false
  }
}

function onExportMenu({ key }: { key: string | number }) {
  void exportDoc(String(key) as DocFormat)
}

function onTabChange() {
  const target: DocFormat = tab.value === 'markdown' ? 'markdown' : 'html'
  if (stale.value[target]) void renderNow(target)
}

const prepareSoon = debounce(() => {
  if (props.open) void prepare(true)
}, 250)

watch(
  () => props.open,
  async (value) => {
    if (!value) {
      flushPersist()
      return
    }
    await loadEnvironments()
    const restored = loadOverlay(props.scope, props.targetId, environmentId.value)
    overlay.value = restored ?? emptyOverlay()
    title.value = restored?.meta.title ?? ''
    subtitle.value = restored?.meta.subtitle ?? ''
    tab.value = 'html'
    draftId.value = ''
    outline.value = null
    warnings.value = []
    html.value = ''
    markdown.value = ''
    stale.value = { html: true, markdown: true }
    await prepare(true)
  },
  { immediate: true },
)

watch(options, () => prepareSoon())
watch(tab, onTabChange)

onBeforeUnmount(() => {
  flushPersist()
})
</script>

<template>
  <a-drawer
    :open="open"
    placement="right"
    width="100vw"
    :closable="false"
    :body-style="{ padding: '0', height: '100vh' }"
    @close="closeDrawer"
  >
    <div class="docs-root">
      <header class="docs-head">
        <div class="head-title">
          <span class="head-name">导出接口文档</span>
          <span class="head-scope">{{ scopeLabel }}<template v-if="projectName"> · {{ projectName }}</template></span>
          <a-tag v-if="edited" color="orange">已编辑 {{ edited }} 处</a-tag>
        </div>
        <div class="head-actions">
          <a-button size="small" :loading="loading" @click="regenerate">重新生成</a-button>
          <a-dropdown :trigger="['click']">
            <a-button type="primary" size="small" :loading="exporting" :disabled="!draftId">
              导出
              <DownOutlined class="caret" />
            </a-button>
            <template #overlay>
              <a-menu @click="onExportMenu">
                <a-menu-item key="markdown">Markdown（.md）</a-menu-item>
                <a-menu-item key="html">独立 HTML（.html）</a-menu-item>
              </a-menu>
            </template>
          </a-dropdown>
          <a-button size="small" type="text" @click="closeDrawer">关闭</a-button>
        </div>
      </header>

      <div class="docs-body">
        <aside class="docs-side">
          <div class="side-block">
            <div class="block-title">环境</div>
            <a-select
              :value="environmentId"
              style="width: 100%"
              placeholder="不解析环境变量"
              allow-clear
              @change="onEnvironmentChange"
            >
              <a-select-option v-for="env in environments" :key="env.id" :value="env.id">
                {{ env.name }}
              </a-select-option>
            </a-select>
          </div>

          <div class="side-block">
            <div class="block-title">设置</div>
            <a-checkbox v-model:checked="options.includeEnvironment">环境变量表</a-checkbox>
            <a-checkbox v-model:checked="options.resolveEnvironment">变量转真实值</a-checkbox>
            <a-checkbox v-model:checked="options.includeScripts">脚本</a-checkbox>
            <a-checkbox v-model:checked="options.includeAssertions">断言</a-checkbox>
            <a-checkbox v-model:checked="options.useRunExamples">取上次运行示例</a-checkbox>
            <a-checkbox v-model:checked="options.includeResponseExample">响应示例</a-checkbox>
            <a-checkbox v-model:checked="options.autoExamples">自动补示例</a-checkbox>
            <a-checkbox v-model:checked="options.pageBreakPerRequest">每接口分页</a-checkbox>
            <a-checkbox v-model:checked="options.includeGeneratedAt">生成时间（时分）</a-checkbox>
          </div>

          <div class="side-block">
            <div class="block-title">文档标题</div>
            <a-input
              :value="title"
              placeholder="文档标题"
              @change="(event: any) => setTitle(event.target.value)"
            />
            <a-input
              :value="subtitle"
              placeholder="副标题（可选）"
              style="margin-top: 6px"
              @change="(event: any) => setSubtitle(event.target.value)"
            />
          </div>

          <div v-if="scope === 'request'" class="side-block">
            <div class="block-title">接口</div>
            <div v-if="singleRequest" class="single-card">
              <div class="single-title">
                <span class="single-method">{{ singleRequest.method }}</span>
                <span>{{ singleRequest.name }}</span>
              </div>
              <div class="single-url">{{ singleRequest.url }}</div>
              <a-checkbox
                :checked="!overlay.excluded.includes(singleRequest.key)"
                @change="toggleExcluded(singleRequest.key)"
              >
                收录此接口
              </a-checkbox>
              <a-textarea
                :value="singleDescription"
                placeholder="接口说明"
                :rows="3"
                style="margin-top: 6px"
                @change="(event: any) => setSingleDescription(event.target.value)"
              />
            </div>
            <a-empty v-else description="暂无接口" />
          </div>

          <div v-else class="side-block outline-block">
            <div class="block-title">集合树（取消勾选可排除）</div>
            <div v-if="outline?.groups?.length" class="outline-tree">
              <DocsOutlineNode
                v-for="group in outline.groups"
                :key="group.key"
                :group="group"
                :overlay="overlay"
                :depth="0"
                @toggle-request="toggleExcluded"
                @toggle-group="toggleExcluded"
                @description="setDescription"
                @field="setField"
                @move-request="moveRequest"
              />
            </div>
            <a-empty v-else description="暂无接口" />
          </div>
        </aside>

        <section class="docs-preview">
          <div class="preview-tabs">
            <a-radio-group v-model:value="tab" size="small" button-style="solid">
              <a-radio-button value="html">渲染效果</a-radio-button>
              <a-radio-button value="markdown">Markdown</a-radio-button>
              <a-radio-button value="print">打印分页</a-radio-button>
            </a-radio-group>
            <span class="preview-tip">
              {{ tab === 'print' ? 'PDF 预览为排版近似，最终以导出文件为准' : '' }}
            </span>
          </div>
          <div class="preview-body">
            <iframe
              v-if="tab !== 'markdown'"
              class="preview-frame"
              :class="{ print: tab === 'print' }"
              sandbox=""
              :srcdoc="html"
            />
            <div v-else class="markdown-view" v-html="markdownHtml" />
          </div>
          <div v-if="warnings.length" class="preview-warnings">
            <a-tag v-for="warning in warnings.slice(0, 3)" :key="warning" color="orange">
              {{ warning }}
            </a-tag>
            <span v-if="warnings.length > 3">等 {{ warnings.length }} 条</span>
          </div>
        </section>
      </div>
    </div>
  </a-drawer>
</template>

<style scoped>
.docs-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}
.docs-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid #e5e6eb;
}
.head-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.head-name {
  font-size: 15px;
  font-weight: 600;
}
.head-scope {
  color: #8c8c8c;
  font-size: 12px;
}
.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.caret {
  margin-left: 4px;
  font-size: 10px;
}
.docs-body {
  display: flex;
  flex: 1;
  min-height: 0;
}
.docs-side {
  width: 340px;
  flex: none;
  border-right: 1px solid #e5e6eb;
  overflow: auto;
  padding: 8px;
  background: #fafafa;
}
.side-block {
  margin-bottom: 8px;
  padding: 8px;
  background: #fff;
  border: 1px solid #eceef1;
}
.side-block :deep(.ant-checkbox-wrapper) {
  display: flex;
  margin: 2px 0;
}
.block-title {
  margin-bottom: 6px;
  color: #646a73;
  font-size: 12px;
  font-weight: 600;
}
.outline-block {
  min-height: 200px;
}
.single-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.single-title {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-weight: 600;
  word-break: break-all;
}
.single-method {
  flex: none;
  color: #009688;
  font-size: 12px;
  font-weight: 700;
}
.single-url {
  color: #646a73;
  font-size: 12px;
  word-break: break-all;
}
.outline-tree {
  max-height: calc(100vh - 460px);
  overflow: auto;
}
.docs-preview {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  background: #f0f2f5;
}
.preview-tabs {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;
  border-bottom: 1px solid #e5e6eb;
  background: #fff;
}
.preview-tip {
  color: #8c8c8c;
  font-size: 12px;
}
.preview-body {
  flex: 1;
  min-height: 0;
  padding: 10px;
}
.preview-frame {
  width: 100%;
  height: 100%;
  border: 1px solid #e5e6eb;
  background: #fff;
}
.preview-frame.print {
  width: 794px;
  max-width: 100%;
  margin: 0 auto;
  display: block;
}
.markdown-view {
  height: 100%;
  overflow: auto;
  padding: 20px 24px;
  border: 1px solid #e5e6eb;
  background: #fff;
  font-size: 14px;
  line-height: 1.7;
}
.markdown-view :deep(h1) {
  font-size: 22px;
  margin: 0 0 12px;
}
.markdown-view :deep(h2) {
  font-size: 18px;
  margin: 20px 0 8px;
  padding-bottom: 4px;
  border-bottom: 1px solid #eee;
}
.markdown-view :deep(h3),
.markdown-view :deep(h4) {
  font-size: 15px;
  margin: 14px 0 6px;
}
.markdown-view :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 8px 0;
  font-size: 13px;
}
.markdown-view :deep(th),
.markdown-view :deep(td) {
  border: 1px solid #e5e6eb;
  padding: 4px 8px;
  text-align: left;
  word-break: break-all;
}
.markdown-view :deep(th) {
  background: #fafafa;
}
.markdown-view :deep(pre) {
  padding: 10px;
  background: #f6f8fa;
  overflow: auto;
}
.markdown-view :deep(code) {
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
}
.preview-warnings {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  background: #fff;
  border-top: 1px solid #e5e6eb;
  font-size: 12px;
}
</style>
