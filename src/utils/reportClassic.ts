/**
 * 经典报告导出（testreport 版式）
 *
 * 目标：输出一份与线上经典报告视觉一致的离线 HTML——
 * 同样的 1600px 版心、灰底表头、5 个横向 KPI 卡、echarts walden 配色的四张统计图、
 * 响应时间 & 状态分布双图，以及三张明细表。
 *
 * 实现说明：
 *   - 不引入 echarts（项目未安装），折线 / 柱状 / 饼图都用内联 SVG 按 walden 主题绘制；
 *   - 样式逐条对齐 testreport.css，配色取自线上 walden.js 主题；
 *   - 数据全部来自 AstroMan 自身的报告快照，缺失字段按经典报告的占位方式留空。
 */
import type { ReportDetail, ReportRequestRecord, ReportSample, UrlStat } from '@/types/models'
import { loadGeneratorsForReport, reportTitle, REPORT_DISCLAIMER } from './reportExport'
import { qpsOverview } from './reportAnalytics'
import { buildAdvice, buildFindings } from './reportNarrative'

/* ---------- 经典报告配色（echarts walden 主题） ---------- */

const WALDEN = ['#3fb1e3', '#6be6c1', '#626c91', '#a0a7e6', '#c4ebad', '#96dee8']
const ERROR_LINE = '#ff6e76'
const KPI_TEAL = '#008B8B'
const KPI_ORANGE = '#FF9D00'
const KPI_LIGHT_TEAL = '#20B2AA'

/** 与 testreport.css 对齐；仅补充 body 重置与 SVG 容器 */
const CLASSIC_CSS = `
[v-cloak]{display:none}
@media print{
  body{-webkit-print-color-adjust:exact;-moz-print-color-adjust:exact;-ms-print-color-adjust:exact;print-color-adjust:exact;color-adjust:exact}
}
body{
  margin:0;
  padding:30px 20px;
  background-color:#ffffff;
  color:#606266;
  font-size:14px;
  word-break:break-all;
  font-family:Roboto, San Francisco, "Helvetica Neue", Helvetica, Arial, PingFangSC-Light, "Hiragina Sans GB", "WenQuanYi Micro Hei", 'microsoft yahei ui', 'microsoft yahei', sans-serif;
}
.href{font-size:16px}
.data-table td{word-wrap:normal;white-space:nowrap;word-break:keep-all}
.block-div{margin-bottom:10px !important}
.top-title{text-align:center;font-size:24px;font-weight:bold;display:block;*padding-top:10px;*padding-bottom:10px}
.header-line{max-width:1360px;margin:0 auto}
.head-title{font-weight:bold;background:#FF9933;display:block;font-size:20px;padding-left:20px;padding-bottom:2px;padding-top:2px;margin-top:10px;margin-bottom:10px}
.sub-header-title{display:block;background:#C0C0C0;font-size:16px;padding-left:20px;padding-bottom:2px;padding-top:2px;margin-top:10px;margin-bottom:10px}
.sub-header-title2{background:#C0C0C0;font-size:16px;padding-left:20px;padding-bottom:2px;padding-top:2px;margin-top:10px;margin-bottom:10px}
.sub-title{font-size:32px;font-weight:bold}
.data-item{display:block;width:200px;margin-left:100px;text-align:right}
.data-item .subtitle{display:block;text-align:right}
.data-table{border:0;font-size:14px;width:900px}
.item-info{width:200px;font-size:16px;text-align:left}
.header-line-div{background-color:#ffffff;padding:5px 0 0 40px;color:#555}
.header{width:100%;display:block;font-size:12px;margin-bottom:20px}
.subtitle{padding-left:20px;display:block;font-size:16px;text-align:left;padding-right:20px}
.show-data-panel-two{display:-webkit-box;box-orient:horizontal;display:-moz-box;display:-webkit-box;*max-width:1360px;margin:0 auto;background-color:#ffffff}
.tc-panel{background-color:#fff;*box-shadow:0 2px 3px 0 rgba(0,0,0,.2);padding:20px;max-width:1360px;margin-left:auto;margin-right:auto;box-sizing:border-box}
.header-item{margin-bottom:20px;max-width:1360px;margin:0 auto;display:table;table-layout:fixed;width:100%;color:#454a51;font-size:14px;padding-top:15px;padding-bottom:15px;margin-top:10px;margin-bottom:10px;box-sizing:border-box;background-color:#fff;*box-shadow:0 2px 3px 0 rgba(0,0,0,.2);box-shadow:0 0 black}
.show-data-inner{display:table-row}
ol,ul{list-style:none;padding:0}
.show-data-panel .show-data-inner > li{display:table-cell;padding:5px 0 0 40px}
li{list-style:none}
.f_ta_c{text-align:center}
.show-data-panel .show-data-inner > li:first-child{border-left:none}
.num-font{font-family:qcloud-num !important;font-size:36px}
b,em,i{font-style:normal;font-weight:400}
.footer{margin-bottom:20px}
.report-sumary{margin-left:100px;font-size:16px}
p{text-indent:2em}
.client-table{font-size:12px;border-collapse:collapse;text-align:center;max-width:1600px;width:1600px}
.url-table{font-size:12px;border-collapse:collapse;text-align:center;word-break:keep-all;white-space:nowrap;max-width:1600px;min-width:830px}
.reportTableClass{*margin-left:10px;margin-top:20px;margin:0px auto}
.reportTableClass .tdnum{width:40px;text-align:right}
.reportTableClass th{text-align:center;border:2px solid #ffffff;padding:2px 5px;*white-space:nowrap;text-align:center;background:#C0C0C0}
.reportTableClass .thnum{*white-space:pre-wrap;width:60px}
.reportTableClass td{max-width:300px;word-wrap:break-word;white-space:pre-wrap;word-break:break-all;text-align:center;*border:1px solid #ddd;border:2px solid #ffffff;padding:2px 5px}
.reportTableClass .indexnum{width:30px;text-align:right}
hr{display:block;margin-block-start:0.5em;margin-block-end:0.5em;margin-inline-start:auto;margin-inline-end:auto;border-top-style:inset;border-top-width:1px}
.reportTableClass .requestid{width:200px;font-size:9px !important}
.cl-chart svg{display:block;width:100%}
.cl-empty{display:flex;align-items:center;justify-content:center;height:360px;color:#999999;font-size:14px}
`

