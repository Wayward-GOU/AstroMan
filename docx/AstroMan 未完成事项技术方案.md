# AstroMan 未完成事项技术方案

- 文档版本：v0.2（实现完成情况见附录 D）
- 代码基线：`master-develop-v1.1.1`，`package.json` / `Cargo.toml` = 1.1.1，`DB_VERSION = 19`
- 日期：2026-11-19（初稿）；2026-11-19 更新完成情况
- 覆盖范围：2026-11-17 评估报告批次一/二遗留项、V1.1.1 方案中未收尾的发布验收与 P2/V1.2 项、分布式压测二期/三期、抓包上游代理实装
- 使用方式：每个事项独立成章，章节内固定顺序为「问题 → 目标与边界 → 方案 → 数据/接口契约 → 代码改造点 → 测试 → 验收 → 风险」。可直接按章节拆 PR。

标记说明：

- 优先级：`P0` 发布前必须；`P1` 本版本内尽量；`P2` 可延后。
- 量级：`S`（≤0.5 人日）/ `M`（1–3 人日）/ `L`（>3 人日）。
- 状态：`待做` / `进行中` / `已完成`。

---

## 零、总览

### 0.1 事项清单

| 编号 | 事项 | 来源 | 现状 | 优先级 | 量级 | 目标版本 | 依赖 |
|---|---|---|---|---|---|---|---|
| R1 | 1.1.1 发布检查表与人工验收 | V1.1.1 方案 7.3–7.10 | 自动化门禁与检查表就绪；Windows 人工验收待执行 | P0 | M | 1.1.1 | 无 |
| R2 | S10 备份密钥闭环 | 评估报告 §3.5 | 已完成 | P0 | M | 1.1.1 | 无 |
| R3 | T8 压测增量落库 | 评估报告 §3.3 | 已完成 | P1 | M | 1.1.1 | R1 |
| R4 | A6 前端 lint 门禁 | 评估报告 §3.7 | 已完成 | P1 | S | 1.1.1 | 无 |
| R5 | 3.6.1 外部监控字段闭环 | V1.1.1 方案 3.6.1 | 已完成 | P1 | S | 1.1.1 | 无 |
| R6 | S2 抓包上游代理实装 | 评估报告 §3.4 | 已完成 | P1 | L | 1.1.1 | 无 |
| R7 | T7 抓包落库与会话化 | 评估报告 §3.3 | 已完成 | P2 | L | 1.1.1 | R6 |
| R8 | 1.5 并发透视（1.5.1/1.5.2/1.5.5） | V1.1.1 方案 1.5 | 已完成 | P2 | M | 1.1.1 | R3 |
| R9 | 分布式压测二期 | V1.1.1 方案 8.6 | 已完成；Windows 防火墙下 mDNS 待验证 | P2 | L | 1.1.1 | 无 |
| R10 | 分布式压测三期 | V1.1.1 方案 8.6 | 已完成；真机断网 / 跨网段验收待执行 | P2 | L | 1.1.1 | R9 |
| R11 | 3.6.1/3.6.2 指标导入与映射 | V1.1.1 方案 3.6 | 已完成 | P2 | M | 1.1.1 | R5 |

> 说明：R5 是「字段闭环」，R11 才是「导入与映射」，两者拆开可以先把契约定死，后续导入不改报告结构。

### 0.2 批次划分

| 批次 | 内容 | 前置 | 退出条件 |
|---|---|---|---|
| W1 | R2、R4、R5 | 无 | 三项代码合并，CI 新增 lint 门禁并全绿 |
| W2 | R1（检查表 + 自动化回归）、R3 | W1 | 检查表归档；长压测中途断电不丢已采样数据 |
| W3 | R6、R8 | W1 | 公司代理环境可抓包；并发透视页可用 |
| W4 | R7、R11 | R6、R5 | 抓包可会话化检索；外部指标可叠加到报告 |
| W5 | R9、R10 | W3 | 二期设备列表可用；三期计划级编排可用 |

> 初稿计划 W3 之后属于 1.2；实际 W1–W5 代码已全部并入 1.1.1，原 1.2 / 1.3 的发布批次顺延，R6–R11 的真机验收并入 1.1.1 发布门禁。
>
> 2026-11-19 更新：W1–W5 的代码与自动化测试均已完成（明细见附录 D）；W2 中的 R1 仅剩 Windows 实机人工矩阵，须在发布前执行。

### 0.3 数据库迁移规划

本方案涉及两次迁移，建议一次做完，避免版本号来回跳：

| 版本 | 内容 | 章节 |
|---|---|---|
| v17 | `stress_job_samples` / `stress_job_meta`；`test_reports.external_metrics_json`（预留，可空） | 3、9 |
| v18 | `capture_sessions` / `capture_flows` / `capture_bodies` | 6、7 |
| v19 | `cluster_devices`（二期设备缓存）、`cluster_jobs.orchestration_json` | 10 |

> 实际落地为三次迁移，而不是初稿设想的两次合并：`DB_VERSION = 19`。`schema.sql` 每次启动重放，新表用 `IF NOT EXISTS`，增量列走 `COLUMN_MIGRATIONS`，迁移幂等。

迁移铁律沿用 `skill/astroman-backend`：列写进 `COLUMN_MIGRATIONS`、索引必须在新列之后建、老库给 `DEFAULT`。

---

## 一、R1 发布验收与检查表

### 1.1 问题

V1.1.1 方案 7.7 要求「按 7.1–7.6 汇总 1.1.1 发布检查表」，仓库里没有对应交付物：`docs/` 下只有 `release-v1.0.8.md` / `release-v1.0.9.md`。7.3 / 7.4 / 7.5 / 7.6 / 7.8 / 7.10 六项人工实机验收也未执行，方案第九章 9.3 自己列为未开工。

这带来两个具体风险：发布说明里不能声称「已验证」，以及回归窗口内发现问题时没有统一的回流入口。

### 1.2 目标与边界

目标：让「哪些验过、谁验的、在哪台机器、证据在哪」可追溯。不目标：不追求自动化覆盖全部人工项——多窗口生命周期、4h 长稳、真实双机这类必须真机。

### 1.3 方案

**1.3.1 交付物：`docs/release-v1.1.1.md`**

结构固定为五段（附录 C 给了完整模板）：

1. 版本信息：版本号、提交哈希、构建产物名称与 SHA256、构建机系统。
2. 自动化门禁结果：CI 四个 job 的结论与链接，附本地复核命令输出摘要。
3. 人工验收矩阵：7.3–7.10 逐项，列「结论 / 执行人 / 机器 / 日期 / 证据路径」。
4. 已知问题与豁免：未通过项必须写原因与后续版本号，不允许留空。
5. 回滚方案：安装包回退路径、数据库降级限制（`version > DB_VERSION` 拒绝打开）。

**1.3.2 验收执行矩阵**

| 编号 | 项 | 环境要求 | 步骤要点 | 判定 |
|---|---|---|---|---|
| 7.3 | 多窗口并行 | Windows 10 x64、8 核 | 三个计划各开一个 `stress-*` 窗口，同时启动 | 进度互不覆盖；停一个不影响其余；产出 3 份报告，`plan_id` 各自正确 |
| 7.4 | 报告长 URL | 同上 | 选一条超 300 字符带 query 的请求压测 | 详情/HTML/Excel 完整可见可复制，字号 ≥12px |
| 7.5 | 环境变量 | 同上 + 本机抓包 | `{{base_url}}/api` 按所选环境解析，抓包看实际请求 | 无 `{{` 字样进入请求行；缺变量时阻止启动并列出变量名 |
| 7.6 | 压测方法论 | 独立施压机 | 4h 长稳模板跑通 | 数据完整、无降档异常、报告体积在预估内 |
| 7.8 | 能力检测 | 同上 | 点检测，中途取消一次，再完整跑一次 | 20–60s 返回；取消后无残留端口与任务；重启后仍展示 |
| 7.9 | 数据库迁移 | **另装一份 v1.0.9 生成 v14 库** | 覆盖升级到 1.1.1 | 计划/报告数不变；无接口计划可保存；删接口后计划保留且 `request_id` 为 NULL |
| 7.10 | 窗口生命周期 | Windows 10 x64 | 运行中关窗再重开；退出应用触发多任务确认 | 任务继续；重开恢复进度不重复上报；取消全部后无半份报告 |

7.9 的 v14 老库不要用当前库改版本号伪造——必须用旧安装包真实生成，否则测不出 `test_plans` 重建分支。

**1.3.3 缺陷回流**

验收发现的问题按严重度分流：阻断发布 → 当版修复；不阻断 → 写入 `docs/release-v1.1.1.md` 第 4 段并挂到下一版；方案文档同步在第九章追加一条落地差异。

### 1.4 测试

自动化部分已覆盖（`cargo test --lib`、`npm run test`、`npm run build`、`vue-tsc --noEmit`）。本章新增的是人工项，不产生单测。

### 1.5 验收

- `docs/release-v1.1.1.md` 存在，五段齐全，人工矩阵无空白行。
- 每项证据至少一张截图或一段日志，路径可访问。

### 1.6 风险

| 风险 | 处理 |
|---|---|
| 4h 长稳要占掉半天机器 | 排进版本冻结前一天，与 R3 联调一起跑 |
| v14 老库难复现 | 保留旧安装包与一份脱敏 v14 库样本在 `testdata/` 外置目录，不入库 |
| 验收执行人只有作者本人 | 至少在 7.3 / 7.10 上找一个非作者复核窗口行为，避免路径依赖 |

---

## 二、R2 备份密钥闭环（S10）

### 2.1 问题

`src-tauri/src/commands/backup.rs` 与 `src-tauri/src/backup.rs` 里没有任何 `key_id` / 试解密逻辑。`backup::validate`（`backup.rs:248`）只做三件事：文件存在、`PRAGMA quick_check`、`user_version <= DB_VERSION`。`backup::mark_restore` 只写一个 `restore.pending` 路径，`apply_pending_restore` 直接覆盖数据库。

后果：把 A 机器备份恢复到 B 机器时，`ai_providers.api_key` 与 `environment_variables.value` 的密文由 A 的主密钥加密，B 的主密钥解不开。恢复「成功」，但所有密钥变成不可用，界面上表现为「解密失败」或被静默当空值，用户很难判断发生了什么。

