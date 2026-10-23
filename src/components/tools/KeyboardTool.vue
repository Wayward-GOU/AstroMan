<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { KEY_CODES, type KeyCodeItem } from '@/tools/calc'

interface KeyEventInfo {
  key: string
  code: string
  keyCode: number
  location: string
  modifiers: string
  time: string
}

const current = ref<KeyEventInfo | null>(null)
const history = ref<KeyEventInfo[]>([])
const filter = ref('')

const table = KEY_CODES.filter((item) =>
  filter.value
    ? `${item.key} ${item.code} ${item.keyCode}`.toLowerCase().includes(filter.value.toLowerCase())
    : true,
)

function describeLocation(location: number): string {
  return ['标准按键', '左侧', '右侧', '数字键盘'][location] ?? '未知'
}

function onKeydown(event: KeyboardEvent) {
  const info: KeyEventInfo = {
    key: event.key === ' ' ? 'Space' : event.key,
    code: event.code,
    keyCode: event.keyCode,
    location: describeLocation(event.location),
    modifiers: [
      event.ctrlKey ? 'Ctrl' : '',
      event.shiftKey ? 'Shift' : '',
      event.altKey ? 'Alt' : '',
      event.metaKey ? 'Meta' : '',
    ]
      .filter(Boolean)
      .join(' + ') || '无',
    time: new Date().toLocaleTimeString('zh-CN'),
  }
  current.value = info
  history.value = [info, ...history.value].slice(0, 10)
  if (['Tab', 'F5', 'F11'].includes(event.key)) event.preventDefault()
}

function findCode(item: KeyCodeItem) {
  filter.value = item.code
}

function reset() {
  current.value = null
  history.value = []
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div class="kb">
    <div class="kb-live">
      <div class="kb-live-label">按下任意键查看信息</div>
      <div class="kb-live-value mono">{{ current?.key ?? '—' }}</div>
      <div class="kb-live-meta">
        <span>code：<b class="mono">{{ current?.code ?? '—' }}</b></span>
        <span>keyCode：<b class="mono">{{ current?.keyCode ?? '—' }}</b></span>
        <span>位置：<b>{{ current?.location ?? '—' }}</b></span>
        <span>修饰键：<b>{{ current?.modifiers ?? '—' }}</b></span>
      </div>
      <a-button size="small" @click="reset">清空记录</a-button>
    </div>

    <div v-if="history.length" class="kb-history">
      <span class="kb-history-title">最近按键</span>
      <a-tag v-for="(item, index) in history" :key="`${item.code}-${index}`" class="mono">
        {{ item.key }} · {{ item.code }} · {{ item.keyCode }}
      </a-tag>
    </div>

    <div class="kb-table-head">
      <span class="kb-table-title">按键码对照（共 {{ table.length }} 项）</span>
      <a-input v-model:value="filter" size="small" placeholder="搜索 key / code / keyCode" style="width: 240px" allow-clear />
    </div>

    <div class="kb-table scroll-y">
      <table>
        <thead>
          <tr>
            <th>按键</th>
            <th>event.code</th>
            <th>keyCode</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in table" :key="item.code" @click="findCode(item)">
            <td>{{ item.key }}</td>
            <td class="mono">{{ item.code }}</td>
            <td class="mono">{{ item.keyCode }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.kb {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.kb-live {
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: center;
  padding: 20px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.kb-live-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.kb-live-value {
  font-size: 30px;
  font-weight: 600;
  color: var(--fm-primary);
  min-height: 38px;
}

.kb-live-meta {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 18px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.kb-history {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.kb-history-title {
  font-size: 12px;
  color: var(--fm-muted);
}

.kb-table-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.kb-table-title {
  font-size: 12px;
  color: var(--fm-text-2);
  font-weight: 500;
}

.kb-table {
  max-height: 360px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.kb-table table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.kb-table th {
  position: sticky;
  top: 0;
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-weight: 500;
  text-align: left;
  padding: 7px 12px;
  border-bottom: 1px solid var(--fm-border);
}

.kb-table td {
  padding: 6px 12px;
  border-bottom: 1px solid var(--fm-border);
  color: var(--fm-text);
}

.kb-table tbody tr {
  cursor: pointer;
}

.kb-table tbody tr:hover td {
  background: var(--fm-primary-soft);
}
</style>
