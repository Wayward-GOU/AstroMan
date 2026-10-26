<script setup lang="ts">
import { computed, defineAsyncComponent, markRaw, onMounted, ref, shallowRef, watch, type Component } from 'vue'
import { useRoute } from 'vue-router'
import {
  ArrowLeftOutlined,
  BookOutlined,
  CalculatorOutlined,
  CodeOutlined,
  FontColorsOutlined,
  GlobalOutlined,
  LockOutlined,
  QrcodeOutlined,
  SearchOutlined,
  ToolOutlined,
} from '@ant-design/icons-vue'
import ToolWorkbench from '@/components/tools/ToolWorkbench.vue'
import { searchTools, TOOL_CATEGORIES, TOOLS, TOOL_MAP } from '@/tools/registry'
import type { ToolCategoryId, ToolDef } from '@/tools/types'

const keyword = ref('')
const activeCategory = ref<ToolCategoryId>('json')
const activeToolId = ref<string | null>(null)
const activeComponent = shallowRef<Component | null>(null)
const loading = ref(false)

const route = useRoute()

/** 支持从工作台等页面通过 ?tool=xxx 直接打开指定工具 */
function openFromRoute() {
  const id = typeof route.query.tool === 'string' ? route.query.tool : ''
  const tool = id ? TOOL_MAP.get(id) : null
  if (!tool) return
  keyword.value = ''
  activeCategory.value = tool.category
  activeToolId.value = tool.id
}

onMounted(openFromRoute)
watch(() => route.query.tool, openFromRoute)

const CATEGORY_ICONS: Record<ToolCategoryId, Component> = {
  json: CodeOutlined,
  format: FontColorsOutlined,
  crypto: LockOutlined,
  text: FontColorsOutlined,
  calc: CalculatorOutlined,
  dev: ToolOutlined,
  network: GlobalOutlined,
  qr: QrcodeOutlined,
  reference: BookOutlined,
}

const categoryCount = (id: ToolCategoryId) => TOOLS.filter((tool) => tool.category === id).length

const filtered = computed(() => {
  const list = keyword.value.trim() ? searchTools(keyword.value) : TOOLS
  if (keyword.value.trim()) return list
  return list.filter((tool) => tool.category === activeCategory.value)
})

const activeTool = computed<ToolDef | null>(() =>
  activeToolId.value ? TOOL_MAP.get(activeToolId.value) ?? null : null,
)

const activeCategoryName = computed(() => {
  if (keyword.value.trim() && !activeToolId.value) return '搜索结果'
  return TOOL_CATEGORIES.find((item) => item.id === activeCategory.value)?.name ?? ''
})

