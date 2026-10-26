import type { ToolCategory, ToolDef, OptionState } from './types'
import { toolHash, toolHmac } from '@/api'
import {
  asciiDecode,
  asciiEncode,
  base32Decode,
  base32Encode,
  base64Decode,
  base64Encode,
  base64UrlDecode,
  base64UrlEncode,
  convertCase,
  dedupeLines,
  escapeString,
  extractText,
  gunzipFromBase64,
  gzipToBase64,
  hexDecode,
  hexEncode,
  htmlEscape,
  htmlUnescape,
  ipToLong,
  lineOps,
  longToIp,
  macInfo,
  morseDecode,
  morseEncode,
  rc4,
  replaceText,
  reverseText,
  rmbUpper,
  textStats,
  toFullWidth,
  toHalfWidth,
  unescapeString,
  unicodeEscape,
  unicodeUnescape,
} from './transforms'
import {
  csvToJson,
  jsonToCsv,
  jsonToJsonl,
  jsonToMarkdown,
  jsonToQuery,
  jsonToSql,
  jsonToXml,
  jsonToYaml,
  jsonlToJson,
  queryToJson,
  xmlToJson,
  yamlToJson,
} from './converters'
import { jsonToEntity, sqlToJava } from './entity'
import { crc16, crc32, dateCalc, luhnCheck, pxRem, randomPassword, randomString, uuidV4 } from './calc'
import { signJwt } from './crypto'
import { cronInfo, snowflakeInfo } from './devtools'
import { urlInfo } from './network'
import type { CaseStyle, ExtractKind, LineOp } from './transforms'

export const TOOL_CATEGORIES: ToolCategory[] = [
  { id: 'json', name: 'JSON 工具', hint: '格式化、互转、生成实体类' },
  { id: 'format', name: '格式化转换', hint: '代码美化与压缩' },
  { id: 'crypto', name: '加密编码', hint: '哈希、AES、Base64、JWT' },
  { id: 'text', name: '文本处理', hint: '命名、统计、对比、替换' },
  { id: 'calc', name: '计算换算', hint: '进制、单位、颜色、时间戳' },
  { id: 'dev', name: '开发辅助', hint: 'MyBatis、正则、随机数据' },
  { id: 'network', name: '网络工具', hint: 'IP、子网、端口' },
  { id: 'qr', name: '二维码', hint: '生成美化、扫码识别' },
  { id: 'reference', name: '查询手册', hint: 'ASCII、状态码、MIME' },
]

function requireText(input: string): string {
  if (!input.trim()) throw new Error('请输入内容')
  return input
}

function opt(options: OptionState, key: string, fallback = ''): string {
  const value = options[key]
  if (value === undefined || value === null) return fallback
  return String(value)
}

function flag(options: OptionState, key: string): boolean {
  return Boolean(options[key])
}

const SAMPLE_JSON = `{
  "id": 1024,
  "name": "FinkMeter",
  "active": true,
  "tags": ["api", "desktop"],
  "owner": {
    "name": "fink",
    "email": "fink@example.com"
  }
}`

const SAMPLE_JSON_ARRAY = `[
  { "id": 1, "name": "张三", "age": 28, "city": "杭州" },
  { "id": 2, "name": "李四", "age": 34, "city": "成都" }
]`

const SAMPLE_YAML = `server:
  port: 8080
  hosts:
    - api.example.com
    - web.example.com
features:
  cache: true
  limit: 100`

const SAMPLE_XML = `<root>
  <user id="1">
    <name>张三</name>
    <role>admin</role>
  </user>
  <user id="2">
    <name>李四</name>
    <role>viewer</role>
  </user>
</root>`

const SAMPLE_CREATE_TABLE = `CREATE TABLE \`t_order\` (
  \`id\` bigint NOT NULL COMMENT '主键',
  \`order_no\` varchar(64) NOT NULL COMMENT '订单号',
  \`amount\` decimal(12,2) DEFAULT NULL COMMENT '金额',
  \`paid\` tinyint(1) DEFAULT '0' COMMENT '是否支付',
  \`created_at\` datetime DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  PRIMARY KEY (\`id\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='订单表';`

