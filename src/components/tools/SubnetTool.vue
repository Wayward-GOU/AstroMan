<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import { ipRange, subnetInfo, type SubnetInfo } from '@/tools/network'
import { ipToBin } from '@/tools/network'

const input = ref('192.168.1.10/24')
const showRange = ref(false)
const error = ref('')

const info = computed<SubnetInfo | null>(() => {
  error.value = ''
  if (!input.value.trim()) return null
  try {
    return subnetInfo(input.value)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return null
  }
})

const binary = computed(() => {
  if (!info.value) return null
  try {
    return ipToBin(info.value.ip)
  } catch {
    return null
  }
})

const range = computed(() => {
  if (!showRange.value || !info.value) return ''
  try {
    return ipRange(input.value)
  } catch {
    return ''
  }
})

const fields = computed(() => {
  if (!info.value) return []
  return [
    { label: '网络地址', value: info.value.network },
    { label: '子网掩码', value: info.value.netmask },
    { label: '通配符掩码', value: info.value.wildcard },
    { label: '广播地址', value: info.value.broadcast },
    { label: '可用地址范围', value: `${info.value.firstHost} - ${info.value.lastHost}` },
    { label: '可用主机数', value: String(info.value.hostCount) },
    { label: 'CIDR', value: info.value.cidr },
    { label: '地址分类', value: info.value.ipClass },
    { label: '地址属性', value: info.value.scope },
  ]
})

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function usePreset(value: string) {
  input.value = value
}
</script>

<template>
  <div class="sn">
    <div class="sn-bar">
      <a-input
        v-model:value="input"
        size="small"
        placeholder="192.168.1.10/24 或 10.0.0.0/8"
        style="width: 260px"
        class="mono"
        allow-clear
      />
      <a-checkbox v-model:checked="showRange">列出地址段</a-checkbox>
      <span class="sn-presets">
        <a-tag v-for="preset in ['10.0.0.0/8', '172.16.0.0/12', '192.168.1.0/24', '127.0.0.1/32']" :key="preset" @click="usePreset(preset)">
          {{ preset }}
        </a-tag>
      </span>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <template v-if="info">
      <div class="sn-fields">
        <div v-for="field in fields" :key="field.label" class="sn-field">
          <span class="sn-label">{{ field.label }}</span>
          <span class="sn-value mono">{{ field.value }}</span>
          <a-button type="link" size="small" @click="copy(field.value)"><CopyOutlined /></a-button>
        </div>
      </div>

      <div v-if="binary" class="sn-binary">
        <span class="sn-binary-label">二进制</span>
        <div class="sn-binary-value mono">
          <div>{{ binary.dot }}</div>
          <div class="sn-binary-sub">{{ binary.grouped }}</div>
        </div>
      </div>

      <div v-if="showRange" class="sn-range">
        <header class="sn-range-head">
          <span>可用地址列表</span>
          <a-button type="link" size="small" @click="copy(range)"><CopyOutlined /> 复制</a-button>
        </header>
        <pre class="sn-range-body mono scroll-y">{{ range }}</pre>
      </div>
    </template>
  </div>
</template>

<style scoped>
.sn {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.sn-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.sn-presets {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.sn-presets :deep(.ant-tag) {
  cursor: pointer;
  margin: 0;
}

.sn-fields {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 8px;
}

.sn-field {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 6px 6px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.sn-label {
  width: 92px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.sn-value {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  word-break: break-all;
}

.sn-binary {
  display: flex;
  gap: 12px;
  padding: 10px 14px;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.sn-binary-label {
  font-size: 12px;
  color: var(--fm-muted);
  flex-shrink: 0;
}

.sn-binary-value {
  font-size: 12px;
  color: var(--fm-text-2);
  line-height: 20px;
}

.sn-binary-sub {
  color: var(--fm-muted);
}

.sn-range {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
}

.sn-range-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 6px 4px 12px;
  font-size: 12px;
  color: var(--fm-text-2);
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.sn-range-body {
  margin: 0;
  padding: 10px 14px;
  max-height: 260px;
  overflow: auto;
  font-size: 12px;
  line-height: 19px;
  color: var(--fm-text-2);
  background: var(--fm-code-bg);
}
</style>
