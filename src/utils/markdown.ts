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

export function renderMarkdown(source: string): string {
  return md.render(source)
}
