<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, InboxOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'

const mode = ref<'encode' | 'decode'>('encode')
const dataUrl = ref('')
const fileName = ref('')
const fileSize = ref(0)
const mime = ref('')
const base64Input = ref('')
const decodeError = ref('')
const dragging = ref(false)

const pureBase64 = computed(() => dataUrl.value.replace(/^data:[^;]+;base64,/, ''))

const sizeText = computed(() => {
  if (!fileSize.value) return ''
  const value = fileSize.value
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
})

const base64SizeText = computed(() => {
  const length = pureBase64.value.length
  if (!length) return ''
  const bytes = Math.round((length * 3) / 4)
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`
})

const previewSrc = computed(() => {
  if (mode.value === 'encode') return dataUrl.value
  const text = base64Input.value.trim()
  if (!text) return ''
  return text.startsWith('data:') ? text : `data:image/png;base64,${text.replace(/\s/g, '')}`
})

function readFile(file: File) {
  if (!file.type.startsWith('image/')) {
    message.error('请选择图片文件')
    return
  }
  if (file.size > 20 * 1024 * 1024) {
    message.error('图片超过 20MB，建议先压缩')
    return
  }
  const reader = new FileReader()
  reader.onload = () => {
    dataUrl.value = String(reader.result ?? '')
    fileName.value = file.name
    fileSize.value = file.size
    mime.value = file.type
    decodeError.value = ''
  }
  reader.readAsDataURL(file)
}

function onPick(event: Event) {
  const target = event.target as HTMLInputElement
  const file = target.files?.[0]
  if (file) readFile(file)
  target.value = ''
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  dragging.value = false
  const file = event.dataTransfer?.files?.[0]
  if (file) readFile(file)
}

async function copy(text: string, label: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success(`已复制${label}`)
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}

function onImageError() {
  decodeError.value = '无法解析为图片，请确认内容是完整的 Base64 图片数据'
}

function clearAll() {
  dataUrl.value = ''
  fileName.value = ''
  fileSize.value = 0
  mime.value = ''
  base64Input.value = ''
  decodeError.value = ''
}
</script>

<template>
  <div class="img64">
    <div class="i64-bar">
      <a-segmented
        v-model:value="mode"
        size="small"
        :options="[
          { label: '图片 → Base64', value: 'encode' },
          { label: 'Base64 → 图片', value: 'decode' },
        ]"
      />
      <a-button size="small" @click="clearAll">清空</a-button>
      <span class="i64-hint">全部在本机读取，图片不会上传</span>
    </div>

    <template v-if="mode === 'encode'">
      <label
        class="i64-drop"
        :class="{ dragging }"
        @dragover.prevent="dragging = true"
        @dragleave="dragging = false"
        @drop="onDrop"
      >
        <input type="file" accept="image/*" class="i64-input" @change="onPick" />
        <InboxOutlined class="i64-icon" />
        <span class="i64-title">拖入图片，或点击选择</span>
        <span class="i64-sub">支持 PNG / JPG / GIF / WebP / SVG，单张不超过 20MB</span>
      </label>

      <div v-if="dataUrl" class="i64-body">
        <section class="i64-pane">
          <header class="i64-head">
            <span>预览</span>
            <span class="i64-stat">{{ mime }} · {{ sizeText }}</span>
          </header>
          <div class="i64-preview">
            <img :src="dataUrl" alt="预览" />
          </div>
        </section>

        <section class="i64-pane">
          <header class="i64-head">
            <span>Data URL</span>
            <span class="i64-stat">{{ pureBase64.length }} 字符 · 原始 {{ base64SizeText }}</span>
            <a-button type="link" size="small" @click="copy(dataUrl, ' Data URL')">
              <CopyOutlined /> 复制 Data URL
            </a-button>
            <a-button type="link" size="small" @click="copy(pureBase64, ' Base64')">
              <CopyOutlined /> 仅 Base64
            </a-button>
          </header>
          <CodeArea :model-value="dataUrl" :rows="18" readonly />
        </section>
      </div>
    </template>

    <template v-else>
      <div class="i64-body">
        <section class="i64-pane">
          <header class="i64-head">
            <span>Base64 内容（可带 data: 前缀）</span>
            <a-button type="link" size="small" :disabled="!base64Input" @click="copy(base64Input, ' Base64')">
              <CopyOutlined /> 复制
            </a-button>
          </header>
          <CodeArea v-model="base64Input" :rows="18" placeholder="粘贴 data:image/png;base64,… 或纯 Base64 字符串" />
        </section>

        <section class="i64-pane">
          <header class="i64-head">
            <span>图片预览</span>
            <span v-if="base64Input" class="i64-stat">{{ previewSrc.length }} 字符</span>
          </header>
          <a-alert v-if="decodeError" type="error" :message="decodeError" show-icon />
          <div v-if="previewSrc && !decodeError" class="i64-preview">
            <img :src="previewSrc" alt="解析结果" @error="onImageError" />
          </div>
          <div v-else class="i64-empty">粘贴 Base64 后在此预览</div>
        </section>
      </div>
    </template>
  </div>
</template>

<style scoped>
.img64 {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.i64-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.i64-hint {
  font-size: 12px;
  color: var(--fm-muted);
}

.i64-drop {
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

.i64-drop:hover,
.i64-drop.dragging {
  border-color: var(--fm-primary);
  background: var(--fm-surface-alt);
}

.i64-input {
  display: none;
}

.i64-icon {
  font-size: 28px;
  color: var(--fm-primary);
}

.i64-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
}

.i64-sub {
  font-size: 12px;
  color: var(--fm-muted);
}

.i64-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.i64-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.i64-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 24px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.i64-stat {
  font-size: 11px;
  color: var(--fm-muted);
  margin-right: auto;
}

.i64-preview {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 260px;
  padding: 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background-image: linear-gradient(45deg, var(--fm-surface-alt) 25%, transparent 25%),
    linear-gradient(-45deg, var(--fm-surface-alt) 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, var(--fm-surface-alt) 75%),
    linear-gradient(-45deg, transparent 75%, var(--fm-surface-alt) 75%);
  background-size: 16px 16px;
  background-position: 0 0, 0 8px, 8px -8px, -8px 0;
}

.i64-preview img {
  max-width: 100%;
  max-height: 420px;
  object-fit: contain;
}

.i64-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 260px;
  font-size: 12px;
  color: var(--fm-muted);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
}

@media (max-width: 1100px) {
  .i64-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
