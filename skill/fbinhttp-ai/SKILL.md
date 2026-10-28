---
name: fbinhttp-ai
description: FbinHttp 内置 AI 助手「小HI」的开发与维护规范。接入新模型厂商、改流式对话、加系统工具（AI 可调用的能力）、调会话与工具调用记录、排查 AI 报错时使用。触发词：小HI、AI 助手、ai_chat、aiTools、工具调用、工具记录、会话、provider、OpenAI、Claude、Gemini。
---

# 小HI（AI 助手）

小HI 是内置在 FbinHttp 里的接口调试助手：用户自备模型 API Key，它既能纯聊天，也能通过「工具」直接操作软件（建项目、建请求、发请求、压测、导入导出），并把每次工具调用单独落库供用户审计。

## 模块地图

| 位置 | 职责 |
|---|---|
| `src-tauri/src/prompts/xiao_hi_system_prompt.md` | **小HI 身份、执行流程与业务规则**（唯一权威版本，改行为先改这里） |
| `src-tauri/src/prompts/mod.rs` | `include_str!` 编译期嵌入 prompt，导出 `XIAO_HI_SYSTEM_PROMPT` |
| `src-tauri/src/commands/ai.rs` | 服务商 CRUD 与测试、流式对话、工具执行代理、会话/消息、工具流水、用量统计、报错中文化 |
| `src-tauri/src/models.rs` | `AiProvider`、`AiMessage`、`AiChatPayload`、`AiStreamEvent`、`AiSession`、`AiStoredMessage`、`AiToolLog`、`AiUsageLog` / `AiUsagePage` / `AiUsageDay` |
| `src-tauri/src/db/schema.sql` | `ai_providers`、`ai_sessions`、`ai_messages`、`ai_tool_logs`、`ai_usage_logs` 五张表 |
| `src/utils/aiTools.ts` | **工具注册表**：39 个工具的声明、参数解析、执行、中文名、敏感标记 |
| `src/utils/aiPricing.ts` | 模型价格表与费用估算：`estimateCost / fmtUsd / fmtCny / fmtUnit`，`USD_TO_CNY` |
| `src/views/AiAssistant.vue` | 对话页：会话侧栏 + 消息气泡 + 工具卡片 + 模型服务抽屉 |
| `src/views/AiUsage.vue` | 用量统计独立页（`/ai/usage`）：6 张指标卡 + 每日 token 柱 + 调用记录 / 模型汇总 / 工具准确率三 Tab + 右侧抽屉 |
| `src/views/AiToolLogs.vue` | 工具调用记录页（`/ai/logs`）：指标卡、趋势柱、表格、详情抽屉 |
| `src/views/Settings.vue` | 设置页「AI 辅助（小HI）」卡片：已上线说明 + 「打开小HI」「用量统计」 |
| `src/views/Dashboard.vue` | 首页使用说明：第 07 步「用小HI 提效」、3 张 AI 技巧卡、3 条 AI FAQ |
| `src/App.vue` | 右侧屏幕垂直居中的悬浮机器人入口（54px，`right: 14px` + `top: 50%`，`/ai` 页隐藏） |
| `src/api/index.ts` | `aiProvider*`、`aiChat`、`aiExecute`、`aiSession*`、`aiMessage*`、`aiToolLog*`、`aiUsage*` |

## 三种协议

`ai_providers.kind` 决定请求形态，新增厂商优先走 OpenAI 兼容（绝大多数国产模型都支持）。

| kind | 端点 | 鉴权头 | 请求字段 |
|---|---|---|---|
| `openai` | `{base}/chat/completions` | `Authorization: Bearer` | `messages[{role,content}]`、`temperature`、`stream` |
| `anthropic` | `{base}/v1/messages` | `x-api-key` + `anthropic-version: 2023-06-01` | `system` 单独字段、`max_tokens` 必填、`messages` 仅 user/assistant |
| `gemini` | `{base}/v1beta/models/{model}:streamGenerateContent?alt=sse` | `x-goog-api-key` | `contents[{role,parts}]`（assistant→model）、`systemInstruction` |

响应统一经 `AiStreamEvent { kind: "delta" | "done" | "error", text }` 通过 Tauri `Channel` 回传；非 SSE 响应（部分网关直接返回整包 JSON）要回退成一次性解析。

