# 开发与打包

> 面向贡献者：本地环境、开发启动、安装包构建、数据位置与代码约定。

## 环境要求

Windows 与 macOS 的依赖不同，按目标系统准备。

### Windows

| 依赖 | 版本 | 说明 |
|---|---|---|
| Node.js | 22.x | 已安装 |
| npm | 10.x | 已安装 |
| Rust | stable（MSVC） | 已安装到 `D:\Software\Rust` |
| MSVC Build Tools | 2019 / 2022 | 已安装（需要「使用 C++ 的桌面开发」） |

本机 Rust 相关环境变量：

| 变量 | 值 |
|---|---|
| `RUSTUP_HOME` | `D:\Software\Rust\rustup` |
| `CARGO_HOME` | `D:\Software\Rust\cargo` |
| `PATH`（追加） | `D:\Software\Rust\cargo\bin` |

> 修改过环境变量后，需要**重开终端或重启 IDEA** 才会生效。

### macOS

| 依赖 | 版本 | 说明 |
|---|---|---|
| Node.js | 20.x / 22.x | `brew install node` 或官网安装包 |
| npm | 10.x | 随 Node.js 安装 |
| Rust | stable | 使用 rustup 安装 |
| Xcode Command Line Tools | 最新 | `xcode-select --install`，编译 rusqlite / Tauri 必需 |

```bash
# 安装 Rust，并补齐通用包所需的双架构标准库
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## 开发启动

在项目根目录执行（Windows）：

```powershell
npm.cmd run tauri dev
```

macOS：

```bash
npm run tauri dev
```

启动过程：

1. Vite 在 `http://localhost:1420` 启动前端
2. Tauri 编译 Rust 并打开桌面窗口
3. 首次编译依赖需要几分钟，之后为增量编译

停止：关闭窗口，或在终端按 `Ctrl + C`。

只启动前端（浏览器调试 UI，IPC 调用不可用）：

```powershell
npm.cmd run dev
```

---

## 打包安装包

> 安装包必须在目标系统上构建：Windows 包在 Windows 打，macOS 包在 macOS 打（Tauri 不支持交叉打包）。

### Windows

全部格式（NSIS + MSI）：

```powershell
npm.cmd run tauri build
```

只打 exe 安装包（NSIS，推荐）：

```powershell
npm.cmd run build:win
```

离线安装包（内置 WebView2，约 211MB）：

```powershell
npm.cmd run build:win:offline
```

32 位安装包（需要先装目标工具链）：

```powershell
rustup target add i686-pc-windows-msvc
npm.cmd run build:win32
```

| 产物 | 路径 |
|---|---|
| **exe 安装包** | `src-tauri\target\release\bundle\nsis\AstroMan_1.0.7_x64-setup.exe` |
| msi 安装包 | `src-tauri\target\release\bundle\msi\AstroMan_1.0.7_x64_en-US.msi` |
| 免安装可执行文件 | `src-tauri\target\release\astroman.exe` |

安装向导内置语言与路径选择：

- 首屏可选安装语言（简体中文 / English），卸载程序同样支持
- 随后在「选择安装位置」页可修改安装目录，默认 `%LOCALAPPDATA%\AstroMan`
- 采用当前用户安装模式，全程无需管理员权限

**分发方式**：把 `AstroMan_1.0.7_x64-setup.exe` 发给对方，双击安装即可使用。`astroman.exe` 只是免安装可执行文件，没有卸载入口和开始菜单快捷方式，不建议直接分发。

标准安装包在缺少 WebView2 时联网下载安装；目标机器完全离线时，改用 `AstroMan_*_offline-setup.exe` 离线包。

> 安装包未做代码签名，首次运行 Windows 会提示 SmartScreen，选择「更多信息 → 仍要运行」即可。

### 安装包体积优化

| 优化项 | 配置 | 作用 |
|---|---|---|
| Rust 发布配置 | `panic = "abort"` + `lto` + `codegen-units = 1` + `opt-level = "z"` + `strip` | 去掉符号表、开启全量 LTO、按体积优化 |
| 前端资源压缩 | `tauri = { features = ["compression"] }` | 内嵌的 JS / CSS 以 Brotli 压缩存放 |
| TLS 后端 | reqwest `native-tls` | 复用系统证书库，移除 rustls / ring 依赖 |
| 安装器压缩 | NSIS `compression = "lzma"` | 安装包使用 LZMA 最高压缩 |

