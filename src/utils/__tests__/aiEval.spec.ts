import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'
import { describe, expect, it } from 'vitest'
import {
  loadEvalCases,
  parseTextToolCalls,
  percentile,
  runEval,
  summarize,
  validateEvalCases,
} from '../../../scripts/aiEval'

// AI-10 的用例集随工具改名 / 删除会失效，放进常规测试防止腐烂；不调用真实模型
describe('AI 评测集结构', () => {
  it('20 条 golden 用例与工具目录保持一致', () => {
    expect(validateEvalCases(loadEvalCases())).toEqual([])
  })

  it('文本协议能解析 fbin-tool 代码块', () => {
    const calls = parseTextToolCalls(
      '先查一下：\n```fbin-tool\n{"tool":"project_list","args":{"limit":10}}\n```\n再看结果。',
    )
    expect(calls).toEqual([{ name: 'project_list', args: { limit: 10 }, source: 'text' }])
  })

  it('P95 与汇总指标按用例计算', () => {
    expect(percentile([100, 300, 200, 400, 500], 0.95)).toBe(500)
    const metrics = summarize([
      { id: 'a', title: 'a', hit: true, toolHit: true, argsHit: true, rounds: 1, tokens: 10, latencyMs: 100, picked: 'x', forbiddenHit: [] },
      { id: 'b', title: 'b', hit: false, toolHit: false, argsHit: false, rounds: 3, tokens: 20, latencyMs: 300, picked: null, forbiddenHit: [] },
    ])
    expect(metrics.hitRate).toBe(0.5)
    expect(metrics.avgRounds).toBe(2)
    expect(metrics.tokens).toBe(30)
    expect(metrics.p95LatencyMs).toBe(300)
  })

  it('本地假网关跑通 OpenAI 兼容协议与门槛判定', async () => {
    const cases = loadEvalCases()
    const byInput = new Map(cases.map((item) => [item.input, item]))
    const server = createServer((req, res) => {
      let body = ''
      req.on('data', (chunk) => {
        body += chunk
      })
      req.on('end', () => {
        const payload = JSON.parse(body) as { messages: { role: string; content: string }[] }
        const user = [...payload.messages].reverse().find((item) => item.role === 'user')
        const testCase = byInput.get(user?.content ?? '')
        const message = testCase?.expectTool
          ? {
              content: null,
              tool_calls: [
                {
                  id: 'call-1',
                  type: 'function',
                  function: {
                    name: testCase.expectTool,
                    arguments: JSON.stringify(testCase.expectArgs ?? {}),
                  },
                },
              ],
            }
          : { content: '这是直接回答，不调用工具。' }
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(
          JSON.stringify({
            choices: [{ message }],
            usage: { prompt_tokens: 100, completion_tokens: 20 },
          }),
        )
      })
    })
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    try {
      const { port } = server.address() as AddressInfo
      const report = await runEval(cases, {
        mode: 'native',
        baseUrl: `http://127.0.0.1:${port}/v1`,
        apiKey: 'local-test',
        model: 'fake-model',
      })
      expect(report.metrics.total).toBe(20)
      expect(report.metrics.hitRate).toBe(1)
      expect(report.metrics.avgRounds).toBe(1)
      expect(report.metrics.tokens).toBe(2400)
      expect(report.verdict.pass).toBe(true)
    } finally {
      await new Promise<void>((resolve, reject) =>
        server.close((err) => (err ? reject(err) : resolve())),
      )
    }
  })
})
