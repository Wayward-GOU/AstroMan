<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { message } from 'ant-design-vue'
import PageHeader from '@/components/PageHeader.vue'
import CodeArea from '@/components/CodeArea.vue'
import JsonTree from '@/components/JsonTree.vue'
import { renderMybatis } from '@/utils/mybatis'
import type { SqlMode, SqlParam } from '@/utils/mybatis'
import * as api from '@/api'

const activeTab = ref('json')

function copyText(text: string, tip = '已复制') {
  if (!text) {
    message.warning('没有可复制的内容')
    return
  }
  navigator.clipboard
    .writeText(text)
    .then(() => message.success(tip))
    .catch(() => message.error('复制失败，请手动选择内容'))
}

/* ---------------- JSON ---------------- */
const jsonIndent = ref('2')
const jsonInput = ref('')
const jsonOutput = ref('')
const jsonStatus = ref('')
const jsonStatusError = ref(false)

function lineCount(text: string) {
  return text ? text.split('\n').length : 0
}

function textMeta(text: string) {
  return `${lineCount(text)} 行 · ${text.length} 字符`
}

const jsonInputMeta = computed(() => textMeta(jsonInput.value))
const jsonOutputMeta = computed(() => textMeta(jsonOutput.value))

const jsonView = ref<'tree' | 'text'>('tree')
const jsonExpand = ref<'default' | 'all' | 'none'>('default')
const jsonExpandVersion = ref(0)

const parsedJson = computed<{ ok: boolean; value?: unknown; error?: string }>(() => {
  const text = jsonInput.value.trim()
  if (!text) return { ok: false }
  try {
    return { ok: true, value: JSON.parse(text) }
  } catch (error) {
    const raw = error instanceof Error ? error.message : String(error)
    const location = jsonErrorLocation(raw)
    const reason = raw.replace(/\s*in JSON at position \d+(\s*\(line \d+ column \d+\))?/i, '')
    return { ok: false, error: location ? `${location} · ${reason}` : raw }
  }
})

const jsonTreeMeta = computed(() => {
  if (!parsedJson.value.ok) return '未解析'
  const value = parsedJson.value.value
  if (Array.isArray(value)) return `数组 · ${value.length} 项`
  if (value !== null && typeof value === 'object')
    return `对象 · ${Object.keys(value as object).length} 个字段`
  return `值 · ${value === null ? 'null' : typeof value}`
})

function expandTree(mode: 'all' | 'none' | 'default') {
  jsonExpand.value = mode
  jsonExpandVersion.value += 1
}

function copyTreeJson() {
  if (!parsedJson.value.ok) {
    message.warning('JSON 尚未解析成功')
    return
  }
  copyText(JSON.stringify(parsedJson.value.value, null, 2), '已复制格式化 JSON')
}

function jsonErrorLocation(message: string): string | null {
  const lineCol = /line (\d+) column (\d+)/i.exec(message)
  if (lineCol) return `第 ${lineCol[1]} 行 第 ${lineCol[2]} 列`
  const pos = /position (\d+)/i.exec(message)
  if (!pos) return null
  const index = Number(pos[1])
  const before = jsonInput.value.slice(0, index)
  const line = before.split('\n').length
  const column = index - before.lastIndexOf('\n')
  return `第 ${line} 行 第 ${column} 列`
}

function setJsonError(error: unknown) {
  const raw = error instanceof Error ? error.message : String(error)
  const location = jsonErrorLocation(raw)
  const reason = raw.replace(/\s*in JSON at position \d+(\s*\(line \d+ column \d+\))?/i, '')
  jsonStatus.value = location ? `解析失败 · ${location} · ${reason}` : `解析失败：${raw}`
  jsonStatusError.value = true
}

function parseJson(): unknown {
  jsonStatus.value = ''
  jsonStatusError.value = false
  try {
    const value: unknown = JSON.parse(jsonInput.value)
    jsonStatus.value = 'JSON 合法'
    jsonStatusError.value = false
    return value
  } catch (error) {
    setJsonError(error)
    return undefined
  }
}

function formatJson() {
  if (!jsonInput.value.trim()) {
    message.warning('请先输入 JSON')
    return
  }
  const value = parseJson()
  if (value === undefined) return
  const space = jsonIndent.value === 'tab' ? '\t' : Number(jsonIndent.value)
  jsonOutput.value = JSON.stringify(value, null, space) ?? ''
}

function minifyJson() {
  if (!jsonInput.value.trim()) {
    message.warning('请先输入 JSON')
    return
  }
  const value = parseJson()
  if (value === undefined) return
  jsonOutput.value = JSON.stringify(value) ?? ''
}

function checkJson() {
  if (!jsonInput.value.trim()) {
    jsonStatus.value = '还没有输入内容'
    jsonStatusError.value = true
    return
  }
  const value = parseJson()
  if (value === undefined) return
  if (Array.isArray(value)) jsonStatus.value = `JSON 合法 · 数组（${value.length} 项）`
  else if (value !== null && typeof value === 'object')
    jsonStatus.value = `JSON 合法 · 对象（${Object.keys(value as object).length} 个字段）`
  else jsonStatus.value = `JSON 合法 · ${typeof value}`
}

function escapeJson() {
  jsonOutput.value = JSON.stringify(jsonInput.value) ?? ''
}

function unescapeJson() {
  try {
    const value: unknown = JSON.parse(`"${jsonInput.value}"`)
    jsonOutput.value = typeof value === 'string' ? value : String(value)
    jsonStatus.value = '已去除转义'
    jsonStatusError.value = false
  } catch (error) {
    setJsonError(error)
  }
}

function clearJson() {
  jsonInput.value = ''
  jsonOutput.value = ''
  jsonStatus.value = ''
  jsonStatusError.value = false
}

/* ---------------- 编码转换 ---------------- */
type EncodeOp = 'b64e' | 'b64d' | 'urle' | 'urld' | 'unie' | 'unid'
const encodeOp = ref<EncodeOp>('b64e')
const encodeInput = ref('')
const encodeOutput = ref('')

