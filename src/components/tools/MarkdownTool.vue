<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, EyeOutlined, FileTextOutlined, ThunderboltOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import SplitLayoutSwitch from '@/components/split/SplitLayoutSwitch.vue'
import SplitPane from '@/components/split/SplitPane.vue'
import { useSplitLayout } from '@/composables/useSplitLayout'
import { renderMarkdown } from '@/utils/markdown'
import { copyToClipboard } from '@/utils/clipboard'

const { direction, isVertical, setDirection, ratioKey } = useSplitLayout('astroman:tools:markdown')

const SAMPLE = `# AstroMan 工具箱

支持 **Markdown 实时预览**、表格、代码块与任务列表，渲染在本机完成。

## 常用语法

| 语法 | 效果 |
| --- | --- |
| \`**粗体**\` | **粗体** |
| \`*斜体*\` | *斜体* |
| \`[链接](https://example.com)\` | [链接](https://example.com) |

## 代码块

\`\`\`bash
curl -X POST https://api.example.com/v1/users \\
  -H "Content-Type: application/json" \\
  -d '{"name":"张三"}'
\`\`\`

- [x] 支持任务列表
- [ ] 支持复制 HTML
- 支持行内代码 \`npm run dev\`

> 提示：原始 HTML 标签会被当作纯文本，避免脚本注入。`

const source = ref(SAMPLE)
const view = ref<'preview' | 'html'>('preview')

const html = computed(() => {
  try {
    return renderMarkdown(source.value)
  } catch (err) {
    return `<p>渲染失败：${err instanceof Error ? err.message : String(err)}</p>`
  }
})

const stats = computed(() => {
  const text = source.value
  if (!text) return ''
  const lines = text.split('\n').length
  return `${lines} 行 · ${text.length} 字符`
})

async function copy() {
  try {
    await copyToClipboard(view.value === 'html' ? html.value : source.value)
    message.success(view.value === 'html' ? '已复制 HTML' : '已复制 Markdown')
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}

function loadSample() {
  source.value = SAMPLE
  view.value = 'preview'
}
</script>

<template>
  <div class="md-tool">
    <div class="md-bar">
      <a-segmented
        v-model:value="view"
        size="small"
        :options="[
          { label: '预览', value: 'preview' },
          { label: 'HTML 源码', value: 'html' },
        ]"
      />
      <span class="md-stat">{{ stats }}</span>
      <a-button size="small" @click="loadSample"><ThunderboltOutlined /> 示例</a-button>
      <a-button size="small" @click="copy"><CopyOutlined /> 复制{{ view === 'html' ? ' HTML' : '原文' }}</a-button>
      <SplitLayoutSwitch :model-value="direction" @update:model-value="setDirection" />
    </div>

    <SplitPane
      class="md-body"
      :class="{ 'is-vertical': isVertical }"
      :direction="direction"
      :storage-key="ratioKey"
      :min-first="isVertical ? 150 : 260"
      :min-second="isVertical ? 150 : 260"
    >
      <template #first>
        <section class="md-pane">
          <header class="md-head"><FileTextOutlined /> Markdown 原文</header>
          <CodeArea v-model="source" fill placeholder="在此输入 Markdown" />
        </section>
      </template>

      <template #second>
        <section class="md-pane">
          <header class="md-head">
            <EyeOutlined v-if="view === 'preview'" />
            <FileTextOutlined v-else />
            {{ view === 'preview' ? '渲染结果' : 'HTML 源码' }}
          </header>
          <div v-if="view === 'preview'" class="md-preview markdown-body scroll-y" v-html="html" />
          <CodeArea v-else :model-value="html" fill readonly />
        </section>
      </template>
    </SplitPane>

    <p class="md-tip">渲染使用与 AI 对话相同的 Markdown 引擎：原始 HTML 关闭、链接自动加 rel="noopener noreferrer"。</p>
  </div>
</template>

<style scoped>
.md-tool {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.md-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.md-stat {
  font-size: 12px;
  color: var(--fm-muted);
  margin-right: auto;
}

.md-body {
  height: clamp(560px, 76vh, 1080px);
  min-height: 0;
}

.md-body.is-vertical {
  height: clamp(620px, 82vh, 1200px);
}

.md-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.md-head {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 24px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.md-preview {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 16px 18px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  font-size: 13px;
  line-height: 1.75;
  color: var(--fm-text);
}

.md-preview :deep(h1),
.md-preview :deep(h2),
.md-preview :deep(h3) {
  margin: 16px 0 8px;
  font-weight: 600;
  line-height: 1.4;
}

.md-preview :deep(h1) {
  font-size: 20px;
  border-bottom: 1px solid var(--fm-border);
  padding-bottom: 6px;
}

.md-preview :deep(h2) {
  font-size: 16px;
}

.md-preview :deep(h3) {
  font-size: 14px;
}

.md-preview :deep(p) {
  margin: 8px 0;
}

.md-preview :deep(a) {
  color: var(--fm-primary);
}

.md-preview :deep(code) {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--fm-surface-alt);
  font-family: var(--fm-mono, monospace);
  font-size: 12px;
}

.md-preview :deep(pre) {
  padding: 12px;
  overflow: auto;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.md-preview :deep(pre code) {
  padding: 0;
  background: transparent;
}

.md-preview :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 10px 0;
  font-size: 12px;
}

.md-preview :deep(th),
.md-preview :deep(td) {
  border: 1px solid var(--fm-border);
  padding: 6px 10px;
  text-align: left;
}

.md-preview :deep(th) {
  background: var(--fm-surface-alt);
  font-weight: 600;
}

.md-preview :deep(blockquote) {
  margin: 10px 0;
  padding: 6px 12px;
  border-left: 3px solid var(--fm-primary);
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
}

.md-preview :deep(ul),
.md-preview :deep(ol) {
  padding-left: 22px;
  margin: 8px 0;
}

.md-preview :deep(img) {
  max-width: 100%;
}

.md-tip {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

@media (max-width: 1100px) {
  .md-body {
    height: clamp(520px, 74vh, 980px);
  }

  .md-body.is-vertical {
    height: clamp(560px, 84vh, 1080px);
  }
}
</style>
