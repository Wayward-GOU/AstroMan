<script setup lang="ts">
import { computed, h, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { useRouter } from 'vue-router'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import {
  ArrowDownOutlined,
  ArrowUpOutlined,
  DeleteOutlined,
  EyeInvisibleOutlined,
  EyeOutlined,
  PlusOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import type { PoolItemStatus, PoolItemView, PoolItemsPage, PoolSummary } from '@/types/models'

const router = useRouter()

/* ---------- 池列表 ---------- */

const pools = ref<PoolSummary[]>([])
const poolsLoading = ref(false)
const poolSearch = ref('')
const activePoolId = ref<number | null>(null)

const filteredPools = computed(() => {
  const keyword = poolSearch.value.trim().toLowerCase()
  if (!keyword) return pools.value
  return pools.value.filter((pool) => pool.name.toLowerCase().includes(keyword))
})

const activePool = computed(
  () => pools.value.find((pool) => pool.id === activePoolId.value) ?? null,
)

/** 动态列较多时给出表格最小总宽度，避免列被挤压；系统列宽与模板保持一致 */
const itemTableScrollX = computed(() => {
  const dynamicColumns = activePool.value?.columns?.length ?? 0
  return 64 + dynamicColumns * 160 + 96 + 72 + 64 + 110 + 190
})

async function loadPools(selectId?: number) {
  poolsLoading.value = true
  try {
    pools.value = await api.poolList()
    const next = selectId ?? activePoolId.value
    const exists = pools.value.some((pool) => pool.id === next)
    activePoolId.value = exists ? (next ?? null) : (pools.value[0]?.id ?? null)
  } catch (error) {
    message.error(String(error))
    pools.value = []
    activePoolId.value = null
  } finally {
    poolsLoading.value = false
  }
}

/* ---------- 新建池 ---------- */

/** 类型对应的默认列；切换类型时整体重置 */
const DEFAULT_COLUMNS: Record<'token' | 'parameter', string[]> = {
  token: ['token', 'account'],
  parameter: ['value'],
}

const createOpen = ref(false)
const newPoolName = ref('')
const newPoolKind = ref<'parameter' | 'token'>('token')
const newPoolSensitive = ref(true)
const newPoolColumns = ref<string[]>([...DEFAULT_COLUMNS.token])
const createBusy = ref(false)

watch(newPoolKind, (kind) => {
  newPoolColumns.value = [...DEFAULT_COLUMNS[kind]]
})

function openCreatePool() {
  newPoolName.value = ''
  newPoolKind.value = 'token'
  newPoolSensitive.value = true
  newPoolColumns.value = [...DEFAULT_COLUMNS.token]
  createOpen.value = true
}

/** 上下移动数组元素；dir = -1 上移 / 1 下移 */
function moveBy<T>(list: T[], index: number, dir: -1 | 1) {
  const next = index + dir
  if (next < 0 || next >= list.length) return
  const [item] = list.splice(index, 1)
  list.splice(next, 0, item)
}

function addCreateColumn() {
  newPoolColumns.value.push('')
}

function removeCreateColumn(index: number) {
  if (newPoolColumns.value.length <= 1) {
    message.warning('至少保留一列')
    return
  }
  newPoolColumns.value.splice(index, 1)
}

function moveCreateColumn(index: number, dir: -1 | 1) {
  moveBy(newPoolColumns.value, index, dir)
}

/** 列名前端校验：空列名 / 重名直接拦下 */
function validateColumns(names: string[]): boolean {
  if (!names.length) {
    message.warning('至少需要一列')
    return false
  }
  if (names.some((name) => !name.trim())) {
    message.warning('列名不能为空')
    return false
  }
  const seen = new Set<string>()
  for (const name of names) {
    const key = name.trim()
    if (seen.has(key)) {
      message.warning(`列名重复：${key}`)
      return false
    }
    seen.add(key)
  }
  return true
}

async function createPool() {
  const name = newPoolName.value.trim()
  if (!name) {
    message.warning('请输入池名称')
    return
  }
  const columns = newPoolColumns.value.map((item) => item.trim())
  if (!validateColumns(columns)) return
  createBusy.value = true
  try {
    const created = await api.poolCreate({
      name,
      kind: newPoolKind.value,
      sensitive: newPoolSensitive.value,
      description: '',
      columns,
    })
    createOpen.value = false
    await loadPools(created.id)
    await reloadItems(true)
    message.success('池已创建')
  } catch (error) {
    message.error(String(error))
  } finally {
    createBusy.value = false
  }
}

/* ---------- 编辑列 ---------- */

interface ColumnRow {
  /** 旧列名；新增列为空字符串 */
  original: string
  name: string
}

const columnsOpen = ref(false)
const columnRows = ref<ColumnRow[]>([])
const columnsBusy = ref(false)

function openColumnsEdit() {
  if (!activePool.value) return
  columnRows.value = (activePool.value.columns ?? []).map((name) => ({ original: name, name }))
  columnsOpen.value = true
}

function addColumnRow() {
  columnRows.value.push({ original: '', name: '' })
}

function removeColumnRow(index: number) {
  if (columnRows.value.length <= 1) {
    message.warning('至少保留一列')
    return
  }
  columnRows.value.splice(index, 1)
}

function moveColumnRow(index: number, dir: -1 | 1) {
  moveBy(columnRows.value, index, dir)
}

async function saveColumns() {
  if (!activePool.value) return
  const names = columnRows.value.map((row) => row.name.trim())
  if (!validateColumns(names)) return
  // 保持数组顺序：from 表示旧列名，新增列只传 name
  const changes = columnRows.value.map((row) =>
    row.original ? { from: row.original, name: row.name.trim() } : { name: row.name.trim() },
  )
  const poolId = activePool.value.id
  const hasItems = (activePool.value.itemCount ?? 0) > 0
  const apply = async () => {
    columnsBusy.value = true
    try {
      await api.poolColumnsSet(poolId, changes)
      columnsOpen.value = false
      message.success('列定义已更新')
      await loadPools()
      await reloadItems(true)
    } catch (error) {
      message.error(String(error))
    } finally {
      columnsBusy.value = false
    }
  }
  if (hasItems) {
    Modal.confirm({
      title: '确认调整列定义？',
      content: '改名会按旧列名迁移数据；删除列会丢弃该列已有数据，且无法恢复。',
      okText: '确认调整',
      cancelText: '取消',
      onOk: apply,
    })
  } else {
    await apply()
  }
}

function deletePool(pool: PoolSummary) {
  Modal.confirm({
    title: `删除池「${pool.name}」？`,
    content: '池内条目会一并删除，绑定该池的计划将无法再预览或启动。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        await api.poolDelete(pool.id)
        if (activePoolId.value === pool.id) activePoolId.value = null
        await loadPools()
        await reloadItems(true)
        message.success('池已删除')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

/* ---------- 条目分页 ---------- */

const pageData = ref<PoolItemsPage | null>(null)
const itemsLoading = ref(false)
const page = ref(1)
const pageSize = ref(50)
const itemSearch = ref('')
const statusFilter = ref<PoolItemStatus | ''>('')

const statusOptions: Array<{ value: PoolItemStatus | ''; label: string }> = [
  { value: '', label: '全部状态' },
  { value: 'active', label: '可用' },
  { value: 'cooling', label: '冷却中' },
  { value: 'invalid', label: '失效' },
  { value: 'expired', label: '已过期' },
  { value: 'disabled', label: '已停用' },
]

const STATUS_LABELS: Record<string, string> = {
  active: '可用',
  cooling: '冷却中',
  invalid: '失效',
  expired: '已过期',
  disabled: '已停用',
}

const STATUS_COLORS: Record<string, string> = {
  active: 'green',
  cooling: 'gold',
  invalid: 'red',
  expired: 'volcano',
  disabled: 'default',
}

async function loadItems() {
  if (!activePoolId.value) {
    pageData.value = null
    return
  }
  itemsLoading.value = true
  try {
    pageData.value = await api.poolItemsPage({
      poolId: activePoolId.value,
      page: page.value,
      pageSize: pageSize.value,
      search: itemSearch.value.trim(),
      status: statusFilter.value,
    })
  } catch (error) {
    message.error(String(error))
    pageData.value = null
  } finally {
    itemsLoading.value = false
  }
}

/** reset=true 时回到第一页；用于切换池、导入、删除等结构性变更 */
async function reloadItems(reset = false) {
  if (reset) page.value = 1
  clearReveals()
  await loadItems()
  // 列表上的条目数/健康度与分页数据来自同一张表，这里顺手刷新左侧摘要
  await loadPools()
}

let searchTimer: number | undefined
watch(itemSearch, () => {
  if (searchTimer) window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    page.value = 1
    clearReveals()
    void loadItems()
  }, 300)
})

watch(statusFilter, () => {
  page.value = 1
  clearReveals()
  void loadItems()
})

watch(activePoolId, () => {
  itemSearch.value = ''
  statusFilter.value = ''
  page.value = 1
  clearReveals()
  void loadItems()
})

function onPageChange(nextPage: number, nextSize: number) {
  page.value = nextPage
  pageSize.value = nextSize
  clearReveals()
  void loadItems()
}

/* ---------- 单条明文：二次确认 + 30 秒自动隐藏 ---------- */

/** 已显示明文的条目：key 为条目 id，value 为各列明文 */
const revealed = reactive<Record<number, Record<string, string>>>({})
/** 明文剩余秒数 */
const revealLeft = reactive<Record<number, number>>({})
let revealTicker: number | undefined

const REVEAL_SECONDS = 30

function startRevealTicker() {
  if (revealTicker) return
  revealTicker = window.setInterval(() => {
    let changed = false
    Object.keys(revealLeft).forEach((key) => {
      const id = Number(key)
      revealLeft[id] -= 1
      if (revealLeft[id] <= 0) {
        delete revealLeft[id]
        delete revealed[id]
        changed = true
      }
    })
    if (changed && !Object.keys(revealLeft).length) stopRevealTicker()
  }, 1000)
}

function stopRevealTicker() {
  if (revealTicker) {
    window.clearInterval(revealTicker)
    revealTicker = undefined
  }
}

function hideReveal(itemId: number) {
  delete revealLeft[itemId]
  delete revealed[itemId]
  if (!Object.keys(revealLeft).length) stopRevealTicker()
}

/** 切换池 / 翻页 / 筛选 / 离开页面都立即清空已显示的明文 */
function clearReveals() {
  Object.keys(revealed).forEach((key) => delete revealed[Number(key)])
  Object.keys(revealLeft).forEach((key) => delete revealLeft[Number(key)])
  stopRevealTicker()
}

function revealItem(item: PoolItemView) {
  if (revealed[item.id]) {
    hideReveal(item.id)
    return
  }
  Modal.confirm({
    title: '显示明文？',
    content: '明文只在本页显示 30 秒，切换池、翻页或离开页面会立即隐藏，请勿截屏外传。',
    okText: '显示明文',
    cancelText: '取消',
    onOk: async () => {
      try {
        const result = await api.poolItemReveal(item.id)
        revealed[item.id] = result.data ?? {}
        revealLeft[item.id] = REVEAL_SECONDS
        startRevealTicker()
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

/* ---------- 条目操作 ---------- */

async function toggleItem(item: PoolItemView) {
  try {
    await api.poolItemToggle(item.id, !item.enabled)
    hideReveal(item.id)
    await loadItems()
    await loadPools()
  } catch (error) {
    message.error(String(error))
  }
}

async function removeItem(item: PoolItemView) {
  try {
    await api.poolDeleteItem(item.id)
    hideReveal(item.id)
    await reloadItems()
  } catch (error) {
    message.error(String(error))
  }
}

function clearItems() {
  if (!activePoolId.value || !activePool.value) return
  const poolName = activePool.value.name
  Modal.confirm({
    title: `清空池「${poolName}」的条目？`,
    content: `将删除全部 ${pageData.value?.health.total ?? 0} 条，池本身保留。`,
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    onOk: async () => {
      try {
        const count = await api.poolClearItems(activePoolId.value as number)
        await reloadItems(true)
        message.success(`已清空 ${count} 条`)
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

const refreshBusy = ref(false)

async function refreshHealth() {
  if (!activePoolId.value) return
  refreshBusy.value = true
  try {
    const health = await api.poolHealthRefresh(activePoolId.value)
    await loadItems()
    await loadPools()
    message.success(
      `健康度已刷新：可用 ${health.active} / 失效 ${health.invalid} / 过期 ${health.expired} / 冷却 ${health.cooling}`,
    )
  } catch (error) {
    message.error(String(error))
  } finally {
    refreshBusy.value = false
  }
}

/* ---------- 导入 ---------- */

const excelBusy = ref(false)

/** 下载 Excel 模板：按当前池的列定义生成表头 */
async function downloadTemplate() {
  if (!activePool.value) return
  try {
    const path = await saveDialog({
      defaultPath: `${activePool.value.name}-导入模板.xlsx`,
      filters: [{ name: 'Excel 工作簿', extensions: ['xlsx'] }],
    })
    if (!path) return
    excelBusy.value = true
    await api.poolExcelTemplate(activePool.value.id, path)
    message.success('模板已保存')
  } catch (error) {
    message.error(String(error))
  } finally {
    excelBusy.value = false
  }
}

/** 导入 Excel：表头必须与列名一致，首列必填 */
async function importExcel() {
  if (!activePool.value) return
  try {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: 'Excel 工作簿', extensions: ['xlsx'] }],
    })
    if (!path || Array.isArray(path)) return
    excelBusy.value = true
    const result = await api.poolExcelImport(activePool.value.id, path)
    await reloadItems(true)
    message.success(`导入完成：新增 ${result.inserted} 条，跳过 ${result.skipped} 条`)
    if (result.ignoredColumns.length) {
      message.warning(`未匹配的列已忽略：${result.ignoredColumns.join('、')}`)
    }
    if (result.numericWarnings.length) {
      const shown = result.numericWarnings.slice(0, 10)
      Modal.info({
        title: '数值格式提醒',
        width: 520,
        content: h('div', { style: 'font-size:12.5px;line-height:1.7' }, [
          ...shown.map((text) => h('div', text)),
          result.numericWarnings.length > shown.length
            ? h(
                'div',
                { style: 'color:#6f7c85' },
                `其余 ${result.numericWarnings.length - shown.length} 条已省略`,
              )
            : null,
        ]),
      })
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    excelBusy.value = false
  }
}

const importText = ref('')
const importBusy = ref(false)

async function importItems() {
  if (!activePoolId.value) return
  if (!importText.value.trim()) {
    message.warning('请输入要导入的数据')
    return
  }
  importBusy.value = true
  try {
    const count = await api.poolImport(activePoolId.value, importText.value)
    importText.value = ''
    await reloadItems(true)
    message.success(`已导入 ${count} 条`)
  } catch (error) {
    message.error(String(error))
  } finally {
    importBusy.value = false
  }
}

/* ---------- 格式化 ---------- */

function fmtTime(value?: number | null): string {
  if (!value) return '—'
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getMonth() + 1}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(
    date.getMinutes(),
  )}`
}

function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status
}

function statusColor(status: string): string {
  return STATUS_COLORS[status] ?? 'default'
}

/* ---------- Token 刷新配置（v19） ---------- */

const refreshOpen = ref(false)
const refreshConfigBusy = ref(false)
const refreshForm = reactive({
  mode: 'none',
  method: 'POST',
  url: '',
  headersText: '',
  body: '',
  tokenPath: '$.data.accessToken',
  expiresPath: '$.data.expiresIn',
  expiresInSec: 0,
})

async function openRefreshConfig() {
  if (!activePool.value) return
  refreshConfigBusy.value = true
  try {
    const config = await api.poolRefreshGet(activePool.value.name)
    refreshForm.mode = config.mode || 'none'
    refreshForm.method = config.method || 'POST'
    refreshForm.url = config.url || ''
    refreshForm.headersText = (config.headers || [])
      .filter((item) => item.enabled)
      .map((item) => `${item.key}: ${item.value}`)
      .join('\n')
    refreshForm.body = config.body || ''
    refreshForm.tokenPath = config.tokenPath || '$.data.accessToken'
    refreshForm.expiresPath = config.expiresPath || '$.data.expiresIn'
    refreshForm.expiresInSec = config.expiresInSec || 0
    refreshOpen.value = true
  } catch (error) {
    message.error(String(error))
  } finally {
    refreshConfigBusy.value = false
  }
}

async function saveRefreshConfig() {
  if (!activePool.value) return
  if (refreshForm.mode === 'http' && !refreshForm.url.trim()) {
    message.warning('请填写刷新地址')
    return
  }
  const headers = refreshForm.headersText
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const index = line.indexOf(':')
      if (index < 0) return { key: line, value: '', enabled: true }
      return {
        key: line.slice(0, index).trim(),
        value: line.slice(index + 1).trim(),
        enabled: true,
      }
    })
  refreshConfigBusy.value = true
  try {
    await api.poolRefreshSet(activePool.value.name, {
      mode: refreshForm.mode,
      method: refreshForm.method,
      url: refreshForm.url,
      headers,
      body: refreshForm.body,
      tokenPath: refreshForm.tokenPath,
      expiresPath: refreshForm.expiresPath,
      expiresInSec: refreshForm.expiresInSec,
    })
    refreshOpen.value = false
    message.success('刷新配置已保存')
  } catch (error) {
    message.error(String(error))
  } finally {
    refreshConfigBusy.value = false
  }
}

async function init() {
  await loadPools()
  await loadItems()
}

void init()

onBeforeUnmount(() => {
  if (searchTimer) window.clearTimeout(searchTimer)
  clearReveals()
})
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Data Pools"
      title="数据池"
      subtitle="参数与 Token 池统一管理：分页、状态筛选、启停与健康度；明文需二次确认且 30 秒后自动隐藏"
    >
      <a-button type="primary" @click="router.push('/stress')">
        <template #icon><ThunderboltOutlined /></template>
        去压测绑定
      </a-button>
    </PageHeader>

    <div class="pools-layout">
      <!-- 左侧：池列表与新建 -->
      <aside class="pane pool-side">
        <div class="pane-head">
          <span>数据池</span>
          <span class="pane-count">{{ pools.length }}</span>
        </div>

        <div class="create-box">
          <a-button type="primary" size="small" block @click="openCreatePool">
            <template #icon><PlusOutlined /></template>
            新建数据池
          </a-button>
        </div>

        <a-input
          v-model:value="poolSearch"
          size="small"
          allow-clear
          placeholder="搜索池名称"
          class="pool-search"
        />

        <a-spin :spinning="poolsLoading">
          <div class="pool-list">
            <div
              v-for="pool in filteredPools"
              :key="pool.id"
              class="pool-list-item"
              :class="{ active: pool.id === activePoolId }"
              @click="activePoolId = pool.id"
            >
              <div class="pool-list-main">
                <span class="pool-list-name">{{ pool.name }}</span>
                <a-tag :color="pool.kind === 'token' ? 'blue' : 'default'" class="tiny-tag">
                  {{ pool.kind === 'token' ? 'Token' : '参数' }}
                </a-tag>
                <a-tag v-if="pool.sensitive" color="orange" class="tiny-tag">敏感</a-tag>
              </div>
              <div class="pool-list-meta">
                <span>{{ pool.itemCount }} 条</span>
                <span class="dot-ok">可用 {{ pool.activeCount ?? 0 }}</span>
                <span v-if="(pool.invalidCount ?? 0) > 0" class="dot-bad">
                  失效 {{ pool.invalidCount }}
                </span>
                <a-button size="small" type="link" danger @click.stop="deletePool(pool)">
                  删除
                </a-button>
              </div>
            </div>
            <div v-if="!filteredPools.length" class="empty">还没有池，先在上方新建一个</div>
          </div>
        </a-spin>
      </aside>

      <!-- 右侧：条目详情 -->
      <section class="pane pool-detail">
        <template v-if="activePoolId">
          <div class="detail-head">
            <div>
              <div class="detail-title">{{ activePool?.name }}</div>
              <div class="detail-sub">
                {{ activePool?.kind === 'token' ? 'Token 池' : '参数池' }} ·
                {{ activePool?.sensitive ? '默认脱敏' : '可直接查看' }}
              </div>
              <div class="detail-columns">
                <span class="detail-columns-label">列：</span>
                <a-tag v-for="col in activePool?.columns ?? []" :key="col" class="tiny-tag">
                  {{ col }}
                </a-tag>
              </div>
            </div>
            <div class="detail-actions">
              <a-button size="small" @click="openColumnsEdit">编辑列</a-button>
              <a-button
                v-if="activePool?.kind === 'token'"
                size="small"
                :loading="refreshConfigBusy"
                @click="openRefreshConfig"
              >
                Token 刷新
              </a-button>
              <a-button size="small" :loading="refreshBusy" @click="refreshHealth">
                <template #icon><ReloadOutlined /></template>
                刷新健康度
              </a-button>
              <a-button size="small" danger @click="clearItems">清空条目</a-button>
            </div>
          </div>

          <div class="health-row" v-if="pageData">
            <button
              class="health-chip"
              :class="{ active: statusFilter === '' }"
              @click="statusFilter = ''"
            >
              全部 <b>{{ pageData.health.total }}</b>
            </button>
            <button
              class="health-chip ok"
              :class="{ active: statusFilter === 'active' }"
              @click="statusFilter = 'active'"
            >
              可用 <b>{{ pageData.health.active }}</b>
            </button>
            <button
              class="health-chip warn"
              :class="{ active: statusFilter === 'cooling' }"
              @click="statusFilter = 'cooling'"
            >
              冷却 <b>{{ pageData.health.cooling }}</b>
            </button>
            <button
              class="health-chip bad"
              :class="{ active: statusFilter === 'invalid' }"
              @click="statusFilter = 'invalid'"
            >
              失效 <b>{{ pageData.health.invalid }}</b>
            </button>
            <button
              class="health-chip bad"
              :class="{ active: statusFilter === 'expired' }"
              @click="statusFilter = 'expired'"
            >
              过期 <b>{{ pageData.health.expired }}</b>
            </button>
            <button
              class="health-chip muted"
              :class="{ active: statusFilter === 'disabled' }"
              @click="statusFilter = 'disabled'"
            >
              停用 <b>{{ pageData.health.disabled }}</b>
            </button>
          </div>

          <div class="toolbar">
            <a-input
              v-model:value="itemSearch"
              size="small"
              allow-clear
              placeholder="搜索条目（任意列）"
              class="item-search"
            />
            <a-select v-model:value="statusFilter" size="small" class="status-select">
              <a-select-option v-for="option in statusOptions" :key="option.value" :value="option.value">
                {{ option.label }}
              </a-select-option>
            </a-select>
          </div>

          <a-table
            :data-source="pageData?.items ?? []"
            :loading="itemsLoading"
            row-key="id"
            size="small"
            :pagination="false"
            :scroll="{ x: itemTableScrollX }"
            class="item-table"
          >
            <a-table-column title="#" key="index" :width="64">
              <template #default="{ record }">{{ record.index }}</template>
            </a-table-column>
            <a-table-column
              v-for="(col, colIndex) in activePool?.columns ?? []"
              :key="`col-${col}`"
              :title="col"
            >
              <template #default="{ record }">
                <span class="mono item-value">
                  {{ revealed[record.id]?.[col] ?? record.data?.[col] ?? '—' }}
                </span>
                <a-tag
                  v-if="colIndex === 0 && revealed[record.id]"
                  color="red"
                  class="tiny-tag"
                >
                  明文 {{ revealLeft[record.id] }}s
                </a-tag>
              </template>
            </a-table-column>
            <a-table-column title="状态" key="status" :width="96">
              <template #default="{ record }">
                <a-tag :color="statusColor(record.status)" class="tiny-tag">
                  {{ statusLabel(record.status) }}
                </a-tag>
              </template>
            </a-table-column>
            <a-table-column title="权重" key="weight" :width="72">
              <template #default="{ record }">{{ record.weight }}</template>
            </a-table-column>
            <a-table-column title="失败" key="fail" :width="64">
              <template #default="{ record }">{{ record.failCount }}</template>
            </a-table-column>
            <a-table-column title="最近使用" key="lastUsed" :width="110">
              <template #default="{ record }">{{ fmtTime(record.lastUsedAt) }}</template>
            </a-table-column>
            <a-table-column title="操作" key="actions" :width="190">
              <template #default="{ record }">
                <a-button
                  size="small"
                  type="link"
                  @click="revealItem(record)"
                >
                  <template #icon>
                    <EyeInvisibleOutlined v-if="revealed[record.id]" />
                    <EyeOutlined v-else />
                  </template>
                  {{ revealed[record.id] ? '隐藏' : '显示明文' }}
                </a-button>
                <a-button size="small" type="link" @click="toggleItem(record)">
                  {{ record.enabled ? '停用' : '启用' }}
                </a-button>
                <a-button size="small" type="link" danger @click="removeItem(record)">
                  <template #icon><DeleteOutlined /></template>
                </a-button>
              </template>
            </a-table-column>
          </a-table>

          <div class="pager">
            <a-pagination
              :current="page"
              :page-size="pageSize"
              :total="pageData?.total ?? 0"
              :show-size-changer="true"
              :page-size-options="['20', '50', '100', '200']"
              size="small"
              @change="onPageChange"
            />
          </div>

          <div class="import-box">
            <div class="import-head">批量导入</div>
            <div class="import-actions">
              <a-button size="small" :loading="excelBusy" @click="downloadTemplate">
                下载 Excel 模板
              </a-button>
              <a-button size="small" type="primary" :loading="excelBusy" @click="importExcel">
                导入 Excel
              </a-button>
            </div>
            <div class="hint">Excel 表头必须与列名一致，首列必填。</div>

            <a-collapse ghost class="import-collapse">
              <a-collapse-panel key="text" header="文本导入">
                <div class="hint">
                  列顺序：{{ (activePool?.columns ?? []).join(',') }}；首行可为表头
                </div>
                <a-textarea
                  v-model:value="importText"
                  :rows="4"
                  :disabled="importBusy"
                  placeholder="每行一条，按列顺序用逗号分隔；也可粘贴 JSON 数组，键名与列名一致"
                />
                <div class="import-foot">
                  <span class="hint"># 开头的行会被忽略，权重留空按 1 处理</span>
                  <a-button size="small" type="primary" :loading="importBusy" @click="importItems">
                    导入
                  </a-button>
                </div>
              </a-collapse-panel>
            </a-collapse>
          </div>
        </template>
        <div v-else class="empty detail-empty">选择左侧的数据池查看条目</div>
      </section>
    </div>

    <a-modal
      v-model:open="refreshOpen"
      title="Token 刷新配置"
      :confirm-loading="refreshConfigBusy"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveRefreshConfig"
    >
      <div class="refresh-form">
        <div class="field">
          <div class="label">刷新方式</div>
          <a-select v-model:value="refreshForm.mode" size="small" style="width: 100%">
            <a-select-option value="none">不刷新（仅 401 切下一条）</a-select-option>
            <a-select-option value="http">HTTP 刷新接口</a-select-option>
          </a-select>
        </div>
        <template v-if="refreshForm.mode === 'http'">
          <div class="field-row">
            <div class="field" style="width: 120px">
              <div class="label">方法</div>
              <a-select v-model:value="refreshForm.method" size="small" style="width: 100%">
                <a-select-option value="POST">POST</a-select-option>
                <a-select-option value="GET">GET</a-select-option>
              </a-select>
            </div>
            <div class="field" style="flex: 1">
              <div class="label">刷新地址</div>
              <a-input v-model:value="refreshForm.url" size="small" placeholder="https://api.example.com/oauth/token" />
            </div>
          </div>
          <div class="field">
            <div class="label">请求头（每行 Key: Value）</div>
            <a-textarea v-model:value="refreshForm.headersText" :rows="2" size="small" placeholder="Content-Type: application/json" />
          </div>
          <div class="field">
            <div class="label">请求体</div>
            <a-textarea
              v-model:value="refreshForm.body"
              :rows="3"
              size="small"
              placeholder='例如 {"grant_type":"refresh_token","refresh_token":"{{refreshToken}}"}'
            />
            <div class="hint">支持 &#123;&#123;token&#125;&#125; / &#123;&#123;refreshToken&#125;&#125; / &#123;&#123;account&#125;&#125; 占位符</div>
          </div>
          <div class="field-row">
            <div class="field">
              <div class="label">Token 路径</div>
              <a-input v-model:value="refreshForm.tokenPath" size="small" placeholder="$.data.accessToken" />
            </div>
            <div class="field">
              <div class="label">过期时间路径</div>
              <a-input v-model:value="refreshForm.expiresPath" size="small" placeholder="$.data.expiresIn" />
            </div>
          </div>
          <div class="field">
            <div class="label">兜底有效期（秒，0 = 不过期）</div>
            <a-input-number v-model:value="refreshForm.expiresInSec" :min="0" :max="86400" size="small" style="width: 100%" />
          </div>
        </template>
      </div>
    </a-modal>

    <a-modal
      v-model:open="createOpen"
      title="新建数据池"
      :confirm-loading="createBusy"
      ok-text="创建"
      cancel-text="取消"
      @ok="createPool"
    >
      <div class="refresh-form">
        <div class="field">
          <div class="label">名称</div>
          <a-input
            v-model:value="newPoolName"
            size="small"
            placeholder="例如：测试账号池"
            @press-enter="createPool"
          />
        </div>
        <div class="field-row">
          <div class="field" style="flex: 1">
            <div class="label">类型</div>
            <a-select v-model:value="newPoolKind" size="small" style="width: 100%">
              <a-select-option value="token">Token 池</a-select-option>
              <a-select-option value="parameter">参数池</a-select-option>
            </a-select>
          </div>
          <div class="field" style="width: 130px">
            <div class="label">敏感</div>
            <a-checkbox v-model:checked="newPoolSensitive">默认脱敏</a-checkbox>
          </div>
        </div>
        <div class="field">
          <div class="label">列定义（顺序即展示顺序）</div>
          <div class="column-editor">
            <div v-for="(_, index) in newPoolColumns" :key="index" class="column-row">
              <span class="column-index">{{ index + 1 }}</span>
              <a-input v-model:value="newPoolColumns[index]" size="small" placeholder="列名" />
              <a-button
                size="small"
                type="text"
                title="上移"
                :disabled="index === 0"
                @click="moveCreateColumn(index, -1)"
              >
                <template #icon><ArrowUpOutlined /></template>
              </a-button>
              <a-button
                size="small"
                type="text"
                title="下移"
                :disabled="index === newPoolColumns.length - 1"
                @click="moveCreateColumn(index, 1)"
              >
                <template #icon><ArrowDownOutlined /></template>
              </a-button>
              <a-button
                size="small"
                type="text"
                danger
                title="删除"
                @click="removeCreateColumn(index)"
              >
                <template #icon><DeleteOutlined /></template>
              </a-button>
            </div>
            <a-button size="small" type="dashed" block @click="addCreateColumn">
              <template #icon><PlusOutlined /></template>
              新增列
            </a-button>
          </div>
          <div class="hint">切换类型会重置为默认列；首列是 value 的镜像。</div>
        </div>
      </div>
    </a-modal>

    <a-modal
      v-model:open="columnsOpen"
      title="编辑列定义"
      :confirm-loading="columnsBusy"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveColumns"
    >
      <div class="column-editor">
        <div
          v-for="(row, index) in columnRows"
          :key="`${row.original || 'new'}-${index}`"
          class="column-row"
        >
          <span class="column-index">{{ index + 1 }}</span>
          <a-input v-model:value="row.name" size="small" placeholder="列名" />
          <a-tag v-if="!row.original" class="tiny-tag">新增</a-tag>
          <a-button
            size="small"
            type="text"
            title="上移"
            :disabled="index === 0"
            @click="moveColumnRow(index, -1)"
          >
            <template #icon><ArrowUpOutlined /></template>
          </a-button>
          <a-button
            size="small"
            type="text"
            title="下移"
            :disabled="index === columnRows.length - 1"
            @click="moveColumnRow(index, 1)"
          >
            <template #icon><ArrowDownOutlined /></template>
          </a-button>
          <a-button size="small" type="text" danger title="删除" @click="removeColumnRow(index)">
            <template #icon><DeleteOutlined /></template>
          </a-button>
        </div>
        <a-button size="small" type="dashed" block @click="addColumnRow">
          <template #icon><PlusOutlined /></template>
          新增列
        </a-button>
      </div>
      <div class="hint">
        改名按旧列名迁移数据；删除列会丢弃该列已有数据；新增列只提交列名。
      </div>
    </a-modal>
  </div>
</template>

<style scoped>
.pools-layout {
  display: grid;
  grid-template-columns: 300px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}

.pane {
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-lg, 10px);
  overflow: hidden;
}

.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 44px;
  padding: 0 14px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 13px;
  font-weight: 600;
}

.pane-count {
  font-size: 11px;
  color: var(--fm-muted);
}

.pool-side {
  position: sticky;
  top: 20px;
  max-height: calc(100vh - 40px);
  display: flex;
  flex-direction: column;
}

.create-box {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  padding: 12px 12px 8px;
  border-bottom: 1px dashed var(--fm-border);
}

.pool-search {
  margin: 10px 12px 4px;
  width: auto;
}

.pool-list {
  overflow-y: auto;
  padding: 6px 8px 12px;
}

.pool-list-item {
  padding: 9px 10px;
  border-radius: var(--fm-radius);
  cursor: pointer;
  border: 1px solid transparent;
}

.pool-list-item:hover {
  background: var(--fm-surface-alt);
}

.pool-list-item.active {
  background: var(--fm-primary-soft);
  border-color: var(--fm-primary);
}

.pool-list-main {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.pool-list-name {
  font-size: 12.5px;
  font-weight: 600;
}

.pool-list-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 5px;
  font-size: 11px;
  color: var(--fm-muted);
}

.dot-ok {
  color: var(--fm-success, #17774a);
}

.dot-bad {
  color: var(--fm-danger, #a23333);
}

.tiny-tag {
  font-size: 10px;
  line-height: 16px;
  padding: 0 5px;
  margin: 0;
}

.pool-detail {
  padding: 14px 16px 18px;
}

.detail-head {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.detail-head > div:first-child {
  min-width: 0;
}

.detail-title {
  font-size: 15px;
  font-weight: 600;
}

.detail-sub {
  font-size: 11px;
  color: var(--fm-muted);
  margin-top: 3px;
}

.detail-columns {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}

.detail-columns-label {
  font-size: 11px;
  color: var(--fm-muted);
}

/* 长列名不撑破详情头部，超长时省略 */
.detail-columns :deep(.ant-tag) {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.health-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
}

.health-chip {
  border: 1px solid var(--fm-border);
  background: var(--fm-surface);
  color: var(--fm-text-2);
  border-radius: 999px;
  padding: 3px 12px;
  font-size: 11.5px;
  cursor: pointer;
}

.health-chip b {
  margin-left: 4px;
  font-variant-numeric: tabular-nums;
}

.health-chip.active {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.health-chip.ok b {
  color: var(--fm-success, #17774a);
}

.health-chip.warn b {
  color: #9c6414;
}

.health-chip.bad b {
  color: var(--fm-danger, #a23333);
}

.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-bottom: 10px;
}

.item-search {
  flex: 1 1 180px;
  max-width: 260px;
}

.status-select {
  width: 120px;
}

.item-value {
  font-size: 11.5px;
  word-break: break-all;
}

.pager {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
}

.import-box {
  margin-top: 16px;
  padding-top: 14px;
  border-top: 1px dashed var(--fm-border);
}

.import-head {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 8px;
}

.import-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 6px;
}

.import-collapse {
  margin-top: 6px;
}

.column-editor {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.column-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.column-row > .ant-input {
  flex: 1;
  min-width: 0;
}

/* 序号 / 标签 / 操作按钮不参与收缩，避免小屏下被压扁 */
.column-row > :deep(.ant-btn),
.column-row > :deep(.ant-tag) {
  flex: none;
}

.column-index {
  width: 16px;
  flex: none;
  text-align: right;
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.import-foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 8px;
}

.hint {
  font-size: 11px;
  color: var(--fm-muted);
  overflow-wrap: anywhere;
}

.empty {
  padding: 18px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--fm-muted);
}

.detail-empty {
  padding: 60px 0;
}

/* 窄屏下左右并排会挤压表格，改为上下堆叠 */
@media (max-width: 1080px) {
  .pools-layout {
    grid-template-columns: minmax(0, 1fr);
  }

  .pool-side {
    position: static;
    max-height: none;
  }
}
</style>