而 `secret.rs` 里其实已有判定材料：`app_meta.key_check`（哨兵密文）与 `app_meta.key_fingerprint`（sha256 前 8 字节，`secret.rs:1096`）。缺的是把它接到备份/恢复流程上。

### 2.2 目标与边界

目标：恢复前就能判断「这份备份的密钥和本机是否一致」，不一致时明确告知影响面并让用户选择。不目标：不自动迁移密钥——跨机器还原主密钥等同于把密钥搬运出去，安全性更差。

### 2.3 方案

**2.3.1 备份期：读取并记录密钥标识**

备份是 `VACUUM INTO` 生成的一致性单文件快照（`backup.rs:95` 起的 `create`），它天然包含 `app_meta`。因此**不需要旁挂 meta 文件**，直接从快照里读即可，好处是备份文件自描述、复制到任何地方都不丢信息。

```rust
// backup.rs 新增
pub struct BackupKeyInfo {
    /// 备份创建时的密钥指纹（app_meta.key_fingerprint，读不到为 None）
    pub key_id: Option<String>,
    /// 备份里是否存在可解密的 enc:v1: 样本
    pub has_encrypted_fields: bool,
}

fn inspect_key(conn: &Connection) -> BackupKeyInfo { /* 读 app_meta + 扫一行 env_variables */ }
```

`BackupItem` 增加两个字段（`backup.rs:82` 的结构体）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `key_id` | `Option<String>` | 备份的密钥指纹；老备份（1.1.1 之前）可能没有 |
| `key_matches` | `bool` | 与本机当前密钥是否一致；无密钥信息时按 `true` 处理并标记 `unknown` |

`key_matches` 需要拿到本机指纹，因此 `secret.rs` 要开一个只读访问器：

```rust
// secret.rs：key_fingerprint 目前是私有 fn（:1100），加公开包装
pub fn current_key_fingerprint() -> Option<String> {
    store().and_then(|s| s.fingerprint())
}
```

**2.3.2 恢复期：先校验，再决定**

`backup_restore`（`commands/backup.rs:38`）现在的流程是 `validate → mark_restore`。改为：

```text
resolve_backup → validate（quick_check + 版本）
  → inspect_key（读备份指纹）
  → 比较本机指纹
     一致 → 直接 mark_restore
     不一致 → 返回 KeyMismatch 错误（不写标记），前端弹窗说明：
              「该备份由另一台机器/另一把密钥创建，恢复后 XX 条密钥需要重新填写。
                继续恢复（保留密文，需手工重填）/ 取消」
     备份无指纹（老版本） → 按 unknown 处理，提示「无法判断，恢复后请检查密钥」
```

需要新增一个带确认参数的入口，避免前端绕过：

```rust
#[tauri::command]
pub fn backup_restore(
    state: State<'_, AppState>,
    name: String,
    accept_key_mismatch: Option<bool>,   // 默认 false
) -> Result<BackupRestorePreview, String>
```

返回值改为预览结构 `BackupRestorePreview { item, key_id, key_matches, affected_secrets }`，`affected_secrets` 是备份里 `enc:v1:` 的条数（`env_variables` + `ai_providers` 各一次 `COUNT`）。

注意还有第二个写 `restore.pending` 的入口：`commands/backup.rs:165` 的 `backup_restore_from_target` 经 `services/backup_target.rs:777` 同样会写恢复标记。如果只拦 `backup_restore`，从异地备份目标恢复就能绕过密钥校验，两个入口必须共用同一段校验逻辑（抽成 `prepare_restore(item, accept) -> Result<..>`）。

**2.3.3 恢复后：把来源写进 meta，让设置页能解释**

`apply_pending_restore`（`backup.rs:286`）在 `open_pool` **之前**执行（`state.rs:405` 早于建池），此时没有连接池可用，函数签名也只有 `(config_file, data_dir)`。因此写 meta 有两种落点：

- 方案 A（推荐）：在 `apply_pending_restore` 复制完成后，用 `Connection::open(&db_file)` 自开一次连接写 `app_meta`，随后立即 drop，赶在建池之前完成。
- 方案 B：函数返回恢复来源路径，由 `AppState::new` 在建池之后调用 `meta_set` 写入；改动面更大但复用现有连接。

两条 key：

| key | 值 |
|---|---|
| `restore.source` | 备份文件名 |
| `restore.at` | 毫秒时间戳 |

`SecretStore::bootstrap`（`secret.rs:153`）在 `verify_key` 失败时已经会置 `mismatch`（赋值在 `secret.rs:222-240`，`mismatch()` 只是 getter）。增强一处：若 `restore.source` 存在且 `mismatch` 非空，设置页文案从「密钥校验失败」改成「从备份 `xxx` 恢复后密钥不匹配，请重新填写密钥或导入密钥包」，并给出两个按钮。这样把「静默不可用」变成「可解释、可修复」。

**2.3.4 不做的事**

- 不把主密钥写进备份。备份文件经常放网盘，带密钥等于把库和钥匙一起送出去。
- 不在恢复时自动 `encrypt_plaintext` 重加密。用错钥匙重加密会覆盖正确密文，`skill/astroman-backend` 已有这条铁律。

### 2.4 数据契约

| 位置 | 变化 |
|---|---|
| `BackupItem`（`backup.rs:82`） | 新增 `key_id: Option<String>`、`key_matches: bool`、`key_state: String`（`match` / `mismatch` / `unknown`） |
| `BackupItem` TS 类型（`src/types/models.ts`） | 同步三字段 |
| `app_meta` | 只新增两个 key，不改表结构 |
| 命令 `backup_restore` | 入参加 `acceptKeyMismatch`，返回 `BackupRestorePreview` |

### 2.5 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/backup.rs` | 新增 `inspect_key`、`BackupKeyInfo`；`validate` 返回里带 `key_id`；`BackupItem` 扩字段；`apply_pending_restore` 写 `restore.source` / `restore.at` |
| `src-tauri/src/secret.rs` | `key_fingerprint` 增加 `SecretStore::fingerprint()` 与模块级 `current_key_fingerprint()`；`mismatch` 文案区分「恢复来源」 |
| `src-tauri/src/commands/backup.rs` | `backup_restore` 改签名与流程；新增预览结构 |
| `src-tauri/src/lib.rs` | `generate_handler!` 无需新增（改的是既有命令签名） |
| `src/api/index.ts` | `backupRestore(name, acceptKeyMismatch?)` |
| `src/components/RemoteBackupPanel.vue` 或设置页备份区 | 恢复前弹窗；列表展示「密钥：一致 / 不一致 / 未知」 |
| `src/types/models.ts` | `BackupItem`、`BackupRestorePreview` |

### 2.6 测试

| 用例 | 断言 |
|---|---|
| `backup_records_key_id` | 创建备份后 `inspect_key` 能读回当前指纹 |
| `restore_rejects_wrong_key_by_default` | 手工把备份 `app_meta.key_fingerprint` 改成别的值，`backup_restore(name, None)` 返回错误且**没有**写 `restore.pending` |
| `restore_accepts_mismatch_when_confirmed` | 同上，`accept_key_mismatch = Some(true)` 时写入标记，预览里 `key_matches = false` |
| `restore_tolerates_backup_without_fingerprint` | 删掉备份里的 `key_fingerprint`，`key_state = unknown`，默认放行并提示 |
| `restore_writes_meta_after_apply` | `apply_pending_restore` 后 `meta_get("restore.source")` 有值 |

指纹篡改要直接在备份文件上 `UPDATE app_meta`，不要 mock，否则测不到 SQL 路径。

### 2.7 验收

- 同机：备份 → 恢复到本机，全程无提示，密钥可用。
- 异机：A 机备份 → B 机恢复，默认被拦截并说明影响条数；确认后恢复成功，设置页显示「恢复后密钥不匹配」与修复入口。
- 老备份（无指纹）：恢复不阻断，提示「无法判断」。

### 2.8 风险

| 风险 | 处理 |
|---|---|
| 指纹可被伪造 | 指纹只做「提示」不做「安全边界」；真正的安全靠不导出密钥 |
| 用户看不懂影响面 | 预览里给出具体条数，而不是抽象警告 |
| 恢复标记与预览不一致 | `acceptKeyMismatch` 必须落到同一次 `mark_restore` 调用内，不能前端校验后再调一次无参版本 |

---

## 三、R3 压测增量落库（T8）

### 3.1 问题

`commands/stress.rs` 的压测主循环把整个运行期的数据攒在内存里：`samples`（:1770）、`errors`（:1771）、`requests`（:1772）、`slow_requests`（:1773）。只有跑到结束才由 `save_report`（:2495）在一个事务里写 `test_reports` / `report_samples` / `report_errors` / `report_requests`。

三个后果：

1. 长稳 4h 中途断电、崩溃、被强杀，已采样数据全丢，只剩一条 `recover_interrupted_runs` 标记的失败记录。
2. 单机并发上限是 5000，4h 降频后采样点也有数万个，全部驻留内存；配合 `MAX_REQUEST_RECORDS = 50000` 的明细缓冲，内存峰值不可控。
3. 报告只在结束后可见，用户中途想看趋势只能依赖实时进度事件；关窗后事件断流，回看不了。

这与 V1.1.1 方案 6.3「运行期不写库，结束后单事务写报告」的原则冲突。要改的不是原则本身，而是把「运行期写库」限定为**只写追加型表**，不触碰任何聚合与报告主体。

### 3.2 目标与边界

目标：崩溃后仍能拿到已采样部分；内存占用与运行时长相解耦。

不目标：不做「崩溃后自动续跑」。压测续跑会产生时间轴断裂的报告，误导性大于价值。恢复出来的只能是「不完整的部分报告」，并且必须显式标注。

### 3.3 方案

**3.3.1 分层**

```text
运行期：内存聚合（不变） → 每 N 秒把增量写 stress_job_samples（追加）
结束：从 stress_job_samples 读回（或直接用内存）→ 单事务写 test_reports 全套 → 删除该 job 的临时行
崩溃：启动时扫描 test_reports 缺失但 stress_job_samples 存在的 job → 生成「不完整」报告
```

