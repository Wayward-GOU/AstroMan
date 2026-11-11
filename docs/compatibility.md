# 系统兼容性与安装

> 安装包下载入口见仓库 [README](../README.md#下载安装)。本文只说明系统要求、安装包差异与首次放行。


| 平台 | 安装包 | 说明 |
|---|---|---|
| Windows 10/11（x64） | `AstroMan_*_x64-setup.exe` | 标准安装包（约 5MB），缺少 WebView2 时安装过程联网补齐 |
| Windows 10/11 离线（x64） | `AstroMan_*_x64-offline-setup.exe` | 内置 WebView2 离线运行时（约 211MB），断网 / 内网 / LTSC / Server 机器用 |
| Windows 10（32 位 x86） | `AstroMan_*_x86-setup.exe` / `AstroMan_*_x86-offline-setup.exe` | 32 位系统专用，标准包与离线包各一份 |
| Windows 便携版 | `AstroMan_*_x64-portable.zip` / `AstroMan_*_x86-portable.zip` | 解压双击 `astroman.exe` 即用，不写注册表；安装器报错时的兜底方案 |
| macOS 10.15+（Apple Silicon / Intel） | `AstroMan_*_universal.dmg` | 通用包，自带 `arm64` 与 `x86_64` 两个切片，**Intel Mac 无需单独下载 Intel 版** |

**系统兼容性**

| 系统 | 下限（最低可运行） | 上限（支持范围） | 架构 / 安装包 | 说明 |
|---|---|---|---|---|
| Windows | Windows 10 1809（build 17763） | Windows 11 最新正式版 | x64 / x86 / ARM64 | ARM64 用 x64 包（系统自带 x64 模拟）；32 位系统用 x86 包；Windows 7 / 8 / 8.1 不支持 |
| macOS | macOS 10.15 Catalina | 当前最新正式版 | Intel `x86_64` / Apple Silicon `arm64` | 10.14 及更早不支持；Intel Mac 需 2012 年及以后且能升级到 Catalina 的机型（Mac Pro 2010/2012 不在内） |
| Linux | — | — | — | 暂未提供构建，不支持 |

补充说明：

- **macOS 10.15 的界面降级**：Catalina 的 WebKit 不支持 `color-mix()`，个别半透明描边、选中底色会变淡或透明；功能正常，Big Sur / Monterey 及以上基本无差异。
- **标准包与离线包**：标准安装包约 5MB，目标机器缺 WebView2 时会在安装过程联网补齐；断网、内网、LTSC / Server 机器请下载带 `offline` 的安装包（约 211MB，已内置 WebView2）。
- **签名与兼容性无关**：默认构建不做商业代码签名，Windows 首次运行可能弹 SmartScreen（选「更多信息 → 仍要运行」），macOS 会提示「无法验证开发者」（按下方 macOS 安装章节放行）。要做到双击零提示，需要 Apple Developer ID 公证与 Windows 代码签名证书；配置方法见 [企业部署与交付](./enterprise.md#4-代码签名)。
- **安装器兜底**：个别机器卡在「创建快捷方式」并报 `Exception Processing Message 0xc0000005`，通常是杀毒软件 / 安全卫士拦截了 shell 快捷方式创建；临时退出安全软件后重试，或直接用便携版 zip 解压运行。

macOS 首次打开若出现下面这个提示（按钮只有「完成 / 移到废纸篓」），是系统对未公证应用的 Gatekeeper 拦截，**不是芯片架构不兼容**：

> Apple 无法验证「AstroMan.app」是否包含可能危害 Mac 安全或泄漏隐私的恶意软件。

按下方「macOS 安装」中的方式一（一键安装脚本）或方式二（系统设置放行）处理即可。

若提示「AstroMan 已损坏，无法打开，你应该将它移到废纸篓」，这是浏览器下载带来的隔离属性问题，**不要点「移到废纸篓」**，在终端执行下面命令后重新打开：

```bash
sudo xattr -dr com.apple.quarantine /Applications/AstroMan.app
open /Applications/AstroMan.app
```

---

## macOS 安装

最低支持 macOS 10.15（Catalina），最高跟随当前 macOS 正式版；Intel Mac 需 2012 年及以后、能升级到 Catalina 的机型（Mac Pro 2010/2012 与 macOS 10.14 及更早的机器不支持）。Release 中的 dmg 是通用包，同时包含 Intel（`x86_64`）与 Apple Silicon（`arm64`）切片，安装脚本会自动校验本机架构是否被支持。

安装包做的是 ad-hoc 签名、未做 Apple 公证，因此首次打开会触发 Gatekeeper 提示「Apple 无法验证 AstroMan.app」。这是签名 / 隔离属性问题，不是芯片不兼容，按下面任一方式放行即可。

**方式一：一键安装脚本（推荐）**

仓库提供了 [scripts/install-macos.sh](../scripts/install-macos.sh)，发布时由 [release.yml](../.github/workflows/release.yml) 上传到 Release 附件，一条命令完成下载、架构校验、安装、清除隔离属性、ad-hoc 签名与启动：

```bash
curl -fLO https://github.com/Wayward-GOU/AstroMan/releases/latest/download/install-macos.sh
bash install-macos.sh v1.0.9
```

不带参数执行时安装最新正式版。仓库若为私有，需要先提供 Token：

```bash
GITHUB_TOKEN=ghp_xxx bash install-macos.sh v1.0.9
```

**方式二：手动安装**

1. 打开 dmg，把 `AstroMan.app` 拖到「应用程序」
2. 双击打开，若提示「Apple 无法验证」：打开「系统设置 → 隐私与安全性」，在「安全性」区域点击「仍要打开」，输入密码确认（macOS 15 起右键「打开」不再能绕过该提示，必须走这一步）
3. 也可以直接在终端手动放行：

```bash
sudo xattr -dr com.apple.quarantine /Applications/AstroMan.app
sudo codesign --force --deep --sign - /Applications/AstroMan.app
open /Applications/AstroMan.app
```

> Gatekeeper 只认 Apple 签发的 Developer ID 证书 + 公证票据，GitHub Actions 无法替代签发，只能执行 `codesign` / `notarytool`。如需彻底消除提示，准备 Apple Developer Program 账号后在 workflow 中配置 `APPLE_CERTIFICATE`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` 等 secrets 即可。当前仓库已配置 ad-hoc 签名（`signingIdentity: "-"`），可避免系统把应用判定为「已损坏」。签名与公证的完整配置见 [企业部署与交付](./enterprise.md#4-代码签名)。

---

## 兼容性回归矩阵（W4）

| # | 环境 | 验证点 | 方式 | 状态 |
|---|---|---|---|---|
| 1 | Windows 10 1809 x64 | 安装、启动、WebView2、HTTPS 请求、抓包、更新检查 | 虚拟机手工回归 | 待人工回归 |
| 2 | Windows 11 最新正式版 x64 | 同上 + 高 DPI / 多显示器 | 物理机手工回归 | 待人工回归 |
| 3 | Windows 10 x86（32 位） | `AstroMan_*_x86-setup.exe` 安装与启动、抓包驱动 | 虚拟机手工回归 | 待人工回归 |
| 4 | 无 Edge / 缺 WebView2 | 标准包联网补齐、离线包内置运行时；安装后 `app_info.webviewVersion` 非空 | 虚拟机断网 + 卸载 WebView2 后手工回归 | 待人工回归 |
| 5 | 200% 缩放 / 最小窗口 | 900×560 等效宽度无整页横向滚动、地址栏与发送按钮可用、键盘焦点可见 | Playwright `e2e/enterprise.spec.ts` | 自动化通过 |
| 6 | 纯键盘 | Tab 聚焦地址栏 → Enter 发送 → 响应渲染；焦点描边可见 | Playwright `e2e/enterprise.spec.ts` | 自动化通过 |
| 7 | macOS 10.15 Catalina | 启动、`color-mix()` 降级、HTTPS 请求 | 旧版 macOS 手工回归 | 待人工回归 |

补充口径：

- 第 1–4、7 项依赖真实系统 / 虚拟机，本地 CI 无法替代；发版前按表逐项勾选，结果记在 Release 说明里。
- 第 5 项等效 200% 缩放：应用最小窗口宽度 900 CSS px，200% 缩放下可视区约 450 CSS px；自动化用 900×560 视口验证不出现整页横向滚动，手工再按「显示设置 → 缩放 200%」复核一次。
- 无 Edge 环境请优先用带 `offline` 的安装包；便携版不内置 WebView2，精简系统需先单独安装运行时。

## 长稳与内存回归（W4）

用 [scripts/measure-rss.ps1](../scripts/measure-rss.ps1) 采样「主进程 + WebView2 子进程」的 Working Set 合计，脚本只读性能计数器，对被测进程零侵入。

```powershell
# 1) 启动 AstroMan，先空转 5 分钟，采一条基线
powershell -ExecutionPolicy Bypass -File scripts/measure-rss.ps1 -DurationSec 300 -Csv rss-idle.csv

# 2) 执行 30 分钟混合负载，同时采样并按门禁判定
powershell -ExecutionPolicy Bypass -File scripts/measure-rss.ps1 -DurationSec 1800 -IntervalMs 5000 -Csv rss-longrun.csv -MaxGrowthMb 120 -MaxPeakMb 800
```

混合负载建议覆盖：批量发送 200+ 请求、抓包录制 2 分钟后筛选 / 展开树 / 导出 HAR、打开大响应并切换 Pretty / Raw、运行一次压测并打开报告、导入一份大 OpenAPI / Postman 集合、切到航仔发一轮对话再切回。

判定口径：

- 末次样本相对首次的增长 ≤ 120 MB，峰值 ≤ 800 MB（按目标机型档位调整），超限脚本退出码 1；
- 期间无崩溃、无未响应、无数据库 `E_BUSY`；
- 连续两轮结果相差不超过 20%，视为无持续泄漏；
- CSV 与结论附在发版记录里，便于下一版对比。