/* ---------- 通用格式化 ---------- */

function esc(text: unknown): string {
  return String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

/** 毫秒时间戳 → YYYY-MM-DD HH:mm:ss，与经典报告 start_time 一致 */
function fmtSeconds(ms: number | null | undefined): string {
  if (!ms) return '—'
  const date = new Date(ms)
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(
    date.getHours(),
  )}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`
}

/** 经典报告 starttime + 秒数 → 完整时钟，用作时间轴标签 */
function wallClock(startMs: number, sec: number): string {
  return fmtSeconds(startMs + sec * 1000)
}

function plain(value: number | null | undefined): string {
  const n = Number(value ?? 0)
  return Number.isFinite(n) ? String(Math.round(n)) : '0'
}

function fixed(value: number | null | undefined, digits: number): string {
  const n = Number(value ?? 0)
  return (Number.isFinite(n) ? n : 0).toFixed(digits)
}

/** attr04 文案规则：≤120 秒用秒，<2 小时用分钟，其余用小时（保留 1 位） */
function durationText(totalSec: number): string {
  const sec = Math.max(0, Math.round(totalSec))
  if (sec <= 60 * 2) return `${sec}秒`
  if (sec < 60 * 120) return `${(sec / 60).toFixed(1)}分钟`
  return `${(sec / 3600).toFixed(1)}小时`
}

function elapsedSeconds(detail: ReportDetail): number {
  const { startTime, endTime } = detail.report
  if (!startTime || !endTime || endTime <= startTime) return 0
  return Math.round((endTime - startTime) / 1000)
}

/** 目标主机：多接口报告写「全部接口」，单接口展示不含 query 的完整地址 */
function targetHost(detail: ReportDetail, requests: ReportRequestRecord[]): string {
  const count = detail.config?.requestCount ?? 0
  if (count > 1) return `全部接口（共 ${count} 个）`
  const url =
    detail.config?.url || detail.config?.target?.resolvedUrl || requests[0]?.url || ''
  if (!url) return detail.config?.agent?.host || '—'
  return url.split('?')[0].split('#')[0]
}

/** 并发数量：阶梯压测展示 min~max，与经典报告一致 */
function concurrencyText(detail: ReportDetail): string {
  const values = (detail.config?.stages ?? []).map((stage) => stage.concurrent).filter((v) => v > 0)
  if (values.length >= 2) {
    const min = Math.min(...values)
    const max = Math.max(...values)
    return min === max ? String(max) : `${min}~${max}`
  }
  return String(detail.report.concurrency ?? 0)
}

/** 多行纯文本 → 经典报告单段落内容（换行转 <br/>，与 v-html 的观感一致） */
function inlineText(text: string): string {
  return esc(text.trim()).replace(/\n/g, '<br/>')
}

/* ---------- SVG 图表（walden 主题） ---------- */

function niceCeil(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1
  const exp = Math.floor(Math.log10(value))
  const base = 10 ** exp
  const n = value / base
  const step = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find((item) => n <= item) ?? 10
  return step * base
}

function axisLabel(value: number): string {
  if (value >= 1000) return String(Math.round(value))
  if (value >= 10) return String(Math.round(value))
  if (value >= 1) return value.toFixed(1).replace(/\.0$/, '')
  return value.toFixed(2).replace(/\.?0+$/, '') || '0'
}

/** Catmull-Rom 转三次贝塞尔，复刻 echarts 的 smooth 折线
 *
 * 控制点会被夹在 top..bottom 之间：样条本身会过冲，不夹紧时曲线会画到绘图区外。
 */
function smoothPath(points: Array<[number, number]>, top: number, bottom: number): string {
  if (!points.length) return ''
  if (points.length === 1) return `M${points[0][0]},${points[0][1]}`
  const clampY = (value: number) => Math.min(bottom, Math.max(top, value))
  let d = `M${points[0][0]},${points[0][1]}`
  for (let i = 0; i < points.length - 1; i += 1) {
    const p0 = points[i - 1] ?? points[i]
    const p1 = points[i]
    const p2 = points[i + 1]
    const p3 = points[i + 2] ?? p2
    const c1x = p1[0] + (p2[0] - p0[0]) / 6
    const c1y = clampY(p1[1] + (p2[1] - p0[1]) / 6)
    const c2x = p2[0] - (p3[0] - p1[0]) / 6
    const c2y = clampY(p2[1] - (p3[1] - p1[1]) / 6)
    d += ` C${c1x.toFixed(2)},${c1y.toFixed(2)} ${c2x.toFixed(2)},${c2y.toFixed(2)} ${p2[0]},${p2[1]}`
  }
  return d
}

interface ChartPoint {
  label: string
  value: number
}

/** 采样点过多时等距抽稀，保留首尾 */
function thin(points: ChartPoint[], maxPoints = 240): ChartPoint[] {
  if (points.length <= maxPoints) return points
  const step = (points.length - 1) / (maxPoints - 1)
  return Array.from({ length: maxPoints }, (_, index) => points[Math.round(index * step)])
}

interface LineChartOptions {
  legend: string
  color: string
  points: ChartPoint[]
  smooth: boolean
  yMax?: number
}

function lineChartSvg(options: LineChartOptions): string {
  const { legend, color, smooth } = options
  const points = thin(options.points)
  const W = 1320
  // 竖排完整日期时间约需 125px，画布加高 40px、底部留白同步加大，绘图区高度保持不变
  const H = 400
  // 左边距给纵轴刻度留白；稍宽一点，横轴首个旋转标签就不用右移太多，刻度能放得更密
  const L = 74
  const R = 30
  const T = 50
  const B = 150
  const pw = W - L - R
  const ph = H - T - B
  const rawMax = Math.max(0, ...points.map((p) => p.value))
  // 纵轴上限：显式上限只作为「最小刻度」，实际峰值仍要能装下，避免曲线画到图外
  const maxValue = Math.max(options.yMax ?? 0, niceCeil(rawMax * 1.08))
  const scaleY = (value: number) => {
    const ratio = Math.min(1, Math.max(0, value) / maxValue)
    return T + ph - ratio * ph
  }
  const scaleX = (index: number) =>
    points.length <= 1 ? L + pw / 2 : L + (index / (points.length - 1)) * pw

  const coords = points.map((point, index) => [scaleX(index), scaleY(point.value)] as [number, number])
  const line = smooth
    ? smoothPath(coords, T, T + ph)
    : coords.map(([x, y], i) => `${i ? 'L' : 'M'}${x},${y}`).join('')

  const grid = Array.from({ length: 6 }, (_, index) => {
    const value = (maxValue / 5) * index
    const y = scaleY(value)
    return `<line x1="${L}" y1="${y}" x2="${W - R}" y2="${y}" stroke="#eeeeee" stroke-width="1"/>
<text x="${L - 8}" y="${y}" text-anchor="end" dominant-baseline="middle" fill="#999999" font-size="12">${axisLabel(
      value,
    )}</text>`
  }).join('')

  // 横轴标签向左下倾斜 60°，按完整日期时间的实际宽度自动计算能放下多少个，保证互不重叠
  const labelAngle = 60
  const labelRad = (labelAngle * Math.PI) / 180
  const sampleLabel = points.reduce((longest, point) => (point.label.length > longest.length ? point.label : longest), '')
  const labelWidth = Math.max(48, sampleLabel.length * 6.6)
  const footprint = Math.ceil(labelWidth * Math.cos(labelRad) + 12 * Math.sin(labelRad))
  const perLabel = footprint + 10
  const tickCount = Math.max(2, Math.min(points.length, Math.floor(pw / perLabel) + 1))
  const tickIndices = [
    ...new Set(
      Array.from({ length: tickCount }, (_, index) =>
        Math.round((index / Math.max(1, tickCount - 1)) * (points.length - 1)),
      ),
    ),
  ]
  // 横轴每秒一个短刻度线（数据抽稀后按实际采样点画），文字标签按上面的数量等距挑选
  const minorTicks = points
    .map(
      (_, index) =>
        `<line x1="${scaleX(index)}" y1="${T + ph}" x2="${scaleX(index)}" y2="${
          T + ph + 4
        }" stroke="#cccccc" stroke-width="1"/>`,
    )
    .join('')
  const majorTicks = tickIndices
    .map(
      (index) =>
        `<line x1="${scaleX(index)}" y1="${T + ph}" x2="${scaleX(index)}" y2="${
          T + ph + 8
        }" stroke="#999999" stroke-width="1"/>`,
    )
    .join('')
  const xLabels = tickIndices
    .map((index) => {
      const point = points[index]
      const x = scaleX(index)
      const y = T + ph + 10
      return `<text x="${x}" y="${y}" text-anchor="end" transform="rotate(-${labelAngle} ${x} ${y})" fill="#999999" font-size="12">${esc(
        point.label,
      )}</text>`
    })
    .join('')

  return `<div class="cl-chart"><svg viewBox="0 0 ${W} ${H}" width="100%" height="${H}" role="img" aria-label="${esc(
    legend,
  )}">
<line x1="${L}" y1="20" x2="${L + 16}" y2="20" stroke="${color}" stroke-width="2"/>
<text x="${L + 22}" y="24" fill="#999999" font-size="12">${esc(legend)}</text>
${grid}
<line x1="${L}" y1="${T}" x2="${L}" y2="${T + ph}" stroke="#cccccc" stroke-width="1"/>
<line x1="${L}" y1="${T + ph}" x2="${W - R}" y2="${T + ph}" stroke="#cccccc" stroke-width="1"/>
${minorTicks}
${majorTicks}
<path d="${line}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>
${xLabels}
</svg></div>`
}