const encodeOptions = [
  { label: 'Base64 编码', value: 'b64e' },
  { label: 'Base64 解码', value: 'b64d' },
  { label: 'URL 编码', value: 'urle' },
  { label: 'URL 解码', value: 'urld' },
  { label: 'Unicode 转义', value: 'unie' },
  { label: 'Unicode 还原', value: 'unid' },
]

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
}

function base64ToBytes(text: string): Uint8Array {
  const binary = atob(text.replace(/\s/g, ''))
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

function unicodeEscape(text: string): string {
  return text.replace(/[^\x00-\x7f]/g, (char) => {
    return `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`
  })
}

function unicodeUnescape(text: string): string {
  return text.replace(/\\u([0-9a-fA-F]{4})/g, (_match, hex: string) => {
    return String.fromCharCode(parseInt(hex, 16))
  })
}

function runEncode() {
  if (!encodeInput.value) {
    message.warning('请先输入内容')
    return
  }
  try {
    switch (encodeOp.value) {
      case 'b64e':
        encodeOutput.value = bytesToBase64(new TextEncoder().encode(encodeInput.value))
        break
      case 'b64d':
        encodeOutput.value = new TextDecoder().decode(base64ToBytes(encodeInput.value))
        break
      case 'urle':
        encodeOutput.value = encodeURIComponent(encodeInput.value)
        break
      case 'urld':
        encodeOutput.value = decodeURIComponent(encodeInput.value)
        break
      case 'unie':
        encodeOutput.value = unicodeEscape(encodeInput.value)
        break
      case 'unid':
        encodeOutput.value = unicodeUnescape(encodeInput.value)
        break
    }
  } catch (error) {
    message.error(`转换失败：${String(error)}`)
  }
}

/* ---------------- 哈希 / HMAC ---------------- */
const hashAlgorithm = ref('md5')
const hashInput = ref('')
const hashKey = ref('')
const hashOutput = ref('')
const hashRunning = ref(false)

const hashOptions = [
  { label: 'MD5', value: 'md5' },
  { label: 'SHA-1', value: 'sha1' },
  { label: 'SHA-256', value: 'sha256' },
  { label: 'SHA-512', value: 'sha512' },
  { label: 'HMAC-MD5', value: 'hmac-md5' },
  { label: 'HMAC-SHA1', value: 'hmac-sha1' },
  { label: 'HMAC-SHA256', value: 'hmac-sha256' },
  { label: 'HMAC-SHA512', value: 'hmac-sha512' },
]

const isHmac = computed(() => hashAlgorithm.value.startsWith('hmac-'))

async function runHash() {
  if (!hashInput.value) {
    message.warning('请先输入要计算的内容')
    return
  }
  if (isHmac.value && !hashKey.value) {
    message.warning('请填写 HMAC 密钥')
    return
  }
  hashRunning.value = true
  try {
    hashOutput.value = isHmac.value
      ? await api.toolHmac(hashInput.value, hashKey.value, hashAlgorithm.value.slice(5))
      : await api.toolHash(hashInput.value, hashAlgorithm.value)
  } catch (error) {
    message.error(String(error))
  } finally {
    hashRunning.value = false
  }
}

/* ---------------- AES ---------------- */
const aesKey = ref('')
const aesInput = ref('')
const aesOutput = ref('')
const aesRunning = ref(false)

async function deriveAesKey(): Promise<CryptoKey> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(aesKey.value))
  return crypto.subtle.importKey('raw', digest, { name: 'AES-CBC' }, false, ['encrypt', 'decrypt'])
}

async function aesEncrypt() {
  if (!aesKey.value) {
    message.warning('请填写 AES 密钥')
    return
  }
  if (!aesInput.value) {
    message.warning('请输入要加密的明文')
    return
  }
  aesRunning.value = true
  try {
    const key = await deriveAesKey()
    const iv = crypto.getRandomValues(new Uint8Array(16))
    const encrypted = await crypto.subtle.encrypt(
      { name: 'AES-CBC', iv },
      key,
      new TextEncoder().encode(aesInput.value),
    )
    aesOutput.value = `${bytesToBase64(iv)}:${bytesToBase64(new Uint8Array(encrypted))}`
    message.success('加密完成')
  } catch (error) {
    message.error(`加密失败：${String(error)}`)
  } finally {
    aesRunning.value = false
  }
}

async function aesDecrypt() {
  if (!aesKey.value) {
    message.warning('请填写 AES 密钥')
    return
  }
  const parts = aesInput.value.trim().split(':')
  if (parts.length !== 2) {
    message.error('密文格式应为 IV:密文（Base64）')
    return
  }
  aesRunning.value = true
  try {
    const iv = new Uint8Array(base64ToBytes(parts[0]))
    if (iv.length !== 16) {
      message.error('IV 必须是 16 字节的 Base64')
      return
    }
    const key = await deriveAesKey()
    const decrypted = await crypto.subtle.decrypt(
      { name: 'AES-CBC', iv },
      key,
      new Uint8Array(base64ToBytes(parts[1])),
    )
    aesOutput.value = new TextDecoder().decode(decrypted)
    message.success('解密完成')
  } catch (error) {
    message.error(`解密失败，请检查密钥与密文：${String(error)}`)
  } finally {
    aesRunning.value = false
  }
}

/* ---------------- 时间戳 ---------------- */
const now = ref(Date.now())
let timer: number | undefined

onMounted(() => {
  timer = window.setInterval(() => {
    now.value = Date.now()
  }, 1000)
})

onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})

const nowSeconds = computed(() => Math.floor(now.value / 1000))
const nowDate = computed(() => formatDate(now.value))
const tzLabel = computed(() => {
  const offset = -new Date().getTimezoneOffset()
  const sign = offset >= 0 ? '+' : '-'
  const hours = Math.floor(Math.abs(offset) / 60)
  const minutes = Math.abs(offset) % 60
  return `${Intl.DateTimeFormat().resolvedOptions().timeZone} (UTC${sign}${pad(hours)}:${pad(minutes)})`
})