关键设计：`stress_job_samples` **不复用 `report_samples`**，因为 `report_samples.report_id` 是 `NOT NULL` 且外键指向 `test_reports`，运行期还没有 report 行。新增独立表，结束后合并删除，逻辑简单且不影响既有读路径。

**3.3.2 写入线程**

不阻塞压测 tick。在 `run_load` 里起一个独立 `tokio::task`，通过 `mpsc` 接收每 tick 的增量：

```rust
enum PersistMsg {
    Sample(ReportSample),
    Error(ReportError),
    Flush,              // 每 10s 主动落盘一次
    Finish,
}

// 写线程：攒到 50 条或 10s 或收到 Flush 时，单事务批量 INSERT
```

背压策略：channel 有界（1024）。满了直接丢并累加 `metrics.persist_dropped`，绝不阻塞主循环——压测的优先级是发出请求，不是留痕。

**3.3.3 表设计**

```sql
CREATE TABLE IF NOT EXISTS stress_job_samples (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id        INTEGER NOT NULL,
    elapsed_sec   INTEGER NOT NULL,
    concurrent    INTEGER NOT NULL DEFAULT 0,
    qps           REAL    NOT NULL DEFAULT 0,
    avg_rt        REAL    NOT NULL DEFAULT 0,
    p50_rt        REAL    NOT NULL DEFAULT 0,
    p95_rt        REAL    NOT NULL DEFAULT 0,
    p99_rt        REAL    NOT NULL DEFAULT 0,
    ttfb_avg      REAL    NOT NULL DEFAULT 0,
    body_avg      REAL    NOT NULL DEFAULT 0,
    body_p95      REAL    NOT NULL DEFAULT 0,
    bytes         INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    error_count   INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_job_samples_job ON stress_job_samples(job_id, elapsed_sec);
```

配套 `stress_job_meta`（每个 job 一行，记录配置快照与开始时间，供崩溃恢复时还原报告头）：

```sql
CREATE TABLE IF NOT EXISTS stress_job_meta (
    job_id        INTEGER PRIMARY KEY,
    plan_id       INTEGER,
    plan_name     TEXT,
    config_json   TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    status        TEXT NOT NULL DEFAULT 'running'
);
```

**3.3.4 崩溃恢复**

启动时（`db::migrate()` 之后、`recover_interrupted_runs` 同一处）执行：

```text
SELECT * FROM stress_job_meta WHERE status = 'running'
  → 对每个 job：若 test_reports 里没有对应 end_time 的报告
      → 用已落盘的 samples 生成报告，status = 'interrupted'
      → 报告 config_json 写 partial: true, restoredAt, droppedSamples
      → 删除该 job 的临时行
```

报告列表里 `status='interrupted'` 要单独配色并标注「运行中断，数据截至 HH:MM:SS」，不能与正常报告混排。`reportNarrative` 的健康结论必须直接给 fail，不允许按 SLA 判定——它没跑完。

**3.3.5 内存侧配套**

`records_dropped` 现在只在超过 `MAX_REQUEST_RECORDS = 50000` 之后才累加（`stress.rs:1827-1830`）。但 `capacity` 预设实际映射到 `Auto`（`stress.rs:1174`），启动时按「并发 × 10」估算 QPS 定档（`stress.rs:1474-1493`）——估算低于 2000 时仍从 `Full` 起步，要等运行期 QPS 触发降档（`stress.rs:1900-1948`）才切到 `Sample` / `Off`。也就是说**容量模式在低并发估算或前十几秒仍会全量攒明细**，高并发下这正是内存最容易堆起来的窗口。

建议为 `capacity` 单独定一档：直接映射为 `Off`（只统计均值、不构造明细记录；`stress.rs:999-1003` 已有「Off 时直接 return」的保护），把「按估算定档 + 运行期降档」留给 `auto`。这样容量模式的内存占用与 QPS、运行时长都解耦，也与 1.6.1「容量模式只统计均值，不保留明细」的文案一致。若担心丢失排障能力，可在设置页提示「需要慢请求明细请切到定位模式」。

### 3.4 数据契约

| 位置 | 变化 |
|---|---|
| `db/schema.sql`、`db/mod.rs::migrate()` | 两张新表 + 索引，`DB_VERSION` 16 → 17 |
| `commands/stress.rs` | `PersistMsg`、写线程、`persist_dropped` 计数（写进 `config_json.metrics`，Rust 侧没有独立的 `RuntimeMetrics` 结构体，该类型只在 `src/types/models.ts:1214`） |
| `commands/stress.rs::with_runtime_metrics`（:2458） | 合并 `persistDropped` / `restoredAt` / `partial` 到快照 |
| `commands/report.rs` | `REPORT_COLUMNS`（:12-17）已含 `status`，列表无需改；只需在详情读 `partial` |
| `src/views/Reports.vue`、`ReportDetail.vue` | `interrupted` 徽标与提示条 |

### 3.5 代码改造点

| 文件 | 具体 |
|---|---|
| `src-tauri/src/commands/stress.rs:1770` 附近 | 起写线程；tick 末尾 `try_send` 增量 |
| `src-tauri/src/commands/stress.rs:2495` `save_report` | 结束前先 `Flush` 并 join 写线程，保证不丢最后一批 |
| `src-tauri/src/db/mod.rs` | v17 迁移；崩溃扫描函数 `recover_interrupted_stress_jobs(state)` |
| `src-tauri/src/state.rs` | `stress_jobs` 注册表新增 `status` 回写（已有 `finish`） |
| `src/utils/reportNarrative.ts` | `partial` 时强制 fail 结论 |

### 3.6 测试

| 用例 | 断言 |
|---|---|
| `job_samples_persist_incrementally` | 起一个 3s 压测，运行中直接查表能看到 ≥2 行 |
| `crash_recovery_builds_partial_report` | 手工写 5 行 sample + 一条 running meta，调用恢复函数，生成报告且 `partial=true`、`status='interrupted'` |
| `finished_job_cleans_temp_rows` | 正常跑完后 `stress_job_samples` 里该 job 为 0 行 |
| `capacity_mode_skips_detail_records` | `captureMode=capacity` 时 `records_dropped` 增长，`requests` 不增长 |
| `writer_backpressure_does_not_stall` | channel 填满后主循环 tick 间隔仍为 1s（用时间断言） |

### 3.7 验收

- 4h 长稳运行到 30 分钟时 `kill -9`，重启后可看到一份标注「数据截至 30:00」的报告，曲线完整到中断点。
- 正常运行内存峰值较改前下降（容量模式）。
- 报告列表中 `interrupted` 报告不能参与横向对比的基线选择。

### 3.8 风险

| 风险 | 处理 |
|---|---|
| 写库与压测抢 IO | 批量写 + WAL；写线程用独立连接，不占主池 |
| 长时间 job 的临时表膨胀 | `elapsed_sec` 唯一索引 + 结束时删除；同时在 `retention_days` 清理里带上孤儿行 |
| 崩溃恢复把半条报告当成有效结论 | 强制 `partial` 标注 + 禁止选作基线 + 健康结论直接 fail |

---

## 四、R4 前端 lint 门禁（A6）

### 4.1 问题

`package.json` 已有 `test` / `build` / `e2e` / `ui:check` / `security:check` / `version:check` / `sbom` / `ai:eval` 等脚本，但没有 ESLint / Prettier，也没有 `lint` 脚本；`.github/workflows/ci.yml` 的 `frontend` job 只跑 `npm audit` / `version:check` / `test` / `build` / `security:check` / `sbom` / 体积门禁。

Rust 侧有 `cargo fmt --check` + `clippy -D warnings`，前端没有任何静态规则。实际后果：`RequestEditor.vue`、`AiAssistant.vue` 这类数千行文件里，未使用变量、浮动 Promise、`v-for` 缺 key、响应式解构错误这类问题只能靠人眼。

### 4.2 目标与边界

目标：给前端补一条与 Rust 对等的静态门禁，且**不产生大规模格式化 diff**。不目标：不追求统一代码风格到每一行——这会把 review 淹没在格式化噪音里，也会让 `git blame` 失真。

### 4.3 方案

**4.3.1 选型**

| 工具 | 版本 | 作用 |
|---|---|---|
| `eslint` | 9.x（flat config） | 主引擎 |
| `typescript-eslint` | 8.x | TS 规则与类型感知规则 |
| `eslint-plugin-vue` | 10.x | `.vue` 模板与脚本 |
| `@vue/eslint-config-typescript` | 14.x | Vue + TS 组合预设 |

不引入 Prettier。理由：项目已有 `ui:check` 的硬编码色与对比度脚本，格式诉求集中在少数字号/缩进规则上，引入 Prettier 会一次性重排 22 个视图，收益不匹配成本。格式化交给编辑器，CI 只管正确性规则。

**4.3.2 规则集（第一版只要这些）**

```js
// eslint.config.js
export default [
  ...vueTsPreset,
  {
    rules: {
      // 正确性，直接 error
      'no-unused-vars': 'off',                    // 交给 TS 版本
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      'no-floating-promises': 'off',
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      'vue/no-mutating-props': 'error',
      'vue/require-v-for-key': 'error',
      'vue/no-use-v-if-with-v-for': 'error',
      'vue/valid-v-model': 'error',
      // 可疑但先不阻断
      'vue/no-unused-components': 'warn',
      '@typescript-eslint/no-explicit-any': 'warn',
    },
  },
  { ignores: ['dist/**', 'node_modules/**', 'src-tauri/**', 'test-results/**'] },
]
```

`@typescript-eslint/no-floating-promises` 需要 type-aware linting，配置 `parserOptions.project = './tsconfig.json'`；首次运行耗时会到 30–60s，可接受（CI 只跑一次）。

**4.3.3 渐进接入**

分两步，避免第一天就红一片：

1. 第一步：`npm run lint` 只输出报告不阻断（`--max-warnings 9999`），先把问题清到 0 error。
2. 第二步：CI 里加 `npm run lint` 作为独立 step，`--max-warnings 0`。

`package.json` 增加：

```json
"lint": "eslint . --max-warnings 0",
"lint:fix": "eslint . --fix"
```

**4.3.4 CI 接入**

插在 `frontend` job 的 `npm ci` 之后、`npm run test` 之前，失败即停，省掉后续构建时间。

### 4.4 代码改造点

