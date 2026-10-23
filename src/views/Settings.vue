<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { message } from 'ant-design-vue'
import {
  DatabaseOutlined,
  SafetyCertificateOutlined,
  ThunderboltOutlined,
  CopyOutlined,
  LaptopOutlined,
  BgColorsOutlined,
  CheckOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import * as api from '@/api'
import { themes, currentThemeId, setTheme } from '@/stores/theme'
import type { AppInfo } from '@/types/models'

const info = ref<AppInfo | null>(null)

onMounted(async () => {
  try {
    info.value = await api.appInfo()
  } catch (e) {
    message.error(String(e))
  }
})

async function copy(text?: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.warning('当前环境不允许访问剪贴板')
  }
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Settings"
      title="设置"
      subtitle="本地版不收集任何数据，所有内容只保存在这台电脑上"
    />

    <div class="grid">
      <div class="fm-card block theme-block">
        <div class="block-head">
          <span class="block-icon blue"><BgColorsOutlined /></span>
          <div>
            <div class="block-title">外观主题</div>
            <div class="block-desc">选择配色方案，切换后立即生效并保存在本机</div>
          </div>
        </div>
        <div class="theme-list">
          <button
            v-for="item in themes"
            :key="item.id"
            class="theme-item"
            :class="{ active: currentThemeId === item.id }"
            @click="setTheme(item.id)"
          >
            <span
              class="theme-preview"
              :style="{ background: item.swatches[0], borderColor: item.swatches[2] }"
            >
              <span class="theme-side" :style="{ background: item.swatches[1] }" />
              <span class="theme-dot" :style="{ background: item.swatches[2] }" />
              <span class="theme-line" :style="{ background: item.swatches[3] }" />
            </span>
            <span class="theme-name">{{ item.name }}</span>
            <span class="theme-desc">{{ item.desc }}</span>
            <CheckOutlined v-if="currentThemeId === item.id" class="theme-check" />
          </button>
        </div>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon blue"><LaptopOutlined /></span>
          <div>
            <div class="block-title">应用信息</div>
            <div class="block-desc">当前版本与本地运行环境</div>
          </div>
        </div>
        <div class="rows">
          <div class="row">
            <span class="label">版本</span>
            <span class="value mono">FinkMeter {{ info?.version || '-' }}</span>
          </div>
          <div class="row">
            <span class="label">运行模式</span>
            <span class="value">纯本地桌面应用（Tauri）</span>
          </div>
          <div class="row">
            <span class="label">数据目录</span>
            <span class="value mono path">{{ info?.dataDir || '-' }}</span>
            <a-button type="text" size="small" class="copy" @click="copy(info?.dataDir)">
              <template #icon><CopyOutlined /></template>
            </a-button>
          </div>
          <div class="row">
            <span class="label">数据库文件</span>
            <span class="value mono path">{{ info?.dbPath || '-' }}</span>
            <a-button type="text" size="small" class="copy" @click="copy(info?.dbPath)">
              <template #icon><CopyOutlined /></template>
            </a-button>
          </div>
        </div>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon green"><SafetyCertificateOutlined /></span>
          <div>
            <div class="block-title">数据与隐私</div>
            <div class="block-desc">所有数据都在本机，可随时备份</div>
          </div>
        </div>
        <ul class="list">
          <li>
            项目、集合、请求、环境变量、历史记录和压测报告全部存放在上方的 SQLite 文件中，
            不依赖任何云端后端。
          </li>
          <li>不需要注册账号，没有登录态、权限体系与付费限制。</li>
          <li>备份方式：退出应用后复制数据库文件；换电脑时放回同名路径即可。</li>
          <li>卸载应用不会自动删除数据目录，如需彻底清理请手动删除。</li>
        </ul>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon purple"><ThunderboltOutlined /></span>
          <div>
            <div class="block-title">AI 辅助</div>
            <div class="block-desc">用自然语言生成请求与脚本</div>
          </div>
          <span class="soon">即将开放</span>
        </div>
        <div class="ai-hint">
          下一阶段将支持配置你自己的 OpenAI 兼容服务（Base URL / 模型 / API Key），
          用一句话描述接口即可生成请求、参数与断言脚本。API Key 只会保存在本机系统钥匙串，不会上传。
        </div>
        <a-button disabled>配置 AI</a-button>
      </div>

      <div class="fm-card block">
        <div class="block-head">
          <span class="block-icon amber"><DatabaseOutlined /></span>
          <div>
            <div class="block-title">快捷键与技巧</div>
            <div class="block-desc">日常使用中最常用的操作</div>
          </div>
        </div>
        <div class="shortcuts">
          <div class="shortcut"><span class="keys"><kbd>Enter</kbd></span><span>在 URL 输入框直接发送请求</span></div>
          <div class="shortcut"><span class="keys"><kbd v-pre>{{base_url}}</kbd></span><span>在 URL / Headers / Body 中引用环境变量</span></div>
          <div class="shortcut"><span class="keys"><kbd v-pre>{{randomInt(1,100)}}</kbd></span><span>生成范围内的随机整数，用于造测试数据</span></div>
          <div class="shortcut"><span class="keys"><kbd>停止</kbd></span><span>压测中可随时停止，数据仍会保存成报告</span></div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
  gap: 16px;
  align-items: start;
}

