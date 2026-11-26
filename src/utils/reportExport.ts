import type {
  ReportDetail,
  ReportError,
  ReportRequestRecord,
  ReportSample,
  PoolReportStat,
  StatusCount,
} from '@/types/models'
import type { ClusterReportMeta, ClusterReportNode, ReportPeaks } from '@/types/cluster'
import type { CellValue, XlsxSheet } from './xlsx'
import { detectKnee, downsample, qpsOverview, reportPeaks, scoreReport, selfCheck, type QpsOverview } from './reportAnalytics'
import { buildAdvice, buildFindings, cdfPoints, gradeReport, type FindingLevel } from './reportNarrative'

/* ---------- 通用格式化 ---------- */

const STATUS_LABELS: Record<string, string> = {
  completed: '已完成',
  stopped: '已手动停止',
  running: '运行中',
}

const RECORD_MODE_LABELS: Record<string, string> = {
  full: '完整明细',
  sample: '明细采 1%',
  off: '高吞吐（无明细）',
  auto: '自动',
}

const MODE_LABELS: Record<string, string> = {
  fixed: '固定并发',
  stages: '阶梯加压',
  cluster: '分布式压测',
}

/**
 * 压测模式文案：集群报告的 mode 会被写成 cluster，
 * 这里结合阶梯阶段数还原成「分布式压测（阶梯/固定）」，页面与导出共用同一口径。
 */
function modeLabelText(mode: string | null | undefined, stageCount = 0): string {
  if (mode === 'cluster') return `分布式压测（${stageCount > 0 ? '阶梯' : '固定'}）`
  if (mode) return MODE_LABELS[mode] ?? mode
  return stageCount > 0 ? '阶梯加压' : '固定并发'
}

/** 页面、HTML 与 Excel 共用的免责声明，保证三处口径一致 */
export const REPORT_DISCLAIMER =
  '该报告仅真实体现当时环境下的测试情况，由此造成的任何误解或相关风险，本软件不承担任何法律责任。'

/** 报告展示标题：用户标题为空时回退到计划名称 */
export function reportTitle(report: { title?: string | null; planName: string }): string {
  const title = report.title?.trim()
  return title || report.planName
}

/**
 * 施压机明细：优先用后端返回的 loadGenerators；旧接口缺省时在前端按报告
 * 聚合值合成单机 1 行（集群报告仍使用 config.cluster.nodes），字段缺失不推断。
 */
export function loadGeneratorsForReport(detail: ReportDetail): ClusterReportNode[] {
  if (detail.loadGenerators?.length) return detail.loadGenerators
  const clusterNodes = detail.config?.cluster?.nodes ?? []
  if (clusterNodes.length) return clusterNodes

  const report = detail.report
  const config = detail.config
  const start = report.startTime ?? 0
  const end = report.endTime ?? 0
  const duration = Math.max(0, end - start) / 1000
  const total = report.totalRequests
  const bytesSent = config?.metrics?.bytesSent
  return [
    {
      name: '本机',
      role: 'master',
      addr: config?.agent?.host || '—',
      weight: 1,
      concurrent: report.concurrency,
      status: report.status,
      qpsAvg: report.avgQps,
      cpuPeak: config?.metrics?.cpuPeakCores ?? 0,
      memPeakMb: config?.metrics?.memPeakMb ?? 0,
      hostCpuPeak: 0,
      hostMemPeakPercent: 0,
      netPeakKbps: 0,
      clockOffsetMs: 0,
      clockUncertaintyMs: 0,
      clockDriftMs: 0,
      droppedSamples: 0,
      startMs: start,
      endMs: end,
      peakQps: Math.max(report.peakQps ?? 0, report.maxQps ?? 0),
      peakConcurrent: report.concurrency,
      share: 1,
      health: { state: 'done', expected: 0, received: 0, lost: 0, gapSec: 0 },
      totalRequests: total,
      successCount: report.successCount,
      errorCount: report.errorCount,
      avgRtMs: total > 0 ? report.avgRt : null,
      tps: duration > 0 ? total / duration : null,
      egressAvgKbps: duration > 0 && bytesSent != null ? bytesSent / duration / 1024 : null,
    },
  ]
}

function fmtInt(value: number | null | undefined): string {
  return Math.round(Number(value ?? 0)).toLocaleString('zh-CN')
}

function fmtNum(value: number | null | undefined, digits = 1): string {
  return Number(value ?? 0).toFixed(digits)
}

function fmtBytes(value: number | null | undefined): string {
  const size = Number(value ?? 0)
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  if (size < 1024 * 1024 * 1024) return `${(size / 1024 / 1024).toFixed(2)} MB`
  return `${(size / 1024 / 1024 / 1024).toFixed(2)} GB`
}