function pad(value: number, length = 2): string {
  return String(value).padStart(length, '0')
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp)
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`
}

const tsInput = ref('')
const tsOutput = ref('')
const dateInput = ref('')
const dateOutput = ref('')

function convertTimestamp() {
  const raw = tsInput.value.trim()
  if (!/^-?\d+$/.test(raw)) {
    tsOutput.value = '请输入纯数字时间戳（秒或毫秒）'
    return
  }
  const value = Number(raw)
  const milliseconds = raw.replace('-', '').length <= 10 ? value * 1000 : value
  const date = new Date(milliseconds)
  if (Number.isNaN(date.getTime())) {
    tsOutput.value = '时间戳超出可解析范围'
    return
  }
  tsOutput.value = [
    `本地时间：${formatDate(milliseconds)}`,
    `UTC 时间：${date.toISOString()}`,
    `秒：${Math.floor(milliseconds / 1000)}`,
    `毫秒：${milliseconds}`,
  ].join('\n')
}

function convertDate() {
  const text = dateInput.value.trim().replace(' ', 'T')
  const date = new Date(text)
  if (Number.isNaN(date.getTime())) {
    dateOutput.value = '请按 2026-10-23 19:30:00 的格式输入'
    return
  }
  dateOutput.value = [`秒：${Math.floor(date.getTime() / 1000)}`, `毫秒：${date.getTime()}`].join('\n')
}

function fillNowTimestamp() {
  tsInput.value = String(Date.now())
  convertTimestamp()
}

function fillNowDate() {
  dateInput.value = formatDate(Date.now()).slice(0, 19)
  convertDate()
}

/* ---------------- 随机生成 ---------------- */
const uuidUpper = ref(false)
const uuidNoDash = ref(false)
const uuidResult = ref('')

const charset = ref<string[]>(['lower', 'upper', 'digit'])
const charsetOptions = [
  { label: '小写字母', value: 'lower' },
  { label: '大写字母', value: 'upper' },
  { label: '数字', value: 'digit' },
  { label: '符号', value: 'symbol' },
]
const strLength = ref(16)
const strResult = ref('')

const randomMin = ref(1)
const randomMax = ref(100)
const randomResult = ref('')

function randomUuid(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

function genUuid() {
  let id = randomUuid()
  if (uuidNoDash.value) id = id.replace(/-/g, '')
  if (uuidUpper.value) id = id.toUpperCase()
  uuidResult.value = id
}

const pools: Record<string, string> = {
  lower: 'abcdefghijklmnopqrstuvwxyz',
  upper: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
  digit: '0123456789',
  symbol: '!@#$%^&*()-_=+[]{};:,.<>?',
}

function genString() {
  const pool = charsetOptions
    .filter((item) => charset.value.includes(item.value))
    .map((item) => pools[item.value])
    .join('')
  if (!pool) {
    message.warning('至少选择一种字符类型')
    return
  }
  const length = Math.min(Math.max(Number(strLength.value) || 1, 1), 256)
  const bytes = crypto.getRandomValues(new Uint32Array(length))
  strResult.value = Array.from(bytes, (value) => pool[value % pool.length]).join('')
}

function genInteger() {
  const min = Number(randomMin.value)
  const max = Number(randomMax.value)
  if (!Number.isFinite(min) || !Number.isFinite(max)) {
    message.warning('请输入有效的整数范围')
    return
  }
  if (max < min) {
    message.warning('最大值不能小于最小值')
    return
  }
  const range = Math.floor(max) - Math.ceil(min) + 1
  const buffer = new Uint32Array(1)
  crypto.getRandomValues(buffer)
  randomResult.value = String(Math.ceil(min) + (buffer[0] % range))
}

/* ---------------- MyBatis SQL ---------------- */
const mybatisMode = ref<SqlMode>('literal')
const mybatisPretty = ref(true)
const mybatisTemplate = ref('')
const mybatisParams = ref('')
const mybatisSql = ref('')
const mybatisParamsUsed = ref<SqlParam[]>([])
const mybatisWarnings = ref<string[]>([])

const MYBATIS_SAMPLE_TEMPLATE = `<select id="findUsers">
  SELECT id, username, status, created_at
  FROM sys_user
  <where>
    <if test="username != null and username != ''">
      AND username LIKE CONCAT('%', #{username}, '%')
    </if>
    <if test="status != null">
      AND status = #{status}
    </if>
    <if test="ids != null and ids.size() > 0">
      AND id IN
      <foreach collection="ids" item="id" open="(" separator="," close=")">
        #{id}
      </foreach>
    </if>
  </where>
  ORDER BY id DESC
  LIMIT #{limit, jdbcType=INTEGER}
</select>`

const MYBATIS_SAMPLE_PARAMS = `{
  "username": "zhang",
  "status": 1,
  "ids": [3, 5, 8],
  "limit": 20
}`

function runMybatis() {
  if (!mybatisTemplate.value.trim()) {
    message.warning('请先粘贴 Mapper 中的 SQL 片段')
    return
  }
  let params: unknown = {}
  if (mybatisParams.value.trim()) {
    try {
      params = JSON.parse(mybatisParams.value)
    } catch (error) {
      message.error(`参数 JSON 解析失败：${String(error)}`)
      return
    }
  }
  const result = renderMybatis(
    mybatisTemplate.value,
    params,
    mybatisMode.value,
    mybatisPretty.value,
  )
  mybatisSql.value = result.sql
  mybatisParamsUsed.value = result.params
  mybatisWarnings.value = result.warnings
}

function loadMybatisSample() {
  mybatisTemplate.value = MYBATIS_SAMPLE_TEMPLATE
  mybatisParams.value = MYBATIS_SAMPLE_PARAMS
  mybatisSql.value = ''
  mybatisParamsUsed.value = []
  mybatisWarnings.value = []
  runMybatis()
}

function clearMybatis() {
  mybatisTemplate.value = ''
  mybatisParams.value = ''
  mybatisSql.value = ''
  mybatisParamsUsed.value = []
  mybatisWarnings.value = []
}

function displayParam(value: unknown): string {
  if (value === undefined) return '缺失'
  if (value === null) return 'NULL'
  if (Array.isArray(value)) return JSON.stringify(value)
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

/* ---------------- JWT ---------------- */
const jwtInput = ref('')
const jwtHeader = ref('')
const jwtPayload = ref('')
const jwtMeta = ref<string[]>([])

function base64UrlDecode(part: string): string {
  const normalized = part.replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4)
  return new TextDecoder().decode(base64ToBytes(padded))
}

function humanizeDuration(ms: number): string {
  if (ms <= 0) return '已过期'
  const minutes = Math.floor(ms / 60000)
  if (minutes < 60) return `${minutes} 分钟`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时 ${minutes % 60} 分钟`
  const days = Math.floor(hours / 24)
  return `${days} 天 ${hours % 24} 小时`
}

