<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import { ASCII_ROWS, HEADER_ROWS, HTTP_STATUS_ROWS, MIME_ROWS } from '@/tools/reference'
import { COMMON_PORTS } from '@/tools/network'

const props = defineProps<{ kind: 'ascii' | 'http' | 'mime' | 'headers' | 'ports' }>()

const filter = ref('')

interface Row {
  cells: string[]
  copy?: string
}

const columns = computed<string[]>(() => {
  switch (props.kind) {
    case 'ascii':
      return ['十进制', '十六进制', '二进制', '字符', '说明']
    case 'http':
      return ['状态码', '名称', '含义', '分类']
    case 'mime':
      return ['MIME 类型', '扩展名', '说明']
    case 'headers':
      return ['请求头', '作用', '示例']
    default:
      return ['端口', '服务', '说明']
  }
})

const rows = computed<Row[]>(() => {
  const keyword = filter.value.trim().toLowerCase()
  const match = (text: string) => !keyword || text.toLowerCase().includes(keyword)
  switch (props.kind) {
    case 'ascii':
      return ASCII_ROWS.filter((row) => match(`${row.dec} ${row.hex} ${row.bin} ${row.char} ${row.note}`)).map(
        (row) => ({
          cells: [String(row.dec), row.hex, row.bin, row.char, row.note],
          copy: row.char,
        }),
      )
    case 'http':
      return HTTP_STATUS_ROWS.filter((row) => match(`${row.code} ${row.name} ${row.desc} ${row.level}`)).map(
        (row) => ({ cells: [String(row.code), row.name, row.desc, row.level], copy: String(row.code) }),
      )
    case 'mime':
      return MIME_ROWS.filter((row) => match(`${row.type} ${row.ext} ${row.desc}`)).map((row) => ({
        cells: [row.type, row.ext, row.desc],
        copy: row.type,
      }))
    case 'headers':
      return HEADER_ROWS.filter((row) => match(`${row.header} ${row.desc} ${row.example}`)).map((row) => ({
        cells: [row.header, row.desc, row.example],
        copy: row.header,
      }))
    default:
      return COMMON_PORTS.filter((row) => match(`${row.port} ${row.service} ${row.note}`)).map((row) => ({
        cells: [String(row.port), row.service, row.note],
        copy: String(row.port),
      }))
  }
})

async function copy(text?: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}
</script>

<template>
  <div class="ref">
    <div class="ref-bar">
      <a-input v-model:value="filter" size="small" placeholder="搜索关键字" style="width: 260px" allow-clear />
      <span class="ref-count">共 {{ rows.length }} 条</span>
    </div>

    <div class="ref-table scroll-y">
      <table>
        <thead>
          <tr>
            <th v-for="column in columns" :key="column">{{ column }}</th>
            <th class="ref-op"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, index) in rows" :key="index">
            <td v-for="(cell, cellIndex) in row.cells" :key="cellIndex" :class="{ mono: cellIndex <= 2 }">
              {{ cell }}
            </td>
            <td class="ref-op">
              <a-button type="link" size="small" @click="copy(row.copy)"><CopyOutlined /></a-button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.ref {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.ref-bar {
  display: flex;
  align-items: center;
  gap: 12px;
}

.ref-count {
  font-size: 12px;
  color: var(--fm-muted);
}

.ref-table {
  max-height: 520px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface);
}

.ref-table table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.ref-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  padding: 8px 12px;
  text-align: left;
  font-weight: 500;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border-bottom: 1px solid var(--fm-border);
}

.ref-table td {
  padding: 6px 12px;
  border-bottom: 1px solid var(--fm-border);
  color: var(--fm-text);
  vertical-align: top;
}

.ref-table tbody tr:hover td {
  background: var(--fm-primary-soft);
}

.ref-op {
  width: 48px;
  text-align: right;
}
</style>
