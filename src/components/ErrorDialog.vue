<script setup lang="ts">
import { computed } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, ExclamationCircleFilled } from '@ant-design/icons-vue'
import { useErrorDialogStore } from '@/stores/errorDialog'
import { formatDateTime } from '@/utils/datetime'

const store = useErrorDialogStore()

const visible = computed({
  get: () => store.open,
  set: (value: boolean) => {
    if (!value) store.close()
  },
})

const hasDetail = computed(() => {
  const payload = store.payload
  if (!payload) return false
  return !!payload.detail && payload.detail !== payload.summary
})

const copyText = computed(() => {
  const payload = store.payload
  if (!payload) return ''
  const lines = [`${payload.title}`, `时间：${formatDateTime(payload.at)}`]
  for (const item of payload.context) lines.push(`${item.label}：${item.value}`)
  lines.push('', payload.summary)
  if (payload.detail && payload.detail !== payload.summary) lines.push('', payload.detail)
  return lines.join('\n')
})

async function copyError() {
  try {
    await navigator.clipboard.writeText(copyText.value)
    message.success('错误信息已复制')
  } catch {
    message.error('复制失败，请手动选中文本复制')
  }
}
</script>

<template>
  <a-modal
    v-model:open="visible"
    :footer="null"
    :width="640"
    :mask-closable="true"
    wrap-class-name="error-dialog-wrap"
  >
    <div v-if="store.payload" class="err-body">
      <div class="err-head">
        <ExclamationCircleFilled class="err-icon" />
        <div class="err-titles">
          <div class="err-title">{{ store.payload.title }}</div>
          <div class="err-time">{{ formatDateTime(store.payload.at) }}</div>
        </div>
      </div>

      <div class="err-summary">{{ store.payload.summary }}</div>

      <div v-if="store.payload.context.length" class="err-context">
        <div v-for="item in store.payload.context" :key="item.label" class="ctx-row">
          <span class="ctx-label">{{ item.label }}</span>
          <span class="ctx-value">{{ item.value }}</span>
        </div>
      </div>

      <template v-if="hasDetail">
        <div class="detail-head">
          <span>详细错误信息</span>
          <a class="detail-copy" @click="copyError">复制</a>
        </div>
        <pre class="detail-pre">{{ store.payload.detail }}</pre>
      </template>

      <div class="err-actions">
        <a-button @click="store.close()">关闭</a-button>
        <a-button type="primary" @click="copyError">
          <template #icon><CopyOutlined /></template>
          复制错误信息
        </a-button>
      </div>
    </div>
  </a-modal>
</template>

<style scoped>
.err-body {
  padding-top: 2px;
}

.err-head {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.err-icon {
  flex-shrink: 0;
  margin-top: 2px;
  font-size: 20px;
  color: var(--fm-danger);
}

.err-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--fm-text);
}

.err-time {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.err-summary {
  margin-top: 14px;
  padding: 10px 12px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-danger-soft);
  color: var(--fm-danger);
  font-size: 12.5px;
  line-height: 1.7;
  word-break: break-word;
}

.err-context {
  margin-top: 12px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
}

.ctx-row {
  display: flex;
  gap: 10px;
  padding: 7px 12px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 12px;
}

.ctx-row:last-child {
  border-bottom: none;
}

.ctx-label {
  width: 64px;
  flex-shrink: 0;
  color: var(--fm-muted);
}

.ctx-value {
  flex: 1;
  min-width: 0;
  color: var(--fm-text-2);
  word-break: break-all;
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
}

.detail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin: 14px 0 6px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.detail-copy {
  font-size: 12px;
}

.detail-pre {
  margin: 0;
  max-height: 240px;
  overflow: auto;
  padding: 10px 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-code-bg);
  color: var(--fm-text-2);
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
  font-size: 12px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.err-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 18px;
}
</style>
