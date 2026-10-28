import type {
  CollectionNode,
  DashboardMetrics,
  Environment,
  EnvVariable,
  HistoryItem,
  HistoryStats,
  HttpRequestDetail,
  HttpResult,
  ReportDetail,
  RequestSearchHit,
  StressStage,
  WorkspaceStats,
} from '@/types/models'

function clip(text: string, max = 8000): string {
  return text.length > max ? `${text.slice(0, max)}\n…（已截断）` : text
}

function block(value: unknown, max = 8000): string {
  try {
    return clip(JSON.stringify(value, null, 2), max)
  } catch {
    return String(value)
  }
}

function kv(items: { key: string; value: string; enabled?: boolean }[] | undefined): string {
  const rows = (items ?? [])
    .filter((item) => item.enabled !== false && String(item.key).trim())
    .map((item) => `${item.key}: ${item.value || '（空）'}`)
  return rows.length ? rows.join('\n') : '（无）'
}

function detailBlock(detail: HttpRequestDetail): string {
  const auth = detail.auth
  const authText =
    auth && auth.kind !== 'none'
      ? block(
          {
            ...auth,
            token: auth.token ? '******' : '',
            password: auth.password ? '******' : '',
            clientSecret: auth.clientSecret ? '******' : '',
          },
          800,
        )
      : '（无）'
  return [
    `名称：${detail.name || '未命名'}`,
    `方法：${detail.method}`,
    `URL：${detail.url}`,
    `请求头：\n${kv(detail.headers)}`,
    `查询参数：\n${kv(detail.params)}`,
    `请求体类型：${detail.bodyType || 'none'}`,
    `请求体：\n${detail.body ? clip(detail.body, 3000) : '（空）'}`,
    `前置脚本：\n${detail.preScript ? clip(detail.preScript, 2000) : '（无）'}`,
    `后置脚本：\n${detail.postScript ? clip(detail.postScript, 2000) : '（无）'}`,
    `断言：\n${detail.assertions ? clip(detail.assertions, 1200) : '（无）'}`,
    `认证：${authText}`,
  ].join('\n')
}

function resultBlock(result: HttpResult | null): string {
  if (!result) {
    return '还没有发送过请求，没有响应数据。只能基于请求定义做静态分析，不要假设响应内容。'
  }
  return [
    `状态码：${result.status} ${result.statusText || ''}`.trim(),
    `耗时：${result.durationMs} ms`,
    `响应大小：${result.sizeBytes} 字节`,
    result.error ? `网络错误：${result.error}` : '网络错误：无',
    `响应头：\n${Object.entries(result.headers ?? {})
      .map(([key, value]) => `${key}: ${value}`)
      .join('\n') || '（无）'}`,
    `响应体：\n${result.body ? clip(result.body, 6000) : '（空）'}`,
    result.truncated ? '提示：响应体已截断。' : '',
  ]
    .filter(Boolean)
    .join('\n')
}

/* ---------------- 单请求场景 ---------------- */

export function requestExplainPrompt(detail: HttpRequestDetail, result: HttpResult | null): string {
  return `请分析下面这次接口调用。

【请求】
${detailBlock(detail)}

【响应】
${resultBlock(result)}

请输出：
1. 结论：这次调用是否成功，状态码含义；
2. 关键响应字段解读（有响应体时）；
3. 可能存在的问题、风险或异常（没有就写「未见明显异常」）；
4. 下一步建议（最多 3 条）。`
}

export function requestAssertionPrompt(detail: HttpRequestDetail, result: HttpResult | null): string {
  return `请为下面的接口生成可直接粘贴到「断言」里的脚本。

【请求】
${detailBlock(detail)}

【最近一次响应】
${resultBlock(result)}

要求：
1. 用 FbinHttp 断言脚本语法：test('名称', () => { assert(条件, '失败说明') })；
2. 覆盖状态码、业务码、关键字段类型或非空、必要字段存在性；
3. 只输出一个 \`\`\`js 代码块，每行加简短中文注释，最多 20 行；
4. 响应体结构不明确的地方用条件判断兜底，不要编造字段。`
}

export function requestPostScriptPrompt(detail: HttpRequestDetail, result: HttpResult | null): string {
  return `请为下面的接口生成后置脚本，用于提取变量供后续请求使用。

【请求】
${detailBlock(detail)}

【最近一次响应】
${resultBlock(result)}

要求：
1. 使用 FbinHttp 脚本引擎：response.status / response.json() / response.text()、vars.set()；
2. 先判断响应是否成功，再提取 token、id 等关键字段，字段不存在时给出提示；
3. 只输出一个 \`\`\`js 代码块，不超过 25 行，关键步骤写中文注释；
4. 提醒我把密钥类变量放环境变量。`
}

