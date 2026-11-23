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

/** pairing / pending / pairReady / preparing / preflight / ready / running / done / lost / left / rejected */
export type ClusterNodeStatus =
  | 'pairing'
  | 'pending'
  | 'pairReady'
  | 'preparing'
  | 'preflight'
  | 'ready'
  | 'running'
  | 'done'
  | 'lost'
  | 'left'
  | 'rejected'

/** 节点能力：核数 / 内存 / 本机预算 */
export interface NodeCapacity {
  cores: number
  memGb: number
  machineLimit: number
}

/** 采集健康度：ok / waiting / gap / lost / done */
export interface SampleHealth {
  state: string
  firstSampleMs?: number | null
  lastSampleMs?: number | null
  expected: number
  received: number
  lost: number
  gapSec: number
  message?: string | null
}

/** 引擎累计计数（从节点心跳回传） */
export interface EngineStats {
  statsAtMs: number
  total: number
  success: number
  errors: number
  qps: number
  concurrent: number
}

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
  /** 整机 CPU 使用率（%） */
  hostCpuPercent?: number | null
  /** 整机内存使用率（%） */
  hostMemPercent?: number | null
  /** 网卡接收 / 发送速率（KB/s） */
  netRxKbps?: number | null
  netTxKbps?: number | null
  /** 进程线程数 */
  threads?: number | null
  /** 1 分钟平均负载 */
  load1?: number | null
  /** 从节点相对主节点的时钟偏移（毫秒，正数表示从节点更快） */
  clockOffsetMs?: number | null
  clockUncertaintyMs?: number | null
  /** 运行期复测的漂移（毫秒）：相对配对时刻偏移的变化量 */
  clockDriftMs?: number | null
  /** 最近一次时钟复测时刻 */
  clockRecheckAt?: number | null
  droppedSamples: number
  joinedAt: number
  lastSeenAt?: number | null
  error?: string | null
  capacity?: NodeCapacity | null
  /** 本次分配并发 */
  quota: number
  health: SampleHealth
  peakQps: number
  peakConcurrent: number
  vuStart: number
  vuCount: number
  /** 最近一次心跳回传的引擎累计计数 */
  engineStats?: EngineStats | null
  /** 最近一次 PrepareAck 报告的缺失变量（含 os:NAME） */
  missingVars?: string[]
  /** 从节点是否支持池自定义列 / 自动注入（v20） */
  poolColumns: boolean
  /** 是否提供加密公钥：旧节点为 false 时只能读取本机同名环境 */
  envBundle: boolean
  /** 本次下发给该节点的池切片字节数（prepare 后才有） */
  poolSliceBytes?: number | null
}

/** 数据池体量预估：创建任务时按池条目算一次 */
export interface PoolSizeEstimate {
  pools: number
  items: number
  bytes: number
}

/** 变量体检：单个变量在某个从节点上的状态 */
export interface VarAuditNodeState {
  /** ok / missing / pending / unknown */
  state: string
  detail?: string | null
}

/** 变量体检：表格一行 */
export interface VarAuditItem {
  name: string
  /** user / secret / os / pool */
  kind: string
  /** 主节点环境里是否有值 */
  masterHasValue: boolean
  /** env / os / pool / none */
  source: string
  message?: string | null
  /** nodeId → 状态 */
  nodeStates: Record<string, VarAuditNodeState>
}

export interface VarAuditNode {
  id: string
  name: string
  addr: string
  status: string
}

/** 变量体检：只读报告，不含变量值 */
export interface VarAuditReport {
  jobId: number
  envName?: string | null
  varPolicy: string
  envVarCount: number
  secretCount: number
  nodes: VarAuditNode[]
  items: VarAuditItem[]
}

/** 待接受节点（审批模式） */
export interface PendingNodeInfo {
  requestId: string
  nodeId: string
  name: string
  addr: string
  deviceId?: string | null
  capacity?: NodeCapacity | null
  requestedAt: number
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
  /** 分配模式：auto / even / custom */
  allocationMode: string
  /** 本机并发预算（主节点） */
  budget: number
  startAtMs?: number | null
  elapsedSec: number
  reportId?: number | null
  incomplete: boolean
  message?: string | null
  nodes: ClusterNodeInfo[]
  /** 待接受节点（审批模式） */
  pending: PendingNodeInfo[]
  autoAccept: boolean
  discovery: boolean
  /** 数据池体量预估；无池绑定时为 null */
  poolEstimate?: PoolSizeEstimate | null
  /** 单节点池切片字节上限 */
  poolSliceLimitBytes: number
  createdAt: number
}

/** 从节点会话快照 */
export interface ClusterSlaveSnapshot {
  /** idle / connecting / pending / preparing / ready / running / done / lost / left / rejected */
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
  engineStats?: EngineStats | null
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
  /** 该节点最少分配并发，缺省 1 */
  minConcurrent?: number | null
  /** 该节点最多分配并发，缺省取本机预算 */
  maxConcurrent?: number | null
}

