import type { NoteRepeat } from '@/types/notes'

/**
 * 「记一笔」的自然语言解析：纯本地、无网络请求。
 *
 * 规则保守——只提取能确定的部分，不认识的写法原样留在标题里。
 * 明确写了「今天 10 点」这类已经过去的时间时不自动顺延，交给列表显示「已过」。
 */
export interface QuickParseResult {
  title: string
  dueAt: number | null
  repeat: NoteRepeat
  tags: string[]
  priority: number
  /** 输入框下方的预览标签 */
  labels: string[]
}

interface TimeOfDay {
  hour: number
  minute: number
  /** 时间段词自带的默认小时（今晚 / 明早），用于没有显式时间时兜底 */
  periodDefault: number | null
}

const WEEKDAY_MAP: Record<string, number> = {
  一: 1,
  二: 2,
  三: 3,
  四: 4,
  五: 5,
  六: 6,
  日: 0,
  天: 0,
}

const REPEAT_LABEL: Record<NoteRepeat, string> = {
  none: '',
  daily: '每天',
  weekdays: '工作日',
  weekly: '每周',
  monthly: '每月',
}

function take(text: string, pattern: RegExp): { rest: string; match: RegExpMatchArray | null } {
  const match = text.match(pattern)
  if (!match || match.index === undefined) return { rest: text, match: null }
  const rest = `${text.slice(0, match.index)} ${text.slice(match.index + match[0].length)}`
  return { rest, match }
}

function atTime(date: Date, hour: number, minute: number): number {
  const next = new Date(date)
  next.setHours(hour, minute, 0, 0)
  return next.getTime()
}

function daysInMonth(year: number, month: number): number {
  return new Date(year, month + 1, 0).getDate()
}

function clampDay(year: number, month: number, day: number): number {
  return Math.min(day, daysInMonth(year, month))
}

/** 下一次该星期，严格晚于 now */
function nextWeekdayAt(target: number, hour: number, minute: number, now: Date): number {
  const base = new Date(now)
  const delta = (target - base.getDay() + 7) % 7
  base.setDate(base.getDate() + delta)
  base.setHours(hour, minute, 0, 0)
  if (base.getTime() <= now.getTime()) base.setDate(base.getDate() + 7)
  return base.getTime()
}

/** 下一次该月日（目标月没有该日取月末），严格晚于 now */
function nextMonthlyAt(day: number, hour: number, minute: number, now: Date): number {
  let year = now.getFullYear()
  let month = now.getMonth()
  for (let i = 0; i < 24; i += 1) {
    const safeDay = clampDay(year, month, day)
    const candidate = atTime(new Date(year, month, safeDay), hour, minute)
    if (candidate > now.getTime()) return candidate
    month += 1
    if (month > 11) {
      month = 0
      year += 1
    }
  }
  return now.getTime() + 24 * 60 * 60 * 1000
}

/** 只写了重复词时的首次触发时间：落在未来，避免刚保存就逾期 */
function repeatAnchor(
  repeat: NoteRepeat,
  weekday: number | null,
  monthDay: number | null,
  hour: number,
  minute: number,
  now: Date,
): number {
  if (repeat === 'weekly') {
    return nextWeekdayAt(weekday ?? now.getDay(), hour, minute, now)
  }
  if (repeat === 'monthly') {
    return nextMonthlyAt(monthDay ?? now.getDate(), hour, minute, now)
  }
  if (repeat === 'weekdays') {
    let candidate = atTime(now, hour, minute)
    if (candidate <= now.getTime()) candidate += 24 * 60 * 60 * 1000
    let cursor = new Date(candidate)
    for (let i = 0; i < 7; i += 1) {
      const day = cursor.getDay()
      if (day !== 0 && day !== 6) break
      cursor = new Date(cursor.getTime() + 24 * 60 * 60 * 1000)
    }
    return cursor.getTime()
  }
  if (now.getHours() > hour || (now.getHours() === hour && now.getMinutes() >= minute)) {
    return atTime(now, hour, minute) + 24 * 60 * 60 * 1000
  }
  return atTime(now, hour, minute)
}

function formatClock(hour: number, minute: number): string {
  return `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`
}