| 文件 | 改动 |
|---|---|
| `eslint.config.js` | 新增 |
| `package.json` | 四个 devDependencies + `lint` / `lint:fix` |
| `.github/workflows/ci.yml` | `frontend` job 增加 lint step |
| `docs/development.md` §测试与质量门禁 | 前端命令补 `npm run lint` |
| `skill/astroman-testing` | 门禁从「四道」改为「五道」 |
| `.eslintignore`（或 config 内 `ignores`） | 排除 `dist` / `test-results` / `src-tauri` |

### 4.5 验收

- `npm run lint` 本地 0 error。
- CI 上故意提交一个 `await` 缺失的 Promise，lint job 必须失败。
- 全量 lint 耗时 < 90s。

### 4.6 风险

| 风险 | 处理 |
|---|---|
| type-aware 规则把大文件跑爆内存 | Node `--max-old-space-size=4096`；必要时对 `src/views/AiAssistant.vue` 单独降级 |
| 首次清理工作量被低估 | 先只启用 `error` 级规则，warn 级留到下一版；一次 PR 只改一个文件域 |
| 与 `vue-tsc` 重复 | 分工明确：类型错误归 `vue-tsc`，代码正确性归 ESLint，两者都不可省 |

---

## 五、R5 外部监控字段闭环（3.6.1）

### 5.1 问题

`src/types/models.ts:1096` 定义了 `ExternalMetricSeries`，`:1144` 在 `ReportConfigSnapshot` 里挂了 `externalMetrics?`。但 Rust 侧写报告时（`commands/stress.rs:1525` 的 `config_snapshot`）根本没有这个字段，`commands/report.rs` 的 `REPORT_COLUMNS`（:12-17）与 `JSON_COLUMNS`（:20-21）也没有对应列。

也就是说「预留字段」只在 TypeScript 里成立：前端声明了一个永远为 `undefined` 的字段，Rust 不产出、数据库不存。后续做导入时，等于从零开始补契约。

### 5.2 目标与边界

目标：把契约在 Rust 与数据库两侧真正落地为可空字段，保证「现在不实现导入，但实现时不需要改表、不需要改老报告读取」。不目标：不实现导入 UI（那是 R11）。

### 5.3 方案

**5.3.1 契约定死**

```rust
// models.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalMetricPoint {
    /// 相对压测开始的秒数，与 report_samples.elapsed_sec 同一时间轴
    pub t: i64,
    pub v: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalMetricSeries {
    /// 数据来源标识：csv:<文件名> / prometheus:<expr 摘要>
    pub source: String,
    /// 指标名，如 cpu.usage / gc.pause / db.pool.wait
    pub metric: String,
    /// 单位：percent / ms / count / bytes
    pub unit: String,
    pub points: Vec<ExternalMetricPoint>,
}
```

与 TS 侧现有定义逐字段对齐；`t` 用相对秒而不是绝对时间戳，避免跨机器时区与时钟偏移问题。

**5.3.2 落库位置**

原方案（V1.1.1 3.6.1）的意图是把字段塞进 `config_json`，理由是「不涉及数据库迁移」。当前 TS 也确实把 `externalMetrics` 定义在 `ReportConfigSnapshot` 里（`src/types/models.ts:1103` 起）。

但按这个做法实现有两个问题：`config_json` 已经承载 20 多个配置字段，塞入采样序列属于语义污染；单条序列可能上万点，调报告详情时解析 `config_json` 会明显变重，而列表查询也会顺带解析。

因此调整为独立列 `external_metrics_json TEXT`，并保留向后兼容的读取顺序：

```text
读：external_metrics_json 非空 → 用它
    → 为空 → 回退读 config_json.externalMetrics（兼容按原方案写入的未来数据）
    → 都没有 → None
写：只写独立列；config_json 不再写该字段（老字段保留解析能力，不再产出）
```

这样既不破坏原方案的「旧报告可读」，也避免了把时序数据混进配置快照。

**5.3.3 写入与读取**

- 写入：`save_report` 的 `ReportPayload` 增加 `external_metrics: &[ExternalMetricSeries]`，当前恒传空数组。
- 读取：`report_detail` 先取列，为空再回退 `config_json.externalMetrics`；两处都无则 `None`。
- 前端：`ReportConfigSnapshot.externalMetrics` 标记 `@deprecated`，新增 `StressReport.externalMetrics` 顶层字段，详情页优先用顶层。
- 导出：`reportExport.ts` 与 Excel 暂不消费，留 R11。

### 5.4 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/models.rs` | 两个结构体 |
| `src-tauri/src/db/schema.sql` + `db/mod.rs` | `test_reports.external_metrics_json TEXT`，v17 迁移 |
| `src-tauri/src/commands/stress.rs` | `ReportPayload` 增字段；`save_report` INSERT 增列 |
| `src-tauri/src/commands/report.rs` | **`JSON_COLUMNS`（:20-21）增列**，不要放进 `REPORT_COLUMNS`——列表查询会带上 REPORT_COLUMNS 的字段，把大 JSON 塞进去会让 `report_list` 变慢；`report_detail` 反序列化 |
| `src/types/models.ts` | `StressReport` 增顶层 `externalMetrics`；`ReportConfigSnapshot.externalMetrics` 标 `@deprecated` |

### 5.5 测试

| 用例 | 断言 |
|---|---|
| `report_without_external_metrics_reads_none` | 写入空数组 → 读回 `None` |
| `report_round_trips_external_metrics` | 写 2 条序列各 3 点 → 读回逐点相等 |
| `broken_external_metrics_json_does_not_break_report` | 手工写入非法 JSON → 详情仍返回，字段为 `None` |
| `legacy_config_external_metrics_falls_back` | 列置空、`config_json` 里放一条序列 → 读回该序列 |
| `old_report_without_column_is_readable` | v16 库升级后读老报告，字段为 `None` |

### 5.6 验收

- 老库升级后报告列表与详情行为不变。
- 数据库里 `external_metrics_json` 为 `NULL` 时，前端不出现空的「外部指标」区块。

### 5.7 风险

| 风险 | 处理 |
|---|---|
| 大序列撑大报告 | 写入侧限制 ≤ 10 条序列、每条 ≤ 3600 点，超出截断并记 `truncated` |
| 与 `config_json` 的双重来源 | 明确 `externalMetrics` 只在顶层列，`config_json` 不再重复存 |

---

## 六、R6 抓包上游代理与 SSL 直通（S2）

### 6.1 问题

`capture/mod.rs` 的 `CaptureConfig.upstream_proxy` 一直是契约字段：`capture_start` 收下它、`CaptureStatus` 回显它，但 `capture/proxy.rs` 组装 hudsucker 时只调 `.with_rustls_connector(...)`，参数从未参与代理构建。上一轮已加 `ensure_supported` 闸门（`capture/mod.rs:77`），非空即返回 `E_UPSTREAM_UNSUPPORTED`，**目前是「明确拒绝」而不是「支持」**。

同时 `ssl_hosts` 也还是死字段（`skill/astroman-capture` 已注明），表现为：做了证书固定的站点（银行、部分 App）在开启抓包后直接断网，因为 MITM 无法解密又不放行。

### 6.2 目标与边界

目标：公司代理环境下能抓包；证书固定站点可直通（不看明文，但至少不断网）。

不目标：不做全局 VPN/TUN 劫持；不做 HTTP/3（QUIC）解密；不做代理认证的凭据管理 UI（第一版只支持 `http://user:pass@host:port` 内联）。

### 6.3 方案

**6.3.1 上游代理：自定义 connector**

hudsucker 0.25 的 `ProxyBuilder::with_http_connector<C>` 接受任意 `hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static`。默认的 `with_rustls_connector(provider)` 内部就是构造一个「TCP + TLS」的 connector。我们要做的是在这个 connector 外面套一层 CONNECT 隧道。

推荐用 `hyper-http-proxy` 的 `ProxyConnector<C>`：它包装一个内层 connector，对 `https` 目标先向代理发 `CONNECT host:port`，隧道建立后再交给内层做 TLS；对 `http` 目标直接用 absolute-form 请求。这正是公司代理的语义。

```rust
// capture/upstream.rs（新增）
pub fn build_connector(
    upstream: Option<&ProxySpec>,
    provider: rustls::crypto::CryptoProvider,
) -> Result<BoxedConnector, String> {
    // 1. 内层：证书校验用系统根（复用现有 C-23 计划），暂用 webpki
    //    具体构造方法以 hyper-rustls 0.27 实际签名为准（with_native_roots 系列）
    let tls = hyper_rustls::HttpsConnectorBuilder::new()
        .with_provider_and_native_roots(provider)?   // ← 实施时按实际 API 核对
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    // 2. 无上游：直接返回 tls
    // 3. 有上游：套 ProxyConnector
    match upstream {
        None => Ok(Box::new(tls)),
        Some(spec) => {
            let proxy = HttpProxy::from(spec.uri())?;  // 支持 user:pass@
            Ok(Box::new(ProxyConnector::from_proxy(tls, proxy)?))
        }
    }
}
```

`BoxedConnector` 用 `enum Connector { Direct(Tls), Upstream(ProxyConnector<Tls>) }` 手写 `tower::Service<Uri>`，避免引入 `Either` 依赖。注意：`with_http_connector` 与 `with_rustls_connector` 二选一，一旦走自定义 connector，**TLS 栈就由我们自己负责**，必须保证无上游分支与现状行为一致（这是本方案最大的回归风险）。

依赖变更：

| crate | 版本 | 用途 |
|---|---|---|
| `hyper-http-proxy` | 0.1.x | CONNECT 隧道 + absolute-form |
| `hyper-rustls` | 0.27 | 与 hudsucker 同版本，避免类型不兼容 |
| `hyper-util` | 0.1 | 已在依赖树，显式声明 |

> 实施前先做一个 spike：用 `hyper-http-proxy` 0.1 与 hudsucker 0.25 的 `hyper 1.7` 做一次编译验证，确认 `ProxyConnector<Tls>` 满足 `Connect` 约束；不满足就先降级为「手写 `Service<Uri>` + `tokio::net::TcpStream` + `httparse` 校验 CONNECT 响应」的兜底实现。

**6.3.2 代理配置解析**