export const TOOLS: ToolDef[] = [
  /* ---------------- JSON ---------------- */
  {
    id: 'json-workbench',
    name: 'JSON 格式化 / 压缩 / 校验',
    category: 'json',
    desc: '左侧粘贴原始 JSON，右侧以树形或文本查看，支持压缩、转义与错误定位',
    keywords: ['json', '格式化', '美化', '压缩', '校验', '树'],
    component: () => import('@/components/tools/JsonWorkbench.vue'),
  },
  {
    id: 'json2java',
    name: 'JSON 生成 Java 实体类',
    category: 'json',
    desc: '自动识别字段类型与嵌套结构，输出带 Lombok 注解或 getter/setter 的类',
    keywords: ['java', '实体类', 'pojo', 'dto', 'lombok'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'Java 代码',
      actionLabel: '生成 Java',
      sample: SAMPLE_JSON,
      options: [
        { key: 'rootName', label: '根类名', type: 'input', default: 'Root', width: 140 },
        { key: 'packageName', label: '包名', type: 'input', default: '', placeholder: 'com.example.model', width: 180 },
        { key: 'lombok', label: 'Lombok 注解', type: 'switch', default: true },
        { key: 'comments', label: '字段注释', type: 'switch', default: false },
      ],
      run: (input, options) =>
        jsonToEntity('java', input, {
          rootName: opt(options, 'rootName', 'Root'),
          packageName: opt(options, 'packageName'),
          lombok: flag(options, 'lombok'),
          comments: flag(options, 'comments'),
        }),
    },
  },
  {
    id: 'json2csharp',
    name: 'JSON 生成 C# 实体类',
    category: 'json',
    desc: '生成 C# 类定义，自动处理 List 泛型与属性命名',
    keywords: ['csharp', 'c#', 'dotnet', '实体类', 'class'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'C# 代码',
      actionLabel: '生成 C#',
      sample: SAMPLE_JSON,
      options: [
        { key: 'rootName', label: '根类名', type: 'input', default: 'Root', width: 140 },
        { key: 'namespace', label: '命名空间', type: 'input', default: '', placeholder: 'Example.Models', width: 180 },
      ],
      run: (input, options) =>
        jsonToEntity('csharp', input, {
          rootName: opt(options, 'rootName', 'Root'),
          namespace: opt(options, 'namespace'),
        }),
    },
  },
  {
    id: 'json2go',
    name: 'JSON 生成 Go 结构体',
    category: 'json',
    desc: '生成带 json tag 的 Go struct，字段自动转大驼峰',
    keywords: ['go', 'golang', 'struct', '结构体'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'Go 代码',
      actionLabel: '生成 Go',
      sample: SAMPLE_JSON,
      options: [
        { key: 'rootName', label: '结构体名', type: 'input', default: 'Root', width: 140 },
        { key: 'packageName', label: '包名', type: 'input', default: 'main', width: 120 },
      ],
      run: (input, options) =>
        jsonToEntity('go', input, {
          rootName: opt(options, 'rootName', 'Root'),
          packageName: opt(options, 'packageName', 'main'),
        }),
    },
  },
  {
    id: 'json2ts',
    name: 'JSON 生成 TypeScript 接口',
    category: 'json',
    desc: '生成 TS interface，嵌套对象自动抽成子接口',
    keywords: ['typescript', 'ts', 'interface', '类型'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'TypeScript 代码',
      actionLabel: '生成接口',
      sample: SAMPLE_JSON,
      options: [{ key: 'rootName', label: '接口名', type: 'input', default: 'Root', width: 140 }],
      run: (input, options) =>
        jsonToEntity('ts', input, { rootName: opt(options, 'rootName', 'Root') }),
    },
  },
  {
    id: 'sql2java',
    name: 'SQL 生成 Java 实体类',
    category: 'json',
    desc: '解析 CREATE TABLE 语句，按字段类型生成属性与注释',
    keywords: ['sql', 'ddl', 'java', '实体类', '建表'],
    simple: {
      inputLabel: 'CREATE TABLE 语句',
      outputLabel: 'Java 实体类',
      actionLabel: '生成实体类',
      sample: SAMPLE_CREATE_TABLE,
      options: [
        { key: 'packageName', label: '包名', type: 'input', default: '', placeholder: 'com.example.model', width: 180 },
        { key: 'lombok', label: 'Lombok 注解', type: 'switch', default: true },
      ],
      run: (input, options) =>
        sqlToJava(requireText(input), {
          packageName: opt(options, 'packageName'),
          lombok: flag(options, 'lombok'),
        }),
    },
  },
  {
    id: 'json2yaml',
    name: 'JSON 转 YAML',
    category: 'json',
    desc: '按层级输出 YAML，字符串自动加引号规避歧义',
    keywords: ['yaml', 'yml', '转换'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'YAML',
      actionLabel: '转 YAML',
      sample: SAMPLE_JSON,
      run: (input) => jsonToYaml(input),
    },
  },
  {
    id: 'yaml2json',
    name: 'YAML 转 JSON',
    category: 'json',
    desc: '支持常用 YAML 子集：对象、数组、注释与基本标量',
    keywords: ['yaml', 'yml', 'json', '转换'],
    simple: {
      inputLabel: 'YAML 内容',
      outputLabel: 'JSON',
      actionLabel: '转 JSON',
      sample: SAMPLE_YAML,
      run: (input) => yamlToJson(input),
    },
  },
  {
    id: 'json2xml',
    name: 'JSON 转 XML',
    category: 'json',
    desc: '数组自动展开为重复节点，@ 前缀字段生成属性',
    keywords: ['xml', '转换'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'XML',
      actionLabel: '转 XML',
      sample: SAMPLE_JSON,
      options: [{ key: 'rootName', label: '根节点', type: 'input', default: 'root', width: 140 }],
      run: (input, options) => jsonToXml(input, opt(options, 'rootName', 'root')),
    },
  },
  {
    id: 'xml2json',
    name: 'XML 转 JSON',
    category: 'json',
    desc: '同名节点自动合并为数组，属性以 @ 前缀保留',
    keywords: ['xml', 'json', '转换'],
    simple: {
      inputLabel: 'XML 内容',
      outputLabel: 'JSON',
      actionLabel: '转 JSON',
      sample: SAMPLE_XML,
      run: (input) => xmlToJson(input),
    },
  },
  {
    id: 'json2csv',
    name: 'JSON 转 CSV',
    category: 'json',
    desc: '数组对象转为表格，自动汇总全部字段为表头',
    keywords: ['csv', 'excel', '表格', '导出'],
    simple: {
      inputLabel: 'JSON 数组',
      outputLabel: 'CSV',
      actionLabel: '转 CSV',
      sample: SAMPLE_JSON_ARRAY,
      options: [
        {
          key: 'delimiter',
          label: '分隔符',
          type: 'select',
          default: ',',
          options: [
            { label: '逗号 ,', value: ',' },
            { label: '分号 ;', value: ';' },
            { label: '制表符 Tab', value: '\t' },
          ],
        },
      ],
      run: (input, options) => jsonToCsv(input, opt(options, 'delimiter', ',')),
    },
  },
  {
    id: 'csv2json',
    name: 'CSV / 表格转 JSON',
    category: 'json',
    desc: '首行为表头，逐行转为对象数组，支持引号包裹与转义',
    keywords: ['csv', 'excel', 'json', '表格'],
    simple: {
      inputLabel: 'CSV 内容',
      outputLabel: 'JSON',
      actionLabel: '转 JSON',
      sample: 'id,name,age,city\n1,张三,28,杭州\n2,李四,34,成都',
      options: [
        {
          key: 'delimiter',
          label: '分隔符',
          type: 'select',
          default: ',',
          options: [
            { label: '逗号 ,', value: ',' },
            { label: '分号 ;', value: ';' },
            { label: '制表符 Tab', value: '\t' },
          ],
        },
      ],
      run: (input, options) => csvToJson(input, opt(options, 'delimiter', ',')),
    },
  },
  {
    id: 'json2query',
    name: 'JSON 转 QueryString',
    category: 'json',
    desc: '嵌套对象展开为 a[b]=1 形式，数组按同名重复拼接',
    keywords: ['query', 'get', 'url', '参数'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'Query String',
      actionLabel: '转 QueryString',
      sample: `{ "page": 1, "size": 20, "filter": { "status": "active" }, "tags": ["a", "b"] }`,
      run: (input) => jsonToQuery(input),
    },
  },
  {
    id: 'query2json',
    name: 'QueryString 转 JSON',
    category: 'json',
    desc: '解析 URL 参数为 JSON，同名参数自动合并为数组',
    keywords: ['query', 'get', 'url', '参数'],
    simple: {
      inputLabel: 'Query String',
      outputLabel: 'JSON',
      actionLabel: '转 JSON',
      sample: '?page=1&size=20&tag=a&tag=b&name=%E5%BC%A0%E4%B8%89',
      run: (input) => queryToJson(input),
    },
  },
  {
    id: 'json2sql',
    name: 'JSON 转 SQL',
    category: 'json',
    desc: '对象或数组生成 INSERT 语句，字段名与表名自动转义',
    keywords: ['sql', 'insert', '建表', '导入', '数据库'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'SQL 语句',
      actionLabel: '生成 SQL',
      sample: `[{"id": 1, "name": "张三", "active": true}, {"id": 2, "name": "李四", "active": false}]`,
      options: [
        { key: 'table', label: '表名', type: 'input', default: 't_data', width: 140 },
        {
          key: 'mode',
          label: '写法',
          type: 'select',
          default: 'single',
          options: [
            { label: '逐条 INSERT', value: 'single' },
            { label: '批量 VALUES', value: 'batch' },
          ],
        },
      ],
      run: (input, options) =>
        jsonToSql(input, opt(options, 'table', 't_data'), opt(options, 'mode', 'single') as 'single' | 'batch'),
    },
  },
  {
    id: 'json2md',
    name: 'JSON 转 Markdown',
    category: 'json',
    desc: '对象数组生成 Markdown 表格，便于贴进文档与 PR 描述',
    keywords: ['markdown', 'md', '表格', '文档'],
    simple: {
      inputLabel: 'JSON 内容',
      outputLabel: 'Markdown',
      actionLabel: '转 Markdown',
      sample: `[{"name": "张三", "role": "后端", "city": "杭州"}, {"name": "李四", "role": "前端", "city": "成都"}]`,
      run: (input) => jsonToMarkdown(input),
    },
  },
  {
    id: 'json2jsonl',
    name: 'JSON 转 JSON Lines',
    category: 'json',
    desc: '数组拆成每行一个对象，适合日志、批量导入与流式处理',
    keywords: ['jsonl', 'ndjson', '逐行', '流式'],
    simple: {
      inputLabel: 'JSON 数组',
      outputLabel: 'JSON Lines',
      actionLabel: '转 JSON Lines',
      sample: `[{"id": 1, "name": "张三"}, {"id": 2, "name": "李四"}]`,
      run: (input) => jsonToJsonl(input),
    },
  },
  {
    id: 'jsonl2json',
    name: 'JSON Lines 转 JSON',
    category: 'json',
    desc: '每行一个对象合并为标准 JSON 数组，自动定位出错行号',
    keywords: ['jsonl', 'ndjson', '逐行', '合并'],
    simple: {
      inputLabel: 'JSON Lines 内容',
      outputLabel: 'JSON 数组',
      actionLabel: '转 JSON',
      sample: `{"id": 1, "name": "张三"}\n{"id": 2, "name": "李四"}`,
      run: (input) => jsonlToJson(input),
    },
  },

  /* ---------------- 格式化 ---------------- */
  {
    id: 'code-format',
    name: '代码格式化 / 压缩',
    category: 'format',
    desc: 'HTML、CSS、JS、SQL、Java、C#、Go、PHP、C 的美化与压缩',
    keywords: ['html', 'css', 'javascript', 'sql', 'java', 'csharp', 'go', 'php', 'c', '格式化', '压缩'],
    component: () => import('@/components/tools/CodeFormatter.vue'),
  },

  /* ---------------- 加密编码 ---------------- */
  {
    id: 'hash',
    name: '哈希 / HMAC 计算',
    category: 'crypto',
    desc: 'MD5、SHA1、SHA256、SHA512 与 HMAC 摘要，本地 Rust 计算',
    keywords: ['md5', 'sha', 'hash', '散列', '摘要', 'hmac'],
    simple: {
      inputLabel: '原始内容',
      outputLabel: '摘要结果',
      actionLabel: '计算摘要',
      sample: 'FinkMeter',
      options: [
        {
          key: 'algorithm',
          label: '算法',
          type: 'select',
          default: 'sha256',
          options: [
            { label: 'MD5', value: 'md5' },
            { label: 'SHA1', value: 'sha1' },
            { label: 'SHA256', value: 'sha256' },
            { label: 'SHA512', value: 'sha512' },
            { label: 'HMAC-SHA256', value: 'hmac-sha256' },
            { label: 'HMAC-SHA512', value: 'hmac-sha512' },
          ],
        },
        { key: 'key', label: 'HMAC 密钥', type: 'input', default: '', placeholder: '选择 HMAC 时必填', width: 200 },
      ],
      run: async (input, options) => {
        const algorithm = opt(options, 'algorithm', 'sha256')
        const text = requireText(input)
        if (algorithm.startsWith('hmac-')) {
          const key = opt(options, 'key')
          if (!key) throw new Error('请填写 HMAC 密钥')
          return toolHmac(text, key, algorithm.slice(5))
        }
        return toolHash(text, algorithm)
      },
    },
  },
  {
    id: 'aes',
    name: 'AES 加解密',
    category: 'crypto',
    desc: 'AES-256-CBC，密钥经 SHA-256 派生，密文格式为 IV:密文',
    keywords: ['aes', '加密', '解密', 'cbc'],
    component: () => import('@/components/tools/AesTool.vue'),
  },
  {
    id: 'jwt',
    name: 'JWT 解码',
    category: 'crypto',
    desc: '拆解 Header / Payload / Signature，并高亮过期时间与签发时间',
    keywords: ['jwt', 'token', 'bearer', '解码'],
    component: () => import('@/components/tools/JwtTool.vue'),
  },
  {
    id: 'base64',
    name: 'Base64 编解码',
    category: 'crypto',
    desc: '支持标准 Base64 与 URL 安全变体，UTF-8 文本无损往返',
    keywords: ['base64', '编码', '解码', 'url'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'FinkMeter 本地工具箱',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '编码', value: 'encode' },
            { label: '解码', value: 'decode' },
            { label: 'URL 安全编码', value: 'url-encode' },
            { label: 'URL 安全解码', value: 'url-decode' },
          ],
        },
      ],
      run: (input, options) => {
        const mode = opt(options, 'mode', 'encode')
        const text = requireText(input)
        if (mode === 'encode') return base64Encode(text)
        if (mode === 'decode') return base64Decode(text.trim())
        if (mode === 'url-encode') return base64UrlEncode(text)
        return base64UrlDecode(text.trim())
      },
    },
  },
  {
    id: 'url-codec',
    name: 'URL 编解码',
    category: 'crypto',
    desc: 'encodeURIComponent 全量编码，或逐段还原',
    keywords: ['url', 'encode', 'decode', '百分号'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'https://example.com/搜索?q=接口 测试',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '编码', value: 'encode' },
            { label: '解码', value: 'decode' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        if (opt(options, 'mode', 'encode') === 'encode') return encodeURIComponent(text)
        try {
          return decodeURIComponent(text)
        } catch {
          throw new Error('内容不是合法的 URL 编码')
        }
      },
    },
  },
  {
    id: 'unicode',
    name: 'Unicode 转义',
    category: 'crypto',
    desc: '中文与特殊字符转 \\uXXXX，或还原为可读文本',
    keywords: ['unicode', '转义', 'u4e2d'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: '你好，FinkMeter',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '转义', value: 'encode' },
            { label: '还原', value: 'decode' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return opt(options, 'mode', 'encode') === 'encode' ? unicodeEscape(text) : unicodeUnescape(text)
      },
    },
  },
  {
    id: 'html-entity',
    name: 'HTML 实体编解码',
    category: 'crypto',
    desc: '转义 & < > 引号等字符，或还原命名 / 数字实体',
    keywords: ['html', 'entity', '转义', 'xss'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: '<div class="card">Tom & Jerry</div>',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '转义', value: 'encode' },
            { label: '还原', value: 'decode' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return opt(options, 'mode', 'encode') === 'encode' ? htmlEscape(text) : htmlUnescape(text)
      },
    },
  },
  {
    id: 'ascii-codec',
    name: '字符码转换',
    category: 'crypto',
    desc: '文本与 ASCII / Unicode 码点互转，支持十进制与十六进制',
    keywords: ['ascii', 'char', 'code', '码点'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'Hello',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '文本转码点', value: 'encode' },
            { label: '码点转文本', value: 'decode' },
          ],
        },
        {
          key: 'radix',
          label: '进制',
          type: 'select',
          default: 'dec',
          options: [
            { label: '十进制', value: 'dec' },
            { label: '十六进制', value: 'hex' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        const radix = opt(options, 'radix', 'dec') as 'dec' | 'hex'
        return opt(options, 'mode', 'encode') === 'encode'
          ? asciiEncode(text, radix)
          : asciiDecode(text, radix)
      },
    },
  },
  {
    id: 'rc4',
    name: 'RC4 对称加密',
    category: 'crypto',
    desc: '经典流密码算法，输出 Base64 密文（仅用于兼容老系统）',
    keywords: ['rc4', '加密', '解密'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'FinkMeter',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '加密', value: 'encode' },
            { label: '解密', value: 'decode' },
          ],
        },
        { key: 'key', label: '密钥', type: 'input', default: 'finkmeter', width: 160 },
      ],
      run: (input, options) => {
        const text = requireText(input)
        const key = opt(options, 'key')
        if (!key) throw new Error('请填写密钥')
        if (opt(options, 'mode', 'encode') === 'encode') return rc4(text, key)
        // RC4 为对称算法：Base64 密文先解码为文本再加密一次即可还原
        const binary = atob(text.trim())
        const bytes = new Uint8Array(binary.length)
        for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i)
        const plainBytes = atob(rc4(new TextDecoder('latin1').decode(bytes), key))
        const result = new Uint8Array(plainBytes.length)
        for (let i = 0; i < plainBytes.length; i += 1) result[i] = plainBytes.charCodeAt(i)
        return new TextDecoder().decode(result)
      },
    },
  },
  {
    id: 'gzip',
    name: 'Gzip 压缩 / 解压',
    category: 'crypto',
    desc: '文本压缩为 Base64，方便塞进 Header 或配置项',
    keywords: ['gzip', '压缩', '解压', 'base64'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'FinkMeter 本地工具集',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '压缩', value: 'encode' },
            { label: '解压', value: 'decode' },
          ],
        },
      ],
      run: async (input, options) => {
        const text = requireText(input)
        return opt(options, 'mode', 'encode') === 'encode'
          ? gzipToBase64(text)
          : gunzipFromBase64(text.trim())
      },
    },
  },
  {
    id: 'checksum',
    name: 'CRC / Luhn 校验',
    category: 'crypto',
    desc: 'CRC32、CRC16-MODBUS 与银行卡 Luhn 校验',
    keywords: ['crc32', 'crc16', 'luhn', '校验'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '校验结果',
      actionLabel: '计算',
      sample: '1234567812345670',
      options: [
        {
          key: 'algorithm',
          label: '算法',
          type: 'select',
          default: 'crc32',
          options: [
            { label: 'CRC32', value: 'crc32' },
            { label: 'CRC16-MODBUS', value: 'crc16' },
            { label: 'Luhn', value: 'luhn' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        const algorithm = opt(options, 'algorithm', 'crc32')
        if (algorithm === 'crc16') return crc16(text)
        if (algorithm === 'luhn') return luhnCheck(text)
        return crc32(text)
      },
    },
  },
  {
    id: 'base32',
    name: 'Base32 编解码',
    category: 'crypto',
    desc: 'RFC 4648 Base32，兼容忽略填充与大小写',
    keywords: ['base32', '编码', '解码', 'rfc4648'],
    simple: {
      inputLabel: '原始内容 / Base32',
      outputLabel: '转换结果',
      actionLabel: '转换',
      sample: 'FinkMeter',
      options: [
        {
          key: 'mode',
          label: '方向',
          type: 'select',
          default: 'encode',
          options: [
            { label: '编码', value: 'encode' },
            { label: '解码', value: 'decode' },
          ],
        },
      ],
      run: (input, options) =>
        opt(options, 'mode', 'encode') === 'encode'
          ? base32Encode(requireText(input))
          : base32Decode(requireText(input)),
    },
  },
  {
    id: 'hex-codec',
    name: '十六进制编解码',
    category: 'crypto',
    desc: '文本与十六进制互转，支持大小写、0x 前缀与连续写法',
    keywords: ['hex', '十六进制', '编码', '解码', '二进制'],
    simple: {
      inputLabel: '文本 / 十六进制',
      outputLabel: '转换结果',
      actionLabel: '转换',
      sample: 'FinkMeter 你好',
      options: [
        {
          key: 'mode',
          label: '方向',
          type: 'select',
          default: 'encode',
          options: [
            { label: '文本转十六进制', value: 'encode' },
            { label: '十六进制转文本', value: 'decode' },
          ],
        },
        { key: 'upper', label: '大写', type: 'switch', default: true },
        { key: 'prefix', label: '0x 前缀', type: 'switch', default: false },
      ],
      run: (input, options) => {
        const text = requireText(input)
        if (opt(options, 'mode', 'encode') === 'decode') return hexDecode(text)
        return hexEncode(text, { upper: options.upper === true, prefix: options.prefix === true })
      },
    },
  },
  {
    id: 'string-escape',
    name: '字符串转义 / 还原',
    category: 'crypto',
    desc: '把换行、引号、制表符转为 \\n、\\" 等转义序列，或反向还原',
    keywords: ['escape', '转义', '反斜杠', '字符串', 'json'],
    simple: {
      inputLabel: '原始内容 / 转义内容',
      outputLabel: '转换结果',
      actionLabel: '转换',
      sample: '第一行\n第二行 "引号" \t 制表符',
      options: [
        {
          key: 'mode',
          label: '方向',
          type: 'select',
          default: 'escape',
          options: [
            { label: '转义', value: 'escape' },
            { label: '还原', value: 'unescape' },
          ],
        },
      ],
      run: (input, options) =>
        opt(options, 'mode', 'escape') === 'escape' ? escapeString(input) : unescapeString(input),
    },
  },
  {
    id: 'jwt-sign',
    name: 'JWT 签发',
    category: 'crypto',
    desc: '按 HS256 / HS512 生成 JWT，自动写入 iat 与 exp',
    keywords: ['jwt', 'token', '签名', 'hs256', 'hs512'],
    simple: {
      inputLabel: 'Payload JSON',
      outputLabel: 'JWT',
      actionLabel: '生成 Token',
      sample: `{"sub": "1001", "name": "张三", "role": "admin"}`,
      options: [
        {
          key: 'algorithm',
          label: '算法',
          type: 'select',
          default: 'HS256',
          options: [
            { label: 'HS256', value: 'HS256' },
            { label: 'HS512', value: 'HS512' },
          ],
        },
        { key: 'secret', label: '密钥', type: 'input', default: 'your-256-bit-secret', width: 200 },
        { key: 'expire', label: '有效期(分钟)', type: 'number', default: 60, width: 110 },
      ],
      run: (input, options) =>
        signJwt(
          requireText(input),
          opt(options, 'secret', 'your-256-bit-secret'),
          opt(options, 'algorithm', 'HS256') as 'HS256' | 'HS512',
          Number(options.expire ?? 60),
        ),
    },
  },

  /* ---------------- 文本 ---------------- */
  {
    id: 'case-convert',
    name: '命名风格转换',
    category: 'text',
    desc: '驼峰、下划线、短横线、常量、点号与大小写互转',
    keywords: ['camel', 'snake', 'kebab', '命名', '驼峰', '下划线'],
    simple: {
      inputLabel: '原始文本（可含空格、下划线、短横线）',
      outputLabel: '转换结果',
      actionLabel: '转换',
      sample: 'user login name',
      options: [
        {
          key: 'style',
          label: '目标风格',
          type: 'select',
          default: 'camel',
          options: [
            { label: '小驼峰 camelCase', value: 'camel' },
            { label: '大驼峰 PascalCase', value: 'pascal' },
            { label: '下划线 snake_case', value: 'snake' },
            { label: '短横线 kebab-case', value: 'kebab' },
            { label: '常量 CONSTANT_CASE', value: 'constant' },
            { label: '点号 dot.case', value: 'dot' },
            { label: '空格分隔', value: 'space' },
            { label: '全部大写', value: 'upper' },
            { label: '全部小写', value: 'lower' },
            { label: '首字母大写', value: 'title' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return convertCase(text, opt(options, 'style', 'camel') as CaseStyle)
      },
    },
  },
  {
    id: 'text-stats',
    name: '文本统计',
    category: 'text',
    desc: '统计字符、行数、非空行、汉字、字母、数字与符号数量',
    keywords: ['统计', '字数', '行数', '字符'],
    simple: {
      inputLabel: '输入文本',
      outputLabel: '统计结果',
      actionLabel: '统计',
      sample: 'FinkMeter 是一款本地 API 调试工具。\n所有数据都保存在这台电脑上。',
      run: (input) => {
        if (!input) throw new Error('请输入文本')
        return textStats(input)
      },
    },
  },
  {
    id: 'dedupe',
    name: '去重 / 排序',
    category: 'text',
    desc: '按行去重，可选忽略大小写、去除首尾空白与排序',
    keywords: ['去重', '排序', 'unique', 'sort'],
    simple: {
      inputLabel: '按行输入',
      outputLabel: '处理结果',
      actionLabel: '去重',
      sample: 'apple\nbanana\nApple\nbanana\ncherry',
      options: [
        {
          key: 'sort',
          label: '排序',
          type: 'select',
          default: 'none',
          options: [
            { label: '不排序', value: 'none' },
            { label: '升序', value: 'asc' },
            { label: '降序', value: 'desc' },
          ],
        },
        { key: 'ignoreCase', label: '忽略大小写', type: 'switch', default: false },
        { key: 'trim', label: '去除首尾空白', type: 'switch', default: true },
      ],
      run: (input, options) =>
        dedupeLines(input, {
          sort: opt(options, 'sort', 'none') as 'none' | 'asc' | 'desc',
          ignoreCase: flag(options, 'ignoreCase'),
          trim: flag(options, 'trim'),
        }),
    },
  },
  {
    id: 'replace',
    name: '查找替换',
    category: 'text',
    desc: '支持普通文本与正则替换，可忽略大小写',
    keywords: ['替换', '查找', 'regex', 'replace'],
    simple: {
      inputLabel: '原始文本',
      outputLabel: '替换结果',
      actionLabel: '替换',
      sample: '接口地址：{{base_url}}/users',
      options: [
        { key: 'find', label: '查找内容', type: 'input', default: '{{base_url}}', width: 180 },
        { key: 'replacement', label: '替换为', type: 'input', default: 'https://api.example.com', width: 220 },
        { key: 'regex', label: '正则模式', type: 'switch', default: false },
        { key: 'ignoreCase', label: '忽略大小写', type: 'switch', default: false },
      ],
      run: (input, options) => {
        if (!input) throw new Error('请输入原始文本')
        return replaceText(input, opt(options, 'find'), opt(options, 'replacement'), {
          regex: flag(options, 'regex'),
          ignoreCase: flag(options, 'ignoreCase'),
        })
      },
    },
  },
  {
    id: 'reverse',
    name: '文本反转 / 镜像',
    category: 'text',
    desc: '整体倒序，或转为镜像字符用于趣味排版',
    keywords: ['反转', '倒序', 'reverse', '镜像'],
    simple: {
      inputLabel: '输入文本',
      outputLabel: '输出结果',
      actionLabel: '反转',
      sample: 'FinkMeter',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'reverse',
          options: [
            { label: '整体反转', value: 'reverse' },
            { label: '镜像字符', value: 'mirror' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return reverseText(text, opt(options, 'mode', 'reverse') as 'reverse' | 'mirror')
      },
    },
  },
  {
    id: 'width-convert',
    name: '全角 / 半角转换',
    category: 'text',
    desc: '中文标点与英文标点互转，清理复制粘贴带来的全角空格',
    keywords: ['全角', '半角', 'width'],
    simple: {
      inputLabel: '输入文本',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'ＦｉｎｋＭｅｔｅｒ　接口测试',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'half',
          options: [
            { label: '转半角', value: 'half' },
            { label: '转全角', value: 'full' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return opt(options, 'mode', 'half') === 'half' ? toHalfWidth(text) : toFullWidth(text)
      },
    },
  },
  {
    id: 'rmb-upper',
    name: '金额大写转换',
    category: 'text',
    desc: '数字金额转中文大写，支持角分与负数',
    keywords: ['人民币', '大写', '金额', '财务'],
    simple: {
      inputLabel: '数字金额',
      outputLabel: '中文大写',
      actionLabel: '转换',
      sample: '1234567.89',
      run: (input) => rmbUpper(requireText(input)),
    },
  },
  {
    id: 'morse',
    name: '摩斯电码',
    category: 'text',
    desc: '英文字符与摩斯电码互转，单词以 / 分隔',
    keywords: ['morse', '摩斯', '电码'],
    simple: {
      inputLabel: '输入内容',
      outputLabel: '输出结果',
      actionLabel: '转换',
      sample: 'sos',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'encode',
          options: [
            { label: '编码', value: 'encode' },
            { label: '解码', value: 'decode' },
          ],
        },
      ],
      run: (input, options) => {
        const text = requireText(input)
        return opt(options, 'mode', 'encode') === 'encode' ? morseEncode(text) : morseDecode(text)
      },
    },
  },
  {
    id: 'diff',
    name: '文本对比',
    category: 'text',
    desc: '行级差异对比，标注新增、删除与相同行',
    keywords: ['diff', '对比', '差异', '比较'],
    component: () => import('@/components/tools/DiffTool.vue'),
  },
  {
    id: 'extract',
    name: '内容提取',
    category: 'text',
    desc: '从大段文本中批量提取邮箱、URL、IP、手机号、UUID 与中文',
    keywords: ['提取', '邮箱', 'email', 'url', 'ip', '手机号', 'uuid', '正则'],
    simple: {
      inputLabel: '待提取文本',
      outputLabel: '提取结果',
      actionLabel: '提取',
      sample:
        '联系邮箱 dev@example.com、ops@example.org；文档 https://docs.example.com/guide 和 http://10.0.0.12:8080/api；' +
        '电话 13800138000；订单号 550e8400-e29b-41d4-a716-446655440000；备注：仅内部使用。',
      options: [
        {
          key: 'kind',
          label: '提取类型',
          type: 'select',
          default: 'email',
          width: 150,
          options: [
            { label: '邮箱', value: 'email' },
            { label: 'URL', value: 'url' },
            { label: 'IPv4', value: 'ipv4' },
            { label: '手机号', value: 'phone' },
            { label: '数字', value: 'number' },
            { label: 'UUID', value: 'uuid' },
            { label: '中文', value: 'chinese' },
          ],
        },
      ],
      run: (input, options) => extractText(input, opt(options, 'kind', 'email') as ExtractKind),
    },
  },
  {
    id: 'line-ops',
    name: '行处理',
    category: 'text',
    desc: '去空行、去重、排序、反转、加/去行号、合并为一行',
    keywords: ['行', '去重', '排序', '空行', '行号', '合并'],
    simple: {
      inputLabel: '多行文本',
      outputLabel: '处理结果',
      actionLabel: '处理',
      sample: '  apple\nbanana\n\nbanana\n  cherry  \n\napple',
      options: [
        {
          key: 'op',
          label: '操作',
          type: 'select',
          default: 'clean',
          width: 160,
          options: [
            { label: '清理（去空格/空行/重复）', value: 'clean' },
            { label: '去除首尾空格', value: 'trim' },
            { label: '去除空行', value: 'noBlank' },
            { label: '去除重复行', value: 'unique' },
            { label: '升序排序', value: 'sortAsc' },
            { label: '降序排序', value: 'sortDesc' },
            { label: '反转行顺序', value: 'reverseLines' },
            { label: '添加行号', value: 'number' },
            { label: '去除行号', value: 'unnumber' },
            { label: '合并为一行', value: 'join' },
          ],
        },
      ],
      run: (input, options) => lineOps(input, opt(options, 'op', 'clean') as LineOp),
    },
  },
  {
    id: 'mac',
    name: 'MAC 地址工具',
    category: 'text',
    desc: '格式化 MAC 地址为多种写法，留空则随机生成一个测试地址',
    keywords: ['mac', '网卡', '物理地址', '格式化', '随机'],
    simple: {
      inputLabel: 'MAC 地址（留空则随机生成）',
      inputPlaceholder: '00-1A-2B-3C-4D-5E',
      outputLabel: '格式化结果',
      actionLabel: '处理',
      sample: '00-1A-2B-3C-4D-5E',
      run: (input) => macInfo(input),
    },
  },

  {
    id: 'text-toolbox',
    name: '文本一键处理',
    category: 'text',
    desc:
      '清空与撤销、随机密码、去空行空格、大小写、去重取重、排序、SQL IN、or/逗号/自定义符号连接、' +
      '列转行、表格转 Markdown 或 HTML、转置、多表拼接、格式化 JSON、MyBatis 提取 SQL',
    keywords: [
      '文本',
      '一键',
      '清空',
      '复制',
      '撤销',
      '还原',
      '密码',
      '空行',
      '空格',
      '大写',
      '小写',
      '去重',
      '重复',
      '翻转',
      '排序',
      'or',
      'sql in',
      '逗号',
      '分隔',
      '连接',
      '列转行',
      'markdown',
      'urlencode',
      '转置',
      'json',
      '多表',
      'join',
      'html',
      'mybatis',
    ],
    component: () => import('@/components/tools/TextToolbox.vue'),
  },

  /* ---------------- 计算换算 ---------------- */
  {
    id: 'base-convert',
    name: '进制转换',
    category: 'calc',
    desc: '2 / 8 / 10 / 16 / 32 / 36 进制互转，支持大整数',
    keywords: ['进制', 'binary', 'hex', 'oct', '二进', '十六'],
    component: () => import('@/components/tools/BaseConvertTool.vue'),
  },
  {
    id: 'unit-convert',
    name: '单位换算',
    category: 'calc',
    desc: '长度、重量、面积、体积、速度、数据、时间与温度',
    keywords: ['单位', '换算', '长度', '重量', '温度', '面积'],
    component: () => import('@/components/tools/UnitTool.vue'),
  },
  {
    id: 'color',
    name: '颜色转换',
    category: 'calc',
    desc: 'HEX / RGB / HSL / HSV / CMYK 互转，含对比度与明度',
    keywords: ['颜色', 'color', 'hex', 'rgb', 'hsl', '对比度'],
    component: () => import('@/components/tools/ColorTool.vue'),
  },
  {
    id: 'timestamp',
    name: '时间戳转换',
    category: 'calc',
    desc: '秒 / 毫秒 / 微秒时间戳与本地、UTC、ISO 时间互转',
    keywords: ['时间戳', 'timestamp', '时间', '日期', '时区'],
    component: () => import('@/components/tools/TimestampTool.vue'),
  },
  {
    id: 'password',
    name: '随机密码生成',
    category: 'calc',
    desc: '可定制大小写、数字、符号并排除易混淆字符',
    keywords: ['密码', 'password', '随机', '强度'],
    simple: {
      inputLabel: '',
      outputLabel: '生成结果',
      actionLabel: '生成密码',
      options: [
        { key: 'length', label: '长度', type: 'number', default: 20 },
        { key: 'symbol', label: '包含符号', type: 'switch', default: true },
        { key: 'excludeAmbiguous', label: '排除易混淆字符', type: 'switch', default: true },
      ],
      run: (_input, options) =>
        randomPassword(Number(options.length ?? 20), {
          symbol: flag(options, 'symbol'),
          excludeAmbiguous: flag(options, 'excludeAmbiguous'),
        }),
    },
  },
  {
    id: 'uuid',
    name: 'UUID / GUID 生成',
    category: 'calc',
    desc: '批量生成符合 RFC 4122 的 v4 UUID',
    keywords: ['uuid', 'guid', '唯一', '随机'],
    simple: {
      inputLabel: '',
      outputLabel: '生成结果',
      actionLabel: '生成 UUID',
      options: [
        { key: 'count', label: '数量', type: 'number', default: 5 },
        {
          key: 'style',
          label: '格式',
          type: 'select',
          default: 'lower',
          options: [
            { label: '小写', value: 'lower' },
            { label: '大写', value: 'upper' },
            { label: '无连字符', value: 'plain' },
          ],
        },
      ],
      run: (_input, options) => {
        const count = Math.min(100, Math.max(1, Number(options.count ?? 5)))
        const style = opt(options, 'style', 'lower')
        return Array.from({ length: count }, () => {
          const value = uuidV4()
          if (style === 'upper') return value.toUpperCase()
          if (style === 'plain') return value.replace(/-/g, '')
          return value
        }).join('\n')
      },
    },
  },
  {
    id: 'random-string',
    name: '随机字符串',
    category: 'calc',
    desc: '按字符集生成指定长度的随机串，可用于签名、盐值',
    keywords: ['随机', '字符串', 'salt', 'nonce'],
    simple: {
      inputLabel: '',
      outputLabel: '生成结果',
      actionLabel: '生成',
      options: [
        { key: 'length', label: '长度', type: 'number', default: 32 },
        {
          key: 'pool',
          label: '字符集',
          type: 'select',
          default: 'alnum',
          options: [
            { label: '字母 + 数字', value: 'alnum' },
            { label: '仅字母', value: 'alpha' },
            { label: '仅数字', value: 'num' },
            { label: '十六进制', value: 'hex' },
            { label: 'URL 安全字符', value: 'url' },
          ],
        },
      ],
      run: (_input, options) => {
        const pools: Record<string, string> = {
          alnum: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789',
          alpha: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz',
          num: '0123456789',
          hex: '0123456789abcdef',
          url: 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_',
        }
        return randomString(Number(options.length ?? 32), pools[opt(options, 'pool', 'alnum')] ?? pools.alnum)
      },
    },
  },
  {
    id: 'keyboard',
    name: '键盘按键码',
    category: 'calc',
    desc: '按下物理按键即可查看 event.key / code / keyCode',
    keywords: ['键盘', 'keycode', 'key', 'code', '按键'],
    component: () => import('@/components/tools/KeyboardTool.vue'),
  },

  {
    id: 'date-calc',
    name: '日期计算',
    category: 'calc',
    desc: '计算日期相差天数、工作日，或按天数推算目标日期',
    keywords: ['日期', '天数', '工作日', '相差', '推算', 'date'],
    simple: {
      inputLabel: '日期（两个日期用空格分隔，或 日期 + 天数）',
      inputPlaceholder: '2024-01-01 2024-03-15',
      outputLabel: '计算结果',
      actionLabel: '计算',
      sample: '2024-01-01 2024-03-15',
      options: [
        {
          key: 'mode',
          label: '模式',
          type: 'select',
          default: 'diff',
          width: 170,
          options: [
            { label: '相差天数 / 时分秒', value: 'diff' },
            { label: '相差工作日', value: 'workdays' },
            { label: '日期加减天数', value: 'add' },
          ],
        },
      ],
      run: (input, options) =>
        dateCalc(requireText(input), opt(options, 'mode', 'diff') as 'diff' | 'add' | 'workdays'),
    },
  },
  {
    id: 'px-rem',
    name: 'px / rem 换算',
    category: 'calc',
    desc: '按根字号批量换算 px 与 rem，支持一次输入多个值',
    keywords: ['px', 'rem', 'css', '换算', '单位'],
    simple: {
      inputLabel: '数值（空格分隔，可带单位）',
      outputLabel: '换算结果',
      actionLabel: '换算',
      sample: '12 14 16 20 24 32',
      options: [
        { key: 'base', label: '根字号', type: 'number', default: 16, width: 100 },
        {
          key: 'direction',
          label: '方向',
          type: 'select',
          default: 'px2rem',
          options: [
            { label: 'px → rem', value: 'px2rem' },
            { label: 'rem → px', value: 'rem2px' },
          ],
        },
      ],
      run: (input, options) =>
        pxRem(
          requireText(input),
          Number(options.base ?? 16),
          opt(options, 'direction', 'px2rem') as 'px2rem' | 'rem2px',
        ),
    },
  },

  /* ---------------- 开发辅助 ---------------- */
  {
    id: 'mybatis',
    name: 'MyBatis SQL 填充',
    category: 'dev',
    desc: '渲染 if / where / foreach / choose 等标签，输出可执行 SQL',
    keywords: ['mybatis', 'sql', 'mapper', '动态', '参数'],
    component: () => import('@/components/tools/MybatisTool.vue'),
  },
  {
    id: 'regex',
    name: '正则表达式测试',
    category: 'dev',
    desc: '实时匹配、分组提取与位置标注，支持 g i m s u y 标志',
    keywords: ['regex', '正则', '匹配', '分组'],
    component: () => import('@/components/tools/RegexTool.vue'),
  },
  {
    id: 'cron',
    name: 'Cron 表达式解析',
    category: 'dev',
    desc: '解析 5 位 Cron，输出字段含义与接下来 5 次执行时间',
    keywords: ['cron', '定时', '任务', '调度', '表达式'],
    simple: {
      inputLabel: 'Cron 表达式',
      inputPlaceholder: '0 9 * * 1-5',
      outputLabel: '解析结果',
      actionLabel: '解析',
      sample: '0 9 * * 1-5',
      options: [{ key: 'count', label: '执行次数', type: 'number', default: 5, width: 100 }],
      run: (input, options) => cronInfo(requireText(input), Number(options.count ?? 5)),
    },
  },
  {
    id: 'snowflake',
    name: '雪花 ID 解析',
    category: 'dev',
    desc: '还原雪花 ID 的生成时间、数据中心、工作节点与序列号',
    keywords: ['snowflake', '雪花', 'id', '分布式', 'mybatis'],
    simple: {
      inputLabel: '雪花 ID',
      outputLabel: '解析结果',
      actionLabel: '解析',
      sample: '1812345678901234567',
      options: [
        {
          key: 'workerBits',
          label: '机器位',
          type: 'select',
          default: '10',
          width: 120,
          options: [
            { label: '10 位（5+5）', value: '10' },
            { label: '5 位', value: '5' },
          ],
        },
        {
          key: 'epoch',
          label: '起始时间',
          type: 'input',
          default: '2010-11-04 16:35:00',
          width: 190,
        },
      ],
      run: (input, options) =>
        snowflakeInfo(
          requireText(input),
          opt(options, 'epoch', '2010-11-04 16:35:00'),
          Number(options.workerBits ?? 10),
        ),
    },
  },

  /* ---------------- 网络 ---------------- */
  {
    id: 'ip-lookup',
    name: 'IP 查询',
    category: 'network',
    desc: '查询本机公网 IP，展示归属地、运营商、经纬度等信息',
    keywords: ['ip', '公网', '归属地', '运营商', '查询'],
    component: () => import('@/components/tools/IpLookupTool.vue'),
  },
  {
    id: 'subnet',
    name: '子网掩码计算',
    category: 'network',
    desc: '输入 CIDR 得到网络地址、广播地址、可用主机范围与地址段',
    keywords: ['ip', 'cidr', '子网', '掩码', '网段'],
    component: () => import('@/components/tools/SubnetTool.vue'),
  },
  {
    id: 'ip-convert',
    name: 'IP 进制转换',
    category: 'network',
    desc: 'IPv4 与整数、二进制互转，方便日志与数据库排查',
    keywords: ['ip', '整数', '二进制', '转换'],
    simple: {
      inputLabel: 'IPv4 地址或整数',
      outputLabel: '转换结果',
      actionLabel: '转换',
      sample: '192.168.1.10',
      run: (input) => {
        const text = requireText(input).trim()
        if (/^\d+$/.test(text)) {
          const ip = longToIp(Number(text))
          const long = Number(text)
          return [
            `IPv4：${ip}`,
            `整数：${long}`,
            `十六进制：0x${long.toString(16).toUpperCase()}`,
            `二进制：${[24, 16, 8, 0].map((shift) => (long >>> shift & 0xff).toString(2).padStart(8, '0')).join('.')}`,
          ].join('\n')
        }
        const long = ipToLong(text)
        return [
          `IPv4：${text.trim()}`,
          `整数：${long}`,
          `十六进制：0x${long.toString(16).toUpperCase()}`,
          `二进制：${[24, 16, 8, 0].map((shift) => (long >>> shift & 0xff).toString(2).padStart(8, '0')).join('.')}`,
        ].join('\n')
      },
    },
  },
  {
    id: 'url-parse',
    name: 'URL 解析',
    category: 'network',
    desc: '拆解协议、主机、端口、路径、片段与全部查询参数',
    keywords: ['url', '解析', '参数', 'query', '域名'],
    simple: {
      inputLabel: 'URL',
      outputLabel: '解析结果',
      actionLabel: '解析',
      sample: 'https://example.com:8443/api/v1/users?page=2&size=20&tag=a&tag=b#list',
      run: (input) => urlInfo(requireText(input)),
    },
  },
  {
    id: 'ports',
    name: '常见端口对照',
    category: 'network',
    desc: '常用服务端口与用途速查',
    keywords: ['端口', 'port', '服务'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'ports' },
  },

  /* ---------------- 二维码 ---------------- */
  {
    id: 'qr-generate',
    name: '二维码生成',
    category: 'qr',
    desc:
      '文本 / 网址、WiFi、名片、电话、短信、邮件、定位七类内容；可调尺寸、留白、容错级别、' +
      '前景背景色、码点与定位点样式、Logo；导出 PNG 或 SVG',
    keywords: [
      '二维码',
      'qrcode',
      'qr',
      '生成',
      'wifi',
      '名片',
      'vcard',
      '电话',
      '短信',
      '邮件',
      '定位',
      'geo',
      'logo',
      'png',
      'svg',
    ],
    component: () => import('@/components/tools/QrTool.vue'),
    props: { mode: 'generate' },
  },
  {
    id: 'qr-decode',
    name: '二维码识别',
    category: 'qr',
    desc: '上传或拖入二维码图片，在本机解析内容并识别 WiFi、名片、邮件、链接等类型',
    keywords: ['二维码', 'qr', '识别', '解析', '扫码', '解码', 'decode', '图片'],
    component: () => import('@/components/tools/QrTool.vue'),
    props: { mode: 'decode' },
  },

  /* ---------------- 参考手册 ---------------- */
  {
    id: 'ascii-table',
    name: 'ASCII 码表',
    category: 'reference',
    desc: '0-127 的十进制、十六进制、二进制与字符对照',
    keywords: ['ascii', '码表', '字符集'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'ascii' },
  },
  {
    id: 'http-status',
    name: 'HTTP 状态码',
    category: 'reference',
    desc: '常用状态码的含义与典型场景',
    keywords: ['http', '状态码', 'status'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'http' },
  },
  {
    id: 'mime-types',
    name: 'MIME 类型表',
    category: 'reference',
    desc: '常见 Content-Type 与扩展名对应关系',
    keywords: ['mime', 'content-type', '类型'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'mime' },
  },
  {
    id: 'http-headers',
    name: 'HTTP 请求头参考',
    category: 'reference',
    desc: '常用请求头的作用与示例值',
    keywords: ['header', '请求头', 'http'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'headers' },
  },
  {
    id: 'git-commands',
    name: 'Git 命令速查',
    category: 'reference',
    desc: '常用 Git 命令的作用与示例，支持关键字搜索与一键复制',
    keywords: ['git', '命令', '版本控制', 'branch', 'commit'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'git' },
  },
  {
    id: 'docker-commands',
    name: 'Docker 命令速查',
    category: 'reference',
    desc: '容器、镜像、Compose 常用命令与参数示例',
    keywords: ['docker', '容器', '镜像', 'compose', '命令'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'docker' },
  },
  {
    id: 'regex-cheatsheet',
    name: '正则语法速查',
    category: 'reference',
    desc: '字符类、量词、分组、断言等常用正则语法与示例',
    keywords: ['regex', '正则', '语法', '断言', '分组'],
    component: () => import('@/components/tools/ReferenceTables.vue'),
    props: { kind: 'regex' },
  },
]

export const TOOL_MAP = new Map(TOOLS.map((tool) => [tool.id, tool]))

export function searchTools(keyword: string): ToolDef[] {
  const text = keyword.trim().toLowerCase()
  if (!text) return TOOLS
  return TOOLS.filter((tool) => {
    const haystack = [
      tool.name,
      tool.desc,
      ...(tool.keywords ?? []),
      TOOL_CATEGORIES.find((category) => category.id === tool.category)?.name ?? '',
    ]
      .join(' ')
      .toLowerCase()
    return haystack.includes(text)
  })
}

export function toolsByCategory(category: string): ToolDef[] {
  return TOOLS.filter((tool) => tool.category === category)
}
