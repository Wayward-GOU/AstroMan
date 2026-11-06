<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useHangzaiTaskStore } from '@/stores/hangzaiTask'

defineEmits<{ (event: 'open'): void }>()

const task = useHangzaiTaskStore()
const now = ref(Date.now())
let timer: number | undefined

watch(
  () => task.active,
  (active) => {
    if (active) {
      now.value = Date.now()
      if (!timer) timer = window.setInterval(() => (now.value = Date.now()), 1000)
    } else if (timer) {
      window.clearInterval(timer)
      timer = undefined
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (timer) window.clearInterval(timer)
})

const elapsed = computed(() => {
  const seconds = Math.max(0, Math.floor((now.value - task.startedAt) / 1000))
  const mm = String(Math.floor(seconds / 60)).padStart(2, '0')
  const ss = String(seconds % 60).padStart(2, '0')
  return `${mm}:${ss}`
})
</script>

<template>
  <transition name="task-dock">
    <div v-if="task.active" class="task-dock">
      <span class="task-dot" :class="{ waiting: task.awaitingConfirm }" />
      <div class="task-info">
        <div class="task-title">
          航仔 · {{ task.phaseText }}
          <span v-if="task.round" class="task-round">第 {{ task.round }}/8 步</span>
        </div>
        <div class="task-sub mono">
          {{ task.sessionTitle || '新对话' }} · 已运行 {{ elapsed }}
        </div>
      </div>
      <a-button size="small" type="primary" @click="$emit('open')">
        {{ task.awaitingConfirm ? '去确认' : '查看' }}
      </a-button>
      <a-button v-if="task.canStop" size="small" danger @click="task.requestStop()">
        停止
      </a-button>
    </div>
  </transition>
</template>

<style scoped>
.task-dock {
  position: fixed;
  right: 22px;
  bottom: 22px;
  z-index: 1200;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 300px;
  max-width: min(460px, calc(100vw - 44px));
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--fm-border, rgba(0, 0, 0, 0.1));
  background: var(--fm-surface, #fff);
  box-shadow: 0 10px 28px rgba(15, 23, 42, 0.16);
}

.task-dot {
  width: 8px;
  height: 8px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--fm-primary, #009688);
  animation: task-pulse 1.4s ease-in-out infinite;
}

.task-dot.waiting {
  background: #d97706;
}

.task-info {
  flex: 1;
  min-width: 0;
}

.task-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text, #1f2937);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.task-round {
  font-size: 11px;
  font-weight: 500;
  color: var(--fm-text-2, #6b7280);
}

.task-sub {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fm-text-2, #6b7280);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

@keyframes task-pulse {
  0%,
  100% {
    opacity: 0.45;
    transform: scale(0.88);
  }
  50% {
    opacity: 1;
    transform: scale(1.08);
  }
}

.task-dock-enter-active,
.task-dock-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.task-dock-enter-from,
.task-dock-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
