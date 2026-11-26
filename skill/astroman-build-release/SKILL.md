---
name: astroman-build-release
description: AstroMan 的打包、图标、托盘与发布流程。需要构建 Windows/macOS 安装包、更换应用图标或托盘图标、发版打 tag、排查构建失败时使用。触发词：打包、构建、安装包、出包、icon、icns、ico、托盘图标、发布、release、构建失败。
---

# 构建与发布

## 命令

| 目的 | 命令 | 产物 |
|---|---|---|
| 前端类型检查 + 构建 | `npm run build` | `dist/` |
| Windows 安装包 | `npm run build:win` | `AstroMan_<版本>_x64-setup.exe`（NSIS） |
| macOS 安装包 | `npm run build:mac` | `AstroMan_<版本>_universal.dmg` |
| 仅校验 Rust | `cd src-tauri && cargo check` | — |
| 前端单测 | `npm run test` | vitest：39 spec / 391 条 |
| Rust 单测 | `cd src-tauri && cargo test --lib` | 475 条（8 ignored） |
| 覆盖率（本地） | `cd src-tauri && cargo tarpaulin --config tarpaulin.toml --engine llvm` | `target/coverage/` |
| 版本号 | `npm run version:check` / `npm run version:set 1.0.9` | 三处版本一致 |

产物统一在 `src-tauri/target/release/bundle/` 下。测试与门禁的完整口径见 skill `astroman-testing`。

Windows 下 `cargo check` 若报 `hit restricted ... D:\Software\Rust\cargo\.global-cache-journal`，说明被沙箱拦住了，需要在关闭沙箱的状态下执行，这不是代码错误。

## 打包配置（`src-tauri/tauri.conf.json`）

- `productName: "AstroMan"` → 决定安装包名、可执行文件名、macOS `.app` 名
- 兼容范围：Windows 10 1809+ x64 / x86（Windows 7/8.1 无新 WebView2，不支持）；macOS 10.15+（Tauri 2 下限），ARM64 Windows 通过 x64 模拟运行
- `bundle.windows.webviewInstallMode = embedBootstrapper`：标准安装包约 5MB，缺 WebView2 时安装过程联网补齐；`src-tauri/tauri.offline.conf.json` 覆盖为 `offlineInstaller`，`npm run build:win:offline` 产出约 211MB 的离线包，CI 会重命名为 `AstroMan_<版本>_<arch>-offline-setup.exe` 一并上传
- 32 位安装包：`rustup target add i686-pc-windows-msvc` 后执行 `npm run build:win32`，产物为 `AstroMan_<版本>_x86-setup.exe`；CI 已加对应矩阵
- 便携版兜底：CI 每组 Windows 构建后额外把 `astroman.exe` 打成 `AstroMan_<版本>_<arch>-portable.zip` 并上传；个别机器 NSIS 创建快捷方式报 `0xc0000005`（安全软件拦截 shell）时，让用户改用便携版解压运行
- Cargo `[package] name = "astroman"` / `[lib] name = "astroman_lib"` → 未打包的裸可执行文件是 `target/release/astroman.exe`，与安装包命名一致；改 crate 名必须同步 `main.rs` 的 `astroman_lib::run()`
- `identifier: "com.astroman.app"` → 决定配置文件目录（`%APPDATA%\com.astroman.app` / `~/Library/Application Support/com.astroman.app`）。旧值 `com.fbinhttp.app`、`com.finkmeter.app` 已迁走，`state.rs` 启动时会把旧目录里的 json 复制过来；数据目录默认 `AstroMan`，数据库 `astroman.db`，会依次从 `FbinHttp\fbinhttp.db` / `FinkMeter\finkmeter.db` 迁移（含 -wal/-shm）；自定义目录里的旧库也会自动改名。再次改动必须同时补迁移，否则数据会"消失"
- `bundle.icon: ["icons/32x32.png","icons/128x128.png","icons/128x128@2x.png","icons/icon.icns","icons/icon.ico"]`
- Windows：`bundle.windows.nsis.installerIcon: "icons/installer.ico"`、`installMode: currentUser`、`languages: [SimpChinese, English]`、`displayLanguageSelector: true`、`compression: lzma`
- macOS：`minimumSystemVersion: "10.15"`、`signingIdentity: "-"`（ad-hoc）

## 图标体系

