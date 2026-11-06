/**
 * 文本附件预算：单文件与总量双上限。
 *
 * 60k 字符约 15k token，单个大 JSON 已经能吃掉小窗口模型的大半预算；
 * 总量再给 200k 上限，避免一次拖入多个日志把请求直接撑爆。
 */

export const MAX_ATTACH_CHARS = 60_000
export const MAX_ATTACH_TOTAL = 200_000

/** 剩余额度低于这个值时不再接收：只留几百字对模型没有意义 */
const MIN_ACCEPT_CHARS = 1_000

const TEXT_EXTENSIONS = new Set([
  'txt', 'json', 'md', 'yaml', 'yml', 'xml', 'csv', 'log',
  'js', 'ts', 'py', 'java', 'go', 'rs', 'sql', 'sh', 'http', 'har',
])

export interface AttachmentPlan {
  accepted: boolean
  /** 实际可放入上下文的内容（已按上限截断） */
  content: string
  /** 截断 / 拒绝的原因，界面直接提示 */
  warning?: string
}

/** 按「已用字符数」判断新附件能否放入，并给出截断结果 */
export function prepareAttachment(usedChars: number, raw: string): AttachmentPlan {
  const room = MAX_ATTACH_TOTAL - Math.max(0, usedChars)
  if (room < MIN_ACCEPT_CHARS) {
    return {
      accepted: false,
      content: '',
      warning: `附件总量已达 ${MAX_ATTACH_TOTAL} 字符上限，请先移除部分文件`,
    }
  }
  let content = raw
  let warning: string | undefined
  if (content.length > MAX_ATTACH_CHARS) {
    content = content.slice(0, MAX_ATTACH_CHARS)
    warning = `超过单文件 ${MAX_ATTACH_CHARS} 字符，已截断`
  }
  if (content.length > room) {
    content = content.slice(0, room)
    warning = `附件总量超过 ${MAX_ATTACH_TOTAL} 字符，本文件只保留前 ${room} 字符`
  }
  return { accepted: true, content, warning }
}

/** 拖拽入口按扩展名放行，避免把图片 / 压缩包当文本读成乱码 */
export function isTextAttachment(fileName: string): boolean {
  const dot = fileName.lastIndexOf('.')
  if (dot < 0 || dot === fileName.length - 1) return false
  return TEXT_EXTENSIONS.has(fileName.slice(dot + 1).toLowerCase())
}
