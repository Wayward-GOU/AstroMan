import { describe, expect, it } from 'vitest'
import { queryJsonPath, jsonPathHits } from '../jsonpath'
import { jsonDiff } from '../jsondiff'
import { inferJsonSchema } from '../schema'
import { ascii85Decode, ascii85Encode, base58Decode, base58Encode, quotedPrintableDecode, quotedPrintableEncode } from '../encoding'
import { maskText } from '../mask'
import { generateData, validateBankCard, validateIdCard, validatePhone, validateUscc } from '../datagen'
import { decodeUlid, generateNanoId, generateUlid } from '../idgen'
import { parseHttpMessage, parseUserAgent } from '../netparse'
import { totp } from '../otp'
import { verifyJwt } from '../jwtverify'
import { searchTools, TOOL_CATEGORIES, TOOLS, TOOL_MAP } from '../registry'

const SAMPLE = `{
  "code": 0,
  "data": {
    "list": [
      { "id": 1, "name": "张三" },
      { "id": 2, "name": "李四" }
    ]
  }
}`

describe('工具注册表', () => {
  it('id 唯一且都有渲染形态', () => {
    const ids = TOOLS.map((tool) => tool.id)
    expect(new Set(ids).size).toBe(ids.length)
    TOOLS.forEach((tool) => expect(Boolean(tool.simple) || Boolean(tool.component)).toBe(true))
  })

  it('分类合法且工具总数符合预期', () => {
    const categoryIds = new Set(TOOL_CATEGORIES.map((item) => item.id))
    TOOLS.forEach((tool) => expect(categoryIds.has(tool.category)).toBe(true))
    expect(TOOLS.length).toBe(92)
    expect(TOOL_MAP.size).toBe(TOOLS.length)
  })

  it('simple 工具的选项默认值类型与控件一致', () => {
    TOOLS.forEach((tool) => {
      tool.simple?.options?.forEach((option) => {
        if (option.type === 'number') expect(typeof option.default).toBe('number')
        if (option.type === 'switch') expect(typeof option.default).toBe('boolean')
        if (option.type === 'select') {
          expect(option.options?.some((item) => item.value === option.default)).toBe(true)
        }
      })
    })
  })

  it('别名可被搜索命中', () => {
    expect(searchTools('jp').some((tool) => tool.id === 'jsonpath')).toBe(true)
    expect(searchTools('打码').some((tool) => tool.id === 'mask')).toBe(true)
    expect(searchTools('b85').some((tool) => tool.id === 'ascii85')).toBe(true)
    expect(searchTools('ua解析').some((tool) => tool.id === 'ua-parse')).toBe(true)
  })
})

describe('JSONPath 查询', () => {
  it('按路径取值', () => {
    const hits = jsonPathHits(SAMPLE, '$.data.list[0].name')
    expect(hits).toHaveLength(1)
    expect(hits[0].value).toBe('张三')
  })

  it('通配符批量取值', () => {
    const hits = jsonPathHits(SAMPLE, '$.data.list[*].id')
    expect(hits.map((item) => item.value)).toEqual([1, 2])
  })

  it('递归下降', () => {
    const hits = jsonPathHits(SAMPLE, '$..name')
    expect(hits).toHaveLength(2)
  })

  it('无命中时给出可读提示', () => {
    expect(queryJsonPath(SAMPLE, '$.nope')).toContain('没有匹配到内容')
  })
})

describe('JSON 结构化对比', () => {
  it('识别修改、新增与删除', () => {
    const output = jsonDiff('{"a":1,"b":2}\n---\n{"a":9,"c":3}')
    expect(output).toContain('$.a: 1 → 9')
    expect(output).toContain('+ $.c')
    expect(output).toContain('- $.b')
  })

  it('缺少分隔符时报错', () => {
    expect(() => jsonDiff('{"a":1}')).toThrow('分隔行')
  })

  it('完全一致时直接说明', () => {
    expect(jsonDiff('{"a":[1,2]}\n---\n{"a":[1,2]}')).toContain('完全一致')
  })
})