| 文件 | 用途 | 来源 |
|---|---|---|
| `icons/icon.ico` | Windows 应用图标（多尺寸） | 软件主图 |
| `icons/32x32.png` / `128x128.png` / `128x128@2x.png` / `icon.png` | 打包与 Linux 图标 | 软件主图 |
| `icons/icon.icns` | macOS `.app` / dmg | Mac 安装图标 |
| `icons/installer.ico` | NSIS 安装向导 | Windows 安装图标 |
| `icons/tray.png` | 系统托盘（32，透明底） | `logo/托盘图标.png` |
| `public/logo.png` | 界面 favicon / 设置页展示 | 软件主图 |
| `docs/social-preview.png` | GitHub 社交预览（1280×640） | `logo/社交预览图.png` |

- 源文件在 `logo/`：`Mac-Logo.png`（白底，需把外部白底转透明后生成 `icon.icns`）、`Windows-Logo.png`（**外圈带白色底板**，生成前必须从边缘 flood-fill 去掉白色底板，否则任务栏图标会出现白边）、`托盘图标.png`、`社交预览图.png`
- 生成：`npx tauri icon <1024 主图> -o src-tauri/icons`；它会顺带生成 `android/`、`ios/`、`Square*Logo.png`、`StoreLogo.png`、`64x64.png`，本项目不用，生成后删除
- `icon.icns` 单独用 Mac 主图再跑一次 `tauri icon` 到临时目录，只拷回 `icon.icns`
- 托盘图标必须单独覆盖 `icons/tray.png`（32×32），`lib.rs` 用 `include_bytes!` 编译期嵌入，改完要重新编译
- `installer.ico` 单独用 Pillow 从 Windows 主图生成 16~256 多尺寸；`icon.icns` 与 `icon.ico` 不要手工拼
- 图标必须是**圆角矩形透明背景**，四周不能有白边：生成时先按真实圆角拟合 alpha 蒙版，再清理最外几像素的浅色描边，否则缩小后会出现一圈灰边

## 发版流程

1. 同步版本号：`npm run version:set 1.0.9` 一次写入 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 三处；脚本**不动** lock 文件——`package-lock.json` 用 `npm install --package-lock-only` 跟上，`Cargo.lock` 跑一次 `cargo check` 会自动同步。`npm run version:check` 必须通过（CI 的 `frontend` job 第一步就是它）
2. 提交前跑一遍与 CI 同源的门禁：`npm run test`、`npm run ui:check`（硬编码样式 + 23 套主题对比度）、`npm run build`、`cargo fmt --check --all`、`cargo clippy --lib --all-targets -- -D warnings`、`cargo test --lib`；覆盖率由 `src-tauri/tarpaulin.toml` 的 `fail-under`（当前 25%）判定，报告在 `target/coverage/`
3. 提交后打 tag：`git tag v1.0.9 && git push origin v1.0.9`，同时把 `.github/workflows/release.yml` 里的 `releaseBody` 更新成本次版本说明（GitHub Release 直接用它）；版本说明追加到 `### 本次更新（x.y.z）` 段落。历史版本不要继续堆在 `releaseBody` 里，只留一行 CHANGELOG 链接
4. **`releaseBody` 有硬上限，改完必须算长度**：该字段含 `${{ github.ref_name }}`，GitHub 按模板值计长，上限 21000，超了不会给出可读报错，而是把整个 workflow 判为 invalid —— Actions 里工作流名从 `Release` 退化成 `.github/workflows/release.yml`，push 不再产生任何 job（`total_count: 0`）。v1.1.5 踩过一次：累积三个版本说明后 11952 字符 / 22921 字节，加了 9 行就变 24047 字节越界。追加文案前先跑：

   ```bash
   python3 -c "
   L=open('.github/workflows/release.yml',encoding='utf-8').read().split('\n')
   s=next(i for i,l in enumerate(L) if 'releaseBody: |' in l)
   e=next(i for i in range(s+1,len(L)) if L[i].strip() and len(L[i])-len(L[i].lstrip())<=10)
   b='\n'.join(L[s+1:e])
   print('releaseBody', len(b), '字符 /', len(b.encode()), '字节，余量', 21000-len(b.encode()))
   "
   ```

   余量低于 2000 就先把历史版本段摘出去再写新内容。瘦身后当前为 4289 字符 / 8426 字节
5. `.github/workflows/release.yml` 在 tag 推送时自动构建 Windows NSIS 与 macOS universal dmg，上传到 GitHub Release
6. 应用内更新检查走 `commands/update.rs` 的 `RELEASE_API`（`https://api.github.com/repos/Wayward-GOU/AstroMan/releases/latest`），仓库地址已改为 AstroMan
7. macOS 用户安装脚本 `scripts/install-macos.sh`（`REPO=Wayward-GOU/AstroMan`）从 Release 下载 dmg，并绕过未签名校验；README 中的安装命令同源

