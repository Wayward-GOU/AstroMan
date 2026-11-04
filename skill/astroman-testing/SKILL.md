---
name: astroman-testing
description: AstroMan 的测试体系与质量门禁：vitest 前端纯函数用例、cargo test、cargo-tarpaulin 覆盖率门槛、CI 四道门禁（fmt / clippy / 版本一致性 / 覆盖率）与版本号同步脚本。加测试、改门禁、排查 CI 变红或本地测试诡异失败时使用。触发词：测试、单测、vitest、cargo test、覆盖率、tarpaulin、clippy、fmt、CI、门禁、版本号、version:check。
---

# 测试与质量门禁

四层验收，命令与规模：

| 层 | 命令 | 规模（2026-11-04 实测） | 位置 |
|---|---|---|---|
| 前端纯函数 | `npm run test` | 14 spec / 160 条 | `src/**/*.spec.ts` |
| Rust lib | `cd src-tauri && cargo test --lib` | 140 条 / 25 文件 | 模块内 `#[cfg(test)]` |
| Rust 覆盖率 | `cargo tarpaulin --config tarpaulin.toml` | 门槛 25%（防倒退） | `src-tauri/tarpaulin.toml` |
| 类型 / 构建 | `npm run build` | `vue-tsc --noEmit` + `vite build` | — |

格式与静态检查（CI 同源）：

```bash
cd src-tauri
cargo fmt --check --all
cargo clippy --lib --all-targets -- -D warnings   # 注意参数顺序
```

`cargo clippy --lib --all-targets -- -D warnings` 里 **`--` 只能出现在 `--all-targets` 之后**；写成 `cargo clippy --lib -- -D warnings --all-targets` 会解析失败。

## 一、前端 vitest

- 配置在 `vitest.config.ts`：`environment: 'node'`、`include: ['src/**/*.spec.ts']`、alias `@ → src`
- **不测组件渲染**：没有 jsdom / `@vue/test-utils`，也不该加——这类用例收益低、脆弱。测的是纯函数与协议层：脚本引擎、AI 协议解析、上下文预算、报告导出 / 分析、工具箱、curl 解析
- 位置两种都行：`src/utils/__tests__/xxx.spec.ts`（成组）或与源码平级 `src/utils/xxx.spec.ts`
- 用例名用中文描述**行为契约**，别测实现细节。例：`scriptHost.spec.ts` 锁的是「没有 Worker 时必须降级而不是报错」「超时值被夹到合法区间」，不是「某个内部函数被调用了几次」
- 断言优先 `toContain` / `toBe` 这种稳定输出，时间、随机数、UUID 一类先固定种子或只校验形状

沙箱坑：临时目录不可写时 vitest 会报 `EPERM ... \Temp\...`，文件数与用例数在 8~10 之间抖动，看起来像「丢了测试文件」。这不是用例失败，串行执行即可：

```bash
npx vitest run --pool=forks --poolOptions.forks.singleFork
```

### 重新生成示例报告

`docs/sample-stress-report.html` 不是手写的，由 `scripts/gen-sample-report.mts` 调 `buildReportHtml()` 生成（5 阶段阶梯 + 503 突发 + 拐点）。改导出结构（KPI 卡片、表头、口径文案）后必须重新生成，否则文档里的样例会和真实导出不一致。

脚本是 `.mts` 且用了 `@/` 别名，直接用 vitest 跑最省事——建一个临时 spec，跑完删掉：

```ts
// src/utils/__tmp_gen_sample.spec.ts
import { test } from 'vitest'
test('gen', async () => {
  await import('../../scripts/gen-sample-report.mts')
})
```

```bash
npx vitest run src/utils/__tmp_gen_sample.spec.ts --pool=forks --poolOptions.forks.singleFork
```

## 二、Rust 单测

- 写法：同文件底部 `#[cfg(test)] mod tests { use super::*; ... }`
- 需要 `AppState` 时用 `AppState::headless`（临时数据目录 + 临时库），不要碰真实数据目录
- **测试绝不能写系统钥匙串**：`secret::keyring_allowed()` 在 `cfg!(test)` 下恒为 false。历史事故就是这样来的——headless 测试在临时库上 `bootstrap`，生成了新主密钥并写进当时还在全局共享的钥匙串条目，把真实安装的密钥覆盖掉
- 改密钥相关代码后至少跑：

```bash
cargo test --lib secret::
```

- 自动化测试域的用例分布：`test_runner.rs` 16 条、`python_exec.rs` 8 条、`report_builder_tests.rs` 5 条、`test_assert.rs` 4 条、`test_dataset.rs` 4 条。改断言、数据行、Python 协议或报告产物后至少跑：

```bash
cargo test --lib test_runner:: test_assert:: test_dataset:: python_exec::
```

- 记事本域的用例在 `src-tauri/src/notes/tests.rs`（覆盖重复顺延、稍后提醒、暂停 / 免打扰、附件去重），抓包拦截在 `src-tauri/src/capture/tests.rs`；改提醒或拦截规则后至少跑：

```bash
cargo test --lib notes::
cargo test --lib capture::
```

- 前端记事本纯函数各有一份 spec：`notesGrouping.spec.ts` / `notesImage.spec.ts` / `notesQuickParse.spec.ts` / `notesTags.spec.ts`；改解析语法或标签配色先补这里

