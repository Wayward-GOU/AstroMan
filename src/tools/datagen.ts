/* 测试数据生成与证件校验（身份证 / 统一社会信用代码 / 银行卡 / 手机号） */

const SURNAMES = '王李张刘陈杨黄赵吴周徐孙马朱胡郭何高林罗郑梁谢宋唐许韩冯邓曹彭曾肖田董袁潘于蒋蔡余杜叶程苏魏吕丁任沈姚卢姜崔钟谭陆汪范金石廖贾夏韦付方白邹孟熊秦邱江尹薛闫段雷侯龙史陶黎贺顾毛郝龚邵万钱严覃武戴莫孔向汤'
const GIVEN = '伟芳娜秀英敏静丽强磊军洋勇艳杰娟涛明超兰霞平刚桂英建国志强晓明春梅海燕玉兰秀珍文博思远嘉怡子涵雨欣浩然子轩'
const STREETS = ['中山路', '人民路', '解放路', '建设路', '文化路', '长江路', '科技大道', '创业路', '望江路', '花园街']
const INDUSTRIES = ['科技', '信息技术', '网络', '数据服务', '智能', '软件']
const DOMAINS = ['example.com', 'test.local', 'mail.example.cn', 'demo.org']
const REGIONS: { code: string; province: string; city: string; district: string }[] = [
  { code: '110101', province: '北京市', city: '北京市', district: '东城区' },
  { code: '310101', province: '上海市', city: '上海市', district: '黄浦区' },
  { code: '440305', province: '广东省', city: '深圳市', district: '南山区' },
  { code: '440106', province: '广东省', city: '广州市', district: '天河区' },
  { code: '330106', province: '浙江省', city: '杭州市', district: '西湖区' },
  { code: '320102', province: '江苏省', city: '南京市', district: '玄武区' },
  { code: '510107', province: '四川省', city: '成都市', district: '武侯区' },
  { code: '420102', province: '湖北省', city: '武汉市', district: '江岸区' },
  { code: '350203', province: '福建省', city: '厦门市', district: '思明区' },
  { code: '610113', province: '陕西省', city: '西安市', district: '雁塔区' },
]

const ID_WEIGHTS = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2]
const ID_CHECK = '10X98765432'
const USCC_CHARSET = '0123456789ABCDEFGHJKLMNPQRTUWXY'
const USCC_WEIGHTS = [1, 3, 9, 27, 19, 26, 16, 17, 20, 29, 25, 13, 8, 24, 10, 30, 28]

function randomBytes(count: number): Uint8Array {
  const bytes = new Uint8Array(count)
  crypto.getRandomValues(bytes)
  return bytes
}

function pick<T>(list: readonly T[]): T {
  return list[randomBytes(1)[0] % list.length]
}

function digits(count: number): string {
  return Array.from(randomBytes(count), (byte) => String(byte % 10)).join('')
}

function idCardCheckChar(first17: string): string {
  let sum = 0
  for (let index = 0; index < 17; index += 1) sum += Number(first17[index]) * ID_WEIGHTS[index]
  return ID_CHECK[sum % 11]
}

export function validateIdCard(value: string): { ok: boolean; detail: string } {
  const code = value.trim().toUpperCase()
  if (!/^\d{17}[\dX]$/.test(code)) return { ok: false, detail: '身份证应为 18 位（末位可为 X）' }
  const area = code.slice(0, 6)
  const birthday = code.slice(6, 14)
  const year = Number(birthday.slice(0, 4))
  const month = Number(birthday.slice(4, 6))
  const day = Number(birthday.slice(6, 8))
  const date = new Date(year, month - 1, day)
  const dateOk =
    date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day && year >= 1900 && year <= 2100
  const region = REGIONS.find((item) => item.code === area)
  const seq = Number(code.slice(14, 17))
  const gender = seq % 2 === 1 ? '男' : '女'
  const expect = idCardCheckChar(code.slice(0, 17))
  const checkOk = expect === code[17]
  const lines = [
    `类型：18 位居民身份证`,
    `校验位：${code[17]}（应为 ${expect}）${checkOk ? ' ✅' : ' ❌'}`,
    `出生日期：${birthday.slice(0, 4)}-${birthday.slice(4, 6)}-${birthday.slice(6, 8)}${dateOk ? ' ✅' : ' ❌ 日期不合法'}`,
    `性别：${gender}`,
    `行政区划：${area}${region ? `（${region.province}${region.city}${region.district}）` : '（未收录，仅校验格式）'}`,
  ]
  return { ok: checkOk && dateOk, detail: lines.join('\n') }
}

