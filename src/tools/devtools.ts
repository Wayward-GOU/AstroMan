/* 开发辅助：Cron 表达式解析、雪花 ID 解析 */

/* ---------------- Cron 解析 ---------------- */

const CRON_LABELS = ['分钟', '小时', '日', '月', '星期']

interface CronField {
  min: number
  max: number
  unit: string
}

const CRON_FIELDS: CronField[] = [
  { min: 0, max: 59, unit: '分' },
  { min: 0, max: 23, unit: '时' },
  { min: 1, max: 31, unit: '日' },
  { min: 1, max: 12, unit: '月' },
  { min: 0, max: 7, unit: '周' },
]

const WEEK_NAMES = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

function expandCronField(expr: string, field: CronField, index: number): Set<number> {
  const values = new Set<number>()
  const push = (raw: number) => {
    const value = index === 4 && raw === 7 ? 0 : raw
    if (value < field.min || value > field.max) {
      throw new Error(`${CRON_LABELS[index]}字段超出范围（${field.min}-${field.max}）：${raw}`)
    }
    values.add(value)
  }
  expr.split(',').forEach((part) => {
    const token = part.trim()
    if (!token) throw new Error(`第 ${index + 1} 个字段为空`)
    const [range, stepText] = token.split('/')
    const step = stepText === undefined ? 1 : Number(stepText)
    if (!Number.isInteger(step) || step <= 0) throw new Error(`步长不合法：${token}`)
    let start = field.min
    let end = field.max
    if (range !== '*') {
      if (range.includes('-')) {
        const [begin, finish] = range.split('-').map((item) => Number(item))
        if (!Number.isInteger(begin) || !Number.isInteger(finish) || begin > finish) {
          throw new Error(`区间不合法：${token}`)
        }
        start = begin
        end = finish
      } else {
        const single = Number(range)
        if (!Number.isInteger(single)) throw new Error(`数值不合法：${token}`)
        start = single
        end = stepText === undefined ? single : field.max
      }
    }
    for (let value = start; value <= end; value += step) push(value)
  })
  return values
}

function describeField(expr: string, index: number): string {
  if (expr === '*') return index === 4 ? '每天' : `每${CRON_LABELS[index]}`
  const stepMatch = /^\*\/(\d+)$/.exec(expr)
  if (stepMatch) return `每 ${stepMatch[1]} ${CRON_LABELS[index]}`
  const rangeMatch = /^(\d+)-(\d+)$/.exec(expr)
  if (rangeMatch) {
    if (index === 4) return `${WEEK_NAMES[Number(rangeMatch[1]) % 7]}至${WEEK_NAMES[Number(rangeMatch[2]) % 7]}`
    return `${rangeMatch[1]}-${rangeMatch[2]} ${CRON_LABELS[index]}`
  }
  const single = Number(expr)
  if (Number.isInteger(single)) {
    if (index === 4) return WEEK_NAMES[single % 7]
    return `第 ${single} ${CRON_LABELS[index]}`
  }
  return expr
}