function decodeJwt() {
  jwtHeader.value = ''
  jwtPayload.value = ''
  jwtMeta.value = []
  const token = jwtInput.value.trim().replace(/^Bearer\s+/i, '')
  if (!token) {
    message.warning('请先粘贴 JWT')
    return
  }
  const parts = token.split('.')
  if (parts.length < 2) {
    message.error('JWT 格式不正确，应包含以点分隔的 header 与 payload')
    return
  }
  try {
    const header = JSON.parse(base64UrlDecode(parts[0])) as Record<string, unknown>
    const payload = JSON.parse(base64UrlDecode(parts[1])) as Record<string, unknown>
    jwtHeader.value = JSON.stringify(header, null, 2)
    jwtPayload.value = JSON.stringify(payload, null, 2)

    const meta: string[] = []
    if (header.alg) meta.push(`算法：${String(header.alg)}`)
    if (header.typ) meta.push(`类型：${String(header.typ)}`)
    if (typeof payload.iss === 'string') meta.push(`签发方：${payload.iss}`)
    if (typeof payload.sub === 'string') meta.push(`主体：${payload.sub}`)
    if (typeof payload.iat === 'number') meta.push(`签发时间：${formatDate(payload.iat * 1000)}`)
    if (typeof payload.nbf === 'number') meta.push(`生效时间：${formatDate(payload.nbf * 1000)}`)
    if (typeof payload.exp === 'number') {
      const left = payload.exp * 1000 - Date.now()
      meta.push(
        `过期时间：${formatDate(payload.exp * 1000)}（${left > 0 ? `剩余 ${humanizeDuration(left)}` : '已过期'}）`,
      )
    }
    if (parts.length < 3) meta.push('该 JWT 没有签名段')
    jwtMeta.value = meta
  } catch (error) {
    message.error(`JWT 解析失败：${String(error)}`)
  }
}

function clearJwt() {
  jwtInput.value = ''
  jwtHeader.value = ''
  jwtPayload.value = ''
  jwtMeta.value = []
}

/* ---------------- 正则测试 ---------------- */
const regexPattern = ref('')
const regexFlags = ref<string[]>(['g', 'i'])
const regexText = ref('')
const regexError = ref('')
const regexHighlight = ref('')
const regexMatches = ref<{ index: number; text: string; groups: string[] }[]>([])
const regexFlagOptions = [
  { label: 'g', value: 'g' },
  { label: 'i', value: 'i' },
  { label: 'm', value: 'm' },
  { label: 's', value: 's' },
  { label: 'u', value: 'u' },
]

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function runRegex() {
  regexError.value = ''
  regexMatches.value = []
  regexHighlight.value = ''
  if (!regexPattern.value) {
    message.warning('请输入正则表达式')
    return
  }
  try {
    const flags = Array.from(new Set([...regexFlags.value, 'g'])).join('')
    const re = new RegExp(regexPattern.value, flags)
    const matches: { index: number; text: string; groups: string[] }[] = []
    let match: RegExpExecArray | null
    while ((match = re.exec(regexText.value))) {
      if (match[0].length === 0) {
        re.lastIndex += 1
        continue
      }
      matches.push({
        index: match.index,
        text: match[0],
        groups: match.slice(1).map((group) => group ?? ''),
      })
      if (matches.length >= 500) break
    }
    regexMatches.value = matches

    let html = ''
    let last = 0
    for (const item of matches) {
      html += escapeHtml(regexText.value.slice(last, item.index))
      html += `<mark>${escapeHtml(item.text)}</mark>`
      last = item.index + item.text.length
    }
    html += escapeHtml(regexText.value.slice(last))
    regexHighlight.value = html || escapeHtml(regexText.value)
  } catch (error) {
    regexError.value = String(error)
  }
}

function clearRegex() {
  regexPattern.value = ''
  regexText.value = ''
  regexError.value = ''
  regexHighlight.value = ''
  regexMatches.value = []
}

/* ---------------- 命名转换 ---------------- */
const namingInput = ref('user_profile_id')
const namingRows = computed(() => {
  const words = namingInput.value
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
    .replace(/[^A-Za-z0-9]+/g, ' ')
    .trim()
    .split(/\s+/)
    .filter(Boolean)
  if (!words.length) return []
  const lower = words.map((word) => word.toLowerCase())
  const upperFirst = words.map((word) => word[0].toUpperCase() + word.slice(1).toLowerCase())
  const camel = lower[0] + upperFirst.slice(1).join('')
  return [
    { label: 'camelCase', value: camel },
    { label: 'PascalCase', value: upperFirst.join('') },
    { label: 'snake_case', value: lower.join('_') },
    { label: 'kebab-case', value: lower.join('-') },
    { label: 'CONSTANT_CASE', value: lower.join('_').toUpperCase() },
    { label: 'dot.case', value: lower.join('.') },
    { label: '空格分隔', value: lower.join(' ') },
  ]
})
</script>

