<script setup lang="ts">
import { computed } from 'vue'
import { isDangerousTool, toolLabel, validateToolArgs } from '@/utils/aiTools'

const props = defineProps<{
  tool: string
  args: Record<string, unknown>
  description: string
  /** 导出类工具：目标文件是否已存在 */
  overwrite?: boolean
  path?: string
  /** 压测计划等需要人话预览的工具 */
  planText?: string
}>()

const labelMap: Record<string, string> = {
  projectId: '项目',
  collectionId: '集合',
  requestId: '接口',
  envId: '环境',
  id: '对象',
  name: '名称',
  title: '标题',
  method: '方法',
  url: '地址',
  path: '路径',
  concurrent: '并发数',
  durationSec: '时长（秒）',
  rampUpSec: '爬坡（秒）',
  thinkTimeMs: '思考时间（毫秒）',
  targetRps: '目标 RPS',
  rounds: '轮数',
  limit: '条数上限',
  page: '页码',
  pageSize: '每页条数',
  format: '格式',
  mode: '模式',
  script: '脚本',
  body: '请求体',
  headers: '请求头',
  params: '查询参数',
  variables: '变量',
  removeKeys: '删除的变量',
  preScript: '前置脚本',
  postScript: '后置脚本',
  assertions: '断言',
  slaP95: 'P95 阈值（毫秒）',
  slaP99: 'P99 阈值（毫秒）',
  slaErrorRate: '错误率阈值（%）',
  slaMinRps: '最低 RPS',
  success: '仅看成功请求',
  requestIds: '请求 ID 列表',
}

function display(value: unknown): string {
  if (value === null || value === undefined) return '—'
  if (typeof value === 'string') {
    const text = value.trim()
    if (!text) return '（空）'
    return text.length > 400 ? `${text.slice(0, 400)}…（共 ${text.length} 字符）` : text
  }
  if (typeof value === 'object') {
    try {
      const text = JSON.stringify(value)
      return text.length > 400 ? `${text.slice(0, 400)}…` : text
    } catch {
      return String(value)
    }
  }
  return String(value)
}

const rows = computed(() =>
  Object.entries(props.args ?? {})
    .filter(([, value]) => value !== undefined && value !== null && value !== '')
    .map(([key, value]) => ({
      key,
      label: labelMap[key] ?? key,
      value: display(value),
    })),
)

const targets = computed(() => {
  const args = props.args ?? {}
  const list: string[] = []
  const push = (value: unknown, label: string) => {
    if (value !== undefined && value !== null && value !== '') list.push(`${label} #${value}`)
  }
  push(args.projectId, '项目')
  push(args.collectionId, '集合')
  push(args.requestId, '接口')
  push(args.envId, '环境')
  push(args.id, '对象')
  if (typeof args.path === 'string' && args.path.trim()) list.push(`文件 ${args.path}`)
  return list.length ? [...new Set(list)] : ['本机数据']
})

const level = computed(() => {
  if (props.overwrite || isDangerousTool(props.tool)) return { text: '高', tone: 'danger' }
  return { text: '中', tone: 'warn' }
})

const risk = computed(() => {
  if (props.overwrite) {
    return `目标文件已存在：确认后原内容会被覆盖，无法撤销。`
  }
  if (isDangerousTool(props.tool)) {
    return '该操作会删除 / 覆盖本机数据或立即开始真实执行，确认后无法撤销。'
  }
  return '该操作会写入本机数据，可在执行后继续修改。'
})

const precheck = computed(() => {
  const invalid = validateToolArgs(props.tool, props.args ?? {})
  if (invalid) return { ok: false, text: `参数校验未通过：${invalid}` }
  if (typeof props.path === 'string' && props.path) {
    return props.overwrite
      ? { ok: false, text: `目标文件已存在：${props.path}` }
      : { ok: true, text: `目标文件不存在，将新建：${props.path}` }
  }
  return { ok: true, text: '参数校验通过，可以执行' }
})
</script>