## 密钥存储

- 入库前：`SHA-256(固定种子 + api_key)` 得 32 字节，作 AES-256-GCM 密钥
- 落库格式：`v1:` + base64(`nonce(12B) || ciphertext`)，nonce 用 `uuid::Uuid::new_v4()` 前 12 字节
- 列表接口永远返回掩码（只留尾部若干位），前端不得回显明文
- 用户没配置任何可用服务商时，整块 AI 功能不可用——页面给出「去配置」引导，不能静默失败

## 流式对话

```ts
const channel = new Channel<AiStreamEvent>()
channel.onmessage = (event) => { /* delta 追加 / error 标红 */ }
await aiChat({ providerId, messages, temperature }, channel)
```

Rust 侧 `ai_chat(payload, on_event: Channel<AiStreamEvent>)`，逐块 `on_event.send(...)`，结束发 `done`，异常发 `error` 且不 panic。前端把流式消息累积到最后一个 `ChatItem`，`streaming` 期间禁止再次发送。

## 会话与消息

- 新建会话时标题为「新对话」；**首条用户消息**写入时后端自动截取前 24 字符作标题（`truncate_title`）
- 每条用户消息与最终助手回复都要 `aiMessageAdd` 落库；流式过程中的半成品不入库
- 切换会话时用 `aiMessageList` 重建消息列表，并从内容里重新解析工具块（保持卡片可再执行）
- 上下文只带最近 12 轮 + system prompt，避免长会话把 token 打爆

## 工具调用（核心机制）

**不是模型原生 function calling**，而是自定义 fenced 块协议：

````
```fbin-tool
{"tool": "request_create", "args": {"collectionId": 3, "name": "登录", "method": "POST", "url": "{{baseUrl}}/login"}}
```
````

流程：模型输出块 → 前端正则切分消息为 `text` / `code` / `tool` 三种 segment → `executeTool(name, args)` → 结果文本回灌给模型 → 模型继续总结。自动链路最多 5 轮（`MAX_TOOL_ROUNDS` / `autoDepth`），每轮只执行 1 个工具，超出后由用户点卡片「运行」手动推进。

### 系统提示词注入（后端强制）

- prompt 正文只维护 `src-tauri/src/prompts/xiao_hi_system_prompt.md` 一份；`prompts/mod.rs` 用 `include_str!` 编译期嵌入，不走运行时磁盘 IO，打包后即在二进制内
- 每次 `ai_chat` 都会把 `XIAO_HI_SYSTEM_PROMPT` **插到 messages[0]**，并剔除客户端传来的同内容副本；前端无法覆盖、删除或改写这套规则
- 前端的 `SYSTEM_PROMPT` 只保留「工具协议 + 脚本引擎」这类随代码动态生成的内容（`TOOL_CATALOG` 来自注册表），不得再写身份与业务流程，避免与后端 prompt 冲突
- 改行为（轮次上限、确认策略、输出格式）时：先改 md，再同步前端协议文案与 `MAX_TOOL_ROUNDS`、`isDangerousTool` 等常量
- md 里的「单轮最多 1 个工具」由 `afterReply` 只取第一个 idle 安全工具来落实；「最多 5 轮」由 `autoDepth` 拦截

### 代码层硬规则（`src-tauri/src/docx/agent_hard_rules.md`）

模型绕不开的调度器约束，全部在 `aiTools.ts` + `AiAssistant.vue` 落地：