- 受限环境（沙箱 / 只读盘）会集中报 `attempt to write a readonly database`，一批用例同时失败；这是环境不是代码。单独重跑、把临时目录换到可写盘，或在关闭沙箱的模式下执行

## 三、覆盖率（cargo-tarpaulin）

`src-tauri/tarpaulin.toml` 是唯一真源，CI 直接读它：

```toml
[coverage]
run-types = ["Lib"]
fail-under = 25
exclude-files = ["src/main.rs", "src/bin/*"]

[report]
out = ["Xml", "Html", "Stdout"]
output-dir = "target/coverage"
```

- **只统计 lib**：桌面装配（窗口 / 托盘 / 菜单）依赖真实 WebView，无头环境初始化不了，留在分母里只会长期拉低数字
- 阈值策略：v1.0.8 先落 25% 做**防倒退**（拦住「删测试 / 砍覆盖」），CI 首跑后按 `tarpaulin-report.html` 的真实值校准，再按版本分批抬向 60%。改门槛只改这一处
- 后端差异：CI 用默认 **ptrace**（只支持 Linux x86_64，所以单独一个 `coverage` job）；**Windows 本地只能 `--engine llvm`**，需先 `rustup component add llvm-tools-preview`
- LLVM 引擎的一个坑：测试**非零退出**时会丢弃覆盖率数据，表现为「没有任何覆盖率输出」。先确认 `cargo test --lib` 全绿，再怀疑配置

```bash
# 本地（Windows）
cd src-tauri
cargo tarpaulin --config tarpaulin.toml --engine llvm
```

## 四、CI 四道门禁（`.github/workflows/ci.yml`）

| job | 跑什么 | 关键点 |
|---|---|---|
| `frontend`（ubuntu） | `version:check` → `npm test` → `npm run build` | 版本不一致第一步就失败；类型错误由 build 拦 |
| `rust`（windows） | `fmt --check` → `clippy -D warnings` → `cargo test --lib` → `cargo build --bin astroman-cli` | 先 `npm ci` + `npm run build` + `prepare-cli.mjs`，`externalBin` 要求 CLI 产物先存在 |
| `coverage`（ubuntu） | `cargo tarpaulin --config tarpaulin.toml` | 需装 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`librsvg2-dev` 等；CLI 用 `prepare-cli.mjs --placeholder` 占位即可；报告 `if: always()` 上传为 `coverage-report` |

约定：

- **不要**把 clippy 和 test 塞进同一个 job：clippy 失败时看不到测试结果，排查成本翻倍
- job 之间不共享产物，每个 job 都要自己 `npm ci` + `npm run build`
- `cargo fmt` 与 `clippy` 是**阻断**门禁，不是提示

## 五、版本号一致性

版本号在三个文件里各存一份，漏一处就会出现「关于页版本和安装包不一致」：

| 文件 | 由谁维护 |
|---|---|
| `package.json` | `version:set` 写入；`package-lock.json` 需 `npm install --package-lock-only` 跟上 |
| `src-tauri/tauri.conf.json` | `version:set` 写入 |
| `src-tauri/Cargo.toml` | `version:set` 写入；`Cargo.lock` 由 `cargo check` 自动同步 |

```bash
npm run version:check        # 只校验，不一致退出码 1（CI 用）
npm run version:set 1.0.9    # 三处统一写入
```

`scripts/sync-version.mjs` 用各自文件的匹配规则（`Cargo.toml` 只取顶层第一个 `version`），不会误伤依赖里的版本号。

## 六、提交前清单

- [ ] `npm run version:check` 通过
- [ ] `npm run test` 全绿（沙箱里用单进程模式）
- [ ] `npm run build` 通过
- [ ] `cargo fmt --check --all` 无输出
- [ ] `cargo clippy --lib --all-targets -- -D warnings` 无告警
- [ ] `cargo test --lib` 全绿
- [ ] 新增敏感代码路径时 `cargo test --lib secret::` 全绿，且没有新增写钥匙串的分支
- [ ] 改了覆盖率相关配置时，本地至少跑一次 `cargo tarpaulin --config tarpaulin.toml --engine llvm`

## 七、常见失败对照

| 现象 | 原因 / 处理 |
|---|---|
| `attempt to write a readonly database`（一批同时失败） | 受限沙箱 / 只读盘无法建临时库；换可写临时目录或关闭沙箱，不是代码问题 |
| vitest 报 `EPERM ... \Temp\...`、文件数抖动 | 临时目录不可写导致 worker 丢文件；`--pool=forks --poolOptions.forks.singleFork` |
| clippy 报参数错误 / 只检查了一部分 | `--` 必须在 `--all-targets` 之后 |
| `externalBin ... not found` | 先跑 `node scripts/prepare-cli.mjs --target <triple>`（CI 里也可用 `--placeholder`） |
| tarpaulin 没有任何输出 | 测试非零退出时 LLVM 会丢数据；先让 `cargo test --lib` 全绿 |
| 覆盖率突然掉很多 | 检查是否新增了不可测代码（main/bin 已排除）、或某组测试被跳过 |
| `version:check` 失败 | 三处版本不一致；`npm run version:set <版本>` 后补 lock 文件 |
| Windows 上 tarpaulin 起不来 | 默认 ptrace 仅 Linux；加 `--engine llvm` 并装 `llvm-tools-preview` |