interface BarRow {
  label: string
  value: number
}

/** 左侧 URL 标签折行：超过一行可容纳的长度时按 / 断开，保证完整路径可读 */
function wrapUrl(url: string, maxChars: number): string[] {
  if (url.length <= maxChars) return [url]
  const half = Math.ceil(url.length / 2)
  const slash = url.lastIndexOf('/', half)
  const cut = slash > half * 0.6 ? slash + 1 : half
  return [url.slice(0, cut), url.slice(cut)]
}

/** 横向柱状图：复刻「耗时Top10」；接口全路径作为左侧类目标签，避免被截断 */
function barChartSvg(rows: BarRow[]): string {
  const W = 660
  const H = 360
  const L = 330
  const R = 46
  const T = 58
  const B = 44
  const pw = W - L - R
  const ph = H - T - B
  const maxValue = niceCeil(Math.max(0, ...rows.map((row) => row.value)))
  const band = ph / Math.max(1, rows.length)
  const barHeight = Math.min(12, band * 0.5)

  const grid = Array.from({ length: 6 }, (_, index) => {
    const value = (maxValue / 5) * index
    const x = L + (value / maxValue) * pw
    return `<line x1="${x}" y1="${T}" x2="${x}" y2="${T + ph}" stroke="#eeeeee" stroke-width="1"/>
<text x="${x}" y="${T + ph + 18}" text-anchor="middle" fill="#999999" font-size="12">${axisLabel(
      value,
    )}</text>`
  }).join('')

  const bars = rows
    .map((row, index) => {
      const centerY = T + band * index + band / 2
      const y = centerY - barHeight / 2
      const width = Math.max(1, (row.value / maxValue) * pw)
      const lines = wrapUrl(row.label, 46)
      const labels = lines
        .map(
          (line, lineIndex) =>
            `<text x="${L - 8}" y="${
              centerY + (lineIndex - (lines.length - 1) / 2) * 13 + 4.2
            }" text-anchor="end" fill="#606266" font-size="12">${
              lineIndex === 0 ? `<title>${esc(row.label)}</title>` : ''
            }${esc(line)}</text>`,
        )
        .join('')
      const valueOutside = width < 56
      const valueX = valueOutside ? L + width + 6 : L + width - 6
      const valueAnchor = valueOutside ? 'start' : 'end'
      const valueColor = valueOutside ? '#606266' : '#ffffff'
      return `${labels}
<rect x="${L}" y="${y}" width="${width}" height="${barHeight}" fill="${WALDEN[0]}"/>
<text x="${valueX}" y="${centerY}" text-anchor="${valueAnchor}" dominant-baseline="middle" fill="${valueColor}" font-size="10">${row.value.toFixed(
        4,
      )}</text>`
    })
    .join('')

  return `<div class="cl-chart"><svg viewBox="0 0 ${W} ${H}" width="100%" height="${H}" role="img" aria-label="耗时Top10">
<text x="4" y="24" fill="#666666" font-size="18">耗时Top10</text>
<rect x="150" y="17" width="14" height="8" fill="${WALDEN[0]}"/>
<text x="170" y="24" fill="#999999" font-size="12">平均请求耗时(成功请求)</text>
${grid}
<line x1="${L}" y1="${T}" x2="${L}" y2="${T + ph}" stroke="#cccccc" stroke-width="1"/>
<line x1="${L}" y1="${T + ph}" x2="${W - R}" y2="${T + ph}" stroke="#cccccc" stroke-width="1"/>
${bars}
<text x="${L + pw / 2}" y="${H - 10}" text-anchor="middle" fill="#999999" font-size="12">平均耗时（秒）</text>
</svg></div>`
}