实测体积：`astroman.exe` 约 7.4 MB，`AstroMan_1.0.7_x64-setup.exe` 约 5.2 MB（离线包约 211 MB）。

> 国内网络首次打包时，下载 NSIS 工具容易超时（`timeout: global`）。可先设置镜像再执行：
>
> ```powershell
> $env:TAURI_BUNDLER_TOOLS_GITHUB_MIRROR='https://gh-proxy.com/https://github.com'
> npm.cmd run build:win
> ```
>
> 工具下载后会缓存在 `%LOCALAPPDATA%\tauri`，后续打包不再联网。

### macOS

首次构建前安装双架构标准库：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

构建通用包（Apple Silicon + Intel 一份安装包）：

```bash
npm run build:mac
```

只构建当前架构的 `app` + `dmg`：

```bash
npm run tauri build
```

| 产物 | 路径 |
|---|---|
| **dmg 安装包** | `src-tauri/target/universal-apple-darwin/release/bundle/dmg/AstroMan_*_universal.dmg` |
| 应用程序 | `src-tauri/target/universal-apple-darwin/release/bundle/macos/AstroMan.app` |
| 当前架构可执行文件 | `src-tauri/target/release/bundle/macos/AstroMan.app` |

最低支持 macOS 10.15（Catalina）。Release 中的 dmg 是通用包，同时包含 Intel（`x86_64`）与 Apple Silicon（`arm64`）切片；安装、放行与 Gatekeeper 说明见 [compatibility.md](compatibility.md)。

---

## 数据位置

| 内容 | Windows | macOS |
|---|---|---|
| 数据库 | `%APPDATA%\AstroMan\astroman.db` | `~/Library/Application Support/AstroMan/astroman.db` |
| WAL 临时文件 | `astroman.db-wal` / `astroman.db-shm`（同目录） | 同目录 |
| 存储位置配置 | `%APPDATA%\com.astroman.app\storage.json` | `~/Library/Application Support/com.astroman.app/storage.json` |

> 从旧版本升级时会自动迁移旧数据：优先从 `%APPDATA%\FbinHttp\fbinhttp.db`（或更早的 `FinkMeter\finkmeter.db`）复制到 `%APPDATA%\AstroMan\astroman.db`，目标已存在则不覆盖，原目录保留。自定义数据目录里的旧数据库也会自动改名为 `astroman.db`。配置目录 identifier 已迁到 `com.astroman.app`，首次启动会从 `com.fbinhttp.app` / `com.finkmeter.app` 目录自动复制 `storage.json`、`settings.json` 与窗口状态，不覆盖已有文件。
>
> Windows 上 identifier 变更会让「应用和功能」里残留一条旧卸载登记，卸载旧条目不影响已迁移的数据，可手动清理。

首次启动会自动建库，并写入一份可直接试用的示例数据：

- 项目：`示例项目`（已存在但仍是初始示例时会自动补齐场景）
- 集合：`01 基础请求`、`02 认证与请求头`、`03 表单与内容类型`、`04 变量与脚本`、`05 状态码与延迟`、`06 压测示例`
- 请求：约 30 个场景接口，覆盖 GET/POST/PUT/PATCH/DELETE/HEAD、Bearer / API Key 认证、Form / Raw / JSON Body、内置变量、前置与后置脚本、`<500` / `401` / `400` 等状态码、延迟与重定向、压测基准接口
- 环境：`本地环境`（`base_url = https://httpbin.org`、`token`、`api_key`、`user_id`、`username`，并附带一段环境脚本示例）

> `httpbin.org` 在国内可能较慢。把 URL 换成你自己的接口，或修改环境变量 `base_url` 即可。

---

## 开发约定

- Rust 结构体统一使用 `#[serde(rename_all = "camelCase")]`，与前端类型对齐
- 前端类型定义集中在 `src/types/models.ts`
- 所有 IPC 调用集中在 `src/api/`
- 页面不直接访问系统能力，网络与存储全部走 Rust 命令
- 新增命令需要在 `src-tauri/src/lib.rs` 的 `generate_handler!` 中注册
