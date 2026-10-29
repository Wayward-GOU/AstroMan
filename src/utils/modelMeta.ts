/**
 * 模型上下文窗口元数据。
 * 只维护「窗口大小」这一类稳定信息，价格仍以 aiPricing.ts 为准。
 * 未知模型一律回退 DEFAULT_CONTEXT_WINDOW，宁保守不冒进——预算给多了会直接报错。
 */

/** 未知模型的保守窗口：32k 覆盖绝大多数本地小模型与老网关 */
export const DEFAULT_CONTEXT_WINDOW = 32_000

/** 上下文预算占窗口比例：留出输出与系统提示词余量 */
export const CONTEXT_BUDGET_RATIO = 0.6

const WINDOWS: [RegExp, number][] = [
  // OpenAI
  [/gpt-4\.1/i, 1_047_576],
  [/gpt-4o|gpt-4-turbo|gpt-4-vision/i, 128_000],
  [/gpt-4/i, 8_192],
  [/gpt-3\.5/i, 16_385],
  [/^o[134](-|$)/i, 200_000],
  // Anthropic
  [/claude/i, 200_000],
  // Google
  [/gemini-2\.|gemini-1\.5/i, 1_000_000],
  [/gemini/i, 32_000],
  // DeepSeek
  [/deepseek/i, 128_000],
  // Kimi / Moonshot
  [/moonshot.*8k/i, 8_192],
  [/moonshot.*32k/i, 32_768],
  [/moonshot.*128k|kimi/i, 128_000],
  // 智谱
  [/glm-4.*long|glm-4\.5/i, 128_000],
  [/glm-4\.1|glm-4-plus|glm-4-air/i, 128_000],
  [/glm-4|glm-z1/i, 128_000],
  // 通义千问
  [/qwen.*long/i, 1_000_000],
  [/qwen3|qwen2\.5|qwen-plus|qwen-max/i, 128_000],
  [/qwen-turbo/i, 1_000_000],
  [/qwen/i, 32_000],
  // 本地常见模型
  [/llama-?3.*70b/i, 128_000],
  [/llama-?3|llama3/i, 8_192],
  [/qwen2/i, 32_768],
  [/mistral|mixtral/i, 32_768],
  [/phi-?3/i, 128_000],
  [/gemma/i, 8_192],
]

/** 模型 → 上下文窗口（token） */
export function contextWindowOf(model: string): number {
  const name = (model ?? '').trim()
  if (!name) return DEFAULT_CONTEXT_WINDOW
  for (const [pattern, window] of WINDOWS) {
    if (pattern.test(name)) return window
  }
  return DEFAULT_CONTEXT_WINDOW
}

/** 可用于「历史 + 本轮提问」的预算（token），默认窗口的 60% */
export function contextBudgetOf(model: string): number {
  return Math.max(2_000, Math.floor(contextWindowOf(model) * CONTEXT_BUDGET_RATIO))
}