function fmtTime(value: number | null | undefined): string {
  if (!value) return '—'
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function fmtDate(value: number | null | undefined): string {
  if (!value) return '—'
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function fmtDuration(start: number | null | undefined, end: number | null | undefined): string {
  if (!start || !end || end <= start) return '—'
  const seconds = Math.round((end - start) / 1000)
  const hour = Math.floor(seconds / 3600)
  const minute = Math.floor((seconds % 3600) / 60)
  const second = seconds % 60
  if (hour) return `${hour} 小时 ${String(minute).padStart(2, '0')} 分`
  return minute ? `${minute} 分 ${String(second).padStart(2, '0')} 秒` : `${second} 秒`
}

function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status
}

function recordModeLabel(mode?: string | null): string {
  if (!mode) return '未知'
  return RECORD_MODE_LABELS[mode] ?? mode
}

function escapeHtml(text: string): string {
  return String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function clip(text: string | null | undefined, max: number): string {
  const value = text ?? ''
  return value.length > max ? `${value.slice(0, max)}…` : value
}

/** 把 [{key,value}] 形式的 JSON 头信息还原成 Key: Value 多行文本 */
function pairsText(json: string | null | undefined): string {
  if (!json) return ''
  try {
    const parsed = JSON.parse(json)
    if (!Array.isArray(parsed)) return ''
    return parsed
      .map((item) => {
        if (Array.isArray(item)) return `${item[0]}: ${item[1]}`
        if (item && typeof item === 'object') return `${item.key}: ${item.value}`
        return ''
      })
      .filter(Boolean)
      .join('\n')
  } catch {
    return ''
  }
}

function apdexGrade(apdex: number): string {
  if (apdex >= 0.94) return '优秀'
  if (apdex >= 0.85) return '良好'
  if (apdex >= 0.7) return '一般'
  return '较差'
}

function durationSeconds(detail: ReportDetail): number {
  const report = detail.report
  if (!report.startTime || !report.endTime) return 1
  return Math.max(1, (report.endTime - report.startTime) / 1000)
}

function bandwidthKb(bytes: number, seconds: number): string {
  return `${(bytes / Math.max(1, seconds) / 1024).toFixed(1)} KB/s`
}

function errorRateOf(item: { successCount: number; errorCount: number }): number {
  const total = item.successCount + item.errorCount
  return total > 0 ? (item.errorCount / total) * 100 : 0
}

/* ---------- SLA ---------- */

interface SlaRow {
  label: string
  target: string
  actual: string
  pass: boolean
}

function slaRows(detail: ReportDetail): SlaRow[] {
  const report = detail.report
  const sla = detail.config?.sla
  if (!sla) return []
  const rows: SlaRow[] = []
  if (sla.p95Ms) {
    rows.push({
      label: 'P95 延迟',
      target: `≤ ${sla.p95Ms} ms`,
      actual: `${fmtNum(report.p95Rt)} ms`,
      pass: report.p95Rt <= sla.p95Ms,
    })
  }
  if (sla.p99Ms) {
    rows.push({
      label: 'P99 延迟',
      target: `≤ ${sla.p99Ms} ms`,
      actual: `${fmtNum(report.p99Rt)} ms`,
      pass: report.p99Rt <= sla.p99Ms,
    })
  }
  if (sla.errorRate) {
    rows.push({
      label: '错误率',
      target: `≤ ${sla.errorRate}%`,
      actual: `${report.errorRate.toFixed(2)}%`,
      pass: report.errorRate <= sla.errorRate,
    })
  }
  if (sla.minRps) {
    rows.push({
      label: '平均 QPS',
      target: `≥ ${sla.minRps}`,
      actual: fmtNum(report.avgQps),
      pass: report.avgQps >= sla.minRps,
    })
  }
  return rows
}

/* ---------- 文档样式 ---------- */

const STYLE = `
:root {
  --ink: #151a1e;
  --ink-2: #414d55;
  --ink-3: #6f7c85;
  --ink-4: #9aa6ad;
  --line: #dfe4e7;
  --line-soft: #edf1f3;
  --paper: #ffffff;
  --shell: #e8ebed;
  --accent: #14655a;
  --accent-deep: #0e4d45;
  --accent-soft: #ecf3f1;
  --ok: #17774a;
  --ok-soft: #ebf4ee;
  --warn: #9c6414;
  --warn-soft: #fbf2e2;
  --bad: #a23333;
  --bad-soft: #fbecec;
  --info: #37628f;
  --mono: ui-monospace, "SFMono-Regular", "Cascadia Mono", "JetBrains Mono", Consolas, monospace;
}
* { box-sizing: border-box; }
html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body {
  margin: 0;
  padding: 44px 24px 72px;
  background: radial-gradient(circle at 50% 0, #f1f4f5 0, var(--shell) 46%);
  color: var(--ink);
  font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC",
    "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
  font-size: 12.5px;
  line-height: 1.62;
  -webkit-font-smoothing: antialiased;
}
.sheet {
  max-width: 900px;
  margin: 0 auto;
  background: var(--paper);
  padding: 44px 54px 42px;
  border-radius: 12px;
  box-shadow: 0 1px 2px rgba(16, 26, 30, .05), 0 22px 54px -18px rgba(16, 26, 30, .22);
}

/* ---- 封面 ---- */
.cover { padding-bottom: 26px; border-bottom: 1px solid var(--line); }
.cover:before {
  content: ''; display: block; height: 3px; border-radius: 2px; margin-bottom: 26px;
  background: linear-gradient(90deg, var(--accent), #2f8d7e 52%, #93c4ba);
}
.cover-top { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding-bottom: 22px; }
.brand { display: flex; align-items: center; gap: 9px; font-size: 12px; font-weight: 700; letter-spacing: .04em; }
.brand-mark {
  width: 24px; height: 24px; border-radius: 7px;
  background: linear-gradient(145deg, #1a7c6f, var(--accent-deep));
  color: #fff; font-size: 12px; font-weight: 800; display: flex; align-items: center;
  justify-content: center; letter-spacing: 0;
  box-shadow: 0 2px 6px rgba(14, 77, 69, .24);
}
.brand-sub { font-size: 10px; letter-spacing: .18em; text-transform: uppercase; color: var(--ink-4); font-weight: 600; }
.cover-kicker {
  display: inline-block; padding: 3px 10px; border-radius: 999px;
  background: var(--accent-soft); color: var(--accent-deep);
  font-size: 9.5px; letter-spacing: .16em; text-transform: uppercase; font-weight: 700;
}
.cover-title {
  margin: 12px 0 12px; font-size: 29px; line-height: 1.25; font-weight: 750;
  letter-spacing: -.02em; max-width: 26em;
}
.cover-lead { font-size: 13px; color: var(--ink-2); max-width: 40em; margin: 0 0 20px; }
.badge {
  display: inline-flex; align-items: center; gap: 7px; padding: 6px 13px 6px 11px;
  border-radius: 999px; font-size: 12px; font-weight: 700; letter-spacing: .01em;
  border: 1px solid transparent;
}
.badge:before { content: ''; width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.badge.pass { background: var(--ok-soft); color: var(--ok); border-color: #cfe4d8; }
.badge.conditional { background: var(--warn-soft); color: var(--warn); border-color: #ecdcbd; }
.badge.fail { background: var(--bad-soft); color: var(--bad); border-color: #ecd2d2; }
.badge.plain { background: #f3f5f6; color: var(--ink-2); border-color: var(--line); }
.meta {
  display: grid; grid-template-columns: repeat(4, minmax(0, 1fr));
  border: 1px solid var(--line); border-radius: 9px; overflow: hidden;
  background: #fbfcfc; margin-top: 22px;
}
.meta > div { padding: 11px 14px; border-right: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.meta > div:nth-child(4n) { border-right: none; }
.meta > div:last-child { border-right: none; border-bottom: none; }
.meta dt { font-size: 9.5px; letter-spacing: .1em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; margin: 0 0 3px; }
.meta dd { margin: 0; font-size: 12px; font-weight: 600; color: var(--ink); font-variant-numeric: tabular-nums; }
.meta dd.target-url { font-size: 12.5px; font-weight: 500; max-width: 100%; word-break: break-all; white-space: normal; }
.meta dd small { display: block; font-size: 10.5px; font-weight: 400; color: var(--ink-3); margin-top: 2px; }

/* ---- 目录 ---- */
.toc { margin-top: 26px; padding: 16px 20px 10px; border: 1px solid var(--line-soft); border-radius: 10px; background: #fcfdfd; }
.toc-h { font-size: 9.5px; letter-spacing: .14em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; margin-bottom: 8px; }
.toc ol { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: 1fr 1fr; gap: 0 34px; }
.toc li { display: flex; align-items: center; gap: 9px; padding: 5px 0; border-bottom: 1px dotted var(--line); font-size: 12px; }
.toc li:hover { border-bottom-color: var(--accent); }
.toc a { color: var(--ink-2); text-decoration: none; transition: color .15s; }
.toc a:hover { color: var(--accent); }
.toc .n {
  display: inline-flex; align-items: center; justify-content: center;
  width: 24px; height: 18px; flex: none; border-radius: 4px;
  background: var(--accent-soft); color: var(--accent-deep);
  font-family: var(--mono); font-size: 9.5px; font-weight: 700;
}

/* ---- 章节 ---- */
.sec { margin-top: 40px; }
.sec-h { display: flex; align-items: flex-end; gap: 11px; padding-bottom: 8px; border-bottom: 1px solid var(--line); margin-bottom: 18px; }
.sec-n {
  font-family: var(--mono); font-size: 10px; font-weight: 700; color: var(--accent-deep);
  letter-spacing: .06em; padding: 2px 8px; border-radius: 4px; background: var(--accent-soft);
}
.sec-t { font-size: 16.5px; font-weight: 750; letter-spacing: -.015em; }
.sec-s { margin-left: auto; font-size: 10.5px; color: var(--ink-4); text-align: right; padding-bottom: 3px; letter-spacing: .06em; text-transform: uppercase; }
.sub { display: flex; align-items: center; gap: 8px; font-size: 12px; font-weight: 700; color: var(--ink-2); margin: 24px 0 10px; }
.sub:before { content: ''; width: 3px; height: 12px; background: linear-gradient(180deg, var(--accent), #4a9b8d); border-radius: 2px; }
.sub:first-child { margin-top: 4px; }
.sub .hint { font-weight: 400; font-size: 10.5px; color: var(--ink-4); }

/* ---- 表格 ---- */
.tbl-wrap { overflow-x: auto; border: 1px solid var(--line); border-radius: 9px; background: var(--paper); }
.tbl { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.tbl th {
  font-size: 9.5px; letter-spacing: .08em; text-transform: uppercase; color: var(--ink-4);
  font-weight: 700; text-align: left; padding: 8px 10px; background: #f7f9f9;
  border-bottom: 1px solid var(--line); white-space: nowrap; vertical-align: bottom;
}
.tbl td { padding: 8px 10px; border-bottom: 1px solid var(--line-soft); vertical-align: top; }
.tbl tbody tr:last-child td { border-bottom: none; }
.tbl tbody tr:nth-child(even) td { background: #fbfcfc; }
.tbl tbody tr:hover td { background: #f4f7f7; }
.tbl .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
.tbl .mono { font-family: var(--mono); font-size: 10.5px; }
.tbl .break { word-break: break-all; }
.tbl .url-cell { display: inline-flex; align-items: flex-start; gap: 6px; max-width: 100%; }
.tbl .url-cell .mono { word-break: break-all; }
button.copy {
  flex: none; font-size: 9.5px; line-height: 1.4; color: var(--ink-3);
  background: transparent; border: 1px solid var(--line); border-radius: 3px; padding: 1px 5px; cursor: pointer;
}
button.copy:hover { color: var(--accent); border-color: var(--accent); }
.tbl tbody tr.total td { font-weight: 700; background: #f1f5f5; }
.tbl caption { caption-side: top; text-align: left; font-size: 10.5px; color: var(--ink-3); padding: 6px 10px; }
.tbl.kv td:first-child { width: 36%; color: var(--ink-2); }

/* ---- KPI ---- */
.kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
.kpi {
  padding: 14px 15px 13px; border: 1px solid var(--line); border-radius: 9px;
  background: var(--paper); box-shadow: inset 0 2px 0 var(--kpi-tone, transparent);
  break-inside: avoid;
}
.kpi-tone-accent { --kpi-tone: var(--accent); background: linear-gradient(180deg, rgba(20,101,90,.055), transparent 60%); }
.kpi-tone-info { --kpi-tone: var(--info); background: linear-gradient(180deg, rgba(55,98,143,.055), transparent 60%); }
.kpi-tone-ok { --kpi-tone: var(--ok); background: linear-gradient(180deg, rgba(23,119,74,.055), transparent 60%); }
.kpi-tone-warn { --kpi-tone: var(--warn); background: linear-gradient(180deg, rgba(156,100,20,.06), transparent 60%); }
.kpi-tone-bad { --kpi-tone: var(--bad); background: linear-gradient(180deg, rgba(162,51,51,.06), transparent 60%); }
.kpi-tone-violet { --kpi-tone: #6b5b95; background: linear-gradient(180deg, rgba(107,91,149,.06), transparent 60%); }
.kpi-k { font-size: 9.5px; letter-spacing: .1em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; }
.kpi-v { font-size: 22px; font-weight: 750; letter-spacing: -.025em; margin: 4px 0 2px; font-variant-numeric: tabular-nums; }
.kpi-v span { font-size: 11px; font-weight: 600; color: var(--ink-3); margin-left: 3px; letter-spacing: 0; }
.kpi-tone-accent .kpi-v { color: var(--accent); }
.kpi-tone-info .kpi-v { color: var(--info); }
.kpi-tone-ok .kpi-v { color: var(--ok); }
.kpi-tone-warn .kpi-v { color: var(--warn); }
.kpi-tone-bad .kpi-v { color: var(--bad); }
.kpi-tone-violet .kpi-v { color: #6b5b95; }
.kpi-n { font-size: 10.5px; color: var(--ink-3); font-variant-numeric: tabular-nums; }
.strip {
  display: flex; flex-wrap: wrap; gap: 0; margin-top: 12px; border: 1px solid var(--line);
  border-radius: 9px; overflow: hidden; background: #fbfcfc; font-size: 11px;
}
.strip > div { padding: 8px 14px; border-right: 1px solid var(--line); flex: 1 1 auto; }
.strip > div:last-child { border-right: none; }
.strip b { font-weight: 700; color: var(--ink); font-variant-numeric: tabular-nums; }
.strip span { color: var(--ink-4); font-size: 9.5px; letter-spacing: .08em; text-transform: uppercase; display: block; font-weight: 700; }
.strip em { display: block; margin-top: 3px; font-style: normal; font-size: 10px; color: var(--ink-4); letter-spacing: 0; }

/* ---- 发现 / 建议 ---- */
.findings { display: flex; flex-direction: column; gap: 0; }
.finding { display: grid; grid-template-columns: 3px 1fr; gap: 0 14px; padding: 13px 0; border-bottom: 1px solid var(--line-soft); }
.finding:last-child { border-bottom: none; }
.finding > i { display: block; border-radius: 2px; background: var(--line); }
.finding.bad > i { background: var(--bad); }
.finding.warn > i { background: var(--warn); }
.finding.ok > i { background: var(--ok); }
.finding.info > i { background: var(--info); }
.finding-t { font-size: 12.5px; font-weight: 700; margin-bottom: 2px; }
.finding-d { font-size: 11.5px; color: var(--ink-2); }
.lvl { font-family: var(--mono); font-size: 9px; font-weight: 700; letter-spacing: .1em; padding: 1px 5px; border-radius: 2px; vertical-align: 1.5px; margin-right: 6px; }
.lvl.bad { background: var(--bad-soft); color: var(--bad); }
.lvl.warn { background: var(--warn-soft); color: var(--warn); }
.lvl.ok { background: var(--ok-soft); color: var(--ok); }
.lvl.info { background: #eff3f7; color: var(--info); }

.advice { display: flex; flex-direction: column; }
.adv { display: grid; grid-template-columns: 34px 1fr; gap: 0 14px; padding: 13px 0; border-bottom: 1px solid var(--line-soft); }
.adv:last-child { border-bottom: none; }
.adv-p { font-family: var(--mono); font-size: 9.5px; font-weight: 700; letter-spacing: .06em; padding: 2px 0; text-align: center; border-radius: 2px; height: fit-content; margin-top: 1px; }
.adv-p.P0 { background: var(--bad-soft); color: var(--bad); }
.adv-p.P1 { background: var(--warn-soft); color: var(--warn); }
.adv-p.P2 { background: #eff3f7; color: var(--info); }
.adv-t { font-size: 12.5px; font-weight: 700; margin-bottom: 2px; }
.adv-d { font-size: 11.5px; color: var(--ink-2); }

/* ---- 结论块 ---- */
.statement {
  border: 1px solid var(--line); border-left: 3px solid var(--accent); border-radius: 9px;
  background: linear-gradient(180deg, #fbfdfc, #ffffff); padding: 16px 19px;
}
.statement.pass { border-left-color: var(--ok); }
.statement.conditional { border-left-color: var(--warn); }
.statement.fail { border-left-color: var(--bad); }
.statement-t { font-size: 13.5px; font-weight: 750; margin-bottom: 4px; }
.statement-d { font-size: 11.5px; color: var(--ink-2); }
.statement-d b { font-variant-numeric: tabular-nums; }

/* ---- 图表 ---- */
.fig { margin: 0 0 24px; padding: 2px 0 0; }
.fig:last-child { margin-bottom: 4px; }
.fig-h { display: flex; align-items: baseline; gap: 9px; margin-bottom: 8px; }
.fig-t { font-size: 11.5px; font-weight: 700; color: var(--ink-2); }
.fig-u { font-size: 10px; color: var(--ink-4); }
.fig-n { margin-left: auto; font-size: 10px; color: var(--ink-4); }
.fig svg { width: 100%; height: auto; display: block; }
.lg { display: flex; flex-wrap: wrap; gap: 15px; margin-top: 8px; font-size: 10.5px; color: var(--ink-3); }
.lg span { display: inline-flex; align-items: center; gap: 6px; }
.lg i { width: 11px; height: 2px; border-radius: 1px; display: inline-block; }
.lg i.dot { width: 7px; height: 7px; border-radius: 50%; }
.lg i.dash { background-image: linear-gradient(90deg, currentColor 55%, transparent 0); background-size: 6px 2px; }

/* ---- 构成环形图：四张一排 ---- */
.pie-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin: 10px 0 20px; }
.pie-card { border: 1px solid var(--line); border-radius: 8px; padding: 9px; background: var(--paper); break-inside: avoid; }
.pie-card-head { display: flex; justify-content: space-between; gap: 6px; font-size: 10px; font-weight: 700; color: var(--ink-2); margin-bottom: 2px; }
.pie-card-head span:last-child { color: var(--ink-4); font-weight: 600; }
.pie-card svg { width: 100%; max-width: 150px; height: auto; display: block; margin: 2px auto 4px; }
.pie-legend { display: flex; flex-direction: column; gap: 3px; }
.pie-legend-row { display: grid; grid-template-columns: 7px 1fr auto auto; gap: 4px; align-items: center; font-size: 9px; }
.pie-legend-row .dot { width: 7px; height: 7px; border-radius: 2px; }
.pie-legend-row .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-3); }
.pie-legend-row .val { color: var(--ink); }
.pie-legend-row .pct { color: var(--ink-4); min-width: 32px; text-align: right; }

/* ---- 布局 ---- */
.cols { display: grid; grid-template-columns: 1fr 1fr; gap: 26px; align-items: start; }
.cols.wide-right { grid-template-columns: 0.85fr 1.15fr; }
.prose { font-size: 11.5px; color: var(--ink-2); line-height: 1.78; }
.prose p { margin: 0 0 8px; }
.prose p:last-child { margin-bottom: 0; }
.note { font-size: 11px; color: var(--ink-3); margin-top: 8px; line-height: 1.7; }
.note-list { margin: 0; padding-left: 16px; font-size: 11px; color: var(--ink-3); line-height: 1.75; }
.note-list li { margin: 2px 0; }

/* ---- 计量条 ---- */
.meter { display: flex; align-items: center; gap: 9px; }
.meter-track { flex: 1; height: 6px; border-radius: 3px; background: var(--line-soft); overflow: hidden; min-width: 44px; }
.meter-fill { height: 100%; border-radius: 3px; background: var(--accent); }
.meter-val { font-variant-numeric: tabular-nums; font-size: 10.5px; color: var(--ink-3); width: 44px; text-align: right; flex: none; }

/* ---- 评分 ---- */
.score-head { display: flex; align-items: baseline; gap: 10px; margin-bottom: 10px; }
.score-num { font-size: 30px; font-weight: 800; letter-spacing: -.03em; font-variant-numeric: tabular-nums; line-height: 1; }
.score-gr { font-size: 11px; font-weight: 700; padding: 2px 8px; border-radius: 2px; }
.dim { padding: 7px 0; border-bottom: 1px solid var(--line-soft); }
.dim:last-child { border-bottom: none; }
.dim-h { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin-bottom: 4px; }
.dim-l { font-size: 11.5px; font-weight: 600; }
.dim-s { font-family: var(--mono); font-size: 11px; font-weight: 700; }
.dim-d { font-size: 10px; color: var(--ink-4); margin-top: 3px; }

/* ---- 附录 ---- */
.appx { font-size: 11px; }
.appx .tbl { font-size: 11px; }
.foot { margin-top: 40px; padding-top: 14px; border-top: 1px solid var(--line); font-size: 10px; color: var(--ink-4); line-height: 1.75; }
.foot-row { display: flex; justify-content: space-between; gap: 16px; flex-wrap: wrap; }

@media (max-width: 860px) {
  body { padding: 16px 10px 32px; }
  .sheet { padding: 26px 20px 28px; border-radius: 8px; }
  .meta { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .meta > div:nth-child(2n) { border-right: none; }
  .kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .cols, .cols.wide-right { grid-template-columns: 1fr; }
  .toc { padding: 14px 16px 8px; }
  .toc ol { grid-template-columns: 1fr; }
  .cover-title { font-size: 23px; }
  .cover:before { margin-bottom: 18px; }
}
@page { size: A4; margin: 15mm 14mm 15mm; }
@media print {
  body { background: #fff; padding: 0; font-size: 11px; }
  .sheet { max-width: none; box-shadow: none; padding: 0; border-radius: 0; }
  .toc, .meta, .tbl-wrap, .strip, .kpi, .statement { border-radius: 0; }
  .cover { break-after: page; }
  .sec { margin-top: 22px; break-inside: auto; }
  .sec-h { break-after: avoid; }
  .sub { break-after: avoid; }
  .fig, .tbl-wrap, .kpis, .strip, .statement { break-inside: avoid; }
  .finding, .adv { break-inside: avoid; }
}
`

/* ---------- 排版原语 ---------- */

function section(id: string, num: string, title: string, subtitle: string, body: string): string {
  return `<section class="sec" id="${id}">
<div class="sec-h"><span class="sec-n">${num}</span><h2 class="sec-t">${escapeHtml(title)}</h2><span class="sec-s">${escapeHtml(
    subtitle,
  )}</span></div>
${body}
</section>`
}

function tableHtml(
  headers: string[],
  rows: string[][],
  numericCols: number[] = [],
  options: { totalRow?: boolean; kv?: boolean } = {},
): string {
  const head = headers
    .map((name, index) => `<th${numericCols.includes(index) ? ' class="num"' : ''}>${escapeHtml(name)}</th>`)
    .join('')
  const body = rows
    .map((row, rowIndex) => {
      const isTotal = options.totalRow && rowIndex === rows.length - 1
      return `<tr${isTotal ? ' class="total"' : ''}>${row
        .map((cell, index) => `<td${numericCols.includes(index) ? ' class="num"' : ''}>${cell}</td>`)
        .join('')}</tr>`
    })
    .join('')
  const cls = options.kv ? 'tbl kv' : 'tbl'
  return `<div class="tbl-wrap"><table class="${cls}"><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>`
}

function kvRows(entries: Array<[string, string]>): string[][] {
  return entries.map(([key, value]) => [escapeHtml(key), value])
}

function meter(value: number, max: number, tone = 'var(--accent)'): string {
  const width = Math.max(1.5, Math.min(100, (value / Math.max(1e-9, max)) * 100))
  return `<div class="meter"><div class="meter-track"><div class="meter-fill" style="width:${width.toFixed(
    1,
  )}%;background:${tone}"></div></div><div class="meter-val">${fmtInt(value)}</div></div>`
}

/* ---------- 图表引擎 ---------- */

const CHART_MAX_POINTS = 300
let gradientSeq = 0

/** 把最大值抬到 2 位有效数字，避免坐标轴出现 9740 这类刻度 */
function niceMax(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1
  const step = Math.pow(10, Math.floor(Math.log10(value)) - 1)
  return Number((Math.ceil(value / step) * step).toFixed(6))
}

function axisNumber(value: number, max: number): string {
  if (max >= 1000) return Math.round(value).toLocaleString('zh-CN')
  if (max >= 10) return String(Math.round(value))
  if (max >= 1) return value.toFixed(1).replace(/\.0$/, '')
  return value.toFixed(2)
}

/** 秒数 → 0:00 / 12:30 形式的时间轴标签 */
function clockLabel(sec: number): string {
  const total = Math.max(0, Math.round(sec))
  const minute = Math.floor(total / 60)
  const second = total % 60
  return minute ? `${minute}:${String(second).padStart(2, '0')}` : `${total}s`
}

/** 秒数 → 报告实际时钟（YYYY-MM-DD HH:mm:ss），用于完整时间轴 */
function wallClock(startMs: number, sec: number): string {
  const date = new Date(startMs + sec * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

interface ChartSeries {
  name: string
  color: string
  values: number[]
  area?: boolean
  dashed?: boolean
}

interface LineChartOptions {
  xs: number[]
  series: ChartSeries[]
  unit?: string
  height?: number
  refLines?: Array<{ value: number; label: string; color?: string }>
  /** 高亮区间，例如升压期 */
  band?: { from: number; to: number; label: string }
  /** 强制 Y 轴上限 */
  yMax?: number
  /** 说明文字，渲染在图题右侧 */
  note?: string
  legend?: boolean
  /** 报告开始时间（epoch ms）：传入后横轴改用完整时钟时间并旋转标签 */
  startMs?: number
}

/**
 * 折线图：带完整 Y 轴刻度、时间轴刻度、轴线、可选面积填充与参考线
 *
 * 使用 preserveAspectRatio 默认行为（等比缩放），避免 900×190 被拉伸后
 * 描边与字号失真 —— 这是原版报告图表「看着不专业」的主因之一。
 */
function lineChart(title: string, options: LineChartOptions): string {
  const {
    xs,
    series,
    unit = '',
    height: baseHeight = 208,
    refLines = [],
    band,
    legend: showLegend = true,
    startMs,
  } = options
  const width = 760
  // 完整时间轴会旋转标签，额外预留底部空间，保持绘图区高度不变
  const pad = { left: 58, right: 26, top: 18, bottom: startMs ? 78 : 30 }
  const height = baseHeight + (startMs ? 48 : 0)
  const plotW = width - pad.left - pad.right
  const plotH = height - pad.top - pad.bottom

  const prepared = series.map((item) => ({ ...item, values: downsample(item.values, CHART_MAX_POINTS) }))
  const pointsX = downsample(xs, CHART_MAX_POINTS)
  const dataMax = Math.max(
    1,
    ...prepared.flatMap((item) => item.values),
    ...refLines.map((item) => item.value),
  )
  const yMax = options.yMax ?? niceMax(dataMax * 1.04)
  const t0 = pointsX[0] ?? 0
  const t1 = pointsX[pointsX.length - 1] ?? t0 + 1
  const span = t1 - t0 || 1
  const X = (time: number) => pad.left + ((time - t0) / span) * plotW
  const Y = (value: number) => pad.top + plotH - (Math.max(0, value) / yMax) * plotH

  const grids = Array.from({ length: 5 }, (_, index) => {
    const y = pad.top + (plotH / 4) * index
    const value = yMax * (1 - index / 4)
    return `<line x1="${pad.left}" y1="${y.toFixed(1)}" x2="${width - pad.right}" y2="${y.toFixed(
      1,
    )}" stroke="#eef1f3" stroke-width="1"/>
<text x="${pad.left - 9}" y="${(y + 3.5).toFixed(1)}" text-anchor="end" font-size="9.5" fill="#9aa6ad">${axisNumber(
      value,
      yMax,
    )}</text>`
  }).join('')

  // X 轴刻度：最多 6 个，均匀取值
  const tickCount = Math.min(6, pointsX.length)
  const ticks = Array.from({ length: tickCount }, (_, index) => {
    if (tickCount === 1) return pointsX[0]
    const at = Math.round((index / (tickCount - 1)) * (pointsX.length - 1))
    return pointsX[at]
  })
  const xAxis = ticks
    .map((time) => {
      const x = X(time)
      const labelY = pad.top + plotH + (startMs ? 13 : 16)
      const label = startMs ? wallClock(startMs, time) : clockLabel(time)
      const labelAttr = startMs
        ? `text-anchor="end" transform="rotate(-35 ${x.toFixed(1)} ${labelY.toFixed(1)})"`
        : 'text-anchor="middle"'
      return `<line x1="${x.toFixed(1)}" y1="${(pad.top + plotH).toFixed(1)}" x2="${x.toFixed(
        1,
      )}" y2="${(pad.top + plotH + 4).toFixed(1)}" stroke="#cfd7db" stroke-width="1"/>
<text x="${x.toFixed(1)}" y="${labelY.toFixed(1)}" ${labelAttr} font-size="9.5" fill="#9aa6ad">${label}</text>`
    })
    .join('')

  const bandRect = band
    ? `<rect x="${X(band.from).toFixed(1)}" y="${pad.top}" width="${Math.max(
        1,
        X(band.to) - X(band.from),
      ).toFixed(1)}" height="${plotH}" fill="#eef2f3"/>
<text x="${(X(band.from) + 5).toFixed(1)}" y="${pad.top + 12}" font-size="9" fill="#9aa6ad" letter-spacing=".06em">${escapeHtml(
        band.label,
      )}</text>`
    : ''

  const defs: string[] = []
  const paths = prepared
    .map((item) => {
      const segments = item.values.map((value, index) => {
        const time = pointsX[Math.min(index, pointsX.length - 1)] ?? t0
        return `${X(time).toFixed(1)},${Y(value).toFixed(1)}`
      })
      if (!segments.length) return ''
      if (item.area) {
        const gradientId = `g${gradientSeq++}`
        defs.push(
          `<linearGradient id="${gradientId}" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${item.color}" stop-opacity=".18"/><stop offset="100%" stop-color="${item.color}" stop-opacity="0"/></linearGradient>`,
        )
        const baseline = (pad.top + plotH).toFixed(1)
        const areaPath = `M${segments[0]} L${segments.slice(1).join(' L')} L${segments[
          segments.length - 1
        ].split(',')[0]},${baseline} L${segments[0].split(',')[0]},${baseline} Z`
        return `<path d="${areaPath}" fill="url(#${gradientId})" stroke="none"/>`
      }
      return ''
    })
    .join('')

  const lines = prepared
    .filter((item) => item.values.length)
    .map((item) => {
      const segments = item.values.map((value, index) => {
        const time = pointsX[Math.min(index, pointsX.length - 1)] ?? t0
        return `${X(time).toFixed(1)},${Y(value).toFixed(1)}`
      })
      return `<polyline fill="none" stroke="${item.color}" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"${
        item.dashed ? ' stroke-dasharray="4 3"' : ''
      } points="${segments.join(' ')}"/>`
    })
    .join('')

  const refs = refLines
    .map((item) => {
      const y = Y(item.value)
      if (y < pad.top - 1 || y > pad.top + plotH + 1) return ''
      const color = item.color ?? '#a23333'
      // 标签防贴边：贴近图表顶端时放到线下方，贴近底端时放到线上方
      const labelY =
        y < pad.top + 14
          ? y + 12
          : y > pad.top + plotH - 8
            ? y - 6
            : y - 4
      return `<line x1="${pad.left}" y1="${y.toFixed(1)}" x2="${width - pad.right}" y2="${y.toFixed(
        1,
      )}" stroke="${color}" stroke-width="1" stroke-dasharray="4 3" opacity=".65"/>
<text x="${width - pad.right - 2}" y="${labelY.toFixed(1)}" text-anchor="end" font-size="9" fill="${color}">${escapeHtml(
        item.label,
      )}</text>`
    })
    .join('')

  const legendHtml = showLegend
    ? `<div class="lg">${prepared
        .map(
          (item) =>
            `<span><i${item.dashed ? ' class="dash"' : ''} style="background:${item.color};color:${item.color}"></i>${escapeHtml(
              item.name,
            )}</span>`,
        )
        .join('')}${
        refLines.length
          ? refLines
              .map(
                (item) =>
                  `<span><i class="dash" style="background:${
                    item.color ?? '#a23333'
                  };color:${item.color ?? '#a23333'}"></i>${escapeHtml(item.label)}</span>`,
              )
              .join('')
          : ''
      }</div>`
    : ''

  return `<figure class="fig">
<div class="fig-h"><span class="fig-t">${escapeHtml(title)}</span>${
    unit ? `<span class="fig-u">单位：${escapeHtml(unit)}</span>` : ''
  }${options.note ? `<span class="fig-n">${escapeHtml(options.note)}</span>` : ''}</div>
<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)}">
<defs>${defs.join('')}</defs>
${bandRect}${grids}
<line x1="${pad.left}" y1="${pad.top + plotH}" x2="${width - pad.right}" y2="${
    pad.top + plotH
  }" stroke="#cfd7db" stroke-width="1"/>