```rust
pub struct ProxySpec {
    pub scheme: String,      // http / https
    pub host: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
}

impl ProxySpec {
    pub fn parse(raw: &str) -> Result<Self, String>;  // 复用 settings.rs 的代理解析，若有
    pub fn uri(&self) -> Uri;
}
```

不支持 `socks5` 第一版——hudsucker 的 connector 没有 SOCKS 支持，硬接需要再包一层 `tokio-socks`。配置里遇到 `socks5://` 返回 `E_UPSTREAM_SCHEME` 并说明。

**6.3.3 SSL 直通白名单**

hudsucker 的 `HttpHandler` 提供两个钩子（已在 0.25 文档确认）：

```rust
fn should_intercept_connect(&mut self, ctx: &HttpContext, req: &Request<Body>) -> impl Future<Output = bool> + Send;
fn should_intercept_tls(&mut self, ctx: HttpContext, hello: ClientHello<'_>) -> impl Future<Output = bool> + Send;
```

- `should_intercept_connect` 返回 `false` → 该 CONNECT 直接建立 TCP 隧道转发，不解密。
- `should_intercept_tls` 返回 `false` → TLS 握手层直通。

在 `CaptureHandler` 上实现：

```rust
async fn should_intercept_connect(&mut self, ctx: &HttpContext, req: &Request<Body>) -> bool {
    let host = ctx.client_addr; // 注意：host 从 req.uri() 取
    !self.passthrough.matches(host_of(req))
}
```

匹配规则：

| 规则写法 | 含义 |
|---|---|
| `*.apple.com` | 后缀匹配，含子域 |
| `api.example.com:443` | 精确到端口 |
| `10.0.0.0/8` | CIDR（复用 `netparse.ts` 的 Rust 版，或新增 `ipnet` 依赖） |

直通的流量要**在列表里留一条提示**：`FlowBrief` 增加 `passthrough: bool`，UI 上显示「已直通（未解密）」。否则用户会以为抓包漏了。

**6.3.4 与既有拦截链的顺序**

现有顺序是「弱网 → 断点 → 本地映射 → 上游」（`handler.rs`）。直通流量不进入 handler，因此：

- 断点、Map Local、弱网对直通域名**不生效**，配置界面必须写明。
- 直通域名也不能被 `ignorePatterns` 与 `captureStatic` 二次影响；两者独立。

### 6.4 数据与接口契约

| 位置 | 变化 |
|---|---|
| `CaptureConfig`（`capture/mod.rs:51`） | `upstream_proxy` 启用；`ssl_hosts` 语义改为直通白名单 |
| `CaptureStatus`（`capture/mod.rs:95`） | 增加 `upstreamActive: bool`、`passthroughCount: u64` |
| `FlowBrief`（`capture/store.rs:57`） | 增加 `passthrough: bool`（`#[serde(default)]`） |
| `capture_start` 入参 | 契约不变，只去掉 `E_UPSTREAM_UNSUPPORTED` 闸门 |
| `src/types/capture.ts` | 同步三个字段 |

### 6.5 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/Cargo.toml` | 新增 `hyper-http-proxy` / `hyper-rustls`；确认 `hyper-util` 版本与 hudsucker 一致 |
| `src-tauri/src/capture/upstream.rs` | 新增：`ProxySpec`、`Connector` 枚举与 `Service<Uri>` 实现 |
| `src-tauri/src/capture/proxy.rs:24-32` | 用 `with_http_connector(build_connector(...)?)` 替换 `with_rustls_connector` |
| `src-tauri/src/capture/mod.rs:77` | 删除 `ensure_supported` 的上游拒绝分支，保留启动前 `ProxySpec::parse` 校验 |
| `src-tauri/src/capture/handler.rs` | 实现两个 `should_intercept_*`；直通流量写 `FlowRecord` 时标 `passthrough` |
| `src/views/Capture.vue` 设置抽屉 | 上游代理输入（含 `user:pass@` 提示）、直通白名单编辑、错误就地提示 |
| `src/components/CaptureInterceptDrawer.vue` | 不需要改，但要加一句「直通域名不参与断点/映射」 |

### 6.6 测试

| 用例 | 做法 | 断言 |
|---|---|---|
| `proxy_spec_parses_auth_and_scheme` | 纯函数 | `http://u:p@h:3128` → auth 正确；`socks5://` 报错 |
| `upstream_proxy_forwards_http` | 本机起一个最小 CONNECT 代理（`TcpListener`，记录首行） | 抓包代理转发后，上游代理收到 `CONNECT host:port` |
| `upstream_proxy_forwards_https` | 同上 + 自签目标 | 隧道建立，目标收到请求 |
| `direct_mode_unchanged` | 不配上游 | 与现状用例 `proxy_forwards_http_and_ca_is_stable` 一致 |
| `passthrough_host_skips_mitm` | `ssl_hosts = ["*.example.test"]` | 目标返回的证书是**服务端真实证书**，列表里 `passthrough=true` |
| `passthrough_does_not_apply_to_other_hosts` | 同上，访问别的域名 | 正常解密，`passthrough=false` |

HTTPS 直通的断言要比较证书指纹，不能只看「能连上」——MITM 也能连上。

### 6.7 验收

- 公司代理环境：系统代理指向 AstroMan，上游填公司代理，能抓到外网 HTTPS 明文。
- 证书固定站点加入直通白名单后，App 不再断网，列表出现「已直通」条目。
- 不配上游时，行为与 1.1.1 完全一致（回归用例 + 手工冒烟）。

### 6.8 风险

| 风险 | 处理 |
|---|---|
| 自定义 connector 破坏现有 TLS 行为 | 无上游分支必须与现状等价；先做 spike 与回归用例再改动 |
| 上游代理需要 NTLM/Kerberos 认证 | 第一版只支持 Basic；文档明确边界，返回可读错误 |
| 直通后用户以为抓不到是 bug | 列表显式标注 + 设置页说明「直通流量不解密」 |
| 依赖版本冲突（hyper-rustls 0.27） | 用 `cargo tree -i hyper-rustls` 确认与 hudsucker 同源版本 |

---

## 七、R7 抓包落库与会话化（T7）

### 7.1 问题

`capture/store.rs` 开头写明「第一版先保留最近若干条流量供 UI 展示，写库与历史检索随后续任务接入」，全部流量在 `VecDeque` 里，上限由 `CaptureLimits`（`services/profile.rs:13`）决定：低内存档 2000 条 / 32MB，标准档 5000 条 / 96MB，高档 5000 条 / 128MB（`profile.rs:30-56`）。

后果：停止录制、重启应用、清空列表都会丢；无法跨会话对比；排查完一个问题想回看昨天的流量做不到，只能提前导出 HAR/ZIP。

### 7.2 目标与边界

目标：可选开启「会话归档」，重启后能检索、能对比；默认关闭，不影响现有轻量路径。

不目标：不做云端同步；不做全量抓包入库（默认只存元信息 + 可选存 body）；不改变「流量默认只在内存」的既有承诺。

### 7.3 方案

**7.3.1 三层存储策略**

| 模式 | 存什么 | 用途 |
|---|---|---|
| `memory`（默认） | 现状不变 | 日常调试，零磁盘 |
| `archive-meta` | 会话 + 流量元信息（方法/URL/状态/耗时/头），不存 body | 事后检索「昨天那个请求打到哪」 |
| `archive-full` | 元信息 + body（文本截断 256KB，二进制存文件） | 完整复盘 |

开关放在抓包设置抽屉，按会话生效；每个会话开始录制时确定，不允许中途改（避免半份归档）。

**7.3.2 表设计**

```sql
CREATE TABLE IF NOT EXISTS capture_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    mode        TEXT NOT NULL,            -- archive-meta / archive-full
    bind_addr   TEXT NOT NULL DEFAULT '',
    port        INTEGER NOT NULL DEFAULT 0,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    flow_count  INTEGER NOT NULL DEFAULT 0,
    dropped     INTEGER NOT NULL DEFAULT 0,
    filter_json TEXT,                     -- 本次录制使用的 ignorePatterns / 直通白名单
    app_version TEXT
);
CREATE INDEX IF NOT EXISTS idx_capture_sessions_started ON capture_sessions(started_at DESC);

CREATE TABLE IF NOT EXISTS capture_flows (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id     INTEGER NOT NULL REFERENCES capture_sessions(id) ON DELETE CASCADE,
    flow_id        INTEGER NOT NULL,      -- 内存 store 的 id，便于与 UI 对应
    started_at     INTEGER NOT NULL,
    method         TEXT NOT NULL,
    scheme         TEXT NOT NULL,
    host           TEXT NOT NULL,
    path           TEXT NOT NULL,
    http_version   TEXT NOT NULL DEFAULT '',
    status         INTEGER,
    duration_ms    INTEGER NOT NULL DEFAULT 0,
    request_size   INTEGER NOT NULL DEFAULT 0,
    response_size  INTEGER NOT NULL DEFAULT 0,
    content_type   TEXT NOT NULL DEFAULT '',
    client_addr    TEXT NOT NULL DEFAULT '',
    error          TEXT,
    mapped         INTEGER NOT NULL DEFAULT 0,
    modified       INTEGER NOT NULL DEFAULT 0,
    passthrough    INTEGER NOT NULL DEFAULT 0,
    request_headers_json  TEXT,
    response_headers_json TEXT,
    request_body_sha  TEXT,
    response_body_sha TEXT
);
CREATE INDEX IF NOT EXISTS idx_capture_flows_session ON capture_flows(session_id, started_at);
CREATE INDEX IF NOT EXISTS idx_capture_flows_host ON capture_flows(host, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_capture_flows_status ON capture_flows(status);

CREATE TABLE IF NOT EXISTS capture_bodies (
    sha256      TEXT PRIMARY KEY,
    size        INTEGER NOT NULL,
    truncated   INTEGER NOT NULL DEFAULT 0,
    binary      INTEGER NOT NULL DEFAULT 0,
    content_type TEXT NOT NULL DEFAULT '',
    storage     TEXT NOT NULL,   -- blob / file
    inline_text TEXT,            -- ≤ 8KB 直接内联
    file_path   TEXT             -- 相对 <数据目录>/capture/bodies/ 的路径
);
```

