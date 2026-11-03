# 自动化测试（测试场景）

v1.0.8 新增「测试场景」：把接口回归沉淀成可保存、可复用、可进 CI 的套件。桌面端与 `astroman-cli` 使用同一套 Rust 执行内核、同一张数据库，结果一致。

## 1. 入口

- 侧边栏「自动化测试」（测试场景）：套件列表 → 用例编排 → 运行 → 报告。
- 批量运行页「另存为测试套件」：把当前勾选的接口、顺序与数据文件迁成套件草稿。
- CLI：`astroman-cli`（随安装包分发，Windows 便携版根目录也有）。

## 2. 用例类型

> 示例项目内置套件「示例回归（httpbin）」：3 个可直接运行的 HTTP 用例，分别断言查询参数回显、JSON 字段回显与 Bearer 认证结果，用来演示断言与运行报告。新库首次启动写入；老库在示例项目尚无套件时也会补齐。跑之前确认本机可访问 `https://httpbin.org`（环境「本地环境」的 `base_url`）。

| 类型 | 用途 | 必填 |
|---|---|---|
| `request` | 调接口 + 声明式断言 + Python 前后置钩子 | 绑定接口 |
| `python` | 纯脚本用例（造数、签名、数据库校验） | `mainScript` |
| `stress` | 内联压测配置，跑完自动做 SLA / 基线判定 | `stress_json` |

断言支持：`status`、`durationMs`、`header`、`body`、`regex`、`jsonPath`。
Python 钩子：`pre`（可改请求 / 写变量）、`post`（读响应 / 加断言）、套件 `setup / teardown`；脚本通过 `context.json` 读取上下文，通过 `result.json` 回写。

> 套件不执行接口自带的 JS 前置 / 后置与环境脚本；预检、运行报告和日志都会列出被跳过的入口。需要迁移时改写为 Python 钩子。

## 3. 压测用例与门禁

```json
{
  "name": "下单压测",
  "method": "POST",
  "url": "{{baseUrl}}/api/orders",
  "headers": [],
  "bodyType": "json",
  "body": "{\"amount\":1}",
  "concurrent": 20,
  "durationSec": 60,
  "sla": { "p95Ms": 500, "p99Ms": 1000, "errorRate": 0.5, "minRps": 50 },
  "slaOverride": null,
  "baselineReportId": null
}
```

- 四项 SLA 任一超标 → 用例 `failed`，失败原因写入断言与 JUnit `<failure>`。
- `baselineReportId` 指向历史压测报告：P95 / P99 超基线 1.2 倍、平均 QPS 低于基线 80%、错误率高于基线 +0.5% → `failed`。
- `slaOverride` 可覆盖配置里的 SLA；`waitTimeoutSec` 指定等待其他压测结束的最长秒数。
- 压测与手工压测互斥；停止套件运行会在一秒内中断压测。
- 报告 HTML / JSON 内嵌核心指标与 SLA 断言，并给出原压测报告编号。

## 4. 套件包（跨机器 / CI）

单文件 `*.astromantest.json`，包含套件、用例、请求快照与数据集；导入后不依赖目标机器的接口库。

```bash
# 桌面端：套件栏「导出套件」；左侧列表顶部按钮导入
# CLI 导入（同名默认追加时间后缀）
astroman-cli suite import order.astromantest.json --project "订单服务" --on-conflict rename
```

- 导入含 Python 脚本的套件前，必须在「设置 → 网络与安全」打开 Python 脚本开关（允许执行本地 Python 脚本），否则直接拒绝。
- 变量值默认不导出：CI 用 `--env-file` 注入，缺 secret 会明确报错。

## 5. CLI 与退出码

```bash
astroman-cli test run --suite 订单回归 --env test \
  --report html,json,junit --out ./artifacts

astroman-cli test validate --suite 订单回归
astroman-cli test list
astroman-cli report export --run 128 --out ./artifacts
astroman-cli python check
```

| 退出码 | 含义 |
|---|---|
| 0 | 全部通过（含 skipped） |
| 1 | 断言失败 / SLA 未达标 |
| 2 | 执行错误（脚本崩溃、超时、请求异常、error 用例） |
| 3 | 配置错误（套件不存在、解释器缺失、数据集非法、缺 secret） |
| 4 | 被取消 / 中断 |
| 5 | 数据库被占用或版本不兼容 |

## 6. 产物与保留

每次运行在数据目录的 `test-reports/<runId>/` 下生成：

```text
report.html     自包含，可离线打开
report.json     schemaVersion 1.0，机器可读
junit.xml       Jenkins / GitHub Actions 直接消费
summary.txt     控制台摘要
logs/           每个用例的完整日志
files/          附件副本
```

默认每个套件保留最近 50 次报告，启动时清理无主目录。

## 7. AI 协同

航仔现有 22 个测试域工具：套件 / 用例的读写、启动与停止运行、运行结果、报告导出，以及数据集 CRUD、套件包导入导出、运行列表、附件列表 / 导出、预检。写入与启动一律先给方案并确认；`test_run_start` 不套 60 秒工具超时，启动后立即返回 `runId`，再用 `test_run_get` 查看失败原因。

## 8. 数据目录

报告、工作目录与附件全部位于设置里的数据目录下，库内只存相对路径；切换自定义数据目录后历史仍可读。CLI 会读取 `storage.json`，默认打开与桌面端同一个库。
