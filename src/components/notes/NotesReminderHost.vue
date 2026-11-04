<script setup lang="ts">
import { h, onBeforeUnmount, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { Button, notification } from 'ant-design-vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { notesSnooze, notesToggle } from '@/api/notes'
import { useNotesStore } from '@/stores/notes'
import type { NotesReminderEvent, NotesReminderItem } from '@/types/notes'

const KEY = 'notes-reminder'

const router = useRouter()
const store = useNotesStore()

let unlisten: UnlistenFn | null = null

function dueText(item: NotesReminderItem): string {
  if (item.dueAt === null) return item.overdue ? '已逾期' : '待办到期'
  const date = new Date(item.dueAt)
  const text = `${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(
    2,
    '0',
  )} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  return item.overdue ? `${text} · 已逾期` : text
}

function close() {
  notification.close(KEY)
}

async function complete(id: number) {
  try {
    await notesToggle(id, true)
    close()
    await store.refreshCounts()
  } catch {
    // 失败时保留通知，让用户重试
  }
}

async function snooze(id: number) {
  try {
    await notesSnooze(id, 10)
    close()
    await store.refreshCounts()
  } catch {
    // 失败时保留通知
  }
}

async function snoozeAll(ids: number[]) {
  try {
    await Promise.all(ids.map((id) => notesSnooze(id, 10)))
    close()
    await store.refreshCounts()
  } catch {
    // 失败时保留通知
  }
}

function openNotes() {
  close()
  router.push('/notes')
}

function show(event: NotesReminderEvent) {
  if (!store.settings.notifyEnabled) return
  const items = event.items ?? []
  if (!items.length) return
  void store.refreshCounts()

  if (items.length === 1) {
    const item = items[0]
    notification.open({
      key: KEY,
      message: item.title,
      description: dueText(item),
      duration: 0,
      class: 'notes-reminder-card',
      placement: 'bottomRight',
      btn: () =>
        h('div', { style: 'display:flex;gap:6px;flex-wrap:wrap' }, [
          h(
            Button,
            { size: 'small', type: 'primary', onClick: () => void complete(item.id) },
            () => '完成',
          ),
          h(Button, { size: 'small', onClick: () => void snooze(item.id) }, () => '稍后 10 分钟'),
          h(Button, { size: 'small', type: 'text', onClick: openNotes }, () => '打开'),
        ]),
    })
    return
  }

  const preview = items
    .slice(0, 3)
    .map((item) => `· ${item.title}`)
    .join('\n')
  const more = items.length > 3 ? `\n…另有 ${items.length - 3} 条` : ''
  const ids = items.map((item) => item.id)
  notification.open({
    key: KEY,
    message: `${items.length} 条待办到期`,
    description: `${preview}${more}`,
    duration: 0,
    class: 'notes-reminder-card',
    placement: 'bottomRight',
    btn: () =>
      h('div', { style: 'display:flex;gap:6px;flex-wrap:wrap' }, [
        h(Button, { size: 'small', type: 'primary', onClick: openNotes }, () => '打开记事本'),
        h(
          Button,
          { size: 'small', onClick: () => void snoozeAll(ids) },
          () => '全部稍后 10 分钟',
        ),
      ]),
  })
}

onMounted(async () => {
  try {
    unlisten = await listen<NotesReminderEvent>('notes:reminder', (event) => show(event.payload))
  } catch {
    // 浏览器预览没有 Tauri 事件，直接跳过
  }
})

onBeforeUnmount(() => {
  unlisten?.()
  unlisten = null
  close()
})
</script>

<template>
  <span />
</template>

<style scoped>
:global(.ant-notification-notice.notes-reminder-card) {
  border-left: 4px solid var(--fm-primary);
  border-radius: var(--fm-radius-lg);
  background: linear-gradient(135deg, var(--fm-primary-soft), var(--fm-surface) 62%);
  box-shadow: var(--fm-shadow-lg);
}

:global(.ant-notification-notice.notes-reminder-card .ant-notification-notice-message) {
  color: var(--fm-text);
  font-weight: 600;
}
</style>
