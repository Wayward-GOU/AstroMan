<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import {
  CompressOutlined,
  CopyOutlined,
  DeleteOutlined,
  ExpandOutlined,
  ShrinkOutlined,
} from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import JsonTree from '@/components/JsonTree.vue'

const input = ref('')
const output = ref('')
const error = ref('')
const errorLocation = ref('')
const view = ref<'tree' | 'text'>('tree')
const treeMode = ref<'default' | 'all' | 'none'>('default')

const parsed = ref<unknown>(undefined)
const parsedOk = ref(false)

const inputStats = computed(() => {
  if (!input.value) return ''
  return `${input.value.split('\n').length} 行 · ${input.value.length} 字符`
})

const outputStats = computed(() => {
  if (!output.value) return ''
  return `${output.value.split('\n').length} 行 · ${output.value.length} 字符`
})

function locate(text: string, position: number): string {
  const before = text.slice(0, Math.max(0, position))
  const lines = before.split('\n')
  return `第 ${lines.length} 行，第 ${lines[lines.length - 1].length + 1} 列`
}

function fail(err: unknown, text: string) {
  const raw = err instanceof Error ? err.message : String(err)
  const match = /position (\d+)/.exec(raw)
  error.value = raw.replace(/^JSON\.parse:\s*/, '解析错误：')
  errorLocation.value = match ? locate(text, Number(match[1])) : ''
  parsedOk.value = false
  parsed.value = undefined
  output.value = ''
}

function parse(): unknown {
  error.value = ''
  errorLocation.value = ''
  if (!input.value.trim()) {
    error.value = '请先输入 JSON 内容'
    parsedOk.value = false
    return undefined
  }
  try {
    const value = JSON.parse(input.value)
    parsedOk.value = true
    parsed.value = value
    return value
  } catch (err) {
    fail(err, input.value)
    return undefined
  }
}

function format() {
  const value = parse()
  if (!parsedOk.value) return
  output.value = JSON.stringify(value, null, 2)
  view.value = 'tree'
  message.success('已格式化')
}

function minify() {
  const value = parse()
  if (!parsedOk.value) return
  output.value = JSON.stringify(value)
  view.value = 'text'
  message.success('已压缩')
}

function check() {
  parse()
  if (parsedOk.value) {
    const value = parsed.value
    const kind = Array.isArray(value) ? `数组（${value.length} 项）` : typeof value === 'object' ? `对象（${Object.keys(value as object).length} 个键）` : typeof value
    message.success(`JSON 格式合法：${kind}`)
    output.value = JSON.stringify(value, null, 2)
  } else {
    message.error('JSON 格式不合法')
  }
}

function escapeJson() {
  error.value = ''
  if (!input.value) return
  output.value = JSON.stringify(input.value)
  view.value = 'text'
  message.success('已转义为字符串字面量')
}

function unescapeJson() {
  error.value = ''
  if (!input.value.trim()) return
  try {
    const value = JSON.parse(input.value)
    if (typeof value !== 'string') {
      error.value = '反转义需要输入被引号包裹的 JSON 字符串'
      return
    }
    output.value = value
    view.value = 'text'
  } catch (err) {
    fail(err, input.value)
  }
}

function clearAll() {
  input.value = ''
  output.value = ''
  error.value = ''
  errorLocation.value = ''
  parsed.value = undefined
  parsedOk.value = false
}

async function copyOutput() {
  if (!output.value) return
  try {
    await navigator.clipboard.writeText(output.value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function loadSample() {
  input.value = `{
  "name": "FinkMeter",
  "version": "1.0.0",
  "local": true,
  "tools": ["json", "aes", "mybatis"],
  "author": { "name": "fink", "contact": "fink@example.com" }
}`
  format()
}
</script>

<template>
  <div class="json-wb">
    <div class="jw-toolbar">
      <div class="jw-actions">
        <a-button size="small" @click="loadSample">示例</a-button>
        <a-button type="primary" size="small" @click="format">格式化</a-button>
        <a-button size="small" @click="minify"><CompressOutlined /> 压缩</a-button>
        <a-button size="small" @click="check">校验</a-button>
        <a-button size="small" @click="escapeJson">转义</a-button>
        <a-button size="small" @click="unescapeJson">反转义</a-button>
        <a-button size="small" @click="clearAll"><DeleteOutlined /> 清空</a-button>
      </div>
      <div class="jw-view">
        <a-segmented v-model:value="view" :options="[{ label: '树形', value: 'tree' }, { label: '文本', value: 'text' }]" size="small" />
        <template v-if="view === 'tree'">
          <a-tooltip title="全部展开">
            <a-button size="small" type="text" @click="treeMode = 'all'"><ExpandOutlined /></a-button>
          </a-tooltip>
          <a-tooltip title="全部折叠">
            <a-button size="small" type="text" @click="treeMode = 'none'"><ShrinkOutlined /></a-button>
          </a-tooltip>
        </template>
        <a-button size="small" type="text" :disabled="!output" @click="copyOutput">
          <CopyOutlined /> 复制
        </a-button>
      </div>
    </div>

    <div class="jw-body">
      <section class="jw-pane">
        <header class="jw-head">
          <span>原始 JSON</span>
          <span class="jw-stat">{{ inputStats }}</span>
        </header>
        <CodeArea v-model="input" :rows="22" placeholder="在此粘贴 JSON，例如 {&quot;a&quot;:1}" />
      </section>

      <section class="jw-pane">
        <header class="jw-head">
          <span>{{ view === 'tree' ? '结构视图' : '结果文本' }}</span>
          <span class="jw-stat">{{ outputStats }}</span>
        </header>
        <a-alert
          v-if="error"
          type="error"
          show-icon
          :message="error"
          :description="errorLocation ? `错误位置：${errorLocation}` : undefined"
          class="jw-error"
        />
        <div class="jw-output">
          <div v-if="view === 'tree'" class="jw-tree scroll-y">
            <JsonTree v-if="parsedOk" :key="treeMode" :value="parsed" :mode="treeMode" />
            <div v-else class="empty-hint">格式化后在此以树形查看结构</div>
          </div>
          <CodeArea
            v-else
            :model-value="output"
            :rows="22"
            readonly
            placeholder="执行结果会显示在这里"
          />
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.json-wb {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.jw-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.jw-actions,
.jw-view {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.jw-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.jw-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.jw-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.jw-stat {
  margin-left: auto;
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.jw-error {
  margin-bottom: 0;
}

.jw-output {
  flex: 1;
  min-height: 0;
}

.jw-tree {
  height: calc(22 * 20px + 22px);
  padding: 10px 12px;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  overflow: auto;
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 12px;
  line-height: 20px;
}

@media (max-width: 1200px) {
  .jw-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
