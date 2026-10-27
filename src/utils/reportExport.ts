import type {
  ReportDetail,
  ReportError,
  ReportRequestRecord,
  ReportSample,
  StatusCount,
} from '@/types/models'
import type { CellValue, XlsxSheet } from './xlsx'

/* ---------- 通用格式化 ---------- */

const STATUS_LABELS: Record<string, string> = {
  completed: '已完成',
  stopped: '已手动停止',
  running: '运行中',
}

function fmtInt(value: number | null | undefined): string {
  return Number(value ?? 0).toLocaleString('zh-CN')
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
  if (!value) return '-'
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function fmtDuration(start: number | null | undefined, end: number | null | undefined): string {
  if (!start || !end || end <= start) return '-'
  const seconds = Math.round((end - start) / 1000)
  const minute = Math.floor(seconds / 60)
  const second = seconds % 60
  return minute ? `${minute} 分 ${second} 秒` : `${second} 秒`
}

function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status
}

function escapeHtml(text: string): string {
  return text
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

function apdexGrade(apdex: number): { label: string; tone: 'ok' | 'warn' | 'bad' } {
  if (apdex >= 0.94) return { label: '优秀', tone: 'ok' }
  if (apdex >= 0.85) return { label: '良好', tone: 'ok' }
  if (apdex >= 0.7) return { label: '一般', tone: 'warn' }
  return { label: '较差', tone: 'bad' }
}

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
      label: '平均 RPS',
      target: `≥ ${sla.minRps}`,
      actual: fmtNum(report.avgQps),
      pass: report.avgQps >= sla.minRps,
    })
  }
  return rows
}

function verdict(detail: ReportDetail): { title: string; desc: string; tone: 'ok' | 'warn' | 'bad' } {
  const report = detail.report
  const sla = slaRows(detail)
  const slaPassed = sla.length === 0 || sla.every((row) => row.pass)
  if (!slaPassed) {
    const failed = sla.filter((row) => !row.pass).map((row) => row.label)
    return {
      title: '未达到 SLA 目标',
      desc: `以下指标未达标：${failed.join('、')}`,
      tone: 'bad',
    }
  }
  if (report.errorRate > 5) {
    return {
      title: '错误率偏高',
      desc: `错误率 ${report.errorRate.toFixed(2)}%，建议检查服务端稳定性`,
      tone: 'bad',
    }
  }
  if (report.apdex < 0.85) {
    return {
      title: '性能表现一般',
      desc: `APDEX ${report.apdex.toFixed(3)}，响应时间仍有优化空间`,
      tone: 'warn',
    }
  }
  return {
    title: '压测通过',
    desc: `APDEX ${report.apdex.toFixed(3)}，错误率 ${report.errorRate.toFixed(2)}%`,
    tone: 'ok',
  }
}

/* ---------- HTML 报告 ---------- */

