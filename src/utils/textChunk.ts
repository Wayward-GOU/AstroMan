/**
 * 大文本分块。
 *
 * 背景：响应正文上限 5MB，如果整段塞进单个 `<pre>`，浏览器要对全部字符做折行排版，
 * 主线程会被占满（表现为界面卡死几秒）。这里把正文切成若干块，每块是一个独立的
 * `<pre>`，追加新块时已渲染的块不会被重新排版，代价只和单块大小相关。
 *
 * 切点优先落在换行符之后，保证块边界在整行之间；找不到换行（单行超长）才退化到
 * 按字符切，此时靠 `word-break: break-all` 保持视觉连续。
 */

export interface ChunkOptions {
  /** 首块字符数，决定打开响应的首屏排版成本 */
  initialChars: number
  /** 后续每块字符数 */
  stepChars: number
}

export const DEFAULT_CHUNK_OPTIONS: ChunkOptions = {
  initialChars: 96 * 1024,
  stepChars: 128 * 1024,
}

/**
 * 计算从 `from` 开始的一块文本的结束位置（不含）。
 *
 * - 一定会返回大于 `from` 的值，保证调用方能推进
 * - 剩余内容不足一块时返回 `text.length`
 * - 优先返回换行符的下一位，让块边界落在行之间
 */
export function nextChunkEnd(text: string, from: number, want: number): number {
  const total = text.length
  if (from >= total) return total
  const step = Math.max(1, Math.floor(want) || 1)
  const limit = Math.min(total, from + step)
  if (limit >= total) return total
  const newline = text.lastIndexOf('\n', limit - 1)
  // 只有换行明显落在当前块内才采用，避免切出 1 个字符的碎块
  return newline > from ? newline + 1 : limit
}

/** 按「首块 + 若干后续块」的规则切完整个文本，供测试与离线场景使用 */
export function splitChunks(text: string, options: ChunkOptions = DEFAULT_CHUNK_OPTIONS): string[] {
  const chunks: string[] = []
  let cursor = 0
  let size = options.initialChars
  while (cursor < text.length) {
    const end = nextChunkEnd(text, cursor, size)
    chunks.push(text.slice(cursor, end))
    cursor = end
    size = options.stepChars
  }
  return chunks
}

/** 字符数展示：与字节数区分开，避免中文场景下体积被低估 */
export function formatCharCount(n: number): string {
  if (n < 1000) return `${n} 字符`
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k 字符`
  return `${(n / 1_000_000).toFixed(2)}M 字符`
}
