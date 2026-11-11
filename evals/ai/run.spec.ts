import { describe, expect, it } from 'vitest'
import {
  defaultReportPath,
  formatSummary,
  loadEvalCases,
  readLiveOptions,
  runEval,
  validateEvalCases,
  writeReport,
} from '../../scripts/aiEval'

const cases = loadEvalCases()
const live = readLiveOptions()

describe('AI-10 golden 评测集', () => {
  it('20 条用例结构有效', () => {
    expect(validateEvalCases(cases)).toEqual([])
  })

  it(
    '真实模型评测（未配置 AI_EVAL_* 时只做结构校验）',
    async () => {
      if (!live) {
        console.warn(
          '[ai-eval] 未配置 AI_EVAL_BASE_URL / AI_EVAL_API_KEY / AI_EVAL_MODEL，' +
            '本次仅校验用例结构，未调用真实模型。配置方式见 evals/ai/README.md。',
        )
        return
      }
      const report = await runEval(cases, live)
      console.log(`\n${formatSummary(report)}\n`)
      writeReport(defaultReportPath(), report)
      expect(report.verdict.reasons.join('；') || 'ok').toBe('ok')
    },
    600_000,
  )
})