interface PieSlice {
  name: string
  value: number
}

function arcPath(cx: number, cy: number, r: number, start: number, end: number): string {
  const x1 = cx + r * Math.cos(start)
  const y1 = cy + r * Math.sin(start)
  const x2 = cx + r * Math.cos(end)
  const y2 = cy + r * Math.sin(end)
  const large = end - start > Math.PI ? 1 : 0
  return `M${cx},${cy} L${x1.toFixed(2)},${y1.toFixed(2)} A${r},${r} 0 ${large} 1 ${x2.toFixed(
    2,
  )},${y2.toFixed(2)} Z`
}

/** 饼图：复刻「http请求状态分布」，标题居中、图例竖排在左 */
function pieChartSvg(slices: PieSlice[]): string {
  const W = 360
  const H = 360
  const cx = W / 2
  const cy = 216
  const r = 99
  const total = slices.reduce((sum, slice) => sum + slice.value, 0)

  const legend = slices
    .map(
      (slice, index) =>
        `<circle cx="10" cy="${64 + index * 20}" r="5" fill="${WALDEN[index % WALDEN.length]}"/>
<text x="22" y="${68 + index * 20}" fill="#999999" font-size="12">${esc(slice.name)}</text>`,
    )
    .join('')

  let angle = -Math.PI / 2
  const paths = slices
    .map((slice, index) => {
      const ratio = total > 0 ? slice.value / total : 0
      const start = angle
      const end = angle + ratio * Math.PI * 2
      angle = end
      const color = WALDEN[index % WALDEN.length]
      // 单一片 100% 时弧线首尾重合，改画整圆
      const shape =
        ratio >= 0.9999
          ? `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${color}"/>`
          : `<path d="${arcPath(cx, cy, r, start, end)}" fill="${color}"/>`
      if (ratio < 0.05) return shape
      const mid = (start + end) / 2
      const lx = cx + Math.cos(mid) * (r + 16)
      const ly = cy + Math.sin(mid) * (r + 16)
      const anchor = Math.cos(mid) >= 0 ? 'start' : 'end'
      return `${shape}
<text x="${lx.toFixed(2)}" y="${ly.toFixed(2)}" text-anchor="${anchor}" dominant-baseline="middle" fill="#666666" font-size="12">${esc(
        slice.name,
      )}</text>`
    })
    .join('')

  return `<div class="cl-chart"><svg viewBox="0 0 ${W} ${H}" width="100%" height="${H}" role="img" aria-label="http请求状态分布">
<text x="${cx}" y="24" text-anchor="middle" fill="#666666" font-size="18">http请求状态分布</text>
${legend}
${paths}
</svg></div>`
}

