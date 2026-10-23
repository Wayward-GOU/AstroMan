/* 网络与进制相关工具：IPv4 / 子网计算 / 端口占用提示 */

import { ipToLong, longToIp } from './transforms'

export interface SubnetInfo {
  cidr: string
  network: string
  broadcast: string
  netmask: string
  wildcard: string
  firstHost: string
  lastHost: string
  hostCount: number
  ipClass: string
  scope: string
  prefix: number
  ip: string
}

function maskToPrefix(mask: number): number {
  let prefix = 0
  let value = mask
  while (value) {
    prefix += value & 1
    value >>>= 1
  }
  return prefix
}

export function subnetInfo(input: string): SubnetInfo {
  const raw = input.trim()
  if (!raw) throw new Error('请输入 CIDR，例如 192.168.1.10/24')
  let ip = raw
  let prefix = 24
  if (raw.includes('/')) {
    const [address, prefixText] = raw.split('/')
    ip = address
    if (!/^\d+$/.test(prefixText)) throw new Error('掩码位数需要是 0-32 的数字')
    prefix = Number(prefixText)
  } else if (/^\d+\.\d+\.\d+\.\d+$/.test(raw)) {
    prefix = 24
  }
  if (prefix < 0 || prefix > 32) throw new Error('掩码位数需要在 0 - 32 之间')

  const ipLong = ipToLong(ip)
  const mask = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0
  const network = (ipLong & mask) >>> 0
  const broadcast = (network | (~mask >>> 0)) >>> 0
  const hostCount = prefix >= 31 ? Math.max(1, 2 ** (32 - prefix) - 2 + (prefix === 32 ? 1 : 0)) : 2 ** (32 - prefix) - 2
  const firstHost = prefix >= 31 ? network : (network + 1) >>> 0
  const lastHost = prefix >= 31 ? broadcast : (broadcast - 1) >>> 0

  const firstOctet = network >>> 24
  const ipClass =
    firstOctet < 128 ? 'A 类' : firstOctet < 192 ? 'B 类' : firstOctet < 224 ? 'C 类' : firstOctet < 240 ? 'D 类（组播）' : 'E 类（保留）'

  let scope = '公网地址'
  if (firstOctet === 10) scope = '私有地址（10.0.0.0/8）'
  else if (firstOctet === 172 && (network >>> 16 & 0xff) >= 16 && (network >>> 16 & 0xff) <= 31) scope = '私有地址（172.16.0.0/12）'
  else if (firstOctet === 192 && (network >>> 16 & 0xff) === 168) scope = '私有地址（192.168.0.0/16）'
  else if (network === 0x7f000000) scope = '回环地址'
  else if (network === 0xa9fe0000) scope = '链路本地地址（169.254.0.0/16）'
  else if ((network >>> 24) === 100 && ((network >>> 16) & 0xff) >= 64 && ((network >>> 16) & 0xff) <= 127) scope = '运营商级 NAT'

  return {
    cidr: `${longToIp(network)}/${prefix}`,
    network: longToIp(network),
    broadcast: longToIp(broadcast),
    netmask: longToIp(mask),
    wildcard: longToIp(~mask >>> 0),
    firstHost: longToIp(firstHost),
    lastHost: longToIp(lastHost),
    hostCount,
    ipClass,
    scope,
    prefix: maskToPrefix(mask),
    ip: longToIp(ipLong),
  }
}

export function ipRange(input: string): string {
  const info = subnetInfo(input)
  const start = ipToLong(info.firstHost)
  const end = ipToLong(info.lastHost)
  if (end - start > 4096) {
    return `可用地址共 ${end - start + 1} 个，数量过多，仅展示首尾：\n${info.firstHost}\n...\n${info.lastHost}`
  }
  const list: string[] = []
  for (let value = start; value <= end; value += 1) list.push(longToIp(value))
  return list.join('\n')
}

export function ipToBin(ip: string): { dot: string; grouped: string } {
  const long = ipToLong(ip)
  const dot = [24, 16, 8, 0].map((shift) => (long >>> shift & 0xff).toString(2).padStart(8, '0')).join('.')
  return { dot, grouped: dot.replace(/\./g, ' ') }
}

export const COMMON_PORTS: Array<{ port: number; service: string; note: string }> = [
  { port: 20, service: 'FTP-DATA', note: 'FTP 数据传输' },
  { port: 21, service: 'FTP', note: 'FTP 控制连接' },
  { port: 22, service: 'SSH', note: '安全远程登录' },
  { port: 23, service: 'Telnet', note: '明文远程登录' },
  { port: 25, service: 'SMTP', note: '邮件发送' },
  { port: 53, service: 'DNS', note: '域名解析' },
  { port: 80, service: 'HTTP', note: '明文 Web' },
  { port: 110, service: 'POP3', note: '邮件接收' },
  { port: 143, service: 'IMAP', note: '邮件接收（IMAP）' },
  { port: 443, service: 'HTTPS', note: 'TLS Web' },
  { port: 445, service: 'SMB', note: 'Windows 文件共享' },
  { port: 465, service: 'SMTPS', note: '加密邮件发送' },
  { port: 587, service: 'SMTP', note: '邮件提交' },
  { port: 6379, service: 'Redis', note: 'Redis 服务' },
  { port: 3306, service: 'MySQL', note: 'MySQL 数据库' },
  { port: 5432, service: 'PostgreSQL', note: 'PostgreSQL 数据库' },
  { port: 9200, service: 'Elasticsearch', note: 'ES HTTP 接口' },
  { port: 27017, service: 'MongoDB', note: 'MongoDB 数据库' },
  { port: 8080, service: 'HTTP-Alt', note: '常见应用端口' },
  { port: 8443, service: 'HTTPS-Alt', note: '常见应用 HTTPS 端口' },
]
