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
  /** 主节点可接受的最低从节点版本 */
  minVersion: number
  /** 失联节点保留状态并允许重连的时长（秒） */
  reconnectGraceSec: number
  /** 从节点重连退避上限（秒） */
  reconnectBackoffMaxSec: number
  /** mDNS 服务类型 */
  mdnsService: string
}

export type ClusterNodeRole = 'master' | 'slave'

/**
 * pairing / pairReady / preparing / preflight / ready / running / done /
 * disconnected（运行期断线，5 分钟内可重连）/ lost / left / rejected
 */
export type ClusterNodeStatus =
  | 'pairing'
  | 'pairReady'
  | 'preparing'
  | 'preflight'
  | 'ready'
  | 'running'
  | 'done'
  | 'disconnected'
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
  /** 协商后的协议版本（1 或 2） */
  protocolVersion: number
  deviceId?: string | null
  /** 从节点上报的饱和并发与 QPS 能力 */
  capacityConcurrent?: number | null
  capacityQps?: number | null
  cores?: number | null
  os?: string | null
  /** 按能力归一化得到的建议权重；未上报能力时为 null */
  suggestedWeight?: number | null
  disconnectedAt?: number | null
  /** 断线累计秒数（含已恢复的历史缺口） */
  gapSeconds: number
}

export type OrchestrationKind = 'ramp' | 'hold' | 'step'

/** 计划级编排阶段：ramp 用 from/to，hold 与 step 用 concurrent */
export interface OrchestrationPhase {
  kind: OrchestrationKind
  durationSec: number
  fromConcurrent?: number | null
  toConcurrent?: number | null
  concurrent?: number | null
}

/** 集群总并发按权重拆分到各节点，目前仅支持 weighted */
export interface Orchestration {
  version: number
  phases: OrchestrationPhase[]
  assignment: string
}

/** 设备列表项：cluster_devices 缓存 + 当前任务在线状态 */
export interface ClusterDevice {
  nodeId: string
  nodeName: string
  lastAddr: string
  lastSeenAt: number
  os?: string | null
  cores: number
  capacityConcurrent: number
  capacityQps: number
  probedAt?: number | null
  online: boolean
}

/** mDNS 浏览到的主节点 */
export interface DiscoveredMaster {
  fullname: string
  name: string
  host: string
  port: number
  addresses: string[]
  version: number
  nodeName: string
  role: string
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
  /** 计划级编排（若有） */
  orchestration?: Orchestration | null
  /** 当前阶段序号（编排任务） */
  currentPhase?: number | null
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
  deviceId?: string | null
  protocolVersion: number
  /** 断线重连中：引擎继续跑，采样等恢复后继续上报 */
  reconnecting: boolean
  phaseIndex?: number | null
  phaseConcurrent: number
  reconnectAttempts: number
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
  /** 监听网卡：缺省 0.0.0.0，跨网段时可绑定指定内网地址 */
  listenAddr?: string | null
  /** 计划级编排：ramp / hold / step，按权重拆分每阶段并发 */
  orchestration?: Orchestration | null
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
  orchestration?: Orchestration | null
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