<template>
  <div class="page page-fill">
    <PageHeader
      eyebrow="Toolbox"
      title="工具箱"
      subtitle="JSON、编码、加解密、时间戳、MyBatis SQL、JWT 与正则等开发工具，全部在本机计算，不上传任何内容"
    />

    <a-tabs v-model:activeKey="activeTab" class="tools">
      <a-tab-pane key="json" tab="JSON">
        <div class="fm-card panel panel-fill">
          <div class="panel-head">
            <div>
              <div class="panel-title">格式化与校验</div>
              <div class="panel-desc">支持格式化、压缩、合法性校验以及字符串转义 / 去转义</div>
            </div>
            <a-radio-group v-model:value="jsonIndent" size="small">
              <a-radio-button value="2">2 空格</a-radio-button>
              <a-radio-button value="4">4 空格</a-radio-button>
              <a-radio-button value="tab">Tab</a-radio-button>
            </a-radio-group>
          </div>
          <div class="pair pair-fill">
            <div class="pane pane-fill">
              <div class="pane-label">
                <span>输入</span>
                <span class="pane-meta">{{ jsonInputMeta }}</span>
              </div>
              <CodeArea
                v-model="jsonInput"
                fill
                placeholder="粘贴 JSON 内容，例如 {&quot;name&quot;:&quot;FinkMeter&quot;}"
              />
            </div>
            <div class="pane pane-fill">
              <div class="pane-label">
                <span class="view-switch">
                  <button :class="{ active: jsonView === 'tree' }" @click="jsonView = 'tree'">层级</button>
                  <button :class="{ active: jsonView === 'text' }" @click="jsonView = 'text'">文本</button>
                </span>
                <span class="pane-meta">{{ jsonView === 'tree' ? jsonTreeMeta : jsonOutputMeta }}</span>
              </div>

              <div v-if="jsonView === 'tree'" class="tree-wrap">
                <div class="tree-tools">
                  <a @click="expandTree('all')">展开全部</a>
                  <a @click="expandTree('none')">折叠全部</a>
                  <a @click="expandTree('default')">默认层级</a>
                  <a @click="copyTreeJson">复制 JSON</a>
                  <span class="tree-tip">输入后自动按层级解析</span>
                </div>
                <div class="tree-scroll">
                  <JsonTree
                    v-if="parsedJson.ok"
                    :key="jsonExpandVersion"
                    :value="parsedJson.value"
                    :mode="jsonExpand"
                  />
                  <div v-else class="tree-empty">
                    {{ jsonInput.trim() ? `无法解析：${parsedJson.error}` : '在左侧粘贴 JSON，右侧会自动显示层级结构' }}
                  </div>
                </div>
              </div>

              <CodeArea v-else v-model="jsonOutput" readonly fill />
            </div>
          </div>
          <div class="panel-actions">
            <a-button type="primary" size="small" @click="formatJson">格式化</a-button>
            <a-button size="small" @click="minifyJson">压缩</a-button>
            <a-button size="small" @click="checkJson">校验</a-button>
            <a-button size="small" @click="escapeJson">转义</a-button>
            <a-button size="small" @click="unescapeJson">去转义</a-button>
            <a-button size="small" @click="clearJson">清空</a-button>
            <span v-if="jsonStatus" class="status" :class="jsonStatusError ? 'err' : 'ok'">
              {{ jsonStatus }}
            </span>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="encode" tab="编码转换">
        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">Base64 / URL / Unicode</div>
              <div class="panel-desc">Base64 与 URL 编解码按 UTF-8 处理，中文不会乱码</div>
            </div>
            <a-select v-model:value="encodeOp" :options="encodeOptions" size="small" style="width: 158px" />
          </div>
          <div class="pair">
            <div class="pane">
              <div class="pane-label">输入</div>
              <a-textarea
                v-model:value="encodeInput"
                class="mono code"
                :rows="12"
                placeholder="粘贴需要转换的内容"
              />
            </div>
            <div class="pane">
              <div class="pane-label">
                结果
                <a class="copy-link" @click="copyText(encodeOutput)">复制</a>
              </div>
              <a-textarea v-model:value="encodeOutput" class="mono code" :rows="12" readonly />
            </div>
          </div>
          <div class="panel-actions">
            <a-button type="primary" size="small" @click="runEncode">开始转换</a-button>
            <a-button size="small" @click="encodeInput = encodeOutput; encodeOutput = ''">结果转输入</a-button>
            <a-button size="small" @click="encodeInput = ''; encodeOutput = ''">清空</a-button>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="crypto" tab="加解密">
        <div class="grid-2">
          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">哈希 / HMAC</div>
                <div class="panel-desc">输出为小写十六进制字符串</div>
              </div>
              <a-select v-model:value="hashAlgorithm" :options="hashOptions" size="small" style="width: 138px" />
            </div>
            <div class="field">
              <div class="pane-label">内容</div>
              <a-textarea v-model:value="hashInput" class="mono code" :rows="6" placeholder="需要计算哈希的文本" />
            </div>
            <div v-if="isHmac" class="field">
              <div class="pane-label">HMAC 密钥</div>
              <a-input v-model:value="hashKey" placeholder="输入密钥" />
            </div>
            <div class="field">
              <div class="pane-label">
                结果
                <a class="copy-link" @click="copyText(hashOutput)">复制</a>
              </div>
              <a-textarea v-model:value="hashOutput" class="mono code" :rows="3" readonly />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" :loading="hashRunning" @click="runHash">计算</a-button>
              <a-button size="small" @click="hashInput = ''; hashOutput = ''">清空</a-button>
            </div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">AES 加解密</div>
                <div class="panel-desc">AES-256-CBC，密钥经 SHA-256 派生，IV 随机生成</div>
              </div>
            </div>
            <div class="field">
              <div class="pane-label">密钥</div>
              <a-input-password v-model:value="aesKey" placeholder="任意字符串，加密解密需一致" />
            </div>
            <div class="field">
              <div class="pane-label">明文 / 密文（IV:Base64）</div>
              <a-textarea
                v-model:value="aesInput"
                class="mono code"
                :rows="4"
                placeholder="加密时填明文；解密时填 IV:密文"
              />
            </div>
            <div class="field">
              <div class="pane-label">
                输出
                <a class="copy-link" @click="copyText(aesOutput)">复制</a>
              </div>
              <a-textarea v-model:value="aesOutput" class="mono code" :rows="4" readonly />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" :loading="aesRunning" @click="aesEncrypt">加密</a-button>
              <a-button size="small" :loading="aesRunning" @click="aesDecrypt">解密</a-button>
              <a-button size="small" @click="aesInput = ''; aesOutput = ''">清空</a-button>
            </div>
          </div>
        </div>
        <div class="note">
          AES 输出格式为 <span class="mono">IV:密文</span>（两段均为 Base64），解密时需完整粘贴；
          同一密钥可在本工具内互相解密。
        </div>
      </a-tab-pane>

      <a-tab-pane key="time" tab="时间戳">
        <div class="fm-card now-card">
          <div>
            <div class="now-label">当前时间 · {{ tzLabel }}</div>
            <div class="now-value mono">{{ nowDate }}</div>
          </div>
          <div class="now-actions">
            <a-button size="small" @click="copyText(nowDate)">复制时间</a-button>
            <a-button size="small" @click="copyText(String(nowSeconds))">复制秒</a-button>
            <a-button size="small" @click="copyText(String(now))">复制毫秒</a-button>
          </div>
        </div>

        <div class="grid-2" style="margin-top: 14px">
          <div class="fm-card panel">
            <div class="panel-head">
              <div class="panel-title">时间戳 → 时间</div>
              <a-button size="small" @click="fillNowTimestamp">用当前时间</a-button>
            </div>
            <div class="field">
              <a-input v-model:value="tsInput" class="mono" placeholder="10 位秒级或 13 位毫秒级" @press-enter="convertTimestamp" />
            </div>
            <div class="field">
              <div class="pane-label">
                结果
                <a class="copy-link" @click="copyText(tsOutput)">复制</a>
              </div>
              <a-textarea v-model:value="tsOutput" class="mono code" :rows="5" readonly />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" @click="convertTimestamp">转换</a-button>
            </div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div class="panel-title">时间 → 时间戳</div>
              <a-button size="small" @click="fillNowDate">用当前时间</a-button>
            </div>
            <div class="field">
              <a-input v-model:value="dateInput" class="mono" placeholder="2026-10-23 19:30:00" @press-enter="convertDate" />
            </div>
            <div class="field">
              <div class="pane-label">
                结果
                <a class="copy-link" @click="copyText(dateOutput)">复制</a>
              </div>
              <a-textarea v-model:value="dateOutput" class="mono code" :rows="5" readonly />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" @click="convertDate">转换</a-button>
            </div>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="random" tab="随机生成">
        <div class="grid-2">
          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">UUID</div>
                <div class="panel-desc">基于安全随机数生成 v4 UUID</div>
              </div>
            </div>
            <div class="field inline">
              <a-checkbox v-model:checked="uuidUpper">大写</a-checkbox>
              <a-checkbox v-model:checked="uuidNoDash">去掉连字符</a-checkbox>
            </div>
            <div class="field">
              <a-input v-model:value="uuidResult" class="mono" readonly placeholder="点击生成" />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" @click="genUuid">生成 UUID</a-button>
              <a-button size="small" @click="copyText(uuidResult)">复制</a-button>
            </div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">随机字符串</div>
                <div class="panel-desc">用于生成密钥、盐值或测试数据</div>
              </div>
              <a-input-number v-model:value="strLength" :min="1" :max="256" size="small" style="width: 96px" />
            </div>
            <div class="field inline">
              <a-checkbox-group v-model:value="charset">
                <a-checkbox v-for="item in charsetOptions" :key="item.value" :value="item.value">
                  {{ item.label }}
                </a-checkbox>
              </a-checkbox-group>
            </div>
            <div class="field">
              <a-input v-model:value="strResult" class="mono" readonly placeholder="点击生成" />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" @click="genString">生成字符串</a-button>
              <a-button size="small" @click="copyText(strResult)">复制</a-button>
            </div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">随机整数</div>
                <div class="panel-desc">在闭区间内生成一个随机整数</div>
              </div>
            </div>
            <div class="field inline">
              <a-input-number v-model:value="randomMin" size="small" style="width: 120px" placeholder="最小值" />
              <span class="range-sep">至</span>
              <a-input-number v-model:value="randomMax" size="small" style="width: 120px" placeholder="最大值" />
            </div>
            <div class="field">
              <a-input v-model:value="randomResult" class="mono" readonly placeholder="点击生成" />
            </div>
            <div class="panel-actions">
              <a-button type="primary" size="small" @click="genInteger">生成整数</a-button>
              <a-button size="small" @click="copyText(randomResult)">复制</a-button>
            </div>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="mybatis" tab="MyBatis SQL">
        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">MyBatis 动态 SQL 填充</div>
              <div class="panel-desc">
                支持 if / where / set / trim / foreach / choose / bind，把 #{} 与 ${} 渲染成可执行的 SQL
              </div>
            </div>
            <div class="inline-controls">
              <a-radio-group v-model:value="mybatisMode" size="small">
                <a-radio-button value="literal">直接填值</a-radio-button>
                <a-radio-button value="placeholder">占位符 ?</a-radio-button>
              </a-radio-group>
              <a-checkbox v-model:checked="mybatisPretty">自动排版</a-checkbox>
            </div>
          </div>
          <div class="pair">
            <div class="pane">
              <div class="pane-label">Mapper SQL（可带 &lt;select&gt; 外层）</div>
              <a-textarea
                v-model:value="mybatisTemplate"
                class="mono code"
                :rows="14"
                placeholder="粘贴 select / update 语句或标签内部片段"
              />
            </div>
            <div class="pane">
              <div class="pane-label">参数 JSON</div>
              <a-textarea
                v-model:value="mybatisParams"
                class="mono code"
                :rows="14"
                placeholder="{&quot;username&quot;:&quot;zhang&quot;,&quot;ids&quot;:[1,2]}"
              />
            </div>
          </div>
          <div class="panel-actions">
            <a-button type="primary" size="small" @click="runMybatis">生成 SQL</a-button>
            <a-button size="small" @click="loadMybatisSample">载入示例</a-button>
            <a-button size="small" @click="clearMybatis">清空</a-button>
          </div>
          <div v-if="mybatisSql" class="field">
            <div class="pane-label">
              生成结果
              <a class="copy-link" @click="copyText(mybatisSql)">复制</a>
            </div>
            <a-textarea v-model:value="mybatisSql" class="mono code" :rows="10" readonly />
          </div>
          <div v-if="mybatisWarnings.length" class="field">
            <div class="warn-box">
              <div v-for="(item, index) in mybatisWarnings" :key="index">· {{ item }}</div>
            </div>
          </div>
          <div v-if="mybatisParamsUsed.length" class="field">
            <div class="pane-label">参数列表（按出现顺序）</div>
            <div class="param-list">
              <div v-for="item in mybatisParamsUsed" :key="item.index" class="param-item">
                <span class="param-index">#{{ item.index }}</span>
                <span class="mono param-name">{{ item.name }}</span>
                <span class="mono param-value">{{ displayParam(item.value) }}</span>
                <span v-if="item.jdbcType" class="param-type">{{ item.jdbcType }}</span>
              </div>
            </div>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="jwt" tab="JWT">
        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">JWT 解码</div>
              <div class="panel-desc">解析 Header 与 Payload，查看签发与过期时间；不会发送 Token 到任何服务器</div>
            </div>
          </div>
          <a-textarea
            v-model:value="jwtInput"
            class="mono code"
            :rows="5"
            placeholder="粘贴 JWT，可带 Bearer 前缀"
          />
          <div class="panel-actions">
            <a-button type="primary" size="small" @click="decodeJwt">解析</a-button>
            <a-button size="small" @click="clearJwt">清空</a-button>
          </div>
        </div>

        <div v-if="jwtHeader || jwtPayload" class="grid-2" style="margin-top: 14px">
          <div class="fm-card panel">
            <div class="pane-label">
              Header
              <a class="copy-link" @click="copyText(jwtHeader)">复制</a>
            </div>
            <pre class="result-block mono">{{ jwtHeader }}</pre>
          </div>
          <div class="fm-card panel">
            <div class="pane-label">
              Payload
              <a class="copy-link" @click="copyText(jwtPayload)">复制</a>
            </div>
            <pre class="result-block mono">{{ jwtPayload }}</pre>
          </div>
        </div>

        <div v-if="jwtMeta.length" class="fm-card panel" style="margin-top: 14px">
          <div class="pane-label">解析信息</div>
          <div class="meta-list">
            <div v-for="(item, index) in jwtMeta" :key="index">· {{ item }}</div>
          </div>
          <div class="note">仅解码内容，不校验签名，过期时间按本机时钟判断。</div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="regex" tab="正则测试">
        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">正则表达式测试</div>
              <div class="panel-desc">实时高亮匹配结果，并列出位置与捕获分组</div>
            </div>
            <a-checkbox-group v-model:value="regexFlags" size="small">
              <a-checkbox v-for="item in regexFlagOptions" :key="item.value" :value="item.value">
                {{ item.label }}
              </a-checkbox>
            </a-checkbox-group>
          </div>
          <div class="regex-input">
            <span class="regex-slash">/</span>
            <a-input
              v-model:value="regexPattern"
              class="mono"
              placeholder="(?&lt;name>\w+)@(\w+\.\w+)"
              @press-enter="runRegex"
            />
            <span class="regex-slash">/{{ regexFlags.join('') || 'g' }}</span>
          </div>
          <div class="field">
            <div class="pane-label">测试文本</div>
            <a-textarea
              v-model:value="regexText"
              class="mono code"
              :rows="7"
              placeholder="粘贴需要匹配的文本"
            />
          </div>
          <div class="panel-actions">
            <a-button type="primary" size="small" @click="runRegex">执行匹配</a-button>
            <a-button size="small" @click="clearRegex">清空</a-button>
            <span v-if="regexMatches.length" class="status ok">
              匹配到 {{ regexMatches.length }} 处
            </span>
            <span v-if="regexError" class="status err">{{ regexError }}</span>
          </div>
          <div v-if="regexHighlight" class="field">
            <div class="pane-label">高亮预览</div>
            <div class="regex-preview mono" v-html="regexHighlight" />
          </div>
          <div v-if="regexMatches.length" class="field">
            <div class="pane-label">匹配明细</div>
            <div class="param-list">
              <div v-for="(item, index) in regexMatches" :key="index" class="param-item">
                <span class="param-index">#{{ index + 1 }}</span>
                <span class="param-type">@{{ item.index }}</span>
                <span class="mono param-name">{{ item.text }}</span>
                <span v-if="item.groups.length" class="param-value mono">
                  分组：{{ item.groups.join(' | ') }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </a-tab-pane>

      <a-tab-pane key="naming" tab="命名转换">
        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">命名风格转换</div>
              <div class="panel-desc">在 camelCase、snake_case、kebab-case 等风格之间互相转换</div>
            </div>
          </div>
          <a-input
            v-model:value="namingInput"
            class="mono"
            placeholder="user_profile_id / userName / USER-NAME"
          />
          <div class="naming-list">
            <div v-for="row in namingRows" :key="row.label" class="naming-row">
              <span class="naming-label">{{ row.label }}</span>
              <span class="mono naming-value">{{ row.value }}</span>
              <a class="copy-link" @click="copyText(row.value)">复制</a>
            </div>
            <div v-if="!namingRows.length" class="empty-tip">输入任意命名后自动转换</div>
          </div>
        </div>
      </a-tab-pane>
    </a-tabs>
  </div>
</template>

<style scoped>
.tools {
  margin-top: 6px;
}

/* 让 JSON 面板吃满剩余视口高度，避免底部留白 */
.page-fill {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding-bottom: 18px;
}

.page-fill .page-header {
  flex-shrink: 0;
}

.page-fill .tools {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tools :deep(.ant-tabs-nav) {
  flex-shrink: 0;
  margin-bottom: 16px;
}

.tools :deep(.ant-tabs-content-holder) {
  flex: 1;
  min-height: 0;
}

.tools :deep(.ant-tabs-content) {
  height: 100%;
  overflow-y: auto;
}

.tools :deep(.ant-tabs-tabpane) {
  height: 100%;
}

.panel {
  padding: 16px 18px 18px;
}

.panel-fill {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
  margin-bottom: 14px;
  flex-shrink: 0;
}

.panel-title {
  font-size: 14px;
  font-weight: 600;
}

.panel-desc {
  margin-top: 3px;
  font-size: 11px;
  color: var(--fm-muted);
}

.pair {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}

.pair-fill {
  display: flex;
  flex: 1;
  min-height: 0;
  gap: 14px;
}

.pane-fill {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.pane-fill .pane-label {
  flex-shrink: 0;
}

.pane-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 7px;
  font-size: 12px;
  font-weight: 500;
  color: var(--fm-text-2);
}

.copy-link {
  font-size: 11px;
  color: var(--fm-primary);
}

.pane-meta {
  font-size: 11px;
  font-weight: 400;
  color: var(--fm-muted);
}

.view-switch {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: 5px;
  background: var(--fm-surface-alt);
}

.view-switch button {
  padding: 1px 10px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--fm-muted);
  font-size: 11px;
  cursor: pointer;
}

.view-switch button.active {
  background: var(--fm-surface);
  color: var(--fm-text-2);
  box-shadow: var(--fm-shadow-sm);
}

.tree-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}

