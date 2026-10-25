<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import * as api from '@/api'
import type { IpLookupInfo } from '@/types/models'

const presets = ['8.8.8.8', '1.1.1.1', '114.114.114.114', '223.5.5.5']

const queryIp = ref('')
const loading = ref(false)
const error = ref('')
const info = ref<IpLookupInfo | null>(null)
const queriedAt = ref('')
// 最近一次成功查询的目标 IP，空字符串表示查询本机
const queriedTarget = ref('')

const isCustom = computed(() => queriedTarget.value !== '')

const fields = computed(() => {
  const item = info.value
  if (!item) return []
  return [
    { label: '国家', value: item.country },
    { label: '省份', value: item.prov },
    { label: '城市', value: item.city },
    { label: '区县', value: item.area },
    { label: '运营商', value: item.isp },
    { label: '大区', value: item.big_area },
    { label: '邮编', value: item.post_code },
    { label: '区号', value: item.area_code },
    { label: '经纬度', value: item.lng && item.lat ? `${item.lng}, ${item.lat}` : '' },
    { label: '整数 IP', value: item.long_ip ? String(item.long_ip) : '' },
  ].map((row) => ({ label: row.label, value: row.value?.trim() || '-' }))
})

async function query() {
  if (loading.value) return
  const target = queryIp.value.trim()
  loading.value = true
  error.value = ''
  try {
    info.value = await api.ipLookup(target)
    queriedTarget.value = target
    queriedAt.value = new Date().toLocaleTimeString('zh-CN', { hour12: false })
  } catch (e) {
    info.value = null
    error.value = typeof e === 'string' ? e : String(e)
  } finally {
    loading.value = false
  }
}

function usePreset(ip: string) {
  queryIp.value = ip
  query()
}

onMounted(query)

async function copy(value: string) {
  if (!value || value === '-') return
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}
</script>

<template>
  <div class="ipq">
    <div class="ipq-bar">
      <div class="ipq-query">
        <a-input
          v-model:value="queryIp"
          class="ipq-input mono"
          placeholder="留空查询本机，或输入要查询的 IP"
          allow-clear
          @press-enter="query"
        />
        <a-button type="primary" :loading="loading" @click="query">查询</a-button>
      </div>
      <div class="ipq-quick">
        <span class="ipq-bar-tip">常用</span>
        <a-tag v-for="item in presets" :key="item" class="ipq-tag" @click="usePreset(item)">
          {{ item }}
        </a-tag>
        <span class="ipq-bar-tip ipq-source">数据来源 ip9.com.cn</span>
      </div>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <a-spin :spinning="loading">
      <div v-if="info" class="ipq-card">
        <div class="ipq-hero">
          <div class="ipq-hero-main">
            <div class="ipq-hero-label">
              {{ isCustom ? '查询 IP' : '本机公网 IP' }}
              <a-tag v-if="isCustom" color="blue" class="ipq-badge">指定查询</a-tag>
            </div>
            <div class="ipq-hero-ip mono">{{ info.ip }}</div>
          </div>
          <div class="ipq-hero-side">
            <a-button type="text" size="small" @click="copy(info.ip)">
              <CopyOutlined /> 复制
            </a-button>
            <span v-if="queriedAt" class="ipq-time">更新于 {{ queriedAt }}</span>
          </div>
        </div>

        <div class="ipq-fields">
          <div v-for="field in fields" :key="field.label" class="ipq-field">
            <span class="ipq-label">{{ field.label }}</span>
            <span class="ipq-value mono">{{ field.value }}</span>
            <a-button type="link" size="small" @click="copy(field.value)"><CopyOutlined /></a-button>
          </div>
        </div>
      </div>
      <div v-else-if="!loading && !error" class="ipq-empty">暂无数据，点击「重新查询」重试</div>
    </a-spin>
  </div>
</template>

<style scoped>
.ipq {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.ipq-bar {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.ipq-query {
  display: flex;
  gap: 8px;
}

.ipq-input {
  max-width: 340px;
}

.ipq-quick {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.ipq-bar-tip {
  font-size: 12px;
  color: var(--fm-muted);
}

.ipq-tag {
  margin: 0;
  cursor: pointer;
  font-family: var(--fm-mono, monospace);
}

.ipq-tag:hover {
  color: var(--fm-primary);
  border-color: var(--fm-primary);
}

.ipq-source {
  margin-left: auto;
}

.ipq-badge {
  margin-left: 8px;
  font-size: 11px;
}

.ipq-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.ipq-hero {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px;
  background: var(--fm-primary-soft);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.ipq-hero-label {
  display: flex;
  align-items: center;
  font-size: 12px;
  color: var(--fm-muted);
  margin-bottom: 6px;
}

.ipq-hero-ip {
  font-size: 24px;
  font-weight: 700;
  letter-spacing: 0.5px;
  color: var(--fm-primary);
  word-break: break-all;
}

.ipq-hero-side {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
  flex-shrink: 0;
}

.ipq-time {
  font-size: 11px;
  color: var(--fm-muted);
}

.ipq-fields {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 8px;
}

.ipq-field {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 6px 6px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.ipq-label {
  width: 72px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.ipq-value {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  word-break: break-all;
}

.ipq-empty {
  padding: 32px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}
</style>
