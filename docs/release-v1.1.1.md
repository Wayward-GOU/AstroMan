# AstroMan v1.1.1 发布检查表

- 文档版本：v0.3（W1–W5 代码与自动化测试完成，人工验收待执行）
- 基线提交：`21241b4`（`master-develop-v1.1.1`），tag `v1.1.1` 指向本文档所在提交
- 日期：2026-11-19
- 状态：**已打 tag 并推送**，由 tag `v1.1.1` 触发 Release 工作流产出安装包草稿。前端、Rust、E2E 与依赖审计已在本机通过；覆盖率为本机 LLVM 近似值，CI tarpaulin 仍需跑；7.3–7.10 人工矩阵及 R9/R10 真机项尚待 Windows 实机执行，不得标注「实机已验证」。

> 本表按《AstroMan 未完成事项技术方案》R1 / 附录 C 生成。人工项没有实际执行前不要勾「通过」，发布说明也不得声称「已验证」。

## 1. 版本信息

| 项 | 值 |
|---|---|
| 版本号 | 1.1.1（`package.json` / `src-tauri/Cargo.toml` / `tauri.conf.json` 一致） |
| 提交哈希 | 代码 `21241b4`；tag `v1.1.1` 指向本文档所在提交 |
| 构建产物 | 由 tag `v1.1.1` 触发 `.github/workflows/release.yml` 构建（Windows x64 / x86、macOS universal），产物在 GitHub Release 草稿中 |
| SHA256 | 以 GitHub Release 资产为准（本机未构建 Windows 安装包） |
| 构建环境 | Windows 10 x64 + Node 22 + Rust stable（本机；CI 不产出安装包） |

## 2. 自动化门禁

以下结果于 2026-11-19 在本机（macOS arm64，Rust stable）实际执行。E2E 使用临时 Node 20.19.4（本机默认 Node 18 低于 Playwright 要求）；覆盖率为 LLVM 近似测量，CI 仍以 Linux tarpaulin 门禁为准。

| 项 | 命令 | 结果 | 备注 |
|---|---|---|---|
| 版本号一致性 | `npm run version:check` | **通过**（三处均为 1.1.1） | `package.json` / `Cargo.toml` / `tauri.conf.json` |
| 前端 lint | `npm run lint` | **通过**（0 error / 0 warning） | 本次新增 ESLint 9 flat config |
| 前端单测 | `npm test` | **部分通过**（300 条通过；13 条基线环境失败） | 失败为干净基线已有的 `crypto is not defined` 与 `aiEval` 超时，非本次回归 |
| 类型检查 | `npx vue-tsc --noEmit` | **通过** | 与 `npm run build` 中的类型阶段一致 |
| 类型与构建 | `npm run build` | **通过** | 产物体积门禁由 CI 再跑 |
| 依赖安全审计 | `npm audit --omit=dev --audit-level=high --registry=https://registry.npmjs.org` | **通过**（0 vulnerabilities） | 默认 npmmirror 未实现 audit endpoint，需显式指定官方 registry |
| 发版安全断言 | `npm run security:check`（Node 20.19.4） | **通过** | 未开启 devtools，CSP 基础指令完整；Node 18 因 `import.meta.dirname` 报错 |
| Rust 格式 | `cargo fmt --check --all` | **通过** | 分支内历史格式已统一 |
| Rust 静态检查 | `cargo clippy --lib --all-targets -- -D warnings` | **通过**（0 warning） | 新增代码路径：backup / db / stress / report / cluster / capture |
| Rust 单测 | `cargo test --lib` | **通过**（455 passed / 0 failed） | 含 34 条集群用例（其中 4 条为集群验收） |
| UI 硬编码门禁 | `npm run ui:check` | **通过**（255/19/487/58/115，对比度 23 套全达标） | 基线按 v1.1.1 实测刷新 |
| 覆盖率 | `cargo llvm-cov --lib --summary-only` | **通过**（行 60.50% / 函数 57.02% / 区域 62.30%，阈值 25%） | macOS 本地近似；CI 的 Linux tarpaulin 仍须执行 |
| E2E 冒烟 | `npm run e2e:smoke` | **通过**（6/6，14.1s） | macOS + 系统 Chrome；使用临时 Node 20.19.4 |

### 2.1 本次 Rust 改动需要重点复核的用例