### 覆盖重发（同一版本号替换安装包）

只适用于「已发布的版本有硬伤、必须立刻换包且不打算跳版本号」的情况。v1.1.5 走过一次（补请求体解析容错），代价与顺序都要清楚：

1. **先删 Release，再动 tag**：`tauri-action` 遇到已存在的 Release 时会上传同名资产并因命名冲突失败。用 API 删除：`curl -X DELETE -H "Authorization: Bearer $TOKEN" https://api.github.com/repos/Wayward-GOU/AstroMan/releases/<id>`（`<id>` 从 `releases?per_page=5` 的返回里取，不是 tag 名）。本机没装 `gh`，也没有 PAT 时需要用户提供一次，别把 token 写进仓库或脚本。
2. **强制移动标签**：`git tag -f v1.1.5 <新提交> && git push origin v1.1.5 --force`。轻量标签直接覆盖；带注解的标签要 `git tag -f -a` 并补一次 `--force`。推送已存在的标签必须带 `--force`，否则被拒。
3. **同一版本号不会触发自动更新**：`update.rs` 比对的是版本号，不会因为同版本号换了构建而提示。必须在 release body 与发布说明里写清「重新下载安装包覆盖安装」。
4. **资产名带版本号**（`AstroMan_1.1.5_universal.dmg`），新旧包同名，下载链接不变；正因如此才必须先删旧 Release，否则用户拿到的可能是旧缓存资产。
5. 事后把 CHANGELOG 与 `docs/release-x.y.z.md` 的日期改成重发日期，并补一段「重发说明」，说明版本号未变、改了什么、怎么升级。

### macOS universal 与 sidecar（externalBin）

`bundle.externalBin` 是 `binaries/astroman-cli`，Tauri 按 target triple 找 `binaries/astroman-cli-<triple>`。`--target universal-apple-darwin` 有两个坑，都已在 `scripts/prepare-cli.mjs` 里处理，别再手写一份：

1. **tauri-cli 的 universal 构建只 lipo 主程序**（`interface/rust/desktop.rs` 里只有 `lipo -create ... out_dir/bin_name`），sidecar 不会跟着合并；而 macOS bundler 打包时固定从 `target/universal-apple-darwin/release/astroman-cli` 取文件，取不到就报 `Failed to copy binary from ... does not exist`。脚本在 lipo 出 `binaries/astroman-cli-universal-apple-darwin` 之后，会把它复制到这个路径预置好
2. **单架构文件必须是真二进制**：`binaries/astroman-cli-aarch64-apple-darwin` 与 `-x86_64-apple-darwin` 若只是 0 字节占位（只为让 `tauri-build` 通过校验而写的那种），被拷进 `target/<triple>/release/` 的 sidecar 就是空文件。脚本现在编译完每个架构就 `install()` 覆盖占位

排错先跑这两条，输出都应是 `x86_64 arm64`：

```bash
lipo -archs src-tauri/binaries/astroman-cli-universal-apple-darwin
lipo -archs src-tauri/target/universal-apple-darwin/release/astroman-cli
```

`release.yml` 里的 `Verify macOS universal CLI` 步骤会在 Rust 编译前就做这两项断言，省掉 3 分钟白等。

## 构建排错

| 现象 | 原因 / 处理 |
|---|---|
| 启动即 panic `no such column ... in CREATE INDEX` | 索引依赖新列但建在 schema.sql 里，移到 `db/mod.rs::migrate()` 的补列之后 |
| 启动日志 `自定义数据目录不可用，回退到默认目录` | 自定义目录打不开（被删/无权限/库损坏）；配置会保留，修好后重启仍会尝试原目录 |
| 窗口一片空白、无报错弹层 | 前端 `setup()` 抛异常，最常见是 `App.vue` 里 `navs` 的 TDZ，用 `npm run dev` 看控制台 |
| 托盘图标没变 | `tray.png` 是编译期嵌入，需重新编译，仅替换文件不生效 |
| 安装包名仍是旧名 | 检查 `productName` 是否被改动，以及构建前是否重新加载了配置 |
| 安装包/客户端界面还是旧版 | 安装版把 `dist/` 编进二进制，只跑 `npm run build` 或重启旧 exe 不会更新；必须重新 `npm run build:win` / `build:mac`（`beforeBuildCommand` 会自动再构建一次前端） |
| macOS 报 `Failed to copy binary from ".../universal-apple-darwin/release/astroman-cli" does not exist` | universal 构建不合并 sidecar；不要跳过 `prepare-cli.mjs --target universal-apple-darwin`，它会把 fat binary 预置到该路径（详见上一节） |