const STYLE = `
:root {
  --bg: #f5f7f8;
  --surface: #ffffff;
  --surface-alt: #f8fafb;
  --border: #e3e8ea;
  --text: #22302f;
  --text-2: #4a5a59;
  --muted: #7d8c8b;
  --primary: #2f6f62;
  --primary-soft: #eaf2f0;
  --ok: #2f8f5b;
  --warn: #b7822f;
  --bad: #c05050;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  padding: 32px 24px 56px;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
  font-size: 13px;
  line-height: 1.6;
}
.wrap { max-width: 1080px; margin: 0 auto; display: flex; flex-direction: column; gap: 18px; }
.report-head {
  display: flex; align-items: flex-start; justify-content: space-between; gap: 16px;
  padding: 20px 24px; background: var(--surface); border: 1px solid var(--border);
  border-radius: 14px; box-shadow: 0 1px 2px rgba(31, 51, 49, .04);
}
.brand { font-size: 11px; letter-spacing: 2px; color: var(--muted); text-transform: uppercase; }
.title { margin: 6px 0 4px; font-size: 22px; font-weight: 700; }
.sub { color: var(--muted); font-size: 12px; }
.tag {
  padding: 3px 10px; border-radius: 999px; font-size: 11px; font-weight: 600;
  background: var(--primary-soft); color: var(--primary); white-space: nowrap;
}
.card {
  background: var(--surface); border: 1px solid var(--border); border-radius: 14px;
  padding: 18px 20px; box-shadow: 0 1px 2px rgba(31, 51, 49, .04);
}
.card-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
.card-title { font-size: 14px; font-weight: 600; }
.card-sub { font-size: 11px; color: var(--muted); margin-top: 2px; }
.verdict { border-left: 4px solid var(--primary); }
.verdict.ok { border-left-color: var(--ok); }
.verdict.warn { border-left-color: var(--warn); }
.verdict.bad { border-left-color: var(--bad); }
.verdict-title { font-size: 15px; font-weight: 700; }
.verdict.ok .verdict-title { color: var(--ok); }
.verdict.warn .verdict-title { color: var(--warn); }
.verdict.bad .verdict-title { color: var(--bad); }
.verdict-desc { font-size: 12px; color: var(--text-2); margin-top: 2px; }
.grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
.kpi { padding: 12px 14px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-alt); }
.kpi-label { font-size: 11px; color: var(--muted); }
.kpi-value { font-size: 19px; font-weight: 700; margin-top: 3px; letter-spacing: .3px; }
.kpi-note { font-size: 10px; color: var(--muted); margin-top: 2px; }
.two-col { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; align-items: start; }
table { width: 100%; border-collapse: collapse; font-size: 12px; }
th, td { padding: 7px 10px; text-align: left; border-bottom: 1px solid var(--border); vertical-align: top; }
th { background: var(--surface-alt); color: var(--text-2); font-weight: 600; font-size: 11px; }
tr:last-child td { border-bottom: none; }
td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
.mono { font-family: "JetBrains Mono", "Cascadia Code", Consolas, monospace; font-size: 11px; }
.break { word-break: break-all; }
.ok-text { color: var(--ok); font-weight: 600; }
.bad-text { color: var(--bad); font-weight: 600; }
.chart { margin-top: 14px; }
.chart-title { font-size: 12px; color: var(--text-2); font-weight: 600; margin-bottom: 6px; }
.chart svg { width: 100%; height: auto; display: block; }
.legend { display: flex; gap: 14px; font-size: 11px; color: var(--muted); margin-top: 4px; }
.legend i { display: inline-block; width: 8px; height: 8px; border-radius: 2px; margin-right: 5px; }
.bar { height: 8px; border-radius: 4px; background: var(--primary); min-width: 2px; }
.bar-wrap { display: flex; align-items: center; gap: 8px; }
.bar-wrap .bar { flex: 1; }
.foot { text-align: center; font-size: 11px; color: var(--muted); padding-top: 4px; }
@media (max-width: 760px) {
  .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .two-col { grid-template-columns: 1fr; }
  body { padding: 18px 12px 32px; }
}
@media print {
  body { background: #fff; padding: 0; font-size: 11px; }
  .card, .report-head { box-shadow: none; break-inside: avoid; }
  .wrap { max-width: none; gap: 12px; }
}
@page { margin: 14mm; }
`

function kpiCards(detail: ReportDetail): string {
  const report = detail.report
  const cards: Array<{ label: string; value: string; note: string }> = [
    { label: '总请求数', value: fmtInt(report.totalRequests), note: `并发 ${report.concurrency}` },
    { label: '成功率', value: `${(100 - report.errorRate).toFixed(2)}%`, note: `失败 ${fmtInt(report.errorCount)}` },
    { label: '平均 RPS', value: fmtNum(report.avgQps), note: `峰值 ${fmtNum(report.maxQps)}` },
    { label: '平均响应时间', value: `${fmtNum(report.avgRt)} ms`, note: `TTFB ${fmtNum(report.ttfbAvg)} ms` },
    { label: 'P95 延迟', value: `${fmtNum(report.p95Rt)} ms`, note: `P99 ${fmtNum(report.p99Rt)} ms` },
    { label: 'APDEX', value: report.apdex.toFixed(3), note: apdexGrade(report.apdex).label },
    { label: '流量总量', value: fmtBytes(report.bytesTotal), note: `平均 ${fmtBytes(report.bytesTotal / Math.max(1, report.totalRequests))}` },
    { label: '持续时长', value: fmtDuration(report.startTime, report.endTime), note: fmtTime(report.startTime) },
  ]
  return cards
    .map(
      (item) =>
        `<div class="kpi"><div class="kpi-label">${escapeHtml(item.label)}</div><div class="kpi-value">${escapeHtml(
          item.value,
        )}</div><div class="kpi-note">${escapeHtml(item.note)}</div></div>`,
    )
    .join('')
}

