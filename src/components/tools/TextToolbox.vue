<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import { randomPassword } from '@/tools/calc'
import {
  dedupeLines,
  duplicateLines,
  extractMybatisSql,
  formatJson,
  joinLines,
  joinTables,
  removeBlankLines,
  removeSpaces,
  reverseText,
  sortLinesAsNumber,
  sortLinesAsText,
  splitColumnsToRows,
  sqlInList,
  tableToHtml,
  tableToMarkdown,
  transposeTable,
} from '@/tools/textbox'

type Tone = 'default' | 'danger' | 'success' | 'warning' | 'primary'

interface ToolButton {
  key: string
  label: string
  hint: string
  tone?: Tone
  action: () => void
}

interface ToolGroup {
  title: string
  items: ToolButton[]
}

const text = ref('')
const separator = ref(',')
const history = ref<string[]>([''])
const cursor = ref(0)
const MAX_HISTORY = 50
let checkpoint: number | undefined

const canUndo = computed(() => cursor.value > 0)
const canRedo = computed(() => cursor.value < history.value.length - 1)
const stats = computed(() => {
  const lines = text.value ? text.value.split('\n').length : 0
  return `${lines} 行 · ${text.value.length} 字符`
})
const undoHint = computed(() => (canUndo.value ? `可还原 ${cursor.value} 步` : '暂无可还原内容'))

function clearCheckpoint() {
  if (checkpoint !== undefined) {
    window.clearTimeout(checkpoint)
    checkpoint = undefined
  }
}

function pushHistory(value: string) {
  const list = history.value.slice(0, cursor.value + 1)
  list.push(value)
  while (list.length > MAX_HISTORY) list.shift()
  history.value = list
  cursor.value = list.length - 1
}

function syncHistory() {
  if (history.value[cursor.value] !== text.value) pushHistory(text.value)
}

function commit(value: string) {
  if (value === text.value) return
  clearCheckpoint()
  syncHistory()
  pushHistory(value)
  text.value = value
}

function onEdit(value: string) {
  text.value = value
  clearCheckpoint()
  checkpoint = window.setTimeout(() => {
    checkpoint = undefined
    syncHistory()
  }, 600)
}

