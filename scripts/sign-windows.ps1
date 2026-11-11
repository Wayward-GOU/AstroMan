# Authenticode 签名 + RFC3161 时间戳（W4 / A-20）
#
# 用法（Windows，仓库根目录）：
#   $env:WINDOWS_CERT_THUMBPRINT = 'A1B2...'      # 证书已导入当前用户 / 本机证书存储
#   powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 -Path 'src-tauri/target/release/bundle/nsis/AstroMan_1.1.0_x64-setup.exe'
#
# 或用 PFX（注意：口令通过环境变量读取，不要写进脚本）：
#   $env:WINDOWS_CERT_PFX = 'D:\certs\astroman.pfx'
#   $env:WINDOWS_CERT_PASSWORD = '***'
#
# 签名后必须 verify 成功；失败即退出 1，避免把未签名包发出去。
param(
  [Parameter(Mandatory = $true)][string]$Path,
  [string]$TimestampUrl = 'http://timestamp.digicert.com',
  [string]$Thumbprint = $env:WINDOWS_CERT_THUMBPRINT,
  [string]$PfxPath = $env:WINDOWS_CERT_PFX,
  [string]$PfxPassword = $env:WINDOWS_CERT_PASSWORD
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Path)) {
  throw "待签名文件不存在：$Path"
}

$signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if (-not $signtool) {
  $candidates = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } |
    Sort-Object FullName -Descending
  if (-not $candidates) { throw '未找到 signtool.exe，请安装 Windows SDK' }
  $signtool = $candidates[0].FullName
} else {
  $signtool = $signtool.Source
}

$args = @('sign', '/fd', 'SHA256', '/tr', $TimestampUrl, '/td', 'SHA256')
if ($Thumbprint) {
  $args += @('/sha1', $Thumbprint)
} elseif ($PfxPath) {
  if (-not (Test-Path -LiteralPath $PfxPath)) { throw "PFX 不存在：$PfxPath" }
  $args += @('/f', $PfxPath)
  if ($PfxPassword) { $args += @('/p', $PfxPassword) }
} else {
  throw '未配置证书：设置 WINDOWS_CERT_THUMBPRINT 或 WINDOWS_CERT_PFX'
}
$args += $Path

Write-Host "签名：$Path"
& $signtool @args
if ($LASTEXITCODE -ne 0) { throw "signtool sign 失败（退出码 $LASTEXITCODE）" }

& $signtool verify /pa /v $Path
if ($LASTEXITCODE -ne 0) { throw "signtool verify 失败（退出码 $LASTEXITCODE）" }

Write-Host 'PASS: Authenticode 签名与时间戳校验通过' -ForegroundColor Green
