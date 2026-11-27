<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { ReloadOutlined } from '@ant-design/icons-vue'
import { message } from 'ant-design-vue'
import PageHeader from '@/components/PageHeader.vue'
import TokenCard from '@/components/token/TokenCard.vue'
import TokenKpiRow from '@/components/token/TokenKpiRow.vue'
import TokenHeatmap from '@/components/token/TokenHeatmap.vue'
import TokenBars from '@/components/token/TokenBars.vue'
import TokenHistogram from '@/components/token/TokenHistogram.vue'
import TokenStackedHours from '@/components/token/TokenStackedHours.vue'
import TokenCoverage from '@/components/token/TokenCoverage.vue'
import TokenTrend, { type TrendMetric } from '@/components/token/TokenTrend.vue'
import TokenDonut from '@/components/token/TokenDonut.vue'
import TokenComposition from '@/components/token/TokenComposition.vue'
import { tokenUsageClients, tokenUsageReveal, tokenUsageRoots, tokenUsageScan } from '@/api/tokenUsage'
import { aggregateTokenUsage } from '@/utils/tokenUsageAgg'
import { fmtCredit, fmtInt, fmtTime, fmtTokens, fmtUsd, USD_TO_CNY } from '@/utils/tokenUsageFormat'
import { TOKEN_SOURCE_LABEL, type ClientProbe, type RootInfo, type ScanResult, type TokenSourceId } from '@/types/tokenUsage'

type HourMetric = 'requests' | 'tokens'

const RANGES = [
  { value: 1, label: '今日' },
  { value: 7, label: '近 7 天' },
  { value: 30, label: '近 30 天' },
  { value: 0, label: '全部' },
]

const data = shallowRef<ScanResult | null>(null)
const clients = ref<ClientProbe[]>([])
const roots = ref<RootInfo[]>([])
const loading = ref(false)
const probing = ref(false)
const error = ref('')
const range = ref(7)
const source = ref<TokenSourceId | null>(null)
const live = ref(true)
const hourMetric = ref<HourMetric>('requests')
/** 卡片 2 的切面：工作空间 / 供应商中转 / 数据来源 */
const distMode = ref<'workspace' | 'provider' | 'source'>('workspace')
/** 趋势折线看哪个指标 */
const trendMetric = ref<TrendMetric>('tokens')

let pollTimer: number | undefined

const agg = computed(() =>
  data.value ? aggregateTokenUsage(data.value.records, range.value, source.value) : null,
)

/** 只显示本机真实存在的来源 */
const sourceOptions = computed(() => {
  const stats = data.value?.stats.sources
  const items: { value: TokenSourceId | null; label: string }[] = [{ value: null, label: '全部来源' }]
  ;(Object.keys(TOKEN_SOURCE_LABEL) as TokenSourceId[]).forEach((id) => {
    if (stats?.[id]?.found) items.push({ value: id, label: TOKEN_SOURCE_LABEL[id] })
  })
  return items
})

const scanMeta = computed(() => {
  const d = data.value
  if (!d) return '正在扫描本机日志…'
  const r = d.stats
  return `${fmtInt(r.files)} 个日志文件 · ${fmtInt(d.records.length)} 条请求 · 扫描 ${r.durationMs}ms · 更新 ${fmtTime(r.scannedAt)}`
})

const missingRoots = computed(() => roots.value.filter((r) => !r.exists))

/** 抓包旁路记账的统计（只在抓包期间有数据） */
const captureStat = computed(() => data.value?.stats.sources.capture)
const totalAll = computed(() =>
  data.value ? data.value.records.reduce((sum, r) => sum + r.total, 0) : 0,
)

/** 模型占比环形图：Top 模型 + 其他 */
const modelSlices = computed(() => {
  const md = agg.value?.modelDaily
  if (!md) return []
  const items = md.models.map((name, i) => ({ name, value: md.modelTotals[i] ?? 0 }))
  if (md.otherTotal > 0) items.push({ name: '其他模型', value: md.otherTotal })
  return items.filter((item) => item.value > 0)
})

/** 用量分布的三个切面 */
const distRows = computed(() => {
  const a = agg.value
  if (!a) return []
  if (distMode.value === 'provider') return a.providers
  if (distMode.value === 'source') return a.sources
  return a.workspaces
})

const distNoun = computed(
  () =>
    ({ provider: '供应商', source: '来源', workspace: '工作空间' })[distMode.value] ?? '工作空间',
)

