/* 文本脱敏：手机号、身份证、邮箱、银行卡、IP 与姓名 */

export type MaskRule = 'auto' | 'phone' | 'idcard' | 'email' | 'bankcard' | 'ip' | 'name'

const RULE_LABELS: Record<Exclude<MaskRule, 'auto'>, string> = {
  phone: '手机号',
  idcard: '身份证',
  email: '邮箱',
  bankcard: '银行卡',
  ip: 'IP',
  name: '姓名',
}

type Counts = Partial<Record<Exclude<MaskRule, 'auto'>, number>>

function countUp(counts: Counts, key: Exclude<MaskRule, 'auto'>) {
  counts[key] = (counts[key] ?? 0) + 1
}

function maskPhone(text: string, counts: Counts): string {
  return text.replace(/(?<!\d)(1[3-9]\d)(\d{4})(\d{4})(?!\d)/g, (_all, head: string, _mid: string, tail: string) => {
    countUp(counts, 'phone')
    return `${head}****${tail}`
  })
}

function maskIdCard(text: string, counts: Counts): string {
  return text.replace(
    /(?<![0-9A-Za-z])([1-9]\d{5})((?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01]))(\d{3}[\dXx])(?![0-9A-Za-z])/g,
    (_all, area: string, _birth: string, tail: string) => {
      countUp(counts, 'idcard')
      return `${area}********${tail.toUpperCase()}`
    },
  )
}

function maskEmail(text: string, counts: Counts): string {
  return text.replace(
    /([A-Za-z0-9._%+-])[A-Za-z0-9._%+-]*@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)/g,
    (_all, head: string, domain: string) => {
      countUp(counts, 'email')
      return `${head}***@${domain}`
    },
  )
}

function maskBankCard(text: string, counts: Counts): string {
  return text.replace(/(?<!\d)(\d{16,19})(?!\d)/g, (all: string, _digits: string) => {
    countUp(counts, 'bankcard')
    return `${all.slice(0, 4)}${'*'.repeat(all.length - 8)}${all.slice(-4)}`
  })
}

function maskIp(text: string, counts: Counts): string {
  return text.replace(/(?<!\d)(\d{1,3}\.\d{1,3})\.\d{1,3}\.\d{1,3}(?!\d)/g, (_all, head: string) => {
    countUp(counts, 'ip')
    return `${head}.*.*`
  })
}

function maskName(text: string, counts: Counts): string {
  return text.replace(
    /((?:姓名|联系人|收件人|开户人|真实姓名)\s*[:：]\s*)([\u4e00-\u9fa5]{2,4})/g,
    (_all, prefix: string, name: string) => {
      countUp(counts, 'name')
      return `${prefix}${name[0]}${'*'.repeat(Math.max(1, name.length - 1))}`
    },
  )
}

export function maskText(input: string, rule: MaskRule = 'auto'): string {
  if (!input) throw new Error('请输入需要脱敏的文本')
  const counts: Counts = {}
  const order: Exclude<MaskRule, 'auto'>[] =
    rule === 'auto' ? ['idcard', 'name', 'phone', 'email', 'bankcard', 'ip'] : [rule]
  const handlers: Record<Exclude<MaskRule, 'auto'>, (text: string, counts: Counts) => string> = {
    phone: maskPhone,
    idcard: maskIdCard,
    email: maskEmail,
    bankcard: maskBankCard,
    ip: maskIp,
    name: maskName,
  }
  let output = input
  order.forEach((key) => {
    output = handlers[key](output, counts)
  })
  const detail = order
    .filter((key) => counts[key])
    .map((key) => `${RULE_LABELS[key]} ${counts[key]} 处`)
  const total = Object.values(counts).reduce((sum, value) => sum + (value ?? 0), 0)
  if (!total) return `未识别到需要脱敏的内容。\n\n原文：\n${input}`
  return [
    `已脱敏 ${total} 处（${detail.join(' · ')}）`,
    '',
    output,
    '',
    '提示：姓名规则只处理「姓名：张三」这类带标签的写法，避免误伤正文。',
  ].join('\n')
}