export function requestOptimizePrompt(detail: HttpRequestDetail): string {
  return `请检查这个接口定义，给出可执行的优化建议。

【请求】
${detailBlock(detail)}

请按「请求设计 / 参数与请求头 / 认证与安全 / 脚本与断言 / 可维护性」分点说明：
1. 现在有什么问题或隐患；
2. 具体怎么改（能给示例就给示例）；
3. 按优先级排序，最多 8 条，不要泛泛而谈。`
}

/* ---------------- 历史记录 ---------------- */

export function historyDiagnosePrompt(items: HistoryItem[], stats: HistoryStats | null): string {
  const failed = items.filter(
    (item) => !item.statusCode || item.statusCode >= 400,
  )
  const sorted = [...items].sort((a, b) => (b.durationMs ?? 0) - (a.durationMs ?? 0))
  const slow = sorted.slice(0, 10)
  return `请分析下面的请求历史，定位失败与慢请求的规律。

【概览】
总记录：${stats?.total ?? items.length}
执行次数：${stats?.runsTotal ?? '未知'}
本次分析样本：${items.length} 条，其中失败/异常 ${failed.length} 条

【失败样例（最多 20 条）】
${block(
  failed.slice(0, 20).map((item) => ({
    接口: item.requestName || '临时请求',
    方法: item.method,
    URL: item.url,
    状态码: item.statusCode,
    耗时: item.durationMs,
    时间: new Date(item.createdAt).toISOString(),
  })),
  6000,
)}

【最慢的 10 条】
${block(
  slow.map((item) => ({
    接口: item.requestName || '临时请求',
    方法: item.method,
    URL: item.url,
    状态码: item.statusCode,
    耗时: item.durationMs,
  })),
  4000,
)}

请输出：
1. 失败集中在哪些接口 / 状态码，最可能的原因；
2. 慢请求特征（哪些接口、是否稳定复现）；
3. 建议的排查步骤和验证方式；
4. 如果信息不足，说明还需要哪些数据（例如响应体、请求头）。`
}

/* ---------------- 压测报告 ---------------- */

export function reportReviewPrompt(detail: ReportDetail): string {
  const report = detail.report
  const config = detail.config
  return `请解读这份压测报告并给出结论。

【压测基本信息】
计划：${report.planName}
状态：${report.status}
并发：${report.concurrency}
目标 RPS：${report.targetRps ?? '未设置'}
总请求：${report.totalRequests}，成功 ${report.successCount}，失败 ${report.errorCount}
错误率：${((report.errorRate ?? 0) * 100).toFixed(2)}%
平均耗时：${Math.round(report.avgRt)}ms，P50 ${Math.round(report.p50Rt)}ms，P95 ${Math.round(report.p95Rt)}ms，P99 ${Math.round(report.p99Rt)}ms
平均 QPS：${report.avgQps}，最高 QPS：${report.maxQps}
Apdex：${report.apdex}

【配置与 SLA】
${block(config, 3000)}

【阶段明细】
${block(detail.stages, 3000)}

【状态码分布】
${block(detail.statusCodes, 2000)}

【错误类型分布】
${block(detail.errorTypes, 2000)}

【错误样本（最多 10 条）】
${block(
  detail.errors.slice(0, 10).map((item) => ({
    类型: item.errorType,
    信息: clip(item.errorMessage ?? '', 200),
    URL: item.requestUrl,
    状态码: item.responseStatus ?? null,
  })),
  3000,
)}

请输出：
1. 一句话结论：本次压测是否达标（若配置了 SLA 就逐项对照）；
2. 关键指标解读：吞吐、延迟分位、错误率是否正常；
3. 瓶颈判断与依据（并发、限速、超时、错误类型）；
4. 可以落地的优化动作，按优先级排序；
5. 下一轮压测建议（并发梯度、时长、观察指标）。`
}

/* ---------------- 压测计划 ---------------- */

export interface StressSettingsLike {
  mode: string
  concurrent: number
  durationSec: number
  rampUpSec: number
  thinkTimeMs: number
  targetRps: number
  slaEnabled: boolean
  slaP95: number
  slaP99: number
  slaErrorRate: number
  slaMinRps: number
}

export function stressPlanPrompt(
  detail: HttpRequestDetail,
  settings: StressSettingsLike,
  stages: StressStage[],
): string {
  return `请基于下面的接口和已有配置，制定一份可执行的压测计划。

【目标接口】
${detailBlock(detail)}

【当前配置】
模式：${settings.mode}
并发：${settings.concurrent}
时长：${settings.durationSec}s
爬坡：${settings.rampUpSec}s
思考时间：${settings.thinkTimeMs}ms
目标 RPS：${settings.targetRps || '未设置'}
SLA：${settings.slaEnabled ? `P95 ≤ ${settings.slaP95}ms，P99 ≤ ${settings.slaP99}ms，错误率 ≤ ${settings.slaErrorRate}%，最低 RPS ${settings.slaMinRps || '未设置'}` : '未启用'}
梯度阶段：${stages.length ? stages.map((item) => `${item.concurrent} 并发 × ${item.durationSec}s`).join(' → ') : '无'}

请输出：
1. 压测目标与前置条件（环境、数据准备、是否可压线上）；
2. 推荐参数：模式（固定并发 / 梯度）、并发、时长、爬坡、思考时间、目标 RPS、SLA 阈值，并说明依据；
3. 风险与止损策略（错误率、超时、下游依赖）；
4. 观察指标与判定标准；
5. 最后给出一段可直接使用的参数（JSON，字段名用 concurrent / durationSec / rampUpSec / thinkTimeMs / targetRps / stages / sla）。`
}