<line x1="${pad.left}" y1="${pad.top}" x2="${pad.left}" y2="${pad.top + plotH}" stroke="#cfd7db" stroke-width="1"/>
${xAxis}${paths}${lines}${refs}
</svg>${legendHtml}</figure>`
}

/** 负载-延迟散点：横轴 QPS，纵轴平均 RT */
function scatterChart(title: string, rows: ReportSample[]): string {
  if (!rows.length) return ''
  const width = 760
  const height = 220
  const pad = { left: 58, right: 26, top: 18, bottom: 34 }
  const plotW = width - pad.left - pad.right
  const plotH = height - pad.top - pad.bottom
  const data = downsample(rows, CHART_MAX_POINTS)
  const xMax = niceMax(Math.max(1, ...data.map((item) => item.qps)) * 1.02)
  const yMax = niceMax(Math.max(1, ...data.map((item) => item.avgRt)) * 1.08)
  const X = (value: number) => pad.left + (value / xMax) * plotW
  const Y = (value: number) => pad.top + plotH - (value / yMax) * plotH

  const grids = Array.from({ length: 5 }, (_, index) => {
    const y = pad.top + (plotH / 4) * index
    return `<line x1="${pad.left}" y1="${y.toFixed(1)}" x2="${width - pad.right}" y2="${y.toFixed(
      1,
    )}" stroke="#eef1f3" stroke-width="1"/>
<text x="${pad.left - 9}" y="${(y + 3.5).toFixed(1)}" text-anchor="end" font-size="9.5" fill="#9aa6ad">${axisNumber(
      yMax * (1 - index / 4),
      yMax,
    )}</text>`
  }).join('')

  const xAxis = Array.from({ length: 5 }, (_, index) => {
    const value = (xMax / 4) * index
    const x = X(value)
    return `<text x="${x.toFixed(1)}" y="${(pad.top + plotH + 16).toFixed(1)}" text-anchor="middle" font-size="9.5" fill="#9aa6ad">${axisNumber(
      value,
      xMax,
    )}</text>`
  }).join('')

  const dots = data
    .map(
      (item) =>
        `<circle cx="${X(item.qps).toFixed(1)}" cy="${Y(item.avgRt).toFixed(1)}" r="2.1" fill="#14655a" opacity=".42"/>`,
    )
    .join('')

  return `<figure class="fig">
<div class="fig-h"><span class="fig-t">${escapeHtml(title)}</span><span class="fig-u">横轴 req/s · 纵轴 ms</span><span class="fig-n">每点为一秒采样</span></div>
<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)}">
${grids}<line x1="${pad.left}" y1="${pad.top + plotH}" x2="${width - pad.right}" y2="${
    pad.top + plotH
  }" stroke="#cfd7db" stroke-width="1"/>
<line x1="${pad.left}" y1="${pad.top}" x2="${pad.left}" y2="${pad.top + plotH}" stroke="#cfd7db" stroke-width="1"/>
${xAxis}${dots}
</svg></figure>`
}

/** 横向条形图：URL 平均响应、施压机 TPS 等离散对象对比 */
function barChart(
  title: string,
  items: Array<{ label: string; value: number; sub?: string }>,
  unit: string,
): string {
  const rows = items.filter((item) => Number.isFinite(item.value) && item.value >= 0)
  if (!rows.length) return ''
  const width = 760
  const rowH = 30
  const pad = { left: 200, right: 96, top: 12, bottom: 12 }
  const height = pad.top + pad.bottom + rows.length * rowH
  const plotW = width - pad.left - pad.right
  const max = niceMax(Math.max(1, ...rows.map((item) => item.value)) * 1.02)
  const bars = rows
    .map((item, index) => {
      const y = pad.top + index * rowH
      const barW = Math.max(1, (Math.min(item.value, max) / max) * plotW)
      const label =
        item.label.length > 34 ? `${item.label.slice(0, 33)}…` : item.label
      const valueText = `${fmtNum(item.value, unit === 'TPS' ? 1 : 0)} ${unit}`
      return `<text x="${pad.left - 9}" y="${(y + 13).toFixed(1)}" text-anchor="end" font-size="9.5" fill="#53616a">${escapeHtml(
        label,
      )}</text>
${
  item.sub
    ? `<text x="${pad.left - 9}" y="${(y + 24).toFixed(1)}" text-anchor="end" font-size="8" fill="#9aa6ad">${escapeHtml(
        item.sub,
      )}</text>`
    : ''
}
<rect x="${pad.left}" y="${(y + 6).toFixed(1)}" width="${plotW}" height="11" rx="2" fill="#eef1f3"/>
<rect x="${pad.left}" y="${(y + 6).toFixed(1)}" width="${barW.toFixed(1)}" height="11" rx="2" fill="#14655a"/>
<text x="${(pad.left + barW + 6).toFixed(1)}" y="${(y + 15).toFixed(1)}" font-size="9" fill="#414d55">${escapeHtml(
        valueText,
      )}</text>`
    })
    .join('')
  return `<figure class="fig">
<div class="fig-h"><span class="fig-t">${escapeHtml(title)}</span><span class="fig-u">横轴 ${escapeHtml(
    unit,
  )}</span><span class="fig-n">${rows.length} 项</span></div>
<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)}">${bars}</svg></figure>`
}

const PIE_COLORS = [
  '#14655a',
  '#37628f',
  '#9c6414',
  '#a23333',
  '#6b4c9a',
  '#17774a',
  '#8a5a2b',
  '#5b6b73',
]

/** 环形构成图：请求结果、状态码、错误类型、流量构成 */
function pieChart(
  title: string,
  items: Array<{ label: string; value: number; color?: string; display?: string }>,
  centerText = '',
): string {
  const rows = items.filter((item) => Number.isFinite(item.value) && item.value > 0)
  const total = rows.reduce((sum, item) => sum + item.value, 0)
  if (!rows.length || total <= 0) return ''
  const size = 164
  const cx = size / 2
  const cy = size / 2
  const radius = 58
  const circumference = 2 * Math.PI * radius
  let offset = 0
  const segments = rows
    .map((item, index) => {
      const length = (item.value / total) * circumference
      const color = item.color || PIE_COLORS[index % PIE_COLORS.length]
      const segment = `<circle cx="${cx}" cy="${cy}" r="${radius}" fill="none" stroke="${color}" stroke-width="20" stroke-dasharray="${length.toFixed(
        2,
      )} ${(circumference - length).toFixed(2)}" stroke-dashoffset="${(-offset).toFixed(
        2,
      )}" transform="rotate(-90 ${cx} ${cy})"/>`
      offset += length
      return segment
    })
    .join('')
  const legend = rows
    .map((item, index) => {
      const color = item.color || PIE_COLORS[index % PIE_COLORS.length]
      const pct = ((item.value / total) * 100).toFixed(1)
      return `<div class="pie-legend-row">
  <span class="dot" style="background:${color}"></span>
  <span class="name">${escapeHtml(item.label)}</span>
  <span class="val mono">${escapeHtml(item.display ?? fmtInt(item.value))}</span>
  <span class="pct mono">${pct}%</span>
</div>`
    })
    .join('')
  return `<div class="pie-card">
  <div class="pie-card-head"><span>${escapeHtml(title)}</span><span>${escapeHtml(
    centerText || fmtInt(total),
  )}</span></div>
  <svg viewBox="0 0 ${size} ${size}" role="img" aria-label="${escapeHtml(title)}">
    <circle cx="${cx}" cy="${cy}" r="${radius}" fill="none" stroke="#eef1f3" stroke-width="20"/>
    ${segments}
    <text x="${cx}" y="${cy - 1}" text-anchor="middle" font-size="15" font-weight="700" fill="#1b2420">${escapeHtml(
      centerText || fmtInt(total),
    )}</text>
    <text x="${cx}" y="${cy + 13}" text-anchor="middle" font-size="8" fill="#9aa6ad">合计</text>
  </svg>
  <div class="pie-legend">${legend}</div>
</div>`
}

/** 响应时间累积分布曲线（由直方图分桶还原） */
function cdfChart(title: string, rawPoints: Array<{ ms: number; pct: number }>): string {
  const points = rawPoints.filter(
    (item) => Number.isFinite(item.ms) && Number.isFinite(item.pct),
  )
  if (points.length < 2) return ''
  const width = 760
  const height = 210
  const pad = { left: 58, right: 26, top: 18, bottom: 34 }
  const plotW = width - pad.left - pad.right
  const plotH = height - pad.top - pad.bottom
  const xMax = niceMax(Math.max(1, ...points.map((item) => item.ms)) * 1.02)
  const X = (value: number) => pad.left + (value / xMax) * plotW
  const Y = (value: number) => pad.top + plotH - (value / 100) * plotH

  const msAt = (target: number): number => {
    for (let index = 1; index < points.length; index += 1) {
      const prev = points[index - 1]
      const current = points[index]
      if (current.pct >= target) {
        const ratio =
          current.pct === prev.pct ? 0 : (target - prev.pct) / (current.pct - prev.pct)
        return prev.ms + (current.ms - prev.ms) * ratio
      }
    }
    return points[points.length - 1].ms
  }

  const grids = Array.from({ length: 5 }, (_, index) => {
    const y = pad.top + (plotH / 4) * index
    return `<line x1="${pad.left}" y1="${y.toFixed(1)}" x2="${width - pad.right}" y2="${y.toFixed(
      1,
    )}" stroke="#eef1f3" stroke-width="1"/>
<text x="${pad.left - 9}" y="${(y + 3.5).toFixed(1)}" text-anchor="end" font-size="9.5" fill="#9aa6ad">${(
      100 - index * 25
    ).toFixed(0)}%</text>`
  }).join('')

  const xAxis = Array.from({ length: 5 }, (_, index) => {
    const value = (xMax / 4) * index
    return `<text x="${X(value).toFixed(1)}" y="${(pad.top + plotH + 16).toFixed(1)}" text-anchor="middle" font-size="9.5" fill="#9aa6ad">${axisNumber(
      value,
      xMax,
    )}</text>`
  }).join('')

  const line = points
    .map((item) => `${X(item.ms).toFixed(1)},${Y(item.pct).toFixed(1)}`)
    .join(' ')
  const area = `M${X(points[0].ms).toFixed(1)},${(pad.top + plotH).toFixed(1)} L${line.split(' ').join(' L')} L${X(
    points[points.length - 1].ms,
  ).toFixed(1)},${(pad.top + plotH).toFixed(1)} Z`

  const markers = [
    { pct: 50, label: 'P50', color: '#37628f' },
    { pct: 95, label: 'P95', color: '#9c6414' },
    { pct: 99, label: 'P99', color: '#a23333' },
  ]
    .map((item) => {
      const value = msAt(item.pct)
      const x = X(value)
      const y = Y(item.pct)
      return `<line x1="${x.toFixed(1)}" y1="${pad.top}" x2="${x.toFixed(1)}" y2="${(
        pad.top + plotH
      ).toFixed(1)}" stroke="${item.color}" stroke-width="1" stroke-dasharray="3 3" opacity=".5"/>
<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="2.6" fill="${item.color}"/>
<text x="${(x + 5).toFixed(1)}" y="${(y - 5).toFixed(1)}" font-size="9" fill="${item.color}">${item.label} ${value.toFixed(
        0,
      )}ms</text>`
    })
    .join('')

  return `<figure class="fig">
<div class="fig-h"><span class="fig-t">${escapeHtml(title)}</span><span class="fig-u">横轴 ms · 纵轴累积占比</span><span class="fig-n">由响应时间直方图还原</span></div>
<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)}">
${grids}<line x1="${pad.left}" y1="${pad.top + plotH}" x2="${width - pad.right}" y2="${
    pad.top + plotH
  }" stroke="#cfd7db" stroke-width="1"/>
<line x1="${pad.left}" y1="${pad.top}" x2="${pad.left}" y2="${pad.top + plotH}" stroke="#cfd7db" stroke-width="1"/>
${xAxis}<path d="${area}" fill="#14655a" fill-opacity=".07"/>
<polyline fill="none" stroke="#14655a" stroke-width="1.8" stroke-linejoin="round" points="${line}"/>
${markers}
</svg></figure>`
}

/** 四维体检雷达图 */
function radarChart(dims: Array<{ label: string; score: number | null }>): string {
  const size = 260
  const center = size / 2
  const radius = 88
  const count = Math.max(3, dims.length)
  const at = (index: number, ratio: number): [number, number] => {
    const angle = (Math.PI * 2 * index) / count - Math.PI / 2
    return [center + Math.cos(angle) * radius * ratio, center + Math.sin(angle) * radius * ratio]
  }
  const rings = [0.25, 0.5, 0.75, 1]
    .map((ratio) => {
      const poly = dims
        .map((_, index) => at(index, ratio).map((value) => value.toFixed(1)).join(','))
        .join(' ')
      return `<polygon points="${poly}" fill="none" stroke="#e7ebed" stroke-width="1"/>`
    })
    .join('')
  const axes = dims
    .map((_, index) => {
      const [x, y] = at(index, 1)
      return `<line x1="${center}" y1="${center}" x2="${x.toFixed(1)}" y2="${y.toFixed(
        1,
      )}" stroke="#e7ebed" stroke-width="1"/>`
    })
    .join('')
  const shape = dims
    .map((item, index) =>
      at(index, Math.min(1, Math.max(0.06, (item.score ?? 0) / 100)))
        .map((value) => value.toFixed(1))
        .join(','),
    )
    .join(' ')
  const labels = dims
    .map((item, index) => {
      const [x, y] = at(index, 1.2)
      const value = item.score === null ? '—' : item.score.toFixed(0)
      return `<text x="${x.toFixed(1)}" y="${(y + 3).toFixed(1)}" text-anchor="middle" font-size="10" font-weight="600" fill="#414d55">${escapeHtml(
        item.label,
      )} <tspan fill="#14655a">${value}</tspan></text>`
    })
    .join('')
  const dots = dims
    .map((item, index) => {
      const [x, y] = at(index, Math.min(1, Math.max(0.06, (item.score ?? 0) / 100)))
      return `<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="2.4" fill="#14655a"/>`
    })
    .join('')

  return `<svg viewBox="0 0 ${size} ${size}" style="max-width:250px;margin:0 auto;display:block" role="img" aria-label="四维体检雷达图">
${rings}${axes}<polygon points="${shape}" fill="rgba(20,101,90,.14)" stroke="#14655a" stroke-width="1.6"/>${dots}${labels}
</svg>`
}

/* ---------- 报告渲染 ---------- */

/**
 * 响应体读取耗时统计：按成功/失败请求数加权平均，
 * P95 取报告层直方图（分位数不能由两个分位数相减）；
 * 旧报告没有 body_avg / body_p95 / body_histogram 时返回 null，避免把缺失当成 0。
 */
function bodyLatencyStats(detail: ReportDetail): { avg: number | null; p95: number | null } {
  const samples = detail.samples ?? []
  const buckets = detail.bodyHistogram ?? []
  const available =
    buckets.length > 0 ||
    samples.some((sample) => sample.bodyAvg != null || sample.bodyP95 != null)
  if (!available) return { avg: null, p95: null }

  let sum = 0
  let weight = 0
  for (const sample of samples) {
    // Body 只统计拿到响应的请求；旧报告没有 responseCount 时退回成功+失败数
    const count =
      sample.responseCount && sample.responseCount > 0
        ? sample.responseCount
        : sample.successCount + sample.errorCount
    sum += (sample.bodyAvg || 0) * count
    weight += count
  }
  const avg = weight ? sum / weight : null

  const total = buckets.reduce((acc, item) => acc + item.count, 0)
  let p95: number | null
  if (!total) {
    p95 = samples.length ? Math.max(0, ...samples.map((sample) => sample.bodyP95 || 0)) : null
  } else {
    const target = total * 0.95
    let acc = 0
    p95 = 0
    for (const bucket of buckets) {
      acc += bucket.count
      if (acc >= target) {
        p95 = Number.isFinite(bucket.maxMs) ? (bucket.minMs + bucket.maxMs) / 2 : bucket.minMs
        break
      }
    }
  }
  return { avg, p95 }
}

