# 脚本 API 与加密示例

> 脚本是同步 JavaScript，由本地沙箱执行，不支持 require / import / fetch / async / await / 定时器。可用对象：`request`、`response`（仅后置）、`vars`、`env`、`console`、`rsa`、`cipher`；后置脚本另有 `test(name, fn)` 与 `assert(条件, 提示)`。

## 示例

### 前置脚本：生成签名

```js
// MD5 签名：appId + 时间戳 + 请求体 + 密钥
var timestamp = String(Date.now())
var raw = ['demo-app', timestamp, request.body, env.APP_SECRET].join('.')
request.headers.timestamp = timestamp
request.headers.sign = cipher.md5(raw)
```

```js
// HMAC-SHA256 签名，Base64 输出
var timestamp = String(Date.now())
var sign = cipher.hmacSha256(env.APP_ID + '.' + timestamp, env.APP_SECRET, 'base64')
request.headers.Authorization = 'HMAC ' + env.APP_ID + ':' + timestamp + ':' + sign
```

### 前置脚本：加密请求内容

```js
// AES/CBC/PKCS7（等价 Java 的 AES/CBC/PKCS5Padding），密钥与 IV 放环境变量
var encrypted = cipher.aes.encrypt(request.body, env.AES_KEY, { iv: env.AES_IV })
request.headers['Content-Type'] = 'text/plain'
request.body = encrypted
```

```js
// RSA 公钥加密，写入变量后在 Header 里用 {{authCode}} 引用
var raw = env.APP_ID + '.' + env.USER_CODE + '.' + Date.now()
vars.set('authCode', rsa.publicEncrypt(raw, env.PUBLIC_KEY))
```

```js
// AES/ECB、3DES、Hex 输出、ZeroPadding 等按选项切换
cipher.aes.encrypt(text, key, { mode: 'ecb' })
cipher.aes.encrypt(text, key, { iv: iv, output: 'hex' })
cipher.tripleDes.encrypt(text, key24, { iv: iv8 })
cipher.des.encrypt(text, key8, { iv: iv8 })
```

### 前置脚本：直接改请求

```js
request.method = 'POST'
request.params.page = '1'
request.headers['X-Trace-Id'] = 'trace-' + Date.now()
request.body = JSON.stringify({ id: Date.now(), name: 'fink' })
```

### 后置脚本：断言与取变量

```js
test('HTTP 200', () => assert(response.status === 200))
test('业务码为 0', () => {
  var data = response.json()
  assert(data.code === 0, '实际业务码：' + data.code)
})
vars.set('accessToken', response.json().token) // 后续请求用 {{accessToken}}
```

```js
// 解密服务端返回的密文
var plain = cipher.aes.decrypt(response.body, env.AES_KEY, { iv: env.AES_IV })
console.log('解密结果：' + plain)
```

> 加密约定：密钥长度必须精确（AES 16/24/32、DES 8、3DES 24 字节），长度不符会直接报错；密钥与 IV 也可用 `{ keyFormat: 'hex' }` / `{ ivFormat: 'base64' }` 指定格式。RSA 默认为 PKCS#1 v1.5 填充，与 Java 默认的 `Cipher.getInstance("RSA")` 一致。

---



## 内置变量速查

| 类别 | 变量 |
|---|---|
| 时间 | `{{timestamp}}`（毫秒）`{{timestampSec}}` `{{isoTime}}` `{{datetime}}` `{{date}}` `{{time}}` `{{yyyyMMdd}}` `{{yyyyMMddHHmmss}}` |
| 随机 | `{{uuid}}` `{{uuidNoDash}}` `{{randomInt(1,100)}}` `{{randomFloat(1,10,2)}}` `{{randomString(8)}}` `{{randomString(8, hex)}}` `{{randomHex(16)}}` `{{randomNumber(6)}}` `{{randomBool}}` `{{randomEmail}}` `{{randomPhone}}` `{{randomIPv4}}` |

所有内置变量都支持 Postman 的 `$` 前缀，例如 `{{$timestamp}}`、`{{$uuid}}`。变量优先级：运行时变量（`vars.set`）> 环境变量 > 内置变量。

## vars / env

| API | 说明 |
|---|---|
| `vars.set(key, value)` | 写入运行时变量，请求里用 `{{key}}` 引用 |
| `vars.get / has / unset / all` | 读取、判断、删除变量，`all()` 返回合并后的全部变量 |
| `env.KEY` | 只读读取当前环境变量，修改请用 `vars.set` |
