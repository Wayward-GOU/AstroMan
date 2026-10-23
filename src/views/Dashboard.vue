<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message, Modal } from 'ant-design-vue'
import { open } from '@tauri-apps/plugin-dialog'
import {
  PlusOutlined,
  FundOutlined,
  ArrowRightOutlined,
  ThunderboltOutlined,
  DeploymentUnitOutlined,
  CodeOutlined,
  FolderOpenOutlined,
  RocketOutlined,
  DatabaseOutlined,
  ImportOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import { useProjectStore } from '@/stores/project'
import * as api from '@/api'
import type { HistoryItem, StressReport } from '@/types/models'

const router = useRouter()
const store = useProjectStore()

const createOpen = ref(false)
const newName = ref('')
const newDesc = ref('')
const submitting = ref(false)
const history = ref<HistoryItem[]>([])
const reports = ref<StressReport[]>([])

const firstProjectId = computed(() => store.projects[0]?.id ?? null)

onMounted(async () => {
  try {
    await store.loadProjects()
    const [his, reps] = await Promise.all([api.historyList(8), api.reportList(200)])
    history.value = his
    reports.value = reps
  } catch (e) {
    message.error(String(e))
  }
})

async function createProject() {
  if (!newName.value.trim()) {
    message.warning('请填写项目名称')
    return
  }
  submitting.value = true
  try {
    const p = await api.projectCreate(newName.value.trim(), newDesc.value.trim() || undefined)
    createOpen.value = false
    newName.value = ''
    newDesc.value = ''
    await store.loadProjects()
    router.push(`/project/${p.id}`)
  } catch (e) {
    message.error(String(e))
  } finally {
    submitting.value = false
  }
}

function removeProject(id: number, name: string) {
  Modal.confirm({
    title: `删除项目「${name}」？`,
    content: '项目下的集合、请求将一并删除，且无法恢复。',
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.projectDelete(id)
      await store.loadProjects()
      message.success('已删除')
    },
  })
}

async function importProject() {
  try {
    const path = await open({
      title: '导入 FinkMeter 项目',
      multiple: false,
      filters: [{ name: 'FinkMeter 项目文件', extensions: ['json'] }],
    })
    if (!path) return
    const project = await api.projectImport(path)
    await store.loadProjects()
    message.success(`已导入项目「${project.name}」`)
    router.push(`/project/${project.id}`)
  } catch (e) {
    message.error(String(e))
  }
}

function clearHistory() {
  Modal.confirm({
    title: '清空全部请求历史？',
    content: '只删除本机记录的请求历史，不会影响项目、接口与压测报告。',
    okText: '清空',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await api.historyClear()
      history.value = []
      message.success('历史已清空')
    },
  })
}

