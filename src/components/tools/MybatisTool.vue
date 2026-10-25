<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined, DeleteOutlined, PlayCircleOutlined } from '@ant-design/icons-vue'
import CodeArea from '@/components/CodeArea.vue'
import {
  fillLogSql,
  parseMybatisLog,
  renderMybatis,
  type LogStatement,
  type SqlMode,
  type SqlParam,
} from '@/utils/mybatis'

const source = ref<'template' | 'log'>('template')
const logText = ref('')
const logStatements = ref<LogStatement[]>([])
const logIndex = ref(0)
const logWarnings = ref<string[]>([])

const template = ref('')
const params = ref('')
const mode = ref<SqlMode>('literal')
const pretty = ref(true)
const sql = ref('')
const usedParams = ref<SqlParam[]>([])
const warnings = ref<string[]>([])
const error = ref('')

const currentLog = computed<LogStatement | null>(() => logStatements.value[logIndex.value] ?? null)

const logOptions = computed(() =>
  logStatements.value.map((item, index) => {
    const name = item.mapper ? item.mapper.slice(item.mapper.lastIndexOf('.') + 1) : ''
    return { value: index, label: `第 ${index + 1} 条${name ? ` · ${name}` : ''}` }
  }),
)

const SAMPLE_TEMPLATE = `<select id="findUsers" resultType="User">
  SELECT id, name, status, created_at
  FROM t_user
  <where>
    <if test="username != null and username != ''">
      AND name LIKE CONCAT('%', #{username}, '%')
    </if>
    <if test="status != null">
      AND status = #{status}
    </if>
    <if test="ids != null and ids.size() > 0">
      AND id IN
      <foreach collection="ids" item="id" open="(" separator="," close=")">
        #{id}
      </foreach>
    </if>
  </where>
  ORDER BY id DESC
  <if test="limit != null">
    LIMIT #{limit}
  </if>
</select>`

const SAMPLE_PARAMS = `{
  "username": "zhang",
  "status": 1,
  "ids": [3, 5, 8],
  "limit": 20
}`

const SAMPLE_LOG = `2026-10-12 17:28:07.367 [PC-ADMIN-1791797287341-799891232885] [http-nio-8888-exec-14] DEBUG cn.com.fink.oauth.mapper.SsoMenuMapper.queryMenu 135 debug - ==>  Preparing: SELECT m.* FROM sso_menu m UNION SELECT m.* FROM sso_menu m where menu_code IN ( SELECT SUBSTR(menu_code,1,(menu_level-?)*5) FROM sso_menu m WHERE m.menu_level>? UNION SELECT SUBSTR(menu_code,1,(menu_level-?)*5) FROM sso_menu m WHERE m.menu_level>? UNION SELECT SUBSTR(menu_code,1,(menu_level-?)*5) FROM sso_menu m WHERE m.menu_level>? ) order by menu_sort,menu_type
2026-10-12 17:28:07.367 [PC-ADMIN-1791797287341-799891232885] [http-nio-8888-exec-14] DEBUG cn.com.fink.oauth.mapper.SsoMenuMapper.queryMenu 135 debug - ==> Parameters: 1(Integer), 1(Integer), 2(Integer), 2(Integer), 3(Integer), 3(Integer)
2026-10-12 17:28:07.404 [PC-ADMIN-1791797287341-799891232885] [http-nio-8888-exec-14] DEBUG cn.com.fink.oauth.mapper.SsoMenuMapper.queryMenu 135 debug - <==      Total: 181`

