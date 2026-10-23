<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { save } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeftOutlined,
  PlusOutlined,
  FolderOutlined,
  SettingOutlined,
  FileTextOutlined,
  ImportOutlined,
  ExportOutlined,
  SearchOutlined,
} from '@ant-design/icons-vue'
import { useProjectStore } from '@/stores/project'
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

const projectId = computed(() => Number(route.params.id))
const selectedKeys = ref<string[]>([])
const selectedCollection = computed(() => {
  if (!selectedKeys.value.length) return null
  return findNode(store.tree, Number(selectedKeys.value[0]))
})
const requests = ref<RequestSummary[]>([])
const loadingRequests = ref(false)

const keyword = ref('')
const searchHits = ref<RequestSearchHit[]>([])
const searching = ref(false)
const searchMode = computed(() => keyword.value.trim().length > 0)
const displayRows = computed<Array<RequestSummary | RequestSearchHit>>(() =>
  searchMode.value ? searchHits.value : requests.value,
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
  try {
    await store.openProject(projectId.value)
    store.syncActiveEnvironment()
    if (store.tree.length) {
      selectedKeys.value = [String(store.tree[0].id)]
      await loadRequests(store.tree[0].id)
    }
  } catch (e) {
    message.error(String(e))
  }
})

async function onSelect(keys: Array<string | number>) {
  const key = keys[0]
  if (key === undefined) return
  selectedKeys.value = [String(key)]
  await loadRequests(Number(key))
}

async function loadRequests(collectionId: number) {
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
    },
  })
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
    router.push({ name: 'request', params: { id: saved.id }, query: { projectId: projectId.value } })
  } catch (e) {
    message.error(String(e))
  }
}

function openRequest(row: RequestSummary | RequestSearchHit) {
  router.push({ name: 'request', params: { id: row.id }, query: { projectId: projectId.value } })
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
    router.push({ name: 'request', params: { id: saved.id }, query: { projectId: projectId.value } })
  } catch (e) {
    message.error(String(e))
  } finally {
    curlLoading.value = false
  }
}

async function exportProject() {
  const safeName = (store.currentProject?.name || 'finkmeter-project').replace(/[\\/:*?"<>|]/g, '_')
  try {
    const path = await save({
      title: '导出项目',
      defaultPath: `${safeName}.finkmeter.json`,
      filters: [{ name: 'FinkMeter 项目文件', extensions: ['json'] }],
    })
    if (!path) return
    await api.projectExport(projectId.value, path)
    message.success('项目已导出，可通过「导入项目」恢复')
  } catch (e) {
    message.error(String(e))
  }
}

function fmtTime(ts: number) {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

const columns = computed(() =>
  searchMode.value
    ? [
        { title: '请求', dataIndex: 'name', key: 'name' },
        { title: '集合', dataIndex: 'collectionName', key: 'collectionName', width: 130, ellipsis: true },
        { title: '方法', dataIndex: 'method', key: 'method', width: 88 },
        { title: 'URL', dataIndex: 'url', key: 'url', ellipsis: true },
        { title: '更新', dataIndex: 'updatedAt', key: 'updatedAt', width: 104 },
        { title: '', key: 'ops', width: 56 },
      ]
    : [
        { title: '请求', dataIndex: 'name', key: 'name' },
        { title: '方法', dataIndex: 'method', key: 'method', width: 92 },
        { title: 'URL', dataIndex: 'url', key: 'url', ellipsis: true },
        { title: '更新', dataIndex: 'updatedAt', key: 'updatedAt', width: 110 },
        { title: '', key: 'ops', width: 56 },
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
        <span class="project-dot" :style="{ background: store.currentProject?.color || '#3d6b8f' }" />
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
        <a-button size="small" @click="openCurl">
          <template #icon><ImportOutlined /></template>
          导入 cURL
        </a-button>
        <a-button size="small" @click="exportProject">
          <template #icon><ExportOutlined /></template>
          导出项目
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
            block-node
            @select="onSelect"
          >
            <template #title="node">
              <div class="tree-node">
                <FolderOutlined class="folder" />
                <span class="node-title">{{ (node as TreeNode).title }}</span>
                <span class="node-ops" @click.stop>
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
        <div class="tree-foot">拖拽排序将在后续版本开放</div>
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
          <div class="table-wrap scroll-y">
          <a-table
            v-if="displayRows.length"
            :data-source="displayRows"
            :columns="columns"
            :pagination="false"
            row-key="id"
            size="middle"
            :custom-row="(record: RequestSummary | RequestSearchHit) => ({ onClick: () => openRequest(record), style: 'cursor:pointer' })"
          >
            <template #bodyCell="{ column, record }">
              <template v-if="column.key === 'name'">
                <span class="req-name">
                  <FileTextOutlined class="req-icon" />
                  {{ (record as RequestSummary).name }}
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
                <span class="mono url">{{ (record as RequestSummary).url }}</span>
              </template>
              <template v-else-if="column.key === 'updatedAt'">
                <span class="muted">{{ fmtTime((record as RequestSummary).updatedAt) }}</span>
              </template>
              <template v-else-if="column.key === 'ops'">
                <a class="row-delete" @click.stop="deleteRequest(record as RequestSummary)">删除</a>
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
  background: rgba(255, 255, 255, 0.86);
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
  color: #d3dae6;
}

.project-dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  flex-shrink: 0;
  box-shadow: 0 0 0 3px rgba(60, 107, 136, 0.1);
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
}

.tree-pane {
  width: 272px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.list-pane {
  flex: 1;
  min-width: 0;
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
  min-height: 24px;
}

.folder {
  color: #c0cad9;
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
  color: #b6c0ce;
}

.list-title {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
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
  border-radius: 5px;
}

.count {
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 7px;
  border-radius: 999px;
}

.req-name {
  display: flex;
  align-items: center;
  gap: 7px;
  font-weight: 500;
}

.req-icon {
  color: #b6c0ce;
  font-size: 12px;
}

.url {
  color: var(--fm-muted);
  font-size: 11px;
}

.muted {
  color: var(--fm-muted);
  font-size: 11px;
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
  border-radius: 14px;
  background: var(--fm-surface-alt);
  color: #c2ccdb;
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
  }

  .tree-pane {
    width: 100%;
    max-height: 210px;
  }

  .tree-foot {
    display: none;
  }

  .list-pane {
    min-height: 320px;
  }

  .search-input {
    width: 140px;
  }
}
</style>