function scrollToGuide() {
  document.getElementById('guide')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

function goEnvironment() {
  if (firstProjectId.value) router.push(`/environment/${firstProjectId.value}`)
  else createOpen.value = true
}

function goProject() {
  if (firstProjectId.value) router.push(`/project/${firstProjectId.value}`)
  else createOpen.value = true
}

function fmtTime(ts: number) {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function statusColor(code?: number | null) {
  if (!code) return '#64748b'
  if (code < 300) return '#3f7f5f'
  if (code < 400) return '#b07d33'
  return '#b0524d'
}

const steps = [
  {
    key: 1,
    icon: FolderOpenOutlined,
    title: '创建项目',
    desc: '一个项目对应一组接口，创建后会自带默认集合，请求按集合分组管理。',
    action: '新建项目',
  },
  {
    key: 2,
    icon: DeploymentUnitOutlined,
    title: '配置环境变量',
    desc: '把 base_url、token 写进环境变量，请求里用 {{base_url}} 引用，切换环境即可换地址。',
    action: '管理环境',
  },
  {
    key: 3,
    icon: CodeOutlined,
    title: '编辑并发送请求',
    desc: '填写 URL、Headers、Body，点「发送」查看状态码与耗时，每次发送都会写入本地历史。',
    action: '打开项目',
  },
  {
    key: 4,
    icon: ThunderboltOutlined,
    title: '发起压测',
    desc: '在请求编辑器点「压测」，设置并发数与时长，实时看 QPS 和延迟，结束后自动生成报告。',
    action: '查看报告',
  },
]

function runStep(key: number) {
  if (key === 1) createOpen.value = true
  else if (key === 2) goEnvironment()
  else if (key === 3) goProject()
  else router.push('/reports')
}

const faqs = [
  {
    q: '数据保存在哪里？会上传到云端吗？',
    a: '不会。所有项目、请求、历史和压测报告都保存在本机的 SQLite 文件中（设置页可以看到数据库路径），断网也能正常使用。',
  },
  {
    q: '请求里的 {{变量}} 是怎么替换的？',
    a: '先选一个环境，再用 {{变量名}} 引用环境变量。还支持内置函数：{{timestamp}}、{{uuid}}、{{randomInt(1,100)}}、{{randomString(8)}}。',
  },
  {
    q: '压测并发数开多少合适？',
    a: '取决于目标服务与本机性能。建议从 10～30 并发、30 秒时长起步，观察错误率与延迟后再逐步提高，避免一次压垮测试环境。',
  },
  {
    q: '压测把接口打挂了怎么办？',
    a: '压测页和运行中都可以随时点「停止」，已产生的数据仍会保存成报告；建议只在测试环境压测，并提前确认目标服务限流策略。',
  },
]
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Workspace"
      title="工作台"
      subtitle="本地 API 调试与压测，创建项目后即可开始，无需登录"
    >
      <a-button @click="scrollToGuide">使用说明</a-button>
      <a-button @click="importProject">
        <template #icon><ImportOutlined /></template>
        导入项目
      </a-button>
      <a-button type="primary" @click="createOpen = true">
        <template #icon><PlusOutlined /></template>
        新建项目
      </a-button>
    </PageHeader>

    <section class="hero">
      <div class="hero-glow" />
      <div class="hero-main">
        <div class="hero-badge">
          <RocketOutlined />
          <span>纯本地 · 免注册 · 开箱即用</span>
        </div>
        <h2>让接口调试和压测<br />都留在你的电脑里</h2>
        <p>
          项目、请求、环境变量、压测报告全部存在本机 SQLite 中，不依赖任何云端服务。
          打开就能发请求，也随时能对接口做一次真实压测。
        </p>
        <div class="hero-actions">
          <a-button type="primary" size="large" @click="createOpen = true">开始创建项目</a-button>
          <a-button size="large" class="ghost-btn" @click="router.push('/reports')">
            <template #icon><FundOutlined /></template>
            压测报告
          </a-button>
        </div>
      </div>

      <div class="hero-stats">
        <div class="stat">
          <span class="stat-value">{{ store.projects.length }}</span>
          <span class="stat-label">项目</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ history.length }}</span>
          <span class="stat-label">最近请求</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ reports.length }}</span>
          <span class="stat-label">压测报告</span>
        </div>
        <div class="hero-note">
          <DatabaseOutlined />
          数据存储在本机，不上传云端
        </div>
      </div>
    </section>

    <section id="guide" class="guide">
      <div class="section-head">
        <div>
          <h2 class="section-title">快速开始</h2>
          <div class="section-desc">四步完成从建项目到出压测报告的完整流程</div>
        </div>
        <a-button type="link" size="small" @click="goProject">
          打开示例项目<ArrowRightOutlined />
        </a-button>
      </div>

      <div class="steps">
        <div v-for="step in steps" :key="step.key" class="step-card fm-card">
          <div class="step-top">
            <span class="step-icon"><component :is="step.icon" /></span>
            <span class="step-index">0{{ step.key }}</span>
          </div>
          <div class="step-title">{{ step.title }}</div>
          <div class="step-desc">{{ step.desc }}</div>
          <a class="step-link" @click="runStep(step.key)">
            {{ step.action }}<ArrowRightOutlined />
          </a>
        </div>
      </div>

      <div class="guide-tips">
        <span class="tips-label">小贴士</span>
        <span class="tip mono" v-pre>{{base_url}}</span>
        <span class="tip-text">引用环境变量</span>
        <span class="divider" />
        <span class="tip mono">Enter</span>
        <span class="tip-text">在 URL 框直接发送</span>
        <span class="divider" />
        <span class="tip mono" v-pre>{{randomInt(1,100)}}</span>
        <span class="tip-text">生成随机参数</span>
      </div>

      <a-collapse class="faq" :bordered="false" ghost>
        <a-collapse-panel v-for="(f, i) in faqs" :key="i" :header="f.q">
          <p class="faq-answer">{{ f.a }}</p>
        </a-collapse-panel>
      </a-collapse>
    </section>

    <div class="section-head">
      <div>
        <h2 class="section-title">我的项目</h2>
        <div class="section-desc">点击项目进入集合与请求管理</div>
      </div>
    </div>

    <a-spin :spinning="store.loading">
      <div v-if="store.projects.length" class="grid">
        <div
          v-for="p in store.projects"
          :key="p.id"
          class="fm-card project-card"
          @click="router.push(`/project/${p.id}`)"
        >
          <div class="project-top">
            <span class="dot" :style="{ background: p.color || '#3d6b8f' }" />
            <span class="project-name">{{ p.name }}</span>
            <a-dropdown :trigger="['click']">
              <a class="more" @click.stop>···</a>
              <template #overlay>
                <a-menu>
                  <a-menu-item key="delete" danger @click="removeProject(p.id, p.name)">删除项目</a-menu-item>
                </a-menu>
              </template>
            </a-dropdown>
          </div>
          <div class="project-desc">{{ p.description || '暂无描述' }}</div>
          <div class="project-foot">
            <span class="project-meta">更新于 {{ fmtTime(p.updatedAt) }}</span>
            <span class="project-open">进入 <ArrowRightOutlined /></span>
          </div>
        </div>

        <button class="new-project-card" @click="createOpen = true">
          <PlusOutlined />
          <span>新建项目</span>
        </button>
      </div>
      <div v-else class="fm-card empty-projects">
        <div class="empty-illustration">📁</div>
        <div class="empty-title">还没有项目</div>
        <div class="empty-desc">创建一个项目，或者直接打开应用自带的示例项目体验完整流程</div>
        <a-button type="primary" @click="createOpen = true">新建项目</a-button>
      </div>
    </a-spin>

    <div class="section-head">
      <div>
        <h2 class="section-title">最近请求</h2>
        <div class="section-desc">每次发送自动记录，最多保留 5000 条</div>
      </div>
      <div v-if="history.length" class="head-actions">
        <a-button size="small" @click="clearHistory">清空历史</a-button>
        <a-button type="link" size="small" @click="goProject">去发送请求</a-button>
      </div>
    </div>

    <div v-if="history.length" class="fm-card list">
      <div v-for="h in history" :key="h.id" class="history-row">
        <span class="method-tag" :class="`method-${h.method}`">{{ h.method }}</span>
        <span class="url mono" :title="h.url">{{ h.url }}</span>
        <span class="status mono" :style="{ color: statusColor(h.statusCode) }">{{ h.statusCode ?? 'ERR' }}</span>
        <span class="time mono">{{ h.durationMs ?? '-' }} ms</span>
        <span class="when">{{ fmtTime(h.createdAt) }}</span>
      </div>
    </div>
    <div v-else class="fm-card empty-hint">
      <span class="empty-icon">⏱</span>
      还没有发送过请求，进入项目后点「发送」即可记录
    </div>

    <a-modal
      v-model:open="createOpen"
      title="新建项目"
      :confirm-loading="submitting"
      ok-text="创建"
      cancel-text="取消"
      @ok="createProject"
    >
      <a-form layout="vertical" style="margin-top: 12px">
        <a-form-item label="项目名称" required>
          <a-input v-model:value="newName" placeholder="例如：订单服务" allow-clear @press-enter="createProject" />
        </a-form-item>
        <a-form-item label="描述">
          <a-textarea v-model:value="newDesc" :rows="3" placeholder="可选，写一句项目用途" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<style scoped>