export function usccCheckChar(first17: string): string {
  let sum = 0
  for (let index = 0; index < 17; index += 1) {
    const value = USCC_CHARSET.indexOf(first17[index])
    if (value < 0) return ''
    sum += value * USCC_WEIGHTS[index]
  }
  const remainder = sum % 31
  return USCC_CHARSET[(31 - remainder) % 31]
}

export function validateUscc(value: string): { ok: boolean; detail: string } {
  const code = value.trim().toUpperCase()
  if (!/^[0-9A-HJ-NPQRTUWXY]{18}$/.test(code)) {
    return { ok: false, detail: '统一社会信用代码应为 18 位，字符集不含 I / O / S / V / Z' }
  }
  const expect = usccCheckChar(code.slice(0, 17))
  const ok = expect === code[17]
  return {
    ok,
    detail: [
      '类型：统一社会信用代码',
      `校验位：${code[17]}（应为 ${expect}）${ok ? ' ✅' : ' ❌'}`,
      `登记管理部门：${code.slice(0, 2)}`,
      `行政区划码：${code.slice(2, 8)}`,
      `主体标识码：${code.slice(8, 17)}`,
    ].join('\n'),
  }
}

function luhnCheckDigit(partial: string): string {
  let sum = 0
  let double = true
  for (let index = partial.length - 1; index >= 0; index -= 1) {
    let value = Number(partial[index])
    if (double) {
      value *= 2
      if (value > 9) value -= 9
    }
    sum += value
    double = !double
  }
  return String((10 - (sum % 10)) % 10)
}

export function validateBankCard(value: string): { ok: boolean; detail: string } {
  const digitsOnly = value.replace(/\s/g, '')
  if (!/^\d{12,19}$/.test(digitsOnly)) return { ok: false, detail: '银行卡号应为 12–19 位数字' }
  let sum = 0
  let double = false
  for (let index = digitsOnly.length - 1; index >= 0; index -= 1) {
    let current = Number(digitsOnly[index])
    if (double) {
      current *= 2
      if (current > 9) current -= 9
    }
    sum += current
    double = !double
  }
  return {
    ok: sum % 10 === 0,
    detail: [`类型：银行卡号`, `Luhn 校验：${sum % 10 === 0 ? '通过 ✅' : '未通过 ❌'}`, `长度：${digitsOnly.length} 位`].join('\n'),
  }
}

export function validatePhone(value: string): { ok: boolean; detail: string } {
  const ok = /^1[3-9]\d{9}$/.test(value.trim())
  return { ok, detail: `类型：手机号\n格式校验：${ok ? '通过 ✅' : '未通过 ❌（应为 1 开头的 11 位数字）'}` }
}

function genName(): string {
  const surname = SURNAMES[randomBytes(1)[0] % SURNAMES.length]
  const length = randomBytes(1)[0] % 2 === 0 ? 1 : 2
  let given = ''
  for (let index = 0; index < length; index += 1) given += GIVEN[randomBytes(1)[0] % GIVEN.length]
  return surname + given
}

function genPhone(): string {
  const prefix = ['3', '5', '6', '7', '8', '9'][randomBytes(1)[0] % 6]
  return `1${prefix}${digits(9)}`
}

function genBirthday(): string {
  const year = 1970 + (randomBytes(2)[0] % 36)
  const month = 1 + (randomBytes(1)[0] % 12)
  const day = 1 + (randomBytes(1)[0] % 28)
  return `${year}${String(month).padStart(2, '0')}${String(day).padStart(2, '0')}`
}

function genIdCard(): string {
  const region = pick(REGIONS)
  const base = `${region.code}${genBirthday()}${digits(3)}`
  return base + idCardCheckChar(base)
}

function genBankCard(): string {
  const base = `6222${digits(11)}`
  return base + luhnCheckDigit(base)
}

function genEmail(name: string): string {
  const letters = 'abcdefghijklmnopqrstuvwxyz'
  const local = Array.from(randomBytes(6), (byte) => letters[byte % 26]).join('')
  return `${local}${randomBytes(1)[0] % 100}@${pick(DOMAINS)}`
}

function genAddress(): string {
  const region = pick(REGIONS)
  return `${region.province}${region.city}${region.district}${pick(STREETS)}${1 + (randomBytes(2)[0] % 300)}号`
}

function genCompany(): string {
  const region = pick(REGIONS)
  const brand = genName().slice(0, 2)
  return `${region.city}${brand}${pick(INDUSTRIES)}有限公司`
}

function genUscc(): string {
  const region = pick(REGIONS)
  let body = `91${region.code}`
  for (let index = 0; index < 9; index += 1) body += USCC_CHARSET[randomBytes(1)[0] % USCC_CHARSET.length]
  return body + usccCheckChar(body)
}

