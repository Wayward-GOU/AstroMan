/**
 * 脚本页的示例代码，供用户一键插入后按需修改。
 * 示例只依赖脚本沙箱内置对象（request / response / vars / env / console / rsa / cipher），可直接运行。
 */

export interface ScriptExample {
  label: string
  code: string
}

const join = (...lines: string[]) => lines.join('\n')

export const PRE_SCRIPT_EXAMPLES: ScriptExample[] = [
  {
    label: '时间戳 + nonce',
    code: join(
      '// 每个请求自动带上时间戳与随机串，服务端可用于防重放校验',
      'request.headers.timestamp = String(Date.now())',
      "request.headers.nonce = Math.random().toString(36).slice(2, 10)",
    ),
  },
  {
    label: 'MD5 签名头',
    code: join(
      '// 常见规则：appId + 时间戳 + 请求体 + 密钥 拼成待签名串（需先配好 APP_SECRET）',
      'var timestamp = String(Date.now())',
      "var raw = ['demo-app', timestamp, request.body, env.APP_SECRET].join('.')",
      'request.headers.timestamp = timestamp',
      'request.headers.sign = cipher.md5(raw)',
    ),
  },
  {
    label: 'HMAC-SHA256 签名',
    code: join(
      '// 生成 Base64 形式的 HMAC 签名，可换成 hmacSha1 与对应算法',
      'var timestamp = String(Date.now())',
      "var sign = cipher.hmacSha256(env.APP_ID + '.' + timestamp, env.APP_SECRET, 'base64')",
      "request.headers.Authorization = 'HMAC ' + env.APP_ID + ':' + timestamp + ':' + sign",
    ),
  },
  {
    label: 'AES 加密请求体',
    code: join(
      '// AES/CBC/PKCS7，等价 Java 的 AES/CBC/PKCS5Padding；密钥与 IV 建议放环境变量',
      'var encrypted = cipher.aes.encrypt(request.body, env.AES_KEY, { iv: env.AES_IV })',
      "request.headers['Content-Type'] = 'text/plain'",
      'request.body = encrypted',
    ),
  },
  {
    label: 'RSA 加密敏感字段',
    code: join(
      '// 公钥加密后写入变量，Header 里用 {{authCode}} 引用即可',
      "var raw = env.APP_ID + '.' + env.USER_CODE + '.' + Date.now()",
      "vars.set('authCode', rsa.publicEncrypt(raw, env.PUBLIC_KEY))",
    ),
  },
  {
    label: '追加参数与请求头',
    code: join(
      '// 也可以直接改 request.params / request.url / request.method',
      "request.params.page = '1'",
      "request.params.size = '20'",
      "request.headers['X-Trace-Id'] = 'trace-' + Date.now()",
    ),
  },
]

export const POST_SCRIPT_EXAMPLES: ScriptExample[] = [
  {
    label: '断言状态码与业务码',
    code: join(
      '// test() 里的断言失败不会中断脚本，会逐条记录在「测试」页',
      "test('HTTP 200', () => assert(response.status === 200))",
      "test('业务码为 0', () => {",
      '  var data = response.json()',
      "  assert(data.code === 0, '实际业务码：' + data.code)",
      '})',
    ),
  },
  {
    label: '提取 Token 到变量',
    code: join(
      '// 后续请求可直接用 {{accessToken}} 引用',
      'var data = response.json()',
      'vars.set(\'accessToken\', data.token)',
      "console.log('已写入运行时变量 accessToken')",
    ),
  },
  {
    label: '断言响应耗时',
    code: join(
      '// 也可以用 response.sizeBytes、response.statusText',
      "test('响应时间小于 800ms', () => assert(response.durationMs < 800, '实际 ' + response.durationMs + ' ms'))",
    ),
  },
  {
    label: 'AES 解密响应',
    code: join(
      '// 服务端返回密文时，本地解密后记录到变量便于核对',
      'var plain = cipher.aes.decrypt(response.body, env.AES_KEY, { iv: env.AES_IV })',
      "console.log('解密结果：' + plain)",
      "vars.set('decryptedBody', plain)",
    ),
  },
]
