<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import { baseConvert } from '@/tools/calc'

const input = ref('255')
const from = ref(10)
const error = ref('')

const results = computed<Array<{ base: number; label: string; value: string }>>(() => {
  error.value = ''
  if (!input.value.trim()) return []
  try {
    const map = baseConvert(input.value, from.value)
    return [2, 8, 10, 16, 32, 36].map((base) => ({
      base,
      label: `${base} 进制`,
      value: map[base],
    }))
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return []
  }
})

watch(from, () => {
  error.value = ''
})

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function setFromBase(base: number) {
  const target = results.value.find((item) => item.base === base)
  if (!target) return
  input.value = target.value
  from.value = base
}
</script>

<template>
  <div class="base">
    <div class="base-bar">
      <span class="base-label">输入</span>
      <a-input v-model:value="input" size="small" placeholder="输入数值" style="width: 280px" class="mono" />
      <span class="base-label">作为</span>
      <a-select v-model:value="from" size="small" style="width: 120px">
        <a-select-option :value="2">2 进制</a-select-option>
        <a-select-option :value="8">8 进制</a-select-option>
        <a-select-option :value="10">10 进制</a-select-option>
        <a-select-option :value="16">16 进制</a-select-option>
        <a-select-option :value="32">32 进制</a-select-option>
        <a-select-option :value="36">36 进制</a-select-option>
      </a-select>
      <a-tag v-if="from === 16" color="blue">可带 0x 前缀</a-tag>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="base-grid">
      <div v-for="item in results" :key="item.base" class="base-item">
        <div class="base-item-head">
          <span class="base-item-label">{{ item.label }}</span>
          <a-button type="link" size="small" @click="copy(item.value)"><CopyOutlined /></a-button>
        </div>
        <div class="base-item-value mono" @click="setFromBase(item.base)">{{ item.value }}</div>
      </div>
    </div>

    <p class="base-tip">点击任意结果可将其设为新的输入值并切换对应进制。</p>
  </div>
</template>

<style scoped>
.base {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.base-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.base-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.base-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 10px;
}

.base-item {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}

.base-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 6px 4px 12px;
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.base-item-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.base-item-value {
  padding: 12px;
  font-size: 14px;
  color: var(--fm-text);
  word-break: break-all;
  cursor: pointer;
  transition: background 0.15s;
}

.base-item-value:hover {
  background: var(--fm-primary-soft);
}

.base-tip {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
