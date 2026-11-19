/**
 * 分布式压测（v1.1.1 一期）共享类型
 *
 * 字段与 `src-tauri/src/services/cluster/protocol.rs` 保持一致；
 * 一期为主从模型，主节点同时作为施压节点参与压测。
 */

import type { StressConfig } from '@/types/models'

export interface ClusterProtocolInfo {
  version: number
  defaultPort: number
  pairCodeTtlSec: number
  minPairCodeTtlSec: number
  maxPairCodeTtlSec: number
  pairCodeDigits: number
  heartbeatSec: number
  heartbeatMisses: number
  clockRounds: number
  clockDropUncertaintyMs: number
  maxNodes: number
}

export type ClusterNodeRole = 'master' | 'slave'

/** pairing / pairReady / preparing / preflight / ready / running / done / lost / left / rejected */
export type ClusterNodeStatus =
  | 'pairing'
  | 'pairReady'
  | 'preparing'
  | 'preflight'
  | 'ready'
  | 'running'
  | 'done'
  | 'lost'
  | 'left'
  | 'rejected'

export interface ClusterNodeInfo {
  id: string
  name: string
  role: ClusterNodeRole
  addr: string
  weight: number
  concurrent: number
  status: ClusterNodeStatus
  qps: number
  cpu?: number | null
  memMb?: number | null
  /** 从节点相对主节点的时钟偏移（毫秒，正数表示从节点更快） */
  clockOffsetMs?: number | null
  clockUncertaintyMs?: number | null
  droppedSamples: number
  joinedAt: number
  lastSeenAt?: number | null
  error?: string | null
}

/** 主节点任务快照 */
export interface ClusterMasterSnapshot {
  jobId: number
  name: string
  planId: number
  target: string
  method: string
  /** pairing / preparing / running / finished / failed */
  status: string
  port: number
  pairCode?: string | null
  pairCodeExpiresAt?: number | null
  /** 本任务创建时选定的配对码有效期（秒） */
  pairCodeTtlSec: number
  localAddresses: string[]
  totalConcurrent: number
  startAtMs?: number | null
  elapsedSec: number
  reportId?: number | null
  incomplete: boolean
  message?: string | null
  nodes: ClusterNodeInfo[]
  createdAt: number
}

/** 从节点会话快照 */
export interface ClusterSlaveSnapshot {
  /** idle / connecting / preparing / ready / running / done / lost / left / rejected */
  status: string
  masterAddr: string
  nodeName: string
  nodeId?: string | null
  planName?: string | null
  target?: string | null
  method?: string | null
  concurrent: number
  startAtMs?: number | null
  elapsedSec: number
  qps: number
  error?: string | null
  missingVars: string[]
  joinedAt?: number | null
}

/** 创建主节点任务：Total 并发取自 config.concurrent，节点间按权重拆分 */
export interface ClusterMasterStartPayload {
  /** 计划 ID：仅用于报告归属，可空 */
  planId?: number | null
  planName?: string | null
  config: StressConfig
  /** 环境名：从节点按名称在本地库查找，主节点不下发变量值 */
  envName?: string | null
  port?: number | null
  /** 配对码有效期（秒）：缺省 300，允许 60–3600 */
  pairCodeTtlSec?: number | null
  /** 主节点自身显示名，缺省取本机计算机名 */
  masterName?: string | null
}

/** 单节点权重：缺省为 1，主节点与从节点一视同仁 */
export interface ClusterNodeWeight {
  nodeId: string
  weight: number
}

/** 从节点加入 */
export interface ClusterSlaveConnectPayload {
  /** 主节点地址：`192.168.1.10:17801`，端口可省略 */
  masterAddr: string
  /** 6 位单次配对码 */
  pairCode: string
  /** 节点名：缺省取本机计算机名 */
  nodeName?: string | null
}

/** 每秒采样（W1/W4 合并用，前端一般只读快照） */
export interface ClusterSamplePayload {
  elapsedSec: number
  concurrent: number
  qps: number
  avgRt: number
  ttfbAvg: number
  bodyAvg: number
  bytes: number
  successCount: number
  errorCount: number
  sumRtMs: number
  /** 本秒 TTFB 累加，跨节点加权与失联节点还原用 */
  sumTtfbMs?: number
  latencyBuckets: [number, number][]
}

/** 每秒采样事件：前端画各节点实时曲线 */
export interface ClusterSampleEvent {
  jobId: number
  nodeId: string
  sample: ClusterSamplePayload
}

/** 写入报告 config.cluster 的元数据 */
export interface ClusterReportMeta {
  protocolVersion: number
  masterNode: string
  totalConcurrent: number
  incomplete: boolean
  startedAt: number
  finishedAt: number
  droppedSamples: number
  nodes: ClusterReportNode[]
  note?: string | null
}

export interface ClusterReportNode {
  name: string
  role: ClusterNodeRole
  addr: string
  weight: number
  concurrent: number
  status: string
  qpsAvg: number
  cpuPeak: number
  memPeakMb: number
  clockOffsetMs: number
  clockUncertaintyMs: number
  droppedSamples: number
  startMs: number
  endMs: number
}
