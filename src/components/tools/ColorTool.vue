<script setup lang="ts">
import { computed, ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import { colorInfo, type ColorInfo } from '@/tools/calc'

const input = ref('#3D6B8F')
const picker = ref('#3D6B8F')
const error = ref('')

const info = computed<ColorInfo | null>(() => {
  error.value = ''
  try {
    return colorInfo(input.value)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return null
  }
})

function onPick(event: Event) {
  const target = event.target as HTMLInputElement
  picker.value = target.value
  input.value = target.value.toUpperCase()
}

function useValue(value: string) {
  input.value = value
  if (/^#[0-9a-fA-F]{6}$/.test(value)) picker.value = value
}

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

const fields = computed(() => {
  if (!info.value) return []
  return [
    { label: 'HEX', value: info.value.hex },
    { label: 'RGB', value: info.value.rgb },
    { label: 'RGBA', value: info.value.rgba },
    { label: 'HSL', value: info.value.hsl },
    { label: 'HSV', value: info.value.hsv },
    { label: 'CMYK', value: info.value.cmyk },
  ]
})
</script>

<template>
  <div class="color">
    <div class="color-bar">
      <label class="color-picker">
        <input type="color" :value="picker" @input="onPick" />
        <span class="color-swatch" :style="{ background: info?.hex ?? 'transparent' }" />
      </label>
      <a-input v-model:value="input" size="small" placeholder="#3D6B8F / rgb(61,107,143) / hsl(207,40%,40%)" style="width: 320px" class="mono" />
      <a-tag v-if="info" color="blue">{{ info.luminance > 0.5 ? '浅色底' : '深色底' }}</a-tag>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <template v-if="info">
      <div class="color-fields">
        <div v-for="field in fields" :key="field.label" class="color-field">
          <span class="color-field-label">{{ field.label }}</span>
          <span class="color-field-value mono" @click="useValue(field.value)">{{ field.value }}</span>
          <a-button type="link" size="small" @click="copy(field.value)"><CopyOutlined /></a-button>
        </div>
      </div>

      <div class="color-extra">
        <div class="color-extra-item">
          <span class="color-extra-label">相对亮度</span>
          <span class="color-extra-value mono">{{ info.luminance }}</span>
        </div>
        <div class="color-extra-item">
          <span class="color-extra-label">与白色对比度</span>
          <span class="color-extra-value mono">{{ info.contrastWithWhite }} : 1</span>
          <a-tag :color="info.contrastWithWhite >= 4.5 ? 'green' : info.contrastWithWhite >= 3 ? 'orange' : 'red'">
            {{ info.contrastWithWhite >= 4.5 ? 'AA 达标' : info.contrastWithWhite >= 3 ? '仅大字达标' : '不达标' }}
          </a-tag>
        </div>
        <div class="color-extra-item">
          <span class="color-extra-label">与黑色对比度</span>
          <span class="color-extra-value mono">{{ info.contrastWithBlack }} : 1</span>
          <a-tag :color="info.contrastWithBlack >= 4.5 ? 'green' : info.contrastWithBlack >= 3 ? 'orange' : 'red'">
            {{ info.contrastWithBlack >= 4.5 ? 'AA 达标' : info.contrastWithBlack >= 3 ? '仅大字达标' : '不达标' }}
          </a-tag>
        </div>
      </div>

      <div class="color-preview" :style="{ background: info.hex }">
        <span class="color-preview-text" :style="{ color: info.contrastWithWhite > 4.5 ? '#ffffff' : '#0f172a' }">
          FbinHttp · {{ info.hex }}
        </span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.color {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.color-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.color-picker {
  position: relative;
  width: 34px;
  height: 34px;
  cursor: pointer;
}

.color-picker input {
  position: absolute;
  inset: 0;
  opacity: 0;
  cursor: pointer;
}

.color-swatch {
  display: block;
  width: 34px;
  height: 34px;
  border-radius: 6px;
  border: 1px solid var(--fm-border-strong);
}

.color-fields {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 8px;
}

.color-field {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 6px 6px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.color-field-label {
  width: 46px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.color-field-value {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  cursor: pointer;
  word-break: break-all;
}

.color-extra {
  display: flex;
  flex-direction: column;
  gap: 6px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  padding: 10px 12px;
}

.color-extra-item {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
}

.color-extra-label {
  width: 110px;
  color: var(--fm-muted);
}

.color-extra-value {
  color: var(--fm-text);
}

.color-preview {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 90px;
  border-radius: var(--fm-radius);
  border: 1px solid var(--fm-border);
}

.color-preview-text {
  font-size: 16px;
  font-weight: 600;
  letter-spacing: 0.4px;
}
</style>
