<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import {
  CopyOutlined,
  RobotOutlined,
  SendOutlined,
  SettingOutlined,
} from '@ant-design/icons-vue'
import { useAiAssistStore } from '@/stores/aiAssist'
import { renderMarkdown } from '@/utils/markdown'

const store = useAiAssistStore()
const router = useRouter()

const input = ref('')
const bodyRef = ref<HTMLElement | null>(null)
const expanded = ref(false)

const providerOptions = computed(() =>
  store.providers
    .filter((item) => item.id)
    .map((item) => ({ label: `${item.name} · ${item.model}`, value: item.id as number })),
)

function scrollBottom() {
  nextTick(() => {
    const el = bodyRef.value
    if (el) el.scrollTop = el.scrollHeight
  })
}

watch(
  () => store.messages.map((item) => item.content).join('\n'),
  () => scrollBottom(),
)

watch(
  () => store.open,
  (value) => {
    if (value) scrollBottom()
  },
)

async function submit() {
  const text = input.value
  if (!text.trim() || store.streaming) return
  input.value = ''
  await store.ask(text)
}

function onEnter(event: KeyboardEvent) {
  if (event.shiftKey || event.isComposing) return
  event.preventDefault()
  void submit()
}

async function copyLast() {
  const last = [...store.messages].reverse().find((item) => item.role === 'assistant' && item.content)
  if (!last) return
  try {
    await navigator.clipboard.writeText(last.content)
    message.success('已复制')
  } catch {
    message.warning('复制失败，请手动选择文本')
  }
}

function goChat() {
  store.close()
  router.push('/ai')
}
</script>

<template>
  <a-drawer
    :open="store.open"
    :width="640"
    placement="right"
    :body-style="{ padding: '0', display: 'flex', flexDirection: 'column' }"
    wrap-class-name="ai-assist-wrap"
    @close="store.close"
  >
    <template #title>
      <div class="assist-title">
        <span class="assist-bot"><RobotOutlined /></span>
        <div class="assist-title-text">
          <span class="assist-name">{{ store.title }}</span>
          <span class="assist-sub">小HI · 基于当前页面数据分析</span>
        </div>
        <a-select
          v-if="providerOptions.length"
          v-model:value="store.providerId"
          class="assist-provider"
          size="small"
          :options="providerOptions"
          :dropdown-match-select-width="260"
        />
        <a-button
          v-else
          size="small"
          @click="goChat"
        >
          <template #icon><SettingOutlined /></template>
          去配置模型
        </a-button>
      </div>
    </template>

    <div ref="bodyRef" class="assist-body">
      <a-alert
        v-if="store.error"
        class="assist-error"
        type="error"
        show-icon
        :message="store.error"
      />

      <div v-if="store.hint" class="assist-hint">
        <span class="assist-hint-label">分析范围</span>
        <a
          class="assist-hint-toggle"
          @click="expanded = !expanded"
        >
          {{ expanded ? '收起' : '展开' }}
        </a>
        <pre v-if="expanded" class="assist-hint-body">{{ store.hint }}</pre>
      </div>

      <div v-if="!store.messages.length && !store.streaming" class="assist-empty">
        <RobotOutlined class="assist-empty-icon" />
        <p>正在准备分析上下文…</p>
      </div>

      <div
        v-for="(item, index) in store.messages"
        :key="index"
        class="assist-row"
        :class="item.role"
      >
        <div v-if="item.role === 'assistant'" class="assist-avatar">HI</div>
        <div class="assist-bubble" :class="{ failed: item.failed }">
          <div v-if="item.role === 'assistant'" class="md" v-html="renderMarkdown(item.content)" />
          <span v-else>{{ item.content }}</span>
          <span v-if="item.streaming" class="assist-caret">▍</span>
        </div>
      </div>

      <div v-if="store.streaming" class="assist-typing">小HI 正在分析…</div>
    </div>

    <div class="assist-footer">
      <a-textarea
        v-model:value="input"
        :auto-size="{ minRows: 1, maxRows: 4 }"
        placeholder="继续追问，例如：把断言脚本改成校验 code=0（Enter 发送，Shift + Enter 换行）"
        :disabled="store.streaming"
        @keydown.enter="onEnter"
      />
      <div class="assist-actions">
        <a-button size="small" type="text" :disabled="!store.messages.length" @click="copyLast">
          <template #icon><CopyOutlined /></template>
          复制回答
        </a-button>
        <a-button size="small" type="text" @click="goChat">在小HI 中继续</a-button>
        <span class="flex-spacer" />
        <a-button
          type="primary"
          size="small"
          :loading="store.streaming"
          :disabled="!input.trim() || store.streaming || !store.ready"
          @click="submit"
        >
          <template #icon><SendOutlined /></template>
          追问
        </a-button>
      </div>
    </div>
  </a-drawer>
