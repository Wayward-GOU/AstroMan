# AI-10 golden 评测集

20 条固定任务，用来在**改提示词 / 换模型**前后对比工具选择是否回退。默认不接 CI 真实模型。

## 运行

```bash
# 1) 只校验用例结构（不联网）
npm run ai:eval

# 2) 跑真实模型（OpenAI 兼容网关）
AI_EVAL_BASE_URL=https://api.openai.com/v1 \
AI_EVAL_API_KEY=sk-xxx \
AI_EVAL_MODEL=gpt-4o-mini \
npm run ai:eval
```

Windows PowerShell 用 `$env:AI_EVAL_BASE_URL="..."` 逐条设置。

| 环境变量 | 说明 |
|---|---|
| `AI_EVAL_BASE_URL` | OpenAI 兼容地址，脚本自动拼 `/chat/completions` |
| `AI_EVAL_API_KEY` | 仅用于 Authorization 头，不写入报告 |
| `AI_EVAL_MODEL` | 被测模型名 |
| `AI_EVAL_PROTOCOL` | `native`（默认，原生 function calling）或 `text`（强制 fbin-tool 文本协议） |
| `AI_EVAL_TEMPERATURE` | 默认 `0` |
| `AI_EVAL_TIMEOUT_MS` | 单次请求超时，默认 `90000` |
| `AI_EVAL_SCORES` | 可选，人工评分 JSON：`{"ai-01":5,"ai-02":4}` |

结果写入 `evals/ai/.last-run.json`（已忽略，便于前后两次 diff），控制台同时打印逐条结果与汇总。

## 用例字段

| 字段 | 说明 |
|---|---|
| `input` | 用户原话 |
| `expectTool` | 期望的首步工具；`null` 表示这轮不应调用任何工具 |
| `acceptTools` | 等价首步工具（如先预检、先查抓包状态） |
| `forbiddenTools` | 出现即判失败的工具（防越权写入 / 多余调用） |
| `expectArgs` | 只比较列出的关键参数（数字 / 字符串按文本比较） |
| `expectedArtifact` | 期望产物，供人工评分对照 |
| `followUps` | 模型没有调用工具时补问的话，最多 3 轮 |

## 指标与门槛

- 命中率：首步工具（及关键参数）匹配且未触发禁用工具；
- 平均轮数：拿到首步工具前消耗的模型往返次数；
- token：`usage.prompt_tokens + completion_tokens` 累计；
- P95 时延：按单条用例端到端耗时取 P95；
- 人工评分：`AI_EVAL_SCORES` 的平均分（1–5）。

**门槛：命中率 ≥85%，平均轮数 ≤4**；未通过时 `npm run ai:eval` 以非 0 退出，发布前应阻断。
模型或提示词变更前后各跑一次，对比 `.last-run.json` 即可看出回退。