/* ---------- Hero ---------- */
.hero {
  position: relative;
  display: flex;
  justify-content: space-between;
  gap: 28px;
  padding: 28px 32px;
  border-radius: var(--fm-radius-lg);
  background: linear-gradient(120deg, #24384a 0%, #2b4257 55%, #33505e 100%);
  color: #fff;
  overflow: hidden;
  box-shadow: 0 8px 22px rgba(15, 23, 42, 0.12);
}

.hero::after {
  content: '';
  position: absolute;
  inset: 0;
  background-image: linear-gradient(rgba(255, 255, 255, 0.02) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255, 255, 255, 0.02) 1px, transparent 1px);
  background-size: 34px 34px;
  pointer-events: none;
}

.hero-glow {
  position: absolute;
  width: 320px;
  height: 320px;
  right: 16%;
  top: -150px;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(143, 179, 201, 0.14), transparent 65%);
}

.hero-main {
  position: relative;
  z-index: 1;
  max-width: 540px;
}

.hero-badge {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  letter-spacing: 0.3px;
  color: #c3d4dd;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(255, 255, 255, 0.12);
  padding: 5px 11px;
  border-radius: 999px;
}

.hero h2 {
  font-size: 27px;
  line-height: 1.34;
  font-weight: 600;
  letter-spacing: -0.3px;
  margin: 16px 0 12px;
}

