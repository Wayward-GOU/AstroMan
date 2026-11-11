/**
 * 模型计费估算：单价按厂商公开价（美元 / 百万 token）整理，仅用于「大概多少钱」的展示。
 * 服务商可以覆盖单价（AI-06）：填了就用自定义价，0 表示沿用内置价格表。
 */

export type PriceCurrency = 'USD' | 'CNY'

export interface PriceOverride {
  /** 输入单价；0 / 空表示使用内置价格表 */
  inputPrice?: number | null
  /** 输出单价；0 / 空表示使用内置价格表 */
  outputPrice?: number | null
  /** 自定义单价的计价币种；默认美元 */
  currency?: string | null
}

export interface PriceRule {
  /** 展示用的价格档位名 */
  label: string
  /** 输入单价：美元 / 百万 token（内部统一按美元计算成本） */
  input: number
  /** 输出单价：美元 / 百万 token */
  output: number
  /** 是否为兜底估算价 */
  fallback: boolean
  /** 报价币种：内置价为 USD，自定义价按服务商配置 */
  currency: PriceCurrency
  /** 原始报价单价（币种见 currency），用于按服务商币种展示 */
  listInput: number
  listOutput: number
  /** 是否来自服务商自定义单价 */
  custom: boolean
}

/** 顺序敏感：越具体的模型名放前面，避免 gpt-4o-mini 命中 gpt-4o */
const PRICE_TABLE: {
  pattern: RegExp
  rule: Omit<PriceRule, 'currency' | 'listInput' | 'listOutput' | 'custom'>
}[] = [
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

const FALLBACK: Omit<PriceRule, 'currency' | 'listInput' | 'listOutput' | 'custom'> = {
  label: '默认估算',
  input: 1,
  output: 3,
  fallback: true,
}

/** 人民币展示汇率，只做粗略换算 */
export const USD_TO_CNY = 7.2

function withDefaults(
  rule: Omit<PriceRule, 'currency' | 'listInput' | 'listOutput' | 'custom'>,
): PriceRule {
  return {
    ...rule,
    currency: 'USD',
    listInput: rule.input,
    listOutput: rule.output,
    custom: false,
  }
}

/** 正数才算有效单价；0 / 空 / 负数都回落到内置价格表 */
function positive(value: number | null | undefined): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return null
  return value
}

/**
 * 解析生效单价：内置表 + 服务商覆盖（AI-06）。
 * - 自定义价 0 / 空 -> 该边沿用内置价；
 * - 自定义价币种为 CNY 时，按固定汇率折算成美元再计算成本，展示仍用人民币原价；
 * - 两边都填了自定义价时不再标记「默认估算」。
 */
export function priceOf(model: string, override?: PriceOverride | null): PriceRule {
  const name = (model ?? '').trim()
  const hit = name ? PRICE_TABLE.find((item) => item.pattern.test(name)) : undefined
  const base = withDefaults(hit ? hit.rule : FALLBACK)
  const inputRaw = positive(override?.inputPrice)
  const outputRaw = positive(override?.outputPrice)
  if (inputRaw === null && outputRaw === null) return base

  const currency: PriceCurrency =
    (override?.currency ?? 'USD').toUpperCase() === 'CNY' ? 'CNY' : 'USD'
  const cny = currency === 'CNY'
  // 缺失的一边沿用内置价；人民币报价时把内置美元价折成人民币展示
  const listInput = inputRaw ?? (cny ? base.input * USD_TO_CNY : base.input)
  const listOutput = outputRaw ?? (cny ? base.output * USD_TO_CNY : base.output)
  const scale = cny ? 1 / USD_TO_CNY : 1
  return {
    label: '自定义单价',
    input: listInput * scale,
    output: listOutput * scale,
    fallback: base.fallback && !(inputRaw !== null && outputRaw !== null),
    currency,
    listInput,
    listOutput,
    custom: true,
  }
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
  override?: PriceOverride | null,
): CostBreakdown {
  const rule = priceOf(model, override)
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

/** 单价展示：$x / 1M tokens（自定义人民币报价直接按 ¥ 展示原价） */
export function fmtUnit(perMillion: number, currency: PriceCurrency = 'USD'): string {
  const text = perMillion >= 1 ? perMillion.toFixed(2) : perMillion.toFixed(3)
  return currency === 'CNY' ? `¥${text}/M` : `$${text}/M`
}
