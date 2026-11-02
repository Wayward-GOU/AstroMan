---
name: astroman-overview
description: AstroMan 项目全景。接手本项目、需要了解目录结构、启动与构建命令、运行时行为（托盘/关闭隐藏）、数据存放位置，或确认哪些旧标识（identifier、数据目录、数据库名、crate 名）必须保留时使用。触发词：AstroMan 项目、项目结构、怎么启动、数据存哪、改名注意事项。
---

# AstroMan 项目全景

AstroMan 是一个**完全本地运行**的 API 调试、压测与开发工具箱：无账号、无云端，项目/接口/环境变量/请求历史/压测报告全部写入本机 SQLite。

- 前端：Vue 3 + TypeScript + Vite + Pinia + vue-router（hash 模式）+ Ant Design Vue 4
- 后端：Tauri 2 + Rust，SQLite（rusqlite + r2d2 连接池）
- 网络请求由 Rust 侧 reqwest 发出，前端不直接跨域请求

## 目录地图

```text
AstroMan/
├─ src/
│  ├─ api/            # invoke 封装（index.ts 为主，project.ts 等按域拆分）
│  ├─ assets/         # buddy.png：航仔宇航员挂件贴图（透明底，界面唯一使用的位图）
│  ├─ components/     # PageHeader / CodeArea / JsonTree / KeyValueEditor / LineChart / AppLogo
│  │                  # DashboardMetrics（历史记录页数据大屏）、tools/（20 个工具组件 + ToolWorkbench）
│  │                  # 航仔：AstroBuddy.vue（挂件，可拖拽）/ AstroBuddyIcon.vue（1em 线框图标）
│  ├─ stores/         # pinia：project（项目+集合树+环境）、tabs（标签页持久化）、theme（21 套主题）、toolPrefs（工具收藏/最近）
│  ├─ styles/main.css # 设计系统：--fm-* 变量
│  ├─ tools/          # 离线工具箱：registry.ts（92 个工具 / 9 分类）+ 各域纯函数 + __tests__/toolbox.spec.ts
│  ├─ types/models.ts # 与 Rust models.rs 对应的 TS 类型
│  ├─ utils/          # scriptRunner（脚本引擎）、cryptoScript、rsa、curl、xlsx、diff、mybatis、markdown
│  │                  # 报告：reportExport（HTML/Excel/JSON）、reportAnalytics（QPS/拐点/体检/自洽）、reportNarrative
│  │                  # AI：aiTools（74 个工具：73 业务 + tool_search）、aiProtocol（围栏协议）、contextBudget（token 预算）、modelMeta（窗口）、untrusted（注入隔离）、aiPricing（费用）、promptText（压缩）
│  └─ views/          # Dashboard / Projects / Project / RequestEditor / StressTest / Runner / Automation / Reports / History
│                     # Tools / SystemMonitor / Settings / Environment / ReportDetail / AiAssistant / AiUsage / AiToolLogs
├─ src-tauri/
│  ├─ src/commands/   # 每个文件一个业务域，全部 #[tauri::command]；测试域为 test_suite/test_case/test_pack/test_preflight/test_run/test_report
│  ├─ src/db/         # schema.sql + mod.rs（init → migrate → seed）+ seed.rs
│  ├─ src/test_runner.rs   # 自动化测试执行内核（不依赖 Tauri，CLI 复用）
│  ├─ src/test_assert.rs   # 6 类声明式断言
│  ├─ src/test_dataset.rs  # CSV / JSON 数据集解析
│  ├─ src/python_exec.rs   # Python 子进程：探测、context/result 协议、超时与附件边界
│  ├─ src/report_builder.rs # 运行报告 HTML / JSON / JUnit / summary
│  ├─ src/net_probe.rs      # 建连计时探针（复用连接时 connectMs = 0）
│  ├─ src/secret.rs   # 主密钥、AES-GCM 字段加密、钥匙串/口令/文件三种来源
│  ├─ src/state.rs    # AppState：连接池、数据目录、网络设置、压测开关、AI 取消表
│  ├─ src/models.rs   # 前后端共享数据模型（serde camelCase）
│  ├─ src/lib.rs      # 托盘、窗口事件、invoke_handler 注册
│  ├─ tarpaulin.toml  # 覆盖率门槛（fail-under）与排除项，CI 直接读
│  ├─ capabilities/default.json # 已授权 core:default / core:webview:allow-set-webview-zoom / dialog:default
│  └─ icons/          # 打包与托盘图标（唯一保留的一套）
├─ scripts/           # install-macos.sh、prepare-cli.mjs、sync-version.mjs、gen-sample-report.mts（重新生成示例报告）
├─ skill/             # 项目内开发规范（本目录）
└─ .github/workflows/ # ci.yml（四道门禁）、release.yml
```