export function cronInfo(input: string, count = 5): string {
  const raw = input.trim()
  if (!raw) throw new Error('请输入 Cron 表达式，例如 0 9 * * 1-5')
  const parts = raw.split(/\s+/)
  if (parts.length !== 5) throw new Error('需要 5 个字段：分钟 小时 日 月 星期')

  const sets = parts.map((part, index) => expandCronField(part, CRON_FIELDS[index], index))
  const [minutes, hours, days, months, weekdays] = sets

  const rows = parts
    .map((part, index) => `${CRON_LABELS[index]}：${part}（${describeField(part, index)}）`)
    .join('\n')

  const domAny = parts[2] === '*'
  const dowAny = parts[4] === '*'
  const matchDay = (date: Date): boolean => {
    if (!months.has(date.getMonth() + 1)) return false
    const domHit = days.has(date.getDate())
    const dowHit = weekdays.has(date.getDay())
    if (domAny && dowAny) return true
    if (domAny) return dowHit
    if (dowAny) return domHit
    return domHit || dowHit
  }

  const runs: string[] = []
  const cursor = new Date()
  cursor.setSeconds(0, 0)
  cursor.setMinutes(cursor.getMinutes() + 1)
  const limit = 60 * 24 * 366 * 2
  for (let step = 0; step < limit && runs.length < Math.max(1, count); step += 1) {
    if (hours.has(cursor.getHours()) && minutes.has(cursor.getMinutes()) && matchDay(cursor)) {
      const pad = (value: number) => String(value).padStart(2, '0')
      runs.push(
        `${cursor.getFullYear()}-${pad(cursor.getMonth() + 1)}-${pad(cursor.getDate())} ` +
          `${pad(cursor.getHours())}:${pad(cursor.getMinutes())}:00 ${WEEK_NAMES[cursor.getDay()]}`,
      )
    }
    cursor.setMinutes(cursor.getMinutes() + 1)
  }

  return [
    `表达式：${raw}`,
    '',
    '字段解析：',
    rows,
    '',
    runs.length ? `接下来 ${runs.length} 次执行（本机时区）：` : '未来两年内没有匹配的执行时间：',
    ...runs.map((run, index) => `  ${index + 1}. ${run}`),
  ].join('\n')
}

/* ---------------- 雪花 ID 解析 ---------------- */

function epochMs(text: string): number {
  const raw = text.trim()
  if (!raw) return 1288834974657
  if (/^\d{13}$/.test(raw)) return Number(raw)
  if (/^\d{10}$/.test(raw)) return Number(raw) * 1000
  const date = new Date(raw.replace(/-/g, '/'))
  if (Number.isNaN(date.getTime())) throw new Error(`无法识别起始时间：${text}`)
  return date.getTime()
}

export function snowflakeInfo(input: string, epoch = '2010-11-04 16:35:00', workerBits = 10): string {
  const raw = input.trim()
  if (!/^\d+$/.test(raw)) throw new Error('请输入纯数字的雪花 ID')
  const bits = workerBits >= 10 ? 10 : 5
  const datacenterBits = bits === 10 ? 5 : 0
  const workerIdBits = bits - datacenterBits
  const sequenceBits = 12
  const totalBits = 1 + 41 + bits + sequenceBits
  const value = BigInt(raw)
  if (value >= 1n << BigInt(totalBits)) throw new Error(`雪花 ID 超出 ${totalBits} 位上限`)

  const mask = (width: number) => (1n << BigInt(width)) - 1n
  const sequence = Number(value & mask(sequenceBits))
  const workerId = Number((value >> BigInt(sequenceBits)) & mask(workerIdBits))
  const datacenter = datacenterBits
    ? Number((value >> BigInt(sequenceBits + workerIdBits)) & mask(datacenterBits))
    : 0
  const base = epochMs(epoch)
  const timestamp = Number(value >> BigInt(bits + sequenceBits)) + base

  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) throw new Error('时间戳超出有效范围')
  const pad = (number: number) => String(number).padStart(2, '0')
  const local =
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds())}`

  const lines = [
    `雪花 ID：${raw}`,
    `生成时间：${local}`,
    `毫秒时间戳：${timestamp}`,
    `距起始时间偏移：${timestamp - base} ms`,
    `起始时间：${new Date(base).toLocaleString('zh-CN')}`,
  ]
  if (datacenterBits) lines.push(`数据中心 ID（${datacenterBits} 位）：${datacenter}`)
  lines.push(`工作节点 ID（${workerIdBits} 位）：${workerId}`)
  lines.push(`序列号（${sequenceBits} 位）：${sequence}`)
  lines.push(`位数构成：1 + 41 + ${bits} + ${sequenceBits} = ${totalBits} 位`)
  return lines.join('\n')
}
