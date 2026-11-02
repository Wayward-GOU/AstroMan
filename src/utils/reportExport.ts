import type {
  ReportDetail,
  ReportError,
  ReportRequestRecord,
  ReportSample,
  StatusCount,
} from '@/types/models'
import type { CellValue, XlsxSheet } from './xlsx'
import { detectKnee, downsample, qpsOverview, scoreReport, selfCheck, type QpsOverview } from './reportAnalytics'
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
  if (hour) return `${hour} 小时 ${minute} 分`
  return minute ? `${minute} 分 ${second} 秒` : `${second} 秒`
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

/** 供 Excel 概览页使用的单行结论 */
function verdict(detail: ReportDetail): { title: string; desc: string; tone: string } {
  const grade = gradeReport(detail)
  const tone = grade.grade === 'pass' ? 'ok' : grade.grade === 'conditional' ? 'warn' : 'bad'
  return { title: grade.label, desc: grade.reason, tone }
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
  padding: 40px 24px 64px;
  background: var(--shell);
  color: var(--ink);
  font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC",
    "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
  font-size: 12.5px;
  line-height: 1.62;
  -webkit-font-smoothing: antialiased;
}
.sheet {
  max-width: 880px;
  margin: 0 auto;
  background: var(--paper);
  padding: 44px 52px 40px;
  box-shadow: 0 1px 2px rgba(16, 26, 30, .06), 0 12px 32px rgba(16, 26, 30, .07);
}

