<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { save } from '@tauri-apps/plugin-dialog'
import * as api from '@/api'
import {
  buildPayload,
  buildQrSvg,
  decodeQrImage,
  describeQrPayload,
  loadImage,
  QR_TYPES,
  renderQrCanvas,
  type QrContent,
  type QrContentType,
  type QrStyle,
} from '@/tools/qrcode'

const props = withDefaults(defineProps<{ mode?: 'generate' | 'decode' }>(), {
  mode: 'generate',
})

const type = ref<QrContentType>('text')

const content = reactive<QrContent>({
  text: 'https://example.com',
  wifiSsid: 'FbinHttp-WiFi',
  wifiPassword: 'fbinhttp2026',
  wifiSecurity: 'WPA',
  wifiHidden: false,
  vName: '张三',
  vPhone: '13800138000',
  vEmail: 'zhangsan@example.com',
  vOrg: 'FbinHttp',
  vTitle: '开发工程师',
  vUrl: 'https://example.com',
  vAddress: '浙江省杭州市',
  tel: '13800138000',
  smsPhone: '13800138000',
  smsMessage: '你好',
  emailTo: 'zhangsan@example.com',
  emailSubject: '来自二维码的邮件',
  emailBody: '',
  geoLat: '30.2741',
  geoLng: '120.1551',
})

const style = reactive<QrStyle>({
  size: 320,
  margin: 2,
  level: 'M',
  fg: '#111827',
  bg: '#ffffff',
  dot: 'square',
  eye: 'rounded',
  logoRatio: 0.22,
})

const payload = computed(() => buildPayload(type.value, content))
const empty = computed(() => !payload.value.trim())
const previewSize = computed(() =>
  Math.min(320, Math.max(160, Number(style.size) || 320)),
)

/** 输入框被清空时回退到默认值，避免画布尺寸异常 */
function effectiveStyle(): QrStyle {
  return {
    ...style,
    size: Number(style.size) || 320,
    margin: Number(style.margin) || 0,
  }
}

const levelOptions = [
  { label: 'L 7%', value: 'L' },
  { label: 'M 15%', value: 'M' },
  { label: 'Q 25%', value: 'Q' },
  { label: 'H 30%', value: 'H' },
]
const dotOptions = [
  { label: '方块', value: 'square' },
  { label: '圆角', value: 'rounded' },
  { label: '圆点', value: 'dot' },
]
const eyeOptions = [
  { label: '直角', value: 'square' },
  { label: '圆角', value: 'rounded' },
]
const securityOptions = [
  { label: 'WPA / WPA2', value: 'WPA' },
  { label: 'WEP', value: 'WEP' },
  { label: '无密码', value: 'nopass' },
]

const canvasRef = ref<HTMLCanvasElement | null>(null)
const renderError = ref('')

function drawPlaceholder(canvas: HTMLCanvasElement) {
  const size = effectiveStyle().size
  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  ctx.fillStyle = '#f8fafc'
  ctx.fillRect(0, 0, size, size)
  ctx.strokeStyle = '#e2e8f0'
  ctx.lineWidth = 2
  ctx.setLineDash([8, 6])
  ctx.strokeRect(1, 1, size - 2, size - 2)
}

function render() {
  const canvas = canvasRef.value
  if (!canvas) return
  if (empty.value) {
    drawPlaceholder(canvas)
    renderError.value = ''
    return
  }
  try {
    renderQrCanvas(canvas, payload.value, effectiveStyle(), logoImage.value)
    renderError.value = ''
  } catch (error) {
    renderError.value = error instanceof Error ? error.message : String(error)
  }
}

watch(payload, render)
watch(style, render, { deep: true })
watch(type, () => nextTick(render))

onMounted(() => nextTick(render))

/* ---------------- Logo ---------------- */

const logoInput = ref<HTMLInputElement | null>(null)
const logoImage = ref<HTMLImageElement | null>(null)
const logoDataUrl = ref('')
const logoName = ref('')

