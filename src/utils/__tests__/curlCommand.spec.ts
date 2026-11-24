import { describe, expect, it } from 'vitest'
import { buildCurl, extractCurlCommand, previewRequest } from '../curl'
import type { EnvVariable, HttpRequestDetail, RequestAuth } from '@/types/models'

function auth(patch: Partial<RequestAuth> = {}): RequestAuth {
  return {
    kind: 'none',
    token: '',
    username: '',
    password: '',
    key: '',
    value: '',
    addTo: 'header',
    ...patch,
  }
}

function detail(patch: Partial<HttpRequestDetail> = {}): HttpRequestDetail {
  return {
    collectionId: 1,
    name: '测试接口',
    method: 'GET',
    url: 'https://api.example.com/items',
    headers: [],
    params: [],
    bodyType: 'none',
    body: '',
    preScript: '',
    postScript: '',
    assertions: '',
    ...patch,
  }
}

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

describe('buildCurl', () => {
  it('把 Bearer Auth 写进 Authorization 头', () => {
    const code = buildCurl(detail({ auth: auth({ kind: 'bearer', token: 'abc' }) }))
    expect(code).toContain("-H 'Authorization: Bearer abc'")
  })

  it('Basic Auth 用 -u，避免占位符提前被 base64', () => {
    const code = buildCurl(
      detail({ auth: auth({ kind: 'basic', username: '{{user}}', password: '{{pass}}' }) }),
    )
    expect(code).toContain("-u '{{user}}:{{pass}}'")
    expect(code).not.toContain('Authorization')
  })

  it('API Key 放到查询参数，且不编码 {{变量}}', () => {
    const code = buildCurl(
      detail({
        url: 'https://api.example.com/x',
        params: [{ key: 'page', value: '{{pageNo}}', enabled: true }],
        auth: auth({ kind: 'apikey', addTo: 'query', key: 'apiKey', value: '{{apiKey}}' }),
      }),
    )
    expect(code).toContain('https://api.example.com/x?page={{pageNo}}&apiKey={{apiKey}}')
  })

  it('手写 Authorization 优先于 Auth 配置', () => {
    const code = buildCurl(
      detail({
        headers: [{ key: 'Authorization', value: 'Token manual', enabled: true }],
        auth: auth({ kind: 'bearer', token: 'abc' }),
      }),
    )
    expect(code).toContain('Authorization: Token manual')
    expect(code).not.toContain('Bearer abc')
  })

  it('环境变量仍能在导出结果里替换', () => {
    const variables: EnvVariable[] = [
      { key: 'word', value: 'a b', isSecret: false, sortOrder: 1 },
    ]
    const code = buildCurl(
      detail({ params: [{ key: 'q', value: '{{word}}', enabled: true }] }),
      variables,
    )
    expect(code).toContain('q=a%20b')
  })
})

describe('previewRequest', () => {
  it('把变量替换与 Query 拼接算进最终地址', () => {
    const variables: EnvVariable[] = [
      { key: 'host', value: 'api.example.com', isSecret: false, sortOrder: 1 },
      { key: 'id', value: '42', isSecret: false, sortOrder: 2 },
    ]
    const preview = previewRequest(
      detail({
        url: 'https://{{host}}/items',
        params: [
          { key: 'id', value: '{{id}}', enabled: true },
          { key: 'skip', value: 'me', enabled: false },
        ],
      }),
      variables,
    )
    expect(preview.url).toBe('https://api.example.com/items?id=42')
    expect(preview.unresolved).toEqual([])
  })

  it('标注请求头来源：手填 / 认证注入 / 自动补 Content-Type', () => {
    const preview = previewRequest(
      detail({
        method: 'post',
        headers: [{ key: 'X-Trace', value: 't-1', enabled: true }],
        auth: auth({ kind: 'bearer', token: 'abc' }),
        bodyType: 'json',
        body: '{"a":1}',
      }),
    )
    expect(preview.method).toBe('POST')
    const byKey = Object.fromEntries(preview.headers.map((item) => [item.key, item]))
    expect(byKey['X-Trace'].source).toBe('user')
    expect(byKey['Authorization']).toMatchObject({ value: 'Bearer abc', source: 'auth' })
    expect(byKey['Content-Type']).toMatchObject({ value: 'application/json', source: 'auto' })
    expect(preview.bodyBytes).toBe(7)
  })

  it('未定义变量列出待补，敏感变量不展开', () => {
    const variables: EnvVariable[] = [
      { key: 'token', value: '••••••', isSecret: true, sortOrder: 1 },
    ]
    const preview = previewRequest(
      detail({
        url: 'https://api.example.com/{{missing}}/x',
        headers: [{ key: 'Authorization', value: 'Bearer {{token}}', enabled: true }],
      }),
      variables,
    )
    expect(preview.unresolved).toEqual(['missing'])
    expect(preview.secretVars).toEqual(['token'])
    expect(preview.headers.find((item) => item.key === 'Authorization')?.value).toBe(
      'Bearer {{token}}',
    )
  })

  it('文件上传类不统计正文字节', () => {
    const preview = previewRequest(
      detail({ bodyType: 'binary', body: '/tmp/big.zip' }),
    )
    expect(preview.bodyBytes).toBe(0)
  })
})