function kpiBlock(detail: ReportDetail, qps: QpsOverview, peaks: ReportPeaks | null): string {
  const report = detail.report
  const peakValue = qps.max || peaks?.qps.value || 0
  const peakNote = peaks
    ? `第 ${peaks.qps.second} 秒 · 平台期 ${peaks.qps.heldSec} 秒`
    : '所有采样点中的最大值'
  const successRate = 100 - report.errorRate
  // 指标按语义分色：吞吐用主色、峰值用蓝、成功率与 APDEX 按结果取色、延迟用警示色
  const cards: Array<{ k: string; v: string; u: string; n: string; tone: string }> = [
    {
      k: '总请求数',
      v: fmtInt(report.totalRequests),
      u: '',
      n: `并发 ${fmtInt(report.concurrency)}`,
      tone: 'accent',
    },
    {
      k: '平均 QPS',
      v: fmtInt(qps.avg),
      u: 'req/s',
      n: `峰值 ${fmtInt(peakValue)}`,
      tone: 'accent',
    },
    {
      k: '峰值 QPS',
      v: fmtInt(peakValue),
      u: 'req/s',
      n: peakNote,
      tone: 'info',
    },
    {
      k: '90% 分布 QPS',
      v: qps.used > 0 ? fmtInt(qps.ninety) : '—',
      u: qps.used > 0 ? 'req/s' : '',
      n: qps.used > 0 ? `剔除 ${fmtInt(qps.dropped)} 个升温采样点` : '采样不足，无法计算',
      tone: 'ok',
    },
    {
      k: '单并发 QPS',
      v: fmtNum(qps.perConcurrent, 2),
      u: 'req/s',
      n: `平均 QPS ÷ ${fmtInt(report.concurrency)} 并发`,
      tone: 'violet',
    },
    {
      k: '成功率',
      v: successRate.toFixed(2),
      u: '%',
      n: `失败 ${fmtInt(report.errorCount)} 次 · 错误率 ${report.errorRate.toFixed(2)}%`,
      tone: successRate >= 99 ? 'ok' : successRate >= 95 ? 'warn' : 'bad',
    },
    {
      k: '平均响应时间',
      v: fmtNum(report.avgRt),
      u: 'ms',
      n: `TTFB ${fmtNum(report.ttfbAvg)} ms`,
      tone: 'info',
    },
    {
      k: 'P95 延迟',
      v: fmtNum(report.p95Rt),
      u: 'ms',
      n: `P99 ${fmtNum(report.p99Rt)} ms`,
      tone: 'warn',
    },
    {
      k: 'APDEX',
      v: report.apdex.toFixed(3),
      u: '',
      n: apdexGrade(report.apdex),
      tone: report.apdex >= 0.85 ? 'ok' : report.apdex >= 0.7 ? 'warn' : 'bad',
    },
  ]
  return `<div class="kpis">${cards
    .map(
      (item) =>
        `<div class="kpi kpi-tone-${item.tone}"><div class="kpi-k">${escapeHtml(
          item.k,
        )}</div><div class="kpi-v">${escapeHtml(item.v)}${
          item.u ? `<span>${escapeHtml(item.u)}</span>` : ''
        }</div><div class="kpi-n">${escapeHtml(item.n)}</div></div>`,
    )
    .join('')}</div>`
}

const LEVEL_LABELS: Record<FindingLevel, string> = {
  bad: '风险',
  warn: '关注',
  info: '观察',
  ok: '正常',
}

function findingsBlock(detail: ReportDetail): string {
  const findings = buildFindings(detail)
  if (!findings.length) return '<div class="note">数据量不足，无法提炼关键发现。</div>'
  return `<div class="findings">${findings
    .map(
      (item) => `<div class="finding ${item.level}"><i></i><div>
<div class="finding-t"><span class="lvl ${item.level}">${LEVEL_LABELS[item.level]}</span>${escapeHtml(
        item.title,
      )}</div>
<div class="finding-d">${escapeHtml(item.detail)}</div></div></div>`,
    )
    .join('')}</div>`
}

function adviceBlock(detail: ReportDetail): string {
  const advice = buildAdvice(detail)
  return `<div class="advice">${advice
    .map(
      (item) => `<div class="adv"><div class="adv-p ${item.priority}">${item.priority}</div><div>
<div class="adv-t">${escapeHtml(item.title)}</div>
<div class="adv-d">${escapeHtml(item.detail)}</div></div></div>`,
    )
    .join('')}</div>`
}

function scoreBlock(detail: ReportDetail): string {
  const score = scoreReport(detail)
  const tone =
    score.total === null
      ? 'var(--ink-3)'
      : score.total >= 90
        ? 'var(--ok)'
        : score.total >= 75
          ? 'var(--accent)'
          : score.total >= 60
            ? 'var(--warn)'
            : 'var(--bad)'
  const bg =
    score.total === null
      ? '#f3f5f6'
      : score.total >= 90
        ? 'var(--ok-soft)'
        : score.total >= 75
          ? 'var(--accent-soft)'
          : score.total >= 60
            ? 'var(--warn-soft)'
            : 'var(--bad-soft)'

  const dims = score.dimensions
    .map((item) => {
      const value = item.score === null ? 0 : item.score
      const color =
        item.score === null
          ? 'var(--line)'
          : value >= 90
            ? 'var(--ok)'
            : value >= 75
              ? 'var(--accent)'
              : value >= 60
                ? 'var(--warn)'
                : 'var(--bad)'
      return `<div class="dim"><div class="dim-h"><span class="dim-l">${escapeHtml(
        item.label,
      )}</span><span class="dim-s" style="color:${color}">${
        item.score === null ? '—' : item.score.toFixed(0)
      }</span></div>
<div class="meter-track"><div class="meter-fill" style="width:${Math.max(
        1.5,
        value,
      ).toFixed(0)}%;background:${color}"></div></div>
<div class="dim-d">${escapeHtml(item.detail)}</div></div>`
    })
    .join('')

  return `<div class="score-head"><span class="score-num" style="color:${tone}">${
    score.total === null ? '—' : score.total.toFixed(0)
  }</span><span class="score-gr" style="background:${bg};color:${tone}">${escapeHtml(
    score.grade,
  )}</span><span class="fig-n">满分 100 · 四维等权</span></div>
${dims}
<div class="note">${escapeHtml(score.ruleNote)}</div>`
}

/**
 * 解析「被测目标」的实际地址。
 *
 * v1.0.8 之前配置快照里存的是变量模板（如 `{{base_url}}/post`），
 * 这里用本次运行明细里第一条请求的实际 URL 兜底还原 —— 明细记录存的
 * 是后端渲染后的真实请求地址，因此老报告也能展示全路径。
 */
function resolvedTargetUrl(detail: ReportDetail, requests: ReportRequestRecord[]): string {
  const raw = detail.config?.url?.trim() ?? ''
  if (raw && !raw.includes('{{')) return raw
  const resolved = detail.config?.target?.resolvedUrl?.trim() ?? ''
  if (resolved && !resolved.includes('{{')) return resolved
  const fromRecord = requests.find((item) => item.url)?.url?.trim() ?? ''
  return fromRecord || raw || '未记录'
}

/** 完整 URL 单元格：整行可换行 + 复制按钮（3.2.4） */
function urlCell(url: string): string {
  const text = url || '未记录'
  return `<span class="url-cell"><span class="mono break">${escapeHtml(
    text,
  )}</span><button class="copy" type="button" data-copy="${escapeHtml(text)}">复制</button></span>`
}

/** 施压端能力卡片（3.7.2）：未检测时明确标注，不推断数值 */
function capacityBlock(detail: ReportDetail): string {
  const capacity = detail.config?.machineCapacity
  if (!capacity?.maxQps) {
    return `<div class="sub">施压端能力</div>
<div class="note">未检测本机施压能力。可在「设置 → 本机施压能力」完成检测后重新压测；本报告不推断数值。</div>`
  }
  const usage = capacity.maxQps > 0 ? (detail.report.avgQps / capacity.maxQps) * 100 : 0
  const limitText = `${capacity.reachedLimit === false ? '≥ ' : ''}${fmtInt(capacity.maxQps)} QPS`
  const usageNote =
    usage >= 80
      ? '客户端接近能力上限，结论需谨慎。'
      : usage < 50
        ? '客户端余量充足，瓶颈不在施压端。'
        : '客户端仍有余量，可结合目标侧指标判断瓶颈。'
  return `<div class="sub">施压端能力</div>
<div class="strip">
  <div><span>客户端极限</span><b>${limitText}</b></div>
  <div><span>本次平均</span><b>${fmtInt(detail.report.avgQps)} QPS</b></div>
  <div><span>客户端负载占比</span><b>${fmtNum(usage, 1)}%</b></div>
  <div><span>检测时间</span><b>${fmtTime(capacity.probedAt)}</b></div>
  <div><span>CPU / 峰值</span><b>${capacity.cores ?? '—'} 核${
    capacity.cpuPeak ? ` / ${fmtNum(capacity.cpuPeak, 1)} 核` : ''
  }</b></div>
  <div><span>饱和并发</span><b>${capacity.saturationConcurrent ?? '—'}</b></div>
</div>
<div class="note">${usageNote}回环检测值，非目标系统容量，实际受网络 RTT 与目标侧影响。</div>`
}

const CLUSTER_HEALTH_LABELS: Record<string, string> = {
  ok: '正常',
  waiting: '等待首样',
  gap: '采样缺口',
  lost: '无数据',
  done: '正常结束',
}

/* ---------- 基线对比 ---------- */

interface BaselineCompareRow {
  label: string
  current: string
  base: string
  delta: string
  tone: 'ok' | 'warn' | 'bad' | 'muted'
}

const TONE_COLORS: Record<BaselineCompareRow['tone'], string> = {
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  bad: 'var(--bad)',
  muted: 'var(--ink-3)',
}

/**
 * 基线与本次报告的对比行，口径与报告页一致：
 * 延迟上升 >15% 关注、>40% 风险；吞吐下降 >5% 关注、>20% 风险；错误率绝对值抬升同理。
 */
function baselineCompareRows(current: ReportDetail, baseline: ReportDetail): BaselineCompareRow[] {
  const c = current.report
  const b = baseline.report
  const ratio = (value: number, origin: number) =>
    origin > 0 ? ((value - origin) / origin) * 100 : null
  const deltaText = (value: number | null) =>
    value === null ? '—' : `${value >= 0 ? '+' : ''}${value.toFixed(1)}%`
  const latencyRow = (label: string, value: number, origin: number): BaselineCompareRow => {
    const pct = ratio(value, origin)
    return {
      label,
      current: `${fmtNum(value)} ms`,
      base: `${fmtNum(origin)} ms`,
      delta: deltaText(pct),
      tone: pct === null ? 'muted' : pct > 40 ? 'bad' : pct > 15 ? 'warn' : 'ok',
    }
  }
  const qpsPct = ratio(c.avgQps, b.avgQps)
  const errPct = ratio(c.errorRate, b.errorRate)
  return [
    latencyRow('P95 延迟', c.p95Rt, b.p95Rt),
    latencyRow('P99 延迟', c.p99Rt, b.p99Rt),
    latencyRow('平均延迟', c.avgRt, b.avgRt),
    {
      label: '平均 QPS',
      current: fmtNum(c.avgQps),
      base: fmtNum(b.avgQps),
      delta: deltaText(qpsPct),
      tone: qpsPct === null ? 'muted' : qpsPct < -20 ? 'bad' : qpsPct < -5 ? 'warn' : 'ok',
    },
    {
      label: '错误率',
      current: `${c.errorRate.toFixed(2)}%`,
      base: `${b.errorRate.toFixed(2)}%`,
      delta: deltaText(errPct),
      tone:
        errPct === null
          ? 'muted'
          : c.errorRate - b.errorRate > 0.5
            ? 'bad'
            : c.errorRate - b.errorRate > 0.1
              ? 'warn'
              : 'ok',
    },
  ]
}

/** 基线对比区块：未选择基线或基线已被删除时不渲染 */
function baselineBlock(detail: ReportDetail, baseline: ReportDetail | null | undefined): string {
  if (!baseline || baseline.report.id === detail.report.id) return ''
  const rows = baselineCompareRows(detail, baseline)
  if (!rows.length) return ''
  return `<div class="sub">基线对比<span class="hint">对照报告 R-${baseline.report.id} · ${escapeHtml(
    reportTitle(baseline.report),
  )}</span></div>
${tableHtml(
  ['指标', '本次', '基线', '变化'],
  rows.map((row) => [
    escapeHtml(row.label),
    row.current,
    row.base,
    `<span style="color:${TONE_COLORS[row.tone]};font-weight:600">${row.delta}</span>`,
  ]),
  [1, 2, 3],
)}
<div class="note">延迟上升超过 15% 或吞吐下降超过 5% 需要关注；变化为「本次相对基线」的百分比。</div>`
}

/* ---------- 分布式压测 ---------- */

const NODE_SERIES_COLORS = [
  '#14655a',
  '#37628f',
  '#9c6414',
  '#a23333',
  '#6b4c9a',
  '#17774a',
  '#8a5a2b',
  '#2f6f7e',
  '#7a3b52',
  '#4f5d2f',
]

/** 节点数超过预设色板时按色相继续生成，避免多条曲线共用同一颜色 */
function nodeSeriesColor(index: number): string {
  return NODE_SERIES_COLORS[index] ?? `hsl(${(index * 47) % 360} 42% 38%)`
}

/** 节点曲线：按秒取并集对齐，缺采样补 0，避免不同节点曲线错位 */
function nodeSeriesChart(
  cluster: ClusterReportMeta,
  pick: (point: ClusterReportMeta['nodeSeries'][number]['points'][number]) => number,
  title: string,
  unit: string,
  startMs?: number,
): string {
  // 旧报告可能没有 points 字段，按页面同款防御式读取，避免整份导出失败
  const series = (cluster.nodeSeries ?? []).filter((item) => item.points?.length)
  if (!series.length) return ''
  const seconds = Array.from(
    new Set(series.flatMap((item) => item.points.map((point) => point.sec))),
  ).sort((a, b) => a - b)
  const chartSeries = series.map((item, index) => {
    const bySec = new Map(item.points.map((point) => [point.sec, pick(point)]))
    return {
      name: item.node,
      color: nodeSeriesColor(index),
      values: seconds.map((sec) => bySec.get(sec) ?? 0),
    }
  })
  return lineChart(title, {
    xs: seconds,
    series: chartSeries,
    unit,
    height: 220,
    note: `${series.length} 个节点 · 缺采样补 0`,
    startMs,
  })
}

/**
 * 节点资源曲线只有在采集到整机数据时才输出，
 * 与报告页一致：老报告没有 host 字段时不画一条全 0 的线。
 */
function clusterHasHostSeries(cluster: ClusterReportMeta): boolean {
  return (cluster.nodeSeries ?? []).some((item) =>
    (item.points ?? []).some(
      (point) => (point.hostCpuPercent ?? 0) > 0 || (point.memMb ?? 0) > 0,
    ),
  )
}

/** 集群节点资源峰值：进程口径 + 整机口径 + 网卡，未采集的项自动省略 */
function nodeResourceText(node: ClusterReportMeta['nodes'][number]): string {
  const parts: string[] = []
  if (node.cpuPeak) parts.push(`进程 ${node.cpuPeak.toFixed(2)} 核`)
  if (node.hostCpuPeak) parts.push(`整机 ${node.hostCpuPeak.toFixed(0)}%`)
  if (node.memPeakMb) parts.push(fmtBytes(node.memPeakMb * 1024 * 1024))
  if (node.netPeakKbps) parts.push(`↓↑ ${(node.netPeakKbps / 1024).toFixed(1)} MB/s`)
  return parts.length ? parts.join(' · ') : '未采集'
}

function clusterHealthText(node: ClusterReportMeta['nodes'][number]): string {
  const state = node.health?.state ?? ''
  const label = CLUSTER_HEALTH_LABELS[state] ?? state ?? '—'
  return node.health?.message ? `${label}（${node.health.message}）` : label
}

/**
 * 分布式压测区块：集群概览、峰值专题与节点贡献表。
 * 页面与 Excel 都已经有这部分，HTML 导出此前完全缺失，导致集群报告只能看到合并后的单机指标。
 */
function clusterBlock(detail: ReportDetail): string {
  const cluster = detail.config?.cluster
  if (!cluster) return ''
  const nodes = cluster.nodes ?? []
  const totalQps = nodes.reduce((sum, node) => sum + (node.qpsAvg ?? 0), 0)

  const headRows = kvRows([
    ['协议版本', `v${cluster.protocolVersion}`],
    ['主节点', escapeHtml(cluster.masterNode || '—')],
    ['节点数', fmtInt(nodes.length)],
    ['集群总 QPS', `${fmtInt(totalQps)} req/s`],
    ['分配总并发', fmtInt(cluster.totalConcurrent)],
    ['起跑 / 结束', `${fmtTime(cluster.startedAt)} / ${fmtTime(cluster.finishedAt)}`],
    ['丢弃采样', `${fmtInt(cluster.droppedSamples)} 条`],
    [
      '结果完整性',
      cluster.incomplete
        ? `<span style="color:var(--bad);font-weight:600">不完整</span>${
            cluster.note ? `：${escapeHtml(cluster.note)}` : ''
          }`
        : '<span style="color:var(--ok);font-weight:600">完整</span>',
    ],
  ])

  const peaks = cluster.peaks
  const peakTable = peaks
    ? tableHtml(
        ['峰值指标', '数值', '出现秒', '平台期(秒)', '来源节点'],
        [
          ['峰值 QPS', fmtInt(peaks.qps.value), String(peaks.qps.second), String(peaks.qps.heldSec), escapeHtml(peaks.qps.topNode || '—')],
          ['峰值并发', fmtInt(peaks.concurrent.value), String(peaks.concurrent.second), String(peaks.concurrent.heldSec), '—'],
          ['最差 P95(ms)', fmtNum(peaks.p95.value, 0), String(peaks.p95.second), String(peaks.p95.heldSec), '—'],
          ['最差 P99(ms)', fmtNum(peaks.p99.value, 0), String(peaks.p99.second), String(peaks.p99.heldSec), '—'],
          ['峰值带宽(B/s)', fmtBytes(peaks.bandwidth.value), String(peaks.bandwidth.second), String(peaks.bandwidth.heldSec), '—'],
          ['最高错误率(%)', peaks.errorRate.value.toFixed(2), String(peaks.errorRate.second), String(peaks.errorRate.heldSec), '—'],
        ],
        [1, 2, 3],
      )
    : '<div class="note">本次集群报告未记录峰值专题。</div>'

  const nodeRows = nodes.map((node) => [
    escapeHtml(node.name || '—'),
    node.role === 'master' ? '主节点' : '从节点',
    `<span class="mono break">${escapeHtml(node.addr || '—')}</span>`,
    fmtNum(node.weight, 2),
    fmtInt(node.concurrent),
    fmtNum(node.qpsAvg),
    fmtNum(node.peakQps),
    fmtInt(node.peakConcurrent),
    `${((node.share ?? 0) * 100).toFixed(1)}%`,
    escapeHtml(clusterHealthText(node)),
    escapeHtml(nodeResourceText(node)),
    node.clockOffsetMs != null
      ? `${Number(node.clockOffsetMs).toFixed(1)} ± ${Number(node.clockUncertaintyMs ?? 0).toFixed(1)} ms${
          Math.abs(node.clockDriftMs ?? 0) >= 1
            ? `（漂移 ${node.clockDriftMs > 0 ? '+' : ''}${node.clockDriftMs.toFixed(1)} ms）`
            : ''
        }`
      : '—',
    fmtInt(node.droppedSamples),
    `${fmtTime(node.startMs)}${node.startMs && node.endMs ? ` · ${fmtDuration(node.startMs, node.endMs)}` : ''}`,
  ])

  // 与报告页一致的 6 种节点曲线口径；整机资源类仅在采集到时输出
  const hasHostSeries = clusterHasHostSeries(cluster)
  const clusterStartMs = detail.report.startTime || undefined
  const nodeCharts = [
    nodeSeriesChart(cluster, (point) => point.qps, '节点 QPS 曲线', 'req/s', clusterStartMs),
    nodeSeriesChart(cluster, (point) => point.concurrent, '节点并发曲线', '并发', clusterStartMs),
    nodeSeriesChart(cluster, (point) => point.p95, '节点 P95 曲线', 'ms', clusterStartMs),
    hasHostSeries
      ? nodeSeriesChart(
          cluster,
          (point) => point.hostCpuPercent ?? 0,
          '节点整机 CPU 曲线',
          '%',
          clusterStartMs,
        )
      : '',
    hasHostSeries
      ? nodeSeriesChart(
          cluster,
          (point) => point.memMb ?? 0,
          '节点进程内存曲线',
          'MB',
          clusterStartMs,
        )
      : '',
    hasHostSeries
      ? nodeSeriesChart(
          cluster,
          (point) => (point.netRxKbps ?? 0) + (point.netTxKbps ?? 0),
          '节点网络曲线',
          'KB/s',
          clusterStartMs,
        )
      : '',
  ].join('')

  return `<div class="sub">分布式压测<span class="hint">主从节点合并口径，数值为主节点汇聚结果</span></div>
${tableHtml(['项目', '取值'], headRows, [], { kv: true })}
<div class="sub">集群峰值专题<span class="hint">按合并后的每秒采样统计</span></div>
${peakTable}
${
  nodeCharts
    ? `<div class="sub">节点曲线<span class="hint">按节点上报的秒级采样绘制，缺采样补 0</span></div>${nodeCharts}`
    : ''
}
${
  nodeRows.length
    ? `<div class="sub">节点贡献<span class="hint">贡献占比 = 节点平均 QPS ÷ 集群总 QPS</span></div>
${tableHtml(
  ['节点', '角色', '地址', '权重', '分配并发', '平均 QPS', '峰值 QPS', '峰值并发', '贡献占比', '采集健康', '资源峰值', '时钟偏移', '丢弃采样', '施压窗口'],
  nodeRows,
  [3, 4, 5, 6, 7, 8, 12],
)}`
    : '<div class="note">未记录节点明细。</div>'}`
}

