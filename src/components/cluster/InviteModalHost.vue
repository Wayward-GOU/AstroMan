<script setup lang="ts">
/**
 * 全局邀请弹窗：任何页面收到主节点的加入邀请都在这里提醒。
 * 用户必须显式选择接受或拒绝——不会因为开着应用就被别人拉进压测。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import * as api from '@/api'
import type { InviteNotice } from '@/types/cluster'

const router = useRouter()
const queue = ref<InviteNotice[]>([])
const busy = ref(false)
const now = ref(Date.now())
const unlisteners: UnlistenFn[] = []
let ticker: number | undefined

const current = computed(() => queue.value[0] ?? null)
const visible = computed(() => Boolean(current.value))
const remainSec = computed(() => {
  if (!current.value) return 0
  return Math.max(0, Math.ceil((current.value.expiresAt - now.value) / 1000))
})

function enqueue(notice: InviteNotice) {
  if (queue.value.some((item) => item.requestId === notice.requestId)) return
  queue.value.push(notice)
}

async function respond(accept: boolean) {
  const notice = current.value
  if (!notice || busy.value) return
  busy.value = true
  try {
    const snapshot = await api.clusterInviteRespond(notice.requestId, accept)
    queue.value = queue.value.filter((item) => item.requestId !== notice.requestId)
    if (accept) {
      message.success(`已加入「${notice.masterName}」的压测任务`)
      if (snapshot.status) router.push('/cluster')
    } else {
      message.info('已拒绝加入邀请')
    }
  } catch (error) {
    message.error(String(error))
    // 失败（过期 / 已被处理）时直接收起，避免卡在弹窗上
    queue.value = queue.value.filter((item) => item.requestId !== notice.requestId)
  } finally {
    busy.value = false
  }
}

onMounted(async () => {
  ticker = window.setInterval(() => {
    now.value = Date.now()
    queue.value = queue.value.filter((item) => item.expiresAt > now.value)
  }, 1000)
  try {
    unlisteners.push(
      await listen<InviteNotice>('cluster:invite', (event) => enqueue(event.payload)),
    )
    unlisteners.push(
      await listen<{ requestId: string; accepted: boolean; message: string }>(
        'cluster:invite-result',
        (event) => {
          queue.value = queue.value.filter((item) => item.requestId !== event.payload.requestId)
          if (!event.payload.accepted) message.info(event.payload.message)
        },
      ),
    )
  } catch {
    /* 浏览器预览环境没有 Tauri 事件总线，忽略 */
  }
})

onBeforeUnmount(() => {
  unlisteners.forEach((off) => off())
  if (ticker) window.clearInterval(ticker)
})
</script>

<template>
  <a-modal
    :open="visible"
    title="收到分布式压测邀请"
    :mask-closable="false"
    :closable="false"
    :keyboard="false"
    :footer="null"
    width="460px"
  >
    <div v-if="current" class="invite-card">
      <p class="invite-line">
        <strong>{{ current.masterName }}</strong> 邀请本机加入分布式压测任务
        <strong>「{{ current.jobName }}」</strong>。
      </p>
      <ul class="invite-meta">
        <li>主节点：<span class="mono">{{ current.masterAddr }}</span></li>
        <li>预计分配并发：{{ current.expectedConcurrency }}</li>
        <li>
          邀请有效期：
          <span :class="{ 'invite-expiring': remainSec <= 30 }">{{ remainSec }} 秒</span>
        </li>
      </ul>
      <p class="invite-tip">
        接受后本机会作为施压节点加入该任务；压测期间本机将按主节点下发的并发产生压力。
      </p>
      <div class="invite-actions">
        <a-button :disabled="busy" @click="respond(false)">拒绝</a-button>
        <a-button type="primary" :loading="busy" @click="respond(true)">接受并加入</a-button>
      </div>
      <p v-if="queue.length > 1" class="invite-queue">还有 {{ queue.length - 1 }} 条邀请等待处理</p>
    </div>
  </a-modal>
</template>

<style scoped>
.invite-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.invite-line {
  margin: 0;
  line-height: 1.7;
}
.invite-meta {
  margin: 0;
  padding-left: 18px;
  color: var(--fm-text-2);
  line-height: 1.9;
}
.invite-tip {
  margin: 0;
  color: var(--fm-text-3);
  font-size: 12px;
  line-height: 1.7;
}
.invite-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.invite-expiring {
  color: var(--fm-danger);
  font-weight: 600;
}
.invite-queue {
  margin: 0;
  font-size: 12px;
  color: var(--fm-text-3);
}
.mono {
  font-family: var(--font-mono, monospace);
}
</style>