/* ---------- 数据整理 ---------- */

function samplePoints(
  samples: ReportSample[],
  startMs: number,
  pick: (sample: ReportSample) => number,
): ChartPoint[] {
  return samples.map((sample) => ({
    label: wallClock(startMs, sample.elapsedSec),
    value: pick(sample),
  }))
}

function topLatencyRows(detail: ReportDetail): BarRow[] {
  const stats: UrlStat[] = detail.urlStats ?? []
  if (stats.length) {
    return [...stats]
      .filter((stat) => stat.avgMs > 0)
      .sort((a, b) => b.avgMs - a.avgMs)
      .slice(0, 10)
      .map((stat) => ({ label: stat.url, value: stat.avgMs / 1000 }))
  }
  // 旧报告没有 urlStats 时退化为慢请求快照按地址聚合
  const grouped = new Map<string, { total: number; count: number }>()
  for (const item of detail.slowRequests ?? []) {
    const entry = grouped.get(item.url) ?? { total: 0, count: 0 }
    entry.total += item.latencyMs
    entry.count += 1
    grouped.set(item.url, entry)
  }
  return [...grouped.entries()]
    .map(([url, entry]) => ({ label: url, value: entry.total / entry.count / 1000 }))
    .sort((a, b) => b.value - a.value)
    .slice(0, 10)
}

const MODE_TEXT: Record<string, string> = {
  fixed: '固定并发',
  stages: '阶梯加压',
  cluster: '分布式压测',
}

/* ---------- 区块渲染 ---------- */

function kpiItem(value: string, unit: string, label: string, color: string): string {
  return `<li class="f_ta_c f_pd_l0">
  <p><em class="num-font" style="color:${color};">${value}</em><span>${unit}</span></p>
  <p>${label}</p>
</li>`
}

function workModeBlock(detail: ReportDetail, requests: ReportRequestRecord[]): string {
  const report = detail.report
  const config = detail.config ?? undefined
  const clients = Math.max(1, loadGeneratorsForReport(detail).length)
  const rows: Array<[string, string, string, string]> = [
    ['测试任务编号:', esc(report.id), '测试开始时间:', fmtSeconds(report.startTime)],
    ['测试机数量:', `${clients}台`, '测试持续时间:', durationText(elapsedSeconds(detail))],
    [
      '并发间隔时间:',
      `${config?.rampUpSec ?? 0}秒`,
      '思考时间:',
      `${((config?.thinkTimeMs ?? 0) / 1000).toFixed(0)}秒`,
    ],
    ['目标主机:', esc(targetHost(detail, requests)), '并发数量:', esc(concurrencyText(detail))],
  ]
  return `<div class="data-item">
<table class="data-table">
${rows
  .map(
    (row) =>
      `<tr><td>${row[0]}</td><td>${row[1]}</td><td>${row[2]}</td><td>${row[3]}</td></tr>`,
  )
  .join('\n')}
</table>
</div>`
}