describe('JSON Schema 推断', () => {
  it('推断对象结构、必填与类型', () => {
    const schema = JSON.parse(inferJsonSchema('{"id":1,"price":9.9,"tags":["a"]}'))
    expect(schema.type).toBe('object')
    expect(schema.properties.id.type).toBe('integer')
    expect(schema.properties.price.type).toBe('number')
    expect(schema.properties.tags.type).toBe('array')
    expect(schema.required).toEqual(['id', 'price', 'tags'])
  })

  it('识别字符串格式', () => {
    const schema = JSON.parse(
      inferJsonSchema('{"mail":"a@b.com","when":"2026-10-30T10:00:00Z","home":"https://x.dev"}'),
    )
    expect(schema.properties.mail.format).toBe('email')
    expect(schema.properties.when.format).toBe('date-time')
    expect(schema.properties.home.format).toBe('uri')
  })

  it('数组合并对象字段', () => {
    const schema = JSON.parse(inferJsonSchema('[{"a":1},{"a":2,"b":"x"}]'))
    expect(Object.keys(schema.properties ?? {})).toEqual([])
    expect(schema.type).toBe('array')
    expect(Object.keys(schema.items.properties)).toEqual(['a', 'b'])
    expect(schema.items.required).toEqual(['a'])
  })
})

describe('编码工具', () => {
  it('Base58 往返', () => {
    const text = 'AstroMan 工具箱'
    expect(base58Decode(base58Encode(text))).toBe(text)
  })

  it('Base58 拒绝非法字符', () => {
    expect(() => base58Decode('0OIl')).toThrow('非法 Base58')
  })

  it('Ascii85 往返并带定界符', () => {
    const text = 'The quick brown fox 你好'
    const encoded = ascii85Encode(text)
    expect(encoded.startsWith('<~')).toBe(true)
    expect(ascii85Decode(encoded)).toBe(text)
  })

  it('Quoted-Printable 往返', () => {
    const text = '你好，AstroMan = 工具'
    expect(quotedPrintableDecode(quotedPrintableEncode(text))).toBe(text)
  })
})

describe('文本脱敏', () => {
  it('自动识别手机号、身份证与邮箱', () => {
    const output = maskText('手机 13800138000，身份证 110101199003071234，邮箱 zhangsan@example.com')
    expect(output).toContain('138****8000')
    expect(output).toContain('110101********1234')
    expect(output).toContain('z***@example.com')
  })

  it('指定规则时只处理该规则', () => {
    const output = maskText('13800138000 / 110101199003071234', 'phone')
    expect(output).toContain('138****8000')
    expect(output).toContain('110101199003071234')
  })

  it('没有可脱敏内容时原样返回', () => {
    expect(maskText('hello world')).toContain('未识别到需要脱敏的内容')
  })
})

describe('造数与证件校验', () => {
  it('生成的身份证校验位正确', () => {
    const rows = JSON.parse(generateData('idcard', 8, 'json')) as { value: string }[]
    rows.forEach((row) => expect(validateIdCard(row.value).ok).toBe(true))
  })

  it('生成的银行卡通过 Luhn 校验', () => {
    const rows = JSON.parse(generateData('bankcard', 8, 'json')) as { value: string }[]
    rows.forEach((row) => expect(validateBankCard(row.value).ok).toBe(true))
  })

  it('生成的统一社会信用代码校验位正确', () => {
    const rows = JSON.parse(generateData('uscc', 8, 'json')) as { value: string }[]
    rows.forEach((row) => expect(validateUscc(row.value).ok).toBe(true))
  })

  it('生成的手机号格式合法', () => {
    const rows = JSON.parse(generateData('phone', 8, 'json')) as { value: string }[]
    rows.forEach((row) => expect(validatePhone(row.value).ok).toBe(true))
  })

  it('错误的身份证校验位会被拒绝', () => {
    expect(validateIdCard('110101199003071235').ok).toBe(false)
  })

  it('CSV 与 SQL 输出结构正确', () => {
    expect(generateData('name', 2, 'csv').split('\n')).toHaveLength(3)
    expect(generateData('name', 2, 'sql')).toContain('INSERT INTO t_test_data')
  })
})