watch(
  activeTool,
  async (tool) => {
    activeComponent.value = null
    if (!tool) return
    if (tool.simple) {
      activeComponent.value = markRaw(ToolWorkbench)
      return
    }
    if (!tool.component) return
    loading.value = true
    try {
      const module = await tool.component()
      activeComponent.value = markRaw(module.default)
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)

function selectCategory(id: ToolCategoryId) {
  activeCategory.value = id
  activeToolId.value = null
  keyword.value = ''
}

function openTool(tool: ToolDef) {
  activeToolId.value = tool.id
}

function backToList() {
  activeToolId.value = null
}

const componentProps = computed(() => {
  if (!activeTool.value) return {}
  if (activeTool.value.simple) return { tool: activeTool.value }
  return activeTool.value.props ?? {}
})

const componentKey = computed(() => activeTool.value?.id ?? '')
</script>

<template>
  <div class="page tools-page">
    <header class="page-head">
      <div>
        <h1 class="page-title">工具箱</h1>
        <p class="page-subtitle">
          共 {{ TOOLS.length }} 个开发工具，全部在本机离线运行，输入内容不会离开这台电脑
        </p>
      </div>
      <a-input
        v-model:value="keyword"
        class="tools-search"
        placeholder="搜索工具名称或关键字，例如：json、aes、时间戳"
        allow-clear
      >
        <template #prefix><SearchOutlined /></template>
      </a-input>
    </header>

    <div class="tools-layout">
      <aside class="tools-nav">
        <button
          v-for="category in TOOL_CATEGORIES"
          :key="category.id"
          class="tools-nav-item"
          :class="{ active: !keyword && activeCategory === category.id }"
          @click="selectCategory(category.id)"
        >
          <component :is="CATEGORY_ICONS[category.id]" class="tools-nav-icon" />
          <span class="tools-nav-text">
            <span class="tools-nav-name">{{ category.name }}</span>
            <span class="tools-nav-hint">{{ category.hint }}</span>
          </span>
          <span class="tools-nav-count">{{ categoryCount(category.id) }}</span>
        </button>
      </aside>

      <main class="tools-main">
        <template v-if="activeTool">
          <div class="tools-detail-head">
            <a-button type="text" size="small" class="tools-back" @click="backToList">
              <ArrowLeftOutlined /> 返回工具列表
            </a-button>
            <div class="tools-detail-title">
              <h2>{{ activeTool.name }}</h2>
              <p>{{ activeTool.desc }}</p>
            </div>
          </div>
          <a-spin :spinning="loading">
            <component
              :is="activeComponent"
              v-if="activeComponent"
              :key="componentKey"
              v-bind="componentProps"
            />
            <div v-else-if="!loading" class="empty-hint">该工具暂未提供界面</div>
          </a-spin>
        </template>

        <template v-else>
          <div class="tools-section-head">
            <span class="tools-section-title">{{ activeCategoryName }}</span>
            <span class="tools-section-count">{{ filtered.length }} 个工具</span>
          </div>

          <div v-if="filtered.length" class="tools-grid">
            <button v-for="tool in filtered" :key="tool.id" class="tool-card" @click="openTool(tool)">
              <span class="tool-card-tag">
                {{ TOOL_CATEGORIES.find((item) => item.id === tool.category)?.name }}
              </span>
              <span class="tool-card-name">{{ tool.name }}</span>
              <span class="tool-card-desc">{{ tool.desc }}</span>
            </button>
          </div>
          <div v-else class="empty-hint">
            没有匹配「{{ keyword }}」的工具，换个关键字试试
          </div>
        </template>
      </main>
    </div>
  </div>
</template>

<style scoped>
.tools-page {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.page-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}

.tools-search {
  width: 340px;
  max-width: 100%;
}

.tools-layout {
  display: grid;
  grid-template-columns: 208px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}

.tools-nav {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  position: sticky;
  top: 0;
}

.tools-nav-item {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 9px;
  border: none;
  background: transparent;
  border-radius: var(--fm-radius-sm);
  cursor: pointer;
  text-align: left;
  color: var(--fm-text-2);
  transition: background 0.15s, color 0.15s;
}

.tools-nav-item:hover {
  background: var(--fm-surface-alt);
}

.tools-nav-item.active {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.tools-nav-icon {
  font-size: 14px;
  flex-shrink: 0;
  color: var(--fm-muted);
}

.tools-nav-item.active .tools-nav-icon {
  color: var(--fm-primary);
}

.tools-nav-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}

.tools-nav-name {
  font-size: 13px;
  font-weight: 500;
}

.tools-nav-hint {
  font-size: 11px;
  color: var(--fm-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tools-nav-count {
  font-size: 11px;
  color: var(--fm-muted);
  font-variant-numeric: tabular-nums;
}

.tools-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.tools-section-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.tools-section-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--fm-text);
}

.tools-section-count {
  font-size: 12px;
  color: var(--fm-muted);
}

.tools-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 10px;
}

.tool-card {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 12px 14px;
  text-align: left;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
}

.tool-card:hover {
  border-color: var(--fm-primary);
  box-shadow: var(--fm-shadow);
}

.tool-card-tag {
  font-size: 10px;
  color: var(--fm-muted);
  letter-spacing: 0.3px;
  text-transform: uppercase;
}

.tool-card-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--fm-text);
}

.tool-card-desc {
  font-size: 12px;
  line-height: 1.55;
  color: var(--fm-muted);
}

.tools-detail-head {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tools-back {
  align-self: flex-start;
  padding-left: 0;
  color: var(--fm-muted);
}

.tools-detail-title h2 {
  margin: 0 0 2px;
  font-size: 16px;
  font-weight: 600;
  color: var(--fm-text);
}

.tools-detail-title p {
  margin: 0;
  font-size: 12px;
  color: var(--fm-muted);
}

@media (max-width: 900px) {
  .tools-layout {
    grid-template-columns: minmax(0, 1fr);
  }

  .tools-nav {
    position: static;
    flex-direction: row;
    overflow-x: auto;
  }

  .tools-nav-text {
    display: none;
  }

  .tools-nav-count {
    display: none;
  }
}
</style>