| 规则 | 落点 |
|---|---|
| 1. 最多连续 5 次工具调用，超限终止并提示「已达到最大执行轮次，本次任务终止，请调整你的需求重试」 | `MAX_TOOL_ROUNDS` + `autoDepth`，超限走 `notifyTerminated()`；`send()` 每次用户发言重置 |
| 2. 一轮输出多个 tool_call 直接拒绝，错误回灌「单次仅允许调用一个工具，请重新选择工具与参数」 | `afterReply` 的 `tools.length > 1` 分支 |
| 3. 入参 JSON Schema 校验（类型 / 必填 / 正则 / 枚举 / 数值与数组范围），非法不调底层 | `SCHEMAS` + `validateToolArgs()`，`executeTool` 入口拦截，错误作为 Observation 回灌 |
| 4. 高危工具不直接执行，弹窗等用户确认；确认或拒绝都进上下文 | `danger: true`（16 个）+ `runToolSegment` 的 `Modal.confirm` |
| 5. HTTP 类 30 秒超时 | `send_request` / `run_batch` 的 `timeoutMs` schema `max: 30000`，超时按工具失败回灌 |
| 6. 回灌前脱敏 | `executeTool` 末尾 `maskSensitive()`（Bearer/Basic、api_key/token/secret/password/cookie 等） |
| 7. Trace 落库 | `ai_messages` 增加 `reasoning` 列（老库 `ALTER TABLE` 迁移），助手思考过程随消息持久化；工具入参/结果/成败在 `ai_tool_logs` |
| 7.1 落库裁剪（防膨胀） | 消息正文 `clip_trace` 上限 8000 字、思考 2000 字，超限追加「…（已截断，原长度 N 字）」；附件 `` ```fbin-file `` 块由 `strip_file_blocks` 换成「文件名 + 字符数」摘要，正文只用于当轮推理；工具流水 `args` / `result_preview` 各 2000 字 |

改规则时的顺序：`agent_hard_rules.md`（需求） → `SCHEMAS` / `MAX_TOOL_ROUNDS` / `danger` 标记 → 前端提示文案 → 本表。

### 工具清单（39）

| 分类 | 工具 |
|---|---|
| 项目 | `project_list` `project_request_counts` `project_create` `project_update` `project_delete` `project_import` `project_export` |
| 集合 | `collection_tree` `collection_create` `collection_rename` `collection_delete` |
| 请求 | `request_list` `request_get` `request_search` `request_options` `request_create` `request_update` `request_script_save` `request_delete` `curl_import` `send_request` `run_batch` |
| 环境 | `env_list` `env_create` `env_var_save` `env_activate` |
| 压测 | `plan_list` `stress_start` `stress_stop` `report_list` `report_get` `report_export` |
| 数据 | `history_list` `history_export` `workspace_stats` `dashboard_metrics` |
| 导入导出 | `api_import` `openapi_import` `openapi_export` |

### 敏感操作（`danger: true`）

`project_update` `project_delete` `collection_rename` `collection_delete` `request_update` `request_script_save` `request_delete` `env_var_save` `stress_start` `stress_stop` `run_batch` `history_export` `project_export` `openapi_export` `report_export`

规则：
1. 敏感工具**不参与自动执行**——`afterReply` 过滤掉，卡片显示橙色「需确认」
2. 点「运行」时 `Modal.confirm` 二次确认，取消则不执行
3. system prompt 强制要求：先说明影响对象与范围、一次只做一件、删除前先核对 id 与名称
4. 新增工具时，只要会改数据/删数据/打流量/写文件，一律 `danger: true`

### 新增一个工具

1. 在 `src/api/index.ts` 确认已有对应 command 包装（没有就先加，参考 `fbinhttp-add-feature`）
2. 在 `src/utils/aiTools.ts` 的 `SPECS` 追加 `{ name, desc, args, handler, danger?, format? }`
   - `args` 是给模型看的签名文案，写清必填/可选
   - 参数解析统一用 `str` / `needNum` / `optionalNum` / `toItems` / `needPath`，不要直接强转
   - 返回对象会被 JSON 美化回灌；长文本类（HTTP 结果）给 `format` 自定义摘要
3. 在 `TOOL_LABELS` 补中文名（`TOOL_CATALOG` 由 `SPECS` 自动生成，system prompt 随之更新）
4. 若涉及新命令，记得 `lib.rs::generate_handler!` 注册 + `models.rs` 建模
5. 更新本文档的工具清单

### 工具流水

每次执行（无论成功失败）都写 `ai_tool_logs`：`session_id`、`tool`、`args`（截 2000 字符）、`ok`、`duration_ms`、`result_preview`（截 2000 字符）。用途：

- 用户审计：`/ai/logs` 页按工具/状态/时间筛选，分页 20/50/100，点行开右侧抽屉看完整参数与输出
- 统计：`ai_tool_log_stats` 给总量、成功率、平均耗时、近 24 小时、按天趋势、按工具分布
- 保留策略：写入时清理 **365 天**前记录；其余只由用户点「清空记录」删除
- 清空只删流水，不动会话与消息

## 界面约定

- 用户消息右对齐、主色浅底气泡；助手消息左对齐、带头像、无重边框，正文走 `renderMarkdown`（`markdown-it`，`html: false`）
- 用户输入不按 Markdown 渲染，只保留换行，避免 `*`、`#` 被误解析
- 工具卡片默认单行折叠：工具名 + 状态标签 + 耗时 + 「运行」，点「详情」展开参数与输出，同时只展开一个
- **思考与执行过程默认折叠**：模型返回的 `reasoning`（OpenAI `reasoning_content` / Claude `thinking` / Gemini `thought`）与全部工具卡片收进一个 `a-collapse` 面板，标题为「思考过程 · N 个工具调用」；气泡正文只留用户的提问与最终回答
- **附件**：输入框左侧回形针选择本地文本文件（≤60000 字符，超出截断），内容以 ```` ```fbin-file 文件名 ```` 块拼进用户消息；`contentParts` 会过滤掉该块，气泡里只显示附件胶囊，模型仍能读到全文
- 输入框：Enter 发送、Shift/Ctrl + Enter 换行，`event.isComposing` 时不发送（中文输入法组字保护）
- 悬浮入口：54px 机器人（纯 CSS/SVG，无图片），`z-index: 900` 低于弹窗，固定在**右侧屏幕垂直居中**（`right: 14px; top: 50%; margin-top: -27px`），`/ai` 页隐藏，不占用主内容区布局
- 其他入口必须与悬浮入口说法一致：设置页「AI 辅助（小HI）」卡片（绿色「已上线」标签 + 打开小HI / 用量统计两个按钮）、首页使用说明第 07 步、3 张 AI 技巧卡与 3 条 AI FAQ；改文案时三处一起改，禁止出现「即将开放」「右下角机器人」这类过期描述
- **只展示结果**：自动执行工具的那一轮（「我先获取…」）标 `hidden`，模板渲染 `visibleMessages`；重开会话用 `isExecutionNote()` 过滤历史，同 deepseek 的「只给答案」
- **用量统计独立页**（`/ai/usage`，`src/views/AiUsage.vue`，工具栏「用量统计」直接路由过去）：6 张指标卡（调用次数 / 模型成功率 / 工具准确率 / 总 token / 估算费用 / 平均耗时）+ 每日 token 堆叠柱（输入/输出，>45 天按周聚合）+ Tab「调用记录 / 模型汇总 / 工具准确率」；调用记录来自 `ai_usage_logs`，一次调用一条，默认 10 条/页（10/20/50/100），完整 `YYYY-MM-DD HH:MM:SS`，整行点击弹右侧 560px 抽屉
- **费用估算**：`src/utils/aiPricing.ts` 按模型名正则匹配厂商公开单价（USD/1M tokens），匹配不到走默认价并在 UI 标注「默认估算」；导出 `estimateCost / fmtUsd / fmtCny / fmtUnit`，汇率常量 `USD_TO_CNY`
- `ai_tool_log_stats` 的 `LIMIT 60`：工具准确率覆盖全部常见工具；`ai_usage_logs` 支持 `ok` 参数筛选成功/失败
- 列表类工具统一 `summarizeList()` 返回 `{ total, shown, items }`（明细限 50），模型引用 `total` 而非数条目
- **工具输出先中文化再回灌**：`report_list` / `report_get` 由 `reportRows()` / `reportDigest()` 把 status（completed→已完成）、mode（fixed→固定并发）、bodyType（json→JSON）、errorType（TIMEOUT→请求超时）映射成中文，`requestId` / `envId` 解析成请求名 / 项目名 / 环境名，时间统一 `YYYY-MM-DD HH:MM:SS`；`history_list` 由 `historyRows()` 把 projectId 换成项目名。映射表是 `REPORT_STATUS_LABELS` / `STRESS_MODE_LABELS` / `BODY_TYPE_LABELS` / `ERROR_TYPE_LABELS`，新增枚举值时先补表
- **其余查询类工具的取值规范**：`project_list` / `request_list` / `request_search` / `request_options` / `plan_list` 统一把 `updatedAt` 换成 `fmtTime` 本地时间；`env_list` 用中文键并给 `isSecret` 变量打码（明文密钥禁止进入模型上下文）；`workspace_stats` 走 `workspaceDigest()`、`dashboard_metrics` 走 `dashboardDigest()`，`topRequests.successRate` / `Reports.errorRate` 后端已是 **0–100**，不要再乘 100；`send_request` 的 `formatHttpResult` 用 `HTTP_STATUS_CN` 输出「200 成功」并把耗时/体积换成毫秒/KB
- **空态自我介绍**：会话为空时显示 hero（头像 + 一段定位说明）、4 张能力卡（调接口 / 做工程 / 查数据 / 有留痕，数据来自 script 里的 `capabilities`）、敏感操作提示、以及 6 个可点击填充的 `suggestions`；改能力清单只改 `capabilities`，不用动模板

## 脚本引擎（前后置脚本）

脚本是同步 JS，由 `src/utils/scriptRunner.ts` 用 `new Function` 注入以下参数执行，**不能用** require/import/fetch/async/timer，也不要顶层 return。`cipher` 来自 `cryptoScript.ts` 的 `CipherFacade`。

- 前置：`request`（method/url/headers/params/body）、`vars`（get/set/unset/has/all）、`env`、`console`、`cipher`、`rsa`
- 后置：额外 `response`（status/headers/text()/json()/durationMs）、`test(name, fn)`、`assert(cond, msg)`

`request_script_save({ id, preScript?, postScript?, assertions? })` 只改脚本字段，保存前用 `new Function(...args, code)` 做**语法检查**（只构造不执行）。改脚本一律用它，不要用 `request_update`——后者会用未传字段覆盖整条请求。system prompt 中已内嵌这份 API 参考（注意：prompt 是模板字符串，正文里禁止出现反引号，否则会截断字符串导致 TS 编译失败）。

## 模型用量统计（token 与准确率）

每次 `ai_chat` 结束（成功或失败）都写一行 `ai_usage_logs`：`provider_id/provider_name/model` 快照、`prompt_tokens`、`completion_tokens`、`ok`、`latency_ms`、`error`、`request_preview`（本轮实际发给模型的用户消息；工具续跑时格式为「首问：… —— 本轮：…」，整体超过 2000 字截断）。保留 365 天，写入时顺带清理过期行。

- **真实 token**：SSE 中解析 `usage`（OpenAI `usage.prompt_tokens/completion_tokens`、Anthropic `message_start.usage.input_tokens` + `message_delta.usage.output_tokens`、Gemini `usageMetadata`），非 SSE 整包也走同一解析
- **估算兜底**：网关不回 usage 时按字符估算——中日韩字符 1 token/字，其余 4 字符 1 token（`estimate_tokens`）
- **准确率口径**：`success_rate = ok 调用 / 总调用`；`tool_success_rate` 通过 `ai_tool_logs JOIN ai_sessions ON provider_id` 归属到模型，不额外建表
- 命令：`ai_usage_stats(days)` 返回总量 + 按模型聚合 + 按天 token 趋势；`ai_usage_clear(all)` 默认只清过期，`all=true` 全清
- 前端入口：小HI 顶部「用量统计」直接路由到 `/ai/usage`（不再用弹窗）；页面为 6 张指标卡（调用次数 / 模型成功率 / 工具准确率 / 总 token / 估算费用 / 平均耗时）+ 每日 token 堆叠柱（>45 天按周聚合）+ Tab「调用记录 / 模型汇总 / 工具准确率」，默认 10 条/页（10/20/50/100），整行点击开右侧 560px 抽屉
- `ai_provider_test` 不计入统计（连通性测试不算真实用量）

## 模型目录与报错中文化

模型服务抽屉（`AiAssistant.vue`）里的四处要保持一致，别只改其一：

| 行为 | 实现 |
|---|---|
| 服务商 → 预置模型 | `presetModels`：OpenAI / DeepSeek / Kimi / 智谱 / 通义千问 / 硅基流动 / OpenRouter / Ollama 本地 / Claude / Gemini，用户从下拉选，避免手填错模型名 |
| Base URL 识别服务商 | `hostHints` 按域名匹配（`api.openai.com`、`deepseek.com` 等），粘贴地址后自动带出服务商 |
| 模型排序 | `naturalCompare()`：`localeCompare(..., { numeric: true })`，保证 `gpt-4o` 排在 `gpt-4` 之后 |
| 模型名校验提示 | `modelHint`：命中预置给推荐说明，未命中提示「可能不被支持」，但仍允许保存 |

报错统一在 Rust 侧转中文（`commands/ai.rs`），前端不得各自拼状态码文案：

- `humanize_api_error(status, message)`：401 / 402 / 403 / 404 / 429 / 5xx、`supported API model names are ...`（列出可用模型并指向「模型服务配置」）、上下文超长、模型下线
- `humanize_network_error(raw)`：DNS 解析失败 / 连接被拒 / 请求超时 / 证书校验失败，中文说明后附原始错误，方便排查自建网关
- 新增厂商或遇到新报错模式，先补这两个函数，再考虑前端提示

## 常见坑

| 现象 | 原因与处理 |
|---|---|
| 列表接口回显明文 Key | 忘了走掩码函数；检查 `ai_provider_list` 的序列化 |
| Claude 400 `max_tokens` | anthropic 协议必须显式传 `max_tokens` |
| Gemini 返回空 | 端点少了 `?alt=sse`，或 role 没映射成 `model` |
| 工具结果回灌后模型重复调用同一工具 | `autoDepth` 用完仍继续；提示词补「结果已拿到，直接总结」 |
| Claude 收到 system 报错 | system 必须提到顶层 `system` 字段，不能留在 messages 里 |
| 老库启动报 `no such column: kind` | 新列必须写进 `db/mod.rs::migrate()` |
| 工具卡片点运行后卡住 | `sending` 未复位，或 `aiExecute` 抛异常未被 try/catch 包住 |
| 模型输出 `<| DSLM | calls>` 等 XML 乱码 | 模型漏出原生工具语法；`liftNativeCalls()` 会抽出 JSON 还原成 fbin-tool 块，流式阶段在 `<\|` 处截断，prompt 里已明令禁用 |
| 用户只看到「无法回答」 | 模型一轮吐多个 tool_call，被硬规则 2 全部拒绝后反复重试；`rejections` 达到 2 直接终止并提示，此时可手动点工具卡片运行 |
| 进入小HI 页面卡顿 | 旧实现 `onMounted` 自动打开最近会话并整段渲染；现只预取服务商/会话列表，消息改为点击会话时加载，`parseSegments` 结果按内容缓存 |
| 对话里满屏「执行过程 · 1 个工具调用」 | 自动执行的中间轮被标记 `hidden`，模板只渲染 `visibleMessages`；历史会话按 `isExecutionNote()` 同样过滤，只回放最终回答 |
| 模型把请求数量说错 / 说「列表被截断」 | 列表类工具统一走 `summarizeList()`：`total` 在前、明细限 50 条，模型直接引用 total；prompt 的「数据准确性」禁止数条数 |
| 改了 AI 页面但桌面客户端没变化 | 安装版把前端打进二进制，只更新 `dist/` 无效；要 `npm run build` 后重新 `npm run build:win` / `build:mac`，开发期用 `npm run tauri dev` |
| 服务商报「模型不存在」但列表里有 | 多半是手填了带版本后缀的名字；让用户从 `presetModels` 下拉重选，再触发 `humanize_api_error` 的中文提示 |

## 校验

```powershell
cargo check          # 在 src-tauri
npm.cmd run build    # 根目录，vue-tsc + vite build
```

两条都过再交付；改动 AI 功能后，手动回归：发一条纯聊天、发一条会触发工具的消息、打开 `/ai/logs` 翻页并点开详情、打开 `/ai/usage` 切换 7/30/90/365 天并翻页。

## 样式与文案

- 名称统一「小HI」：导航、页面标题、占位符、system prompt 都用小HI；设置页卡片可写「AI 辅助（小HI）」，但不要再把「AI 助手」当成产品名
- 所有面向用户的文案中文、简洁、可执行；工具失败要说清「下一步怎么改」
