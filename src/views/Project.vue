<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeftOutlined,
  PlusOutlined,
  FolderOutlined,
  SettingOutlined,
  FileTextOutlined,
  ImportOutlined,
  ExportOutlined,
  SearchOutlined,
  DownOutlined,
  SafetyCertificateOutlined,
  GlobalOutlined,
  ApiOutlined,
} from '@ant-design/icons-vue'
import AiSparkIcon from '@/components/AiSparkIcon.vue'
import DocsPreviewDrawer from '@/components/DocsPreviewDrawer.vue'
import DocsSiteModal from '@/components/DocsSiteModal.vue'
import ContractPanelDrawer from '@/components/ContractPanelDrawer.vue'
import MockPanel from '@/components/MockPanel.vue'
import type { DocScope } from '@/types/docs'
import { useProjectStore } from '@/stores/project'
import { useAiAssistStore } from '@/stores/aiAssist'
import { projectAuditPrompt } from '@/utils/aiScenarios'
import { MAX_REQUEST_TABS, useTabsStore } from '@/stores/tabs'
import { formatDateTime } from '@/utils/datetime'
import {
  PROJECT_PAGE_THRESHOLD,
  pageOfIndex,
  projectPagination,
} from '@/utils/tablePaging'
import { buildCurl } from '@/utils/curl'
import {
  forgetLastRequest,
  readLastRequest,
  rememberLastRequest,
  type LastRequestMemory,
} from '@/utils/lastRequest'
import * as api from '@/api'
import type { CollectionNode, RequestSearchHit, RequestSummary } from '@/types/models'

interface TreeNode {
  key: string
  title: string
  children?: TreeNode[]
  raw: CollectionNode
}

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const tabs = useTabsStore()

const projectId = computed(() => Number(route.params.id))
const selectedKeys = ref<string[]>([])
const selectedCollection = computed(() => {
  if (!selectedKeys.value.length) return null
  return findNode(store.tree, Number(selectedKeys.value[0]))
})
const requests = ref<RequestSummary[]>([])
const loadingRequests = ref(false)
const expandedKeys = ref<string[]>([])
const tableWrap = ref<HTMLElement | null>(null)
/** 上次打开的接口：返回项目页时自动定位到它的集合并高亮 */
const lastRequest = ref<LastRequestMemory | null>(null)

const aiAssist = useAiAssistStore()

async function aiAudit() {
  try {
    const items = await api.requestOptions(projectId.value)
    void aiAssist.show({
      title: '项目结构体检',
      prompt: projectAuditPrompt(store.currentProject?.name ?? '当前项目', store.tree, items),
      source: 'project',
      hint: `${items.length} 个接口 · ${store.tree.length} 个顶层集合`,
    })
  } catch (e) {
    message.error(String(e))
  }
}

const keyword = ref('')
const searchHits = ref<RequestSearchHit[]>([])
const searching = ref(false)
const searchMode = computed(() => keyword.value.trim().length > 0)
const displayRows = computed<Array<RequestSummary | RequestSearchHit>>(() =>
  searchMode.value ? searchHits.value : requests.value,
)

/** 接口数超过阈值才分页；用户可手动全部展开（P-03） */
const currentPage = ref(1)
const showAllRows = ref(false)
const pagination = computed(() => {
  const base = projectPagination(displayRows.value.length, showAllRows.value)
  if (!base) return false
  return {
    ...base,
    current: currentPage.value,
    onChange: (page: number) => {
      currentPage.value = page
    },
  }
})
watch(
  () => displayRows.value.length,
  () => {
    currentPage.value = 1
  },
)

let searchTimer: number | undefined
watch(keyword, (value) => {
  if (searchTimer) window.clearTimeout(searchTimer)
  if (!value.trim()) {
    searchHits.value = []
    searching.value = false
    return
  }
  searching.value = true
  searchTimer = window.setTimeout(() => runSearch(), 220)
})

async function runSearch() {
  const word = keyword.value.trim()
  if (!word) return
  currentPage.value = 1
  showAllRows.value = false
  searching.value = true
  try {
    searchHits.value = await api.requestSearch(projectId.value, word)
  } catch (e) {
    message.error(String(e))
  } finally {
    searching.value = false
  }
}

function clearSearch() {
  keyword.value = ''
}

const collectionModalOpen = ref(false)
const collectionName = ref('')
const collectionParent = ref<CollectionNode | null>(null)
const editingCollection = ref<CollectionNode | null>(null)

function findNode(nodes: CollectionNode[], id: number): CollectionNode | null {
  for (const n of nodes) {
    if (n.id === id) return n
    const found = findNode(n.children, id)
    if (found) return found
  }
  return null
}

function toTreeData(nodes: CollectionNode[]): TreeNode[] {
  return nodes.map((n) => ({
    key: String(n.id),
    title: n.name,
    raw: n,
    children: n.children.length ? toTreeData(n.children) : undefined,
  }))
}

const treeData = computed(() => toTreeData(store.tree))

onMounted(async () => {
  tabs.ensureProject(projectId.value)
  try {
    await store.openProject(projectId.value)
    store.syncActiveEnvironment()
    await restoreSelection()
  } catch (e) {
    message.error(String(e))
  }
})

