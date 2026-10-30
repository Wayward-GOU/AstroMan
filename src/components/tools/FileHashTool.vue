<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, InboxOutlined } from '@ant-design/icons-vue'
import CryptoJS from 'crypto-js'

interface HashRow {
  label: string
  value: string
}

const fileName = ref('')
const fileSize = ref(0)
const lastModified = ref('')
const running = ref(false)
const error = ref('')
const rows = ref<HashRow[]>([])

const sizeText = computed(() => {
  if (!fileSize.value) return ''
  const units = ['B', 'KB', 'MB', 'GB']
  let value = fileSize.value
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value.toFixed(index === 0 ? 0 : 2)} ${units[index]}`
})

function bytesToWordArray(bytes: Uint8Array) {
  const words: number[] = []
  for (let index = 0; index < bytes.length; index += 1) {
    words[index >>> 2] |= bytes[index] << (24 - (index % 4) * 8)
  }
  return CryptoJS.lib.WordArray.create(words, bytes.length)
}

async function md5OfFile(file: File): Promise<string> {
  const hasher = CryptoJS.algo.MD5.create()
  const chunkSize = 4 * 1024 * 1024
  let leftover = new Uint8Array(0)
  for (let offset = 0; offset < file.size; offset += chunkSize) {
    const slice = new Uint8Array(await file.slice(offset, offset + chunkSize).arrayBuffer())
    const merged = new Uint8Array(leftover.length + slice.length)
    merged.set(leftover)
    merged.set(slice, leftover.length)
    const usable = merged.length - (merged.length % 4)
    if (usable > 0) hasher.update(bytesToWordArray(merged.subarray(0, usable)))
    leftover = merged.slice(usable)
  }
  if (leftover.length) hasher.update(bytesToWordArray(leftover))
  return hasher.finalize().toString()
}

function crc32OfBytes(bytes: Uint8Array): string {
  let crc = 0xffffffff
  for (const byte of bytes) {
    crc ^= byte
    for (let bit = 0; bit < 8; bit += 1) {
      crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1
    }
  }
  return ((crc ^ 0xffffffff) >>> 0).toString(16).padStart(8, '0').toUpperCase()
}

async function handleFile(file: File) {
  error.value = ''
  rows.value = []
  fileName.value = file.name
  fileSize.value = file.size
  lastModified.value = new Date(file.lastModified).toLocaleString()
  running.value = true
  try {
    const buffer = await file.arrayBuffer()
    const bytes = new Uint8Array(buffer)
    const [sha1, sha256, sha512] = await Promise.all([
      crypto.subtle.digest('SHA-1', buffer),
      crypto.subtle.digest('SHA-256', buffer),
      crypto.subtle.digest('SHA-512', buffer),
    ])
    const hex = (value: ArrayBuffer) =>
      Array.from(new Uint8Array(value))
        .map((byte) => byte.toString(16).padStart(2, '0'))
        .join('')
    rows.value = [
      { label: 'MD5', value: await md5OfFile(file) },
      { label: 'SHA-1', value: hex(sha1) },
      { label: 'SHA-256', value: hex(sha256) },
      { label: 'SHA-512', value: hex(sha512) },
      { label: 'CRC32', value: crc32OfBytes(bytes) },
    ]
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    running.value = false
  }
}

function onPick(event: Event) {
  const target = event.target as HTMLInputElement
  const file = target.files?.[0]
  if (file) void handleFile(file)
  target.value = ''
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  const file = event.dataTransfer?.files?.[0]
  if (file) void handleFile(file)
}

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}
</script>

<template>
  <div class="file-hash">
    <label
      class="fh-drop"
      @dragover.prevent
      @drop="onDrop"
    >
      <input type="file" class="fh-input" @change="onPick" />
      <InboxOutlined class="fh-icon" />
      <span class="fh-title">拖入文件，或点击选择</span>
      <span class="fh-hint">本地流式计算 MD5 / SHA-1 / SHA-256 / SHA-512 / CRC32，文件不会上传</span>
    </label>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <a-spin :spinning="running" tip="正在计算摘要…">
      <div v-if="fileName" class="fh-meta">
        <span class="fh-name">{{ fileName }}</span>
        <span class="fh-stat">{{ sizeText }} · 修改时间 {{ lastModified }}</span>
        <a-progress v-if="running" :percent="100" status="active" :show-info="false" size="small" />
      </div>

      <div v-if="rows.length" class="fh-rows">
        <div v-for="row in rows" :key="row.label" class="fh-row">
          <span class="fh-label">{{ row.label }}</span>
          <code class="fh-value">{{ row.value }}</code>
          <a-button type="text" size="small" @click="copy(row.value)">
            <CopyOutlined />
          </a-button>
        </div>
      </div>
    </a-spin>

    <p class="fh-tip">
      用途：下载校验、发布产物核对、排查「文件被改写但大小不变」。MD5 与 CRC32 仅用于完整性校验，不要用于密码存储。
    </p>
  </div>
</template>

<style scoped>
.file-hash {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.fh-drop {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 28px 16px;
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.fh-drop:hover {
  border-color: var(--fm-primary);
  background: var(--fm-surface-alt);
}

.fh-input {
  display: none;
}

.fh-icon {
  font-size: 28px;
  color: var(--fm-primary);
}

.fh-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
}

.fh-hint {
  font-size: 12px;
  color: var(--fm-muted);
}

.fh-meta {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 8px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.fh-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
  word-break: break-all;
}

.fh-stat {
  font-size: 12px;
  color: var(--fm-muted);
}

.fh-rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 10px;
}

.fh-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.fh-label {
  width: 78px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.fh-value {
  flex: 1;
  min-width: 0;
  font-family: var(--fm-mono, monospace);
  font-size: 12px;
  color: var(--fm-text);
  word-break: break-all;
}

.fh-tip {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.6;
}
</style>
