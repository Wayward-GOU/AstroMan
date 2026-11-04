import type { NoteSummary } from '@/types/notes'

export type NoteGroupKey = 'pinned' | 'overdue' | 'today' | 'later' | 'none'

export interface NoteGroup {
  key: NoteGroupKey
  title: string
  items: NoteSummary[]
}

const GROUP_TITLE: Record<NoteGroupKey, string> = {
  pinned: '置顶',
  overdue: '逾期',
  today: '今天',
  later: '之后',
  none: '无截止',
}

function startOfDay(ts: number): number {
  const date = new Date(ts)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

/** 未完成条目按「置顶 → 逾期 → 今天 → 之后 → 无截止」分组，空分组跳过 */
export function groupOpenNotes(items: NoteSummary[], now = Date.now()): NoteGroup[] {
  const pinned: NoteSummary[] = []
  const overdue: NoteSummary[] = []
  const today: NoteSummary[] = []
  const later: NoteSummary[] = []
  const none: NoteSummary[] = []
  const tomorrow = startOfDay(now) + 24 * 60 * 60 * 1000

  items.forEach((item) => {
    if (item.pinned) {
      pinned.push(item)
      return
    }
    if (item.dueAt === null) {
      none.push(item)
      return
    }
    if (item.dueAt < now) {
      overdue.push(item)
      return
    }
    if (item.dueAt < tomorrow) {
      today.push(item)
      return
    }
    later.push(item)
  })

  const buckets: [NoteGroupKey, NoteSummary[]][] = [
    ['pinned', pinned],
    ['overdue', overdue],
    ['today', today],
    ['later', later],
    ['none', none],
  ]
  return buckets
    .filter(([, list]) => list.length > 0)
    .map(([key, list]) => ({ key, title: GROUP_TITLE[key], items: list }))
}

export interface DoneGroup {
  title: string
  items: NoteSummary[]
}

/** 已完成条目按完成日期分组：今天 / 昨天 / 具体日期 */
export function groupDoneNotes(items: NoteSummary[], now = Date.now()): DoneGroup[] {
  const todayStart = startOfDay(now)
  const groups: DoneGroup[] = []
  items.forEach((item) => {
    const doneAt = item.doneAt ?? item.updatedAt
    const dayStart = startOfDay(doneAt)
    const diff = Math.round((todayStart - dayStart) / (24 * 60 * 60 * 1000))
    let title = ''
    if (diff === 0) title = '今天完成'
    else if (diff === 1) title = '昨天完成'
    else {
      const date = new Date(doneAt)
      title = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(
        date.getDate(),
      ).padStart(2, '0')}`
    }
    const last = groups[groups.length - 1]
    if (last && last.title === title) last.items.push(item)
    else groups.push({ title, items: [item] })
  })
  return groups
}

/** 「今天」视图下，按截止时间判断是否需要「已逾期」标记 */
export function isOverdue(item: NoteSummary, now = Date.now()): boolean {
  return item.status === 'open' && item.dueAt !== null && item.dueAt < now
}
