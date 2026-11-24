<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import {
  CopyOutlined,
  DeleteOutlined,
  PlayCircleOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import SplitLayoutSwitch from '@/components/split/SplitLayoutSwitch.vue'
import SplitPane from '@/components/split/SplitPane.vue'
import { useSplitLayout } from '@/composables/useSplitLayout'
import type { OptionState, ToolDef } from '@/tools/types'
import { copyToClipboard } from '@/utils/clipboard'

const props = defineProps<{ tool: ToolDef }>()

const { direction, isVertical, setDirection, ratioKey } = useSplitLayout('astroman:tools:workbench')

const input = ref('')
const output = ref('')
const error = ref('')
const running = ref(false)

const simple = computed(() => props.tool.simple!)

const options = reactive<OptionState>({})

function resetOptions() {
  Object.keys(options).forEach((key) => delete options[key])
  ;(props.tool.simple?.options ?? []).forEach((option) => {
    options[option.key] = option.default
  })
}

watch(
  () => props.tool.id,
  () => {
    input.value = ''
    output.value = ''
    error.value = ''
    resetOptions()
  },
  { immediate: true },
)

const hasInput = computed(() => Boolean(props.tool.simple?.inputLabel))

const inputStats = computed(() => {
  if (!input.value) return ''
  const lines = input.value.split('\n').length
  return `${lines} 行 · ${input.value.length} 字符`
})

const outputStats = computed(() => {
  if (!output.value) return ''
  const lines = output.value.split('\n').length
  return `${lines} 行 · ${output.value.length} 字符`
})

async function run() {
  error.value = ''
  running.value = true
  try {
    const result = await simple.value.run(input.value, options)
    output.value = result
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    output.value = ''
  } finally {
    running.value = false
  }
}

function loadSample() {
  if (!simple.value.sample) return
  input.value = simple.value.sample
  error.value = ''
  if (!hasInput.value) void run()
}

function clearAll() {
  input.value = ''
  output.value = ''
  error.value = ''
}

async function copyOutput() {
  if (!output.value) return
  try {
    await copyToClipboard(output.value)
    message.success('已复制到剪贴板')
  } catch {
    message.error('复制失败，请手动选择文本')
  }
}

if (!hasInput.value) void run()
</script>

<template>
  <div class="workbench">
    <div class="wb-toolbar">
      <div class="wb-options">
        <label v-for="option in simple.options ?? []" :key="option.key" class="wb-field">
          <span class="wb-label" :title="option.hint">{{ option.label }}</span>
          <a-select
            v-if="option.type === 'select'"
            v-model:value="options[option.key]"
            size="small"
            :style="{ width: `${option.width ?? 130}px` }"
          >
            <a-select-option v-for="item in option.options" :key="item.value" :value="item.value">
              {{ item.label }}
            </a-select-option>
          </a-select>
          <a-switch
            v-else-if="option.type === 'switch'"
            v-model:checked="options[option.key]"
            size="small"
          />
          <a-input-number
            v-else-if="option.type === 'number'"
            v-model:value="options[option.key]"
            size="small"
            :min="1"
            :max="4096"
            :style="{ width: `${option.width ?? 90}px` }"
          />
          <a-input
            v-else
            v-model:value="options[option.key]"
            size="small"
            :placeholder="option.placeholder"
            :style="{ width: `${option.width ?? 160}px` }"
          />
        </label>
      </div>
      <div class="wb-actions">
        <template v-if="hasInput">
          <SplitLayoutSwitch :model-value="direction" @update:model-value="setDirection" />
          <span class="wb-sep" aria-hidden="true" />
        </template>
        <a-button v-if="simple.sample" size="small" @click="loadSample">
          <ThunderboltOutlined /> 示例
        </a-button>
        <a-button size="small" @click="clearAll">
          <DeleteOutlined /> 清空
        </a-button>
        <a-button type="primary" size="small" :loading="running" @click="run">
          <PlayCircleOutlined /> {{ simple.actionLabel ?? '执行' }}
        </a-button>
      </div>
    </div>

    <SplitPane
      v-if="hasInput"
      class="wb-body"
      :class="{ 'is-vertical': isVertical }"
      :direction="direction"
      :storage-key="ratioKey"
      :min-first="isVertical ? 150 : 260"
      :min-second="isVertical ? 150 : 260"
    >
      <template #first>
        <section class="wb-panel">
          <header class="wb-head">
            <span>{{ simple.inputLabel }}</span>
            <span class="wb-stat">{{ inputStats }}</span>
          </header>
          <CodeArea
            v-model="input"
            fill
            :placeholder="simple.inputPlaceholder ?? '在此粘贴或输入内容'"
            @update:model-value="error = ''"
          />
        </section>
      </template>

      <template #second>
        <section class="wb-panel">
          <header class="wb-head">
            <span>{{ simple.outputLabel ?? '输出结果' }}</span>
            <span class="wb-stat">{{ outputStats }}</span>
            <a-button type="link" size="small" :disabled="!output" @click="copyOutput">
              <CopyOutlined /> 复制
            </a-button>
          </header>
          <a-alert v-if="error" type="error" :message="error" show-icon class="wb-error" />
          <CodeArea
            :model-value="output"
            fill
            readonly
            :placeholder="simple.outputPlaceholder ?? '执行后在此显示结果'"
          />
        </section>
      </template>
    </SplitPane>

    <section v-else class="wb-body wb-body-single">
      <header class="wb-head">
        <span>{{ simple.outputLabel ?? '输出结果' }}</span>
        <span class="wb-stat">{{ outputStats }}</span>
        <a-button type="link" size="small" :disabled="!output" @click="copyOutput">
          <CopyOutlined /> 复制
        </a-button>
      </header>
      <a-alert v-if="error" type="error" :message="error" show-icon class="wb-error" />
      <CodeArea
        :model-value="output"
        fill
        readonly
        :placeholder="simple.outputPlaceholder ?? '执行后在此显示结果'"
      />
    </section>
  </div>
</template>

<style scoped>
.workbench {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.wb-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  padding: 10px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.wb-options {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
}

.wb-field {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.wb-label {
  font-size: 12px;
  color: var(--fm-muted);
  white-space: nowrap;
}

.wb-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.wb-sep {
  width: 1px;
  height: 16px;
  background: var(--fm-border);
  flex: none;
}

.wb-body {
  /* 固定可用高度，内部两栏按比例分，CodeArea 用 fill 撑满各自一侧 */
  height: clamp(560px, 76vh, 1080px);
  min-height: 0;
}

/* 上下分栏时两栏各分一半高度，容器不拉高就只剩几行 */
.wb-body.is-vertical {
  height: clamp(620px, 82vh, 1200px);
}

.wb-body-single {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.wb-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.wb-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
  min-height: 24px;
}

.wb-stat {
  margin-left: auto;
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.wb-error {
  margin-bottom: 0;
}

@media (max-width: 1100px) {
  .wb-body {
    height: clamp(520px, 74vh, 980px);
  }

  .wb-body.is-vertical {
    height: clamp(560px, 84vh, 1080px);
  }
}
</style>
