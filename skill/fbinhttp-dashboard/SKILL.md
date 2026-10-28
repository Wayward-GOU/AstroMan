---
name: fbinhttp-dashboard
description: FbinHttp 首页数据大屏与批量运行介绍区块的开发约定。改首页统计口径、新增一块图表、调整大屏布局，或需要解释 Runner 用法时使用。触发词：首页、数据大屏、Dashboard、图表、趋势、状态码分布、Top 接口、P95、批量运行、Runner 示例。
---

# 首页数据大屏

首页（`src/views/Dashboard.vue`）除了快捷入口和历史列表，还有两块「讲清楚产品 + 看懂数据」的内容：

| 区块 | 作用 |
|---|---|
| 数据大屏 | 近 14 天请求趋势、状态码分布、高频接口 Top 5、最近压测 P95 走势 |
| 批量运行介绍 | 解释 Runner 是什么、四步用法、CSV 参数化示例，并给出入口 |

两块都**只读现有表**，不新增数据表、不写库。

## 数据来源与口径

| 图表 | 表 | 口径 |
|---|---|---|
| 请求趋势 | `request_history` | 近 14 天（含今天），按**本地日期**分桶；成功 = `status_code BETWEEN 200 AND 399` |
| 状态码分布 | `request_history` | 全量历史，2xx / 3xx / 4xx / 5xx，其余（NULL、<200、≥600）= 异常 |
| 请求方法分布 | `request_history` | 固定 GET/POST/PUT/DELETE/PATCH/其他 六档，一次 CASE 聚合 |
| 今日时段分布 | `request_history` | 今天本地时间 0–23 点，`strftime('%H', …, 'localtime')` 分桶，缺段补 0 |
| 响应耗时分布 | `request_history` | `<100ms` / `100-300ms` / `300ms-1s` / `1-3s` / `≥3s` 五档 |
| 项目调用量 | `request_history` + `requests`→`collections`→`projects` | 按项目名分组取前 5，请求已删或未归属归入「未归类」 |
| 高频接口 | `request_history` | 按 `request_name + method + url` 分组，按次数取前 5 |
| 压测走势 | `test_reports` | 按 `created_at` 倒序取最近 6 份，展示 `p95_rt` 与 `error_rate` |

- 时间字段统一毫秒时间戳，SQL 里用 `date(created_at / 1000, 'unixepoch', 'localtime')` 分桶，与 Rust 侧 `chrono::Local` 对齐
- 清空历史（`history_cleanup`）后大屏自动归零，不需要额外清缓存
- 压测请求也会写历史，因此趋势图包含压测产生的请求量

## 后端聚合

命令：`commands/system.rs::dashboard_metrics`，返回 `DashboardMetrics`（`models.rs`）：

```text
DashboardMetrics { days, codeBuckets, topRequests, reportPoints,
                   methodBuckets, hourBuckets, latencyBuckets, topProjects }
DailyPoint  { date, total, success, avgRt }
CodeBucket  { label, count }            // 状态码 / 方法 / 耗时共用
HourPoint   { hour, count }
ProjectPoint{ name, count, avgRt, successRate }
TopRequest  { name, method, url, count, avgRt, successRate }
ReportPoint { id, name, p95Rt, errorRate, totalRequests, createdAt }
```

实现要点：

1. 起点 `today - 13 天`，用 `Local.from_local_datetime(&start.and_hms_opt(0,0,0))` 取本地零点毫秒，不要用 UTC
2. SQL 只返回有数据的日期，**Rust 侧按 14 天循环补 0**，否则柱状图会缺格、宽度跳动
3. 聚合列一律 `COALESCE(..., 0)`；`AVG` 结果读 `f64`，`COUNT/SUM` 读 `i64`
4. 状态码用 `SUM(CASE WHEN ... THEN 1 ELSE 0 END)` 五路一次查出，不要五次查询
5. 报告列表 `ORDER BY created_at DESC LIMIT 6` 后再 `reverse()`，让走势从左到右按时间递增
6. 每组图表尽量一条 SQL 出：用 `SUM(CASE WHEN … THEN 1 ELSE 0 END)` 一次拿多个分档，不要一档一个查询
7. 「今日时段」和「近 14 天」都要在 Rust 侧补零，保证柱子数量固定、宽度不跳动
8. 项目维度要 `LEFT JOIN` 链：`request_history.request_id → requests → collections → projects`，历史里的请求被删后 `COALESCE(p.name, '未归类')` 兜底
9. 注册只需在 `lib.rs` 的 `generate_handler!` 增加 `commands::system::dashboard_metrics`（`system` 模块已存在）

## 大屏布局（紧凑 4 列）

`.dash-grid` 固定 `repeat(4, minmax(0,1fr))`，宽图占两列：

| 卡片 | 跨列 |
|---|---|
| 近 14 天请求趋势 / 今日各时段 / 高频接口 / 压测走势 | 2 |
| 状态码分布 / 方法分布 / 耗时分布 / 项目 Top 5 | 1 |

- 卡片内边距 13×15，趋势图 128px，时段图 96px，横向条形高 7px
- `@media (max-width: 1180px)` → 2 列（宽图仍占满两列）；`@media (max-width: 980px)` → 单列，同时把 `grid-column` 复位为 `auto`
- 横向条形统一 `.bar-list / .bar-row / .bar-track / .bar-fill`，颜色档位 `ok|info|warn|danger|other` 对应 `--fm-success/primary/warning/danger/border-strong`