function ancestorsOf(nodes: CollectionNode[], id: number, path: number[] = []): number[] | null {
  for (const node of nodes) {
    if (node.id === id) return path
    const found = ancestorsOf(node.children, id, [...path, node.id])
    if (found) return found
  }
  return null
}

/** 优先恢复上次打开的接口，其次选第一个集合 */
async function restoreSelection() {
  if (!store.tree.length) return
  const memory = readLastRequest(projectId.value)
  lastRequest.value = memory
  if (memory && findNode(store.tree, memory.collectionId)) {
    selectedKeys.value = [String(memory.collectionId)]
    expandedKeys.value = (ancestorsOf(store.tree, memory.collectionId) ?? []).map(String)
    await loadRequests(memory.collectionId)
    if (requests.value.some((item) => item.id === memory.id)) {
      await scrollToRequest(memory.id)
      return
    }
    // 接口已被删除或移出该集合，记忆作废
    lastRequest.value = null
    forgetLastRequest(projectId.value, memory.id)
    return
  }
  if (memory) forgetLastRequest(projectId.value, memory.id)
  const first = store.tree[0]
  selectedKeys.value = [String(first.id)]
  expandedKeys.value = (ancestorsOf(store.tree, first.id) ?? []).map(String)
  await loadRequests(first.id)
}

async function scrollToRequest(id: number) {
  // 分页时先切到目标行所在页，否则 DOM 里根本找不到这一行
  const index = displayRows.value.findIndex((row) => row.id === id)
  const page = projectPagination(displayRows.value.length, showAllRows.value)
  if (index >= 0 && page) currentPage.value = pageOfIndex(index, page.pageSize)
  await nextTick()
  const row = tableWrap.value?.querySelector<HTMLElement>(`[data-row-key="${id}"]`)
  row?.scrollIntoView({ block: 'nearest' })
}

function onExpand(keys: Array<string | number>) {
  expandedKeys.value = keys.map(String)
}

function rowClassName(record: RequestSummary | RequestSearchHit) {
  return lastRequest.value && record.id === lastRequest.value.id ? 'last-opened-row' : ''
}

async function onSelect(keys: Array<string | number>) {
  const key = keys[0]
  if (key === undefined) return
  selectedKeys.value = [String(key)]
  currentPage.value = 1
  showAllRows.value = false
  await loadRequests(Number(key))
}

async function loadRequests(collectionId: number) {
  currentPage.value = 1
  loadingRequests.value = true
  try {
    requests.value = await api.requestList(collectionId)
  } catch (e) {
    message.error(String(e))
  } finally {
    loadingRequests.value = false
  }
}

async function addCollection(parent?: CollectionNode | null) {
  collectionParent.value = parent ?? null
  editingCollection.value = null
  collectionName.value = ''
  collectionModalOpen.value = true
}

function renameCollection(node: CollectionNode) {
  collectionParent.value = null
  editingCollection.value = node
  collectionName.value = node.name
  collectionModalOpen.value = true
}

async function submitCollection() {
  const name = collectionName.value.trim()
  if (!name) {
    message.warning('名称不能为空')
    return
  }
  try {
    if (editingCollection.value) {
      await api.collectionRename(editingCollection.value.id, name)
    } else {
      await api.collectionCreate(projectId.value, collectionParent.value?.id ?? null, name)
    }
    collectionModalOpen.value = false
    await store.refreshTree(projectId.value)
    message.success(editingCollection.value ? '已重命名' : '已创建')
  } catch (e) {
    message.error(String(e))
  }
}

function deleteCollection(node: CollectionNode) {
  Modal.confirm({
    title: `删除集合「${node.name}」？`,
    content: '集合下的请求会一并删除。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.collectionDelete(node.id)
      await store.refreshTree(projectId.value)
      requests.value = []
      selectedKeys.value = []
      if (lastRequest.value && subtreeHas(node, lastRequest.value.collectionId)) {
        lastRequest.value = null
        forgetLastRequest(projectId.value)
      }
    },
  })
}

function subtreeHas(node: CollectionNode, id: number) {
  return node.id === id || !!findNode(node.children, id)
}

async function addRequest(collection: CollectionNode) {
  const payload = {
    collectionId: collection.id,
    name: '新建请求',
    method: 'GET',
    url: '{{base_url}}/',
    headers: [],
    params: [],
    bodyType: 'none',
    body: '',
    preScript: '',
    postScript: '',
    assertions: '',
  }
  try {
    const saved = await api.requestSave(payload)
    await loadRequests(collection.id)
    openRequestTab(saved.id!, {
      collectionId: collection.id,
      name: payload.name,
      method: payload.method,
    })
  } catch (e) {
    message.error(String(e))
  }
}

function openRequestTab(
  id: number,
  meta?: { collectionId?: number; name?: string; method?: string },
) {
  tabs.ensureProject(projectId.value)
  if (!tabs.canOpen(id)) {
    message.warning(`最多同时打开 ${MAX_REQUEST_TABS} 个接口，请先关闭部分标签`)
    return
  }
  rememberLastRequest(projectId.value, { id, ...meta })
  lastRequest.value = readLastRequest(projectId.value)
  router.push({ name: 'request', params: { id }, query: { projectId: projectId.value } })
}

function openRequest(row: RequestSummary | RequestSearchHit) {
  openRequestTab(row.id, {
    collectionId: row.collectionId,
    name: row.name,
    method: row.method,
  })
}

