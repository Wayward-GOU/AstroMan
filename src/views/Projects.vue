<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import {
  PlusOutlined,
  ArrowRightOutlined,
  EditOutlined,
  DeleteOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  SearchOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useProjectStore } from '@/stores/project'
import { formatDateTime } from '@/utils/datetime'
import * as api from '@/api'
import type { Project } from '@/types/models'

const router = useRouter()
const store = useProjectStore()
const loading = ref(true)
const keyword = ref('')

const projects = computed(() => {
  const text = keyword.value.trim().toLowerCase()
  if (!text) return store.projects
  return store.projects.filter(
    (item) =>
      item.name.toLowerCase().includes(text) ||
      (item.description ?? '').toLowerCase().includes(text),
  )
})

async function load() {
  loading.value = true
  try {
    await store.loadProjects()
  } catch (error) {
    message.error(String(error))
  } finally {
    loading.value = false
  }
}

/* ---------- 新建 / 编辑 ---------- */
const editor = reactive({
  open: false,
  saving: false,
  id: undefined as number | undefined,
  name: '',
  description: '',
})

function openCreate() {
  editor.open = true
  editor.id = undefined
  editor.name = ''
  editor.description = ''
}

function openEdit(project: Project) {
  editor.open = true
  editor.id = project.id
  editor.name = project.name
  editor.description = project.description ?? ''
}

async function saveProject() {
  const name = editor.name.trim()
  if (!name) {
    message.warning('请填写项目名称')
    return
  }
  editor.saving = true
  try {
    if (editor.id) {
      await api.projectUpdate(editor.id, name, editor.description.trim() || undefined)
      message.success('项目已更新')
    } else {
      await api.projectCreate(name, editor.description.trim() || undefined)
      message.success('项目已创建')
    }
    editor.open = false
    await load()
  } catch (error) {
    message.error(String(error))
  } finally {
    editor.saving = false
  }
}

function remove(project: Project) {
  Modal.confirm({
    title: `删除项目「${project.name}」？`,
    content: '项目下的集合、接口、环境、历史记录都会一起删除，且不可恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      try {
        await api.projectDelete(project.id)
        await load()
        message.success('项目已删除')
      } catch (error) {
        message.error(String(error))
      }
    },
  })
}

function enter(project: Project) {
  router.push(`/project/${project.id}`)
}

onMounted(load)
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Projects"
      title="项目管理"
      subtitle="按项目组织接口、环境与测试场景；进入项目后可管理集合与请求"
    >
      <a-input
        v-model:value="keyword"
        allow-clear
        placeholder="搜索项目"
        style="width: 200px"
        size="small"
      >
        <template #prefix><SearchOutlined /></template>
      </a-input>
      <a-button size="small" @click="load"><ReloadOutlined /> 刷新</a-button>
      <a-button type="primary" size="small" @click="openCreate"><PlusOutlined /> 新建项目</a-button>
    </PageHeader>

    <a-spin :spinning="loading">
      <div v-if="projects.length" class="project-grid">
        <div v-for="project in projects" :key="project.id" class="fm-card project-card">
          <div class="card-head">
            <div class="folder"><FolderOpenOutlined /></div>
            <div class="title-box">
              <div class="name">{{ project.name }}</div>
              <div class="time">更新于 {{ formatDateTime(project.updatedAt) }}</div>
            </div>
          </div>
          <div class="desc">{{ project.description || '暂无项目描述' }}</div>
          <div class="card-foot">
            <a-button type="text" size="small" @click="openEdit(project)"><EditOutlined /> 编辑</a-button>
            <a-button type="text" size="small" danger @click="remove(project)"><DeleteOutlined /></a-button>
            <a-button type="primary" size="small" class="enter" @click="enter(project)">
              进入项目 <ArrowRightOutlined />
            </a-button>
          </div>
        </div>
        <button class="fm-card new-card" type="button" @click="openCreate">
          <PlusOutlined />
          <span>新建项目</span>
        </button>
      </div>
      <div v-else-if="!loading" class="fm-card empty">
        <div class="empty-icon"><FolderOpenOutlined /></div>
        <div class="empty-title">{{ keyword ? '没有匹配的项目' : '还没有项目' }}</div>
        <div class="empty-desc">新建一个项目，把接口、环境与自动化测试套件放在一起管理。</div>
        <a-button type="primary" @click="openCreate"><PlusOutlined /> 新建项目</a-button>
      </div>
    </a-spin>

    <a-modal
      v-model:open="editor.open"
      :title="editor.id ? '编辑项目' : '新建项目'"
      :confirm-loading="editor.saving"
      ok-text="保存"
      cancel-text="取消"
      @ok="saveProject"
    >
      <a-form layout="vertical">
        <a-form-item label="项目名称" required>
          <a-input v-model:value="editor.name" placeholder="例如：订单服务" allow-clear @press-enter="saveProject" />
        </a-form-item>
        <a-form-item label="项目描述">
          <a-textarea v-model:value="editor.description" :rows="3" placeholder="可选，写一句项目用途" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<style scoped>
.project-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}
.project-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  min-height: 150px;
  transition: border-color 0.15s;
}

.project-card:hover {
  border-color: var(--fm-border-strong);
}
.card-head {
  display: flex;
  gap: 10px;
  align-items: center;
}
.folder {
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 17px;
}
.title-box {
  min-width: 0;
  flex: 1;
}
.name {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.time {
  font-size: 10.5px;
  color: var(--fm-muted);
  margin-top: 2px;
}
.desc {
  flex: 1;
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.7;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.card-foot {
  display: flex;
  align-items: center;
  gap: 2px;
  border-top: 1px solid var(--fm-border);
  padding-top: 10px;
}
.card-foot .enter {
  margin-left: auto;
}
.new-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 150px;
  border: 1px dashed var(--fm-border-strong);
  background: transparent;
  color: var(--fm-muted);
  cursor: pointer;
  font-size: 12px;
  font-family: inherit;
}
.new-card :deep(.anticon) {
  font-size: 20px;
}
.new-card:hover {
  color: var(--fm-primary);
  border-color: var(--fm-primary);
}
.empty {
  padding: 66px 24px;
  text-align: center;
}
.empty-icon {
  width: 56px;
  height: 56px;
  margin: 0 auto 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 24px;
}
.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}
.empty-desc {
  max-width: 420px;
  margin: 0 auto 18px;
  font-size: 12px;
  color: var(--fm-muted);
  line-height: 1.7;
}
</style>