<template>
  <div class="confirm-card">
    <section class="row">
      <div class="label">任务</div>
      <div class="value">
        <strong>{{ toolLabel(tool) }}</strong>
        <span class="tool-id">{{ tool }}</span>
      </div>
    </section>

    <section class="row">
      <div class="label">影响对象</div>
      <div class="value">
        <span v-for="item in targets" :key="item" class="chip">{{ item }}</span>
      </div>
    </section>

    <section v-if="planText" class="row">
      <div class="label">计划</div>
      <pre class="plan">{{ planText }}</pre>
    </section>

    <section class="row">
      <div class="label">参数</div>
      <div class="value">
        <table v-if="rows.length" class="params">
          <tbody>
            <tr v-for="row in rows" :key="row.key">
              <th>{{ row.label }}</th>
              <td>{{ row.value }}</td>
            </tr>
          </tbody>
        </table>
        <span v-else class="muted">无额外参数</span>
      </div>
    </section>

    <section class="row">
      <div class="label">风险</div>
      <div class="value">
        <span class="risk" :class="level.tone">风险 {{ level.text }}</span>
        <span>{{ risk }}</span>
      </div>
    </section>

    <section class="row">
      <div class="label">预检</div>
      <div class="value">
        <span class="precheck" :class="precheck.ok ? 'ok' : 'bad'">
          {{ precheck.ok ? '通过' : '注意' }}
        </span>
        <span>{{ precheck.text }}</span>
      </div>
    </section>

    <p class="foot">{{ description }}确认后才会真正写入本机数据；取消则不做任何改动。</p>
  </div>
</template>

<style scoped>
.confirm-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  font-size: 12.5px;
  color: var(--fm-text, #1f2329);
  line-height: 1.7;
}

.row {
  display: flex;
  gap: 10px;
}

.label {
  flex: 0 0 64px;
  color: var(--fm-muted, #8a8f99);
  font-size: 12px;
}

.value {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.tool-id {
  margin-left: 6px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11px;
  color: var(--fm-muted, #8a8f99);
}

.chip {
  padding: 1px 8px;
  border-radius: 4px;
  background: var(--fm-fill, #f2f4f7);
  border: 1px solid var(--fm-border, #e6e8eb);
  font-size: 11.5px;
}

.params {
  width: 100%;
  border-collapse: collapse;
  background: var(--fm-fill, #f7f8fa);
  border: 1px solid var(--fm-border, #e6e8eb);
  border-radius: 6px;
  overflow: hidden;
  display: table;
}

.plan {
  flex: 1;
  margin: 0;
  padding: 6px 8px;
  background: var(--fm-fill, #f7f8fa);
  border: 1px solid var(--fm-border, #e6e8eb);
  border-radius: 6px;
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-all;
}

.params th,
.params td {
  padding: 4px 8px;
  vertical-align: top;
  text-align: left;
  font-weight: 400;
  border-bottom: 1px solid var(--fm-border, #e6e8eb);
}

.params tr:last-child th,
.params tr:last-child td {
  border-bottom: none;
}

.params th {
  width: 30%;
  color: var(--fm-muted, #8a8f99);
  white-space: nowrap;
}

.params td {
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 11.5px;
  word-break: break-all;
}

.risk,
.precheck {
  padding: 0 6px;
  border-radius: 3px;
  font-size: 11.5px;
  border: 1px solid transparent;
}

.risk.danger,
.precheck.bad {
  color: #d4380d;
  background: #fff2e8;
  border-color: #ffbb96;
}

.risk.warn {
  color: #d46b08;
  background: #fff7e6;
  border-color: #ffd591;
}

.precheck.ok {
  color: #237804;
  background: #f6ffed;
  border-color: #b7eb8f;
}

.muted {
  color: var(--fm-muted, #8a8f99);
}

.foot {
  margin: 0;
  padding-top: 6px;
  border-top: 1px dashed var(--fm-border, #e6e8eb);
  color: var(--fm-muted, #8a8f99);
  font-size: 11.5px;
}
</style>