function tableHtml(headers: string[], rows: string[][], numericCols: number[] = []): string {
  const head = headers
    .map((name, index) => `<th${numericCols.includes(index) ? ' class="num"' : ''}>${escapeHtml(name)}</th>`)
    .join('')
  const body = rows
    .map(
      (row) =>
        `<tr>${row
          .map(
            (cell, index) =>
              `<td${numericCols.includes(index) ? ' class="num"' : ''}>${cell}</td>`,
          )
          .join('')}</tr>`,
    )
    .join('')
  return `<div style="overflow-x:auto"><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>`
}

function svgChart(
  title: string,
  series: Array<{ name: string; color: string; values: number[] }>,
): string {
  const width = 900
  const height = 190
  const padding = { left: 56, right: 16, top: 14, bottom: 24 }
  const plotWidth = width - padding.left - padding.right
  const plotHeight = height - padding.top - padding.bottom
  const maxValue = Math.max(1, ...series.flatMap((item) => item.values))
  const count = Math.max(1, ...series.map((item) => item.values.length))

  const grid = Array.from({ length: 5 }, (_, index) => {
    const y = padding.top + (plotHeight / 4) * index
    const value = maxValue * (1 - index / 4)
    return `<line x1="${padding.left}" y1="${y}" x2="${width - padding.right}" y2="${y}" stroke="#eef1f2" stroke-width="1"/>
<text x="${padding.left - 8}" y="${y + 3}" text-anchor="end" font-size="10" fill="#8a9a99">${value.toFixed(
      value >= 100 ? 0 : 1,
    )}</text>`
  }).join('')

  const lines = series
    .map((item) => {
      const points = item.values
        .map((value, index) => {
          const x = padding.left + (count <= 1 ? 0 : (index / (count - 1)) * plotWidth)
          const y = padding.top + (1 - value / maxValue) * plotHeight
          return `${x.toFixed(1)},${y.toFixed(1)}`
        })
        .join(' ')
      return `<polyline fill="none" stroke="${item.color}" stroke-width="1.8" stroke-linejoin="round" points="${points}"/>`
    })
    .join('')

  const legend = series
    .map(
      (item) =>
        `<span><i style="background:${item.color}"></i>${escapeHtml(item.name)}</span>`,
    )
    .join('')

  return `<div class="chart"><div class="chart-title">${escapeHtml(title)}</div>
<svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" role="img">${grid}${lines}</svg>
<div class="legend">${legend}</div></div>`
}

/** HTML 报告中最多展示的请求明细条数 */
const HTML_REQUEST_LIMIT = 5000