body 去重沿用 notes 附件的 sha256 思路（`skill/astroman-notes`）：重复下载同一个 JS 只存一份，这是抓包归档体积的主要节省点。大 body 走文件而不是 BLOB——SQLite 读 BLOB 会把整块拉进内存，和 `report_requests` 的教训一样。

**7.3.3 写入路径**

绝不能在 handler 里同步写库：抓包 handler 跑在 `capture-proxy` 线程的 tokio runtime 上（`capture/proxy.rs`），同步 SQLite 写入会阻塞 reactor，直接拖慢所有在途连接的转发。

```text
handler 组装 FlowRecord
  → 若该会话模式非 memory → try_send 到 archive_tx（有界 4096）
  → 独立 writer 线程：每 200 条或 2s 批量提交一个事务
  → 满队列丢弃并累加 session.dropped（与内存 store 的 dropped 分开计）
```

writer 线程生命周期跟随 `CaptureManager`，在 `capture_stop` 时先 `Finish` 再 join，保证最后一批落盘。

**7.3.4 读取路径**

| 能力 | 实现 |
|---|---|
| 会话列表 | `SELECT ... ORDER BY started_at DESC LIMIT 50`，返回条数/大小/时间 |
| 流量检索 | `session_id` + 关键字（`LIKE '%kw%'` 命中 host/path）+ 方法/状态码过滤，分页 200 条 |
| 详情 | 按 `flow_id` 读 `capture_flows` + 拼 body（内联或读文件） |
| 对比 | 选两个会话，按 `method + host + path` 归一化后比较状态码与耗时变化 |

第一阶段不做 FTS5：`LIKE` + 索引在单会话几千条的量级足够；等会话数上百再考虑 FTS。

**7.3.5 保留与清理**

| 策略 | 默认 |
|---|---|
| 保留会话数 | 20 |
| 总占用上限 | 1GB |
| 清理时机 | 每次新建会话前；启动时检查一次 |
| 清理顺序 | 最旧优先；`archive-full` 先降级为 `archive-meta`（删 body）再整会话删除 |

清理要同时删 `capture_bodies` 的孤儿（`NOT EXISTS` 引用），否则文件目录会越涨越大。

**7.3.6 脱敏**

落库前按 `docs::masking::SecretSet` 处理请求头与 body 中的敏感值（复用 S5 的机制）。会话级再加一道可配置的「排除域名」——生产环境登录态域名默认排除。

### 7.4 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/capture/persist.rs` | **新建**：归档 writer（channel、批量事务、sha256 body 存储）。不要复用 `capture/archive.rs`——那个文件现在是 ZIP 导出实现（`archive.rs:1`，被 `capture/mod.rs:324/336` 调用），改作 writer 需先把导出迁走，属于不必要的风险 |
| `src-tauri/src/capture/mod.rs` | 会话生命周期：start 建行、stop 收尾；`CaptureConfig` 增加 `archive_mode` |
| `src-tauri/src/capture/handler.rs` | 组装后 `try_send`，不改转发路径 |
| `src-tauri/src/commands/capture.rs` | 新增 `capture_sessions_list` / `capture_session_flows` / `capture_session_flow_detail` / `capture_session_delete` / `capture_session_compare` |
| `src-tauri/src/db/mod.rs` + `schema.sql` | 三张表，v17 |
| `src/api/capture.ts`、`src/types/capture.ts` | 五个命令 + 类型 |
| `src/views/Capture.vue` | 顶部加「历史会话」抽屉：列表、检索、打开、对比、删除 |

### 7.5 测试

| 用例 | 断言 |
|---|---|
| `archive_writes_session_and_flows` | 起代理 + 归档模式，发 1 条请求，停止后库里 1 会话 1 流量 |
| `archive_dedups_identical_bodies` | 同一 body 的 2 条请求只产生 1 行 `capture_bodies` |
| `archive_drops_when_queue_full` | 注入大量记录不消费，`dropped` 增长且转发不受影响 |
| `archive_masks_secrets` | 请求头带 `Authorization: Bearer xxx`，落库为掩码 |
| `retention_deletes_oldest_and_orphan_bodies` | 造 25 个会话触发清理，最旧的被删且 body 文件消失 |
| `memory_mode_writes_nothing` | 默认模式跑完，三张表均为 0 行 |

### 7.6 验收

- 开 `archive-full` 录一轮，重启应用后能检索到并打开详情，body 可读。
- 录 100MB 流量，库增长与 `capture_bodies` 去重后体积相符，内存不随会话数增长。
- 清理后磁盘占用回到上限内，`<数据目录>/capture/bodies/` 无孤儿文件。

### 7.7 风险

| 风险 | 处理 |
|---|---|
| 归档拖慢转发 | 有界队列 + 独立线程 + 丢新不阻塞；默认关闭 |
| 体积失控 | 三级保留策略 + body 去重 + 默认只存元信息 |
| 与「流量不落库」的既有承诺冲突 | 改为「默认不落库，显式开启归档才落库」，文档与 UI 同步说明 |
| 隐私 | 落库前脱敏；会话可一键删除 |

---

## 八、R8 并发透视（1.5.1 / 1.5.2 / 1.5.5）

### 8.1 问题

V1.1.1 方案把完整并发透视放到 V1.2，本期只做了 1.5.3（TTFB/body 拆分）与 1.5.4（TopN 慢请求）。缺的三项：

- 1.5.1 worker 状态机与每秒状态直方图——回答「500 并发到底在干什么」。
- 1.5.2 按需 worker 快照——点一下看到当前 500 个在途请求各自卡在哪。
- 1.5.5 自动诊断提示——只输出「疑似饱和/疑似连接池等待」，根因仍要服务端数据。

### 8.2 方案

**8.2.1 worker 状态机**

`commands/stress.rs` 的 `worker()` 现在是一个循环，没有显式状态。加一个轻量枚举并用 `AtomicU32` 数组按状态计数（**不要**每 worker 一个 Mutex）：

```rust
#[repr(u8)]
enum WorkerState { Idle, RampWait, StageWait, RateWait, AwaitHeader, ReadBody, Think, Done }
```

要点：

- 每个 worker 持有 `Arc<Arc<[AtomicU32; 8]>>` 状态计数表；状态切换 = 两次 `fetch_sub` / `fetch_add`，成本可接受（每秒 500 次 × 2）。
- 状态边界必须与耗时口径对齐：`AwaitHeader` 从请求写出到收到响应头，`ReadBody` 从响应头到 body 读完，两者之和 ≈ 现有 latency。
- `RateWait` 只在本机限速（`target_rps`）时出现——这正是「设了 RPS 但 QPS 上不去」的常见误判来源。

**8.2.2 每秒状态直方图**

复用现有每秒 tick（`stress.rs:1810`），追加一条序列：

```rust
pub struct WorkerStateSample {
    pub elapsed_sec: u64,
    pub idle: u32, pub ramp_wait: u32, pub stage_wait: u32, pub rate_wait: u32,
    pub await_header: u32, pub read_body: u32, pub think: u32, pub done: u32,
}
```

存储：写进 `test_reports.worker_states_json`（与 `body_histogram_json` 同级），不新建表。运行期通过 `StressProgress` 的增量下发，前端堆叠面积图。

**8.2.3 按需 worker 快照**

```rust
#[tauri::command]
pub fn stress_workers(state: State<'_, AppState>, job_id: u64) -> Result<Vec<WorkerSnapshot>, String>;

pub struct WorkerSnapshot {
    pub index: u32,
    pub state: String,
    pub method: String,
    pub url: String,
    pub stage_index: u32,
    pub elapsed_ms: u64,
    pub started_at: i64,
}
```

实现：每个 worker 用一个 `Arc<Mutex<Option<WorkerLive>>>`，正常路径不写，只在 tick 时更新一次（1s 粒度足够）。快照只读不阻塞，上限返回 200 条，按耗时倒序。

**8.2.4 自动诊断**

规则表（全部「疑似」措辞，不推断根因）：

| 条件 | 提示 |
|---|---|
| `rate_wait` 占比 > 50% | 疑似限速未生效或目标 RPS 设得过低 |
| `await_header` 持续 > 90% 且 P95 上升 | 疑似目标侧饱和，等待首字节 |
| `read_body` 持续 > 50% 且 body 直方图右移 | 疑似带宽或响应体过大 |
| `idle` 持续 > 30% 且并发已满 | 疑似客户端 CPU 或事件循环瓶颈（结合能力检测利用率） |
| 错误类型集中于 timeout | 疑似连接池耗尽或目标限流 |

输出进报告的 `findings`，标签统一 P1，文案后缀「（基于客户端观测，服务端监控数据缺失）」。

### 8.3 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/commands/stress.rs` | 状态计数表、`WorkerLive`、tick 采集、`stress_workers` 命令 |
| `src-tauri/src/models.rs` | `WorkerStateSample`、`WorkerSnapshot`、`StressProgress` 增字段 |
| `src-tauri/src/db/schema.sql` + `db/mod.rs` | `test_reports.worker_states_json TEXT`，v17 |
| `src-tauri/src/lib.rs` | 注册 `stress_workers` |
| `src/api/index.ts`、`src/types/models.ts` | 同步 |
| `src/views/StressTest.vue` | 状态堆叠图；「查看在途请求」按钮弹抽屉 |
| `src/utils/reportAnalytics.ts` | `diagnoseWorkerStates()` 纯函数 + 单测 |
| `src/utils/reportExport.ts` | HTML 增加状态分布区块（可选，后置） |

### 8.4 测试

| 用例 | 断言 |
|---|---|
| `worker_states_sum_equals_concurrency` | 任一 tick，8 个状态之和 == 当前并发 |
| `rate_limit_shows_rate_wait` | `target_rps` 设低，`rate_wait` 占比 > 30% |
| `slow_server_shows_await_header` | 目标 sleep 500ms，`await_header` 为主 |
| `diagnosis_flags_rate_wait` | 纯函数输入 → 输出对应提示，措辞含「疑似」 |
| `workers_snapshot_capped` | 并发 500 时只返回 200 条 |

### 8.5 风险

| 风险 | 处理 |
|---|---|
| 状态切换的原子开销影响压测本身 | 每秒 500 次切换可忽略；用 benchmark 示例跑一次对比 |
| 状态归属不准导致误判 | 规则全部用「持续 N 秒」而非单 tick 判定 |
| 快照锁竞争 | 每 worker 独立 Mutex，只在 tick 写；读侧 try_lock 失败就跳过 |