function genIp(): string {
  const first = [203, 202, 111, 120, 47][randomBytes(1)[0] % 5]
  return `${first}.${1 + (randomBytes(1)[0] % 254)}.${randomBytes(1)[0] % 255}.${1 + (randomBytes(1)[0] % 254)}`
}

export type DataKind = 'mixed' | 'name' | 'phone' | 'idcard' | 'bankcard' | 'email' | 'address' | 'company' | 'uscc' | 'ip'

const KIND_LABELS: Record<DataKind, string> = {
  mixed: '完整个人信息',
  name: '姓名',
  phone: '手机号',
  idcard: '身份证',
  bankcard: '银行卡',
  email: '邮箱',
  address: '地址',
  company: '公司名',
  uscc: '统一社会信用代码',
  ip: 'IP 地址',
}

const MIXED_HEADER = ['name', 'phone', 'idcard', 'bankcard', 'email', 'address']

function mixedRecord() {
  const name = genName()
  return {
    name,
    phone: genPhone(),
    idcard: genIdCard(),
    bankcard: genBankCard(),
    email: genEmail(name),
    address: genAddress(),
  }
}

function singleValue(kind: Exclude<DataKind, 'mixed'>): string {
  if (kind === 'name') return genName()
  if (kind === 'phone') return genPhone()
  if (kind === 'idcard') return genIdCard()
  if (kind === 'bankcard') return genBankCard()
  if (kind === 'email') return genEmail(genName())
  if (kind === 'address') return genAddress()
  if (kind === 'company') return genCompany()
  if (kind === 'uscc') return genUscc()
  return genIp()
}

export function generateData(
  kind: DataKind = 'mixed',
  count = 10,
  format: 'text' | 'csv' | 'json' | 'sql' = 'text',
): string {
  const size = Math.min(500, Math.max(1, Math.floor(count)))
  if (kind === 'mixed') {
    const rows = Array.from({ length: size }, () => mixedRecord())
    if (format === 'json') return JSON.stringify(rows, null, 2)
    if (format === 'csv') {
      return [MIXED_HEADER.join(','), ...rows.map((row) => MIXED_HEADER.map((key) => row[key as keyof typeof row]).join(','))].join('\n')
    }
    if (format === 'sql') {
      const values = rows
        .map(
          (row) =>
            `(${MIXED_HEADER.map((key) => `'${String(row[key as keyof typeof row]).replace(/'/g, "''")}'`).join(', ')})`,
        )
        .join(',\n  ')
      return `INSERT INTO t_test_data (${MIXED_HEADER.join(', ')}) VALUES\n  ${values};`
    }
    return [MIXED_HEADER.join('\t'), ...rows.map((row) => MIXED_HEADER.map((key) => row[key as keyof typeof row]).join('\t'))].join('\n')
  }

  const values = Array.from({ length: size }, () => singleValue(kind))
  if (format === 'json') return JSON.stringify(values.map((value) => ({ kind, value })), null, 2)
  if (format === 'csv') return ['kind,value', ...values.map((value) => `${kind},${value}`)].join('\n')
  if (format === 'sql') {
    return `INSERT INTO t_test_data (kind, value) VALUES\n  ${values
      .map((value) => `('${kind}', '${value.replace(/'/g, "''")}')`)
      .join(',\n  ')};`
  }
  return values.join('\n')
}

export function checkDocument(input: string): string {
  const tokens = input
    .split(/[\s,，;；]+/)
    .map((item) => item.trim())
    .filter(Boolean)
  if (!tokens.length) throw new Error('请输入需要校验的证件号、银行卡号或手机号')
  const blocks = tokens.map((token, index) => {
    let result: { ok: boolean; detail: string }
    if (/^\d{17}[\dXx]$/.test(token)) result = validateIdCard(token)
    else if (/^[0-9A-HJ-NPQRTUWXY]{18}$/i.test(token) && /[A-Za-z]/.test(token)) result = validateUscc(token)
    else if (/^\d{12,19}$/.test(token)) result = validateBankCard(token)
    else if (/^1\d{10}$/.test(token)) result = validatePhone(token)
    else result = { ok: false, detail: '类型：未识别（支持 18 位身份证、18 位统一社会信用代码、12–19 位银行卡、11 位手机号）' }
    return `${tokens.length > 1 ? `#${index + 1} ` : ''}${token}\n结论：${result.ok ? '有效 ✅' : '无效或未通过 ❌'}\n${result.detail}`
  })
  return blocks.join('\n\n')
}

export const DATA_KINDS = Object.entries(KIND_LABELS).map(([value, label]) => ({ value, label }))
