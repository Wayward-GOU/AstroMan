const pad = (value: number, size = 2) => String(value).padStart(size, '0')

/** 统一时间展示格式：YYYY-MM-DD HH:MM:SS */
export function formatDateTime(value?: number | string | Date | null): string {
  if (value === null || value === undefined || value === '') return '-'
  const date = value instanceof Date ? value : new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  )
}

/** 仅用于图表横轴等空间受限的场景：HH:MM */
export function formatMinute(value?: number | null): string {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`
}