</template>

<style scoped>
.assist-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.assist-bot {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 9px;
  background: var(--fm-accent-soft, #e8f0fe);
  color: var(--fm-accent, #2b6cf6);
  flex: none;
}

.assist-title-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}

.assist-name {
  font-weight: 600;
  font-size: 14px;
  line-height: 1.3;
}

.assist-sub {
  font-size: 11px;
  color: var(--fm-text-muted, #8a94a6);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.assist-provider {
  width: 190px;
  flex: none;
}

.assist-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 18px 8px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.assist-error {
  margin-bottom: 4px;
}

.assist-hint {
  border: 1px dashed var(--fm-border, #dfe5ef);
  border-radius: 8px;
  padding: 8px 10px;
  font-size: 12px;
  color: var(--fm-text-muted, #8a94a6);
}

.assist-hint-label {
  margin-right: 8px;
}

.assist-hint-toggle {
  font-size: 12px;
}

.assist-hint-body {
  margin: 8px 0 0;
  max-height: 220px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 11px;
  line-height: 1.55;
  color: var(--fm-text, #2b3445);
}

.assist-empty {
  margin: 40px auto;
  text-align: center;
  color: var(--fm-text-muted, #8a94a6);
  font-size: 13px;
}

.assist-empty-icon {
  font-size: 28px;
  opacity: 0.5;
}

.assist-row {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}

.assist-row.user {
  justify-content: flex-end;
}

.assist-avatar {
  flex: none;
  width: 26px;
  height: 26px;
  border-radius: 8px;
  display: grid;
  place-items: center;
  font-size: 11px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(135deg, #4d7cff, #2b55d6);
}

.assist-bubble {
  max-width: calc(100% - 48px);
  padding: 10px 12px;
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.65;
  word-break: break-word;
}

.assist-row.assistant .assist-bubble {
  background: var(--fm-surface-2, #f5f7fb);
  color: var(--fm-text, #2b3445);
}

.assist-row.user .assist-bubble {
  background: var(--fm-accent, #2b6cf6);
  color: #fff;
  max-width: 84%;
  white-space: pre-wrap;
}

.assist-bubble.failed {
  border: 1px solid #ffccc7;
}

.assist-caret {
  display: inline-block;
  margin-left: 2px;
  animation: assist-blink 1s steps(2, start) infinite;
}

.assist-typing {
  font-size: 12px;
  color: var(--fm-text-muted, #8a94a6);
  padding-left: 34px;
}

.assist-footer {
  border-top: 1px solid var(--fm-border, #e6ebf3);
  padding: 10px 14px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.assist-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.flex-spacer {
  flex: 1;
}

.assist-bubble :deep(.md) {
  font-size: 13px;
}

.assist-bubble :deep(.md p) {
  margin: 0 0 8px;
}

.assist-bubble :deep(.md p:last-child) {
  margin-bottom: 0;
}

.assist-bubble :deep(.md pre) {
  margin: 8px 0;
  padding: 10px;
  border-radius: 8px;
  overflow-x: auto;
  background: rgba(15, 23, 42, 0.06);
  font-size: 12px;
}

@keyframes assist-blink {
  to {
    opacity: 0;
  }
}
</style>