改动口径时必须同步：`models.rs` → `types/models.ts` → `api/index.ts` 的 `dashboardMetrics()` → 页面模板，漏一处 `vue-tsc` 会报错（这是好事，别用 `any` 绕过）。

## 前端图表约定

- **不引图表库**：全部用 flex + 百分比高度/宽度 + `--fm-*` 变量绘制，主题切换自动跟随
- 柱高按峰值归一：`height = value / peak * 100%`，`peak` 用 `Math.max(1, ...values)` 防止除零
- 趋势图用两段 `trend-seg`（成功/失败）按百分比高度堆叠；时段图用单色 `.hour-bar` 贴底对齐
- 只要有一丁点数据就画图，全 0 时显示 `.dash-empty` 文案（"还没有请求记录…"），不要画空坐标轴
- 时段图的刻度每 3 小时显示一次（`h.hour % 3`），其余占位保持列宽一致
- 悬停信息用原生 `title`（`trendTitle()`），避免为提示框引入浮层组件
- 报告行整行可点击：`router.push('/reports/' + id)`；"全部历史 / 全部报告"用文字链接跳转
- 百分比统一保留 1 位小数，耗时统一走 `fmtMs()`，整数走 `fmtInt()`，不要各写各的格式化

### 页面内的位置

```text
PageHeader
console（总览 + 6 个 KPI）
launch-grid（快捷入口）
批量运行介绍（runner-band：左文案 + 右四步）
批量运行示例（runner-example：CSV / Body / 结果三列）
intro-band（产品概览）
feature-grid（能做什么）
main-grid（我的项目：左列表 + 右侧最近压测/本机状态/常用工具）
数据大屏（dash-grid：趋势 1.5fr + 状态码 1fr，下面两列各 1 份）——紧跟「最近请求」之前
最近请求
使用说明 / FAQ
```

响应式断点（与首页其他区块一致）：

- `max-width: 1180px`：`.console`、`.main-grid`、`.dash-grid` 全部单列
- `max-width: 980px`：`.runner-band` 单列、`.example-grid` 单列

### 使用说明与 AI 引导

底部「使用说明 / FAQ」由 script 里的三个常量驱动，改文案只改数据、不动模板：

| 常量 | 内容 | 现状 |
|---|---|---|
| `steps` | 八步上手：01 建项目 → 02 配环境 → 03 调接口 → 04 压测 → 05 签名加密 → 06 导入接口 → **07 用小HI 提效** → 08 数据与备份 | 每步带 `action` 文案与 `go()` 跳转，AI 步走 `router.push({ name: 'ai' })` |
| `tips` | 12 张技巧卡：变量优先级、3 个前置脚本示例、后置断言、cURL 导入、多标签、压测参数、**3 张 AI 卡**（自然语言操作 / 写脚本与查数据 / 用量与费用）、数据备份 | AI 卡必须写「点右侧屏幕中间的机器人」，不能写右下角 |
| `faqs` | 15 条常见问题，含「小HI 能帮我做什么」「AI 服务怎么配置、密钥是否安全」「AI 用了多少 token、花了多少钱」 | 与 `Settings.vue` 的 AI 卡片、README 的 FAQ 保持同一口径 |

改 AI 入口/权限/计费口径时，同时检查：本文件的 `steps/tips/faqs`、`Settings.vue` 的 AI 卡片、`AiAssistant.vue` 的工具文案、README「已实现功能」。

### 新增一块图表

1. 在 `dashboard_metrics` 里加聚合字段（能一条 SQL 出就不要加命令），补 `models.rs` 结构
2. 同步 `types/models.ts` 与 `api/index.ts`
3. 在 `Dashboard.vue` 的 `.dash-grid` 里加一张 `.fm-card.dash-card`：`dash-card-head` + 图表区 + `dash-empty`
4. 归一化/格式化写成 `computed` 或纯函数（`xxxPeak` / `xxxBarHeight`），不要在模板里算
5. 加对应媒体查询，确认 1180px 以下不横向溢出
6. `npm run build` + `cargo check` 验证

## 批量运行（Runner）介绍区块

介绍文案要与 `src/views/Runner.vue` 的**真实能力**一致，改 Runner 时同步这里：

| 事实 | 说明 |
|---|---|
| 多接口 | 勾选项目内任意多个接口，支持关键字过滤 |
| 数据来源 | 固定迭代次数，或导入 CSV（表头即变量名，一行一轮） |
| 变量语法 | `{{列名}}` 用于 CSV 列，`{{变量名}}` 用于环境变量，URL/Header/Body 都生效 |
| 单次上限 | 接口数 × 轮数 ≤ 5000，超出前端直接拦截 |
| 结果 | 状态码、耗时、大小、断言通过/失败、错误，支持导出 |
| 与压测区别 | Runner 顺序执行、逐条结果、面向回归与数据驱动；压测是固定并发施压，看 QPS/P95/错误率 |

示例区注意：`{{username}}`、`{{base_url}}` 这类文本直接写进模板会被 Vue 当插值，**必须放在 `script` 常量里**（`csvExample` / `bodyExample` / `runnerUrlExample` 等），再用插值输出。

## 验证

```bash
npm run build                 # 类型 + 构建
cd src-tauri && cargo check   # 关闭沙箱
npm run tauri dev             # 浏览器模式没有 IPC，大屏会走空态，必须用桌面端验证
```

只看 `npm run dev` 时 `invoke` 不可用，所有卡片都会落到空态，属预期。