.tree-tools {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 6px 10px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-size: 11px;
}

.tree-tools a {
  color: var(--fm-primary);
}

.tree-tip {
  margin-left: auto;
  color: var(--fm-muted);
  font-size: 10px;
}

.tree-scroll {
  flex: 1;
  min-height: 0;
  padding: 8px 10px;
  overflow: auto;
}

.tree-empty {
  padding: 16px 6px;
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-muted);
  word-break: break-all;
}

.code {
  font-size: 12px;
  line-height: 1.7;
  resize: vertical;
}

.panel-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
  flex-shrink: 0;
}

.status {
  font-size: 11px;
}

.status.ok {
  color: var(--fm-success);
}

.status.err {
  color: var(--fm-danger);
}

.field {
  margin-top: 10px;
}

.field:first-of-type {
  margin-top: 0;
}

.field.inline {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
}

.grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}

.note {
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-primary-soft);
  border: 1px dashed var(--fm-border-strong);
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-text-2);
}

.now-card {
  padding: 18px 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

.now-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.now-value {
  margin-top: 6px;
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 0.4px;
  color: var(--fm-text);
}

.now-actions {
  display: flex;
  gap: 8px;
}

.range-sep {
  font-size: 11px;
  color: var(--fm-muted);
}

.inline-controls {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.warn-box {
  padding: 9px 12px;
  border-radius: var(--fm-radius);
  background: var(--fm-warning-soft);
  border: 1px dashed var(--fm-border-strong);
  font-size: 11px;
  line-height: 1.8;
  color: var(--fm-warning);
}

.param-list {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
}

.param-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 12px;
  font-size: 11px;
  border-top: 1px solid var(--fm-border);
  background: var(--fm-surface);
}

.param-item:first-child {
  border-top: none;
}

.param-item:nth-child(even) {
  background: var(--fm-surface-alt);
}

.param-index {
  min-width: 28px;
  color: var(--fm-muted);
}

.param-name {
  font-weight: 500;
  color: var(--fm-text);
}

.param-value {
  color: var(--fm-text-2);
  word-break: break-all;
}

.param-type {
  margin-left: auto;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--fm-primary-soft);
  font-size: 10px;
  color: var(--fm-primary);
  white-space: nowrap;
}