function displayParam(value: unknown): string {
  if (value === undefined) return '缺失'
  if (value === null) return 'NULL'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

function run() {
  error.value = ''
  if (source.value === 'log') {
    runLog()
    return
  }
  if (!template.value.trim()) {
    error.value = '请先粘贴 Mapper 中的 SQL 片段'
    return
  }
  let payload: unknown = {}
  if (params.value.trim()) {
    try {
      payload = JSON.parse(params.value)
    } catch (err) {
      error.value = `参数 JSON 解析失败：${err instanceof Error ? err.message : String(err)}`
      return
    }
  }
  try {
    const result = renderMybatis(template.value, payload, mode.value, pretty.value)
    sql.value = result.sql
    usedParams.value = result.params
    warnings.value = result.warnings
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

function runLog() {
  if (!logText.value.trim()) {
    error.value = '请先粘贴 MyBatis DEBUG 日志'
    return
  }
  const parsed = parseMybatisLog(logText.value)
  logStatements.value = parsed.statements
  logWarnings.value = parsed.warnings
  if (!parsed.statements.length) {
    sql.value = ''
    usedParams.value = []
    warnings.value = parsed.warnings
    error.value = '未在日志中找到 "==> Preparing:" 语句'
    return
  }
  logIndex.value = Math.min(logIndex.value, parsed.statements.length - 1)
  applyLogStatement()
}

function applyLogStatement() {
  const statement = logStatements.value[logIndex.value]
  if (!statement) return
  const result = fillLogSql(statement, pretty.value)
  sql.value = result.sql
  usedParams.value = result.params
  warnings.value = [...logWarnings.value, ...result.warnings]
}

function loadSample() {
  if (source.value === 'log') {
    logText.value = SAMPLE_LOG
    logIndex.value = 0
    runLog()
    return
  }
  template.value = SAMPLE_TEMPLATE
  params.value = SAMPLE_PARAMS
  run()
}

function clearAll() {
  template.value = ''
  params.value = ''
  logText.value = ''
  logStatements.value = []
  logWarnings.value = []
  logIndex.value = 0
  sql.value = ''
  usedParams.value = []
  warnings.value = []
  error.value = ''
}

watch(source, () => {
  sql.value = ''
  usedParams.value = []
  warnings.value = []
  error.value = ''
  logStatements.value = []
  logIndex.value = 0
})

watch(pretty, () => {
  if (source.value === 'log' && logStatements.value.length) applyLogStatement()
})

async function copySql() {
  if (!sql.value) return
  try {
    await navigator.clipboard.writeText(sql.value)
    message.success('SQL 已复制')
  } catch {
    message.error('复制失败')
  }
}
</script>

<template>
  <div class="mb">
    <div class="mb-bar">
      <a-radio-group v-model:value="source" size="small" button-style="solid">
        <a-radio-button value="template">动态 SQL</a-radio-button>
        <a-radio-button value="log">日志回填</a-radio-button>
      </a-radio-group>
      <a-radio-group v-if="source === 'template'" v-model:value="mode" size="small" button-style="solid">
        <a-radio-button value="literal">填充为字面量</a-radio-button>
        <a-radio-button value="placeholder">保留占位符</a-radio-button>
      </a-radio-group>
      <a-checkbox v-model:checked="pretty">自动美化 SQL</a-checkbox>
      <a-button size="small" @click="loadSample">示例</a-button>
      <a-button size="small" @click="clearAll"><DeleteOutlined /> 清空</a-button>
      <a-button type="primary" size="small" @click="run">
        <PlayCircleOutlined /> {{ source === 'log' ? '解析并填充' : '生成 SQL' }}
      </a-button>
    </div>

    <a-alert v-if="error" type="error" :message="error" show-icon />

    <div class="mb-body">
      <section v-if="source === 'template'" class="mb-pane">
        <header class="mb-head">Mapper 片段</header>
        <CodeArea v-model="template" :rows="18" placeholder="粘贴 <select> / <update> 等动态 SQL 片段" />
        <header class="mb-head">参数 JSON</header>
        <CodeArea v-model="params" :rows="7" placeholder="{ &quot;username&quot;: &quot;zhang&quot; }" />
      </section>

      <section v-else class="mb-pane">
        <header class="mb-head">
          <span>MyBatis 日志</span>
          <span class="mb-head-hint">支持 Preparing / Parameters / Total 多语句日志</span>
        </header>
        <CodeArea
          v-model="logText"
          :rows="25"
          placeholder="粘贴包含 ==> Preparing: 与 ==> Parameters: 的 DEBUG 日志"
        />
      </section>

      <section class="mb-pane">
        <header class="mb-head">
          <span>生成结果</span>
          <a-select
            v-if="source === 'log' && logStatements.length > 1"
            v-model:value="logIndex"
            :options="logOptions"
            size="small"
            class="mb-stmt-select"
            @change="applyLogStatement"
          />
          <a-button type="link" size="small" :disabled="!sql" @click="copySql"><CopyOutlined /> 复制</a-button>
        </header>

        <div v-if="source === 'log' && currentLog" class="mb-meta">
          <span v-if="currentLog?.mapper" class="mb-meta-item">
            <span class="mb-meta-label">Mapper</span>{{ currentLog?.mapper }}
          </span>
          <span v-if="currentLog?.time" class="mb-meta-item">
            <span class="mb-meta-label">时间</span>{{ currentLog?.time }}
          </span>
          <span v-if="currentLog?.total != null" class="mb-meta-item">
            <span class="mb-meta-label">结果行数</span>{{ currentLog?.total }}
          </span>
          <span v-if="currentLog?.updates != null" class="mb-meta-item">
            <span class="mb-meta-label">影响行数</span>{{ currentLog?.updates }}
          </span>
        </div>

        <CodeArea :model-value="sql" :rows="18" readonly placeholder="生成的 SQL 会显示在这里" />

        <div v-if="warnings.length" class="mb-warnings">
          <a-alert v-for="(item, index) in warnings" :key="index" type="warning" :message="item" show-icon />
        </div>

        <div v-if="usedParams.length" class="mb-params">
          <div class="mb-params-title">参数清单（{{ usedParams.length }} 项）</div>
          <div v-for="item in usedParams" :key="`${item.name}-${item.value}`" class="mb-param">
            <span class="mb-param-name mono">
              {{ item.name }}
              <span v-if="item.jdbcType" class="mb-param-type">{{ item.jdbcType }}</span>
            </span>
            <span class="mb-param-value mono">{{ displayParam(item.value) }}</span>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.mb {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.mb-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.mb-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.mb-pane {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.mb-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.mb-head-hint {
  font-weight: 400;
  font-size: 11px;
  color: var(--fm-muted);
}

.mb-stmt-select {
  min-width: 200px;
  margin-left: auto;
}

.mb-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 8px 10px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  font-size: 11px;
}

.mb-meta-item {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--fm-text-2);
  word-break: break-all;
}

.mb-meta-label {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 4px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  line-height: 18px;
}

.mb-warnings {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.mb-params {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  overflow: hidden;
}

.mb-params-title {
  padding: 6px 12px;
  font-size: 12px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
}

.mb-param {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 5px 12px;
  font-size: 12px;
  border-top: 1px solid var(--fm-border);
}

.mb-param-name {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--fm-json-key);
}

.mb-param-type {
  font-family: var(--fm-font);
  font-size: 10px;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  line-height: 16px;
}

.mb-param-value {
  color: var(--fm-text-2);
  word-break: break-all;
  text-align: right;
}

@media (max-width: 1200px) {
  .mb-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