## 常用命令

```bash
npm run dev        # 只跑前端（浏览器调试，无 Tauri IPC）
npm run tauri dev  # 完整桌面应用
npm run build      # vue-tsc --noEmit + vite build
npm run build:win  # tauri build --bundles nsis
npm run build:mac  # tauri build --target universal-apple-darwin --bundles dmg
npm run test       # vitest run：10 spec / 134 条
npm run version:check       # 三处版本号一致性校验，不一致退出码 1
npm run version:set 1.0.8   # 一次写入 package.json / tauri.conf.json / Cargo.toml
cargo check        # 在 src-tauri 目录执行
cargo test --lib   # 101 条 Rust 用例
cargo tarpaulin --config tarpaulin.toml   # 覆盖率门槛；Windows 本地加 --engine llvm
```

Windows 下 `cargo check` 在受限沙箱里会命中 `D:\Software\Rust\cargo\.global-cache-journal`，需在**关闭沙箱**的模式下执行。测试体系的完整说明见 skill `astroman-testing`。

## 运行时行为

- 主窗口 label 固定为 `main`（`tauri.conf.json` 的 `app.windows[0].label`）。
- 单实例：`tauri-plugin-single-instance` 必须是第一个注册的插件，重复启动时只唤出已有窗口，不再开第二个进程抢同一个数据库。
- 窗口状态：`tauri-plugin-window-state` 记住尺寸、位置与最大化状态，下次启动原样恢复；改动窗口配置后若想重来，删掉配置目录下的窗口状态文件即可。
- 系统托盘在 `lib.rs::setup_tray` 创建：左键点击 → 显示主窗口；右键菜单「显示主窗口 / 退出 AstroMan」。
- **关闭窗口 = 隐藏到托盘**（`on_window_event` 中 `prevent_close` + `hide`），真正退出只走托盘菜单或 `app.exit(0)`。
- macOS 点击 Dock 图标时通过 `RunEvent::Reopen` 重新显示窗口。
- 托盘图标来自 `include_bytes!("../icons/tray.png")`，改图标必须重新生成该文件，否则编译期就嵌入旧图。

## 数据位置

| 内容 | 路径 |
|---|---|
| 默认数据目录 | `%APPDATA%\AstroMan`（macOS 为 `~/Library/Application Support/AstroMan`） |
| 数据库文件 | `数据目录\astroman.db` |
| 自定义数据目录配置 | 系统配置目录下 `storage.json` |
| 网络设置（代理、mTLS 证书） | 系统配置目录下 `settings.json` |

启动时若发现旧版 `FinkMeter\finkmeter.db` 且新目录还没有数据库，会**自动复制**到 `AstroMan\astroman.db`（含 `-wal`/`-shm`），原目录保留；自定义目录里的旧数据库会被就地重命名为 `astroman.db`。自定义目录打不开时只回退默认目录、不删除配置。

## 必须保留的旧标识（不要"顺手"改名）

