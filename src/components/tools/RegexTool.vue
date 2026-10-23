<script setup lang="ts">
import { computed, ref } from 'vue'
import CodeArea from '@/components/CodeArea.vue'
import { runRegex, type RegexResult } from '@/tools/crypto'
import { htmlEscape } from '@/tools/transforms'

const pattern = ref('(\\w+)@(\\w+)\\.com')
const flags = ref('g')
const text = ref('联系 fink@example.com，备用 tom@demo.com')
const error = ref('')

const result = computed<RegexResult | null>(() => {
  error.value = ''
  if (!text.value) return null
  try {
    return runRegex(pattern.value, flags.value, text.value)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    return null
  }
})

const highlighted = computed(() => {
  if (!pattern.value) return htmlEscape(text.value)
  try {
    const regex = new RegExp(pattern.value, Array.from(new Set(flags.value.replace(/[^gimsuy]/g, '') + 'g')).join(''))
    return htmlEscape(text.value).replace(
      regex,
      (value) => `<mark class="rx-mark">${htmlEscape(value)}</mark>`,
    )
  } catch {
    return htmlEscape(text.value)
  }
})

const FLAGS = [
  { value: 'g', label: 'g 全局' },
  { value: 'i', label: 'i 忽略大小写' },
  { value: 'm', label: 'm 多行' },
  { value: 's', label: 's 点匹配换行' },
  { value: 'u', label: 'u Unicode' },
]

function toggleFlag(flag: string) {
  if (flags.value.includes(flag)) {
    flags.value = flags.value.replace(flag, '')
  } else {
    flags.value += flag
  }
}

const PRESETS = [
  { label: '邮箱', value: '[\\w.%+-]+@[\\w.-]+\\.[A-Za-z]{2,}' },
  { label: '手机号', value: '1[3-9]\\d{9}' },
  { label: 'IPv4', value: '((25[0-5]|2[0-4]\\d|1\\d{2}|[1-9]?\\d)\\.){3}(25[0-5]|2[0-4]\\d|1\\d{2}|[1-9]?\\d)' },
  { label: 'URL', value: 'https?://[\\w.-]+(?::\\d+)?(?:/[\\w./?%&=#-]*)?' },
  { label: '日期', value: '\\d{4}-\\d{2}-\\d{2}' },
  { label: '中文字符', value: '[\\u4e00-\\u9fa5]+' },
]
</script>

<template>
  <div class="rx">
    <div class="rx-bar">
      <span class="rx-slash">/</span>
      <a-input v-model:value="pattern" size="small" placeholder="正则表达式" class="mono rx-pattern" />
      <span class="rx-slash">/</span>
      <div class="rx-flags">
        <a-tag
          v-for="flag in FLAGS"
          :key="flag.value"
          class="rx-flag"
          :class="{ active: flags.includes(flag.value) }"
          @click="toggleFlag(flag.value)"
        >
          {{ flag.label }}
        </a-tag>
      </div>
    </div>

    <div class="rx-presets">
      <span class="rx-presets-label">常用</span>
      <a-tag v-for="preset in PRESETS" :key="preset.label" class="rx-preset" @click="pattern = preset.value">
        {{ preset.label }}
      </a-tag>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="rx-body">
      <section class="rx-pane">
        <header class="rx-head">测试文本</header>
        <CodeArea v-model="text" :rows="12" placeholder="粘贴需要匹配的文本" />
        <header class="rx-head">高亮预览</header>
        <div class="rx-preview mono scroll-y" v-html="highlighted" />
      </section>

      <section class="rx-pane">
        <header class="rx-head">
          <span>匹配结果</span>
          <a-tag v-if="result" color="blue">{{ result.matches.length }} 处</a-tag>
        </header>
        <div class="rx-result scroll-y">
          <template v-if="result && result.matches.length">
            <div v-for="(item, index) in result.matches" :key="index" class="rx-item">
              <div class="rx-item-head">
                <span class="rx-item-index">#{{ index + 1 }}</span>
                <span class="rx-item-pos">位置 {{ item.index }}</span>
              </div>
              <div class="rx-item-text mono">{{ item.text }}</div>
              <div v-if="item.groups.length" class="rx-item-groups">
                <span v-for="(group, groupIndex) in item.groups" :key="groupIndex" class="rx-group mono">
                  ${{ groupIndex + 1 }}={{ group }}
                </span>
              </div>
            </div>
          </template>
          <div v-else class="empty-hint">{{ result ? '没有匹配到内容' : '输入正则与文本后实时展示匹配结果' }}</div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.rx {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.rx-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  padding: 8px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.rx-slash {
  color: var(--fm-muted);
  font-family: 'JetBrains Mono', Consolas, monospace;
}

.rx-pattern {
  flex: 1;
  min-width: 220px;
}

.rx-flags {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.rx-flag {
  cursor: pointer;
  user-select: none;
  margin: 0;
}

.rx-flag.active {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
  color: #fff;
}

.rx-presets {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.rx-presets-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.rx-preset {
  cursor: pointer;
  margin: 0;
}

.rx-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.rx-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.rx-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.rx-preview {
  height: 150px;
  overflow: auto;
  padding: 10px 12px;
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  font-size: 12px;
  line-height: 20px;
  white-space: pre-wrap;
  word-break: break-all;
}

.rx-preview :deep(.rx-mark) {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
  border-radius: 2px;
  padding: 0 2px;
}

.rx-result {
  max-height: 400px;
  overflow: auto;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface);
}

.rx-item {
  padding: 8px 12px;
  border-bottom: 1px solid var(--fm-border);
}

.rx-item:last-child {
  border-bottom: none;
}

.rx-item-head {
  display: flex;
  gap: 10px;
  font-size: 11px;
  color: var(--fm-muted);
}

.rx-item-text {
  font-size: 13px;
  color: var(--fm-text);
  word-break: break-all;
  margin: 4px 0;
}

.rx-item-groups {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.rx-group {
  font-size: 11px;
  color: var(--fm-json-string);
  background: var(--fm-success-soft);
  padding: 1px 6px;
  border-radius: 4px;
}

@media (max-width: 1100px) {
  .rx-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