function summaryBlock(detail: ReportDetail): string {
  const text = (detail.summaryText ?? '').trim()
  if (text) return inlineText(text)
  const findings = buildFindings(detail).slice(0, 4)
  const advice = buildAdvice(detail).slice(0, 3)
  if (!findings.length && !advice.length) return '暂无'
  const blocks = findings.map(
    (finding) => `<b>${esc(finding.title)}</b>：${esc(finding.detail)}`,
  )
  if (advice.length) {
    blocks.push('<b>优化建议</b>')
    for (const item of advice) {
      blocks.push(`${esc(item.priority)} ${esc(item.title)}：${esc(item.detail)}`)
    }
  }
  return blocks.join('<br/>')
}

function clientRowsHtml(detail: ReportDetail): string {
  const nodes = loadGeneratorsForReport(detail)
  if (!nodes.length) return '<tr><td colspan="11">暂无施压机数据</td></tr>'
  return nodes
    .map((node, index) => {
      const total = node.totalRequests ?? 0
      const success = node.successCount ?? 0
      const failed = node.errorCount ?? 0
      const rate = total > 0 ? (success / total) * 100 : 0
      return `<tr>
  <td class="indexnum">${index + 1}</td>
  <td>${esc(node.addr || node.name || '—')}</td>
  <td>${fmtSeconds(node.startMs)}</td>
  <td class="tdnum">${plain(total)}</td>
  <td class="tdnum">${plain(success)}</td>
  <td class="tdnum">${plain(failed)}</td>
  <td class="tdnum">${plain(node.concurrent)}</td>
  <td class="tdnum">${fixed(rate, 1)}%</td>
  <td class="tdnum">${fixed((node.avgRtMs ?? 0) / 1000, 5)}</td>
  <td class="tdnum">${plain(node.tps ?? node.qpsAvg)}</td>
  <td class="tdnum">${fixed(node.egressAvgKbps ?? 0, 1)}</td>
</tr>`
    })
    .join('')
}

function urlRowsHtml(detail: ReportDetail): string {
  const stats = detail.urlStats ?? []
  if (!stats.length) return '<tr><td colspan="11">暂无地址统计</td></tr>'
  return stats
    .map(
      (stat, index) => `<tr>
  <td class="indexnum">${index + 1}</td>
  <td style="text-align:left;min-width:600px">${esc(stat.url)}</td>
  <td>${stat.statusCode ? esc(stat.statusCode) : '—'}</td>
  <td class="tdnum">${plain(stat.requests)}</td>
  <td class="tdnum">${fixed(stat.avgMs / 1000, 2)}</td>
  <td class="tdnum">${fixed(stat.maxMs / 1000, 2)}</td>
  <td class="date">${fmtSeconds(stat.maxAtMs ?? null)}</td>
  <td class="requestid"></td>
  <td class="tdnum">${fixed(stat.totalHours, 1)}</td>
  <td class="tdnum">${fixed(stat.avgKb, 1)}</td>
  <td class="tdnum">${fixed(stat.totalMb, 1)}</td>
</tr>`,
    )
    .join('')
}

interface ErrorRow {
  url: string
  status: string
  count: number
  avg: string
  max: string
  at: string
  message: string
}

function errorRows(detail: ReportDetail): ErrorRow[] {
  const stats = (detail.urlStats ?? []).filter((stat) => stat.errorCount > 0)
  if (stats.length) {
    return stats.map((stat) => ({
      url: stat.url,
      status: stat.statusCode ? String(stat.statusCode) : '—',
      count: stat.errorCount,
      avg: fixed(stat.avgMs / 1000, 2),
      max: fixed(stat.maxMs / 1000, 2),
      at: fmtSeconds(stat.maxAtMs ?? null),
      message: stat.errorMessage || stat.statusSummary || '—',
    }))
  }
  // 老报告没有 urlStats：按地址 + 状态码聚合原始错误记录
  const grouped = new Map<string, ErrorRow>()
  for (const item of detail.errors ?? []) {
    const key = `${item.requestUrl}#${item.responseStatus ?? 0}`
    const exists = grouped.get(key)
    if (exists) {
      exists.count += 1
      continue
    }
    grouped.set(key, {
      url: item.requestUrl,
      status: item.responseStatus ? String(item.responseStatus) : '—',
      count: 1,
      avg: '—',
      max: '—',
      at: '—',
      message: item.errorMessage || item.errorType || '—',
    })
  }
  return [...grouped.values()]
}

