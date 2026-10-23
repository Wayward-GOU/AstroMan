<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, DeleteOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import { decodeJwt, type JwtParts } from '@/tools/crypto'

const token = ref('')
const error = ref('')

const parts = computed<JwtParts | null>(() => {
  if (!token.value.trim()) return null
  try {
    error.value = ''
    return decodeJwt(token.value)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return null
  }
})

const claims = computed(() => {
  if (!parts.value) return []
  try {
    const payload = JSON.parse(parts.value.payload) as Record<string, unknown>
    const rows: Array<{ key: string; value: string }> = []
    const formatTime = (value: unknown) => {
      if (typeof value !== 'number') return ''
      const date = new Date(value * 1000)
      return `${date.toLocaleString('zh-CN')}（${value}）`
    }
    if (payload.iat !== undefined) rows.push({ key: 'iat 签发时间', value: formatTime(payload.iat) })
    if (payload.nbf !== undefined) rows.push({ key: 'nbf 生效时间', value: formatTime(payload.nbf) })
    if (payload.exp !== undefined) {
      const exp = Number(payload.exp)
      const left = exp - Math.floor(Date.now() / 1000)
      rows.push({
        key: 'exp 过期时间',
        value: `${formatTime(payload.exp)} ${left > 0 ? `（剩余 ${Math.ceil(left / 60)} 分钟）` : '（已过期）'}`,
      })
    }
    if (payload.iss !== undefined) rows.push({ key: 'iss 签发者', value: String(payload.iss) })
    if (payload.sub !== undefined) rows.push({ key: 'sub 主体', value: String(payload.sub) })
    if (payload.aud !== undefined) rows.push({ key: 'aud 受众', value: JSON.stringify(payload.aud) })
    if (payload.jti !== undefined) rows.push({ key: 'jti 编号', value: String(payload.jti) })
    return rows
  } catch {
    return []
  }
})

const status = computed(() => {
  if (!parts.value) return null
  try {
    const payload = JSON.parse(parts.value.payload) as Record<string, unknown>
    if (typeof payload.exp !== 'number') return { ok: true, text: '未设置过期时间' }
    const left = Number(payload.exp) - Math.floor(Date.now() / 1000)
    if (left <= 0) return { ok: false, text: `已过期 ${Math.ceil(-left / 60)} 分钟` }
    if (left < 300) return { ok: false, text: `即将过期（剩余 ${left} 秒）` }
    return { ok: true, text: `有效，剩余 ${Math.ceil(left / 60)} 分钟` }
  } catch {
    return null
  }
})

function loadSample() {
  token.value =
    'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkZpbmtNZXRlciIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoxOTAwMDAwMDAwLCJpc3MiOiJmaW5rbWV0ZXIifQ.3q7H0mZ4cJ0Yw6uJz8l2n9pQx1sT5vB7kR4eW2yA8dM'
}

async function copy(text: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function clearAll() {
  token.value = ''
  error.value = ''
}
</script>

<template>
  <div class="jwt">
    <div class="jwt-bar">
      <a-button size="small" @click="loadSample">示例 Token</a-button>
      <a-button size="small" @click="clearAll"><DeleteOutlined /> 清空</a-button>
      <a-tag v-if="status" :color="status.ok ? 'green' : 'red'">{{ status.text }}</a-tag>
    </div>

    <CodeArea v-model="token" :rows="4" placeholder="粘贴 JWT，可带 Bearer 前缀" />

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div v-if="parts" class="jwt-body">
      <section class="jwt-pane">
        <header class="jwt-head">
          <span>Header</span>
          <a-button type="link" size="small" @click="copy(parts.header)"><CopyOutlined /> 复制</a-button>
        </header>
        <CodeArea :model-value="parts.header" :rows="8" readonly />
      </section>
      <section class="jwt-pane">
        <header class="jwt-head">
          <span>Payload</span>
          <a-button type="link" size="small" @click="copy(parts.payload)"><CopyOutlined /> 复制</a-button>
        </header>
        <CodeArea :model-value="parts.payload" :rows="8" readonly />
      </section>
    </div>

    <div v-if="parts" class="jwt-sig">
      <span class="jwt-sig-label">Signature</span>
      <span class="jwt-sig-value mono">{{ parts.signature || '（无签名段）' }}</span>
    </div>

    <div v-if="claims.length" class="jwt-claims">
      <div class="jwt-claims-title">时间与声明</div>
      <div v-for="item in claims" :key="item.key" class="jwt-claim">
        <span class="jwt-claim-key">{{ item.key }}</span>
        <span class="jwt-claim-value">{{ item.value }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.jwt {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.jwt-bar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.jwt-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.jwt-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.jwt-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.jwt-sig {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  min-width: 0;
}

.jwt-sig-label {
  font-size: 12px;
  color: var(--fm-muted);
  flex-shrink: 0;
}

.jwt-sig-value {
  font-size: 12px;
  word-break: break-all;
  color: var(--fm-text-2);
}

.jwt-claims {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
}

.jwt-claims-title {
  padding: 8px 12px;
  font-size: 12px;
  font-weight: 500;
  background: var(--fm-surface-alt);
  color: var(--fm-text-2);
}

.jwt-claim {
  display: flex;
  gap: 16px;
  padding: 7px 12px;
  font-size: 12px;
  border-top: 1px solid var(--fm-border);
}

.jwt-claim-key {
  width: 110px;
  flex-shrink: 0;
  color: var(--fm-muted);
}

.jwt-claim-value {
  color: var(--fm-text);
}

@media (max-width: 1100px) {
  .jwt-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