```powershell
cd src-tauri

# R2 备份密钥闭环
cargo test --lib backup::
# R3 增量落库与崩溃恢复
cargo test --lib db::tests::crash_recovery
cargo test --lib db::tests::saved_job
cargo test --lib commands::stress::tests::job_samples_persist_incrementally
cargo test --lib commands::stress::tests::finished_job_cleans_temp_rows
cargo test --lib commands::stress::tests::capacity_mode_skips_detail_records
# R5 / R11 外部指标字段与导入
cargo test --lib commands::report::tests
cargo test --lib services::external_metrics

# R7 抓包归档
cargo test --lib capture::

# R9 / R10 分布式二期三期
cargo test --lib services::cluster::

# 全量
cargo test --lib
cargo clippy --lib --all-targets -- -D warnings
cargo fmt --check --all
```

## 3. 人工验收

| 编号 | 项 | 结论 | 执行人 | 机器 | 日期 | 证据 |
|---|---|---|---|---|---|---|
| 7.3 | 多窗口并行 | 待执行 | 苟默 | Windows 10 x64 / 8 核 | — | — |
| 7.4 | 报告长 URL | 待执行 | 苟默 | Windows 10 x64 / 8 核 | — | — |
| 7.5 | 环境变量 | 待执行 | 苟默 | Windows 10 x64 + 本机抓包 | — | — |
| 7.6 | 4h 长稳 | 待执行 | 苟默 | 独立施压机 | — | — |
| 7.8 | 能力检测 | 待执行 | 苟默 | 独立施压机 | — | — |
| 7.9 | 数据库迁移 | 待执行 | 苟默 | 另装 v1.0.9 生成 v14 库 | — | — |
| 7.10 | 窗口生命周期 | 待执行 | 苟默 | Windows 10 x64 | — | — |

执行要点（摘自技术方案 1.3.2）：

- **7.3**：三个计划各开一个 `stress-*` 窗口同时启动；进度互不覆盖、停一个不影响其余、产出 3 份报告且 `plan_id` 正确。
- **7.4**：选超 300 字符带 query 的请求压测；详情 / HTML / Excel 完整可复制，字号 ≥ 12px。
- **7.5**：`{{base_url}}/api` 按环境解析；请求行不得出现 `{{`；缺变量时阻止启动并列出变量名。
- **7.6**：4h 长稳模板跑通；数据完整、无降档异常、报告体积在预估内。建议与 R3「中途 kill 恢复」联调一起跑。
- **7.8**：点检测 20–60s 返回；中途取消一次无残留端口与任务；重启后仍展示结果。
- **7.9**：必须用 v1.0.9 安装包真实生成 v14 库后覆盖升级，不要改当前库版本号伪造；计划/报告数不变，删接口后计划保留且 `request_id` 为 NULL。
- **7.10**：运行中关窗再重开任务继续；重开恢复进度不重复上报；退出应用触发多任务确认；取消全部后无半份报告。

本地（macOS）已执行以下自动化等价检查，仅用于提前暴露代码问题，**不替代 Windows 实机人工结论**：

| 对应项 | 本地检查 | 结果 |
|---|---|---|
| 7.5 环境变量 | `cargo test --lib util::tests::render` | 7 通过（自定义变量、内置变量、嵌套路径、函数变量） |
| 7.9 数据库迁移 | `cargo test --lib db::tests` | 10 通过，含 `upgrades_v14_plans_to_set_null_and_adds_capacity_table` |
| R9 / R10 协议与合并 | `cargo test --lib services::cluster::` | 34 通过（进程内双节点、失联、编排、权重、v1 兼容） |

无法在本机执行、必须留到 Windows 实机的项：7.3 多窗口、7.4 报告长 URL 渲染、7.6 4h 长稳、7.8 能力检测、7.10 窗口生命周期、R9 防火墙下 mDNS、R10 真机断网 / 跨网段。

### 3.1 本次新增、建议一并冒烟的项