| 标识 | 值 | 原因 |
|---|---|---|
| tauri `identifier` | `com.finkmeter.app` | 旧值，仅作为配置目录迁移来源；现行值 `com.astroman.app` |
| 旧数据目录名 | `FinkMeter` | 只作为迁移来源读取，不再写入 |
| 旧数据库文件名 | `finkmeter.db` | 只作为迁移来源，新版统一 `astroman.db` |
| 旧项目文件格式 | `finkmeter.project` | 导入时仍接受，新导出写 `astroman.project` |
| 旧主题 key | `finkmeter.theme` | 读取一次后迁移到 `astroman.theme` |

产品名、窗口标题、安装包名、界面文案、User-Agent、Release 仓库地址均已是 **AstroMan**。

## 约定速览

- 前端调用：`invoke<返回类型>('snake_case_command', { camelCaseArgs })`
- Rust 模型：`#[serde(rename_all = "camelCase")]`，前端类型在 `src/types/models.ts`
- 样式：只用 `--fm-*` 变量，不写死颜色；主题在 `stores/theme.ts`（**21 套**，14 浅 / 7 深，每套覆盖 35 个颜色 / 阴影变量）里整体覆盖
- 航仔入口：`components/AstroBuddy.vue`，用设计稿贴图 `src/assets/buddy.png`（透明底，显示 72×121），可拖拽；小尺寸图标用 `AstroBuddyIcon.vue` 的 1em 线框版，不要把位图缩到 14px
- 命令全部返回 `Result<T, String>`，错误文案面向用户、中文
- 请求 Body（JSON）允许写 `//`、`/* */` 注释，发送前由 `util::strip_json_comments` 剥离；前端 `CodeArea` 负责着色与 `Ctrl / ⌘ + /` 注释，不负责删除
- 敏感值：`src-tauri/src/secret.rs` 统一 AES-256-GCM 加密（`enc:v1:`），主密钥按数据目录隔离，测试构建永不写钥匙串
- 自动化测试：桌面端与 `astroman-cli` 共用 `test_runner::run_suite`，产物写 `<数据目录>/test-reports/<runId>/` 且库里只存相对路径；改判定逻辑必须两端一起验
- 工具箱：92 个工具的实现全部在 `src/tools/*.ts` 纯函数，`registry.ts` 只做声明；页面与 `ToolWorkbench` 只管交互
- 压测报告 QPS 指标组唯一入口：`utils/reportAnalytics.ts::qpsOverview()`，详情页与 HTML / Excel 导出都调它

## 配套 skill

| skill | 用途 |
|---|---|
| `astroman-add-feature` | 端到端新增功能（命令 → api → 页面 → 注册） |
| `astroman-backend` | Rust 命令、SQLite 迁移、压测引擎、主密钥与钥匙串规范 |
| `astroman-frontend` | 页面骨架、主题变量、脚本引擎、导出 |
| `astroman-appearance` | 21 套主题的变量约定与新增流程、对比度校验、航仔贴图与拖拽、线框图标 |
| `astroman-dashboard` | 工作台首页区块、历史记录页数据大屏、引导 / 技巧 / FAQ 文案口径、Runner 与测试场景介绍 |
| `astroman-automation` | 自动化测试子系统：套件 / 用例 / 断言 / 数据集 / Python 钩子 / 运行产物 / CLI 退出码 / 压测门禁 |
| `astroman-toolbox` | 离线工具箱：92 个工具的注册契约、两种接入方式、新增流程、搜索别名与工具清单同步 |
| `astroman-ai` | AI 助手「航仔」：双模式、三协议、流式渲染、上下文预算与摘要、停止/重试/工具超时、注入隔离、74+1 工具与目录分层、写操作确认卡、消息操作、复盘时间线、会话管理 |
| `astroman-testing` | vitest / cargo test / tarpaulin 覆盖率 / CI 四道门禁 / 版本号同步与沙箱坑 |
| `astroman-build-release` | 构建、打包与发布 |