async function copyRowCurl(row: RequestSummary | RequestSearchHit) {
  try {
    const detail = await api.requestGet(row.id)
    await navigator.clipboard.writeText(
      buildCurl(detail, store.activeEnvironment?.variables),
    )
    message.success(`已复制「${row.name}」的 cURL`)
  } catch (e) {
    message.error(String(e))
  }
}

function deleteRequest(row: RequestSummary | RequestSearchHit) {
  Modal.confirm({
    title: `删除请求「${row.name}」？`,
    content: '该请求及其配置将被永久删除，无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.requestDelete(row.id)
      tabs.ensureProject(projectId.value)
      tabs.close(row.id)
      if (lastRequest.value?.id === row.id) {
        lastRequest.value = null
        forgetLastRequest(projectId.value, row.id)
      }
      message.success('已删除请求')
      if (searchMode.value) await runSearch()
      else if (selectedKeys.value.length) await loadRequests(Number(selectedKeys.value[0]))
    },
  })
}

function switchEnv(value: number) {
  api
    .envActivate(projectId.value, value)
    .then(() => store.refreshEnvironments(projectId.value))
    .then(() => store.syncActiveEnvironment())
    .catch((e) => message.error(String(e)))
}

const curlOpen = ref(false)
const curlText = ref('')
const curlLoading = ref(false)
const curlTarget = ref<number | null>(null)

const collectionOptions = computed(() => {
  const options: { label: string; value: number }[] = []
  const walk = (nodes: CollectionNode[], depth: number) => {
    for (const node of nodes) {
      options.push({ label: `${'　'.repeat(depth)}${node.name}`, value: node.id })
      walk(node.children, depth + 1)
    }
  }
  walk(store.tree, 0)
  return options
})

function openCurl() {
  curlText.value = ''
  curlTarget.value = selectedCollection.value?.id ?? null
  curlOpen.value = true
}

async function submitCurl() {
  if (!curlText.value.trim()) {
    message.warning('请先粘贴 cURL 命令')
    return
  }
  curlLoading.value = true
  try {
    const parsed = await api.curlParse(curlText.value)
    let collectionId = curlTarget.value
    if (!collectionId) {
      const created = await api.collectionCreate(projectId.value, null, 'cURL 导入')
      collectionId = created.id
      await store.refreshTree(projectId.value)
      selectedKeys.value = [String(collectionId)]
    }
    const saved = await api.requestSave({
      collectionId,
      name: parsed.name,
      method: parsed.method,
      url: parsed.url,
      headers: parsed.headers,
      params: parsed.params,
      bodyType: parsed.bodyType,
      body: parsed.body,
      preScript: '',
      postScript: '',
      assertions: '',
    })
    curlOpen.value = false
    if (parsed.warnings.length) {
      message.warning(parsed.warnings.join('；'))
    }
    await loadRequests(collectionId)
    message.success('已导入请求')
    openRequestTab(saved.id!, {
      collectionId,
      name: parsed.name,
      method: parsed.method,
    })
  } catch (e) {
    message.error(String(e))
  } finally {
    curlLoading.value = false
  }
}

