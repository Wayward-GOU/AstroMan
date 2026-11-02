<script setup lang="ts">
import { useRouter } from 'vue-router'
import { ThunderboltOutlined } from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import StressReportList from '@/components/StressReportList.vue'

const router = useRouter()
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

    <StressReportList />
  </div>
</template>