.result-block {
  margin: 0;
  max-height: 260px;
  padding: 12px 14px;
  overflow: auto;
  border-radius: var(--fm-radius);
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  font-size: 11.5px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--fm-text-2);
}

.meta-list {
  font-size: 11.5px;
  line-height: 1.9;
  color: var(--fm-text-2);
}

.regex-input {
  display: flex;
  align-items: center;
  gap: 8px;
}

.regex-slash {
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 13px;
  color: var(--fm-muted);
}

.regex-preview {
  max-height: 200px;
  padding: 12px 14px;
  overflow: auto;
  border-radius: var(--fm-radius);
  background: var(--fm-code-bg);
  border: 1px solid var(--fm-border);
  font-size: 12px;
  line-height: 1.8;
  white-space: pre-wrap;
  word-break: break-all;
}

.regex-preview :deep(mark) {
  padding: 1px 2px;
  border-radius: 3px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
}

.naming-list {
  margin-top: 14px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  overflow: hidden;
}

.naming-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 12px;
  border-top: 1px solid var(--fm-border);
}

.naming-row:first-child {
  border-top: none;
}

.naming-label {
  min-width: 118px;
  font-size: 11px;
  color: var(--fm-muted);
}

.naming-value {
  flex: 1;
  font-size: 12.5px;
  color: var(--fm-text);
  word-break: break-all;
}

.empty-tip {
  padding: 14px 12px;
  font-size: 11px;
  color: var(--fm-muted);
}

@media (max-width: 980px) {
  .pair,
  .grid-2 {
    grid-template-columns: 1fr;
  }

  .pair-fill {
    flex-direction: column;
  }

  .pair-fill .pane-fill {
    flex: 1 1 0;
    min-height: 220px;
  }

  .panel-fill {
    height: auto;
  }

  .now-card {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
