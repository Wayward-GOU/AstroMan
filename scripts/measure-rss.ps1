# 进程组内存采样：主进程 + WebView2 子进程的 Working Set 合计
#
# 用法（在仓库根目录或任意目录均可）：
#   powershell -ExecutionPolicy Bypass -File scripts/measure-rss.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/measure-rss.ps1 -ProcessName astroman -IntervalMs 500 -DurationSec 120 -Csv rss.csv
#   powershell -ExecutionPolicy Bypass -File scripts/measure-rss.ps1 -DurationSec 1800 -MaxGrowthMb 20 -MaxPeakMb 700
#
# 说明：
# - 开发态进程名是 `cargo run` 派生的 astroman.exe，安装态同名，默认可用。
# - WebView2 子进程按父进程链归集：msedgewebview2.exe 的父进程可能是主进程，
#   也可能挂在其他 msedgewebview2.exe 下，这里递归展开整棵子树。
# - Working Set 是任务管理器「内存（专用工作集）」之外的常驻集，跨进程加总能反映
#   真实占用；采样只读性能计数器，对被测进程零侵入。
# - W4 长稳门禁：-MaxGrowthMb 控制末次相对首次的增长，-MaxPeakMb 控制峰值；
#   任一超限以退出码 1 结束，可直接接 CI / 手工长稳流程。
param(
  [string]$ProcessName = 'astroman',
  [int]$IntervalMs = 1000,
  [int]$DurationSec = 60,
  [string]$Csv = '',
  [double]$MaxGrowthMb = 0,
  [double]$MaxPeakMb = 0
)

$ErrorActionPreference = 'Stop'

function Get-ProcessTree([int[]]$RootIds) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, WorkingSetSize
  $byParent = @{}
  foreach ($p in $all) {
    if (-not $byParent.ContainsKey([int]$p.ParentProcessId)) {
      $byParent[[int]$p.ParentProcessId] = @()
    }
    $byParent[[int]$p.ParentProcessId] += $p
  }
  $seen = @{}
  $queue = New-Object System.Collections.Queue
  foreach ($id in $RootIds) { $queue.Enqueue($id) }
  $result = @()
  while ($queue.Count -gt 0) {
    $id = [int]$queue.Dequeue()
    if ($seen.ContainsKey($id)) { continue }
    $seen[$id] = $true
    if ($byParent.ContainsKey($id)) {
      foreach ($child in $byParent[$id]) {
        $result += $child
        $queue.Enqueue([int]$child.ProcessId)
      }
    }
  }
  return $result
}

$roots = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
if ($roots.Count -eq 0) {
  Write-Error "未找到进程 $ProcessName.exe，请先启动 AstroMan"
}

$rootIds = @($roots | ForEach-Object { $_.Id })
Write-Host ("采样中：{0}.exe（PID {1}）每 {2}ms 一次，共 {3}s" -f $ProcessName, ($rootIds -join ','), $IntervalMs, $DurationSec)

$rows = New-Object System.Collections.Generic.List[object]
$peak = 0.0
$first = $null
$deadline = (Get-Date).AddSeconds($DurationSec)

while ((Get-Date) -lt $deadline) {
  $rootsNow = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
  if ($rootsNow.Count -eq 0) {
    Write-Host '主进程已退出，采样结束'
    break
  }
  $rootIds = @($rootsNow | ForEach-Object { $_.Id })
  $tree = @(Get-ProcessTree $rootIds)
  $rootBytes = 0.0
  foreach ($r in $rootsNow) { $rootBytes += [double]$r.WorkingSet64 }
  $childBytes = 0.0
  foreach ($p in $tree) { $childBytes += [double]$p.WorkingSetSize }
  $totalMb = [math]::Round(($rootBytes + $childBytes) / 1MB, 1)
  $rootMb = [math]::Round($rootBytes / 1MB, 1)
  if ($totalMb -gt $peak) { $peak = $totalMb }
  if ($null -eq $first) { $first = $totalMb }
  $rows.Add([pscustomobject]@{
    Time      = (Get-Date).ToString('HH:mm:ss')
    RootMb    = $rootMb
    ChildMb   = [math]::Round($childBytes / 1MB, 1)
    TotalMb   = $totalMb
    Processes = $tree.Count + $rootsNow.Count
  })
  Start-Sleep -Milliseconds $IntervalMs
}

if ($rows.Count -eq 0) {
  Write-Error '没有采到任何样本'
}

$last = $rows[$rows.Count - 1]
$growth = [math]::Round([double]$last.TotalMb - [double]$first, 1)
Write-Host ''
Write-Host '======== 结果 ========'
Write-Host ("采样点数 : {0}" -f $rows.Count)
Write-Host ("首个样本 : {0} MB" -f $first)
Write-Host ("最终样本 : {0} MB（主进程 {1} MB + 子进程 {2} MB，共 {3} 个进程）" -f $last.TotalMb, $last.RootMb, $last.ChildMb, $last.Processes)
Write-Host ("峰值     : {0} MB" -f $peak)
Write-Host ("增长     : {0} MB" -f $growth)

if ($Csv) {
  $rows | Export-Csv -Path $Csv -NoTypeInformation -Encoding UTF8
  Write-Host ("明细已写入 {0}" -f (Resolve-Path $Csv))
}

$failed = $false
if ($MaxGrowthMb -gt 0 -and $growth -gt $MaxGrowthMb) {
  Write-Host ("FAIL: 空闲增长 {0} MB 超过门禁 {1} MB" -f $growth, $MaxGrowthMb) -ForegroundColor Red
  $failed = $true
}
if ($MaxPeakMb -gt 0 -and $peak -gt $MaxPeakMb) {
  Write-Host ("FAIL: 峰值 {0} MB 超过门禁 {1} MB" -f $peak, $MaxPeakMb) -ForegroundColor Red
  $failed = $true
}
if ($failed) { exit 1 }
Write-Host 'PASS: 长稳内存门禁通过' -ForegroundColor Green
