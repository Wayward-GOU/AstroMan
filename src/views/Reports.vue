<script setup lang="ts">
import { useRouter } from 'vue-router'
import { ThunderboltOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import StressReportList from '@/components/StressReportList.vue'

const router = useRouter()

function switchTab(key: string | number) {
  if (key === 'stress') router.push('/stress')
  else if (key === 'plans') router.push('/plans')
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="Reports"
      title="压测报告"
      subtitle="每次压测结束后自动生成，包含吞吐、延迟分位与错误明细"
    >
      <a-button type="primary" @click="router.push('/stress')">
        <template #icon><ThunderboltOutlined /></template>
        发起压测
      </a-button>
    </PageHeader>

    <a-tabs :active-key="'reports'" size="small" class="stress-top-tabs" @change="switchTab">
      <a-tab-pane key="stress" tab="压测" />
      <a-tab-pane key="plans" tab="计划" />
      <a-tab-pane key="reports" tab="报告" />
    </a-tabs>

    <StressReportList />
  </div>
</template>

<style scoped>
.stress-top-tabs {
  margin: -8px 0 14px;
}

.stress-top-tabs :deep(.ant-tabs-nav) {
  margin: 0;
}
</style>