.hero p {
  font-size: 13px;
  line-height: 1.8;
  color: #a7bac6;
  margin: 0 0 22px;
}

.hero-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

.ghost-btn {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.18);
  color: #fff;
}

.ghost-btn:hover {
  background: rgba(255, 255, 255, 0.16);
  border-color: rgba(255, 255, 255, 0.3);
  color: #fff;
}

.hero-stats {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 14px;
  min-width: 150px;
  padding-left: 28px;
  border-left: 1px solid rgba(255, 255, 255, 0.1);
}

.stat {
  display: flex;
  align-items: baseline;
  gap: 9px;
}

.stat-value {
  font-size: 25px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: #f2f7f9;
}

.stat-label {
  font-size: 12px;
  color: #93a7b4;
}

.hero-note {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  font-size: 10px;
  color: #8aa0ad;
}

/* ---------- 快速开始 ---------- */
.steps {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(232px, 1fr));
  gap: 14px;
}

.step-card {
  padding: 18px;
  display: flex;
  flex-direction: column;
  transition: box-shadow 0.18s;
}

.step-card:hover {
  box-shadow: var(--fm-shadow-lg);
}

.step-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.step-icon {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
  color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.step-index {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 1px;
  color: #d3dbe8;
}

.step-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 7px;
}

.step-desc {
  font-size: 12px;
  line-height: 1.75;
  color: var(--fm-muted);
  flex: 1;
}

.step-link {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-top: 14px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-primary);
}

.step-link :deep(.anticon) {
  font-size: 10px;
}

.guide-tips {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 16px;
  padding: 12px 16px;
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  border: 1px dashed var(--fm-border-strong);
  font-size: 12px;
  color: var(--fm-muted);
}

.tips-label {
  font-weight: 600;
  color: var(--fm-text-2);
}

.tip {
  padding: 1px 7px;
  border-radius: 5px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 11px;
}

.divider {
  width: 1px;
  height: 12px;
  background: var(--fm-border-strong);
}

.faq {
  margin-top: 10px;
  background: transparent;
}