function errorRowsHtml(rows: ErrorRow[]): string {
  return rows
    .map(
      (row, index) => `<tr>
  <td class="indexnum">${index + 1}</td>
  <td style="text-align:left;min-width:600px">${esc(row.url)}</td>
  <td>${esc(row.status)}</td>
  <td class="tdnum">${plain(row.count)}</td>
  <td class="tdnum">${esc(row.avg)}</td>
  <td class="tdnum">${esc(row.max)}</td>
  <td class="date">${esc(row.at)}</td>
  <td class="requestid"></td>
  <td style="text-align:left;min-width:450px">${esc(row.message)}</td>
</tr>`,
    )
    .join('')
}

/* ---------- 主入口 ---------- */

/**
 * 生成经典报告版式的自包含 HTML。
 *
 * @param detail 报告详情快照
 * @param requests 首屏请求明细，仅在 config.url 缺失时用于兜底目标主机
 */
export function buildClassicReportHtml(
  detail: ReportDetail,
  requests: ReportRequestRecord[] = [],
): string {
  const report = detail.report
  const samples = detail.samples ?? []
  const startMs = report.startTime ?? 0
  const qps = qpsOverview(detail)
  const total = report.totalRequests ?? 0
  const success = report.successCount ?? 0

  const rateText = fixed(total > 0 ? (success / total) * 100 : 0, 1)
  const [rateInt, rateDec] = rateText.split('.')
  const successRateValue = `${rateInt}.<span style="font-size:20px">${rateDec ?? '0'}</span>`

  const totalWan = fixed(total / 10000, 2)
  const trafficMb = fixed((report.bytesTotal ?? 0) / 1024 / 1024, 1)
  const maxQps = Math.max(report.peakQps ?? 0, report.maxQps ?? 0)
  const ninetyQps = samples.length ? qps.ninety : 0

  const summaryItems = [
    kpiItem(totalWan, '(万)个', '总请求次数', KPI_TEAL),
    kpiItem(successRateValue, '%', '成功率', KPI_ORANGE),
    kpiItem(trafficMb, 'MB', '外网总流量(入+出)', KPI_TEAL),
    kpiItem(plain(maxQps), 'QPS', '最大QPS', KPI_ORANGE),
    kpiItem(plain(ninetyQps), 'QPS', '90%分布QPS', KPI_LIGHT_TEAL),
  ].join('\n')

  const qpsPoints = samplePoints(samples, startMs, (sample) => sample.qps)
  const errorPoints = samplePoints(samples, startMs, (sample) => sample.errorCount)
  const concurrentPoints = samplePoints(samples, startMs, (sample) => sample.concurrent)
  const latencyPoints = samplePoints(samples, startMs, (sample) => sample.avgRt / 1000)
  const latencyMean =
    latencyPoints.reduce((sum, point) => sum + point.value, 0) / Math.max(1, latencyPoints.length)
  const latencyMax = Math.max(0.5, Math.ceil(latencyMean * 2 * 10) / 10)

  const topRows = topLatencyRows(detail)
  const statusSlices = (detail.statusCodes ?? []).map((item) => ({
    name: String(item.code),
    value: item.count,
  }))
  const errors = errorRows(detail)

  const scene = (detail.scenarioText ?? '').trim()
  const modeText = MODE_TEXT[detail.config?.mode ?? ''] ?? detail.config?.mode ?? '固定并发'
  const target = targetHost(detail, requests)
  const sceneText = scene ? inlineText(scene) : `按${esc(modeText)}模式对 ${esc(target)} 发起压测。`

  const chartPanel = (id: string, width: string, body: string) =>
    `<div id="${id}" class="tc-panel" style="width:${width};height:400px;margin:0 auto;">${body}</div>`

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(reportTitle(report))}-${esc(report.id)}</title>
<style>${CLASSIC_CSS}</style>
</head>
<body>
<div id="app" style="width:1600px;margin:auto;">
  <div class="header">
    <div>
      <div>
        <span class="top-title" style="margin-left:30px;">${esc(reportTitle(report))}</span>
      </div>
    </div>

    <div class="block-div" style="margin-bottom:10px !important;">
      <div class="sub-title"><span class="sub-header-title">测试场景</span></div>
      <div style="margin:auto;margin-top:20px;">
        <p style="font-size:16px;line-height:2em;padding-left:20px;padding-right:20px;">${sceneText}</p>
      </div>
    </div>

    <div class="block-div">
      <div class="sub-title"><span class="sub-header-title">工作模式</span></div>
      ${workModeBlock(detail, requests)}
    </div>

    <div>
      <div class="sub-title"><span class="sub-header-title">结果汇总</span></div>
      <div class="header-item block-div show-data-panel" style="padding:0px;">
        <ul class="show-data-inner">
