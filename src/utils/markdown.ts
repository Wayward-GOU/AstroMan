import MarkdownIt from 'markdown-it'

/**
 * 轻量 Markdown 渲染：关闭原始 HTML，避免模型输出里的脚本被执行。
 * breaks 打开后单个换行也会变成 <br>，更贴近聊天场景。
 */
const md = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true,
  typographer: false,
})

md.renderer.rules.link_open = (tokens, index, options, env, self) => {
  tokens[index].attrSet('target', '_blank')
  tokens[index].attrSet('rel', 'noopener noreferrer')
  return self.renderToken(tokens, index, options)
}

/**
 * 渲染结果 LRU 缓存。
 *
 * 聊天页是一个大 v-for：任一条消息变化（流式时每 50ms 一次）都会让整块模板
 * 重新执行，于是**所有历史消息**的 renderMarkdown 都会被重新调用，即使它们
 * 的内容一个字节都没变。没有缓存时，每一次都重新走一遍 markdown-it 解析，
 * 并分配等长的新 HTML 字符串，表现为持续的堆增长与 GC 抖动——长会话尤其明显。
 *
 * 用「源文本 → HTML」做有界缓存：历史消息稳定命中，不再重复解析也不产生垃圾；
 * 流式中的那条消息内容每次都在变，命中不到就正常解析，行为与原来一致。
 * 容量上限保证缓存本身不会退化成泄漏。
 */
const CACHE_MAX_ENTRIES = 120
const CACHE_MAX_CHARS = 2_000_000
let cachedChars = 0
const cache = new Map<string, string>()

export function renderMarkdown(source: string): string {
  if (!source) return ''

  const hit = cache.get(source)
  if (hit !== undefined) {
    // 命中后移到队尾，保证常用内容最后被淘汰
    cache.delete(source)
    cache.set(source, hit)
    return hit
  }

  const html = md.render(source)
  cache.set(source, html)
  cachedChars += source.length + html.length

  // 超限时从最旧的一条开始淘汰，直到回到预算内
  while (cache.size > 1 && (cache.size > CACHE_MAX_ENTRIES || cachedChars > CACHE_MAX_CHARS)) {
    const oldest = cache.keys().next().value as string
    const oldHtml = cache.get(oldest) ?? ''
    cachedChars -= oldest.length + oldHtml.length
    cache.delete(oldest)
  }

  return html
}

/** 测试与「内存吃紧」场景使用：清空缓存 */
export function clearMarkdownCache() {
  cache.clear()
  cachedChars = 0
}