---

## 九、R11 外部监控导入与指标映射（3.6.1 / 3.6.2）

### 9.1 方案（在 R5 契约落地之后）

**9.1.1 CSV 导入（优先）**

格式约定，文件名即 source：

```csv
t,v
0,12.5
1,13.1
```

- `t` 为相对压测开始的秒数；也接受绝对时间戳（>1e9 时按 `min(t)` 归一）。
- 单位由用户在导入时选择（percent / ms / count / bytes），写入 `unit`。
- 指标名由用户填，支持 `cpu.usage` / `gc.pause` / `db.pool.wait` 这类点分命名。
- 一次导入一个指标；多条曲线分次导入。

解析放在 Rust（`services/external_metrics.rs`），前端只负责选文件与填元信息。原因：报告落库在 Rust，放前端会多一次序列化往返。

**9.1.2 Prometheus（后续）**

不直接存 PromQL 结果，而是让用户粘贴查询结果 CSV 或用 `query_range` 拉一次。认证与网络策略不进本版本——内网 Prometheus 常有 mTLS，先做 CSV 通用路径。

**9.1.3 指标映射（3.6.2）**

映射表决定「这条曲线能推导什么结论」：

| metric | 含义 | 可用推断 |
|---|---|---|
| `cpu.usage` | 目标机 CPU% | ≥ 85% 且 QPS 平台期 → 「疑似 CPU 饱和」 |
| `gc.pause` | GC 停顿 ms | 与 P99 峰值时间对齐 → 「疑似 GC 导致长尾」 |
| `db.pool.wait` | 连接池等待 ms | 与 RT 上升对齐 → 「疑似连接池等待」 |
| `thread.pool.active` | 线程池活跃数 | 达到上限且 QPS 不涨 → 「疑似线程池饱和」 |

未映射的 metric 只画曲线，不给结论——这是 3.6.2 的边界。

**9.1.4 展示**

报告详情在吞吐/延迟曲线下方增加「外部指标」区：同一 `elapsed_sec` 时间轴叠加，双 Y 轴，图例显示 `source · metric (unit)`。导出 HTML 同步；Excel 增加一列或独立 sheet。

### 9.2 代码改造点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/services/external_metrics.rs` | 新增：CSV 解析、归一化、限流（≤10 序列 / 每条 ≤3600 点） |
| `src-tauri/src/commands/report.rs` | `report_external_metrics_import(reportId, path, metric, unit)` |
| `src-tauri/src/commands/report.rs` | 读侧合并进详情 |
| `src/utils/reportNarrative.ts` | 映射规则产出 findings |
| `src/views/ReportDetail.vue`、`src/utils/reportExport.ts` | 叠加图与导出 |
| `src/api/index.ts` | 新命令 |

### 9.3 测试

CSV 解析边界（空行、BOM、乱序 t、重复 t、超量截断）；映射规则的时间对齐（取时间窗口内最大值，不做插值）；旧报告无该字段时不渲染区块。

### 9.4 验收

导入一份目标机 CPU CSV，报告里曲线与压测阶段对齐；CPU 持续 90% 且 QPS 平台期时出现「疑似 CPU 饱和」；未映射指标只显示曲线不产结论。

---

## 十、R9 / R10 分布式压测二期与三期

### 10.1 二期：发现、能力上报与权重

**10.1.1 mDNS 发现**

- 主节点在 `_astroman-cluster._tcp.local.` 注册服务，TXT 记录带 `version` / `nodeName` / `role=master`。
- 从节点浏览同一服务类型，列出可选主节点。
- 依赖：`mdns-sd` crate（纯 Rust，无系统依赖）。
- 只做「便利入口」：手动填 IP + 配对码永久保留，mDNS 失败不影响主流程。

**10.1.2 设备列表**

```sql
CREATE TABLE IF NOT EXISTS cluster_devices (
    node_id      TEXT PRIMARY KEY,
    node_name    TEXT NOT NULL,
    last_addr    TEXT NOT NULL,
    last_seen_at INTEGER NOT NULL,
    os           TEXT,
    cores        INTEGER NOT NULL DEFAULT 0,
    capacity_concurrent INTEGER NOT NULL DEFAULT 0,
    capacity_qps REAL NOT NULL DEFAULT 0,
    probed_at    INTEGER
);
```

从节点首次连接时上报本机能力检测结果（复用 `machine_capacity`），主节点缓存并展示。`capacity_concurrent` 用于权重建议。

**10.1.3 权重可视化**

- 默认权重 = `capacity_concurrent` 归一化。
- 用户可手动改，但总并发不足节点数时给告警（一期已有）。
- UI：横向条 + 数字输入，改动即时预览「每节点分到多少并发」。

**10.1.4 协议演进**

`PROTOCOL_VERSION` 1 → 2，新增消息 `CapabilityReport` / `DeviceList`。兼容规则：主节点接受 1 与 2；从节点 1 连 2 时按最小能力集工作（不上报能力、不接受权重下发）。

报告侧的集群元数据结构当前定义在 `services/cluster/protocol.rs:344`（不在 `models.rs`），新增字段要同步改 `src/types/cluster.ts`。

### 10.2 三期：编排、重连与跨网段

**10.2.1 计划级编排**

`cluster_jobs.orchestration_json`：

```json
{
  "version": 1,
  "phases": [
    { "kind": "ramp",  "durationSec": 60, "fromConcurrent": 50, "toConcurrent": 300 },
    { "kind": "hold",  "durationSec": 300, "concurrent": 300 },
    { "kind": "step",  "durationSec": 120, "concurrent": 500 }
  ],
  "assignment": "weighted"
}
```

主节点按阶段下发 `PhaseStart { index, atMs, concurrent, weights }`，不再一次性下发整份配置。

**10.2.2 断线重连**

- 从节点指数退避重连（1s → 2s → 4s → … 上限 30s），重连后带 `sessionToken` 恢复原 job。
- 断线期间该节点的采样缺失，报告标注缺口区间，不做插值。
- 主节点保留失联节点状态 5 分钟，超时才标记 `lost` 并定格。

**10.2.3 跨网段**

- 主节点监听端口需支持绑定指定网卡与端口范围。
- 若 NAT 严格，提供「中继模式」：第三方节点转发，但本版本不实现，只保留协议留白。

**10.2.4 失联后并发重分配（明确不做）**

一期、二期、三期都不做。理由：重分配会让同一份报告的并发曲线出现断层，容量结论不可比。失联即定格是更可解释的选择。

### 10.3 验收

| 项 | 标准 |
|---|---|
| mDNS | 同网段从节点 5s 内出现在主节点列表 |
| 能力上报 | 从节点检测结果出现在设备列表，权重默认值随能力变化 |
| 重连 | 拔网线 10s 后恢复，报告标注缺口，不重跑 |
| 编排 | 三阶段计划在多节点上按时间同步切换 |
| 兼容 | v1 从节点连接 v2 主节点仍能完成一次压测 |

---

## 十一、排期与依赖

| 批次 | 事项 | 前置 | 预估 | 交付物 |
|---|---|---|---|---|
| W1 | R2 + R4 + R5 | 无 | 4–5 人日 | 密钥闭环、lint 门禁、外部指标字段 |
| W2 | R1 + R3 | W1 | 5–6 人日 | 发布检查表、增量落库 |
| — | **1.1.1 发布** | W2 | — | `docs/release-v1.1.1.md` |
| W3 | R6 + R8 | W1 | 6–8 人日 | 上游代理、直通白名单、并发透视 |
| W4 | R7 + R11 | R6、R5 | 8–10 人日 | 抓包归档、外部指标导入 |
| — | ~~1.2 发布~~ | — | — | 已并入 1.1.1，不再单独发布 |
| W5 | R9 + R10 | W3 | 10–15 人日 | mDNS、设备列表、编排、重连 |

> 实际执行：W1–W5 全部完成后作为 1.1.1 一次发布（`DB_VERSION = 19`）。原 1.2 / 1.3 里程碑取消，只保留 1.1.1 发布门禁。

依赖图：

```text
R2 ─┐
R4 ─┼─→ R1 ─→ 1.1.1
R5 ─┴─→ R3

R6 ─→ R7        R5 ─→ R11
R6 ─→ R8 ─→ R9 ─→ R10 ─→ 1.1.1
```

---

## 十二、风险登记

| # | 风险 | 影响 | 应对 |
|---|---|---|---|
| 1 | 自定义 connector 引发抓包 TLS 回归 | 高 | 先 spike；无上游分支与现状等价；回归用例先行 |
| 2 | 归档与「流量不落库」承诺冲突 | 中 | 默认关闭 + 文档/UI 同步说明 |
| 3 | 增量落库引入写锁竞争 | 中 | 独立连接 + 批量事务 + 背压丢弃 |
| 4 | 并发透视影响压测本身精度 | 中 | 原子计数、benchmark 对比 |
| 5 | 二期 mDNS 在 Windows 被防火墙拦截 | 中 | 保留手动路径；失败给可读原因 |
| 6 | lint 首次清理被低估 | 中 | 只启 error 级规则，warn 后置 |
| 7 | 备份指纹被误当安全边界 | 低 | 文档写明仅提示，不导出密钥 |
| 8 | v17–v19 三档迁移连续升级 | 中 | 迁移前备份 + 逐表 `IF NOT EXISTS` + `crash_recovery` / v14 升级回归用例 |

---

## 附录 A：本次涉及数据库迁移合并

```sql
-- v17：压测增量落库 + 报告扩展列（R3 / R5 / R8）
CREATE TABLE IF NOT EXISTS stress_job_samples (...);
CREATE TABLE IF NOT EXISTS stress_job_meta (...);
ALTER TABLE test_reports ADD COLUMN external_metrics_json TEXT;
ALTER TABLE test_reports ADD COLUMN worker_states_json TEXT;
CREATE INDEX IF NOT EXISTS idx_job_samples_job ON stress_job_samples(job_id, elapsed_sec);

-- v18：抓包会话归档（R7）
CREATE TABLE IF NOT EXISTS capture_sessions (...);
CREATE TABLE IF NOT EXISTS capture_flows (...);
CREATE TABLE IF NOT EXISTS capture_bodies (...);
CREATE INDEX IF NOT EXISTS idx_capture_flows_session ON capture_flows(session_id, started_at);
CREATE INDEX IF NOT EXISTS idx_capture_flows_host ON capture_flows(host, started_at DESC);

-- v19：分布式二期三期（R9 / R10）
CREATE TABLE IF NOT EXISTS cluster_devices (...);
ALTER TABLE cluster_jobs ADD COLUMN orchestration_json TEXT;
```