async function exportProject() {
  const safeName = (store.currentProject?.name || 'astroman-project').replace(/[\\/:*?"<>|]/g, '_')
  try {
    const path = await save({
      title: '导出项目 JSON',
      defaultPath: `${safeName}.astroman.json`,
      filters: [{ name: 'AstroMan 项目文件', extensions: ['json'] }],
    })
    if (!path) return
    await api.projectExport(projectId.value, path)
    message.success('项目已导出，可通过「导入项目」恢复')
  } catch (e) {
    message.error(String(e))
  }
}

const importingApi = ref(false)

/* ---------- 接口文档导出 ---------- */
const docsOpen = ref(false)
const docsScope = ref<DocScope>('project')
const docsTargetId = ref(0)

function openDocs(scope: DocScope, targetId: number) {
  docsScope.value = scope
  docsTargetId.value = targetId
  docsOpen.value = true
}

/* ---------- 契约基线 ---------- */
const contractOpen = ref(false)

/* ---------- 只读文档站 ---------- */
const docsSiteOpen = ref(false)

/* ---------- 拖拽排序 ---------- */
const dragCollectionId = ref<number | null>(null)
const dragRequestId = ref<number | null>(null)

function siblingsOf(node: CollectionNode): CollectionNode[] {
  if (!node.parentId) return store.tree
  return findNode(store.tree, node.parentId)?.children ?? []
}

function onCollectionDragStart(node: CollectionNode) {
  dragCollectionId.value = node.id
}

async function onCollectionDrop(target: CollectionNode) {
  const sourceId = dragCollectionId.value
  dragCollectionId.value = null
  if (!sourceId || sourceId === target.id) return
  const source = findNode(store.tree, sourceId)
  if (!source) return
  if ((source.parentId ?? null) !== (target.parentId ?? null)) {
    message.warning('暂只支持同级集合排序')
    return
  }
  const ids = siblingsOf(target).map((item) => item.id)
  const from = ids.indexOf(sourceId)
  const to = ids.indexOf(target.id)
  if (from < 0 || to < 0) return
  ids.splice(to, 0, ids.splice(from, 1)[0])
  try {
    await api.collectionReorder(projectId.value, target.parentId ?? null, ids)
    await store.refreshTree(projectId.value)
  } catch (e) {
    message.error(String(e))
  }
}

function onRequestDragStart(record: RequestSummary) {
  dragRequestId.value = record.id
}

async function onRequestDrop(target: RequestSummary) {
  const sourceId = dragRequestId.value
  dragRequestId.value = null
  if (!sourceId || sourceId === target.id || searchMode.value) return
  const collectionId = selectedCollection.value?.id ?? target.collectionId
  const ids = requests.value.map((item) => item.id)
  const from = ids.indexOf(sourceId)
  const to = ids.indexOf(target.id)
  if (from < 0 || to < 0) return
  ids.splice(to, 0, ids.splice(from, 1)[0])
  try {
    await api.requestReorder(collectionId, ids)
    await loadRequests(collectionId)
  } catch (e) {
    message.error(String(e))
  }
}

async function exportOpenapi() {
  const safeName = (store.currentProject?.name || 'openapi').replace(/[\\/:*?"<>|]/g, '_')
  try {
    const path = await save({
      title: '导出 OpenAPI 3.0',
      defaultPath: `${safeName}.openapi.json`,
      filters: [{ name: 'OpenAPI JSON', extensions: ['json'] }],
    })
    if (!path) return
    await api.openapiExport(projectId.value, path)
    message.success('已导出 OpenAPI 3.0 文档，可导入 Apifox / Swagger 等工具')
  } catch (e) {
    message.error(String(e))
  }
}

async function importOpenapiFile() {
  try {
    const path = await open({
      title: '导入 OpenAPI / Swagger 文档',
      multiple: false,
      filters: [{ name: 'OpenAPI JSON', extensions: ['json'] }],
    })
    if (!path) return
    importingApi.value = true
    const project = await api.openapiImport(path, projectId.value)
    await store.refreshTree(project.id)
    requests.value = []
    selectedKeys.value = []
    if (store.tree.length) {
      selectedKeys.value = [String(store.tree[0].id)]
      await loadRequests(store.tree[0].id)
    }
    message.success(`已按 tag 导入到项目「${project.name}」`)
  } catch (e) {
    message.error(String(e))
  } finally {
    importingApi.value = false
  }
}

async function importApiFile() {
  try {
    const path = await open({
      title: '导入 Postman / Apifox / HAR 数据',
      multiple: false,
      filters: [{ name: 'JSON / HAR 文件', extensions: ['json', 'har'] }],
    })
    if (!path) return
    importingApi.value = true
    await api.apiImport(path, projectId.value)
    await store.refreshTree(projectId.value)
    await store.refreshEnvironments(projectId.value)
    store.syncActiveEnvironment()
    requests.value = []
    selectedKeys.value = []
    if (store.tree.length) {
      selectedKeys.value = [String(store.tree[0].id)]
      await loadRequests(store.tree[0].id)
    }
    message.success('接口已导入到当前项目')
    if (path.toLowerCase().endsWith('.har')) {
      message.info('HAR 中 Authorization / Cookie 已默认停用，上传文件字段与 base64 请求体未导入', 6)
    }
  } catch (e) {
    message.error(String(e))
  } finally {
    importingApi.value = false
  }
}

const mockOpen = ref(false)

/* ---------- W2：导入导出补齐（G-07） ---------- */
const openapiUrlOpen = ref(false)
const openapiUrlBusy = ref(false)
const openapiUrlForm = ref({ url: '', token: '', save: true })

async function importBruno() {
  try {
    const path = await open({
      title: '选择 Bruno 工作目录（包含 .bru 文件）',
      directory: true,
      multiple: false,
    })
    if (!path) return
    importingApi.value = true
    const project = await api.brunoImport(String(path), projectId.value)
    await store.refreshTree(project.id)
    requests.value = []
    selectedKeys.value = []
    if (store.tree.length) {
      selectedKeys.value = [String(store.tree[0].id)]
      await loadRequests(store.tree[0].id)
    }
    message.success(`已从 Bruno 目录导入到项目「${project.name}」`)
  } catch (e) {
    message.error(String(e))
  } finally {
    importingApi.value = false
  }
}

function openOpenapiUrl() {
  openapiUrlForm.value = { url: '', token: '', save: true }
  openapiUrlOpen.value = true
}

async function submitOpenapiUrl() {
  const form = openapiUrlForm.value
  const url = form.url.trim()
  if (!url) {
    message.warning('请填写 OpenAPI / Swagger JSON 地址')
    return
  }
  const token = form.token.trim()
  const headers = token
    ? [{ key: 'Authorization', value: /^Bearer\s/i.test(token) ? token : `Bearer ${token}`, enabled: true }]
    : []
  openapiUrlBusy.value = true
  try {
    const project = await api.openapiImportUrl({
      url,
      headers,
      projectId: projectId.value,
      save: form.save,
    })
    await store.refreshTree(project.id)
    requests.value = []
    selectedKeys.value = []
    if (store.tree.length) {
      selectedKeys.value = [String(store.tree[0].id)]
      await loadRequests(store.tree[0].id)
    }
    openapiUrlOpen.value = false
    message.success(`已从 URL 导入到项目「${project.name}」`)
  } catch (e) {
    message.error(String(e))
  } finally {
    openapiUrlBusy.value = false
  }
}

async function exportPostman() {
  const safeName = (store.currentProject?.name || 'collection').replace(/[\\/:*?"<>|]/g, '_')
  try {
    const path = await save({
      title: '导出 Postman Collection v2.1',
      defaultPath: `${safeName}.postman_collection.json`,
      filters: [{ name: 'Postman Collection', extensions: ['json'] }],
    })
    if (!path) return
    const report = await api.postmanExport(projectId.value, path)
    message.success(
      `已导出 ${report.collections} 个集合 / ${report.requests} 个请求` +
        (report.skipped.length ? `；${report.skipped.length} 项需手工补：${report.skipped.slice(0, 3).join('、')}` : ''),
    )
  } catch (e) {
    message.error(String(e))
  }
}

function onImportMenu({ key }: { key: string | number }) {
  if (importingApi.value) return
  if (key === 'curl') openCurl()
  else if (key === 'postman') void importApiFile()
  else if (key === 'openapi') void importOpenapiFile()
  else if (key === 'bruno') void importBruno()
  else if (key === 'openapi-url') openOpenapiUrl()
}

function onExportMenu({ key }: { key: string | number }) {
  if (key === 'project') void exportProject()
  else if (key === 'openapi') void exportOpenapi()
  else if (key === 'postman') void exportPostman()
  else if (key === 'docs') openDocs('project', projectId.value)
}

function requestRowProps(record: RequestSummary | RequestSearchHit) {
  if (searchMode.value) {
    return { onClick: () => openRequest(record), style: 'cursor:pointer' }
  }
  return {
    onClick: () => openRequest(record),
    style: 'cursor:pointer',
    draggable: 'true',
    onDragstart: () => onRequestDragStart(record as RequestSummary),
    onDragover: (event: DragEvent) => event.preventDefault(),
    onDrop: (event: DragEvent) => {
      event.preventDefault()
      void onRequestDrop(record as RequestSummary)
    },
  }
}

function fmtTime(ts: number) {
  return formatDateTime(ts)
}

const columns = computed(() =>
  searchMode.value
    ? [
        { title: '请求', dataIndex: 'name', key: 'name' },
        { title: '集合', dataIndex: 'collectionName', key: 'collectionName', width: 130, ellipsis: true },
        { title: '方法', dataIndex: 'method', key: 'method', width: 88 },
        { title: 'URL', dataIndex: 'url', key: 'url', ellipsis: true },
        { title: '更新', dataIndex: 'updatedAt', key: 'updatedAt', width: 158 },
        { title: '', key: 'ops', width: 96 },
      ]
    : [
        { title: '请求', dataIndex: 'name', key: 'name' },
        { title: '方法', dataIndex: 'method', key: 'method', width: 92 },
        { title: 'URL', dataIndex: 'url', key: 'url', ellipsis: true },
        { title: '更新', dataIndex: 'updatedAt', key: 'updatedAt', width: 158 },
        { title: '', key: 'ops', width: 96 },
      ],
)
</script>

<template>
  <div class="project-page">
    <div class="topbar">
      <div class="left">
        <a-button type="text" size="small" class="back-btn" @click="router.push('/')">
          <template #icon><ArrowLeftOutlined /></template>
          工作台
        </a-button>
        <span class="slash">/</span>
        <span class="project-dot" :style="{ background: store.currentProject?.color || 'var(--fm-primary)' }" />
        <div class="project-title">
          <span class="name">{{ store.currentProject?.name || '项目' }}</span>
          <span class="desc">{{ store.currentProject?.description || '本地项目' }}</span>
        </div>
      </div>
      <div class="actions">
        <a-select
          :value="store.activeEnvironment?.id"
          size="small"
          style="width: 150px"
          placeholder="环境"
          @change="(v: number) => switchEnv(v)"
        >
          <a-select-option v-for="env in store.environments" :key="env.id" :value="env.id">
            {{ env.name }}
          </a-select-option>
        </a-select>
        <a-button size="small" @click="aiAudit">
          <template #icon><AiSparkIcon /></template>
          AI 体检
        </a-button>
        <a-dropdown :trigger="['click']" :disabled="importingApi">
          <a-button size="small" :loading="importingApi">
            <template #icon><ImportOutlined /></template>
            导入
            <DownOutlined class="caret" />
          </a-button>
          <template #overlay>
            <a-menu @click="onImportMenu">
              <a-menu-item key="curl">cURL 命令</a-menu-item>
              <a-menu-item key="postman">Postman / Apifox / Insomnia / HAR</a-menu-item>
              <a-menu-item key="openapi">OpenAPI / Swagger 文件</a-menu-item>
              <a-menu-item key="openapi-url">OpenAPI / Swagger URL</a-menu-item>
              <a-menu-item key="bruno">Bruno 目录</a-menu-item>
            </a-menu>
          </template>
        </a-dropdown>
        <a-dropdown :trigger="['click']">
          <a-button size="small">
            <template #icon><ExportOutlined /></template>
            导出
            <DownOutlined class="caret" />
          </a-button>
          <template #overlay>
            <a-menu @click="onExportMenu">
              <a-menu-item key="project">项目 JSON</a-menu-item>
              <a-menu-item key="postman">Postman Collection v2.1</a-menu-item>
              <a-menu-item key="openapi">OpenAPI 3.0</a-menu-item>
              <a-menu-item key="docs">接口文档（预览后导出）</a-menu-item>
            </a-menu>
          </template>
        </a-dropdown>
        <a-button size="small" @click="mockOpen = true">
          <template #icon><ApiOutlined /></template>
          Mock
        </a-button>
        <a-button size="small" @click="contractOpen = true">
          <template #icon><SafetyCertificateOutlined /></template>
          契约
        </a-button>
        <a-button size="small" @click="docsSiteOpen = true">
          <template #icon><GlobalOutlined /></template>
          文档站
        </a-button>
        <a-button size="small" @click="router.push(`/environment/${projectId}`)">
          <template #icon><SettingOutlined /></template>
          管理环境
        </a-button>
      </div>
    </div>

    <div class="body">
      <aside class="tree-pane fm-card">
        <div class="pane-head">
          <span class="pane-title">集合</span>
          <a-tooltip title="新建根集合">
            <a-button type="text" size="small" class="add-btn" @click="addCollection(null)">
              <template #icon><PlusOutlined /></template>
            </a-button>
          </a-tooltip>
        </div>
        <div class="tree-wrap scroll-y">
          <a-tree
            v-if="treeData.length"
            :tree-data="treeData"
            :selected-keys="selectedKeys"
            :expanded-keys="expandedKeys"
            block-node
            @select="onSelect"
            @expand="onExpand"
          >
            <template #title="node">
              <div
                class="tree-node"
                draggable="true"
                @dragstart.stop="onCollectionDragStart((node as TreeNode).raw)"
                @dragover.prevent.stop
                @drop.stop="onCollectionDrop((node as TreeNode).raw)"
              >
                <FolderOutlined class="folder" />
                <span class="node-title">{{ (node as TreeNode).title }}</span>
                <span class="node-ops" @click.stop>
                  <a @click="openDocs('collection', (node as TreeNode).raw.id)">文档</a>
                  <a @click="addCollection((node as TreeNode).raw)">+子集</a>
                  <a @click="renameCollection((node as TreeNode).raw)">改名</a>
                  <a class="danger" @click="deleteCollection((node as TreeNode).raw)">删除</a>
                </span>
              </div>
            </template>
          </a-tree>
          <div v-else class="tree-empty">
            <div class="tree-empty-desc">还没有集合</div>
            <a-button size="small" type="primary" ghost @click="addCollection(null)">新建集合</a-button>
          </div>
        </div>
        <div class="tree-foot">拖拽集合或请求可调整顺序（同级）</div>
      </aside>

      <section class="list-pane fm-card">
        <div class="pane-head">
          <div class="list-title">
            <span class="pane-title">
              {{ searchMode ? '项目搜索' : selectedCollection ? selectedCollection.name : '请选择集合' }}
            </span>
            <span v-if="searchMode" class="count">
              {{ searching ? '搜索中' : `${searchHits.length} 个结果` }}
            </span>
            <span v-else-if="selectedCollection" class="count">{{ requests.length }} 个请求</span>
          </div>
          <div class="list-actions">
            <a-button
              v-if="displayRows.length > PROJECT_PAGE_THRESHOLD"
              type="link"
              size="small"
              :title="showAllRows ? '恢复每页 50 条' : '一次渲染全部接口，低配机器可能卡顿'"
              @click="showAllRows = !showAllRows"
            >
              {{ showAllRows ? '分页显示' : '全部展开' }}
            </a-button>
            <a-input
              v-model:value="keyword"
              class="search-input"
              size="small"
              allow-clear
              placeholder="搜索本项目接口"
              @press-enter="runSearch"
            >
              <template #prefix><SearchOutlined class="search-icon" /></template>
            </a-input>
            <a-button
              v-if="!searchMode"
              type="primary"
              size="small"
              :disabled="!selectedCollection"
              @click="selectedCollection && addRequest(selectedCollection)"
            >
              <template #icon><PlusOutlined /></template>
              新建请求
            </a-button>
          </div>
        </div>
        <a-spin :spinning="loadingRequests || searching">
          <div ref="tableWrap" class="table-wrap scroll-y">
          <a-table
            v-if="displayRows.length"
            :data-source="displayRows"
            :columns="columns"
            :pagination="pagination"
            row-key="id"
            size="middle"
            :custom-row="requestRowProps"
            :row-class-name="rowClassName"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'name'">
                <span class="req-name">
                  <FileTextOutlined class="req-icon" />
                  <span class="req-label" :title="(record as RequestSummary).name">
                    {{ (record as RequestSummary).name }}
                  </span>
                  <span
                    v-if="lastRequest && lastRequest.id === (record as RequestSummary).id"
                    class="last-badge"
                    title="上次打开的接口"
                  >
                    上次
                  </span>
                </span>
              </template>
              <template v-else-if="column.key === 'collectionName'">
                <span class="collection-name">{{ (record as RequestSearchHit).collectionName }}</span>
              </template>
              <template v-else-if="column.key === 'method'">
                <span class="method-tag" :class="`method-${(record as RequestSummary).method}`">
                  {{ (record as RequestSummary).method }}
                </span>
              </template>
              <template v-else-if="column.key === 'url'">
                <span class="mono url" :title="(record as RequestSummary).url">
                  {{ (record as RequestSummary).url }}
                </span>
              </template>
              <template v-else-if="column.key === 'updatedAt'">
                <span class="muted">{{ fmtTime((record as RequestSummary).updatedAt) }}</span>
              </template>
              <template v-else-if="column.key === 'ops'">
                <span class="row-ops">
                  <a class="row-copy" @click.stop="copyRowCurl(record as RequestSummary)">cURL</a>
                  <a class="row-delete" @click.stop="deleteRequest(record as RequestSummary)">删除</a>
                </span>
              </template>
            </template>
          </a-table>
          <div v-else class="list-empty">
            <div class="list-empty-icon">{{ searchMode ? '⌕' : '⌘' }}</div>
            <div class="list-empty-title">
              <template v-if="searchMode">
                {{ searching ? '正在搜索…' : `没有匹配「${keyword.trim()}」的接口` }}
              </template>
              <template v-else>
                {{ selectedCollection ? '该集合下还没有请求' : '从左侧选择一个集合' }}
              </template>
            </div>
            <div class="list-empty-desc">
              <template v-if="searchMode">
                搜索范围是当前项目的全部集合，可尝试接口名称、路径或关键字
              </template>
              <template v-else>
                {{
                  selectedCollection
                    ? '点击右上角「新建请求」，填写 URL 后即可发送或压测'
                    : '集合用于对请求分组，一个项目可以有多个集合'
                }}
              </template>
            </div>
            <a-button
              v-if="!searchMode && selectedCollection"
              type="primary"
              @click="addRequest(selectedCollection)"
            >
              新建请求
            </a-button>
          </div>
          </div>
        </a-spin>
      </section>
    </div>

    <a-modal
      v-model:open="curlOpen"
      title="从 cURL 导入请求"
      width="640px"
      :confirm-loading="curlLoading"
      ok-text="导入"
      cancel-text="取消"
      @ok="submitCurl"
    >
      <div class="curl-tip">
        把浏览器开发者工具或 Postman 里复制的 cURL 命令粘贴进来，会自动解析出方法、URL、Header 和 Body。
      </div>
      <a-form layout="vertical" style="margin-top: 12px">
        <a-form-item label="导入到集合">
          <a-select
            v-model:value="curlTarget"
            :options="collectionOptions"
            allow-clear
            placeholder="不选则自动创建「cURL 导入」集合"
          />
        </a-form-item>
        <a-form-item label="cURL 命令">
          <a-textarea
            v-model:value="curlText"
            class="mono"
            :rows="10"
            placeholder="curl -X POST https://api.example.com/users -H Content-Type:application/json -d {...}"
          />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal
      v-model:open="collectionModalOpen"
      :title="editingCollection ? '重命名集合' : '新建集合'"
      ok-text="保存"
      cancel-text="取消"
      @ok="submitCollection"
    >
      <a-input
        v-model:value="collectionName"
        placeholder="集合名称，例如：用户接口"
        style="margin-top: 12px"
        @press-enter="submitCollection"
      />
    </a-modal>

    <DocsPreviewDrawer
      v-model:open="docsOpen"
      :scope="docsScope"
      :target-id="docsTargetId"
      :project-id="projectId"
      :project-name="store.currentProject?.name"
      :default-environment-id="store.activeEnvironment?.id ?? null"
    />

    <ContractPanelDrawer
      v-model:open="contractOpen"
      :project-id="projectId"
      :project-name="store.currentProject?.name"
    />

    <DocsSiteModal
      v-model:open="docsSiteOpen"
      :project-id="projectId"
      :project-name="store.currentProject?.name"
      :environment-id="store.activeEnvironment?.id ?? null"
    />

    <MockPanel
      v-model:open="mockOpen"
      :project-id="projectId"
      :project-name="store.currentProject?.name"
    />

    <a-modal
      v-model:open="openapiUrlOpen"
      title="从 URL 导入 OpenAPI / Swagger"
      width="560px"
      :confirm-loading="openapiUrlBusy"
      ok-text="拉取并导入"
      cancel-text="取消"
      @ok="submitOpenapiUrl"
    >
      <a-form layout="vertical" style="margin-top: 12px">
        <a-form-item label="JSON 地址" required>
          <a-input v-model:value="openapiUrlForm.url" placeholder="https://example.com/openapi.json" />
        </a-form-item>
        <a-form-item label="访问令牌（可选）">
          <a-input-password
            v-model:value="openapiUrlForm.token"
            placeholder="Bearer Token；留空表示匿名访问"
            autocomplete="off"
          />
        </a-form-item>
        <a-form-item>
          <a-checkbox v-model:checked="openapiUrlForm.save">保存来源，便于以后刷新</a-checkbox>
        </a-form-item>
        <div class="curl-tip">仅支持 JSON；YAML 文档请先转换为 JSON。</div>
      </a-form>
    </a-modal>
  </div>
</template>

<style scoped>
.project-page {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.topbar {
  height: 62px;
  padding: 0 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--fm-border);
  background: color-mix(in srgb, var(--fm-surface) 86%, transparent);
  backdrop-filter: blur(8px);
}

.left {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.back-btn {
  color: var(--fm-text-2);
}

.slash {
  color: var(--fm-border-strong);
}

.project-dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  flex-shrink: 0;
  box-shadow: 0 0 0 3px var(--fm-primary-soft);
}

.project-title {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.name {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desc {
  font-size: 10px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 420px;
}

.actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.caret {
  margin-left: 2px;
  font-size: 10px;
  opacity: 0.65;
}

.curl-tip {
  padding: 10px 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-primary-soft);
  border: 1px solid var(--fm-border);
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-text-2);
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 14px;
  padding: 16px;
  overflow-x: auto;
}

.tree-pane {
  width: 272px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.list-pane {
  flex: 1 1 480px;
  /* 保证窄窗口下接口列表仍可用，宁可横向滚动也不压成一条缝 */
  min-width: 400px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.list-pane :deep(.ant-spin-nested-loading) {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.list-pane :deep(.ant-spin-container) {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

/* 固定表格布局：名称/URL 过长时在当前列内省略，不再挤开方法列 */
.table-wrap :deep(.ant-table table) {
  table-layout: fixed;
}

.table-wrap :deep(.ant-table-cell) {
  overflow: hidden;
}

.table-wrap :deep(.ant-table-tbody > tr > td) {
  padding-top: 10px !important;
  padding-bottom: 10px !important;
}

.pane-head {
  height: 48px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--fm-border);
}

.pane-title {
  font-size: 13px;
  font-weight: 600;
}

.add-btn {
  color: var(--fm-muted);
}

.tree-wrap {
  flex: 1;
  padding: 10px 8px;
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  min-height: 28px;
  line-height: 28px;
}

.folder {
  color: var(--fm-muted);
  font-size: 12px;
  flex-shrink: 0;
}

.node-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-ops {
  display: none;
  gap: 8px;
  font-size: 11px;
  flex-shrink: 0;
}

.tree-node:hover .node-ops {
  display: flex;
}

.node-ops .danger {
  color: var(--fm-danger);
}

.tree-empty {
  padding: 30px 12px;
  text-align: center;
}

.tree-empty-desc {
  font-size: 12px;
  color: var(--fm-muted);
  margin-bottom: 10px;
}

.tree-foot {
  padding: 10px 14px;
  border-top: 1px solid var(--fm-border);
  font-size: 10px;
  color: var(--fm-muted);
}

.list-title {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
  overflow: hidden;
}

.list-title .pane-title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.list-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.search-input {
  width: 200px;
}

.search-input :deep(.ant-input-prefix) {
  margin-right: 4px;
}

.search-icon {
  color: var(--fm-muted);
  font-size: 12px;
}

.collection-name {
  font-size: 11px;
  color: var(--fm-text-2);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 7px;
  border-radius: var(--fm-radius-sm);
}

.count {
  flex-shrink: 0;
  white-space: nowrap;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 7px;
  border-radius: 999px;
}

.last-badge {
  flex-shrink: 0;
  font-size: 10px;
  font-weight: 400;
  line-height: 1;
  padding: 2px 6px;
  border-radius: 999px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
  border: 1px solid var(--fm-border);
}

.table-wrap :deep(tr.last-opened-row > td) {
  background: var(--fm-primary-soft);
}

.table-wrap :deep(tr.last-opened-row > td:first-child) {
  box-shadow: inset 2px 0 0 var(--fm-primary);
}

.table-wrap :deep(tr.last-opened-row:hover > td) {
  background: var(--fm-primary-soft);
}

.req-name {
  display: flex;
  align-items: center;
  gap: 7px;
  font-weight: 500;
  min-width: 0;
  overflow: hidden;
}

.req-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.req-icon {
  color: var(--fm-muted);
  font-size: 12px;
  flex-shrink: 0;
}

.url {
  display: block;
  color: var(--fm-muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.muted {
  color: var(--fm-muted);
  font-size: 11px;
}

.row-ops {
  display: inline-flex;
  align-items: center;
  gap: 10px;
}

.row-copy {
  font-size: 11px;
  color: var(--fm-primary);
  opacity: 0.8;
}

.row-copy:hover {
  opacity: 1;
}

.row-delete {
  font-size: 11px;
  color: var(--fm-danger);
  opacity: 0.75;
}

.row-delete:hover {
  opacity: 1;
}

.list-empty {
  padding: 70px 24px;
  text-align: center;
}

.list-empty-icon {
  width: 48px;
  height: 48px;
  margin: 0 auto 14px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface-alt);
  color: var(--fm-muted);
  font-size: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.list-empty-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--fm-text-2);
}

.list-empty-desc {
  margin: 6px auto 16px;
  max-width: 360px;
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-muted);
}

@media (max-width: 1080px) {
  .tree-pane {
    width: 224px;
  }

  .search-input {
    width: 168px;
  }

  .topbar {
    padding: 0 16px;
  }

  .actions :deep(.ant-btn) {
    padding: 0 9px;
  }
}

@media (max-width: 900px) {
  .topbar {
    height: auto;
    min-height: 62px;
    padding: 10px 14px;
    flex-wrap: wrap;
    gap: 8px;
  }

  .actions {
    flex-wrap: wrap;
  }

  .body {
    flex-direction: column;
    padding: 12px;
    overflow-x: hidden;
  }

  .tree-pane {
    width: 100%;
    max-height: 210px;
  }

  .tree-foot {
    display: none;
  }

  .list-pane {
    flex: 1;
    min-width: 0;
    min-height: 320px;
  }

  .search-input {
    width: 140px;
  }
}
</style>