.block {
  padding: 20px 22px;
}

.theme-block {
  grid-column: 1 / -1;
}

.theme-list {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 12px;
}

.theme-item {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 10px;
  border: 1px solid var(--fm-border);
  border-radius: 10px;
  background: var(--fm-surface);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.16s, box-shadow 0.16s;
}

.theme-item:hover {
  border-color: var(--fm-border-strong);
}

.theme-item.active {
  border-color: var(--fm-primary);
  box-shadow: 0 0 0 2px var(--fm-primary-soft);
}

.theme-preview {
  position: relative;
  display: block;
  height: 46px;
  margin-bottom: 5px;
  border: 1px solid;
  border-radius: 6px;
  overflow: hidden;
}

.theme-side {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  width: 14px;
}

.theme-dot {
  position: absolute;
  top: 10px;
  left: 22px;
  width: 20px;
  height: 6px;
  border-radius: 3px;
}

.theme-line {
  position: absolute;
  top: 24px;
  left: 22px;
  right: 10px;
  height: 4px;
  border-radius: 2px;
  opacity: 0.32;
}

.theme-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
}

.theme-desc {
  font-size: 10px;
  color: var(--fm-muted);
}

.theme-check {
  position: absolute;
  top: 12px;
  right: 12px;
  font-size: 12px;
  color: var(--fm-primary);
}

.block-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 16px;
}

.block-icon {
  width: 34px;
  height: 34px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  flex-shrink: 0;
}

.block-icon.blue {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.block-icon.green {
  background: var(--fm-success-soft);
  color: var(--fm-success);
}

.block-icon.purple {
  background: var(--fm-purple-soft);
  color: var(--fm-purple);
}

.block-icon.amber {
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
}

.block-title {
  font-size: 14px;
  font-weight: 600;
}

.block-desc {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-muted);
}

.soon {
  margin-left: auto;
  font-size: 10px;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  padding: 1px 8px;
  border-radius: 999px;
}

.rows {
  display: flex;
  flex-direction: column;
}

.row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 0;
  border-bottom: 1px dashed var(--fm-border);
  font-size: 12px;
}

.row:last-child {
  border-bottom: none;
}

.label {
  width: 84px;
  flex-shrink: 0;
  color: var(--fm-muted);
  font-size: 11px;
}

.value {
  flex: 1;
  min-width: 0;
  word-break: break-all;
  color: var(--fm-text-2);
}

.path {
  font-size: 11px;
  line-height: 1.6;
}

.copy {
  color: var(--fm-muted);
  flex-shrink: 0;
}

.list {
  margin: 0;
  padding-left: 18px;
}

.list li {
  font-size: 12px;
  line-height: 1.9;
  color: var(--fm-text-2);
}

.ai-hint {
  font-size: 12px;
  line-height: 1.9;
  color: var(--fm-text-2);
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--fm-surface-alt);
  border: 1px dashed var(--fm-border-strong);
  margin-bottom: 14px;
}

.shortcuts {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.shortcut {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--fm-text-2);
}

.keys {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 128px;
  flex-shrink: 0;
}

kbd {
  display: inline-block;
  padding: 2px 7px;
  border-radius: 5px;
  border: 1px solid var(--fm-border-strong);
  border-bottom-width: 2px;
  background: var(--fm-surface-alt);
  font-family: 'JetBrains Mono', Consolas, monospace;
  font-size: 10px;
  color: var(--fm-text-2);
}
</style>
