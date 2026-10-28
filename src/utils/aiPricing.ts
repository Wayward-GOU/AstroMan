/**
 * 模型计费估算：单价按厂商公开价（美元 / 百万 token）整理，仅用于「大概多少钱」的展示。
 * 匹配不上的模型走兜底价，并在界面上标注「默认估算」。
 */
export interface PriceRule {
  /** 展示用的价格档位名 */
  label: string
  /** 输入单价：美元 / 百万 token */
  input: number
  /** 输出单价：美元 / 百万 token */
  output: number
  /** 是否为兜底估算价 */
  fallback: boolean
}

/** 顺序敏感：越具体的模型名放前面，避免 gpt-4o-mini 命中 gpt-4o */
const PRICE_TABLE: { pattern: RegExp; rule: PriceRule }[] = [
  { pattern: /gpt-4o-mini/i, rule: { label: 'gpt-4o-mini', input: 0.15, output: 0.6, fallback: false } },
  { pattern: /gpt-4o/i, rule: { label: 'gpt-4o', input: 2.5, output: 10, fallback: false } },
  { pattern: /gpt-4\.1-mini/i, rule: { label: 'gpt-4.1-mini', input: 0.4, output: 1.6, fallback: false } },
  { pattern: /gpt-4\.1-nano/i, rule: { label: 'gpt-4.1-nano', input: 0.1, output: 0.4, fallback: false } },
  { pattern: /gpt-4\.1/i, rule: { label: 'gpt-4.1', input: 2, output: 8, fallback: false } },
  { pattern: /o4-mini/i, rule: { label: 'o4-mini', input: 1.1, output: 4.4, fallback: false } },
  { pattern: /o3-mini/i, rule: { label: 'o3-mini', input: 1.1, output: 4.4, fallback: false } },
  { pattern: /^o3|o3-/i, rule: { label: 'o3', input: 2, output: 8, fallback: false } },
  { pattern: /gpt-4-turbo/i, rule: { label: 'gpt-4-turbo', input: 10, output: 30, fallback: false } },
  { pattern: /gpt-3\.5/i, rule: { label: 'gpt-3.5-turbo', input: 0.5, output: 1.5, fallback: false } },
  { pattern: /claude.*opus/i, rule: { label: 'claude-opus', input: 15, output: 75, fallback: false } },
  { pattern: /claude.*haiku/i, rule: { label: 'claude-haiku', input: 0.8, output: 4, fallback: false } },
  { pattern: /claude/i, rule: { label: 'claude-sonnet', input: 3, output: 15, fallback: false } },
  { pattern: /gemini-2\.5-pro/i, rule: { label: 'gemini-2.5-pro', input: 1.25, output: 10, fallback: false } },
  { pattern: /gemini-2\.5-flash/i, rule: { label: 'gemini-2.5-flash', input: 0.3, output: 2.5, fallback: false } },
  { pattern: /gemini-2\.0-flash/i, rule: { label: 'gemini-2.0-flash', input: 0.1, output: 0.4, fallback: false } },
  { pattern: /gemini-1\.5-pro/i, rule: { label: 'gemini-1.5-pro', input: 1.25, output: 5, fallback: false } },
  { pattern: /gemini.*flash/i, rule: { label: 'gemini-flash', input: 0.075, output: 0.3, fallback: false } },
  { pattern: /deepseek-reasoner|deepseek-r1/i, rule: { label: 'deepseek-reasoner', input: 0.55, output: 2.19, fallback: false } },
  { pattern: /deepseek/i, rule: { label: 'deepseek-chat', input: 0.27, output: 1.1, fallback: false } },
  { pattern: /qwen-max/i, rule: { label: 'qwen-max', input: 1.6, output: 6.4, fallback: false } },
  { pattern: /qwen-plus/i, rule: { label: 'qwen-plus', input: 0.4, output: 1.2, fallback: false } },
  { pattern: /qwen-turbo/i, rule: { label: 'qwen-turbo', input: 0.05, output: 0.1, fallback: false } },
  { pattern: /qwen/i, rule: { label: 'qwen', input: 0.4, output: 1.2, fallback: false } },
  { pattern: /glm-4-plus|glm-4\.5/i, rule: { label: 'glm-4', input: 0.7, output: 0.7, fallback: false } },
  { pattern: /glm/i, rule: { label: 'glm', input: 0.1, output: 0.1, fallback: false } },
  { pattern: /kimi|moonshot/i, rule: { label: 'kimi', input: 0.6, output: 2.5, fallback: false } },
  { pattern: /doubao-pro|doubao-1\.5-pro/i, rule: { label: 'doubao-pro', input: 0.11, output: 0.28, fallback: false } },
  { pattern: /doubao/i, rule: { label: 'doubao', input: 0.11, output: 0.28, fallback: false } },
  { pattern: /ernie|wenxin/i, rule: { label: 'ernie', input: 0.11, output: 0.28, fallback: false } },
  { pattern: /llama|mistral|mixtral/i, rule: { label: '开源模型', input: 0.2, output: 0.6, fallback: false } },
]

const FALLBACK: PriceRule = { label: '默认估算', input: 1, output: 3, fallback: true }

/** 人民币展示汇率，只做粗略换算 */
export const USD_TO_CNY = 7.2

export function priceOf(model: string): PriceRule {
  const name = (model ?? '').trim()
  if (!name) return FALLBACK
  const hit = PRICE_TABLE.find((item) => item.pattern.test(name))
  return hit ? hit.rule : FALLBACK
}

export interface CostBreakdown {
  /** 输入费用（美元） */
  inputCost: number
  /** 输出费用（美元） */
  outputCost: number
  /** 合计（美元） */
  total: number
  rule: PriceRule
}

export function estimateCost(
  model: string,
  promptTokens: number,
  completionTokens: number,
): CostBreakdown {
  const rule = priceOf(model)
  const inputCost = ((promptTokens || 0) / 1_000_000) * rule.input
  const outputCost = ((completionTokens || 0) / 1_000_000) * rule.output
  return { inputCost, outputCost, total: inputCost + outputCost, rule }
}

/** 金额展示：极小的值保留 4 位小数，避免全是 $0.00 */
export function fmtUsd(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '$0.00'
  if (value < 0.01) return `$${value.toFixed(4)}`
  if (value < 1) return `$${value.toFixed(3)}`
  return `$${value.toFixed(2)}`
}

export function fmtCny(usd: number): string {
  const cny = usd * USD_TO_CNY
  if (!Number.isFinite(cny) || cny <= 0) return '¥0.00'
  if (cny < 1) return `¥${cny.toFixed(3)}`
  return `¥${cny.toFixed(2)}`
}

/** 单价展示：$x / 1M tokens */
export function fmtUnit(perMillion: number): string {
  const text = perMillion >= 1 ? perMillion.toFixed(2) : perMillion.toFixed(3)
  return `$${text}/M`
}