/** 从节点加入 */
export interface ClusterSlaveConnectPayload {
  /** 主节点地址：`192.168.1.10:17801`，端口可省略 */
  masterAddr: string
  /** 6 位单次配对码：审批模式可留空 */
  pairCode: string
  /** 加入方式：code（配对码）/ request（发现后申请，需审批） */
  joinMode?: 'code' | 'request' | null
  /** 节点名：缺省取本机计算机名 */
  nodeName?: string | null
}

/** 发现到的主节点 */
export interface DiscoveredMaster {
  instanceId: string
  name: string
  host: string
  port: number
  addr: string
  jobId: number
  status: string
  joinMode: string
  nodeCount: number
  maxNodes: number
  capacity?: { cores: number; memMb: number; budget: number } | null
  lastSeenMs: number
  latencyMs: number
}

/** 发现监听状态 */
export interface DiscoveryStatus {
  running: boolean
  port: number
  ttlSec: number
  error?: string | null
}

/** 并发预算来源 */
export interface BudgetSources {
  cores: number
  memGb: number
  fdLimit: number
  probeSaturation?: number | null
  byCpu: number
  byFd: number
  byMem: number
  limitedBy: string
  computedAt: number
}

/** 本机并发预算 */
export interface StressBudget {
  mode: 'auto' | 'manual'
  machineLimit: number
  perJobLimit: number
  activeJobs: number
  maxJobs: number
  sources: BudgetSources
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

/** 单节点某一秒的图表字段：直方图桶留在后端，界面上只画需要的值 */
export interface ClusterSampleNode {
  nodeId: string
  qps: number
  concurrent: number
  avgRt: number
  errorCount: number
  cpu?: number | null
  memMb?: number | null
}

/** 采样帧：每秒一帧，替代逐节点逐条推送 */
export interface ClusterSampleFrame {
  jobId: number
  sec: number
  nodes: ClusterSampleNode[]
}

/** 主节点任务历史 */
export interface ClusterJobSummary {
  id: number
  createdAt: number
  finishedAt?: number | null
  status: string
  planName: string
  masterNode: string
  totalConcurrent: number
  nodeCount: number
  avgQps?: number | null
  reportId?: number | null
  incomplete: boolean
  message?: string | null
}

export interface ClusterJobPage {
  total: number
  items: ClusterJobSummary[]
}

/** 历史任务里的节点记录 */
export interface ClusterWorkerRow {
  nodeName: string
  addr: string
  role: string
  weight: number
  concurrent: number
  status: string
  qps: number
  cpu?: number | null
  memMb?: number | null
  clockOffsetMs: number
  clockUncertaintyMs: number
  droppedSamples: number
  joinedAt: number
  lastSeenAt?: number | null
  error?: string | null
}

export interface ClusterJobDetail {
  job: ClusterJobSummary
  nodes: ClusterWorkerRow[]
}

/** 从节点参与记录 */
export interface ClusterSlaveHistoryItem {
  id: number
  createdAt: number
  masterAddr: string
  masterNode: string
  masterJobId?: number | null
  nodeName: string
  planName: string
  joinMode: string
  concurrent: number
  totalConcurrent: number
  status: string
  qpsAvg: number
  cpuPeak: number
  memPeakMb: number
  /** 整机 CPU 使用率峰值（%） */
  hostCpuPeak: number
  /** 网卡收发合计峰值（KB/s） */
  netPeakKbps: number
  startedAt?: number | null
  endedAt?: number | null
  message?: string | null
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
  peaks?: ReportPeaks | null
  nodeSeries: NodeSeries[]
  note?: string | null
}

/** 单个峰值指标 */
export interface PeakMetric {
  value: number
  atMs: number
  second: number
  heldSec: number
  topNode?: string | null
}

/** 报告峰值专题 */
export interface ReportPeaks {
  qps: PeakMetric
  concurrent: PeakMetric
  bandwidth: PeakMetric
  p95: PeakMetric
  p99: PeakMetric
  errorRate: PeakMetric
}

export interface NodeSeries {
  node: string
  role: string
  points: SeriesPoint[]
}

export interface SeriesPoint {
  sec: number
  qps: number
  concurrent: number
  p95: number
  errors: number
  cpu?: number
  hostCpuPercent?: number
  memMb?: number
  netRxKbps?: number
  netTxKbps?: number
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
  /** 整机 CPU 使用率峰值（%） */
  hostCpuPeak: number
  /** 整机内存使用率峰值（%） */
  hostMemPeakPercent: number
  /** 网卡收发合计峰值（KB/s） */
  netPeakKbps: number
  clockOffsetMs: number
  clockUncertaintyMs: number
  /** 运行期时钟复测得到的漂移（毫秒） */
  clockDriftMs: number
  droppedSamples: number
  startMs: number
  endMs: number
  peakQps: number
  peakConcurrent: number
  /** 贡献占比（0~1） */
  share: number
  health: SampleHealth
  /** 节点总请求数：旧报告 / 旧节点缺失时为 null */
  totalRequests?: number | null
  successCount?: number | null
  errorCount?: number | null
  /** 客户端平均耗时（ms） */
  avgRtMs?: number | null
  /** 总请求数 / 施压时长 */
  tps?: number | null
  /** 出站带宽平均（KB/s） */
  egressAvgKbps?: number | null
}