/** 截止时间的可读文案：今天 / 明天 / 后天 + 时刻，其余带日期 */
export function formatDueLabel(ts: number, now = Date.now()): string {
  const target = new Date(ts)
  const today = new Date(now)
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()
  const dayDiff = Math.floor(
    (new Date(target.getFullYear(), target.getMonth(), target.getDate()).getTime() - start) /
      (24 * 60 * 60 * 1000),
  )
  const clock = formatClock(target.getHours(), target.getMinutes())
  if (dayDiff === 0) return `今天 ${clock}`
  if (dayDiff === 1) return `明天 ${clock}`
  if (dayDiff === 2) return `后天 ${clock}`
  if (dayDiff === -1) return `昨天 ${clock}`
  const month = String(target.getMonth() + 1).padStart(2, '0')
  const day = String(target.getDate()).padStart(2, '0')
  const year = target.getFullYear() === today.getFullYear() ? '' : `${target.getFullYear()}-`
  return `${year}${month}-${day} ${clock}`
}

export function parseQuickNote(raw: string, now = new Date()): QuickParseResult {
  let text = raw
  const labels: string[] = []
  let priority = 1
  let repeat: NoteRepeat = 'none'
  let weekday: number | null = null
  let monthDay: number | null = null
  let explicitDate: number | null = null
  let time: TimeOfDay | null = null
  let periodDefault: number | null = null
  let dayPeriod: 'am' | 'pm' | null = null

  // 标签：#工作 / ＃工作，最多 20 个
  const tags: string[] = []
  const tagPattern = /[#＃]([^\s#＃,，。；;]+)/g
  for (const match of text.matchAll(tagPattern)) {
    const tag = match[1].trim()
    if (tag && tag.length <= 32 && !tags.includes(tag) && tags.length < 20) tags.push(tag)
  }
  text = text.replace(tagPattern, ' ')

  // 优先级：!! / !（全角 ！）
  const priorityMatch = text.match(/[!！]{1,2}(?=\s|$)/)
  if (priorityMatch) {
    priority = 2
    text = text.replace(priorityMatch[0], ' ')
  }

  // 重复：优先吃掉「每周X / 每月X号」，普通日期词才不会被误判
  const weekly = take(text, /每(?:周|星期)([一二三四五六日天])/)
  if (weekly.match) {
    repeat = 'weekly'
    weekday = WEEKDAY_MAP[weekly.match[1]]
    text = weekly.rest
  } else {
    const monthly = take(text, /每(?:个)?月(\d{1,2})[号日]/)
    if (monthly.match) {
      repeat = 'monthly'
      monthDay = Number(monthly.match[1])
      text = monthly.rest
    } else {
      const daily = take(text, /每天|每日/)
      if (daily.match) {
        repeat = 'daily'
        text = daily.rest
      } else {
        const workdays = take(text, /工作日|每个工作日/)
        if (workdays.match) {
          repeat = 'weekdays'
          text = workdays.rest
        } else {
          const plainWeekly = take(text, /每(?:周|星期)/)
          if (plainWeekly.match) {
            repeat = 'weekly'
            text = plainWeekly.rest
          } else {
            const plainMonthly = take(text, /每(?:个)?月/)
            if (plainMonthly.match) {
              repeat = 'monthly'
              text = plainMonthly.rest
            }
          }
        }
      }
    }
  }

  // 具体日期：2026-11-05 / 11月5日 / 下月3号
  const iso = take(text, /(\d{4})[-/年](\d{1,2})[-/月](\d{1,2})[日号]?/)
  if (iso.match) {
    explicitDate = new Date(
      Number(iso.match[1]),
      Number(iso.match[2]) - 1,
      Number(iso.match[3]),
    ).getTime()
    text = iso.rest
  } else {
    const nextMonth = take(text, /下个?月(\d{1,2})[号日]/)
    if (nextMonth.match) {
      const base = new Date(now.getFullYear(), now.getMonth() + 1, 1)
      const day = clampDay(base.getFullYear(), base.getMonth(), Number(nextMonth.match[1]))
      explicitDate = new Date(base.getFullYear(), base.getMonth(), day).getTime()
      text = nextMonth.rest
    } else {
      const monthDayMatch = take(text, /(\d{1,2})月(\d{1,2})[号日]/)
      if (monthDayMatch.match) {
        explicitDate = new Date(
          now.getFullYear(),
          Number(monthDayMatch.match[1]) - 1,
          Number(monthDayMatch.match[2]),
        ).getTime()
        text = monthDayMatch.rest
      }
    }
  }

  // 相对日期
  if (explicitDate === null) {
    const relative = take(text, /大后天|后天|明天|明日|今天|今日|今晚|今夜|明晚|今早|明早/)
    if (relative.match) {
      const token = relative.match[0]
      const base = new Date(now)
      let offset = 0
      if (token.includes('后') && token !== '后天') offset = 3
      else if (token.includes('后')) offset = 2
      else if (token.startsWith('明')) offset = 1
      if (token.endsWith('晚') || token.endsWith('夜')) {
        offset = token === '今晚' || token === '今夜' ? 0 : 1
        periodDefault = 20
        dayPeriod = 'pm'
      } else if (token.endsWith('早')) {
        offset = token === '今早' ? 0 : 1
        periodDefault = 8
        dayPeriod = 'am'
      } else {
        periodDefault = null
      }
      base.setDate(base.getDate() + offset)
      explicitDate = new Date(base.getFullYear(), base.getMonth(), base.getDate()).getTime()
      text = relative.rest
    }
  }

  // 星期：下周一 / 周三 / 星期日
  if (explicitDate === null) {
    const weekdayMatch = take(text, /(下)?(?:周|星期)([一二三四五六日天])/)
    if (weekdayMatch.match) {
      const isNext = Boolean(weekdayMatch.match[1])
      const target = WEEKDAY_MAP[weekdayMatch.match[2]]
      const base = new Date(now)
      const delta = (target - base.getDay() + 7) % 7
      const offset = isNext ? (delta === 0 ? 7 : delta) : delta
      base.setDate(base.getDate() + offset)
      explicitDate = new Date(base.getFullYear(), base.getMonth(), base.getDate()).getTime()
      if (repeat === 'none') weekday = target
      text = weekdayMatch.rest
    }
  }

  // 时刻：下午3点 / 10:30 / 晚上8点半
  const clock = take(text, /(上午|下午|晚上|傍晚|中午|凌晨)?\s*(\d{1,2})\s*[点:：时]\s*(半|\d{1,2})?\s*分?/)
  if (clock.match) {
    const period = clock.match[1] ?? ''
    let hour = Number(clock.match[2])
    const minuteToken = clock.match[3]
    const minute = minuteToken === '半' ? 30 : Number(minuteToken ?? 0)
    if (period === '下午' || period === '晚上' || period === '傍晚') {
      if (hour < 12) hour += 12
    } else if (period === '中午') {
      if (hour < 11) hour += 12
    } else if (period === '凌晨') {
      if (hour === 12) hour = 0
    } else if (dayPeriod === 'pm' && hour < 12) {
      // 「今晚 8 点」的晚间信息来自日期词，补上 12 小时
      hour += 12
    } else if (dayPeriod === 'am' && hour === 12) {
      hour = 0
    }
    if (hour >= 0 && hour <= 23 && minute >= 0 && minute <= 59) {
      time = {
        hour,
        minute,
        periodDefault: period === '晚上' || period === '傍晚' ? 20 : null,
      }
      text = clock.rest
    }
  }

  let dueAt: number | null = null
  if (explicitDate !== null) {
    const hour = time?.hour ?? periodDefault ?? 18
    const minute = time?.minute ?? 0
    dueAt = atTime(new Date(explicitDate), hour, minute)
  } else if (time) {
    if (repeat !== 'none') {
      dueAt = repeatAnchor(repeat, weekday, monthDay, time.hour, time.minute, now)
    } else {
      dueAt = atTime(now, time.hour, time.minute)
    }
  } else if (repeat !== 'none') {
    dueAt = repeatAnchor(repeat, weekday, monthDay, periodDefault ?? 18, 0, now)
  }

  // 标题：去掉已识别部分，压缩空白与首尾标点
  const title = text
    .replace(/[，,。;；、]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()

  if (dueAt !== null) labels.push(`截止 ${formatDueLabel(dueAt, now.getTime())}`)
  if (repeat !== 'none') {
    const suffix = repeat === 'weekly' && weekday !== null
      ? `周${'日一二三四五六'[weekday]}`
      : repeat === 'monthly' && monthDay !== null
        ? `${monthDay} 号`
        : ''
    labels.push(`${REPEAT_LABEL[repeat]}${suffix}`)
  }
  tags.forEach((tag) => labels.push(`#${tag}`))
  if (priority === 2) labels.push('高优先级')

  return { title, dueAt, repeat, tags, priority, labels }
}
