import { describe, expect, it } from 'vitest'
import { extractCurlCommand } from '../curl'

describe('extractCurlCommand', () => {
  it('从代码块里取出完整命令，保留换行与长 Body', () => {
    const body = JSON.stringify({ rows: Array.from({ length: 80 }, (_, i) => ({ id: i, v: 'x'.repeat(40) })) })
    const command = `curl 'https://api.example.com/batch' \\\n  -H 'content-type: application/json' \\\n  --data-raw '${body}'`
    const text = `帮我导入这个接口：\n\n\`\`\`bash\n${command}\n\`\`\`\n\n谢谢`
    const picked = extractCurlCommand(text)
    expect(picked).toBe(command)
    // 完整保留：结尾是 Body 的收尾引号，而不是省略号
    expect(picked?.endsWith("'")).toBe(true)
    expect(picked?.length).toBe(command.length)
  })

  it('正文里直接出现的 curl 也能取出', () => {
    const text = 'curl https://api.example.com/ping -H "accept: */*"'
    expect(extractCurlCommand(text)).toBe('curl https://api.example.com/ping -H "accept: */*"')
  })

  it('没有 curl 时返回 null', () => {
    expect(extractCurlCommand('帮我看看这个接口的耗时')).toBeNull()
    expect(extractCurlCommand('')).toBeNull()
  })

  it('优先取含 curl 的代码块，而不是外层说明文字', () => {
    const text = [
      '命令如下：',
      '```shell',
      "curl 'https://a.example.com/x' -X POST",
      '```',
      '上面这条即可',
    ].join('\n')
    expect(extractCurlCommand(text)).toBe("curl 'https://a.example.com/x' -X POST")
  })
})
