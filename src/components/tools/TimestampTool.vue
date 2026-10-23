<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, ReloadOutlined } from '@ant-design/icons-vue'
import { timestampInfo } from '@/tools/calc'

const input = ref('')
const dateInput = ref('')
const error = ref('')

const rows = computed<Array<{ label: string; value: string }>>(() => {
  error.value = ''
  try {
    return timestampInfo(input.value)
      .split('\n')
      .map((line) => {
        const index = line.indexOf('：')
        return { label: line.slice(0, index), value: line.slice(index + 1) }
      })
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return []
  }
})

function fillNow() {
  input.value = String(Math.floor(Date.now() / 1000))
}

function fillNowMs() {
  input.value = String(Date.now())
}

function fromDate() {
  if (!dateInput.value) {
    error.value = '请选择日期时间'
    return
  }
  const date = new Date(dateInput.value)
  if (Number.isNaN(date.getTime())) {
    error.value = '日期时间格式不正确'
    return
  }
  input.value = String(Math.floor(date.getTime() / 1000))
}

function useValue(value: string) {
  const digits = value.replace(/[^\d]/g, '')
  if (!digits) return
  input.value = digits.length >= 13 ? digits.slice(0, 13) : digits
}

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}
</script>

<template>
  <div class="ts">
    <div class="ts-bar">
      <a-input
        v-model:value="input"
        size="small"
        placeholder="留空表示当前时间，可填秒/毫秒时间戳或 2026-10-23 12:00:00"
        style="width: 400px"
        class="mono"
      />
      <a-button size="small" @click="fillNow"><ReloadOutlined /> 现在（秒）</a-button>
      <a-button size="small" @click="fillNowMs">现在（毫秒）</a-button>
      <span class="ts-divider" />
      <a-input v-model:value="dateInput" type="datetime-local" size="small" style="width: 210px" />
      <a-button size="small" @click="fromDate">日期转时间戳</a-button>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="ts-list">
      <div v-for="row in rows" :key="row.label" class="ts-row">
        <span class="ts-label">{{ row.label }}</span>
        <span class="ts-value mono" @click="useValue(row.value)">{{ row.value }}</span>
        <a-button type="link" size="small" @click="copy(row.value)"><CopyOutlined /></a-button>
      </div>
    </div>

    <p class="ts-tip">点击任意时间值可将其填入输入框继续换算；时区跟随本机（当前为 Asia/Shanghai）。</p>
  </div>
</template>

<style scoped>
.ts {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.ts-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.ts-divider {
  width: 1px;
  height: 20px;
  background: var(--fm-border);
  margin: 0 4px;
}

.ts-list {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}

.ts-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 8px 8px 14px;
  border-bottom: 1px solid var(--fm-border);
}

.ts-row:last-child {
  border-bottom: none;
}

.ts-label {
  width: 110px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.ts-value {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  cursor: pointer;
  word-break: break-all;
}

.ts-value:hover {
  color: var(--fm-primary);
}

.ts-tip {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
