export interface Project {
  id: number
  name: string
  description?: string | null
  color?: string | null
  sortOrder: number
  createdAt: number
  updatedAt: number
}

export interface CollectionNode {
  id: number
  projectId: number
  parentId?: number | null
  name: string
  sortOrder: number
  children: CollectionNode[]
}

export interface RequestSummary {
  id: number
  collectionId: number
  name: string
  method: string
  url: string
  sortOrder: number
  updatedAt: number
}

export interface RequestSearchHit {
  id: number
  collectionId: number
  collectionName: string
  name: string
  method: string
  url: string
  updatedAt: number
}

export interface KeyValueItem {
  key: string
  value: string
  enabled: boolean
}

export interface CurlParsed {
  name: string
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  warnings: string[]
}

export interface HttpRequestDetail {
  id?: number
  collectionId: number
  name: string
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  preScript: string
  postScript: string
  assertions: string
}

export interface SendRequestPayload {
  requestId?: number | null
  name?: string | null
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body?: string | null
  envId?: number | null
  insecure?: boolean
  timeoutMs?: number
}

export interface HttpResult {
  status: number
  statusText: string
  durationMs: number
  sizeBytes: number
  headers: Record<string, string>
  body: string
  truncated: boolean
  error?: string | null
}

export interface Environment {
  id: number
  projectId: number
  name: string
  isActive: boolean
  variables: EnvVariable[]
}

export interface EnvVariable {
  id?: number
  key: string
  value: string
  isSecret: boolean
  sortOrder: number
}

export interface HistoryItem {
  id: number
  requestId?: number | null
  requestName?: string | null
  method: string
  url: string
  statusCode?: number | null
  durationMs?: number | null
  createdAt: number
}

export interface AppInfo {
  version: string
  dataDir: string
  dbPath: string
}

export interface WorkspaceStats {
  projectCount: number
  collectionCount: number
  requestCount: number
  environmentCount: number
  historyCount: number
  reportCount: number
  reportRequestTotal: number
  reportSuccessTotal: number
  recentHistory: number
  recentReports: number
}

export interface RequestRun {
  id: number
  requestId: number
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body: string
  responseStatus: number
  responseStatusText: string
  responseHeaders: Record<string, string>
  responseBody: string
  durationMs: number
  sizeBytes: number
  error?: string | null
  createdAt: number
}

export interface StressStage {
  concurrent: number
  durationSec: number
}

export interface SlaTargets {
  p95Ms?: number | null
  p99Ms?: number | null
  errorRate?: number | null
  minRps?: number | null
}

export interface StressConfig {
  requestId?: number | null
  name?: string | null
  method: string
  url: string
  headers: KeyValueItem[]
  params: KeyValueItem[]
  bodyType: string
  body?: string | null
  envId?: number | null
  insecure?: boolean
  timeoutMs?: number
  connectTimeoutMs?: number
  followRedirects?: boolean
  stopErrorRate?: number | null
  stopMinRequests?: number | null
  concurrent: number
  durationSec: number
  rampUpSec: number
  thinkTimeMs: number
  stages?: StressStage[]
  targetRps?: number | null
  sla?: SlaTargets | null
}

export interface ReportSample {
  elapsedSec: number
  concurrent: number
  qps: number
  avgRt: number
  p50Rt: number
  p95Rt: number
  p99Rt: number
  ttfbAvg: number
  bytes: number
  successCount: number
  errorCount: number
}

export interface ReportError {
  elapsedSec: number
  errorType: string
  errorMessage: string
  requestUrl: string
  responseStatus?: number | null
  responseBody: string
}

export interface ReportRequestRecord {
  elapsedMs: number
  timeMs: number
  method: string
  url: string
  status: number
  latencyMs: number
  ttfbMs: number
  bytes: number
  success: boolean
  body?: string | null
  params?: string | null
  headers?: string | null
  responseHeaders?: string | null
  responseBody?: string | null
  errorType?: string | null
  errorMessage?: string | null
}

export interface ReportRequestsPage {
  total: number
  items: ReportRequestRecord[]
}