function renderHtml(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
  options: { includePools?: boolean; baseline?: ReportDetail | null } = {},
): string {
  const report = detail.report
  const config = detail.config
  const metrics = config?.metrics
  const agent = config?.agent
  const samples = detail.samples ?? []
  const seconds = durationSeconds(detail)
  const rows = downsample(samples, CHART_MAX_POINTS)
  /** 趋势图横轴统一使用报告实际开始时间，旧报告缺失时回退到相对秒数 */
  const startMs = report.startTime || undefined

  const qps = qpsOverview(detail, { rampUpSec: config?.rampUpSec ?? 0 })
  const peaks = reportPeaks(detail)
  const peakQps = qps.max || peaks?.qps.value || 0
  const bodyStats = bodyLatencyStats(detail)
  const checks = selfCheck(detail)
  const checksPassed = checks.every((item) => item.pass)
  const sla = slaRows(detail)
  const slaPassed = sla.every((row) => row.pass)

  const bytesReceived = report.bytesTotal ?? 0
  const bytesSent = metrics?.bytesSent ?? 0
  const totalTraffic = bytesReceived + bytesSent
  const recordMode = metrics?.recordMode ?? config?.recordDetail ?? 'full'
  const droppedRecords = metrics?.recordsDropped ?? 0
  const droppedErrors = metrics?.errorsDropped ?? 0

  const modeText = modeLabelText(config?.mode, detail.stages.length)
  const rampUp = config?.rampUpSec ?? 0
  const machineCount = config?.cluster?.nodes?.length ?? 1
  const rampText = config?.stages?.length
    ? config.stages.map((stage) => `${stage.concurrent}C/${stage.durationSec}s`).join(' → ')
    : rampUp > 0
      ? `升压 ${rampUp}s`
      : '固定并发'

  /* ---- 03 吞吐 ---- */
  const throughput = lineChart('吞吐量（每秒完成请求数）', {
    xs: rows.map((item) => item.elapsedSec),
    series: [{ name: '实际 QPS', color: '#14655a', values: rows.map((item) => item.qps), area: true }],
    unit: 'req/s',
    refLines: report.targetRps
      ? [{ value: report.targetRps, label: `目标 ${fmtInt(report.targetRps)} req/s`, color: '#37628f' }]
      : [],
    band: rampUp > 1 ? { from: 0, to: rampUp, label: '升压期' } : undefined,
    note: `90% 分布 ${fmtInt(qps.ninety)} req/s`,
    startMs,
  })

  const concurrencyTrend = lineChart('并发数趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '并发', color: '#37628f', values: rows.map((item) => item.concurrent), area: true },
    ],
    unit: '并发',
    height: 170,
    startMs,
  })

  const successQpsTrend = lineChart('成功 QPS（每秒成功请求数）', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '成功 QPS', color: '#17774a', values: rows.map((item) => item.successCount), area: true },
    ],
    unit: 'req/s',
    height: 170,
    note: `成功合计 ${fmtInt(report.successCount)} 次`,
    startMs,
  })

  const failureQpsTrend = lineChart('失败 QPS（每秒失败请求数）', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '失败 QPS', color: '#a23333', values: rows.map((item) => item.errorCount), area: true },
    ],
    unit: 'req/s',
    height: 170,
    note: `失败合计 ${fmtInt(report.errorCount)} 次`,
    startMs,
  })

  const bandwidth = lineChart('网络吞吐（接收字节 / 秒）', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '接收', color: '#37628f', values: rows.map((item) => item.bytes / 1024), area: true },
    ],
    unit: 'KB/s',
    note: `接收合计 ${fmtBytes(bytesReceived)} · 发送合计 ${fmtBytes(bytesSent)}`,
    startMs,
  })

  const resultPie = pieChart('请求结果构成', [
    { label: '成功', value: report.successCount, color: '#2A7253' },
    { label: '失败', value: report.errorCount, color: '#A83A2C' },
  ])
  const trafficPie = pieChart(
    '流量构成',
    [
      { label: '接收', value: bytesReceived, color: '#37628f', display: fmtBytes(bytesReceived) },
      {
        label: '发送',
        value: bytesSent,
        color: '#9c6414',
        display: bytesSent > 0 ? fmtBytes(bytesSent) : '未记录',
      },
    ],
    fmtBytes(bytesReceived + bytesSent),
  )
  const statusColor = (code: number) =>
    code >= 500 ? '#a23333' : code >= 400 ? '#9c6414' : code >= 300 ? '#37628f' : '#14655a'
  const statusPie = pieChart(
    '状态码分布',
    [...detail.statusCodes]
      .sort((a, b) => b.count - a.count)
      .slice(0, 8)
      .map((item) => ({
        label: `状态码 ${item.code}`,
        value: item.count,
        color: statusColor(item.code),
      })),
  )
  const errorTypePie = pieChart(
    '错误类型分布',
    [...detail.errorTypes]
      .sort((a, b) => b.count - a.count)
      .slice(0, 8)
      .map((item) => ({
        label: item.errorType,
        value: item.count,
      })),
  )
  const stageQpsBar =
    detail.stages.length > 1
      ? barChart(
          '阶段平均 QPS 对比',
          detail.stages.map((item) => ({
            label: `阶段 ${item.index + 1}`,
            value: item.avgQps,
            sub: `${item.concurrent} 并发`,
          })),
          'QPS',
        )
      : ''
  const stageP95Bar =
    detail.stages.length > 1
      ? barChart(
          '阶段 P95 响应时间对比',
          detail.stages.map((item) => ({
            label: `阶段 ${item.index + 1}`,
            value: item.p95Rt,
            sub: `${item.concurrent} 并发`,
          })),
          'ms',
        )
      : ''

  const compositionBlock = [resultPie, statusPie, errorTypePie, trafficPie].some(Boolean)
    ? `<div class="sub">构成分析<span class="hint">请求结果、状态码、错误类型与流量占比</span></div>
<div class="pie-grid">${resultPie}${statusPie}${errorTypePie}${trafficPie}</div>`
    : ''

  /* ---- 04 响应时间 ---- */
  const latency = lineChart('响应时间分位趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '平均', color: '#414d55', values: rows.map((item) => item.avgRt) },
      { name: 'P95', color: '#9c6414', values: rows.map((item) => item.p95Rt) },
      { name: 'P99', color: '#a23333', values: rows.map((item) => item.p99Rt) },
    ],
    unit: 'ms',
    height: 220,
    refLines: config?.sla?.p95Ms
      ? [{ value: config.sla.p95Ms, label: `SLA P95 ≤ ${config.sla.p95Ms} ms`, color: '#9c6414' }]
      : [],
    startMs,
  })

  const avgRtTrend = lineChart('平均响应时间', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '平均响应时间', color: '#14655a', values: rows.map((item) => item.avgRt), area: true },
    ],
    unit: 'ms',
    height: 170,
    note: `整体平均 ${fmtNum(report.avgRt)} ms`,
    startMs,
  })

  const cdf = cdfChart('响应时间累积分布', cdfPoints(detail))
  const qpsLatency = scatterChart('QPS 与平均响应时间', rows)

  const bodySeries = rows.some((item) => item.bodyAvg > 0)
    ? [{ name: 'Body 平均', color: '#17774a', values: rows.map((item) => item.bodyAvg) }]
    : []
  const ttfb = lineChart('TTFB 与响应体读取耗时趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: 'TTFB 平均', color: '#6b5b95', values: rows.map((item) => item.ttfbAvg), area: true },
      ...bodySeries,
    ],
    unit: 'ms',
    height: 190,
    note: `TTFB P95 ${fmtNum(report.ttfbP95)} ms`,
    startMs,
  })

  /* ---- 05 错误 ---- */
  const errorCountTrend = lineChart('错误数趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '错误数', color: '#a23333', values: rows.map((item) => item.errorCount), area: true },
    ],
    unit: '个/s',
    height: 170,
    note: `失败合计 ${fmtInt(report.errorCount)} 次`,
    startMs,
  })

  const errorTrend = lineChart('错误率趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '错误率', color: '#a23333', values: rows.map((item) => errorRateOf(item)), area: true },
    ],
    unit: '%',
    height: 170,
    refLines: config?.sla?.errorRate
      ? [{ value: config.sla.errorRate, label: `SLA ≤ ${config.sla.errorRate}%`, color: '#a23333' }]
      : [],
    startMs,
  })

  /* ---- 表格 ---- */
  const statsRows = kvRows([
    ['样本总数', fmtInt(report.totalRequests)],
    ['成功 / 失败', `${fmtInt(report.successCount)} / ${fmtInt(report.errorCount)}`],
    ['错误率', `${report.errorRate.toFixed(3)}%`],
    ['平均 / 峰值 QPS', `${fmtInt(qps.avg)} / ${fmtInt(peakQps)} req/s`],
    ['90% 分布 QPS', qps.used > 0 ? `${fmtInt(qps.ninety)} req/s` : '—'],
    ['最低 QPS（稳态下限）', qps.used > 0 ? `${fmtInt(qps.min)} req/s` : '—'],
    ['单并发 QPS', `${fmtNum(qps.perConcurrent, 2)} req/s`],
    [
      '目标 QPS / 达成率',
      qps.target ? `${fmtInt(qps.target)} / ${fmtNum(qps.attainment, 1)}%` : '未设目标',
    ],
    ['平均响应时间', `${fmtNum(report.avgRt)} ms`],
    ['中位数 P50', `${fmtNum(report.p50Rt)} ms`],
    ['P90 / P95', `${fmtNum(report.p90Rt)} / ${fmtNum(report.p95Rt)} ms`],
    ['P99 / P99.9', `${fmtNum(report.p99Rt)} / ${fmtNum(report.p999Rt)} ms`],
    ['最大响应时间', `${fmtNum(report.maxRt)} ms`],
    ['标准差', `${fmtNum(report.stddevRt)} ms`],
    ['APDEX', report.apdex.toFixed(3)],
    ['接收带宽', `${(bytesReceived / Math.max(1, seconds) / 1024).toFixed(1)} KB/s`],
    ['发送带宽', `${(bytesSent / Math.max(1, seconds) / 1024).toFixed(1)} KB/s`],
    ...(bodyStats.avg != null ? [['Body 平均', `${fmtNum(bodyStats.avg)} ms`] as [string, string]] : []),
    ...(bodyStats.p95 != null ? [['Body P95', `${fmtNum(bodyStats.p95)} ms`] as [string, string]] : []),
    ['明细 / 错误丢弃', `${fmtInt(droppedRecords)} / ${fmtInt(droppedErrors)} 条`],
    ['总流量（入 / 出）', `${fmtBytes(bytesReceived)} / ${fmtBytes(bytesSent)}`],
  ])


  const percentileRows = kvRows([
    ['最小', `${fmtNum(report.minRt)} ms`],
    ['P50', `${fmtNum(report.p50Rt)} ms`],
    ['P75', `${fmtNum(report.p75Rt)} ms`],
    ['P90', `${fmtNum(report.p90Rt)} ms`],
    ['P95', `${fmtNum(report.p95Rt)} ms`],
    ['P99', `${fmtNum(report.p99Rt)} ms`],
    ['P99.9', `${fmtNum(report.p999Rt)} ms`],
    ['最大', `${fmtNum(report.maxRt)} ms`],
    ['TTFB 平均 / P95', `${fmtNum(report.ttfbAvg)} / ${fmtNum(report.ttfbP95)} ms`],
  ])

  const histogramMax = Math.max(1, ...detail.histogram.map((item) => item.count))
  const histogramRows = detail.histogram.map((item) => [
    `<span class="mono">${escapeHtml(item.label)}</span>`,
    meter(item.count, histogramMax),
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const bodyBuckets = detail.bodyHistogram ?? []
  const bodyHistogramMax = Math.max(1, ...bodyBuckets.map((item) => item.count))
  const bodyHistogramRows = bodyBuckets.map((item) => [
    `<span class="mono">${escapeHtml(item.label)}</span>`,
    meter(item.count, bodyHistogramMax),
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const codeMax = Math.max(1, ...detail.statusCodes.map((item: StatusCount) => item.count))
  const codeRows = detail.statusCodes.map((item) => [
    `<span class="mono">${item.code}</span>`,
    meter(item.count, codeMax, String(item.code).startsWith('2') ? 'var(--ok)' : 'var(--bad)'),
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const errorTotal = detail.errorTypes.reduce((sum, item) => sum + item.count, 0)
  const errorTypeMax = Math.max(1, ...detail.errorTypes.map((item) => item.count))
  const errorTypeRows = detail.errorTypes.map((item) => [
    escapeHtml(item.errorType),
    meter(item.count, errorTypeMax, 'var(--bad)'),
    `${((item.count / Math.max(1, errorTotal)) * 100).toFixed(1)}%`,
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const topErrors = [...detail.errorTypes]
    .sort((a, b) => b.count - a.count)
    .slice(0, 5)
    .map((item, index) => {
      const sample = detail.errors.find((error) => error.errorType === item.errorType)
      return [
        String(index + 1),
        escapeHtml(item.errorType),
        fmtInt(item.count),
        `<span class="mono break">${escapeHtml(clip(sample?.errorMessage ?? '—', 150))}</span>`,
      ]
    })

  // 分阶段只展示实测值，不再输出扩展效率等推断性指标
  const stageRows = detail.stages.map((stage) => [
    `<span class="mono">S${stage.index + 1}</span>`,
    fmtInt(stage.concurrent),
    `${stage.durationSec}s`,
    fmtInt(stage.requests),
    fmtInt(stage.errors),
    fmtNum(stage.avgQps),
    fmtNum(stage.avgRt),
    fmtNum(stage.p95Rt),
  ])
  const stageBlock = stageRows.length
    ? `<div class="sub">分阶段汇总<span class="hint">仅展示实测值</span></div>
${tableHtml(
  ['阶段', '并发', '时长', '请求数', '错误', '平均 QPS', '平均 RT', 'P95'],
  stageRows,
  [1, 2, 3, 4, 5, 6, 7],
)}`
    : ''

  // 定位模式没有全量明细，优先用报告自带的 TopN 慢请求
  const slowRows = (
    detail.slowRequests?.length
      ? [...detail.slowRequests].sort((a, b) => b.latencyMs - a.latencyMs).slice(0, 10)
      : [...requests].sort((a, b) => b.latencyMs - a.latencyMs).slice(0, 10)
  ).map((item) => {
    const ok = 'success' in item ? item.success : item.status > 0 && item.status < 400
    return [
      `<span class="mono">${(item.elapsedMs / 1000).toFixed(2)}s</span>`,
      `<span class="mono break">${escapeHtml(item.url)}</span>`,
      fmtNum(item.latencyMs, 0),
      item.status ? `<span class="mono">${item.status}</span>` : '—',
      ok ? '<span style="color:var(--ok)">成功</span>' : '<span style="color:var(--bad)">失败</span>',
    ]
  })

  /* ---- 组装 ---- */
  const cover = `<header class="cover">
  <div class="cover-top">
    <div class="brand"><span class="brand-mark">A</span><div>AstroMan<span class="brand-sub" style="display:block">Performance Report</span></div></div>
    <span class="badge plain">报告编号 R-${report.id}</span>
  </div>
  <div class="cover-kicker">Performance Test Report · 性能测试报告</div>
  <h1 class="cover-title">${escapeHtml(reportTitle(report))}</h1>
  <p class="cover-lead">共执行 <b>${fmtInt(report.totalRequests)}</b> 次请求，成功率 <b>${(100 - report.errorRate).toFixed(2)}%</b>，平均吞吐 <b>${fmtInt(qps.avg)}</b> QPS，错误率 <b>${report.errorRate.toFixed(2)}%</b>。</p>
  <span class="badge plain">${escapeHtml(statusLabel(report.status))}</span>
  <dl class="meta">
    <div><dt>被测目标</dt><dd class="mono target-url">${escapeHtml(
      (config?.requestCount ?? 0) > 1
        ? `全部接口（共 ${config?.requestCount} 个）`
        : resolvedTargetUrl(detail, requests),
    )}</dd></div>
    <div><dt>压测模式</dt><dd>${escapeHtml(modeText)}<small>${escapeHtml(
      recordModeLabel(recordMode),
    )}</small></dd></div>
    <div><dt>执行时间</dt><dd>${fmtDate(report.startTime)}<small>${fmtTime(
      report.startTime,
    )}</small></dd></div>
    <div><dt>持续时长</dt><dd>${fmtDuration(report.startTime, report.endTime)}<small>并发 ${fmtInt(
      report.concurrency,
    )}</small></dd></div>
    <div><dt>测试机数量</dt><dd>${fmtInt(machineCount)} 台<small>${escapeHtml(rampText)}</small></dd></div>
  </dl>
</header>

<nav class="toc">
  <div class="toc-h">目录 · Contents</div>
  <ol>
    <li><span class="n">01</span><a href="#s1">结果汇总</a></li>
    <li><span class="n">02</span><a href="#s2">测试环境与配置</a></li>
    <li><span class="n">03</span><a href="#s3">吞吐与并发</a></li>
    <li><span class="n">04</span><a href="#s4">响应时间分析</a></li>
    <li><span class="n">05</span><a href="#s5">错误与状态码</a></li>
    <li><span class="n">06</span><a href="#s6">URL 与施压机</a></li>
    <li><span class="n">A</span><a href="#sa">指标口径定义</a></li>
    <li><span class="n">B</span><a href="#sb">数据完整性与自洽校验</a></li>
    <li><span class="n">C</span><a href="#sc">明细数据与导出</a></li>
  </ol>
</nav>`

  const statement = `<div class="statement">
<div class="statement-t">结果汇总</div>
<div class="statement-d">共执行 <b>${fmtInt(
    report.totalRequests,
  )}</b> 次请求，平均吞吐 <b>${fmtInt(qps.avg)}</b> QPS（90% 分布 <b>${fmtInt(
    qps.ninety,
  )}</b> QPS，峰值 <b>${fmtInt(qps.max)}</b>），平均响应时间 <b>${fmtNum(
    report.avgRt,
  )}</b> ms，错误率 <b>${report.errorRate.toFixed(2)}%</b>。</div>
</div>`

  const slaSection = sla.length
    ? `<div class="sub">SLA 验收<span class="hint">压测配置中声明的验收阈值</span></div>
${
  slaPassed
    ? ''
    : '<div class="note" style="margin:-2px 0 8px;color:var(--bad)">存在未达标项，结论已按 SLA 判定为未通过。</div>'
}
${tableHtml(
        ['验收项', '目标', '实测', '余量', '结论'],
        sla.map((row) => {
          const gap = row.pass ? '—' : '超出'
          return [
            escapeHtml(row.label),
            escapeHtml(row.target),
            escapeHtml(row.actual),
            gap,
            row.pass
              ? '<span style="color:var(--ok);font-weight:600">通过</span>'
              : '<span style="color:var(--bad);font-weight:600">未通过</span>',
          ]
        }),
        [1, 2],
      )}`
    : ''

  const scene = (detail.scenarioText ?? '').trim()
  const summary = (detail.summaryText ?? '').trim()
  const peaksBlock = peaks
    ? `<div class="sub">峰值专题<span class="hint">出现时刻与平台期按每秒采样统计</span></div>
<div class="strip">
  <div><span>峰值 QPS</span><b>${fmtNum(peaks.qps.value, 0)}</b><em>第 ${peaks.qps.second} 秒 · 持续 ${peaks.qps.heldSec} 秒${
    peaks.qps.topNode ? ` · ${escapeHtml(peaks.qps.topNode)}` : ''
  }</em></div>
  <div><span>峰值并发</span><b>${fmtNum(peaks.concurrent.value, 0)}</b><em>第 ${peaks.concurrent.second} 秒 · 持续 ${peaks.concurrent.heldSec} 秒</em></div>
  <div><span>最差 P95 / P99</span><b>${fmtNum(peaks.p95.value, 0)} / ${fmtNum(peaks.p99.value, 0)} ms</b><em>峰值时刻延迟</em></div>
  <div><span>峰值带宽 / 错误率</span><b>${fmtBytes(peaks.bandwidth.value)}/s</b><em>最高错误率 ${peaks.errorRate.value.toFixed(2)}%</em></div>
</div>`
    : ''

  const sec1 = section(
    's1',
    '01',
    '结果汇总',
    'Result Summary',
    `${statement}
<div class="sub">核心指标</div>
${kpiBlock(detail, qps, peaks)}
<div class="strip">
  <div><span>目标 / 达成</span><b>${
    qps.target
      ? `${fmtInt(qps.target)} / ${fmtNum(qps.attainment, 1)}%`
      : '不限速'
  }</b></div>
  <div><span>总流量（入+出）</span><b>${fmtBytes(totalTraffic)}</b></div>
  <div><span>接收 / 发送</span><b>${fmtBytes(bytesReceived)} / ${fmtBytes(bytesSent)}</b></div>
  <div><span>明细模式</span><b>${escapeHtml(recordModeLabel(recordMode))}</b></div>
  <div><span>施压端</span><b>${escapeHtml(agent?.host ?? '未记录')}</b></div>
</div>
${peaksBlock}
${compositionBlock}
${slaSection}
${capacityBlock(detail)}
${baselineBlock(detail, options.baseline)}
${
  summary
    ? `<div class="sub">报告总结</div><div class="prose">${escapeHtml(summary).replace(/\n/g, '<br/>')}</div>`
    : ''
}`,
  )

  // 多接口报告：目标写「全部接口」，不再只展示第一个
  const targetCount = config?.requestCount ?? 0
  const multiTarget = targetCount > 1
  const targetUrl = multiTarget
    ? `全部接口（共 ${targetCount} 个）`
    : resolvedTargetUrl(detail, requests)
  const targetRows = kvRows([
    ['请求方法', multiTarget ? '多个接口' : escapeHtml(config?.method ?? '—')],
    ['请求地址', multiTarget ? escapeHtml(targetUrl) : urlCell(targetUrl)],
    ['请求头 / URL 参数', `${config?.headers ?? 0} / ${config?.params ?? 0}`],
    ['请求体类型', escapeHtml(config?.bodyType ?? '无')],
    ['超时时间', config?.timeoutMs ? `${config.timeoutMs} ms` : '—'],
    ['连接超时', config?.connectTimeoutMs ? `${config.connectTimeoutMs} ms` : '—'],
    ['跟随重定向', config?.followRedirects ? '是' : '否'],
  ])

  const methodRows = kvRows([
    ['压测模式', escapeHtml(modeText)],
    ['并发数', fmtInt(report.concurrency)],
    [
      '阶梯配置',
      config?.stages?.length
        ? config.stages.map((stage) => `${stage.concurrent}C/${stage.durationSec}s`).join(' → ')
        : '—',
    ],
    ['升压时间', rampUp ? `${rampUp} s` : '—'],
    ['思考时间', config?.thinkTimeMs ? `${config.thinkTimeMs} ms` : '无'],
    ['目标 QPS', report.targetRps ? `${fmtInt(report.targetRps)}（限速）` : '不限速'],
    ['明细采集', escapeHtml(recordModeLabel(recordMode))],
    [
      '熔断条件',
      config?.stopErrorRate || config?.stopMinRequests
        ? `错误率 ≥ ${config?.stopErrorRate ?? '—'}%（≥ ${config?.stopMinRequests ?? '—'} 请求）`
        : '未启用',
    ],
  ])

  const envRows = kvRows([
    ['施压工具', `${escapeHtml(agent?.tool ?? 'AstroMan')} ${escapeHtml(agent?.version ?? '')}`],
    ['施压主机', escapeHtml(agent?.host ?? '—')],
    ['操作系统', `${escapeHtml(agent?.os ?? '—')} ${escapeHtml(agent?.osVersion ?? '')} ${escapeHtml(agent?.arch ?? '')}`],
    ['CPU', agent?.cpuCores ? `${agent.cpuCores} 核` : '—'],
    ['内存', agent?.memTotalMb ? `${fmtInt(agent.memTotalMb)} MB` : '—'],
    [
      '压测期峰值 CPU',
      metrics?.cpuPeakCores
        ? `${metrics.cpuPeakCores.toFixed(1)} 核${
            agent?.cpuCores ? `（${((metrics.cpuPeakCores / agent.cpuCores) * 100).toFixed(0)}%）` : ''
          }`
        : '未采集',
    ],
    ['压测期峰值内存', metrics?.memPeakMb ? `${fmtInt(metrics.memPeakMb)} MB` : '未采集'],
  ])

  const poolStats = options.includePools === false ? [] : (config?.pools ?? [])
  const poolBlock = poolStats.length
    ? `<div class="sub">参数 / Token 池<span class="hint">只统计条目与使用次数，不含明文</span></div>
${tableHtml(
  ['池名', '条目', '已用', '注入请求', '401 刷新', '失效', '统计范围'],
  poolStats.map((pool) => [
    escapeHtml(pool.name),
    fmtInt(pool.items),
    fmtInt(pool.used),
    fmtInt(pool.requests),
    fmtInt(pool.refreshed),
    fmtInt(pool.invalid),
    poolScopeLabel(pool),
  ]),
  [1, 2, 3, 4, 5],
)}`
    : ''

  const sec2 = section(
    's2',
    '02',
    '测试环境与配置',
    'Environment & Configuration',
    `<div class="cols">
<div><div class="sub">被测目标</div>${tableHtml(['项目', '取值'], targetRows, [], { kv: true })}</div>
<div><div class="sub">施压端环境</div>${tableHtml(['项目', '取值'], envRows, [], { kv: true })}</div>
</div>
<div class="cols" style="margin-top:6px">
<div><div class="sub">施加方式</div>${tableHtml(['项目', '取值'], methodRows, [], { kv: true })}</div>
<div><div class="sub">测试场景说明</div>${
      scene
        ? `<div class="prose">${escapeHtml(scene).replace(/\n/g, '<br/>')}</div>`
        : '<div class="note">本次报告未填写场景说明。建议在报告页补充业务背景、数据量级与验收目标，便于阅读者理解结论边界。</div>'
    }
${
      report.targetRps
        ? `<div class="note">按 Little 定律，理论所需并发 ≈ 目标 QPS × (平均响应时间 + 思考时间) = ${fmtInt(
            report.targetRps,
          )} × (${fmtNum(report.avgRt / 1000, 3)}s + ${fmtNum(
            (config?.thinkTimeMs ?? 0) / 1000,
            3,
          )}s) ≈ ${fmtInt(
            (report.targetRps * (report.avgRt + (config?.thinkTimeMs ?? 0))) / 1000,
          )} 并发。</div>`
        : ''
    }</div>
</div>${poolBlock}`,
  )

  const sec3 = section(
    's3',
    '03',
    '吞吐与并发',
    'Throughput & Concurrency',
    `${throughput}${concurrencyTrend}${successQpsTrend}${failureQpsTrend}${bandwidth}${stageQpsBar}${stageP95Bar}${stageBlock}`,
  )

  const sec4 = section(
    's4',
    '04',
    '响应时间分析',
    'Latency Analysis',
    `${avgRtTrend}${latency}${cdf}${qpsLatency}
<div class="cols">
<div><div class="sub">分位统计</div>${tableHtml(['分位', '响应时间'], percentileRows, [1], { kv: true })}</div>
<div><div class="sub">响应时间分布<span class="hint">按区间计数</span></div>${
      histogramRows.length
        ? tableHtml(['区间', '数量', '占比'], histogramRows, [2])
        : '<div class="note">暂无直方图数据。</div>'
    }</div>
</div>
${
  bodyHistogramRows.length
    ? `<div class="sub">响应体读取耗时分布<span class="hint">body = 总耗时 − TTFB，单位 ms</span></div>
${tableHtml(['区间', '数量', '占比'], bodyHistogramRows, [2])}`
    : ''
}
${ttfb}`,
  )

  const sec5 = section(
    's5',
    '05',
    '错误与状态码',
    'Errors & Status Codes',
    `${
      report.errorCount === 0
        ? '<div class="note" style="margin-top:0">本次压测未产生失败请求，以下分布仅含成功响应。</div>'
        : ''
    }
${errorCountTrend}
${errorTrend}
<div class="cols">
<div><div class="sub">状态码分布</div>${
      codeRows.length ? tableHtml(['状态码', '数量', '占比'], codeRows, [2]) : '<div class="note">暂无数据。</div>'
    }</div>
<div><div class="sub">错误类型聚合</div>${
      errorTypeRows.length
        ? tableHtml(['错误类型', '数量', '占错误', '占样本'], errorTypeRows, [2, 3])
        : '<div class="note">无错误记录。</div>'
    }</div>
</div>
${
  topErrors.length
    ? `<div class="sub">Top 5 错误样本<span class="hint">按类型聚合，样本取自最早一条</span></div>
${tableHtml(['#', '错误类型', '次数', '代表信息'], topErrors, [0, 2])}`
    : ''
}`,
  )

  const urlStatsData = detail.urlStats ?? []
  const urlStatsComplete = detail.urlStatsComplete ?? urlStatsData.length > 0
  // HTML 只展示前 500 个 URL，Excel 输出全量，避免报告体积随 URL 基数膨胀
  const visibleUrlStats = urlStatsData.slice(0, 500)
  const urlRows = visibleUrlStats.map((row) => [
    `<span class="mono">${escapeHtml(row.method)}</span>`,
    urlCell(row.url),
    fmtInt(row.requests),
    `${fmtInt(row.successCount)} / ${fmtInt(row.errorCount)}`,
    `${(row.requests > 0 ? (row.successCount / row.requests) * 100 : 0).toFixed(2)}%`,
    `${fmtNum(row.avgMs, 0)} ms`,
    `${fmtNum(row.maxMs, 0)} ms`,
    row.maxAtMs == null ? '—' : `${(row.maxAtMs / 1000).toFixed(2)} s`,
    `${row.totalHours.toFixed(3)} h`,
    `${row.avgKb.toFixed(1)} KB`,
    `${row.totalMb.toFixed(1)} MB`,
    escapeHtml(row.statusSummary || '—'),
  ])
  const errorUrlRows = visibleUrlStats
    .filter((row) => row.errorCount > 0)
    .map((row) => [
      `<span class="mono">${escapeHtml(row.method)}</span>`,
      urlCell(row.url),
      fmtInt(row.errorCount),
      `${fmtNum(row.avgMs, 0)} ms`,
      `${fmtNum(row.maxMs, 0)} ms`,
      row.maxAtMs == null ? '—' : `${(row.maxAtMs / 1000).toFixed(2)} s`,
      `<span class="mono break">${escapeHtml(row.errorMessage || '—')}</span>`,
    ])
  const urlStatsBlock = `${urlStatsData.length
      ? `<div class="sub">按 URL 聚合统计<span class="hint">与是否记录请求明细无关，URL 中的敏感参数已脱敏</span></div>
${tableHtml(
  ['方法', 'URL', '请求数', '成功 / 失败', '成功率', '平均耗时', '最长耗时', '最长耗时时间', '总耗时', '平均包', '总流量', '状态码'],
  urlRows,
  [2, 3, 4, 5, 6, 7, 8, 9, 10],
)}${
        urlStatsData.length > visibleUrlStats.length
          ? `<div class="note">仅展示请求数最多的前 ${visibleUrlStats.length} 个 URL，完整数据请在 Excel 导出中查看。</div>`
          : ''
      }
<div class="sub">错误地址<span class="hint">只列出存在失败请求的 URL</span></div>
${
  errorUrlRows.length
    ? tableHtml(['方法', 'URL', '错误数', '平均耗时', '最长耗时', '最长耗时时间', '错误信息'], errorUrlRows, [2, 3, 4, 5])
    : '<div class="note">本次压测没有失败请求。</div>'
}`
      : `<div class="note">${
          urlStatsComplete
            ? '本次压测没有可统计的 URL 数据。'
            : '该报告生成于旧版本，未记录按 URL 统计。'
        }</div>`}${
    urlStatsData.length && !urlStatsComplete
      ? '<div class="note">URL 统计不完整：URL 键超过上限或部分从节点未上报。</div>'
      : ''
  }`

  const loadGenerators = loadGeneratorsForReport(detail)
  const loadRows = loadGenerators.map((node) => {
    const total = node.totalRequests
    const success = node.successCount
    const errors = node.errorCount
    return [
      `<span class="mono">${escapeHtml(node.name)}</span><span class="hint">${node.role === 'master' ? '主节点' : '从节点'}</span>`,
      `<span class="mono break">${escapeHtml(node.addr || '—')}</span>`,
      node.startMs ? fmtTime(node.startMs) : '—',
      total == null ? '—' : fmtInt(total),
      `${success == null ? '—' : fmtInt(success)} / ${errors == null ? '—' : fmtInt(errors)}`,
      fmtInt(node.concurrent),
      total != null && total > 0 && success != null
        ? `${((success / total) * 100).toFixed(2)}%`
        : '—',
      node.avgRtMs == null ? '—' : `${fmtNum(node.avgRtMs, 0)} ms`,
      node.tps == null ? '—' : fmtNum(node.tps, 1),
      node.egressAvgKbps == null ? '—' : `${fmtNum(node.egressAvgKbps, 1)} KB/s`,
    ]
  })
  const loadGeneratorBlock = `<div class="sub">施压机明细<span class="hint">请求数、TPS 与出站带宽按该施压机自身施压窗口统计</span></div>
${tableHtml(
  ['施压机', '地址', '开始时间', '总请求', '成功 / 失败', '并发', '成功率', '平均耗时', 'TPS', '出站带宽平均'],
  loadRows,
  [3, 4, 5, 6, 7, 8, 9],
)}`

  // URL / 施压机对比图：单 URL 或单机时没有对比意义，直接省略
  const urlCompare =
    urlStatsData.length > 1
      ? barChart(
          'URL 平均响应时间对比',
          urlStatsData.slice(0, 12).map((row) => ({
            label: `${row.method} ${row.url}`,
            value: row.avgMs,
            sub: `${fmtInt(row.requests)} 次`,
          })),
          'ms',
        )
      : ''
  const generatorCompare =
    loadGenerators.length > 1
      ? barChart(
          '施压机 TPS 对比',
          loadGenerators
            .filter((node) => node.tps != null)
            .map((node) => ({
              label: node.name,
              value: node.tps ?? 0,
              sub: `${fmtInt(node.concurrent)} 并发`,
            })),
          'TPS',
        )
      : ''

  const sec6 = section(
    's6',
    '06',
    'URL 与施压机',
    'URL & Load Generators',
    `${urlStatsBlock}${loadGeneratorBlock}${urlCompare}${generatorCompare}${clusterBlock(detail)}`,
  )

  const metricDefs: string[][] = [
    ['请求数', '成功请求数 + 失败请求数。失败包含连接错误、超时、以及状态码非 2xx 的响应。'],
    ['平均响应时间', '全部请求耗时的算术平均，含失败请求，用于反映整体体验而非仅成功路径。'],
    ['P50 / P95 / P99', '对逐请求耗时做线性插值分位数，表示「该比例请求不超过此值」。'],
    ['QPS / RPS', '同义，报告统一以 QPS 呈现。每秒完成请求数，按实际采样间隔 Δt 归一，非按固定 1 秒估算。'],
    ['平均 QPS', '总请求数 ÷ 实际施压时长（后端聚合），不取每秒采样的算术平均，避免末尾不完整秒拉低结果。'],
    ['90% 分布 QPS', '每秒采样 QPS 的 P10，含义为「90% 的时间吞吐不低于该值」；已剔除升压段与首末不完整采样。验收与横向对比优先用该值，而非峰值。'],
    ['峰值 QPS', '所有采样点中的最大每秒请求数，仅反映瞬时能力，不代表可持续承载。'],
    ['最低 QPS', '稳态窗口内的最小每秒请求数（与 90% 分布同口径，已剔除升压段与末点），用于观察抖动下限，不作为容量结论依据。'],
    ['单并发 QPS', '平均 QPS ÷ 并发数，反映单连接吞吐效率；并发翻倍而该值接近减半，说明新增并发只在排队、吞吐已到瓶颈。'],
    ['目标达成率', '平均 QPS ÷ 目标 QPS；与 SLA「最小 QPS」同一口径，超过 100% 表示超出限速目标。'],
    ['TTFB', '首字节时间，从请求发出到收到响应首字节的耗时，可区分网络/排队与服务端处理。'],
    ['APDEX', '(满意请求数 + 容忍请求数 / 2) / 总请求数，容忍阈值 T = 4 × 平均响应时间。'],
    ['错误率', '失败请求数 / 总请求数。SLA 判定以此口径为准。'],
    ['总流量', '发送字节 + 接收字节；趋势图中的带宽按每秒采样折算。'],
  ]

  const secA = section(
    'sa',
    'A',
    '指标口径定义',
    'Metric Definitions',
    `<div class="appx">${tableHtml(['指标', '定义'], metricDefs)}</div>
<div class="note">报告中所有结论均可由上述口径复算。若与其他工具（JMeter / wrk / k6）对比，请先确认分位数插值方式、失败请求是否计入平均响应时间、以及升压段是否剔除，三项口径不一致会带来 10%~30% 的数值差异。</div>`,
  )

  const dropNotes: string[] = []
  if (droppedRecords > 0) dropNotes.push(`请求明细达到存储上限后被丢弃 ${fmtInt(droppedRecords)} 条`)
  if (droppedErrors > 0) dropNotes.push(`错误明细被丢弃 ${fmtInt(droppedErrors)} 条`)
  if (recordMode === 'off') dropNotes.push('本次采用高吞吐模式，未记录逐请求明细')
  if (recordMode === 'sample') dropNotes.push('明细按 1% 采样记录，慢请求排名不代表全集')
  if (metrics?.sampleIntervalSec && metrics.sampleIntervalSec > 1) {
    dropNotes.push(`长压测期间采样间隔已放宽至 ${metrics.sampleIntervalSec}s，趋势图颗粒度相应变粗`)
  }
  if (samples.length > CHART_MAX_POINTS) {
    dropNotes.push(`趋势图由 ${samples.length} 个采样点等距抽稀至 ${CHART_MAX_POINTS} 点绘制，聚合指标不受影响`)
  }

  const secB = section(
    'sb',
    'B',
    '数据完整性与自洽校验',
    'Data Integrity',
    `<div class="sub">指标总览<span class="hint">全部聚合指标一览</span></div>
${tableHtml(['指标', '数值'], statsRows, [1], { kv: true })}
<div class="sub">自洽校验<span class="hint">三处独立口径互相勾稽，任一处偏差即说明采集异常</span></div>
${
  checksPassed
    ? ''
    : '<div class="note" style="margin:-2px 0 8px;color:var(--warn)">存在偏差项，相关指标建议视为参考值。</div>'
}
${tableHtml(
      ['校验项', '结论', '实测依据'],
      checks.map((item) => [
        escapeHtml(item.label),
        item.pass
          ? '<span style="color:var(--ok);font-weight:600">通过</span>'
          : '<span style="color:var(--bad);font-weight:600">偏差</span>',
        `<span class="mono">${escapeHtml(item.detail)}</span>`,
      ]),
    )}
<div class="sub">数据说明</div>
${
  dropNotes.length
    ? `<ul class="note-list">${dropNotes.map((note) => `<li>${escapeHtml(note)}</li>`).join('')}</ul>`
    : '<div class="note">聚合指标与逐请求明细完整，未发生丢弃或降采样。</div>'
}`,
  )

  const secC = section(
    'sc',
    'C',
    '明细数据与导出',
    'Raw Data',
    `<div class="prose"><p>逐请求明细（URL、状态码、耗时、TTFB、请求头、响应头、响应体）、错误明细以及每秒采样序列已完整入库。为避免报告体积随压测时长线性膨胀，HTML 报告不再内嵌明细表 —— 8 小时压测内嵌明细会使文件从数百 KB 增长到数十 MB 并导致浏览器卡顿。</p>
<p>需要原始数据时，请在 AstroMan 报告页使用「导出明细 CSV / 错误 CSV / Excel / JSON」，导出为服务端流式直写，不受文件体积限制。</p></div>
${
  slowRows.length
    ? `<div class="sub">Top 10 慢请求<span class="hint">取自已记录明细${
        recordMode === 'off' ? '（本次未记录）' : `（共 ${fmtInt(report.totalRequests)} 条请求）`
      }</span></div>
${tableHtml(['时间点', 'URL', '耗时(ms)', '状态码', '结果'], slowRows, [2, 3])}`
    : ''
}
<div class="sub">脱敏说明</div>
<div class="prose"><p>URL 中的 token / password / secret / authorization / cookie 等参数值已掩码；错误信息中的 Bearer、password、token、sk- 等凭证已掩码。</p></div>
<div class="sub">免责声明</div>
<div class="prose"><p>${escapeHtml(REPORT_DISCLAIMER)}</p></div>`,
  )

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escapeHtml(reportTitle(report))} · 性能测试报告 R-${report.id}</title>
<style>${STYLE}</style>
</head>
<body>
<div class="sheet">
${cover}
${sec1}
${sec2}
${sec3}
${sec4}
${sec5}
${sec6}
${secA}
${secB}
${secC}
<footer class="foot">
  <div class="foot-row">
    <span>AstroMan ${escapeHtml(agent?.version ?? '')} · 报告生成于 ${fmtTime(Date.now())}</span>
    <span>R-${report.id} · ${escapeHtml(statusLabel(report.status))} · ${escapeHtml(
      recordModeLabel(recordMode),
    )}</span>
  </div>
  <div style="margin-top:6px"><b>免责声明：</b>${escapeHtml(REPORT_DISCLAIMER)}</div>
</footer>
</div>
<script>
document.addEventListener('click', function (event) {
  var btn = event.target && event.target.closest ? event.target.closest('[data-copy]') : null;
  if (!btn) return;
  var text = btn.getAttribute('data-copy') || '';
  var reset = function (label) {
    btn.textContent = label;
    setTimeout(function () { btn.textContent = '复制'; }, 1500);
  };
  var fallback = function () {
    var cell = btn.parentElement ? btn.parentElement.querySelector('.mono') : null;
    if (!cell) { reset('复制失败'); return; }
    var range = document.createRange();
    range.selectNodeContents(cell);
    var selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    var ok = false;
    try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
    reset(ok ? '已复制' : '已选中');
  };
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(function () { reset('已复制'); }, fallback);
  } else {
    fallback();
  }
});
</script>
</body>
</html>`
}

/** 生成自包含 HTML 压测报告 */
export function buildReportHtml(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
  options: { includePools?: boolean; baseline?: ReportDetail | null } = {},
): string {
  return renderHtml(detail, requests, options)
}

/** 池统计范围文案：集群汇总 / 仅主节点 / 单机本机 */
function poolScopeLabel(pool: PoolReportStat): string {
  if (pool.nodeScope === 'cluster') {
    return pool.nodes?.length ? `全部节点（${pool.nodes.length}）` : '全部节点'
  }
  return pool.nodeScope === 'master' ? '仅主节点' : '本机'
}

/** Excel「池统计」工作表：只含计数与序号，不含任何明文 */
function poolSheet(detail: ReportDetail): CellValue[][] {
  const pools = detail.config?.pools ?? []
  if (!pools.length) return [['说明', '该报告未记录参数 / Token 池统计']]
  const rows: CellValue[][] = [
    ['池名', '条目', '已用', '注入请求', '401 刷新', '失效', '范围'],
    ...pools.map((pool) => [
      pool.name,
      pool.items,
      pool.used,
      pool.requests,
      pool.refreshed,
      pool.invalid,
      poolScopeLabel(pool),
    ]),
  ]
  const perNodePools = pools.filter((pool) => pool.byNode?.length)
  if (perNodePools.length) {
    rows.push([], ['节点用量明细', '', '', '', '', '', ''])
    rows.push(['池名', '节点', '已用', '注入请求', '401 刷新'])
    perNodePools.forEach((pool) => {
      pool.byNode?.forEach((node) =>
        rows.push([pool.name, node.name, node.used, node.requests, node.refreshed]),
      )
    })
  }
  const perItemPools = pools.filter((pool) => pool.perItem?.length)
  if (perItemPools.length) {
    rows.push([], ['条目使用明细（按请求数取 Top 200）', '', ''])
    rows.push(['池名', '条目序号', '请求次数'])
    perItemPools.forEach((pool) => {
      pool.perItem?.forEach((item) => rows.push([pool.name, item.index, item.requests]))
    })
  }
  return rows
}

/* ---------- Excel 工作簿 ---------- */

function percentileSheet(detail: ReportDetail): CellValue[][] {
  const report = detail.report
  const body = bodyLatencyStats(detail)
  return [
    ['指标', '数值', '单位'],
    ['最小响应时间', report.minRt, 'ms'],
    ['平均响应时间', report.avgRt, 'ms'],
    ['P50', report.p50Rt, 'ms'],
    ['P75', report.p75Rt, 'ms'],
    ['P90', report.p90Rt, 'ms'],
    ['P95', report.p95Rt, 'ms'],
    ['P99', report.p99Rt, 'ms'],
    ['P99.9', report.p999Rt, 'ms'],
    ['最大响应时间', report.maxRt, 'ms'],
    ['标准差', report.stddevRt, 'ms'],
    ['TTFB 平均', report.ttfbAvg, 'ms'],
    ['TTFB P95', report.ttfbP95, 'ms'],
    ...(body.avg != null ? [['Body 平均', Number(body.avg.toFixed(1)), 'ms'] as CellValue[]] : []),
    ...(body.p95 != null ? [['Body P95', Number(body.p95.toFixed(1)), 'ms'] as CellValue[]] : []),
  ]
}

/** 生成 Excel 多工作表数据 */
export function buildReportSheets(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
  options: {
    includePools?: boolean
    baseline?: ReportDetail | null
    includeRequestRows?: boolean
    includeSampleRows?: boolean
  } = {},
): XlsxSheet[] {
  const report = detail.report
  const config = detail.config
  const metrics = config?.metrics
  const agent = config?.agent
  const capacity = config?.machineCapacity
  const sla = slaRows(detail)
  const qps = qpsOverview(detail, { rampUpSec: detail.config?.rampUpSec ?? 0 })
  const peaks = reportPeaks(detail)
  const peakQps = qps.max || peaks?.qps.value || 0
  const body = bodyLatencyStats(detail)
  const receivedBytes = report.bytesTotal ?? 0
  const sentBytes = metrics?.bytesSent ?? 0

  const overview: CellValue[][] = [
    ['压测报告', reportTitle(report)],
    ['报告编号', `R-${report.id}`],
    ['执行状态', statusLabel(report.status)],
    ['开始时间', fmtTime(report.startTime)],
    ['结束时间', fmtTime(report.endTime)],
    ['持续时长', fmtDuration(report.startTime, report.endTime)],
    [],
    ['核心指标', '数值'],
    ['总请求数', report.totalRequests],
    ['成功请求数', report.successCount],
    ['失败请求数', report.errorCount],
    ['错误率(%)', report.errorRate],
    ['成功率(%)', Number((100 - report.errorRate).toFixed(4))],
    ['平均 QPS', qps.avg],
    ['峰值 QPS', peakQps],
    ['峰值 QPS 出现秒', peaks ? peaks.qps.second : '—'],
    ['峰值 QPS 平台期(秒)', peaks ? peaks.qps.heldSec : '—'],
    ['90% 分布 QPS', Number(qps.ninety.toFixed(1))],
    ['最低 QPS', qps.used > 0 ? Number(qps.min.toFixed(1)) : '—'],
    ['单并发 QPS', Number(qps.perConcurrent.toFixed(4))],
    ['目标 QPS', qps.target || '不限速'],
    ['目标达成率(%)', qps.attainment === null ? '未设目标' : Number(qps.attainment.toFixed(2))],
    ['并发数', report.concurrency],
    ['平均响应时间(ms)', report.avgRt],
    ['最小响应时间(ms)', report.minRt],
    ['最大响应时间(ms)', report.maxRt],
    ['P50(ms)', report.p50Rt],
    ['P95(ms)', report.p95Rt],
    ['P99(ms)', report.p99Rt],
    ['APDEX', report.apdex],
    ...(body.avg != null ? [['Body 平均(ms)', Number(body.avg.toFixed(1))] as CellValue[]] : []),
    ...(body.p95 != null ? [['Body P95(ms)', Number(body.p95.toFixed(1))] as CellValue[]] : []),
    ['接收流量(B)', receivedBytes],
    ['发送流量(B)', sentBytes],
    ['总流量(B)', receivedBytes + sentBytes],
    ['明细丢弃(条)', metrics?.recordsDropped ?? 0],
    ['错误丢弃(条)', metrics?.errorsDropped ?? 0],
    [],
  ]

  // 峰值专题：与报告页、AI 共用 reportPeaks，集群取 cluster.peaks，单机取 metricsPeaks
  if (peaks) {
    overview.push(
      ['峰值专题', '数值', '出现秒', '平台期(秒)', '来源节点'],
      ['秒级峰值 QPS', peaks.qps.value, peaks.qps.second, peaks.qps.heldSec, peaks.qps.topNode || '—'],
      ['峰值并发', peaks.concurrent.value, peaks.concurrent.second, peaks.concurrent.heldSec, '—'],
      ['峰值带宽(B/s)', peaks.bandwidth.value, peaks.bandwidth.second, peaks.bandwidth.heldSec, '—'],
      ['最差 P95(ms)', peaks.p95.value, peaks.p95.second, peaks.p95.heldSec, '—'],
      ['最差 P99(ms)', peaks.p99.value, peaks.p99.second, peaks.p99.heldSec, '—'],
      ['最高错误率(%)', peaks.errorRate.value, peaks.errorRate.second, peaks.errorRate.heldSec, '—'],
      [],
    )
  }

  if (sla.length) {
    overview.push(['SLA 验收', '目标', '实测', '结论'])
    sla.forEach((row) => overview.push([row.label, row.target, row.actual, row.pass ? '通过' : '未通过']))
  }

  const samples: CellValue[][] = [
    ['时间(s)', '并发', 'QPS', '平均 RT(ms)', 'P50(ms)', 'P95(ms)', 'P99(ms)', 'TTFB(ms)', 'Body 平均(ms)', 'Body P95(ms)', '流量(B)', '成功数', '失败数'],
    ...detail.samples.map((sample: ReportSample) => [
      sample.elapsedSec,
      sample.concurrent,
      sample.qps,
      sample.avgRt,
      sample.p50Rt,
      sample.p95Rt,
      sample.p99Rt,
      sample.ttfbAvg,
      sample.bodyAvg,
      sample.bodyP95,
      sample.bytes,
      sample.successCount,
      sample.errorCount,
    ]),
  ]

  const requestSheet: CellValue[][] = [
    [
      '时间(s)',
      '请求时间',
      '方法',
      'URL',
      '请求体',
      '查询参数',
      '请求头',
      '响应头',
      '响应体',
      '状态码',
      '耗时(ms)',
      'TTFB(ms)',
      '响应大小(B)',
      '结果',
      '错误类型',
      '错误信息',
    ],
    ...requests.map((item) => [
      Number((item.elapsedMs / 1000).toFixed(3)),
      fmtTime(item.timeMs),
      item.method,
      item.url,
      clip(item.body ?? '', 1000),
      clip(item.params ?? '', 1000),
      clip(pairsText(item.headers), 1500),
      clip(pairsText(item.responseHeaders), 1500),
      clip(item.responseBody ?? '', 2000),
      item.status || '',
      item.latencyMs,
      item.ttfbMs,
      item.bytes,
      item.success ? '成功' : '失败',
      item.errorType ?? '',
      clip(item.errorMessage ?? '', 500),
    ]),
  ]

  const statusCodes: CellValue[][] = [
    ['状态码', '次数', '占比(%)'],
    ...detail.statusCodes.map((item: StatusCount) => [
      item.code,
      item.count,
      Number(((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(4)),
    ]),
  ]

  const errorTypes: CellValue[][] = [
    ['错误类型', '次数', '占比(%)'],
    ...detail.errorTypes.map((item) => [
      item.errorType,
      item.count,
      Number(((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(4)),
    ]),
  ]

  const errors: CellValue[][] = [
    ['时间(s)', '错误类型', '状态码', '错误信息', '请求地址', '响应内容'],
    ...detail.errors.map((error: ReportError) => [
      error.elapsedSec,
      error.errorType,
      error.responseStatus ?? '',
      clip(error.errorMessage, 500),
      clip(error.requestUrl, 500),
      clip(error.responseBody, 500),
    ]),
  ]

  const urlStatsSheet: CellValue[][] = detail.urlStats?.length
    ? [
        [
          '方法',
          'URL',
          '状态码',
          '请求数',
          '成功数',
          '失败数',
          '成功率(%)',
          '平均耗时(ms)',
          '最长耗时(ms)',
          '最长耗时时间(s)',
          '总耗时(h)',
          '平均包(KB)',
          '总流量(MB)',
          '状态码汇总',
          '错误信息',
        ],
        ...detail.urlStats.map((row) => [
          row.method,
          row.url,
          row.statusCode || '—',
          row.requests,
          row.successCount,
          row.errorCount,
          Number((row.requests > 0 ? (row.successCount / row.requests) * 100 : 0).toFixed(2)),
          Number(row.avgMs.toFixed(1)),
          Number(row.maxMs.toFixed(1)),
          row.maxAtMs == null ? '—' : Number((row.maxAtMs / 1000).toFixed(3)),
          Number(row.totalHours.toFixed(4)),
          Number(row.avgKb.toFixed(2)),
          Number(row.totalMb.toFixed(3)),
          row.statusSummary || '—',
          clip(row.errorMessage ?? '', 500),
        ]),
      ]
    : [
        [
          '说明',
          (detail.urlStatsComplete ?? false)
            ? '本次压测没有可统计的 URL 数据'
            : '该报告生成于旧版本，未记录按 URL 统计',
        ],
      ]

  const errorUrlSheet: CellValue[][] = (() => {
    const rows = (detail.urlStats ?? []).filter((row) => row.errorCount > 0)
    if (!rows.length) {
      return [['说明', detail.urlStats?.length ? '本次压测没有失败请求' : '未记录错误地址统计']]
    }
    return [
      ['方法', 'URL', '错误数', '平均耗时(ms)', '最长耗时(ms)', '最长耗时时间(s)', '错误信息'],
      ...rows.map((row) => [
        row.method,
        row.url,
        row.errorCount,
        Number(row.avgMs.toFixed(1)),
        Number(row.maxMs.toFixed(1)),
        row.maxAtMs == null ? '—' : Number((row.maxAtMs / 1000).toFixed(3)),
        clip(row.errorMessage ?? '', 500),
      ]),
    ]
  })()

  const histogram: CellValue[][] = [
    ['区间', '最小值(ms)', '最大值(ms)', '数量', '占比(%)'],
    ...detail.histogram.map((item) => [
      item.label,
      item.minMs,
      item.maxMs,
      item.count,
      Number(((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(4)),
    ]),
  ]

  const stages: CellValue[][] = [
    ['阶段', '并发', '时长(s)', '请求数', '错误数', '平均 QPS', '平均 RT(ms)', 'P95(ms)'],
    ...detail.stages.map((stage) => [
      `第 ${stage.index + 1} 阶段`,
      stage.concurrent,
      stage.durationSec,
      stage.requests,
      stage.errors,
      stage.avgQps,
      stage.avgRt,
      stage.p95Rt,
    ]),
  ]

  const snapshot: CellValue[][] = config
    ? [
        ['配置项', '取值'],
        ['压测模式', modeLabelText(config.mode, config.stages?.length ?? 0)],
        ['请求方法', config.method ?? '-'],
        ['请求地址', resolvedTargetUrl(detail, requests)],
        ['请求头数量', config.headers ?? 0],
        ['参数数量', config.params ?? 0],
        ['请求体类型', config.bodyType ?? '-'],
        ['环境 ID', config.envId ?? '-'],
        ['环境名称', config.envName ?? '-'],
        ['测试机数量', config.cluster?.nodes?.length ?? 1],
        ['超时时间(ms)', config.timeoutMs ?? '-'],
        ['连接超时(ms)', config.connectTimeoutMs ?? '-'],
        ['跟随重定向', config.followRedirects ? '是' : '否'],
        ['思考时间(ms)', config.thinkTimeMs ?? '-'],
        ['加压时间(s)', config.rampUpSec ?? '-'],
        ['目标 QPS', config.targetRps || '不限速'],
        ['明细记录模式', recordModeLabel(config.recordDetail)],
        ['采集预设', config.captureMode ?? '-'],
        ['TopN 慢请求', config.topN ?? '-'],
        ['慢请求阈值(ms)', config.slowThresholdMs ?? '-'],
        ['采样间隔(s)', metrics?.sampleIntervalSec ?? '-'],
        [
          '阶梯配置',
          config.stages?.length
            ? config.stages.map((stage) => `${stage.concurrent} 并发 / ${stage.durationSec}s`).join('，')
            : '-',
        ],
        ['SLA P95(ms)', config.sla?.p95Ms ?? '-'],
        ['SLA P99(ms)', config.sla?.p99Ms ?? '-'],
        ['SLA 错误率(%)', config.sla?.errorRate ?? '-'],
        ['SLA 最小 QPS', config.sla?.minRps ?? '-'],
        ['基线报告 ID', config.baselineReportId ?? '-'],
        ['同主机施压告警', config.sameHostWarning ?? '-'],
        ['施压工具', `${agent?.tool ?? 'AstroMan'} ${agent?.version ?? ''}`.trim()],
        ['施压端主机', agent?.host ?? '-'],
        ['操作系统', `${agent?.os ?? '-'} ${agent?.osVersion ?? ''}`.trim()],
        ['CPU 核数', agent?.cpuCores ?? '-'],
        ['内存(MB)', agent?.memTotalMb ?? '-'],
        ['压测期峰值 CPU(核)', metrics?.cpuPeakCores ?? '-'],
        ['压测期峰值内存(MB)', metrics?.memPeakMb ?? '-'],
        ['客户端极限 QPS', capacity?.maxQps ?? '-'],
        ['客户端负载占比(%)', capacity?.maxQps ? Number(((report.avgQps / capacity.maxQps) * 100).toFixed(1)) : '-'],
        ['饱和并发', capacity?.saturationConcurrent ?? '-'],
        ['明细丢弃(条)', metrics?.recordsDropped ?? 0],
        ['错误丢弃(条)', metrics?.errorsDropped ?? 0],
        ['报告生成时间', fmtTime(Date.now())],
      ]
    : [['配置项', '取值'], ['说明', '该报告未保存配置快照']]

  const clusterMeta = (config as unknown as { cluster?: ClusterReportMeta } | null)?.cluster
  const clusterTotalQps = (clusterMeta?.nodes ?? []).reduce((sum, node) => sum + (node.qpsAvg ?? 0), 0)
  // 集群概览放在峰值页顶部，避免与「概览」工作表重复
  const clusterHead: CellValue[][] = clusterMeta
    ? [
        ['分布式压测', ''],
        ['协议版本', `v${clusterMeta.protocolVersion}`],
        ['主节点', clusterMeta.masterNode || '—'],
        ['节点数', clusterMeta.nodes.length],
        ['集群总 QPS', Number(clusterTotalQps.toFixed(1))],
        ['总并发', clusterMeta.totalConcurrent],
        ['起跑时间', fmtTime(clusterMeta.startedAt)],
        ['结束时间', fmtTime(clusterMeta.finishedAt)],
        ['丢弃采样', clusterMeta.droppedSamples],
        ['结果完整', clusterMeta.incomplete ? '不完整' : '完整'],
        ['不完整原因', clusterMeta.note || '-'],
        [],
      ]
    : []

  // 峰值专题单独一张表
  const clusterPeakSheet: CellValue[][] = clusterMeta
    ? [
        ...clusterHead,
        ...(clusterMeta.peaks
          ? [
              ['峰值专题', '数值', '出现秒', '持续秒', '节点'],
              ['峰值 QPS', clusterMeta.peaks.qps.value, clusterMeta.peaks.qps.second, clusterMeta.peaks.qps.heldSec, clusterMeta.peaks.qps.topNode || '—'],
              ['峰值并发', clusterMeta.peaks.concurrent.value, clusterMeta.peaks.concurrent.second, clusterMeta.peaks.concurrent.heldSec, '—'],
              ['最差 P95(ms)', clusterMeta.peaks.p95.value, clusterMeta.peaks.p95.second, clusterMeta.peaks.p95.heldSec, '—'],
              ['最差 P99(ms)', clusterMeta.peaks.p99.value, clusterMeta.peaks.p99.second, clusterMeta.peaks.p99.heldSec, '—'],
              ['峰值带宽(B/s)', clusterMeta.peaks.bandwidth.value, clusterMeta.peaks.bandwidth.second, clusterMeta.peaks.bandwidth.heldSec, '—'],
              ['最高错误率(%)', clusterMeta.peaks.errorRate.value, clusterMeta.peaks.errorRate.second, clusterMeta.peaks.errorRate.heldSec, '—'],
            ]
          : [['峰值专题', '本次报告未记录峰值']]),
      ]
    : []

  // 节点贡献单独一张表
  const clusterNodeSheet: CellValue[][] = clusterMeta
    ? [
        [
          '节点',
          '角色',
          '地址',
          '权重',
          '分配并发',
          '状态',
          '平均 QPS',
          '峰值 QPS',
          '峰值并发',
          '贡献占比(%)',
          '采集健康',
          '进程核数',
          '整机 CPU 峰值(%)',
          '整机内存峰值(%)',
          '进程内存峰值(MB)',
          '网络峰值(KB/s)',
          '时钟偏移(ms)',
          '时钟漂移(ms)',
          '丢弃采样',
          '施压开始时间',
          '施压时长(s)',
        ],
        ...clusterMeta.nodes.map((node) => [
          node.name,
          node.role === 'master' ? '主节点' : '从节点',
          node.addr,
          node.weight ?? 1,
          node.concurrent,
          node.status,
          node.qpsAvg,
          node.peakQps,
          node.peakConcurrent,
          Number(((node.share ?? 0) * 100).toFixed(2)),
          node.health?.state === 'ok' || node.health?.state === 'done'
            ? '正常'
            : node.health?.message || node.health?.state || '—',
          node.cpuPeak ? Number(node.cpuPeak.toFixed(2)) : null,
          node.hostCpuPeak ? Number(node.hostCpuPeak.toFixed(1)) : null,
          node.hostMemPeakPercent ? Number(node.hostMemPeakPercent.toFixed(1)) : null,
          node.memPeakMb || null,
          node.netPeakKbps ? Number(node.netPeakKbps.toFixed(1)) : null,
          Number((node.clockOffsetMs ?? 0).toFixed(1)),
          node.clockDriftMs ? Number(node.clockDriftMs.toFixed(1)) : null,
          node.droppedSamples ?? 0,
          fmtTime(node.startMs),
          node.startMs && node.endMs ? Number(((node.endMs - node.startMs) / 1000).toFixed(1)) : null,
        ]),
      ]
    : []

  // 施压机明细：单机 1 行、集群 N 行；缺失字段输出「—」，不推断
  const loadGenerators = loadGeneratorsForReport(detail)
  const loadGeneratorSheet: CellValue[][] = loadGenerators.length
    ? [
        [
          '施压机',
          '角色',
          '地址',
          '状态',
          '开始时间',
          '结束时间',
          '施压时长(s)',
          '总请求',
          '成功数',
          '失败数',
          '并发',
          '成功率(%)',
          '平均耗时(ms)',
          'TPS',
          '出站带宽平均(KB/s)',
        ],
        ...loadGenerators.map((node) => {
          const total = node.totalRequests
          const success = node.successCount
          const errors = node.errorCount
          return [
            node.name,
            node.role === 'master' ? '主节点' : '从节点',
            node.addr || '—',
            node.status,
            node.startMs ? fmtTime(node.startMs) : '—',
            node.endMs ? fmtTime(node.endMs) : '—',
            node.startMs && node.endMs
              ? Number(((node.endMs - node.startMs) / 1000).toFixed(1))
              : '—',
            total ?? '—',
            success ?? '—',
            errors ?? '—',
            node.concurrent,
            total != null && total > 0 && success != null
              ? Number(((success / total) * 100).toFixed(2))
              : '—',
            node.avgRtMs != null ? Number(node.avgRtMs.toFixed(1)) : '—',
            node.tps != null ? Number(node.tps.toFixed(1)) : '—',
            node.egressAvgKbps != null ? Number(node.egressAvgKbps.toFixed(1)) : '—',
          ]
        }),
      ]
    : [['说明', '该报告未记录施压机明细']]

  // 节点秒级曲线单独一张表：按秒对齐（取各节点秒并集），缺采样留空，
  // 避免节点晚加入或采样错位时按下标对齐画出错误曲线。
  const clusterSeriesSheet: CellValue[][] = []
  if (clusterMeta?.nodeSeries?.length) {
    const series = clusterMeta.nodeSeries.filter((item) => item.points?.length)
    if (series.length) {
      const seconds = Array.from(
        new Set(series.flatMap((item) => item.points.map((point) => point.sec))),
      ).sort((a, b) => a - b)
      const bySec = series.map(
        (item) => new Map(item.points.map((point) => [point.sec, point])),
      )
      clusterSeriesSheet.push(
        ['节点秒级采样', ...series.map((item) => item.node)],
        [
          '秒',
          ...series.map((item) => `${item.node} QPS`),
          ...series.map((item) => `${item.node} 并发`),
          ...series.map((item) => `${item.node} P95(ms)`),
          ...series.map((item) => `${item.node} 整机CPU%`),
          ...series.map((item) => `${item.node} 进程内存MB`),
          ...series.map((item) => `${item.node} 网络KB/s`),
        ],
      )
      for (const sec of seconds) {
        const points = bySec.map((map) => map.get(sec))
        clusterSeriesSheet.push([
          sec,
          ...points.map((point) => point?.qps ?? null),
          ...points.map((point) => point?.concurrent ?? null),
          ...points.map((point) => (point?.p95 != null ? Number(point.p95.toFixed(1)) : null)),
          ...points.map((point) => point?.hostCpuPercent ?? null),
          ...points.map((point) => point?.memMb ?? null),
          ...points.map((point) =>
            point ? Number(((point.netRxKbps ?? 0) + (point.netTxKbps ?? 0)).toFixed(1)) : null,
          ),
        ])
      }
    }
  }

  const bodyBucketsAll = detail.bodyHistogram ?? []
  const bodyHistogramSheet: CellValue[][] = bodyBucketsAll.length
    ? [
        ['区间', '最小值(ms)', '最大值(ms)', '数量', '占比(%)'],
        ...bodyBucketsAll.map((item) => [
          item.label,
          item.minMs,
          item.maxMs,
          item.count,
          Number(((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(4)),
        ]),
      ]
    : []

  // 说明页：口径、完整性、脱敏与免责声明，与 HTML/页面保持一致
  const notesSheet: CellValue[][] = [
    ['说明', '内容'],
    [
      '指标口径',
      '平均 QPS = 总请求数 ÷ 实际施压时长；P50 / P95 / P99 为逐请求耗时线性插值分位数。',
    ],
    [
      '吞吐口径',
      '峰值 QPS 取每秒采样最大值，仅反映瞬时能力；90% 分布 QPS 为每秒采样 QPS 的 P10。',
    ],
    [
      '施压机口径',
      '客户端平均耗时 = 节点 sum_rt / 请求数；TPS = 总请求 / 施压时长；出站带宽平均 = 发送字节 / 施压时长 / 1024。',
    ],
    [
      'URL 口径',
      'URL 统计按方法 + 地址 + 状态码聚合，与是否记录请求明细无关；平均耗时与包大小按该 URL 请求数计算。',
    ],
    [
      '数据完整性',
      `明细模式：${recordModeLabel(metrics?.recordMode ?? config?.recordDetail ?? 'full')}；请求明细丢弃 ${fmtInt(
        metrics?.recordsDropped ?? 0,
      )} 条；错误明细丢弃 ${fmtInt(metrics?.errorsDropped ?? 0)} 条；URL 统计${
        detail.urlStats?.length
          ? detail.urlStatsComplete
            ? '完整'
            : '不完整（键溢出或节点未上报）'
          : '未记录'
      }。`,
    ],
    [
      '脱敏说明',
      'URL 中的 token / password / secret / authorization / cookie 等参数值已掩码；错误信息中的 Bearer、password、token、sk- 等凭证已掩码。',
    ],
    ['免责声明', REPORT_DISCLAIMER],
  ]

  const baseline = options.baseline ?? null
  const baselineSheet: CellValue[][] =
    baseline && baseline.report.id !== report.id
      ? [
          ['基线报告', `R-${baseline.report.id} · ${reportTitle(baseline.report)}`],
          ['基线执行时间', fmtTime(baseline.report.startTime)],
          [],
          ['指标', '本次', '基线', '变化', '结论'],
          ...baselineCompareRows(detail, baseline).map((row) => [
            row.label,
            row.current,
            row.base,
            row.delta,
            row.tone === 'ok' ? '正常' : row.tone === 'warn' ? '关注' : row.tone === 'bad' ? '风险' : '—',
          ]),
        ]
      : []

  const sheets: XlsxSheet[] = [
    { name: '说明', rows: notesSheet },
    { name: '结果汇总', rows: overview },
    { name: '配置快照', rows: snapshot },
    { name: '延迟分位', rows: percentileSheet(detail) },
    { name: '采样明细', rows: options.includeSampleRows === false ? [] : samples },
    // 后端生成 Excel 时这里留空，由 Rust 直接查库流式写入请求明细
    { name: '请求明细', rows: options.includeRequestRows === false ? [] : requestSheet },
    { name: '响应时间分布', rows: histogram },
    { name: '状态码分布', rows: statusCodes },
    { name: '错误类型', rows: errorTypes },
    { name: '错误明细', rows: errors },
    { name: 'URL 统计', rows: urlStatsSheet },
    { name: '错误地址', rows: errorUrlSheet },
    { name: '施压机详情', rows: loadGeneratorSheet },
    { name: '分阶段汇总', rows: stages },
  ]
  if (baselineSheet.length) {
    sheets.splice(1, 0, { name: '基线对比', rows: baselineSheet })
  }
  if (bodyHistogramSheet.length) {
    sheets.push({ name: '响应体耗时分布', rows: bodyHistogramSheet })
  }
  if (clusterPeakSheet.length) {
    sheets.push({ name: '集群峰值', rows: clusterPeakSheet })
  }
  if (clusterNodeSheet.length) {
    sheets.push({ name: '集群节点', rows: clusterNodeSheet })
  }
  if (clusterSeriesSheet.length) {
    sheets.push({ name: '节点采样', rows: clusterSeriesSheet })
  }
  if (options.includePools !== false && (detail.config?.pools?.length ?? 0) > 0) {
    sheets.push({ name: '池统计', rows: poolSheet(detail) })
  }
  return sheets
}