/* ---- 封面 ---- */
.cover { padding-bottom: 26px; border-bottom: 2px solid var(--ink); }
.cover-top { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding-bottom: 22px; }
.brand { display: flex; align-items: center; gap: 9px; font-size: 12px; font-weight: 700; letter-spacing: .04em; }
.brand-mark {
  width: 22px; height: 22px; border-radius: 4px; background: var(--accent);
  color: #fff; font-size: 12px; font-weight: 800; display: flex; align-items: center;
  justify-content: center; letter-spacing: 0;
}
.brand-sub { font-size: 10px; letter-spacing: .18em; text-transform: uppercase; color: var(--ink-4); font-weight: 600; }
.cover-kicker { font-size: 10px; letter-spacing: .2em; text-transform: uppercase; color: var(--accent); font-weight: 700; }
.cover-title {
  margin: 10px 0 12px; font-size: 27px; line-height: 1.28; font-weight: 700;
  letter-spacing: -.015em; max-width: 26em;
}
.cover-lead { font-size: 13px; color: var(--ink-2); max-width: 40em; margin: 0 0 20px; }
.badge {
  display: inline-flex; align-items: center; gap: 7px; padding: 6px 13px 6px 11px;
  border-radius: 3px; font-size: 12px; font-weight: 700; letter-spacing: .01em;
  border: 1px solid transparent;
}
.badge:before { content: ''; width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.badge.pass { background: var(--ok-soft); color: var(--ok); border-color: #cfe4d8; }
.badge.conditional { background: var(--warn-soft); color: var(--warn); border-color: #ecdcbd; }
.badge.fail { background: var(--bad-soft); color: var(--bad); border-color: #ecd2d2; }
.badge.plain { background: #f3f5f6; color: var(--ink-2); border-color: var(--line); }
.meta {
  display: grid; grid-template-columns: repeat(4, minmax(0, 1fr));
  border-top: 1px solid var(--line); border-left: 1px solid var(--line); margin-top: 22px;
}
.meta > div { padding: 10px 13px; border-right: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.meta dt { font-size: 9.5px; letter-spacing: .1em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; margin: 0 0 3px; }
.meta dd { margin: 0; font-size: 12px; font-weight: 600; color: var(--ink); font-variant-numeric: tabular-nums; }
.meta dd small { display: block; font-size: 10.5px; font-weight: 400; color: var(--ink-3); margin-top: 2px; }

/* ---- 目录 ---- */
.toc { margin-top: 22px; }
.toc-h { font-size: 9.5px; letter-spacing: .14em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; margin-bottom: 8px; }
.toc ol { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: 1fr 1fr; gap: 0 34px; }
.toc li { display: flex; align-items: baseline; gap: 8px; padding: 4px 0; border-bottom: 1px dotted var(--line); font-size: 12px; }
.toc a { color: var(--ink-2); text-decoration: none; }
.toc .n { font-family: var(--mono); font-size: 10.5px; color: var(--accent); width: 22px; flex: none; font-weight: 700; }

/* ---- 章节 ---- */
.sec { margin-top: 38px; }
.sec-h { display: flex; align-items: flex-end; gap: 11px; padding-bottom: 7px; border-bottom: 1px solid var(--ink); margin-bottom: 16px; }
.sec-n { font-family: var(--mono); font-size: 10.5px; font-weight: 700; color: var(--accent); letter-spacing: .06em; padding-bottom: 2px; }
.sec-t { font-size: 15.5px; font-weight: 700; letter-spacing: -.01em; }
.sec-s { margin-left: auto; font-size: 10.5px; color: var(--ink-4); text-align: right; padding-bottom: 2px; }
.sub { display: flex; align-items: center; gap: 8px; font-size: 12px; font-weight: 700; color: var(--ink-2); margin: 22px 0 9px; }
.sub:before { content: ''; width: 3px; height: 11px; background: var(--accent); border-radius: 1px; }
.sub:first-child { margin-top: 4px; }
.sub .hint { font-weight: 400; font-size: 10.5px; color: var(--ink-4); }

/* ---- 表格 ---- */
.tbl-wrap { overflow-x: auto; }
.tbl { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.tbl th {
  font-size: 9.5px; letter-spacing: .08em; text-transform: uppercase; color: var(--ink-4);
  font-weight: 700; text-align: left; padding: 7px 9px; border-top: 1px solid var(--line);
  border-bottom: 1px solid var(--line); white-space: nowrap; vertical-align: bottom;
}
.tbl td { padding: 7px 9px; border-bottom: 1px solid var(--line-soft); vertical-align: top; }
.tbl tbody tr:last-child td { border-bottom: 1px solid var(--line); }
.tbl .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
.tbl .mono { font-family: var(--mono); font-size: 10.5px; }
.tbl .break { word-break: break-all; }
.tbl tbody tr.total td { font-weight: 700; background: #fafbfb; }
.tbl caption { caption-side: top; text-align: left; font-size: 10.5px; color: var(--ink-3); padding-bottom: 6px; }
.tbl.kv td:first-child { width: 36%; color: var(--ink-2); }

/* ---- KPI ---- */
.kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); border-top: 1px solid var(--line); border-left: 1px solid var(--line); }
.kpi { padding: 13px 14px 12px; border-right: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.kpi-k { font-size: 9.5px; letter-spacing: .1em; text-transform: uppercase; color: var(--ink-4); font-weight: 700; }
.kpi-v { font-size: 21px; font-weight: 700; letter-spacing: -.02em; margin: 3px 0 1px; font-variant-numeric: tabular-nums; }
.kpi-v span { font-size: 11px; font-weight: 600; color: var(--ink-3); margin-left: 3px; letter-spacing: 0; }
.kpi-n { font-size: 10.5px; color: var(--ink-3); font-variant-numeric: tabular-nums; }
.strip {
  display: flex; flex-wrap: wrap; gap: 0; margin-top: 10px; border: 1px solid var(--line);
  background: #fafbfb; font-size: 11px;
}
.strip > div { padding: 7px 13px; border-right: 1px solid var(--line); flex: 1 1 auto; }
.strip > div:last-child { border-right: none; }
.strip b { font-weight: 700; color: var(--ink); font-variant-numeric: tabular-nums; }
.strip span { color: var(--ink-4); font-size: 9.5px; letter-spacing: .08em; text-transform: uppercase; display: block; font-weight: 700; }

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
.statement { border: 1px solid var(--line); border-left: 3px solid var(--accent); background: #fcfdfd; padding: 15px 18px; }
.statement.pass { border-left-color: var(--ok); }
.statement.conditional { border-left-color: var(--warn); }
.statement.fail { border-left-color: var(--bad); }
.statement-t { font-size: 13.5px; font-weight: 700; margin-bottom: 4px; }
.statement-d { font-size: 11.5px; color: var(--ink-2); }
.statement-d b { font-variant-numeric: tabular-nums; }

/* ---- 图表 ---- */
.fig { margin: 0 0 22px; }
.fig:last-child { margin-bottom: 4px; }
.fig-h { display: flex; align-items: baseline; gap: 9px; margin-bottom: 7px; }
.fig-t { font-size: 11.5px; font-weight: 700; color: var(--ink-2); }
.fig-u { font-size: 10px; color: var(--ink-4); }
.fig-n { margin-left: auto; font-size: 10px; color: var(--ink-4); }
.fig svg { width: 100%; height: auto; display: block; }
.lg { display: flex; flex-wrap: wrap; gap: 15px; margin-top: 7px; font-size: 10.5px; color: var(--ink-3); }
.lg span { display: inline-flex; align-items: center; gap: 6px; }
.lg i { width: 11px; height: 2px; border-radius: 1px; display: inline-block; }
.lg i.dot { width: 7px; height: 7px; border-radius: 50%; }
.lg i.dash { background-image: linear-gradient(90deg, currentColor 55%, transparent 0); background-size: 6px 2px; }

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
  .sheet { padding: 26px 20px 28px; }
  .meta { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .cols, .cols.wide-right { grid-template-columns: 1fr; }
  .toc ol { grid-template-columns: 1fr; }
  .cover-title { font-size: 22px; }
}
@page { size: A4; margin: 15mm 14mm 15mm; }
@media print {
  body { background: #fff; padding: 0; font-size: 11px; }
  .sheet { max-width: none; box-shadow: none; padding: 0; }
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
}

/**
 * 折线图：带完整 Y 轴刻度、时间轴刻度、轴线、可选面积填充与参考线
 *
 * 使用 preserveAspectRatio 默认行为（等比缩放），避免 900×190 被拉伸后
 * 描边与字号失真 —— 这是原版报告图表「看着不专业」的主因之一。
 */
function lineChart(title: string, options: LineChartOptions): string {
  const { xs, series, unit = '', height = 208, refLines = [], band, legend: showLegend = true } = options
  const width = 760
  const pad = { left: 58, right: 26, top: 18, bottom: 30 }
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
      return `<line x1="${x.toFixed(1)}" y1="${(pad.top + plotH).toFixed(1)}" x2="${x.toFixed(
        1,
      )}" y2="${(pad.top + plotH + 4).toFixed(1)}" stroke="#cfd7db" stroke-width="1"/>
<text x="${x.toFixed(1)}" y="${(pad.top + plotH + 16).toFixed(1)}" text-anchor="middle" font-size="9.5" fill="#9aa6ad">${clockLabel(
        time,
      )}</text>`
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

/** 每秒成功 / 失败堆叠柱 */
function stackedBars(title: string, rows: ReportSample[], note?: string): string {
  if (!rows.length) return ''
  const width = 760
  const height = 190
  const pad = { left: 58, right: 26, top: 16, bottom: 30 }
  const plotW = width - pad.left - pad.right
  const plotH = height - pad.top - pad.bottom
  const data = downsample(rows, CHART_MAX_POINTS)
  const totals = data.map((item) => item.successCount + item.errorCount)
  const yMax = niceMax(Math.max(1, ...totals) * 1.04)
  const slot = plotW / data.length
  const barWidth = Math.max(0.7, slot - 0.5)

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

  const baseY = pad.top + plotH
  const bars = data
    .map((item, index) => {
      const x = pad.left + index * slot
      const successHeight = (item.successCount / yMax) * plotH
      const errorHeight = (item.errorCount / yMax) * plotH
      const successBar = `<rect x="${x.toFixed(1)}" y="${(baseY - successHeight).toFixed(
        1,
      )}" width="${barWidth.toFixed(1)}" height="${successHeight.toFixed(1)}" fill="#17774a" opacity=".72"/>`
      const errorBar = errorHeight
        ? `<rect x="${x.toFixed(1)}" y="${(baseY - successHeight - errorHeight).toFixed(1)}" width="${barWidth.toFixed(
            1,
          )}" height="${errorHeight.toFixed(1)}" fill="#a23333" opacity=".9"/>`
        : ''
      return successBar + errorBar
    })
    .join('')

  const tickCount = Math.min(6, data.length)
  const xAxis = Array.from({ length: tickCount }, (_, index) => {
    const at = Math.round((index / Math.max(1, tickCount - 1)) * (data.length - 1))
    const x = pad.left + at * slot + barWidth / 2
    return `<text x="${x.toFixed(1)}" y="${(baseY + 16).toFixed(1)}" text-anchor="middle" font-size="9.5" fill="#9aa6ad">${clockLabel(
      data[at].elapsedSec,
    )}</text>`
  }).join('')

  return `<figure class="fig">
<div class="fig-h"><span class="fig-t">${escapeHtml(title)}</span><span class="fig-u">单位：req/s</span>${
    note ? `<span class="fig-n">${escapeHtml(note)}</span>` : ''
  }</div>
<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)}">
${grids}<line x1="${pad.left}" y1="${baseY}" x2="${width - pad.right}" y2="${baseY}" stroke="#cfd7db" stroke-width="1"/>
${bars}${xAxis}
</svg>
<div class="lg"><span><i style="background:#17774a"></i>成功</span><span><i style="background:#a23333"></i>失败</span></div>
</figure>`
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

/** 响应时间累积分布曲线（由直方图分桶还原） */
function cdfChart(title: string, points: Array<{ ms: number; pct: number }>): string {
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

function kpiBlock(detail: ReportDetail, qps: QpsOverview): string {
  const report = detail.report
  const cards = [
    { k: '总请求数', v: fmtInt(report.totalRequests), u: '', n: `并发 ${fmtInt(report.concurrency)}` },
    {
      k: '平均 QPS',
      v: fmtInt(qps.avg),
      u: 'req/s',
      n: `峰值 ${fmtInt(qps.max)}`,
    },
    {
      k: '90% 分布 QPS',
      v: qps.used > 0 ? fmtInt(qps.ninety) : '—',
      u: qps.used > 0 ? 'req/s' : '',
      n: qps.used > 0 ? `剔除 ${fmtInt(qps.dropped)} 个升温采样点` : '采样不足，无法计算',
    },
    {
      k: '单并发 QPS',
      v: fmtNum(qps.perConcurrent, 2),
      u: 'req/s',
      n: `平均 QPS ÷ ${fmtInt(report.concurrency)} 并发`,
    },
    {
      k: '成功率',
      v: (100 - report.errorRate).toFixed(2),
      u: '%',
      n: `失败 ${fmtInt(report.errorCount)} 次 · 错误率 ${report.errorRate.toFixed(2)}%`,
    },
    { k: '平均响应时间', v: fmtNum(report.avgRt), u: 'ms', n: `TTFB ${fmtNum(report.ttfbAvg)} ms` },
    { k: 'P95 延迟', v: fmtNum(report.p95Rt), u: 'ms', n: `P99 ${fmtNum(report.p99Rt)} ms` },
    { k: 'APDEX', v: report.apdex.toFixed(3), u: '', n: apdexGrade(report.apdex) },
  ]
  return `<div class="kpis">${cards
    .map(
      (item) =>
        `<div class="kpi"><div class="kpi-k">${escapeHtml(item.k)}</div><div class="kpi-v">${escapeHtml(
          item.v,
        )}${item.u ? `<span>${escapeHtml(item.u)}</span>` : ''}</div><div class="kpi-n">${escapeHtml(
          item.n,
        )}</div></div>`,
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
  const fromRecord = requests.find((item) => item.url)?.url?.trim() ?? ''
  return fromRecord || raw || '未记录'
}

function renderHtml(detail: ReportDetail, requests: ReportRequestRecord[] = []): string {
  const report = detail.report
  const config = detail.config
  const metrics = config?.metrics
  const agent = config?.agent
  const samples = detail.samples ?? []
  const seconds = durationSeconds(detail)
  const rows = downsample(samples, CHART_MAX_POINTS)

  const qps = qpsOverview(detail, { rampUpSec: config?.rampUpSec ?? 0 })
  const knee = detectKnee(detail)
  const score = scoreReport(detail)
  const checks = selfCheck(detail)
  const checksPassed = checks.every((item) => item.pass)
  const grade = gradeReport(detail)
  const sla = slaRows(detail)
  const slaPassed = sla.every((row) => row.pass)

  const bytesReceived = report.bytesTotal ?? 0
  const bytesSent = metrics?.bytesSent ?? 0
  const totalTraffic = bytesReceived + bytesSent
  const recordMode = metrics?.recordMode ?? config?.recordDetail ?? 'full'
  const droppedRecords = metrics?.recordsDropped ?? 0
  const droppedErrors = metrics?.errorsDropped ?? 0

  const modeText = MODE_LABELS[config?.mode ?? ''] ?? (detail.stages.length ? '阶梯加压' : '固定并发')
  const rampUp = config?.rampUpSec ?? 0

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
  })

  const stacked = stackedBars(
    '每秒成功 / 失败请求数',
    rows,
    `失败合计 ${fmtInt(report.errorCount)} 次`,
  )

  const bandwidth = lineChart('网络吞吐（接收字节 / 秒）', {
    xs: rows.map((item) => item.elapsedSec),
    series: [
      { name: '接收', color: '#37628f', values: rows.map((item) => item.bytes / 1024), area: true },
    ],
    unit: 'KB/s',
    note: `接收合计 ${fmtBytes(bytesReceived)} · 发送合计 ${fmtBytes(bytesSent)}`,
  })

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
  })

  const cdf = cdfChart('响应时间累积分布', cdfPoints(detail))

  const ttfb = lineChart('首字节时间（TTFB）趋势', {
    xs: rows.map((item) => item.elapsedSec),
    series: [{ name: 'TTFB 平均', color: '#6b5b95', values: rows.map((item) => item.ttfbAvg), area: true }],
    unit: 'ms',
    height: 170,
    note: `P95 ${fmtNum(report.ttfbP95)} ms`,
  })

  /* ---- 05 错误 ---- */
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
  })

  /* ---- 表格 ---- */
  const statsRows = kvRows([
    ['样本总数', fmtInt(report.totalRequests)],
    ['成功 / 失败', `${fmtInt(report.successCount)} / ${fmtInt(report.errorCount)}`],
    ['错误率', `${report.errorRate.toFixed(3)}%`],
    ['平均 / 峰值 QPS', `${fmtInt(qps.avg)} / ${fmtInt(qps.max)} req/s`],
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
    ['总流量（入 / 出）', `${fmtBytes(bytesSent)} / ${fmtBytes(bytesReceived)}`],
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

  // 分阶段：补扩展效率（并发增幅与吞吐增幅的比值）
  const stageRows = detail.stages.map((stage, index) => {
    const steady = knee.stages[index]
    const prev = detail.stages[index - 1]
    const concurGain =
      prev && prev.concurrent > 0 ? (stage.concurrent - prev.concurrent) / prev.concurrent : null
    const efficiency = steady && concurGain !== null && concurGain > 0 ? steady.gain / concurGain : null
    return [
      `<span class="mono">S${stage.index + 1}</span>`,
      fmtInt(stage.concurrent),
      `${stage.durationSec}s`,
      fmtInt(stage.requests),
      fmtInt(stage.errors),
      fmtNum(stage.avgQps),
      steady ? fmtInt(steady.qps) : '—',
      fmtNum(stage.avgRt),
      fmtNum(stage.p95Rt),
      steady ? `${(steady.gain * 100).toFixed(1)}%` : '—',
      efficiency === null ? '—' : `${(efficiency * 100).toFixed(0)}%`,
    ]
  })

  const slowRequests = [...requests].sort((a, b) => b.latencyMs - a.latencyMs).slice(0, 10)
  const slowRows = slowRequests.map((item) => [
    `<span class="mono">${(item.elapsedMs / 1000).toFixed(2)}s</span>`,
    `<span class="mono break">${escapeHtml(clip(item.url, 120))}</span>`,
    fmtNum(item.latencyMs, 0),
    item.status ? `<span class="mono">${item.status}</span>` : '—',
    item.success ? '<span style="color:var(--ok)">成功</span>' : '<span style="color:var(--bad)">失败</span>',
  ])

  /* ---- 组装 ---- */
  const cover = `<header class="cover">
  <div class="cover-top">
    <div class="brand"><span class="brand-mark">A</span><div>AstroMan<span class="brand-sub" style="display:block">Performance Report</span></div></div>
    <span class="badge plain">报告编号 R-${report.id}</span>
  </div>
  <div class="cover-kicker">Performance Test Report · 性能测试报告</div>
  <h1 class="cover-title">${escapeHtml(report.planName)}</h1>
  <p class="cover-lead">${escapeHtml(grade.statement)}</p>
  <span class="badge ${grade.grade}">${escapeHtml(grade.label)}</span>
  <dl class="meta">
    <div><dt>被测目标</dt><dd class="mono" style="font-size:11px;font-weight:500">${escapeHtml(
      clip(resolvedTargetUrl(detail, requests), 46),
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
  </dl>
</header>

<nav class="toc">
  <div class="toc-h">目录 · Contents</div>
  <ol>
    <li><span class="n">01</span><a href="#s1">执行摘要</a></li>
    <li><span class="n">02</span><a href="#s2">测试环境与配置</a></li>
    <li><span class="n">03</span><a href="#s3">吞吐量与稳定性</a></li>
    <li><span class="n">04</span><a href="#s4">响应时间分析</a></li>
    <li><span class="n">05</span><a href="#s5">错误分析</a></li>
    <li><span class="n">06</span><a href="#s6">容量评估与瓶颈定位</a></li>
    <li><span class="n">07</span><a href="#s7">结论与优化建议</a></li>
    <li><span class="n">A</span><a href="#sa">指标口径定义</a></li>
    <li><span class="n">B</span><a href="#sb">数据完整性与自洽校验</a></li>
    <li><span class="n">C</span><a href="#sc">明细数据与导出</a></li>
  </ol>
</nav>`

  const statement = `<div class="statement ${grade.grade}">
<div class="statement-t">${escapeHtml(grade.label)} · ${escapeHtml(grade.reason)}</div>
<div class="statement-d">${escapeHtml(grade.statement)} 共执行 <b>${fmtInt(
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

  const sec1 = section(
    's1',
    '01',
    '执行摘要',
    'Executive Summary',
    `${statement}
<div class="sub">核心指标</div>
${kpiBlock(detail, qps)}
<div class="strip">
  <div><span>目标 / 达成</span><b>${
    qps.target
      ? `${fmtInt(qps.target)} / ${fmtNum(qps.attainment, 1)}%`
      : '不限速'
  }</b></div>
  <div><span>总流量</span><b>${fmtBytes(totalTraffic)}</b></div>
  <div><span>接收 / 发送</span><b>${fmtBytes(bytesReceived)} / ${fmtBytes(bytesSent)}</b></div>
  <div><span>明细模式</span><b>${escapeHtml(recordModeLabel(recordMode))}</b></div>
  <div><span>施压端</span><b>${escapeHtml(agent?.host ?? '未记录')}</b></div>
</div>
${slaSection}
<div class="sub">关键发现<span class="hint">按严重程度排序</span></div>
${findingsBlock(detail)}`,
  )

  const targetRows = kvRows([
    ['请求方法', escapeHtml(config?.method ?? '—')],
    ['请求地址', `<span class="mono break">${escapeHtml(clip(resolvedTargetUrl(detail, requests), 300))}</span>`],
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
</div>`,
  )

  const sec3 = section(
    's3',
    '03',
    '吞吐量与稳定性',
    'Throughput & Stability',
    `${throughput}${stacked}${bandwidth}`,
  )

  const sec4 = section(
    's4',
    '04',
    '响应时间分析',
    'Latency Analysis',
    `${latency}${cdf}
<div class="cols">
<div><div class="sub">分位统计</div>${tableHtml(['分位', '响应时间'], percentileRows, [1], { kv: true })}</div>
<div><div class="sub">响应时间分布<span class="hint">按区间计数</span></div>${
      histogramRows.length
        ? tableHtml(['区间', '数量', '占比'], histogramRows, [2])
        : '<div class="note">暂无直方图数据。</div>'
    }</div>
</div>
${ttfb}`,
  )

  const sec5 = section(
    's5',
    '05',
    '错误分析',
    'Error Analysis',
    `${
      report.errorCount === 0
        ? '<div class="note" style="margin-top:0">本次压测未产生失败请求，以下分布仅含成功响应。</div>'
        : ''
    }
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

  const sec6 = section(
    's6',
    '06',
    '容量评估与瓶颈定位',
    'Capacity & Bottleneck',
    `<div class="statement" style="border-left-color:${
      knee.saturated ? 'var(--warn)' : 'var(--accent)'
    };margin-bottom:16px">
<div class="statement-t">${knee.saturated ? '已检出饱和拐点' : '未检出饱和拐点'}</div>
<div class="statement-d">${escapeHtml(knee.note)}</div>
</div>
${scatterChart('负载-延迟关系', rows)}
${
  stageRows.length
    ? `<div class="sub">分阶段明细<span class="hint">稳态值取阶段中位，扩展效率 = 吞吐增幅 / 并发增幅</span></div>
${tableHtml(
        ['阶段', '并发', '时长', '请求数', '错误', '平均 QPS', '稳态 QPS', '平均 RT', 'P95', '吞吐增幅', '扩展效率'],
        stageRows,
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
      )}`
    : '<div class="note">固定并发模式不做分阶段容量判定，如需测量容量上限请改用阶梯加压。</div>'
}
<div class="sub">综合体检<span class="hint">四维等权评分，规则见下方注</span></div>
<div class="cols">
<div>${radarChart(score.dimensions)}</div>
<div>${scoreBlock(detail)}</div>
</div>`,
  )

  const sec7 = section(
    's7',
    '07',
    '结论与优化建议',
    'Conclusion & Recommendations',
    `<div class="statement ${grade.grade}">
<div class="statement-t">${escapeHtml(grade.label)}</div>
<div class="statement-d">${escapeHtml(grade.statement)}</div>
</div>
<div class="sub">优化建议<span class="hint">P0 阻断上线 / P1 本次迭代 / P2 长期</span></div>
${adviceBlock(detail)}
${
  summary
    ? `<div class="sub">补充说明</div><div class="prose">${escapeHtml(summary).replace(/\n/g, '<br/>')}</div>`
    : ''
}`,
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
    ['错误率', '失败请求数 / 总请求数。SLA 判定与红黄绿结论均以此口径为准。'],
    ['总流量', '发送字节 + 接收字节；趋势图中的带宽按每秒采样折算。'],
    ['稳态 QPS', '该阶段剔除前 20%（升压）与后 10%（切换抖动）后采样 QPS 的中位数。'],
    ['扩展效率', '吞吐增幅百分比 ÷ 并发增幅百分比；100% 表示线性扩展，明显低于 100% 说明已接近瓶颈。'],
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
        recordMode === 'off' ? '（本次未记录）' : `（共 ${fmtInt(requests.length)} 条样本）`
      }</span></div>
${tableHtml(['时间点', 'URL', '耗时(ms)', '状态码', '结果'], slowRows, [2, 3])}`
    : ''
}`,
  )

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escapeHtml(report.planName)} · 性能测试报告 R-${report.id}</title>
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
${sec7}
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
  <div style="margin-top:6px">本报告仅反映上述施压端、目标环境与参数组合下的实测结果，不代表目标系统的绝对容量上限。容量结论应在生产等价拓扑与真实数据量下复测确认。</div>
</footer>
</div>
</body>
</html>`
}

/** 生成自包含 HTML 压测报告 */
export function buildReportHtml(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
): string {
  return renderHtml(detail, requests)
}

/* ---------- Excel 工作簿 ---------- */

function percentileSheet(detail: ReportDetail): CellValue[][] {
  const report = detail.report
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
  ]
}

/** 生成 Excel 多工作表数据 */
export function buildReportSheets(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
): XlsxSheet[] {
  const report = detail.report
  const sla = slaRows(detail)
  const qps = qpsOverview(detail, { rampUpSec: detail.config?.rampUpSec ?? 0 })

  const overview: CellValue[][] = [
    ['压测报告', report.planName],
    ['报告编号', `R-${report.id}`],
    ['执行状态', statusLabel(report.status)],
    ['开始时间', fmtTime(report.startTime)],
    ['结束时间', fmtTime(report.endTime)],
    ['持续时长', fmtDuration(report.startTime, report.endTime)],
    ['压测结论', verdict(detail).title],
    ['结论依据', verdict(detail).desc],
    [],
    ['核心指标', '数值'],
    ['总请求数', report.totalRequests],
    ['成功请求数', report.successCount],
    ['失败请求数', report.errorCount],
    ['错误率(%)', report.errorRate],
    ['成功率(%)', Number((100 - report.errorRate).toFixed(4))],
    ['平均 QPS', qps.avg],
    ['峰值 QPS', qps.max],
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
    ['流量总量(B)', report.bytesTotal],
    [],
  ]

  if (sla.length) {
    overview.push(['SLA 验收', '目标', '实测', '结论'])
    sla.forEach((row) => overview.push([row.label, row.target, row.actual, row.pass ? '通过' : '未通过']))
  }

  const samples: CellValue[][] = [
    ['时间(s)', '并发', 'QPS', '平均 RT(ms)', 'P50(ms)', 'P95(ms)', 'P99(ms)', 'TTFB(ms)', '流量(B)', '成功数', '失败数'],
    ...detail.samples.map((sample: ReportSample) => [
      sample.elapsedSec,
      sample.concurrent,
      sample.qps,
      sample.avgRt,
      sample.p50Rt,
      sample.p95Rt,
      sample.p99Rt,
      sample.ttfbAvg,
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

  const config = detail.config
  const snapshot: CellValue[][] = config
    ? [
        ['配置项', '取值'],
        ['压测模式', MODE_LABELS[config.mode ?? ''] ?? config.mode ?? '-'],
        ['请求方法', config.method ?? '-'],
        ['请求地址', resolvedTargetUrl(detail, requests)],
        ['请求头数量', config.headers ?? 0],
        ['参数数量', config.params ?? 0],
        ['请求体类型', config.bodyType ?? '-'],
        ['环境 ID', config.envId ?? '-'],
        ['超时时间(ms)', config.timeoutMs ?? '-'],
        ['思考时间(ms)', config.thinkTimeMs ?? '-'],
        ['加压时间(s)', config.rampUpSec ?? '-'],
        ['目标 QPS', config.targetRps || '不限速'],
        ['明细记录模式', recordModeLabel(config.recordDetail)],
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
        ['施压端主机', config.agent?.host ?? '-'],
        ['报告生成时间', fmtTime(Date.now())],
      ]
    : [['配置项', '取值'], ['说明', '该报告未保存配置快照']]

  const advice: CellValue[][] = [
    ['优先级', '建议', '说明'],
    ...buildAdvice(detail).map((item) => [item.priority, item.title, item.detail]),
  ]

  return [
    { name: '概览', rows: overview },
    { name: '优化建议', rows: advice },
    { name: '延迟分位', rows: percentileSheet(detail) },
    { name: '采样明细', rows: samples },
    { name: '请求明细', rows: requestSheet },
    { name: '响应时间分布', rows: histogram },
    { name: '状态码分布', rows: statusCodes },
    { name: '错误类型', rows: errorTypes },
    { name: '错误明细', rows: errors },
    { name: '分阶段汇总', rows: stages },
    { name: '配置快照', rows: snapshot },
  ]
}