function transform(run: (input: string) => string) {
  try {
    commit(run(text.value))
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function undo() {
  clearCheckpoint()
  syncHistory()
  if (cursor.value <= 0) {
    message.info('已经是第一版内容')
    return
  }
  cursor.value -= 1
  text.value = history.value[cursor.value]
}

function redo() {
  clearCheckpoint()
  syncHistory()
  if (cursor.value >= history.value.length - 1) {
    message.info('已经是最后一版内容')
    return
  }
  cursor.value += 1
  text.value = history.value[cursor.value]
}

async function copyText() {
  if (!text.value) {
    message.info('内容为空，没有可复制的内容')
    return
  }
  try {
    await navigator.clipboard.writeText(text.value)
    message.success('已复制到剪贴板')
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}

onBeforeUnmount(() => {
  clearCheckpoint()
})

const groups: ToolGroup[] = [
  {
    title: '基础操作',
    items: [
      {
        key: 'clear',
        label: '清空内容',
        hint: '清空编辑区，可用「还原上一次内容」恢复',
        tone: 'danger',
        action: () => commit(''),
      },
      {
        key: 'undo',
        label: '还原上一次内容',
        hint: '回到上一版内容',
        action: () => undo(),
      },
      {
        key: 'redo',
        label: '还原下一次内容',
        hint: '前进到下一版内容',
        action: () => redo(),
      },
    ],
  },
  {
    title: '清理与大小写',
    items: [
      {
        key: 'password',
        label: '生成随机密码',
        hint: '生成 20 位、含符号并排除易混淆字符的密码，替换当前内容',
        action: () =>
          transform(() => randomPassword(20, { symbol: true, excludeAmbiguous: true })),
      },
      {
        key: 'no-blank',
        label: '去除空行',
        hint: '删除所有空白行',
        action: () => transform(removeBlankLines),
      },
      {
        key: 'no-space',
        label: '去除空格',
        hint: '删除半角、全角空格与制表符，保留换行',
        action: () => transform(removeSpaces),
      },
      {
        key: 'upper',
        label: '转大写',
        hint: '字母全部转为大写',
        tone: 'success',
        action: () => transform((input) => input.toUpperCase()),
      },
      {
        key: 'lower',
        label: '转小写',
        hint: '字母全部转为小写',
        tone: 'success',
        action: () => transform((input) => input.toLowerCase()),
      },
      {
        key: 'dedupe',
        label: '去除重复',
        hint: '按行去重，保留首次出现的内容',
        tone: 'danger',
        action: () => transform(dedupeLines),
      },
      {
        key: 'duplicate',
        label: '取重复值',
        hint: '只保留出现两次及以上的行，重复项只输出一次',
        tone: 'danger',
        action: () => transform(duplicateLines),
      },
      {
        key: 'reverse',
        label: '内容翻转',
        hint: '整体倒序排列',
        tone: 'warning',
        action: () => transform(reverseText),
      },
    ],
  },
  {
    title: '排序与连接',
    items: [
      {
        key: 'sort-text',
        label: '字符正向排序',
        hint: '按行做字符升序排序',
        tone: 'warning',
        action: () => transform(sortLinesAsText),
      },
      {
        key: 'sort-number',
        label: '数字正向排序',
        hint: '按每行第一个数字升序，没有数字的行排在最后',
        tone: 'warning',
        action: () => transform(sortLinesAsNumber),
      },
      {
        key: 'join-or',
        label: '使用or连接',
        hint: '非空行用「 or 」连接',
        tone: 'primary',
        action: () => transform((input) => joinLines(input, ' or ')),
      },
      {
        key: 'sql-in',
        label: 'SQL IN',
        hint: "按换行或逗号拆分，所有值加单引号，输出 IN('值1','值2');",
        tone: 'primary',
        action: () => transform(sqlInList),
      },
      {
        key: 'join-comma',
        label: '逗号分隔',
        hint: '非空行用逗号连接',
        action: () => transform((input) => joinLines(input, ',')),
      },
      {
        key: 'join-custom',
        label: '指定符号分隔',
        hint: '非空行用上方「指定符号」连接',
        tone: 'warning',
        action: () => transform((input) => joinLines(input, separator.value)),
      },
      {
        key: 'join-raw',
        label: '连接字符',
        hint: '非空行直接拼接，不添加任何分隔符',
        action: () => transform((input) => joinLines(input, '')),
      },
      {
        key: 'column-to-row',
        label: '列转行',
        hint: '每行按「指定符号」拆开，每个值单独占一行',
        tone: 'warning',
        action: () => transform((input) => splitColumnsToRows(input, separator.value)),
      },
    ],
  },
  {
    title: '表格与数据',
    items: [
      {
        key: 'table-md',
        label: '表格markdown',
        hint: '自动识别 Tab / 逗号 / 竖线 / 分号，首行作为表头',
        tone: 'success',
        action: () => transform(tableToMarkdown),
      },
      {
        key: 'url-encode',
        label: 'URLEncode',
        hint: '按 encodeURIComponent 编码当前内容',
        tone: 'success',
        action: () => transform((input) => encodeURIComponent(input)),
      },
      {
        key: 'transpose-auto',
        label: '自动转置',
        hint: '自动识别分隔符后行列互换',
        tone: 'warning',
        action: () => transform((input) => transposeTable(input)),
      },
      {
        key: 'transpose-custom',
        label: '指定符号转置',
        hint: '按「指定符号」拆列后行列互换',
        tone: 'warning',
        action: () => transform((input) => transposeTable(input, separator.value)),
      },
      {
        key: 'format-json',
        label: '格式化json',
        hint: '校验并格式化为 2 空格缩进',
        tone: 'success',
        action: () => transform(formatJson),
      },
      {
        key: 'join-tables',
        label: '多表连接(join)',
        hint: '用空行分隔多个表格，按行左右拼接为一张表',
        tone: 'warning',
        action: () => transform((input) => joinTables(input, separator.value)),
      },
      {
        key: 'table-html',
        label: '表格转HTML',
        hint: '输出带 thead / tbody 的 HTML 表格',
        tone: 'danger',
        action: () => transform(tableToHtml),
      },
      {
        key: 'mybatis',
        label: 'mybatis提取sql',
        hint: '解析 DEBUG 日志中的 Preparing / Parameters，输出可执行 SQL',
        tone: 'danger',
        action: () => transform(extractMybatisSql),
      },
    ],
  },
]
</script>

<template>
  <div class="textbox">
    <div class="tb-options">
      <span class="tb-option-label">指定符号</span>
      <a-input v-model:value="separator" size="small" class="tb-sep" placeholder=", 或 Tab" />
      <span class="tb-option-tip">
        用于「指定符号分隔 / 列转行 / 指定符号转置 / 多表连接」；自动转置与表格类操作会自动识别分隔符
      </span>
    </div>

    <section v-for="group in groups" :key="group.title" class="tb-group">
      <div class="tb-group-title">{{ group.title }}</div>
      <div class="tb-grid">
        <button
          v-for="item in group.items"
          :key="item.key"
          type="button"
          class="tb-btn"
          :class="`tone-${item.tone ?? 'default'}`"
          :title="item.hint"
          @click="item.action()"
        >
          {{ item.label }}
        </button>
      </div>
    </section>

    <section class="tb-editor">
      <header class="tb-editor-head">
        <span class="tb-editor-title">编辑区</span>
        <a-button size="small" :disabled="!text" @click="copyText">
          <template #icon><CopyOutlined /></template>
          复制
        </a-button>
        <span class="tb-stat">{{ stats }}</span>
        <span class="tb-stat">{{ undoHint }}</span>
      </header>
      <CodeArea
        :model-value="text"
        :rows="22"
        placeholder="在此粘贴或输入内容，点击上方按钮一键处理"
        @update:model-value="onEdit"
      />
    </section>
  </div>
</template>

<style scoped>
.textbox {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.tb-options {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 10px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.tb-option-label {
  font-size: 12px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.tb-sep {
  width: 90px;
}

.tb-option-tip {
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.5;
}

.tb-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tb-group-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.4px;
  text-transform: uppercase;
  color: var(--fm-muted);
}

.tb-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 6px;
}

.tb-btn {
  height: 34px;
  padding: 0 10px;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--fm-text-2);
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease, color 0.15s ease;
}

.tb-btn:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.tb-btn:active {
  transform: translateY(1px);
}

.tb-btn.tone-danger {
  color: var(--fm-danger);
  background: var(--fm-danger-soft);
  border-color: transparent;
}

.tb-btn.tone-danger:hover {
  border-color: var(--fm-danger);
  color: var(--fm-danger);
}

.tb-btn.tone-success {
  color: var(--fm-success);
  background: var(--fm-success-soft);
  border-color: transparent;
}

.tb-btn.tone-success:hover {
  border-color: var(--fm-success);
  color: var(--fm-success);
}

.tb-btn.tone-warning {
  color: var(--fm-warning);
  background: var(--fm-warning-soft);
  border-color: transparent;
}

.tb-btn.tone-warning:hover {
  border-color: var(--fm-warning);
  color: var(--fm-warning);
}

.tb-btn.tone-primary {
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-color: transparent;
}

.tb-btn.tone-primary:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.tb-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tb-editor-head {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.tb-editor-title {
  margin-right: 2px;
}

.tb-stat {
  margin-left: auto;
  font-size: 11px;
  font-weight: 400;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.tb-stat + .tb-stat {
  margin-left: 0;
}
</style>
