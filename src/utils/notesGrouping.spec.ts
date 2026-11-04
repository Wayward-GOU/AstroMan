import { describe, expect, it } from 'vitest'
import { groupDoneNotes, groupOpenNotes, isOverdue } from './notesGrouping'
import type { NoteSummary } from '@/types/notes'

const NOW = new Date(2026, 10, 4, 9, 0, 0).getTime()

function note(id: number, patch: Partial<NoteSummary> = {}): NoteSummary {
  return {
    id,
    title: `任务 ${id}`,
    excerpt: '',
    status: 'open',
    kind: 'todo',
    priority: 1,
    tags: [],
    dueAt: null,
    remindAt: null,
    remindLeadMin: 0,
    repeat: 'none',
    snoozeUntil: null,
    pinned: false,
    doneAt: null,
    updatedAt: NOW,
    attachmentCount: 0,
    ...patch,
  }
}

describe('groupOpenNotes', () => {
  it('按置顶、逾期、今天、之后、无截止分组并跳过空组', () => {
    const groups = groupOpenNotes(
      [
        note(1, { dueAt: NOW - 60_000 }),
        note(2, { dueAt: NOW + 60 * 60 * 1000 }),
        note(3, { dueAt: NOW + 3 * 24 * 60 * 60 * 1000 }),
        note(4),
        note(5, { pinned: true }),
      ],
      NOW,
    )
    expect(groups.map((group) => group.key)).toEqual(['pinned', 'overdue', 'today', 'later', 'none'])
  })

  it('没有条目时返回空数组', () => {
    expect(groupOpenNotes([], NOW)).toEqual([])
  })

  it('逾期判定只看未完成且有截止时间', () => {
    expect(isOverdue(note(1, { dueAt: NOW - 1 }), NOW)).toBe(true)
    expect(isOverdue(note(2, { dueAt: NOW - 1, status: 'done' }), NOW)).toBe(false)
    expect(isOverdue(note(3), NOW)).toBe(false)
  })
})

describe('groupDoneNotes', () => {
  it('按完成日期聚合', () => {
    const groups = groupDoneNotes(
      [
        note(1, { status: 'done', doneAt: NOW - 60_000 }),
        note(2, { status: 'done', doneAt: NOW - 2 * 60 * 1000 }),
        note(3, { status: 'done', doneAt: NOW - 24 * 60 * 60 * 1000 }),
      ],
      NOW,
    )
    expect(groups.map((group) => group.title)).toEqual(['今天完成', '昨天完成'])
    expect(groups[0].items).toHaveLength(2)
  })
})