const distSubtitle = computed(
  () =>
    ({
      provider: '按供应商 / 中转聚合，看 token 流向哪个通道',
      source: '按客户端数据来源聚合，看每个工具各占多少',
      workspace: '按会话 cwd 末段聚合，看哪个项目最费 token',
    })[distMode.value] ?? '',
)

const distFootnote = computed(
  () =>
    ({
      provider:
        '「CC Switch 代理」行来自代理流水里的供应商字段；本地日志来源没有供应商信息，按客户端名归组',
      source:
        '抓包观测来自录制期间客户端直连的响应 usage，与 CC Switch 代理流水已按「总量 + 输出 + 一分钟内」判重',
      workspace: '',
    })[distMode.value] ?? '',
)

/** 每日模型明细表 */
const dailyColumns = computed(() => {
  const md = agg.value?.modelDaily
  if (!md) return []
  const cols: {
    title: string
    dataIndex: string
    key: string
    width?: number
    align?: 'left' | 'right'
  }[] = [{ title: '日期', dataIndex: 'date', key: 'date', width: 96, align: 'left' }]
  md.models.forEach((model, i) => {
    cols.push({
      title: model,
      dataIndex: `m${i}`,
      key: `m${i}`,
      width: 106,
      align: 'right',
    })
  })
  if (md.otherTotal > 0) {
    cols.push({ title: '其他', dataIndex: 'other', key: 'other', width: 84, align: 'right' })
  }
  cols.push(
    { title: '合计', dataIndex: 'total', key: 'total', width: 92, align: 'right' },
    { title: '请求', dataIndex: 'requests', key: 'requests', width: 68, align: 'right' },
    { title: '积分', dataIndex: 'credit', key: 'credit', width: 64, align: 'right' },
    { title: '费用', dataIndex: 'cost', key: 'cost', width: 76, align: 'right' },
  )
  return cols
})

const dailyRows = computed(() => {
  const md = agg.value?.modelDaily
  if (!md) return []
  return md.dates.map((date, i) => {
    const row: Record<string, string | number> = {
      key: date,
      date,
      other: md.other[i] ?? 0,
      total: md.totals[i] ?? 0,
      requests: md.dayRequests[i] ?? 0,
      credit: Math.round((md.dayCredit[i] ?? 0) * 100) / 100,
      cost: fmtUsd(md.dayCost[i] ?? 0),
    }
    md.models.forEach((_, m) => {
      row[`m${m}`] = md.tokens[i]?.[m] ?? 0
    })
    return row
  })
})

