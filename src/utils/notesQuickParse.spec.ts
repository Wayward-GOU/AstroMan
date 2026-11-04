import { describe, expect, it } from 'vitest'
import { formatDueLabel, parseQuickNote } from './notesQuickParse'

const NOW = new Date(2026, 10, 4, 9, 0, 0) // 2026-11-04 09:00 本地时间

describe('parseQuickNote', () => {
  it('解析相对日期、时刻与标签', () => {
    const result = parseQuickNote('明天10点 交周报 #工作', NOW)
    expect(result.title).toBe('交周报')
    expect(result.tags).toEqual(['工作'])
    expect(result.repeat).toBe('none')
    expect(result.priority).toBe(1)
    expect(new Date(result.dueAt!).getTime()).toBe(new Date(2026, 10, 5, 10, 0, 0).getTime())
    expect(result.labels.some((label) => label.startsWith('截止 明天 10:00'))).toBe(true)
  })

  it('今晚 N 点按晚间折算成 24 小时制', () => {
    const result = parseQuickNote('今晚8点 取快递', NOW)
    expect(result.title).toBe('取快递')
    expect(new Date(result.dueAt!).getTime()).toBe(new Date(2026, 10, 4, 20, 0, 0).getTime())
  })

  it('每周 X 设置重复并取未来的下一次', () => {
    const result = parseQuickNote('每周一 9:30 站会', NOW)
    expect(result.title).toBe('站会')
    expect(result.repeat).toBe('weekly')
    const due = new Date(result.dueAt!)
    expect(due.getDay()).toBe(1)
    expect(due.getHours()).toBe(9)
    expect(due.getMinutes()).toBe(30)
    expect(due.getTime()).toBeGreaterThan(NOW.getTime())
  })

  it('下月 N 号保留 18:00 且叹号表示高优先级', () => {
    const result = parseQuickNote('下月3号 续费域名 !', NOW)
    expect(result.title).toBe('续费域名')
    expect(result.priority).toBe(2)
    const due = new Date(result.dueAt!)
    expect(due.getMonth()).toBe(11)
    expect(due.getDate()).toBe(3)
    expect(due.getHours()).toBe(18)
  })

  it('工作日重复落在未来的工作日', () => {
    const result = parseQuickNote('工作日 9点 看邮件', NOW)
    expect(result.title).toBe('看邮件')
    expect(result.repeat).toBe('weekdays')
    const due = new Date(result.dueAt!)
    expect(due.getDay()).not.toBe(0)
    expect(due.getDay()).not.toBe(6)
    expect(due.getHours()).toBe(9)
    expect(due.getTime()).toBeGreaterThan(NOW.getTime())
  })

  it('没有时间词时保留原样且不设置截止', () => {
    const result = parseQuickNote('网关报错排查结论', NOW)
    expect(result.title).toBe('网关报错排查结论')
    expect(result.dueAt).toBeNull()
    expect(result.repeat).toBe('none')
    expect(result.labels).toEqual([])
  })

  it('只有时间词时标题为空，交给界面拒绝保存', () => {
    const result = parseQuickNote('10点', NOW)
    expect(result.title).toBe('')
    expect(result.dueAt).not.toBeNull()
  })

  it('明确写了过去的时间不顺延', () => {
    const later = new Date(2026, 10, 4, 15, 0, 0)
    const result = parseQuickNote('今天10点 开会', later)
    const due = new Date(result.dueAt!)
    expect(due.getHours()).toBe(10)
    expect(due.getTime()).toBeLessThan(later.getTime())
  })

  it('具体日期与下午时刻组合', () => {
    const result = parseQuickNote('11月5日 下午3点 评审 #项目', NOW)
    expect(result.title).toBe('评审')
    expect(result.tags).toEqual(['项目'])
    expect(new Date(result.dueAt!).getTime()).toBe(new Date(2026, 10, 5, 15, 0, 0).getTime())
  })
})

describe('formatDueLabel', () => {
  it('今天 / 明天 / 昨天使用相对文案', () => {
    const now = new Date(2026, 10, 4, 9, 0, 0).getTime()
    expect(formatDueLabel(new Date(2026, 10, 4, 18, 0).getTime(), now)).toBe('今天 18:00')
    expect(formatDueLabel(new Date(2026, 10, 5, 8, 5).getTime(), now)).toBe('明天 08:05')
    expect(formatDueLabel(new Date(2026, 10, 3, 20, 0).getTime(), now)).toBe('昨天 20:00')
  })
})