function renderHtml(detail: ReportDetail, requests: ReportRequestRecord[] = []): string {
  const report = detail.report
  const sla = slaRows(detail)
  const slaPassed = sla.length === 0 || sla.every((row) => row.pass)
  const result = verdict(detail)
  const htmlRequests = requests.slice(0, HTML_REQUEST_LIMIT)

  const sampleRows = detail.samples.map((sample) => [
    `<span class="mono">${sample.elapsedSec}s</span>`,
    String(sample.concurrent),
    fmtNum(sample.qps),
    fmtNum(sample.avgRt),
    fmtNum(sample.p50Rt),
    fmtNum(sample.p95Rt),
    fmtNum(sample.p99Rt),
    fmtNum(sample.ttfbAvg),
    fmtBytes(sample.bytes),
    String(sample.successCount),
    String(sample.errorCount),
  ])

  const errorRows = detail.errors.map((error: ReportError) => [
    `<span class="mono">${error.elapsedSec}s</span>`,
    escapeHtml(error.errorType),
    error.responseStatus ? String(error.responseStatus) : '-',
    `<div class="break">${escapeHtml(clip(error.errorMessage, 300))}</div>`,
    `<div class="break mono">${escapeHtml(clip(error.requestUrl, 300))}</div>`,
    `<div class="break mono">${escapeHtml(clip(error.responseBody, 400))}</div>`,
  ])

  const requestRows = htmlRequests.map((item) => [
    `<span class="mono">${(item.elapsedMs / 1000).toFixed(3)}s</span>`,
    `<span class="mono">${fmtTime(item.timeMs)}</span>`,
    escapeHtml(item.method),
    `<div class="break mono">${escapeHtml(clip(item.url, 300))}</div>`,
    `<div class="break mono">${escapeHtml(clip(item.body ?? '', 300))}</div>`,
    `<div class="break mono">${escapeHtml(clip(item.params ?? '', 200))}</div>`,
    `<div class="break mono">${escapeHtml(clip(item.responseBody ?? '', 300))}</div>`,
    item.status ? String(item.status) : '-',
    fmtNum(item.latencyMs, 0),
    fmtNum(item.ttfbMs, 0),
    fmtBytes(item.bytes),
    item.success
      ? '<span class="ok-text">成功</span>'
      : '<span class="bad-text">失败</span>',
    escapeHtml(item.errorType ?? ''),
    `<div class="break">${escapeHtml(clip(item.errorMessage ?? '', 300))}</div>`,
  ])

  const maxCode = Math.max(1, ...detail.statusCodes.map((item: StatusCount) => item.count))
  const codeRows = detail.statusCodes.map((item) => [
    `<span class="mono">${item.code}</span>`,
    `<div class="bar-wrap"><div class="bar" style="width:${(item.count / maxCode) * 100}%"></div></div>`,
    String(item.count),
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const errorTypeRows = detail.errorTypes.map((item) => [
    escapeHtml(item.errorType),
    `<div class="bar-wrap"><div class="bar" style="width:${(item.count / Math.max(1, ...detail.errorTypes.map((e) => e.count))) * 100}%"></div></div>`,
    String(item.count),
  ])

  const histogramRows = detail.histogram.map((item) => [
    escapeHtml(item.label),
    `<div class="bar-wrap"><div class="bar" style="width:${(item.count / Math.max(1, ...detail.histogram.map((b) => b.count))) * 100}%"></div></div>`,
    String(item.count),
    `${((item.count / Math.max(1, report.totalRequests)) * 100).toFixed(2)}%`,
  ])

  const stageRows = detail.stages.map((stage) => [
    `第 ${stage.index + 1} 阶段`,
    String(stage.concurrent),
    `${stage.durationSec}s`,
    fmtInt(stage.requests),
    fmtInt(stage.errors),
    fmtNum(stage.avgQps),
    fmtNum(stage.avgRt),
    fmtNum(stage.p95Rt),
  ])

  const slaSection = sla.length
    ? `<div class="card"><div class="card-head"><div><div class="card-title">SLA 达标判定</div><div class="card-sub">压测配置中声明的验收阈值</div></div>
<span class="tag" style="${slaPassed ? '' : 'background:#fdeeee;color:#c05050'}">${slaPassed ? '全部达标' : '存在未达标项'}</span></div>
${tableHtml(
        ['验收项', '目标', '实测', '结论'],
        sla.map((row) => [
          escapeHtml(row.label),
          escapeHtml(row.target),
          escapeHtml(row.actual),
          row.pass ? '<span class="ok-text">通过</span>' : '<span class="bad-text">未通过</span>',
        ]),
      )}</div>`
    : ''

  const config = detail.config
  const configRows: string[][] = config
    ? [
        ['压测模式', config.mode === 'staged' ? '阶梯加压' : '固定并发'],
        ['请求方法', escapeHtml(config.method ?? '-')],
        ['请求地址', `<span class="mono break">${escapeHtml(clip(config.url, 300))}</span>`],
        ['请求头数量', String(config.headers ?? 0)],
        ['参数数量', String(config.params ?? 0)],
        ['请求体类型', escapeHtml(config.bodyType ?? '-')],
        ['超时时间', config.timeoutMs ? `${config.timeoutMs} ms` : '-'],
        ['思考时间', config.thinkTimeMs ? `${config.thinkTimeMs} ms` : '-'],
        ['加压时间', config.rampUpSec ? `${config.rampUpSec} s` : '-'],
        ['目标 RPS', config.targetRps ? String(config.targetRps) : '不限速'],
        [
          '阶梯配置',
          config.stages?.length
            ? config.stages.map((stage) => `${stage.concurrent} 并发 / ${stage.durationSec}s`).join('，')
            : '-',
        ],
      ]
    : []

  const samples = detail.samples
  const charts = [
    svgChart('吞吐量趋势（RPS）', [
      { name: 'QPS', color: '#2f6f62', values: samples.map((item) => item.qps) },
    ]),
    svgChart('响应时间趋势（ms）', [
      { name: '平均', color: '#2f6f62', values: samples.map((item) => item.avgRt) },
      { name: 'P95', color: '#b7822f', values: samples.map((item) => item.p95Rt) },
      { name: 'P99', color: '#c05050', values: samples.map((item) => item.p99Rt) },
    ]),
  ].join('')

  const percentile = tableHtml(
    ['指标', '数值'],
    [
      ['最小响应时间', `${fmtNum(report.minRt)} ms`],
      ['平均响应时间', `${fmtNum(report.avgRt)} ms`],
      ['P50', `${fmtNum(report.p50Rt)} ms`],
      ['P75', `${fmtNum(report.p75Rt)} ms`],
      ['P90', `${fmtNum(report.p90Rt)} ms`],
      ['P95', `${fmtNum(report.p95Rt)} ms`],
      ['P99', `${fmtNum(report.p99Rt)} ms`],
      ['P99.9', `${fmtNum(report.p999Rt)} ms`],
      ['最大响应时间', `${fmtNum(report.maxRt)} ms`],
      ['标准差', `${fmtNum(report.stddevRt)} ms`],
      ['TTFB 平均', `${fmtNum(report.ttfbAvg)} ms`],
      ['TTFB P95', `${fmtNum(report.ttfbP95)} ms`],
    ],
    [1],
  )

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escapeHtml(report.planName)} - 压测报告</title>
<style>${STYLE}</style>
</head>
<body>
<div class="wrap">
  <header class="report-head">
    <div>
      <div class="brand">FbinHttp · 压测报告</div>
      <h1 class="title">${escapeHtml(report.planName)}</h1>
      <div class="sub">开始 ${fmtTime(report.startTime)} · 结束 ${fmtTime(report.endTime)} · 持续 ${fmtDuration(
        report.startTime,
        report.endTime,
      )} · 报告 #${report.id}</div>
    </div>
    <span class="tag">${escapeHtml(statusLabel(report.status))}</span>
  </header>

  <div class="card verdict ${result.tone}">
    <div class="verdict-title">${escapeHtml(result.title)}</div>
    <div class="verdict-desc">${escapeHtml(result.desc)}</div>
  </div>

  <div class="card">
    <div class="card-head"><div class="card-title">核心指标</div></div>
    <div class="grid">${kpiCards(detail)}</div>
  </div>

  ${slaSection}

  <div class="card">
    <div class="card-head"><div><div class="card-title">实时趋势</div><div class="card-sub">按秒采样，共 ${samples.length} 个采样点</div></div></div>
    ${charts}
  </div>

  <div class="card">
    <div class="card-head"><div class="card-title">延迟分位统计</div></div>
    ${percentile}
  </div>

  <div class="card">
    <div class="card-head"><div class="card-title">响应时间分布</div></div>
    ${histogramRows.length ? tableHtml(['区间', '占比', '数量', '比例'], histogramRows, [2, 3]) : '<div class="card-sub">暂无数据</div>'}
  </div>

  ${
    stageRows.length
      ? `<div class="card"><div class="card-head"><div class="card-title">分阶段汇总</div></div>
${tableHtml(['阶段', '并发', '时长', '请求数', '错误数', '平均 RPS', '平均 RT', 'P95'], stageRows, [1, 2, 3, 4, 5, 6, 7])}</div>`
      : ''
  }

  <div class="two-col">
    <div class="card">
      <div class="card-head"><div class="card-title">状态码分布</div></div>
      ${codeRows.length ? tableHtml(['状态码', '占比', '次数', '比例'], codeRows, [2, 3]) : '<div class="card-sub">暂无数据</div>'}
    </div>
    <div class="card">
      <div class="card-head"><div class="card-title">错误类型聚合</div></div>
      ${errorTypeRows.length ? tableHtml(['错误类型', '占比', '次数'], errorTypeRows, [2]) : '<div class="card-sub">无错误记录</div>'}
    </div>
  </div>

  ${
    configRows.length
      ? `<div class="card"><div class="card-head"><div class="card-title">压测配置快照</div></div>
${tableHtml(['配置项', '取值'], configRows)}</div>`
      : ''
  }

  <div class="card">
    <div class="card-head"><div><div class="card-title">采样明细</div><div class="card-sub">按秒的并发、吞吐与延迟数据</div></div></div>
    ${
      sampleRows.length
        ? tableHtml(
            ['时间', '并发', 'RPS', '平均 RT', 'P50', 'P95', 'P99', 'TTFB', '流量', '成功', '失败'],
            sampleRows,
            [1, 2, 3, 4, 5, 6, 8, 9, 10],
          )
        : '<div class="card-sub">暂无采样数据</div>'
    }
  </div>

  <div class="card">
    <div class="card-head"><div><div class="card-title">请求明细</div><div class="card-sub">逐条记录每次请求的时间、耗时、状态与结果${
      htmlRequests.length < requests.length
        ? `，此处展示前 ${HTML_REQUEST_LIMIT} 条（共 ${requests.length} 条，完整数据请导出 Excel）`
        : `，共 ${htmlRequests.length} 条`
    }</div></div><span class="tag">${htmlRequests.length} 条</span></div>
    ${
      requestRows.length
        ? tableHtml(
            ['时间', '请求时间', '方法', 'URL', '请求体', '查询参数', '响应体', '状态码', '耗时(ms)', 'TTFB(ms)', '大小', '结果', '错误类型', '错误信息'],
            requestRows,
            [8, 9],
          )
        : '<div class="card-sub">暂无请求明细（旧版本报告不包含逐请求数据）</div>'
    }
  </div>

  <div class="card">
    <div class="card-head"><div><div class="card-title">错误明细</div><div class="card-sub">最多展示 500 条失败请求</div></div><span class="tag">${detail.errors.length} 条</span></div>
    ${
      errorRows.length
        ? tableHtml(['时间', '类型', '状态码', '错误信息', '请求地址', '响应内容'], errorRows)
        : '<div class="card-sub">无错误记录</div>'
    }
  </div>

  <div class="foot">由 FbinHttp 生成 · ${fmtTime(Date.now())}</div>
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

  const overview: CellValue[][] = [
    ['压测报告', report.planName],
    ['报告编号', report.id],
    ['执行状态', statusLabel(report.status)],
    ['开始时间', fmtTime(report.startTime)],
    ['结束时间', fmtTime(report.endTime)],
    ['持续时长', fmtDuration(report.startTime, report.endTime)],
    ['压测结论', verdict(detail).title],
    [],
    ['核心指标', '数值'],
    ['总请求数', report.totalRequests],
    ['成功请求数', report.successCount],
    ['失败请求数', report.errorCount],
    ['错误率(%)', report.errorRate],
    ['成功率(%)', Number((100 - report.errorRate).toFixed(4))],
    ['平均 RPS', report.avgQps],
    ['峰值 RPS', report.maxQps],
    ['目标 RPS', report.targetRps || '不限速'],
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
    ['时间(s)', '并发', 'RPS', '平均 RT(ms)', 'P50(ms)', 'P95(ms)', 'P99(ms)', 'TTFB(ms)', '流量(B)', '成功数', '失败数'],
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
    ['阶段', '并发', '时长(s)', '请求数', '错误数', '平均 RPS', '平均 RT(ms)', 'P95(ms)'],
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
        ['压测模式', config.mode === 'staged' ? '阶梯加压' : '固定并发'],
        ['请求方法', config.method ?? '-'],
        ['请求地址', config.url ?? '-'],
        ['请求头数量', config.headers ?? 0],
        ['参数数量', config.params ?? 0],
        ['请求体类型', config.bodyType ?? '-'],
        ['环境 ID', config.envId ?? '-'],
        ['超时时间(ms)', config.timeoutMs ?? '-'],
        ['思考时间(ms)', config.thinkTimeMs ?? '-'],
        ['加压时间(s)', config.rampUpSec ?? '-'],
        ['目标 RPS', config.targetRps || '不限速'],
        [
          '阶梯配置',
          config.stages?.length
            ? config.stages.map((stage) => `${stage.concurrent} 并发 / ${stage.durationSec}s`).join('，')
            : '-',
        ],
        ['SLA P95(ms)', config.sla?.p95Ms ?? '-'],
        ['SLA P99(ms)', config.sla?.p99Ms ?? '-'],
        ['SLA 错误率(%)', config.sla?.errorRate ?? '-'],
        ['SLA 最小 RPS', config.sla?.minRps ?? '-'],
        ['报告生成时间', fmtTime(Date.now())],
      ]
    : [['配置项', '取值'], ['说明', '该报告未保存配置快照']]

  return [
    { name: '概览', rows: overview },
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