describe('ID 生成', () => {
  it('ULID 长度 26 且可还原时间', () => {
    const ulid = generateUlid(1700000000000)
    expect(ulid).toHaveLength(26)
    expect(ulid).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/)
    expect(decodeUlid(ulid)).toBe(1700000000000)
  })

  it('NanoID 按指定长度生成且字符集安全', () => {
    const id = generateNanoId(32)
    expect(id).toHaveLength(32)
    expect(id).toMatch(/^[A-Za-z0-9_-]+$/)
  })
})

describe('网络文本解析', () => {
  it('识别 Edge 与 Windows', () => {
    const ua =
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0'
    const output = parseUserAgent(ua)
    expect(output).toContain('Edge')
    expect(output).toContain('Windows 11 / 10')
  })

  it('识别微信内置浏览器与 Android', () => {
    const ua =
      'Mozilla/5.0 (Linux; Android 13; PGT-AN10) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0 Mobile Safari/537.36 MicroMessenger/8.0.42'
    const output = parseUserAgent(ua)
    expect(output).toContain('微信内置浏览器 8.0.42')
    expect(output).toContain('Android 手机')
  })

  it('解析原始请求行、查询参数与请求头', () => {
    const raw = 'GET /api/users?page=2&size=20 HTTP/1.1\nHost: api.example.com\nAccept: */*\n\n'
    const parsed = parseHttpMessage(raw)
    expect(parsed.kind).toBe('request')
    expect(parsed.method).toBe('GET')
    expect(parsed.url).toBe('/api/users?page=2&size=20')
    expect(parsed.headers).toHaveLength(2)
  })

  it('解析状态行', () => {
    const parsed = parseHttpMessage('HTTP/1.1 404 Not Found\nContent-Type: application/json\n\n{"msg":"x"}')
    expect(parsed.kind).toBe('response')
    expect(parsed.status).toBe('404 Not Found')
  })
})

describe('TOTP', () => {
  it('通过 RFC 6238 SHA-1 测试向量（T=59）', async () => {
    const secret = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'
    const result = await totp(secret, { digits: 8, timestamp: 59000 })
    expect(result.code).toBe('94287082')
  })

  it('剩余秒数在周期范围内', async () => {
    const result = await totp('JBSWY3DPEHPK3PXP', { period: 30, timestamp: 61000 })
    expect(result.remaining).toBeGreaterThan(0)
    expect(result.remaining).toBeLessThanOrEqual(30)
  })
})

describe('JWT 验签', () => {
  async function signHs256(payload: Record<string, unknown>, secret: string): Promise<string> {
    const encoder = new TextEncoder()
    const part = (value: string) => Buffer.from(value).toString('base64url')
    const header = part(JSON.stringify({ alg: 'HS256', typ: 'JWT' }))
    const body = part(JSON.stringify(payload))
    const key = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, [
      'sign',
    ])
    const signature = new Uint8Array(await crypto.subtle.sign('HMAC', key, encoder.encode(`${header}.${body}`)))
    return `${header}.${body}.${Buffer.from(signature).toString('base64url')}`
  }

  it('HS256 签名通过校验', async () => {
    const token = await signHs256({ sub: '1001', exp: Math.floor(Date.now() / 1000) + 600 }, 'secret-key')
    const output = await verifyJwt(token, 'secret-key')
    expect(output).toContain('通过')
    expect(output).toContain('有效')
  })

  it('篡改后签名不通过', async () => {
    const token = await signHs256({ sub: '1001' }, 'secret-key')
    const parts = token.split('.')
    const tampered = `${parts[0]}.${Buffer.from('{"sub":"9999"}').toString('base64url')}.${parts[2]}`
    const output = await verifyJwt(tampered, 'secret-key')
    expect(output).toContain('未通过')
  })

  it('过期令牌给出过期状态', async () => {
    const token = await signHs256({ exp: Math.floor(Date.now() / 1000) - 3600 }, 'k')
    const output = await verifyJwt(token, 'k')
    expect(output).toContain('已过期')
  })
})
