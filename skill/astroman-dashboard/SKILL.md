---
name: astroman-dashboard
description: AstroMan 工作台首页与「历史记录」页数据大屏的开发约定。改首页统计口径、调整引导 / 技巧 / FAQ 文案、改大屏图表、或需要解释批量运行与测试场景入口时使用。触发词：工作台、首页、Dashboard、数据大屏、图表、趋势、状态码分布、Top 接口、P95、批量运行、Runner、测试场景介绍。
---

# 工作台首页与数据大屏

v1.0.8 把首页拆成「工作台」和「数据大屏」两块，**大屏不在首页**，别往 `Dashboard.vue` 里再加图表：

| 位置 | 文件 | 内容 |
|---|---|---|
| 工作台 `/` | `views/Dashboard.vue` | 总览 + 6 个 KPI、快捷入口、测试场景介绍与示例、批量运行介绍与示例、产品介绍、能力清单、上手引导、常用操作、FAQ |
| 历史记录 `/history` | `views/History.vue` + `components/DashboardMetrics.vue` | 历史检索 / 清理 + 数据大屏（8 张卡，挂在列表下方） |

两个页面都调 `api.dashboardMetrics()`：首页只用它填 KPI 与概况文案，图表全部在 `DashboardMetrics.vue` 里。

## 数据口径

| 图表 | 表 | 口径 |
|---|---|---|
| 近 14 天请求趋势 | `request_history` | 近 14 天（含今天），按**本地日期**分桶；成功 = `status_code BETWEEN 200 AND 399` |
| 状态码分布 | `request_history` | 全量历史，2xx / 3xx / 4xx / 5xx，其余（NULL、<200、≥600）= 异常 |
| 请求方法分布 | `request_history` | 固定 GET/POST/PUT/DELETE/PATCH/其他 六档，一次 CASE 聚合 |
| 今日各时段请求量 | `request_history` | 今天本地时间 0–23 点，`strftime('%H', …, 'localtime')` 分桶，缺段补 0 |
| 响应耗时分布 | `request_history` | `<100ms` / `100-300ms` / `300ms-1s` / `1-3s` / `≥3s` 五档 |
| 项目调用量 Top 5 | `request_history` + `requests`→`collections`→`projects` | 按项目名分组取前 5，请求已删或未归属归入「未归类」 |
| 高频接口 Top 5 | `request_history` | 按 `request_name + method + url` 分组，按次数取前 5 |
| 最近压测 P95 走势 | `test_reports` | 按 `created_at` 倒序取最近 6 份，展示 `p95_rt` 与 `error_rate` |

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
6. 每组图表尽量一条 SQL 出；「今日时段」和「近 14 天」都要在 Rust 侧补零
7. 项目维度要 `LEFT JOIN` 链：`request_history.request_id → requests → collections → projects`，历史里的请求被删后 `COALESCE(p.name, '未归类')` 兜底
8. 改口径的顺序固定：`models.rs` → `types/models.ts` → `api/index.ts::dashboardMetrics()` → 页面，漏一处 `vue-tsc` 会报错（别用 `any` 绕过）

## 大屏组件约定（`components/DashboardMetrics.vue`）

- `.dash-grid` 固定 4 列（`repeat(4, minmax(0,1fr))`），趋势、时段、高频接口三张宽卡各占两列，其余一列；卡片内边距与字号沿用 `.fm-card` 体系
- **不引图表库**：柱高 / 条宽用峰值归一后的百分比，颜色只用 `--fm-*`（成功 / 主色 / 警告 / 危险 / 边框）
- 柱高归一：`height = value / peak * 100%`，`peak` 用 `Math.max(1, ...values)` 防除零
- 只要有一点数据就画图，全 0 时显示 `.dash-empty` 文案（"还没有请求记录…"），不要画空坐标轴
- 悬停信息用原生 `title`；百分比保留 1 位小数，耗时走 `fmtMs()`、整数走 `fmtInt()`，不要各写各的
- 报告行整行可点，跳 `/reports/<id>` 报告详情；「全部历史 / 全部报告」用文字链接

## 首页区块（`views/Dashboard.vue`）

模板顺序固定，新增区块插在对应注释位置，别打乱：

```text
PageHeader
console（总览 + 6 个 KPI：项目 / 接口 / 集合 / 环境 / 压测报告 / 历史请求）
kpi-grid（点击 KPI 跳到对应页面）
测试场景介绍（suite-band）+ 测试场景示例
批量运行介绍（runner-band）+ 批量运行示例
产品介绍（intro-band）
功能能力（feature-grid）
上手引导（steps）
常用操作（tips）
FAQ（faqs）
```

### 文案常量

| 常量 | 现状 | 约定 |
|---|---|---|
| `steps` | **9 步**：01 创建项目 → 02 配置环境 → 03 调试接口 → 04 发起压测 → 05 沉淀回归套件 → 06 签名与加密 → 07 导入现成接口 → **08 用航仔 提效** → 09 数据与备份 | 每步带 `action` 文案与 `go()` 跳转；AI 步走 `router.push({ name: 'ai' })` |
| `tips` | **15 张**技巧卡：变量优先级、3 个前置脚本示例、后置断言、cURL 导入、多标签、压测参数、批量运行转套件、压测用例门禁、4 张 AI 卡（自然语言操作 / 聊天与出方案 / 写脚本与查数据 / 用量与费用）、数据备份 | AI 卡写「点右侧屏幕中间的宇航员」；操作类注明「写入前先给计划、确认后执行」 |
| `faqs` | **19 条**常见问题 | 与 `Settings.vue` 的 AI 卡片、README 的 FAQ 同一口径；「能帮我做什么」必须同时写聊天 / 出方案与操作软件两种模式 |

改文案只改数据、不动模板。`{{username}}` 这类文本会被 Vue 当插值，示例必须放在 `script` 常量里再插值输出。

### 两个介绍区块

- **测试场景介绍（`suite-band`）**：讲「批量运行跑通 → 另存为测试套件 → 自动化测试页复跑」的路径，右栏四步与示例要和 `Automation.vue` 的真实流程一致（预检、失败策略、报告）
- **批量运行介绍（`runner-band`）**：讲 Runner 的定位与用法，事实以 `views/Runner.vue` 为准

| 事实 | 说明 |
|---|---|
| 多接口 | 勾选项目内任意多个接口，支持关键字过滤 |
| 数据来源 | 固定迭代次数，或导入 CSV（表头即变量名，一行一轮） |
| 变量语法 | `{{列名}}` 用于 CSV 列，`{{变量名}}` 用于环境变量，URL/Header/Body 都生效 |
| 单次上限 | 接口数 × 轮数 ≤ 5000，超出前端直接拦截 |
| 与压测区别 | Runner 顺序执行、逐条结果、面向回归与数据驱动；压测是固定并发施压，看 QPS/P95/错误率 |
| 与测试场景区别 | Runner 是临时勾选即跑、不落套件；测试套件可保存、可进 CI、有断言与报告 |

## 验证

```bash
npm run build                 # 类型 + 构建
npm run tauri dev             # 浏览器模式没有 IPC，KPI 与大屏都会落空态，必须用桌面端验证
```

只看 `npm run dev` 时 `invoke` 不可用，所有卡片都会落到空态，属预期。