| 项 | 步骤 | 通过标准 |
|---|---|---|
| 备份密钥同机 | 立即备份 → 恢复到本机 | 列表显示「密钥：一致」，恢复后无额外提示，密钥可用 |
| 备份密钥异机 | A 机备份 → B 机恢复 | 默认拦截并提示受影响条数；确认后恢复成功，设置页说明「从备份 xxx 恢复后密钥不匹配」 |
| 老备份兼容 | 用 1.1.1 之前的备份恢复 | 不阻断，提示「密钥未知」，恢复后检查密钥可用性 |
| 压测崩溃恢复 | 运行到约 30 分钟 `kill -9` 后重启 | 报告列表出现「运行中断 / 不完整」，曲线到中断点，不能选作基线，健康结论为 fail |
| 容量模式内存 | 容量预设跑高并发 | 不保留逐请求明细，`persistDropped` 可见（有丢弃时）且压测 tick 不阻塞 |
| 分布式编排 | 主节点选 ramp 30s → hold 60s，2 个从节点加权启动 | 每节点阶段按权重分配，主 / 从界面当前阶段一致，报告 meta 带 `orchestration` |
| mDNS 发现 | Windows 允许应用通过防火墙后从节点浏览 | 20s 内列出主节点；拒绝权限时给可读原因，手动地址仍可用 |
| 从节点断线重连 | 运行中给从节点断网 60s 后恢复 | 状态 `reconnecting` → `running`，重试次数递增，数据连续；超过 5 分钟宽限则报告标记不完整 |
| 能力权重 | 查看设备历史表并点击「采用建议权重」 | 显示 QPS / 并发 / 核数；应用后节点权重变化，prepare 不再被默认值覆盖 |
| 抓包上游代理 | 公司代理环境开启上游代理压测并抓包 | HTTP / HTTPS 均能经过上游；CONNECT 直通白名单命中时不再二次解密 |
| 外部指标导入 | 导入 CSV 后查看报告并导出 Excel | 序列叠加到报告图表，导出文件包含外部指标 sheet / 列 |

## 4. 已知问题与豁免

| 问题 | 影响 | 处理 | 版本 |
|---|---|---|---|
| 7.3–7.10 人工验收未执行 | 不能声称「实机已验证」 | 版本冻结前一天排期，证据路径填入第 3 节 | 1.1.1 发布前 |
| R9 / R10 真机项未验证 | mDNS 在 Windows 防火墙下、真机断网重连、跨网段、混合协议版本未实测 | 按 3.1 新增冒烟项执行；通过前不宣称「实机已验证」 | 1.1.1 发布前 |
| CI tarpaulin 覆盖率未执行 | 本机只有 LLVM 近似值（行 60.50%），与 CI 统计口径可能略有差异 | CI 的 Linux tarpaulin 按 `tarpaulin.toml` 跑；低于 25% 直接失败 | 1.1.1 发布前 |
| 前端 13 条基线测试失败 | `npm test` 非全绿，易被误判为回归 | 干净基线即存在（`crypto is not defined`、`aiEval` 超时），修复单独立项 | 长期 |
| v17–v19 三档迁移 | 本版把 `user_version` 从 16 升到 19，新增压测采样、抓包归档、集群设备与编排列 | 迁移逐档 `IF NOT EXISTS` / `COLUMN_MIGRATIONS`，已有 v14→19 升级与崩溃恢复用例 | 1.1.1 |
| 崩溃恢复报告为近似值 | 分位与均值按每秒样本加权回算，无逐请求明细、错误列表和 TopN | 报告标记 `partial` + fail 结论 + 禁止选作基线；仅用于「数据截至中断点」的趋势判断 | 1.1.1 |
| 首次接入 ESLint 产生批量机械改动 | 约 40 个文件补 `void` / 清理未使用符号 | 只启用正确性规则，不引入 Prettier；review 时按文件域看，避免格式化噪音 | 1.1.1 |
| 同机备份密钥指纹可伪造 | 指纹只做提示，不构成安全边界 | 文档与设置页明确「不导出主密钥」；真正的安全靠不把密钥放进备份 | 长期 |

## 5. 回滚方案

- **安装包回退**：直接安装上一版 `AstroMan_1.1.0_x64-setup.exe`。覆盖安装前建议先用 1.1.1 导出一次项目 JSON / 备份。
- **数据库降级限制**：v1.1.1 把 `PRAGMA user_version` 升到 **19**（v17 压测采样与外部指标、v18 抓包归档、v19 集群设备与编排）；旧版应用（DB_VERSION = 16）打开该库会直接拒绝并提示「数据库由更高版本创建」。回退旧版前必须：
  1. 用 1.1.1 创建一份 pre-restore 快照；
  2. 回退安装包后，用该快照恢复（快照的 `user_version` 仍是 19，旧版同样打不开）——因此**正确做法是回退到升级前的 v16 备份**，而不是 1.1.1 生成的快照；
  3. 若没有 v16 备份，只能保留 1.1.1 或联系维护者手工降级（会丢弃 v17–v19 新表数据）。
- **自动快照**：升级前 `snapshot_before_migrate` 会生成 `pre-migrate-*.db`（v16），这是回退旧版时应使用的文件。
- **恢复标记**：回退后如存在 `restore.pending`，删除该文件即可取消待恢复。
