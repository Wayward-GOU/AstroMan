# 企业部署与交付

> 面向内网 / 受管终端的部署说明：数据目录、企业策略、更新渠道与灰度、代码签名、离线安装、卸载与诊断。
> 普通个人用户不需要看这份文档，装完即用。

## 1. 数据目录与卸载

| 平台 | 默认数据目录 |
|---|---|
| Windows | `%APPDATA%\AstroMan` |
| macOS | `~/Library/Application Support/AstroMan` |
| Linux | `~/.local/share/AstroMan` |

数据目录里是 `astroman.db`（SQLite）、日志、备份、文档站配置与 `policy.json`；可以在 **设置 → 数据与存储** 改到其它位置（例如加密盘），改完会迁移并在下次启动生效。

**卸载不会删除数据**。NSIS 安装器只移除程序与快捷方式，`%APPDATA%\AstroMan` 原样保留；便携版解压即用，删除文件夹即可，同样不碰数据目录。重装后数据、密钥与历史都会回来。需要彻底清理时，先备份再手动删除数据目录。

安装模式为 `both`：安装时可选择「仅当前用户」（无需管理员）或「所有用户」（写入 Program Files，需要管理员）。两种模式共用同一份用户数据目录。

## 2. 企业策略 `policy.json`

把 `policy.json` 放到数据目录根下（与 `astroman.db` 同级），启动时读取，**设置 → 企业策略** 可以查看生效内容、复制路径、重新加载。模板见 [src-tauri/policy.example.json](../src-tauri/policy.example.json)。

| 字段 | 类型 | 默认 | 作用 |
|---|---|---|---|
| `disableAi` | bool | `false` | 禁用航仔 AI：会话、模型列表、连通性测试、页面分析都会返回 `E_POLICY` |
| `disableLanServices` | bool | `false` | 禁止 Mock Server / 文档站监听 `0.0.0.0` 等非回环地址，`127.0.0.1` 仍可用 |
| `disableTelemetry` | bool | `false` | 保留位：当前版本没有遥测，后续上报能力统一受它控制 |
| `forceEncryptBackup` | bool | `false` | 异地备份必须填写口令；空口令上传直接拒绝 |
| `allowedImportFormats` | string[] | `[]` | 允许导入的格式：`curl` / `postman` / `insomnia` / `openapi` / `har` / `bruno`；空数组表示不限制 |
| `retentionDays` | number | `0` | 请求历史保留天数，`0` 表示只按 10 万条上限保留；上限 3650 天 |

```json
{
  "disableAi": true,
  "disableLanServices": true,
  "forceEncryptBackup": true,
  "allowedImportFormats": ["openapi", "har"],
  "retentionDays": 30
}
```

行为约定：

- **文件不存在** = 不限制，个人用户无感；**格式写错** = 按默认策略启动，设置页和诊断包会显示解析错误，不会因为一个逗号导致整台机器打不开。
- 策略只做「能力闸门」，不改变已有数据的可读性：关闭 AI / 局域网服务后，历史请求、文档、备份仍可查看和导出。
- 字段全部可选，未知字段忽略；老版本读新字段不会拒绝启动。
- 修改后点「重新加载」即可生效，无需重启。

## 3. 更新渠道与灰度

| 变量 / 配置 | 说明 |
|---|---|
| `ASTROMAN_UPDATE_CHANNEL=beta` | 走 GitHub Releases 列表，优先最新的预发布版本；默认 `stable` 走 latest 正式版 |
| `ASTROMAN_UPDATE_MANIFEST=https://...` | 企业内网更新清单，只接受 `https://`；配置后优先于 GitHub |

内网清单格式见 [update-manifest.example.json](./update-manifest.example.json)：

```json
{
  "version": "1.2.0",
  "channel": "stable",
  "rolloutPercent": 20,
  "releaseUrl": "https://intranet.example.com/astroman/releases/1.2.0",
  "assets": [
    {
      "name": "AstroMan_1.2.0_x64-setup.exe",
      "url": "https://intranet.example.com/astroman/releases/1.2.0/AstroMan_1.2.0_x64-setup.exe",
      "sha256": "64 位小写十六进制摘要",
      "size": 5242880
    }
  ]
}
```