`test_reports.worker_states_json` 对应 R8 并发透视；初稿把它和 `capture_*` 一起放进 v17，实际拆成 v17/v18/v19 三档，避免一次迁移引入五张表。

迁移顺序固定：`schema.sql`（`IF NOT EXISTS`）→ `COLUMN_MIGRATIONS` 补列 → 依赖新列的索引 → `PRAGMA user_version`。新建库与迁移库结构必须逐列一致。

## 附录 B：涉及文件总表

| 域 | 文件 | 关联事项 |
|---|---|---|
| 备份 | `src-tauri/src/backup.rs`、`commands/backup.rs`、`secret.rs` | R2 |
| 压测 | `commands/stress.rs`、`models.rs`、`db/schema.sql` | R3、R8 |
| 前端工程 | `eslint.config.js`、`package.json`、`.github/workflows/ci.yml` | R4 |
| 报告 | `commands/report.rs`、`utils/reportExport.ts`、`ReportDetail.vue` | R5、R11 |
| 抓包 | `capture/proxy.rs`、`capture/upstream.rs`、`capture/handler.rs`、`capture/archive.rs`、`commands/capture.rs` | R6、R7 |
| 集群 | `services/cluster/*`、`db/schema.sql` | R9、R10 |
| 文档 | `docs/release-v1.1.1.md`、`docs/development.md`、`skill/*` | R1、R4 |

## 附录 C：发布检查表模板

```markdown
# AstroMan v<X.Y.Z> 发布检查表

## 1. 版本信息
- 版本号：
- 提交哈希：
- 构建产物 / SHA256：
- 构建环境：

## 2. 自动化门禁
| 项 | 命令 | 结果 | 备注 |
|---|---|---|---|
| 前端单测 | npm run test | | |
| 类型与构建 | npm run build | | |
| 前端 lint | npm run lint | | |
| Rust 格式 | cargo fmt --check --all | | |
| Rust 静态检查 | cargo clippy --lib --all-targets -- -D warnings | | |
| Rust 单测 | cargo test --lib | | |
| 覆盖率 | cargo tarpaulin --config tarpaulin.toml | | |
| E2E 冒烟 | npx playwright test --project=smoke | | |

## 3. 人工验收
| 编号 | 项 | 结论 | 执行人 | 机器 | 日期 | 证据 |
|---|---|---|---|---|---|---|
| 7.3 | 多窗口并行 | | | | | |
| 7.4 | 报告长 URL | | | | | |
| 7.5 | 环境变量 | | | | | |
| 7.6 | 4h 长稳 | | | | | |
| 7.8 | 能力检测 | | | | | |
| 7.9 | 数据库迁移 | | | | | |
| 7.10 | 窗口生命周期 | | | | | |

## 4. 已知问题与豁免
| 问题 | 影响 | 处理 | 版本 |

## 5. 回滚方案
- 安装包回退路径：
- 数据库降级限制：user_version > DB_VERSION 拒绝打开
```

## 附录 D：实现完成情况（2026-11-19）

基线 `4ebd21b`（`master-develop-v1.1.1`），W1–W5 代码与自动化测试已完成。以下「自动化证据」均为本机实际执行结果；人工项未执行前不视为通过。

### D.1 逐项落点

| # | 状态 | 代码落点 | 自动化证据 | 仍待人工 |
|---|---|---|---|---|
| R1 | 检查表就绪 | `docs/release-v1.1.1.md`、`.github/workflows/ci.yml` | lint / 单测 / 类型构建 / E2E 6/6 / 审计 0 漏洞 / Rust fmt+clippy+test 均绿；覆盖率 60.50%（LLVM 近似） | Windows 7.3–7.10 矩阵 |
| R2 | 已完成 | `backup.rs`、`secret.rs`、`commands/backup.rs`、`services/backup_target.rs` | `backup::` 用例；密钥一致 / 不一致 / 未知三条路径 | 同机、异机、老备份冒烟 |
| R3 | 已完成 | `commands/stress.rs`、`db/mod.rs`（`stress_job_samples` / `stress_job_meta`）、`db/schema.sql` | `commands::stress::tests::job_samples_persist_incrementally`、`finished_job_cleans_temp_rows`、`capacity_mode_skips_detail_records`、`db::tests::crash_recovery` | 4h 长稳 + 中途 kill 联调 |
| R4 | 已完成 | `eslint.config.js`、`package.json`、CI | `npm run lint` 0 error / 0 warning | 无 |
| R5 | 已完成 | `commands/report.rs`、`models.rs`；`test_reports.external_metrics_json` | `commands::report::tests` 覆盖空值与序列化 | 无 |
| R6 | 已完成 | `capture/upstream.rs`、`capture/proxy.rs`、`capture/handler.rs` | 上游代理 / CONNECT 直通用例 | 公司代理真机抓包 |
| R7 | 已完成 | `capture/persist.rs`、`capture/archive.rs`、`commands/capture.rs`、`Capture.vue`；v18 三张表 | 归档 / 检索 / 删除 / 对比用例 | 大流量归档体积观察 |
| R8 | 已完成 | `commands/stress.rs`、`test_reports.worker_states_json`、压测详情页 | 并发透视每秒分布用例 | 高并发下 UI 流畅度 |
| R9 | 已完成 | `services/cluster/protocol.rs` / `discovery.rs` / `devices.rs` / `server.rs` / `host.rs`、`commands/cluster.rs`、`Cluster.vue`；v19 | 协议字段、v1 兼容、mDNS 服务名、私有地址过滤、权重分配共 34 条集群用例 | Windows 防火墙下 mDNS 发现 |
| R10 | 已完成 | `services/cluster/server.rs`（编排、断线宽限）、`client.rs`（重连退避）、`merge.rs`、`Cluster.vue` | `orchestration_expands_to_weighted_per_node_stages`、`orchestration_expands_per_second_totals`、`allocation_sums_to_total_and_keeps_minimum_one`、`lost_node_counts_only_samples_until_lost` | 真机断网 / 跨网段 / 混合版本 |
| R11 | 已完成 | `services/external_metrics.rs`、`commands/report.rs`、`reportExport.ts`、报告详情 / 导出 | 导入解析与字段映射用例 | 真实监控系统导出文件 |

### D.2 关键实现说明

- **R2 备份密钥**：备份文件不导出主密钥；恢复时按指纹给出「一致 / 不一致 / 未知」，不一致默认拦截并要求确认受影响条数，设置页文案同步。指纹只做提示，不构成安全边界。
- **R3 增量落库**：运行期每秒写 `stress_job_samples`，`stress_job_meta` 记录状态；正常结束清理临时行，崩溃后按样本加权回算分位与均值，报告标记 `partial` + fail 结论 + 禁止选作基线。
- **R6/R7 抓包**：新增上游代理链与 CONNECT 直通白名单；归档默认关闭，开启后按会话写入 `capture_sessions` / `capture_flows` / `capture_bodies`，支持关键字 + 方法 + 状态检索、删除（录制中禁止）与两会话对比。
- **R8 并发透视**：每秒 worker 状态分布写入 `worker_states_json`，仅详情读取，列表查询不背负大字段。
- **R9 分布式二期**：协议升到 v2（保留 v1 解析），Hello 携带设备能力并与 `cluster_devices` 缓存合并；mDNS 广播/浏览保留手动地址降级；配对时默认权重按能力 `capacity/floor` 归一化并夹紧上下限，用户显式改动在 prepare 阶段覆盖。
- **R10 分布式三期**：编排支持 `ramp` / `hold` / `step`，最多 32 段，按秒展开后按权重分配到节点并合成每节点阶段；主节点断线给 5 分钟宽限，从节点 1/2/4/8/16/30 秒指数退避重连，重连后补发 `Done`，报告标记不完整。

### D.3 自动化验证结果

| 项 | 命令 | 结果 |
|---|---|---|
| 前端 lint | `npm run lint` | 通过 |
| 类型检查 | `npx vue-tsc --noEmit` | 通过 |
| 前端单测 | `npm test` | 300 通过；13 条为干净基线即存在的环境性失败（`crypto is not defined`、`aiEval` 超时），非本次回归 |
| 前端构建 | `npm run build` | 通过 |
| 依赖审计 | `npm audit --omit=dev --audit-level=high --registry=https://registry.npmjs.org` | 通过（0 vulnerabilities） |
| E2E 冒烟 | `npm run e2e:smoke` | 通过（6/6，14.1s；临时 Node 20.19.4 + 系统 Chrome） |
| Rust 格式 | `cargo fmt --check --all` | 通过 |
| Rust 静态检查 | `cargo clippy --lib --all-targets -- -D warnings` | 通过（0 warning） |
| Rust 单测 | `cargo test --lib` | 451 通过 / 0 失败（含 34 条集群用例，其中 4 条为集群验收） |
| 覆盖率 | `cargo llvm-cov --lib --summary-only` | 行 60.50% / 函数 57.02% / 区域 62.30%（本机 LLVM 近似，阈值 25%）；CI tarpaulin 待跑 |

### D.4 发布前剩余动作

1. 在 Windows 10 x64 构建机复核 `cargo fmt` / `clippy` / `test` 与 `npm run version:check`，产出安装包并记录 SHA256；E2E 与依赖审计已在本机通过，Windows 侧只需确认可复现。
2. 执行 7.3–7.10 人工矩阵及 D.1 中「仍待人工」各列，证据路径回填 `docs/release-v1.1.1.md` 第 3 节；7.5、7.9、R9/R10 已有本机自动化等价检查，见该文档第 3 节开头的表格。
3. CI 跑 Linux tarpaulin 覆盖率（本机 llvm-cov 行覆盖 60.50%，仅作参考）。
4. 重新记录最终提交哈希（当前改动未提交）。
5. 真机断网 / 跨网段验收通过前，R10 不宣称「实机已验证」。
