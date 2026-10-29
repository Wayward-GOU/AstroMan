---
name: fbinhttp-build-release
description: FbinHttp 的打包、图标、托盘与发布流程。需要构建 Windows/macOS 安装包、更换应用图标或托盘图标、发版打 tag、排查构建失败时使用。触发词：打包、构建、安装包、出包、icon、icns、ico、托盘图标、发布、release、构建失败。
---

# 构建与发布

## 命令

| 目的 | 命令 | 产物 |
|---|---|---|
| 前端类型检查 + 构建 | `npm run build` | `dist/` |
| Windows 安装包 | `npm run build:win` | `FbinHttp_<版本>_x64-setup.exe`（NSIS） |
| macOS 安装包 | `npm run build:mac` | `FbinHttp_<版本>_universal.dmg` |
| 仅校验 Rust | `cd src-tauri && cargo check` | — |
| 前端单测 | `npm run test` | vitest 46 条 |
| Rust 单测 | `cd src-tauri && cargo test` | 22 条 |

产物统一在 `src-tauri/target/release/bundle/` 下。

Windows 下 `cargo check` 若报 `hit restricted ... D:\Software\Rust\cargo\.global-cache-journal`，说明被沙箱拦住了，需要在关闭沙箱的状态下执行，这不是代码错误。

## 打包配置（`src-tauri/tauri.conf.json`）

- `productName: "FbinHttp"` → 决定安装包名、可执行文件名、macOS `.app` 名
- Cargo `[package] name = "fbinhttp"` / `[lib] name = "fbinhttp_lib"` → 未打包的裸可执行文件是 `target/release/fbinhttp.exe`，与安装包命名一致；改 crate 名必须同步 `main.rs` 的 `fbinhttp_lib::run()`
- `identifier: "com.fbinhttp.app"` → 决定配置文件目录（`%APPDATA%\com.fbinhttp.app` / `~/Library/Application Support/com.fbinhttp.app`）。旧值 `com.finkmeter.app` 已迁走，`state.rs::migrate_legacy_config` 会在启动时把旧目录里的 json 复制过来；再次改动必须同时补迁移，否则 `storage.json`、`settings.json` 与窗口状态都会"消失"
- `bundle.icon: ["icons/32x32.png","icons/128x128.png","icons/128x128@2x.png","icons/icon.icns","icons/icon.ico"]`
- Windows：`bundle.windows.nsis.installerIcon: "icons/installer.ico"`、`installMode: currentUser`、`languages: [SimpChinese, English]`、`displayLanguageSelector: true`、`compression: lzma`
- macOS：`minimumSystemVersion: "11.0"`、`signingIdentity: "-"`（ad-hoc）

## 图标体系

| 文件 | 用途 | 来源 |
|---|---|---|
| `icons/icon.ico` | Windows 应用图标（多尺寸） | 软件主图 |
| `icons/32x32.png` / `128x128.png` / `128x128@2x.png` / `icon.png` | 打包与 Linux 图标 | 软件主图 |
| `icons/icon.icns` | macOS `.app` / dmg | Mac 安装图标 |
| `icons/installer.ico` | NSIS 安装向导 | Windows 安装图标 |
| `icons/tray.png` / `tray@2x.png` | 系统托盘（32 / 64，透明底） | 托盘图标 |
| `public/logo.png` | 界面 favicon / 设置页展示 | 软件主图 |

- 原 4 张设计稿已删除，`design/` 目录不再存在。若需更换图标，请重新提供正方形 PNG（建议 1024×1024），再执行下面流程
- 生成全套：`npx tauri icon <主图.png> -o src-tauri/icons`
- 托盘图标必须单独覆盖 `icons/tray.png`，因为 `lib.rs` 用 `include_bytes!` 在**编译期**嵌入，改完要重新编译
- `installer.ico` 用多尺寸 ICO（16~256）；`icon.icns` 可用 Pillow 从 PNG 转换
- 图标必须是**圆角矩形透明背景**，四周不能有白边：生成时先按真实圆角拟合 alpha 蒙版，再清理最外几像素的浅色描边，否则缩小后会出现一圈灰边

## 发版流程

1. 同步版本号：`package.json`、`package-lock.json`（2 处）、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 的 `fbinhttp` 条目、`src-tauri/tauri.conf.json`（当前 1.0.7）
2. 提交后打 tag：`git tag v1.0.7 && git push origin v1.0.7`，同时把 `.github/workflows/release.yml` 里的 `releaseBody` 更新成本次版本说明（GitHub Release 直接用它）；版本说明追加到 `### 本次更新（x.y.z）` 段落
3. `.github/workflows/release.yml` 在 tag 推送时自动构建 Windows NSIS 与 macOS universal dmg，上传到 GitHub Release
4. 应用内更新检查走 `commands/update.rs` 的 `RELEASE_API`（`https://api.github.com/repos/Wayward-GOU/FbinHttp/releases/latest`），仓库地址已改为 FbinHttp
5. macOS 用户安装脚本 `scripts/install-macos.sh`（`REPO=Wayward-GOU/FbinHttp`）从 Release 下载 dmg，并绕过未签名校验；README 中的安装命令同源

## 构建排错

| 现象 | 原因 / 处理 |
|---|---|
| 启动即 panic `no such column ... in CREATE INDEX` | 索引依赖新列但建在 schema.sql 里，移到 `db/mod.rs::migrate()` 的补列之后 |
| 启动日志 `自定义数据目录不可用，回退到默认目录` | 自定义目录打不开（被删/无权限/库损坏）；配置会保留，修好后重启仍会尝试原目录 |
| 窗口一片空白、无报错弹层 | 前端 `setup()` 抛异常，最常见是 `App.vue` 里 `navs` 的 TDZ，用 `npm run dev` 看控制台 |
| 托盘图标没变 | `tray.png` 是编译期嵌入，需重新编译，仅替换文件不生效 |
| 安装包名仍是旧名 | 检查 `productName` 是否被改动，以及构建前是否重新加载了配置 |
| 安装包/客户端界面还是旧版 | 安装版把 `dist/` 编进二进制，只跑 `npm run build` 或重启旧 exe 不会更新；必须重新 `npm run build:win` / `build:mac`（`beforeBuildCommand` 会自动再构建一次前端） |