- 清单里所有 `assets` 必须带 `sha256`，缺一个就拒绝自动更新；下载完成后先校验摘要，再写盘并打开安装包，避免内网镜像被替换后静默安装。
- `rolloutPercent` 是灰度比例（0–100），按安装 ID 稳定分桶：同一台机器每次都落在同一桶，不会反复摇号。安装 ID 存在数据目录的 `install-id`，删掉后会重新分桶。
- 只会升级，不会把远端版本当降级包安装；下载失败会打开发布页兜底。
- 检查更新在 **设置 → 版本更新**，或在「关于」里手动触发；自动检查可以在设置里关闭。

## 4. 代码签名

仓库提供了签名脚本，CI 里配置对应 secrets 后自动生效；没配置时保持现状（未签名 / ad-hoc），流程不会失败。

**Windows（Authenticode + RFC3161 时间戳）**

```powershell
$env:WINDOWS_CERT_PFX = 'D:\certs\astroman.pfx'
$env:WINDOWS_CERT_PASSWORD = '***'
powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 -Path 'AstroMan_1.1.0_x64-setup.exe'
```

也可以改用证书存储里的指纹：`$env:WINDOWS_CERT_THUMBPRINT = 'A1B2...'`。脚本签名后会执行 `signtool verify /pa`，失败直接退出 1。

CI secrets：`WINDOWS_CERT_PFX_BASE64`（PFX 文件 base64）、`WINDOWS_CERT_PASSWORD`。配置后 Release 流程会签名标准安装包、离线安装包与便携版 exe，并用签名产物替换已上传的资产。

**macOS（Developer ID 签名 + 公证 + 装订）**

```bash
export MACOS_SIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
export APPLE_ID='you@example.com'
export APPLE_TEAM_ID='TEAMID'
export APPLE_APP_PASSWORD='xxxx-xxxx-xxxx-xxxx'
bash scripts/notarize-macos.sh '/Applications/AstroMan.app'
```

CI secrets：`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`。配置后 Tauri 构建阶段直接签名并公证；未配置时仍做 ad-hoc 签名，首次打开按 [兼容性与安装](./compatibility.md#macos-安装) 放行。

> 证书本身无法由仓库代签：Windows 需要商业代码签名证书，macOS 需要 Apple Developer Program 账号。脚本只负责把已有证书接进构建与发布链路。

## 5. 离线安装

| 场景 | 产物 |
|---|---|
| 能联网、缺 WebView2 | `AstroMan_*_x64-setup.exe`（约 5MB，安装时联网补齐 WebView2） |
| 断网 / 内网 / LTSC / Server | `AstroMan_*_x64-offline-setup.exe` / `*_x86-offline-setup.exe`（约 211MB，内置 WebView2 离线运行时） |
| 不允许安装、绿色使用 | `AstroMan_*_x64-portable.zip` / `*_x86-portable.zip`，解压双击 `astroman.exe` |

便携版不写注册表，但仍需要系统里存在 WebView2 运行时；Windows 10 1809+ / Windows 11 通常自带，精简版系统请先装 WebView2 Runtime。

## 6. 诊断包

**设置 → 备份与自检 → 诊断包 → 导出诊断包**，生成一个 ZIP：版本 / 系统 / WebView2 版本、运行档位与内存上限、数据库版本 / journal 模式 / 表行数、脱敏后的设置、最近 200 行日志、数据目录风险提示（OneDrive / 网盘同步、UNC 网络盘、超长路径、磁盘剩余空间不足 1GB）。

**不会包含**：AI Key、客户端证书口令、环境变量密文、请求正文、文档站访问 token。代理 URL 中的账号口令与系统用户名会做掩码。发给厂商或 IT 之前可以自己解压检查。

## 7. 兼容性回归与长稳

- 兼容矩阵（Win10 1809 / Win11 / x86 / 无 Edge / 200% 缩放）与安装包差异见 [compatibility.md](./compatibility.md#兼容性回归矩阵w4)。
- 长稳与内存回归步骤见 [compatibility.md](./compatibility.md#长稳与内存回归w4)，脚本为 [scripts/measure-rss.ps1](../scripts/measure-rss.ps1)，支持 `-MaxGrowthMb` / `-MaxPeakMb` 门禁。
