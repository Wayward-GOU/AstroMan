<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, SwapOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import { diffLines, diffAsText, type DiffResult } from '@/tools/analysis'

const left = ref('')
const right = ref('')
const result = ref<DiffResult | null>(null)
const error = ref('')
const onlyDiff = ref(false)

const visible = computed(() => {
  if (!result.value) return []
  if (!onlyDiff.value) return result.value.lines
  return result.value.lines.filter((line) => line.kind !== 'same')
})

const summary = computed(() => {
  if (!result.value) return ''
  return `新增 ${result.value.added} 行 · 删除 ${result.value.removed} 行 · 相同 ${result.value.same} 行`
})

function compare() {
  error.value = ''
  try {
    result.value = diffLines(left.value, right.value)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    result.value = null
  }
}

function swap() {
  const temp = left.value
  left.value = right.value
  right.value = temp
  if (result.value) compare()
}

function loadSample() {
  left.value = `server.port=8080
spring.datasource.url=jdbc:mysql://localhost:3306/demo
spring.datasource.username=root
logging.level.root=info`
  right.value = `server.port=9090
spring.datasource.url=jdbc:mysql://localhost:3306/demo
spring.datasource.username=admin
spring.datasource.password=secret
logging.level.root=debug`
  compare()
}

async function copyDiff() {
  if (!result.value) return
  try {
    await navigator.clipboard.writeText(diffAsText(result.value))
    message.success('差异已复制')
  } catch {
    message.error('复制失败')
  }
}

function clearAll() {
  left.value = ''
  right.value = ''
  result.value = null
  error.value = ''
}
</script>

<template>
  <div class="diff">
    <div class="diff-bar">
      <a-button size="small" @click="loadSample">示例</a-button>
      <a-button type="primary" size="small" @click="compare">对比</a-button>
      <a-button size="small" @click="swap"><SwapOutlined /> 交换</a-button>
      <a-button size="small" :disabled="!result" @click="copyDiff"><CopyOutlined /> 复制差异</a-button>
      <a-button size="small" @click="clearAll">清空</a-button>
      <span class="diff-summary">{{ summary }}</span>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="diff-body">
      <section class="diff-pane">
        <header class="diff-head">原文本</header>
        <CodeArea v-model="left" :rows="16" placeholder="粘贴原始内容" />
      </section>
      <section class="diff-pane">
        <header class="diff-head">新文本</header>
        <CodeArea v-model="right" :rows="16" placeholder="粘贴修改后的内容" />
      </section>
    </div>

    <div class="diff-result">
      <div class="diff-toolbar">
        <span class="diff-title">差异结果</span>
        <a-checkbox v-model:checked="onlyDiff" :disabled="!result">仅显示差异行</a-checkbox>
      </div>
      <div v-if="!result" class="empty-hint">点击「对比」查看行级差异</div>
      <div v-else class="diff-lines scroll-y">
        <div
          v-for="(line, index) in visible"
          :key="`${index}-${line.leftNo}-${line.rightNo}`"
          class="diff-line"
          :class="`diff-${line.kind}`"
        >
          <span class="diff-no">{{ line.leftNo ?? '' }}</span>
          <span class="diff-no">{{ line.rightNo ?? '' }}</span>
          <span class="diff-mark">{{ line.kind === 'add' ? '+' : line.kind === 'remove' ? '-' : ' ' }}</span>
          <span class="diff-text">{{ line.text || ' ' }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.diff {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.diff-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.diff-summary {
  margin-left: auto;
  font-size: 12px;
  color: var(--fm-muted);
}

.diff-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.diff-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.diff-head {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.diff-result {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.diff-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.diff-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.diff-lines {
  max-height: 420px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-code-bg);
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 12px;
}

.diff-line {
  display: flex;
  align-items: flex-start;
  min-height: 20px;
  line-height: 20px;
  padding-right: 8px;
  white-space: pre-wrap;
  word-break: break-all;
}

.diff-no {
  width: 44px;
  flex-shrink: 0;
  text-align: right;
  padding-right: 8px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
  user-select: none;
}

.diff-mark {
  width: 16px;
  flex-shrink: 0;
  text-align: center;
  color: var(--fm-muted);
  user-select: none;
}

.diff-text {
  flex: 1;
  min-width: 0;
}

.diff-add {
  background: var(--fm-success-soft);
}

.diff-add .diff-mark {
  color: var(--fm-success);
}

.diff-remove {
  background: var(--fm-danger-soft);
}

.diff-remove .diff-mark {
  color: var(--fm-danger);
}

@media (max-width: 1100px) {
  .diff-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