function pickLogo() {
  logoInput.value?.click()
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(new Error('图片读取失败'))
    reader.readAsDataURL(file)
  })
}

async function onLogoChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  if (!file.type.startsWith('image/')) {
    message.warning('请选择图片文件')
    return
  }
  if (file.size > 2 * 1024 * 1024) {
    message.warning('Logo 请小于 2MB')
    return
  }
  try {
    const dataUrl = await readAsDataUrl(file)
    logoImage.value = await loadImage(dataUrl)
    logoDataUrl.value = dataUrl
    logoName.value = file.name
    if (style.level !== 'H') style.level = 'H'
    message.success('已添加 Logo，容错级别自动设为 H')
    render()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function removeLogo() {
  logoImage.value = null
  logoDataUrl.value = ''
  logoName.value = ''
  render()
}

/* ---------------- 导出 ---------------- */

const exporting = ref(false)

function timestamp(): string {
  const now = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  return (
    `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}` +
    `-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  )
}

async function saveFile(bytes: number[], extension: 'png' | 'svg') {
  const filterName = extension === 'png' ? 'PNG 图片' : 'SVG 矢量图'
  const path = await save({
    title: '保存二维码',
    defaultPath: `qrcode-${timestamp()}.${extension}`,
    filters: [{ name: filterName, extensions: [extension] }],
  })
  if (!path) return
  const suffix = `.${extension}`
  const target = path.toLowerCase().endsWith(suffix) ? path : `${path}${suffix}`
  await api.reportWriteFile(target, bytes)
  message.success('二维码已保存')
}

async function downloadPng() {
  if (empty.value) {
    message.warning('请先填写二维码内容')
    return
  }
  const canvas = canvasRef.value
  if (!canvas) return
  exporting.value = true
  try {
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
    if (!blob) throw new Error('生成 PNG 失败')
    await saveFile(Array.from(new Uint8Array(await blob.arrayBuffer())), 'png')
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    exporting.value = false
  }
}

async function downloadSvg() {
  if (empty.value) {
    message.warning('请先填写二维码内容')
    return
  }
  exporting.value = true
  try {
    const svg = buildQrSvg(payload.value, effectiveStyle(), logoDataUrl.value || undefined)
    await saveFile(Array.from(new TextEncoder().encode(svg)), 'svg')
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    exporting.value = false
  }
}

/* ---------------- 识别 ---------------- */

const decoding = ref(false)
const decoded = ref<{ data: string; version: number } | null>(null)
const decodedName = ref('')
const decodeInput = ref<HTMLInputElement | null>(null)
const dragging = ref(false)

const summary = computed(() =>
  decoded.value ? describeQrPayload(decoded.value.data) : null,
)

function openDecodePicker() {
  decodeInput.value?.click()
}

async function handleFile(file: File) {
  if (!file.type.startsWith('image/')) {
    message.warning('请选择图片文件')
    return
  }
  decoding.value = true
  try {
    decoded.value = await decodeQrImage(file)
    decodedName.value = file.name
  } catch (error) {
    decoded.value = null
    decodedName.value = ''
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    decoding.value = false
  }
}

function onDecodeChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (file) void handleFile(file)
}

async function onDrop(event: DragEvent) {
  dragging.value = false
  const file = event.dataTransfer?.files?.[0]
  if (file) await handleFile(file)
}

async function copyDecoded() {
  if (!decoded.value) return
  try {
    await navigator.clipboard.writeText(decoded.value.data)
    message.success('已复制识别结果')
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}

function clearDecoded() {
  decoded.value = null
  decodedName.value = ''
}
</script>

<template>
  <div v-if="props.mode === 'generate'" class="qr-tool">
    <div class="qr-grid">
      <section class="qr-panel">
        <div class="qr-types">
          <button
            v-for="item in QR_TYPES"
            :key="item.value"
            type="button"
            class="qr-type"
            :class="{ active: type === item.value }"
            :title="item.hint"
            @click="type = item.value"
          >
            {{ item.label }}
          </button>
        </div>

        <div class="qr-fields">
          <template v-if="type === 'text'">
            <label class="qr-field qr-field-full">
              <span>文本或网址</span>
              <a-textarea
                v-model:value="content.text"
                :rows="5"
                placeholder="支持任意文本，填网址扫码可直接打开"
              />
            </label>
          </template>

          <template v-else-if="type === 'wifi'">
            <label class="qr-field qr-field-full">
              <span>网络名称 SSID</span>
              <a-input v-model:value="content.wifiSsid" placeholder="WiFi 名称" />
            </label>
            <label class="qr-field qr-field-full">
              <span>密码</span>
              <a-input
                v-model:value="content.wifiPassword"
                :disabled="content.wifiSecurity === 'nopass'"
                placeholder="WiFi 密码"
              />
            </label>
            <label class="qr-field">
              <span>加密方式</span>
              <a-select
                v-model:value="content.wifiSecurity"
                size="small"
                :options="securityOptions"
              />
            </label>
            <label class="qr-field qr-switch">
              <span>隐藏网络</span>
              <a-switch v-model:checked="content.wifiHidden" size="small" />
            </label>
          </template>

          <template v-else-if="type === 'vcard'">
            <label class="qr-field">
              <span>姓名</span>
              <a-input v-model:value="content.vName" />
            </label>
            <label class="qr-field">
              <span>电话</span>
              <a-input v-model:value="content.vPhone" />
            </label>
            <label class="qr-field">
              <span>邮箱</span>
              <a-input v-model:value="content.vEmail" />
            </label>
            <label class="qr-field">
              <span>公司</span>
              <a-input v-model:value="content.vOrg" />
            </label>
            <label class="qr-field">
              <span>职位</span>
              <a-input v-model:value="content.vTitle" />
            </label>
            <label class="qr-field">
              <span>网址</span>
              <a-input v-model:value="content.vUrl" />
            </label>
            <label class="qr-field qr-field-full">
              <span>地址</span>
              <a-input v-model:value="content.vAddress" />
            </label>
          </template>

          <template v-else-if="type === 'tel'">
            <label class="qr-field qr-field-full">
              <span>电话号码</span>
              <a-input v-model:value="content.tel" placeholder="例如 13800138000" />
            </label>
          </template>

          <template v-else-if="type === 'sms'">
            <label class="qr-field qr-field-full">
              <span>接收号码</span>
              <a-input v-model:value="content.smsPhone" placeholder="例如 13800138000" />
            </label>
            <label class="qr-field qr-field-full">
              <span>短信内容</span>
              <a-textarea v-model:value="content.smsMessage" :rows="3" />
            </label>
          </template>

          <template v-else-if="type === 'email'">
            <label class="qr-field qr-field-full">
              <span>收件人</span>
              <a-input v-model:value="content.emailTo" placeholder="name@example.com" />
            </label>
            <label class="qr-field qr-field-full">
              <span>主题</span>
              <a-input v-model:value="content.emailSubject" />
            </label>
            <label class="qr-field qr-field-full">
              <span>正文</span>
              <a-textarea v-model:value="content.emailBody" :rows="3" />
            </label>
          </template>

          <template v-else>
            <label class="qr-field">
              <span>纬度</span>
              <a-input v-model:value="content.geoLat" placeholder="30.2741" />
            </label>
            <label class="qr-field">
              <span>经度</span>
              <a-input v-model:value="content.geoLng" placeholder="120.1551" />
            </label>
          </template>
        </div>

        <div class="qr-payload">
          <span class="qr-payload-label">编码内容</span>
          <code class="qr-payload-value">{{ payload || '填写内容后自动生成' }}</code>
        </div>
      </section>

      <section class="qr-panel qr-preview">
        <div class="qr-canvas-wrap">
          <canvas
            ref="canvasRef"
            class="qr-canvas"
            :style="{ width: `${previewSize}px`, height: `${previewSize}px` }"
          />
          <div v-if="empty" class="qr-empty">等待填写内容</div>
        </div>
        <p v-if="renderError" class="qr-error">{{ renderError }}</p>

        <div class="qr-controls">
          <label class="qr-control">
            <span>尺寸</span>
            <a-input-number
              v-model:value="style.size"
              size="small"
              :min="128"
              :max="2048"
              :step="32"
              :style="{ width: '82px' }"
            />
          </label>
          <label class="qr-control">
            <span>留白</span>
            <a-input-number
              v-model:value="style.margin"
              size="small"
              :min="0"
              :max="8"
              :style="{ width: '70px' }"
            />
          </label>
          <label class="qr-control">
            <span>容错</span>
            <a-select
              v-model:value="style.level"
              size="small"
              :options="levelOptions"
              :style="{ width: '86px' }"
            />
          </label>
          <label class="qr-control">
            <span>码点</span>
            <a-select
              v-model:value="style.dot"
              size="small"
              :options="dotOptions"
              :style="{ width: '86px' }"
            />
          </label>
          <label class="qr-control">
            <span>定位点</span>
            <a-select
              v-model:value="style.eye"
              size="small"
              :options="eyeOptions"
              :style="{ width: '86px' }"
            />
          </label>
          <label class="qr-control qr-color">
            <span>前景</span>
            <input v-model="style.fg" type="color" class="qr-color-input" />
            <b>{{ style.fg }}</b>
          </label>
          <label class="qr-control qr-color">
            <span>背景</span>
            <input v-model="style.bg" type="color" class="qr-color-input" />
            <b>{{ style.bg }}</b>
          </label>
        </div>

        <div class="qr-logo-row">
          <a-button size="small" @click="pickLogo">上传 Logo</a-button>
          <a-button v-if="logoImage" size="small" danger @click="removeLogo">移除 Logo</a-button>
          <span class="qr-logo-name">{{ logoName || '可选，添加后自动使用 H 级容错' }}</span>
          <input ref="logoInput" type="file" accept="image/*" class="qr-hidden" @change="onLogoChange" />
        </div>

        <div class="qr-actions">
          <a-button type="primary" :loading="exporting" @click="downloadPng">下载 PNG</a-button>
          <a-button :loading="exporting" @click="downloadSvg">下载 SVG</a-button>
        </div>
      </section>
    </div>
  </div>

  <div v-else class="qr-tool">
    <div
      class="qr-dropzone"
      :class="{ dragging }"
      @click="openDecodePicker"
      @dragover.prevent="dragging = true"
      @dragleave.prevent="dragging = false"
      @drop.prevent="onDrop"
    >
      <div class="qr-drop-title">{{ decoding ? '正在识别…' : '点击选择图片，或将图片拖到这里' }}</div>
      <div class="qr-drop-tip">支持 PNG / JPG / WEBP，识别全部在本机完成</div>
    </div>
    <input ref="decodeInput" type="file" accept="image/*" class="qr-hidden" @change="onDecodeChange" />

    <section v-if="decoded" class="qr-panel qr-result">
      <header class="qr-result-head">
        <span class="qr-badge">{{ summary?.label }}</span>
        <span class="qr-result-name">{{ decodedName }} · QR v{{ decoded.version }}</span>
        <a-button size="small" @click="copyDecoded">复制内容</a-button>
        <a-button size="small" @click="clearDecoded">清空</a-button>
      </header>
      <dl v-if="summary?.fields.length" class="qr-fields-list">
        <template v-for="field in summary.fields" :key="`${field.key}-${field.value}`">
          <dt>{{ field.key }}</dt>
          <dd>{{ field.value }}</dd>
        </template>
      </dl>
      <pre class="qr-raw">{{ decoded.data }}</pre>
    </section>
    <div v-else class="qr-empty-result">还没有识别结果</div>
  </div>
</template>

<style scoped>
.qr-tool {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.qr-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.05fr) minmax(0, 0.95fr);
  gap: 12px;
  align-items: start;
}

.qr-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.qr-types {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(88px, 1fr));
  gap: 6px;
}

.qr-type {
  height: 30px;
  padding: 0 8px;
  font-size: 12px;
  color: var(--fm-text-2);
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  cursor: pointer;
  transition: border-color 0.15s ease, color 0.15s ease, background 0.15s ease;
}

.qr-type:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
}

.qr-type.active {
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-color: var(--fm-primary);
}

.qr-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.qr-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--fm-muted);
}

.qr-field-full {
  grid-column: 1 / -1;
}

.qr-switch {
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  padding-top: 18px;
}

.qr-payload {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  background: var(--fm-bg-subtle, #f8fafc);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.qr-payload-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.qr-payload-value {
  max-height: 96px;
  overflow: auto;
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Consolas, monospace);
  font-size: 11px;
  line-height: 1.6;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  word-break: break-all;
}

.qr-preview {
  align-items: stretch;
}

.qr-canvas-wrap {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  background: var(--fm-bg-subtle, #f8fafc);
  border-radius: var(--fm-radius-sm);
}

.qr-canvas {
  display: block;
  max-width: 100%;
  border-radius: 6px;
}

.qr-empty {
  position: absolute;
  bottom: 10px;
  font-size: 12px;
  color: var(--fm-muted);
}

.qr-error {
  margin: 0;
  font-size: 12px;
  color: var(--fm-danger);
}

.qr-controls {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
  gap: 8px 10px;
}

.qr-control {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--fm-muted);
}

.qr-control > span {
  flex-shrink: 0;
}

.qr-color {
  gap: 4px;
}

.qr-color-input {
  width: 26px;
  height: 24px;
  padding: 0;
  background: transparent;
  border: 1px solid var(--fm-border);
  border-radius: 4px;
  cursor: pointer;
}

.qr-color b {
  font-size: 11px;
  font-weight: 400;
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Consolas, monospace);
  text-transform: uppercase;
}

.qr-logo-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.qr-logo-name {
  flex: 1;
  min-width: 120px;
  font-size: 12px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.qr-actions {
  display: flex;
  gap: 8px;
}

.qr-hidden {
  display: none;
}

.qr-dropzone {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 36px 16px;
  text-align: center;
  background: var(--fm-surface);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}

.qr-dropzone:hover,
.qr-dropzone.dragging {
  border-color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.qr-drop-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.qr-drop-tip {
  font-size: 12px;
  color: var(--fm-muted);
}

.qr-result {
  gap: 10px;
}

.qr-result-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.qr-badge {
  padding: 1px 8px;
  font-size: 11px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border-radius: 999px;
}

.qr-result-name {
  flex: 1;
  min-width: 120px;
  font-size: 12px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.qr-fields-list {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 6px 14px;
  margin: 0;
  font-size: 12px;
}

.qr-fields-list dt {
  color: var(--fm-muted);
}

.qr-fields-list dd {
  margin: 0;
  color: var(--fm-text-2);
  word-break: break-all;
}

.qr-raw {
  margin: 0;
  padding: 10px 12px;
  font-family: var(--fm-mono, ui-monospace, SFMono-Regular, Consolas, monospace);
  font-size: 12px;
  line-height: 1.6;
  color: var(--fm-text-2);
  background: var(--fm-bg-subtle, #f8fafc);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 320px;
  overflow: auto;
}

.qr-empty-result {
  padding: 24px;
  font-size: 12px;
  text-align: center;
  color: var(--fm-muted);
  background: var(--fm-surface);
  border: 1px dashed var(--fm-border);
  border-radius: var(--fm-radius);
}

@media (max-width: 900px) {
  .qr-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