/* ---------------- 环境变量 ---------------- */

export function envAuditPrompt(environment: Environment, variables: EnvVariable[]): string {
  const rows = variables.map((item) => ({
    名称: item.key,
    值: item.isSecret ? '******（已标记为密钥）' : clip(item.value ?? '', 200),
    密钥: item.isSecret ? '是' : '否',
  }))
  return `请体检当前项目的环境变量配置。

【环境】
名称：${environment.name}
是否当前环境：${environment.isActive ? '是' : '否'}
环境脚本：\n${environment.script ? clip(environment.script, 2500) : '（无）'}

【变量】
${block(rows, 6000)}

请输出：
1. 高风险项：明文密钥、疑似密钥但未勾选密钥、敏感值明文（只指出变量名，不要回显完整值）；
2. 规范性问题：命名不一致、重复、空值、URL 末尾斜杠等；
3. 可能缺失的常见变量（base_url、token、超时等），说明用途；
4. 环境脚本建议（能给示例就给）；
5. 按优先级排序，最多 8 条。`
}

/* ---------------- 批量运行 ---------------- */

export function runnerAnalyzePrompt(
  summary: Record<string, unknown>,
  results: unknown[],
): string {
  return `请分析这次批量运行的结果。

【汇总】
${block(summary, 2000)}

【执行明细（最多 40 条）】
${block(results.slice(0, 40), 8000)}

请输出：
1. 通过率结论与失败分布（接口 / 状态码 / 断言）；
2. 失败原因归类（网络、超时、业务码、断言失败）；
3. 疑似不稳定的接口与复现建议；
4. 下一步动作（重跑范围、断言修正、性能关注点）。`
}

/* ---------------- 工作台 ---------------- */

export function dashboardInsightPrompt(
  stats: WorkspaceStats | null,
  metrics: DashboardMetrics | null,
): string {
  return `请根据工作台数据做一次使用情况与质量洞察。

【总量】
${block(stats, 1500)}

【按日趋势（最近）】
${block(metrics?.days?.slice(-14) ?? [], 3000)}

【状态码分组】
${block(metrics?.codeBuckets ?? [], 1500)}

【请求最多的接口（Top 10）】
${block(metrics?.topRequests?.slice(0, 10) ?? [], 3000)}

【最活跃项目】
${block(metrics?.topProjects ?? [], 1500)}

【延迟分布】
${block(metrics?.latencyBuckets ?? [], 1500)}

【压测报告趋势】
${block(metrics?.reportPoints?.slice(-5) ?? [], 2000)}

请输出：
1. 使用概况：最活跃的项目与接口；
2. 质量信号：失败/异常比例、可能存在问题的接口；
3. 性能趋势：延迟与吞吐变化是否值得关注；
4. 三条最值得做的优化动作。`
}

/* ---------------- 项目结构 ---------------- */

function flattenCollections(
  nodes: CollectionNode[],
  prefix = '',
): Array<{ 集合: string; 层级: number }> {
  const rows: Array<{ 集合: string; 层级: number }> = []
  for (const node of nodes) {
    const path = prefix ? `${prefix} / ${node.name}` : node.name
    rows.push({ 集合: path, 层级: path.split(' / ').length })
    if (node.children?.length) rows.push(...flattenCollections(node.children, path))
  }
  return rows
}

export function projectAuditPrompt(
  projectName: string,
  tree: CollectionNode[],
  requests: RequestSearchHit[],
): string {
  const collections = flattenCollections(tree)
  return `请体检这个接口项目的结构。

【项目】
${projectName}

【集合结构（共 ${collections.length} 个）】
${block(collections, 5000)}

【接口清单（共 ${requests.length} 个，最多展示 120 个）】
${block(
  requests.slice(0, 120).map((item) => ({
    集合: item.collectionName,
    名称: item.name,
    方法: item.method,
    URL: item.url,
  })),
  8000,
)}

请输出：
1. 结构问题：空集合、过深层级、命名不统一、集合划分不清晰；
2. 接口问题：疑似重复、命名/URL 不规范、明显缺少环境变量的写法；
3. 覆盖度建议：还应该补充哪些集合或接口（按业务模块）；
4. 最多 8 条可执行建议，按优先级排序。`
}
