import { invoke } from '@tauri-apps/api/core'
import type { ClientProbe, RootInfo, ScanResult } from '@/types/tokenUsage'

/** 增量扫描本机 AI 客户端日志；force 为 true 时丢弃缓存全量重解析 */
export const tokenUsageScan = (force = false) =>
  invoke<ScanResult>('token_usage_scan', { force })

/** 数据根目录是否存在（用于「为什么是空的」提示） */
export const tokenUsageRoots = () => invoke<RootInfo[]>('token_usage_roots')

/** 数据源体检：逐个判断客户端能不能被读到 */
export const tokenUsageClients = () => invoke<ClientProbe[]>('token_usage_clients')

/** 在访达 / 资源管理器中显示某个日志目录 */
export const tokenUsageReveal = (path: string) =>
  invoke<void>('token_usage_reveal', { path })