async function load(force = false) {
  loading.value = true
  try {
    const result = await tokenUsageScan(force)
    data.value = result
    error.value = ''
    if (!roots.value.length) void loadRoots()
    // 体检要读多个目录，单独异步，不拖住图表首屏
    probing.value = true
    void tokenUsageClients()
      .then((list) => {
        clients.value = list
      })
      .catch(() => {
        clients.value = []
      })
      .finally(() => {
        probing.value = false
      })
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

async function loadRoots() {
  try {
    roots.value = await tokenUsageRoots()
  } catch {
    roots.value = []
  }
}

async function reveal(path: string) {
  try {
    await tokenUsageReveal(path)
  } catch (e) {
    message.error(String(e))
  }
}

function startPolling() {
  stopPolling()
  pollTimer = window.setInterval(() => {
    if (live.value && !loading.value) void load(false)
  }, 30_000)
}

function stopPolling() {
  if (pollTimer !== undefined) {
    window.clearInterval(pollTimer)
    pollTimer = undefined
  }
}

onMounted(() => {
  void loadRoots()
  void load(false)
  startPolling()
})

onBeforeUnmount(stopPolling)
</script>

<template>
  <div class="page token-page">
    <PageHeader
      eyebrow="LLM Usage"
      title="Token 看板"
      subtitle="本机 WorkBuddy / Codex / Claude 的真实 token 用量 + CC Switch 代理记账 + 抓包观测：只读本地日志与账本，不上传"
    >
      <div class="head-actions">
        <span class="scan-meta">{{ scanMeta }}</span>
        <a-button :loading="loading" @click="load(false)">
          <template #icon><ReloadOutlined /></template>
          刷新
        </a-button>
        <a-button :disabled="loading" @click="load(true)">全量重扫</a-button>
      </div>
    </PageHeader>

    <div class="toolbar">
      <a-segmented v-model:value="range" :options="RANGES" size="small" />
      <a-segmented v-model:value="source" :options="sourceOptions" size="small" />
      <span class="spacer" />
      <label class="live-switch" title="开启后每 30 秒自动增量扫描一次">
        <a-switch v-model:checked="live" size="small" />
        <span>实时跟随</span>
      </label>
    </div>

    <a-alert v-if="error" type="error" show-icon class="tip" :message="`读取日志失败：${error}`" />

    <a-alert
      v-else-if="data && data.stats.files === 0"
      type="warning"
      show-icon
      class="tip"
      message="没有找到任何调用日志"
    >
      <template #description>
        <p>需要本机至少存在以下目录之一，并用过一次对应客户端：</p>
        <ul class="roots">
          <li v-for="root in roots" :key="root.dir">
            <code>{{ root.dir }}</code>
            <em>{{ root.exists ? '存在' : '不存在' }}</em>
          </li>
        </ul>
        <p v-if="missingRoots.length">当前缺失 {{ missingRoots.length }} 个目录。</p>
      </template>
    </a-alert>

    <template v-if="agg">
      <TokenKpiRow :kpis="agg.kpis" />

      <p class="capture-note">
        <template v-if="captureStat?.found">
          抓包观测：{{ fmtInt(captureStat.requests) }} 次调用 /
          {{ fmtTokens(captureStat.tokens) }} token，来自录制期间客户端直连的响应 usage（只记数字，不存正文）
        </template>
        <template v-else>
          抓包观测未开启：到「抓包」页开始录制，桌面版直连别家中转的调用也能记进这里（只记 token，不存正文）
        </template>
      </p>

      <div class="grid">
        <TokenCard
          class="span-2"
          title="1 · 每日用量趋势"
          :subtitle="`按天看 ${agg.windowLabel} 的走势，缺口补 0`"
          :span="2"
          :footnote="`日均与峰值都按日历天算，含没有调用的 0 值天；缓存命中率 = 缓存读 / 输入总量（含缓存），可切到该指标单独看`"
        >
          <template #extra>
            <a-segmented
              v-model:value="trendMetric"
              size="small"
              :options="[
                { value: 'tokens', label: 'Token' },
                { value: 'requests', label: '请求' },
                { value: 'cost', label: '费用' },
                { value: 'cacheRate', label: '缓存' },
              ]"
            />
          </template>
          <TokenTrend :daily="agg.daily" :metric="trendMetric" />
        </TokenCard>

        <TokenCard
          class="span-2"
          title="数据源体检"
          subtitle="本机哪些 AI 客户端能被读到，读不到的如实标注"
          :span="2"
          :footnote="`逐请求明细共 ${fmtInt(agg.recordCount)} 条，去重 ${fmtInt(data?.stats.deduped ?? 0)} 条；解析失败 ${fmtInt(data?.stats.parseErrors ?? 0)} 行（单行损坏不影响其余记录）`"
        >
          <TokenCoverage :clients="clients" :probing="probing" @reveal="reveal" />
        </TokenCard>

        <TokenCard
          title="2 · Token 构成"
          subtitle="一次调用里，新输入 / 缓存读 / 缓存写 / 输出各占多少"
          footnote="input 字段本身含缓存读取，这里已拆分，四项相加才是总量；推理 token 已含在输出内，不重复计入"
        >
          <TokenComposition :composition="agg.composition" />
        </TokenCard>

        <TokenCard
          title="3 · 模型占比"
          :subtitle="`Top ${agg.modelDaily.models.length} 模型 + 其他`"
          footnote="按 token 量排序；环上每段与图例同色，鼠标悬停可看具体数值与占比"
        >
          <TokenDonut
            :items="modelSlices"
            :total="agg.kpis.total"
            center-label="Token"
          />
        </TokenCard>

        <TokenCard
          class="span-2"
          title="4 · 日志分布（日期 × 小时）"
          subtitle="按真实 token 消耗着色，一眼看出你几点在干活"
          :span="2"
          :footnote="`所选窗口 ${agg.windowLabel}；空格子代表该小时没有调用`"
        >
          <TokenHeatmap :heat="agg.heat" />
        </TokenCard>

        <TokenCard
          title="5 · 用量分布"
          :subtitle="distSubtitle"
          :footnote="distFootnote"
        >
          <template #extra>
            <a-segmented
              v-model:value="distMode"
              size="small"
              :options="[
                { value: 'workspace', label: '工作空间' },
                { value: 'provider', label: '供应商' },
                { value: 'source', label: '来源' },
              ]"
            />
          </template>
          <TokenBars :rows="distRows" :noun="distNoun" />
        </TokenCard>

        <TokenCard title="6 · 单次请求 token 大小分布" subtitle="看单轮上下文有多重，判断是不是该拆会话">
          <TokenHistogram :sizes="agg.sizes" />
        </TokenCard>

        <TokenCard
          class="span-2"
          title="7 · 模型日使用分布（0–24 时）"
          subtitle="同一小时内各模型各占多少"
          :span="2"
          footnote="按小时堆叠；悬浮柱子可同时看到该小时该模型的次数与 token。空白时段代表没有调用。"
        >
          <template #extra>
            <a-segmented
              v-model:value="hourMetric"
              size="small"
              :options="[
                { value: 'requests', label: '调用次数' },
                { value: 'tokens', label: 'Token 量' },
              ]"
            />
          </template>
          <TokenStackedHours :data="agg.hourModel" :metric="hourMetric" />
        </TokenCard>

        <TokenCard
          class="span-2"
          title="8 · 每日模型用量明细"
          :subtitle="`Top ${agg.modelDaily.models.length} 模型 + 其他`"
          :span="2"
          :footnote="`按天倒序；token 与积分来自日志原始字段。费用＝CC Switch 代理流水取账面 total_cost_usd，本地日志按同一份价目表估算（输入 / 输出 / 缓存读 / 缓存写分别计价）；无价目的 ${fmtInt(agg.kpis.unpriced)} 条未计入。美元按固定汇率 ${USD_TO_CNY} 折算人民币，非实时行情。`"
        >
          <a-table
            size="small"
            class="daily-table"
            :columns="dailyColumns"
            :data-source="dailyRows"
            :pagination="false"
            :scroll="{ x: 1060, y: 260 }"
            :row-key="(row: Record<string, unknown>) => String(row.key)"
          >
            <template #bodyCell="{ column, text }">
              <template v-if="column.key === 'total'">{{ fmtTokens(Number(text)) }}</template>
              <template v-else-if="column.key === 'requests'">{{ fmtInt(Number(text)) }}</template>
              <template v-else-if="column.key === 'credit'">{{ fmtCredit(Number(text)) }}</template>
              <template v-else-if="column.key === 'cost'">{{ text }}</template>
              <template v-else-if="column.key === 'date'">{{ text }}</template>
              <template v-else-if="column.key !== 'date'">{{ fmtTokens(Number(text)) }}</template>
            </template>
          </a-table>
          <div v-if="!dailyRows.length" class="empty">所选窗口内没有记录</div>
        </TokenCard>
      </div>

      <p class="foot">
        统计口径：日志里 <code>providerData.usage</code> / <code>token_usage_record</code> 的真实 usage，
        非估算；积分取 <code>rawUsage.credit</code>；费用见明细表脚注（代理账面 + 价目表估算）。
        全量数据源 {{ fmtTokens(totalAll) }} token，全部留在本机，不上传。
      </p>
    </template>

    <div v-else-if="loading" class="loading">正在解析本机日志…</div>
  </div>