export interface StressReport {
  id: number
  planName: string
  requestId?: number | null
  totalRequests: number
  successCount: number
  errorCount: number
  avgRt: number
  minRt: number
  maxRt: number
  p50Rt: number
  p75Rt: number
  p90Rt: number
  p95Rt: number
  p99Rt: number
  p999Rt: number
  stddevRt: number
  ttfbAvg: number
  ttfbP95: number
  apdex: number
  bytesTotal: number
  maxQps: number
  avgQps: number
  targetRps: number
  errorRate: number
  concurrency: number
  status: string
  startTime?: number | null
  endTime?: number | null
  createdAt: number
}

export interface StatusCount {
  code: number
  count: number
}

export interface ErrorTypeCount {
  errorType: string
  count: number
}

export interface HistogramBucket {
  label: string
  minMs: number
  maxMs: number
  count: number
}

export interface StageSummary {
  index: number
  concurrent: number
  durationSec: number
  requests: number
  errors: number
  avgQps: number
  avgRt: number
  p95Rt: number
}

export interface ReportConfigSnapshot {
  mode?: string
  method?: string
  url?: string
  headers?: number
  params?: number
  bodyType?: string
  envId?: number | null
  timeoutMs?: number
  thinkTimeMs?: number
  rampUpSec?: number
  targetRps?: number
  stages?: StressStage[]
  sla?: SlaTargets | null
}

export interface ReportDetail {
  report: StressReport
  samples: ReportSample[]
  errors: ReportError[]
  statusCodes: StatusCount[]
  errorTypes: ErrorTypeCount[]
  histogram: HistogramBucket[]
  stages: StageSummary[]
  config?: ReportConfigSnapshot | null
}

export interface StressProgress {
  name: string
  running: boolean
  elapsedSec: number
  total: number
  success: number
  errors: number
  rps: number
  avgRt: number
  p95Rt: number
  p99Rt: number
  concurrent: number
  stageIndex: number
  activeConcurrent: number
  statusCodes: StatusCount[]
  samples: ReportSample[]
}

export interface StressDone {
  reportId?: number | null
  name: string
  status: string
  total: number
  success: number
  errors: number
  elapsedSec: number
  message?: string | null
}

/* ---------- 系统监视（winstats 风格） ---------- */

export interface MonitorSystemInfo {
  name: string
  osVersion: string
  kernelVersion: string
  hostName: string
  uptimeSec: number
  physicalCores: number
  logicalCores: number
}

export interface MonitorCpu {
  total: number
  brand: string
  vendor: string
  frequencyMhz: number
  cores: number[]
}

export interface MonitorMemory {
  total: number
  used: number
  available: number
  percent: number
}

export interface MonitorSwap {
  total: number
  used: number
  percent: number
}

export interface MonitorDisk {
  name: string
  mount: string
  fileSystem: string
  total: number
  available: number
  percent: number
  readPerSec: number
  writePerSec: number
  removable: boolean
}

export interface MonitorNetwork {
  name: string
  receivedPerSec: number
  transmittedPerSec: number
  totalReceived: number
  totalTransmitted: number
  mac: string
  ipv4: string[]
  ipv6: string[]
  gateway: string[]
}

export interface MonitorLocalIp {
  adapter: string
  address: string
  family: string
  gateway: string
  primary: boolean
}

export interface StorageInfo {
  dataDir: string
  dbPath: string
  defaultDir: string
  isCustom: boolean
  dbSize: number
  walSize: number
  pendingDir?: string | null
}

export interface MonitorBattery {
  present: boolean
  onAc: boolean
  percent: number
  minutesLeft: number
}

export interface MonitorGpu {
  name: string
  usage: number
}

export interface MonitorSnapshot {
  timestamp: number
  system: MonitorSystemInfo
  cpu: MonitorCpu
  memory: MonitorMemory
  swap?: MonitorSwap | null
  disks: MonitorDisk[]
  networks: MonitorNetwork[]
  ips: MonitorLocalIp[]
  battery?: MonitorBattery | null
  gpus: MonitorGpu[]
}

export interface MonitorProcess {
  pid: number
  name: string
  cpu: number
  memory: number
}

/* ---------- 工具箱：IP 查询 ---------- */

export interface IpLookupInfo {
  ip: string
  country?: string
  country_code?: string
  prov?: string
  city?: string
  city_code?: string
  city_short_code?: string
  area?: string
  post_code?: string
  area_code?: string
  isp?: string
  lng?: string
  lat?: string
  long_ip?: number
  big_area?: string
}
