---
name: fbinhttp-overview
description: FbinHttp 项目全景。接手本项目、需要了解目录结构、启动与构建命令、运行时行为（托盘/关闭隐藏）、数据存放位置，或确认哪些旧标识（identifier、数据目录、数据库名、crate 名）必须保留时使用。触发词：FbinHttp 项目、项目结构、怎么启动、数据存哪、改名注意事项。
---

# FbinHttp 项目全景

FbinHttp 是一个**完全本地运行**的 API 调试、压测与开发工具箱：无账号、无云端，项目/接口/环境变量/请求历史/压测报告全部写入本机 SQLite。

- 前端：Vue 3 + TypeScript + Vite + Pinia + vue-router（hash 模式）+ Ant Design Vue 4
- 后端：Tauri 2 + Rust，SQLite（rusqlite + r2d2 连接池）
- 网络请求由 Rust 侧 reqwest 发出，前端不直接跨域请求

## 目录地图

```text
FbinHttp/
├─ src/
│  ├─ api/            # invoke 封装（index.ts 为主，project.ts 等按域拆分）
│  ├─ components/     # PageHeader / CodeArea / JsonTree / KeyValueEditor / LineChart / AppLogo
│  ├─ stores/         # pinia：project（项目+集合树+环境）、tabs（标签页持久化）、theme
│  ├─ styles/main.css # 设计系统：--fm-* 变量
│  ├─ tools/          # 离线工具注册表（registry.ts）与各工具组件
│  ├─ types/models.ts # 与 Rust models.rs 对应的 TS 类型
│  ├─ utils/          # scriptRunner（脚本引擎）、cryptoScript、rsa、curl、reportExport、xlsx、diff、mybatis、aiTools（AI 工具注册表）、aiPricing（费用估算）、markdown
│  └─ views/          # Dashboard / Project / RequestEditor / StressTest / Runner / Reports / History / Tools / SystemMonitor / Settings / Environment / ReportDetail / AiAssistant / AiUsage / AiToolLogs
├─ src-tauri/
│  ├─ src/commands/   # 每个文件一个业务域，全部 #[tauri::command]
│  ├─ src/db/         # schema.sql + mod.rs（init → migrate → seed）+ seed.rs
│  ├─ src/state.rs    # AppState：连接池、数据目录、网络设置、压测开关
│  ├─ src/models.rs   # 前后端共享数据模型（serde camelCase）
│  ├─ src/lib.rs      # 托盘、窗口事件、invoke_handler 注册
│  ├─ capabilities/default.json # 已授权 core:default / core:webview:allow-set-webview-zoom / dialog:default
│  └─ icons/          # 打包与托盘图标（唯一保留的一套）
├─ scripts/install-macos.sh
└─ .github/workflows/release.yml
```

## 常用命令

```bash
npm run dev        # 只跑前端（浏览器调试，无 Tauri IPC）
npm run tauri dev  # 完整桌面应用
npm run build      # vue-tsc --noEmit + vite build
npm run build:win  # tauri build --bundles nsis
npm run build:mac  # tauri build --target universal-apple-darwin --bundles dmg
cargo check        # 在 src-tauri 目录执行
```

Windows 下 `cargo check` 在受限沙箱里会命中 `D:\Software\Rust\cargo\.global-cache-journal`，需在**关闭沙箱**的模式下执行。

## 运行时行为

- 主窗口 label 固定为 `main`（`tauri.conf.json` 的 `app.windows[0].label`）。
- 单实例：`tauri-plugin-single-instance` 必须是第一个注册的插件，重复启动时只唤出已有窗口，不再开第二个进程抢同一个数据库。
- 窗口状态：`tauri-plugin-window-state` 记住尺寸、位置与最大化状态，下次启动原样恢复；改动窗口配置后若想重来，删掉配置目录下的窗口状态文件即可。
- 系统托盘在 `lib.rs::setup_tray` 创建：左键点击 → 显示主窗口；右键菜单「显示主窗口 / 退出 FbinHttp」。
- **关闭窗口 = 隐藏到托盘**（`on_window_event` 中 `prevent_close` + `hide`），真正退出只走托盘菜单或 `app.exit(0)`。
- macOS 点击 Dock 图标时通过 `RunEvent::Reopen` 重新显示窗口。
- 托盘图标来自 `include_bytes!("../icons/tray.png")`，改图标必须重新生成该文件，否则编译期就嵌入旧图。

## 数据位置

| 内容 | 路径 |
|---|---|
| 默认数据目录 | `%APPDATA%\FbinHttp`（macOS 为 `~/Library/Application Support/FbinHttp`） |
| 数据库文件 | `数据目录\fbinhttp.db` |
| 自定义数据目录配置 | 系统配置目录下 `storage.json` |
| 网络设置（代理、mTLS 证书） | 系统配置目录下 `settings.json` |

启动时若发现旧版 `FinkMeter\finkmeter.db` 且新目录还没有数据库，会**自动复制**到 `FbinHttp\fbinhttp.db`（含 `-wal`/`-shm`），原目录保留；自定义目录里的旧数据库会被就地重命名为 `fbinhttp.db`。自定义目录打不开时只回退默认目录、不删除配置。

## 必须保留的旧标识（不要"顺手"改名）

| 标识 | 值 | 原因 |
|---|---|---|
| tauri `identifier` | `com.finkmeter.app` | 旧值，仅作为配置目录迁移来源；现行值 `com.fbinhttp.app` |
| 旧数据目录名 | `FinkMeter` | 只作为迁移来源读取，不再写入 |
| 旧数据库文件名 | `finkmeter.db` | 只作为迁移来源，新版统一 `fbinhttp.db` |
| 旧项目文件格式 | `finkmeter.project` | 导入时仍接受，新导出写 `fbinhttp.project` |
| 旧主题 key | `finkmeter.theme` | 读取一次后迁移到 `fbinhttp.theme` |

产品名、窗口标题、安装包名、界面文案、User-Agent、Release 仓库地址均已是 **FbinHttp**。

## 约定速览

- 前端调用：`invoke<返回类型>('snake_case_command', { camelCaseArgs })`
- Rust 模型：`#[serde(rename_all = "camelCase")]`，前端类型在 `src/types/models.ts`
- 样式：只用 `--fm-*` 变量，不写死颜色；主题在 `stores/theme.ts` 里按同一批变量覆盖
- 命令全部返回 `Result<T, String>`，错误文案面向用户、中文
- 请求 Body（JSON）允许写 `//`、`/* */` 注释，发送前由 `util::strip_json_comments` 剥离；前端 `CodeArea` 负责着色与 `Ctrl / ⌘ + /` 注释，不负责删除

## 配套 skill

| skill | 用途 |
|---|---|
| `fbinhttp-add-feature` | 端到端新增功能（命令 → api → 页面 → 注册） |
| `fbinhttp-backend` | Rust 命令、SQLite 迁移、压测引擎规范 |
| `fbinhttp-frontend` | 页面骨架、主题变量、脚本引擎、导出 |
| `fbinhttp-dashboard` | 首页数据大屏口径、CSS 图表约定、Runner 介绍文案 |
| `fbinhttp-ai` | AI 助手「小HI」：协议接入、会话、工具注册表、工具调用记录 |
| `fbinhttp-build-release` | 构建、打包与发布 |