</template>

<style scoped>
.token-page {
  display: flex;
  flex-direction: column;
}

.head-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.scan-meta {
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.capture-note {
  margin: 0 0 12px;
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--fm-muted);
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: -6px 0 12px;
}

.spacer {
  flex: 1;
}

.live-switch {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 11.5px;
  color: var(--fm-muted);
  cursor: pointer;
  user-select: none;
}

.tip {
  margin-bottom: 12px;
}

.roots {
  margin: 4px 0 0;
  padding-left: 18px;
  font-size: 11px;
  line-height: 1.8;
}

.roots em {
  margin-left: 6px;
  font-style: normal;
  color: var(--fm-muted);
}

.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
  align-items: start;
}

.grid > .span-2 {
  grid-column: span 2;
}

.empty {
  padding: 28px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.loading {
  padding: 60px 0;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.foot {
  margin: 12px 0 0;
  font-size: 10.5px;
  line-height: 1.7;
  color: var(--fm-muted);
}

/* 模型名是标识符，大写化会读错；表头回到正文字体 */
.daily-table :deep(.ant-table-thead > tr > th) {
  text-transform: none;
  font-family: var(--fm-font-sans);
  letter-spacing: 0;
  font-size: 10.5px !important;
  color: var(--fm-text-2) !important;
  white-space: nowrap;
}

.foot code {
  padding: 1px 4px;
  border-radius: var(--fm-radius-sm);
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 10px;
}

@media (max-width: 1180px) {
  .grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .grid > .span-2 {
    grid-column: span 1;
  }
}
</style>
