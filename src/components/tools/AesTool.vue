<script setup lang="ts">
import { ref } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, LockOutlined, UnlockOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import { aesDecrypt, aesEncrypt } from '@/tools/crypto'

const key = ref('')
const mode = ref<'encrypt' | 'decrypt'>('encrypt')
const input = ref('')
const output = ref('')
const error = ref('')
const running = ref(false)

async function run() {
  error.value = ''
  output.value = ''
  running.value = true
  try {
    if (!key.value) throw new Error('请填写 AES 密钥')
    if (!input.value) throw new Error(mode.value === 'encrypt' ? '请输入要加密的明文' : '请输入要解密的密文')
    output.value =
      mode.value === 'encrypt'
        ? await aesEncrypt(input.value, key.value)
        : await aesDecrypt(input.value, key.value)
    message.success(mode.value === 'encrypt' ? '加密完成' : '解密完成')
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    running.value = false
  }
}

async function copyOutput() {
  if (!output.value) return
  try {
    await navigator.clipboard.writeText(output.value)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}

function swap() {
  input.value = output.value
  output.value = ''
  mode.value = mode.value === 'encrypt' ? 'decrypt' : 'encrypt'
}
</script>

<template>
  <div class="aes">
    <div class="aes-bar">
      <span class="aes-label">密钥</span>
      <a-input-password v-model:value="key" size="small" placeholder="任意长度，内部按 SHA-256 派生 256 位密钥" style="width: 320px" />
      <a-segmented
        v-model:value="mode"
        size="small"
        :options="[
          { label: '加密', value: 'encrypt' },
          { label: '解密', value: 'decrypt' },
        ]"
      />
      <a-button type="primary" size="small" :loading="running" @click="run">
        <LockOutlined v-if="mode === 'encrypt'" />
        <UnlockOutlined v-else />
        {{ mode === 'encrypt' ? '加密' : '解密' }}
      </a-button>
      <a-button size="small" :disabled="!output" @click="swap">结果转输入</a-button>
      <a-button size="small" :disabled="!output" @click="copyOutput"><CopyOutlined /> 复制</a-button>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="aes-body">
      <section class="aes-pane">
        <header class="aes-head">{{ mode === 'encrypt' ? '明文' : '密文（IV:密文）' }}</header>
        <CodeArea v-model="input" :rows="16" :placeholder="mode === 'encrypt' ? '输入需要加密的内容' : '粘贴 IV:密文（Base64）'" />
      </section>
      <section class="aes-pane">
        <header class="aes-head">输出</header>
        <CodeArea :model-value="output" :rows="16" readonly placeholder="结果会显示在这里" />
      </section>
    </div>

    <p class="aes-tip">
      AES-256-CBC：密钥经 SHA-256 派生，每次加密随机生成 16 字节 IV，密文格式为
      <code>Base64(IV):Base64(密文)</code>。
    </p>
  </div>
</template>

<style scoped>
.aes {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.aes-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.aes-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.aes-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.aes-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.aes-head {
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.aes-tip {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

.aes-tip code {
  background: var(--fm-surface-alt);
  padding: 1px 5px;
  border-radius: 4px;
}

@media (max-width: 1100px) {
  .aes-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