${summaryItems}
        </ul>
      </div>
    </div>

    <div class="PageNext1">
      <div class="sub-title"><span class="sub-header-title">统计分析</span></div>
      <div class="show-data-panel block-div">
        ${chartPanel(
          'timeEChart',
          '1360px',
          samples.length
            ? lineChartSvg({
                legend: '成功QPS(每秒查询率)',
                color: WALDEN[0],
                points: qpsPoints,
                smooth: true,
              })
            : '<div class="cl-empty">暂无采样数据</div>',
        )}
        ${chartPanel(
          'errortimeEChart',
          '1360px',
          samples.length
            ? lineChartSvg({
                legend: '失败QPS(每秒查询率)',
                color: ERROR_LINE,
                points: errorPoints,
                smooth: true,
              })
            : '<div class="cl-empty">暂无采样数据</div>',
        )}
        ${chartPanel(
          'bingfaEChart',
          '1360px',
          samples.length
            ? lineChartSvg({
                legend: '并发请求数',
                color: WALDEN[0],
                points: concurrentPoints,
                smooth: false,
              })
            : '<div class="cl-empty">暂无采样数据</div>',
        )}
        ${chartPanel(
          'urldetailtimeEChart',
          '1360px',
          samples.length
            ? lineChartSvg({
                legend: '平均响应时间',
                color: WALDEN[0],
                points: latencyPoints,
                smooth: true,
                yMax: latencyMax,
              })
            : '<div class="cl-empty">暂无采样数据</div>',
        )}
      </div>
    </div>

    <div class="block-div PageNext">
      <div class="sub-title"><span class="sub-header-title">响应时间&amp;状态分布</span></div>
      <div class="show-data-panel block-div show-data-panel-two" style="width:100%;">
        <div style="box-orient:horizontal;-moz-box-flex:3;-webkit-box-flex:3;box-flex:3;">
          ${chartPanel(
            'urlEChart',
            '700px',
            topRows.length
              ? barChartSvg(topRows)
              : '<div class="cl-empty">暂无地址耗时数据</div>',
          )}
        </div>
        <div style="box-orient:horizontal;-moz-box-flex:1;-webkit-box-flex:1;box-flex:1;">
          ${chartPanel(
            'typeEchart',
            '400px',
            statusSlices.length
              ? pieChartSvg(statusSlices)
              : '<div class="cl-empty">暂无状态数据</div>',
          )}
        </div>
      </div>
    </div>

    <div class="block-div PageNext1">
      <div class="sub-title"><span class="sub-header-title">测试机(客户端)详情</span></div>
      <table class="client-table reportTableClass" id="reportTable2">
        <tr class="thead">
          <th style="width:30px">序号</th>
          <th style="width:100px;">配置信息</th>
          <th style="width:90px">开始运行时间</th>
          <th class="thnum">总请求数</th>
          <th class="thnum">成功次数</th>
          <th class="thnum">失败次数</th>
          <th class="thnum">并发数量</th>
          <th class="thnum">成功率</th>
          <th class="thnum">平均耗时(秒)</th>
          <th class="thnum">事务数/秒(TPS)</th>
          <th class="thnum">出站带宽平均值(KB/s)</th>
        </tr>
        ${clientRowsHtml(detail)}
      </table>
    </div>

    <div class="block-div PageNext1">
      <div class="sub-title"><span class="sub-header-title">测试地址详情</span></div>
      <table class="url-table reportTableClass" id="reportTable3">
        <tr>
          <th class="indexnum">序号</th>
          <th>地址</th>
          <th>HTTP Code</th>
          <th class="thnum">请求次数</th>
          <th class="thnum">平均耗时</th>
          <th class="thnum">最长耗时(秒)</th>
          <th class="date">最长耗时日期</th>
          <th class="requestid">最长耗时请求ID</th>
          <th class="thnum">总耗时(小时)</th>
          <th class="thnum">数据包平均(KB)</th>
          <th class="thnum">数据包总计(MB)</th>
        </tr>
        ${urlRowsHtml(detail)}
      </table>
    </div>
    ${
      errors.length
        ? `<div class="block-div PageNext1">
      <div class="sub-title"><span class="sub-header-title">错误的测试地址详情</span></div>
      <table class="url-table reportTableClass" id="reportTable-errorurl">
        <tr>
          <th class="indexnum">序号</th>
          <th>地址</th>
          <th>HTTP Code</th>
          <th class="thnum">总数</th>
          <th class="thnum">平均耗时</th>
          <th class="thnum">最长耗时(秒)</th>
          <th class="date">最长耗时日期</th>
          <th class="requestid">最长耗时请求ID</th>
          <th>错误信息</th>
        </tr>
        ${errorRowsHtml(errors)}
      </table>
    </div>`
        : ''
    }

    <div class="block-div" style="margin-bottom:100px !important;">
      <div class="sub-title"><span class="sub-header-title">报告总结</span></div>
      <div style="margin:auto;margin-top:20px;">
        <p style="font-size:16px;line-height:2em;padding-left:20px;padding-right:20px;">${summaryBlock(
          detail,
        )}</p>
      </div>
    </div>
  </div>
  <hr/>
  <span style="display:block;text-align:right;width:100%;margin-right:20px;font-size:12px;color:#555;">免责声明:${esc(
    REPORT_DISCLAIMER,
  )}</span>
</div>
</body>
</html>`
}
