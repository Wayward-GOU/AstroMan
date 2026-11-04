<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { save } from '@tauri-apps/plugin-dialog'
import {
  DownloadOutlined,
  LeftOutlined,
  RightOutlined,
  RotateRightOutlined,
  ZoomInOutlined,
  ZoomOutOutlined,
} from '@ant-design/icons-vue'
import { notesAttachmentData, notesAttachmentSave } from '@/api/notes'
import { toDataUrl } from '@/utils/notesImage'
import type { NoteFile } from '@/types/notes'

const props = defineProps<{
  open: boolean
  files: NoteFile[]
  index: number
}>()

const emit = defineEmits<{
  (event: 'update:open', value: boolean): void
  (event: 'update:index', value: number): void
  (event: 'remove', fileId: number): void
}>()

const dataUrl = ref('')
const loading = ref(false)
const zoom = ref(1)
const rotation = ref(0)

const current = computed(() => props.files[props.index] ?? null)

watch(
  () => [props.open, current.value?.id] as const,
  async ([open, fileId]) => {
    if (!open || !fileId) {
      dataUrl.value = ''
      return
    }
    loading.value = true
    zoom.value = 1
    rotation.value = 0
    try {
      const data = await notesAttachmentData(fileId)
      dataUrl.value = toDataUrl(data.base64, data.mime)
    } catch (error) {
      message.error(String(error))
      dataUrl.value = ''
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)

function close() {
  emit('update:open', false)
}

function step(delta: number) {
  if (props.files.length < 2) return
  const next = (props.index + delta + props.files.length) % props.files.length
  emit('update:index', next)
}

async function saveAs() {
  if (!current.value) return
  try {
    const path = await save({
      title: '保存图片',
      defaultPath: current.value.fileName || 'note-image.png',
    })
    if (!path) return
    await notesAttachmentSave(current.value.id, path)
    message.success('图片已保存')
  } catch (error) {
    message.error(String(error))
  }
}
</script>

<template>
  <a-modal
    :open="open"
    :footer="null"
    :width="'92vw'"
    wrap-class-name="notes-image-modal"
    destroy-on-close
    @cancel="close"
  >
    <div class="viewer">
      <div class="viewer-head">
        <div class="viewer-title">
          {{ current?.fileName || '图片预览' }}
          <span class="viewer-size">{{ index + 1 }} / {{ files.length }}</span>
        </div>
        <a-space :size="4">
          <a-tooltip title="缩小">
            <a-button size="small" :disabled="zoom <= 0.4" @click="zoom = Math.max(0.4, zoom - 0.2)">
              <template #icon><ZoomOutOutlined /></template>
            </a-button>
          </a-tooltip>
          <a-tooltip title="放大">
            <a-button size="small" :disabled="zoom >= 3" @click="zoom = Math.min(3, zoom + 0.2)">
              <template #icon><ZoomInOutlined /></template>
            </a-button>
          </a-tooltip>
          <a-tooltip title="旋转 90°">
            <a-button size="small" @click="rotation = (rotation + 90) % 360">
              <template #icon><RotateRightOutlined /></template>
            </a-button>
          </a-tooltip>
          <a-tooltip title="另存为">
            <a-button size="small" @click="saveAs">
              <template #icon><DownloadOutlined /></template>
            </a-button>
          </a-tooltip>
        </a-space>
      </div>

      <div class="viewer-stage">
        <a-spin v-if="loading" />
        <img
          v-else-if="dataUrl"
          :src="dataUrl"
          alt="附件预览"
          :style="{ transform: `scale(${zoom}) rotate(${rotation}deg)` }"
        />
        <div v-else class="viewer-empty">图片不可用</div>
        <button
          v-if="files.length > 1"
          class="viewer-nav prev"
          type="button"
          title="上一张"
          @click="step(-1)"
        >
          <LeftOutlined />
        </button>
        <button
          v-if="files.length > 1"
          class="viewer-nav next"
          type="button"
          title="下一张"
          @click="step(1)"
        >
          <RightOutlined />
        </button>
      </div>

      <div v-if="files.length > 1" class="viewer-thumbs">
        <button
          v-for="(file, i) in files"
          :key="file.id"
          type="button"
          class="thumb"
          :class="{ active: i === index }"
          :title="file.fileName"
          @click="emit('update:index', i)"
        >
          {{ i + 1 }}
        </button>
      </div>
    </div>
  </a-modal>
</template>

<style scoped>
.viewer {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 60vh;
}

.viewer-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.viewer-title {
  font-weight: 600;
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.viewer-size {
  font-size: 11px;
  color: var(--fm-muted);
  font-weight: 400;
}

.viewer-stage {
  position: relative;
  flex: 1;
  min-height: 56vh;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  background: var(--fm-bg);
  border-radius: 8px;
}

.viewer-stage img {
  max-width: 100%;
  max-height: 62vh;
  transition: transform 0.12s ease;
  user-select: none;
}

.viewer-empty {
  color: var(--fm-muted);
}

.viewer-nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  width: 34px;
  height: 34px;
  border-radius: 50%;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  color: var(--fm-text);
  cursor: pointer;
  display: grid;
  place-items: center;
}

.viewer-nav:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.viewer-nav.prev {
  left: 10px;
}

.viewer-nav.next {
  right: 10px;
}

.viewer-thumbs {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.thumb {
  width: 28px;
  height: 28px;
  border-radius: 6px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  color: var(--fm-text-2);
  cursor: pointer;
}

.thumb.active {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}
</style>
