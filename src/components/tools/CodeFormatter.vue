<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, DeleteOutlined, PlayCircleOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import { formatCodeByLanguage, type CodeLanguage, type FormatMode } from '@/tools/formatters'

const language = ref<CodeLanguage>('js')
const mode = ref<FormatMode>('pretty')
const indent = ref(2)
const input = ref('')
const output = ref('')
const error = ref('')

const LANGUAGE_OPTIONS: Array<{ label: string; value: CodeLanguage }> = [
  { label: 'HTML', value: 'html' },
  { label: 'XML', value: 'xml' },
  { label: 'CSS', value: 'css' },
  { label: 'JavaScript', value: 'js' },
  { label: 'Java', value: 'java' },
  { label: 'C#', value: 'csharp' },
  { label: 'Go', value: 'go' },
  { label: 'PHP', value: 'php' },
  { label: 'C / C++', value: 'c' },
  { label: 'SQL', value: 'sql' },
]

const SAMPLES: Record<CodeLanguage, string> = {
  html: '<div class="card"><h2>标题</h2><p>正文内容</p></div>',
  xml: '<root><item id="1"><name>A</name></item><item id="2"><name>B</name></item></root>',
  css: '.card{display:flex;gap:8px;padding:12px}.card .title{font-size:14px;color:#333}',
  js: 'function greet(name){if(name){return "hi "+name;}else{return "hi";}}',
  java: 'public class Demo { public static void main(String[] args) { System.out.println("hi"); } }',
  csharp: 'public class Demo { public int Id { get; set; } public void Run() { Console.WriteLine("hi"); } }',
  go: 'package main\nfunc main() { fmt.Println("hi") }',
  php: '<?php function run($a) { if ($a > 1) { return $a; } return 0; }',
  c: '#include <stdio.h>\nint main(){int a=1;if(a>0){printf("hi");}return 0;}',
  sql: 'select id, name, count(*) as total from t_order where status=1 group by id, name having count(*)>2 order by total desc limit 10',
}

const stats = computed(() => {
  if (!output.value) return ''
  return `${output.value.split('\n').length} 行 · ${output.value.length} 字符`
})

watch(language, () => {
  output.value = ''
  error.value = ''
})

function run() {
  error.value = ''
  try {
    output.value = formatCodeByLanguage(language.value, input.value, mode.value, indent.value)
    if (!output.value) message.warning('内容为空，未生成结果')
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    output.value = ''
  }
}

function loadSample() {
  input.value = SAMPLES[language.value]
  mode.value = 'pretty'
  run()
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

function clearAll() {
  input.value = ''
  output.value = ''
  error.value = ''
}
</script>

<template>
  <div class="fmt">
    <div class="fmt-toolbar">
      <div class="fmt-left">
        <a-select v-model:value="language" size="small" :style="{ width: '150px' }">
          <a-select-option v-for="item in LANGUAGE_OPTIONS" :key="item.value" :value="item.value">
            {{ item.label }}
          </a-select-option>
        </a-select>
        <a-segmented
          v-model:value="mode"
          size="small"
          :options="[
            { label: '美化', value: 'pretty' },
            { label: '压缩', value: 'minify' },
          ]"
        />
        <span class="fmt-field">
          <span class="fmt-label">缩进</span>
          <a-input-number v-model:value="indent" size="small" :min="1" :max="8" :style="{ width: '64px' }" />
        </span>
      </div>
      <div class="fmt-actions">
        <a-button size="small" @click="loadSample">示例</a-button>
        <a-button size="small" @click="clearAll"><DeleteOutlined /> 清空</a-button>
        <a-button type="primary" size="small" @click="run"><PlayCircleOutlined /> 执行</a-button>
        <a-button size="small" :disabled="!output" @click="copyOutput"><CopyOutlined /> 复制</a-button>
      </div>
    </div>

    <div class="fmt-body">
      <section class="fmt-pane">
        <header class="fmt-head">源代码</header>
        <CodeArea v-model="input" :rows="22" placeholder="粘贴需要格式化的代码" />
      </section>
      <section class="fmt-pane">
        <header class="fmt-head">
          <span>格式化结果</span>
          <span class="fmt-stat">{{ stats }}</span>
        </header>
        <a-alert v-if="error" type="error" :message="error" show-icon class="fmt-error" />
        <CodeArea :model-value="output" :rows="22" readonly placeholder="结果会显示在这里" />
      </section>
    </div>
  </div>
</template>

<style scoped>
.fmt {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.fmt-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.fmt-left,
.fmt-actions,
.fmt-field {
  display: flex;
  align-items: center;
  gap: 8px;
}

.fmt-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.fmt-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.fmt-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.fmt-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.fmt-stat {
  margin-left: auto;
  font-size: 11px;
  color: var(--fm-muted);
}

.fmt-error {
  margin-bottom: 0;
}

@media (max-width: 1200px) {
  .fmt-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