.faq :deep(.ant-collapse-item) {
  border-bottom: 1px solid var(--fm-border);
}

.faq :deep(.ant-collapse-header) {
  font-size: 13px;
  font-weight: 500;
  padding-left: 0 !important;
}

.faq-answer {
  margin: 0;
  font-size: 12px;
  line-height: 1.8;
  color: var(--fm-text-2);
}

/* ---------- 项目 ---------- */
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(248px, 1fr));
  gap: 14px;
}

.project-card {
  padding: 17px 18px;
  cursor: pointer;
  transition: box-shadow 0.18s, transform 0.18s, border-color 0.18s;
}

.project-card:hover {
  box-shadow: var(--fm-shadow-lg);
  border-color: var(--fm-border-strong);
  transform: translateY(-2px);
}

.project-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  flex-shrink: 0;
  box-shadow: 0 0 0 3px rgba(61, 107, 143, 0.1);
}

.project-name {
  font-weight: 600;
  font-size: 14px;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.more {
  color: var(--fm-muted);
  letter-spacing: 2px;
  padding: 0 2px;
}

.project-desc {
  margin-top: 10px;
  color: var(--fm-muted);
  font-size: 12px;
  line-height: 1.7;
  min-height: 38px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.project-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px dashed var(--fm-border);
}

.project-meta {
  font-size: 11px;
  color: #aab3c2;
}

.project-open {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--fm-primary);
  opacity: 0;
  transition: opacity 0.18s;
}

.project-card:hover .project-open {
  opacity: 1;
}

.new-project-card {
  min-height: 148px;
  border-radius: var(--fm-radius);
  border: 1.5px dashed var(--fm-border-strong);
  background: transparent;
  color: var(--fm-muted);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 9px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.18s;
}

.new-project-card:hover {
  border-color: var(--fm-primary);
  color: var(--fm-primary);
  background: rgba(61, 107, 143, 0.04);
}

.empty-projects {
  padding: 48px 24px;
  text-align: center;
}

.empty-illustration {
  font-size: 34px;
  margin-bottom: 12px;
}

.empty-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 6px;
}

.empty-desc {
  font-size: 12px;
  color: var(--fm-muted);
  margin-bottom: 18px;
}

/* ---------- 最近请求 ---------- */
.list {
  overflow: hidden;
}

.history-row {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) 60px 90px 96px;
  align-items: center;
  gap: 12px;
  padding: 11px 16px;
  border-bottom: 1px solid #f1f4f9;
  font-size: 12px;
  transition: background 0.14s;
}

.history-row:last-child {
  border-bottom: none;
}

.history-row:hover {
  background: var(--fm-surface-alt);
}

.url {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fm-text-2);
}

.status,
.time {
  text-align: right;
  font-weight: 600;
}

.when {
  color: var(--fm-muted);
  font-size: 11px;
  text-align: right;
}

.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

@media (max-width: 980px) {
  .hero {
    flex-direction: column;
    gap: 20px;
    padding: 22px 24px;
  }

  .hero-stats {
    flex-direction: row;
    flex-wrap: wrap;
    gap: 18px 28px;
    min-width: 0;
    padding-left: 0;
    padding-top: 18px;
    border-left: none;
    border-top: 1px solid rgba(255, 255, 255, 0.1);
  }

  .hero-note {
    flex-basis: 100%;
  }

  .history-row {
    grid-template-columns: 58px minmax(0, 1fr) 56px 84px;
    gap: 10px;
    padding: 10px 14px;
  }

  .when {
    display: none;
  }
}

@media (max-width: 720px) {
  .hero {
    padding: 18px 18px;
  }

  .hero h2 {
    font-size: 22px;
  }

  .history-row {
    grid-template-columns: 54px minmax(0, 1fr) 52px;
  }

  .time {
    display: none;
  }

  .section-head {
    flex-wrap: wrap;
    gap: 8px;
  }
}
</style>
