<#
.SYNOPSIS
  获取并校验 ETS2Nav 导航数据集（P4R Batch 5 §9）。

.DESCRIPTION
  职责严格限定为「把外部资产变成可信输入」，不含任何测试判定：

    1. 固定 URL：由 Tag + Asset 拼出 releases/download 路径，不使用 "latest"。
    2. 下载到受控工作目录（或复用**摘要已核对通过**的本地副本）。
    3. 计算 SHA-256 并与固定摘要逐字节比较。
    4. 摘要不符立即以非零退出，**不尝试继续**（半可信的数据集比没有数据集更危险）。
    5. 解压到暂存目录。
    6. 结构校验：必需文件存在且字节数非零。
    7. 落到目标目录，并打印可直接引用的路径与 routing.graph 摘要。

  信任根是**摘要**，不是缓存，也不是文件名。缓存只用来省一次 166 MB 下载：
  命中缓存同样要重算摘要，不符即失败（§10）。空缓存必须也能成功。

  本脚本不需要任何凭据：数据集发布在公共 Release 上，URL 走匿名 HTTPS。

.PARAMETER Dest
  数据集落地目录（将包含 routing.graph 等文件）。

.PARAMETER WorkDir
  下载缓存与暂存目录。缺省为系统临时目录下的 ets2nav-ci-dataset-<Tag>。

.EXAMPLE
  .\scripts\ci\prepare-dataset.ps1 -Dest "$env:RUNNER_TEMP\ets2nav-dataset\europe-v5"
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Repo = 'HaowenCang/ETS2Nav',
    [string]$Tag = 'dataset-europe-v5',
    [string]$Asset = 'ets2nav-dataset-europe-v5.zip',
    # 与 GitHub Release 附件元数据中的 digest 字段逐字节一致（2026-09-12 经 gh api 核对）。
    [string]$Sha256 = '15b03a6322bff6bd800f2ca1cb7b158ae843e937b6f3204fc438fb8c482831bb',

    [Parameter(Mandatory)][string]$Dest,
    [string]$WorkDir,
    [switch]$KeepStaging
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$EXIT_PASS = 0
$EXIT_USAGE = 2
$EXIT_ASSET = 3      # 外部资产失败：下载失败 / 摘要不符 / 结构不完整

# 必需文件（§9）。README-dataset.txt 与 diagnostics.json 是数据集自带说明与诊断，
# 不参与判定；下列 5 项是导航核心与 P5 真正读取的输入。
$REQUIRED = @('routing.graph', 'junction.graph', 'map.db', 'search.db', 'manifest.json')

function Write-Log { param([string]$Text = '') Write-Host $Text }

function Fail-Asset {
    param([string]$Message)
    Write-Log ''
    Write-Log "EXTERNAL ASSET FAILURE: $Message"
    exit $EXIT_ASSET
}

function Get-Sha256Lower {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

# ── 参数与工作目录 ───────────────────────────────────────────────────────────
if ($Sha256 -notmatch '^[0-9a-fA-F]{64}$') {
    Write-Log "用法错误: -Sha256 不是 64 位十六进制摘要"
    exit $EXIT_USAGE
}
$Sha256 = $Sha256.ToLowerInvariant()
$Dest = [System.IO.Path]::GetFullPath($Dest)
if (-not $WorkDir) { $WorkDir = Join-Path ([System.IO.Path]::GetTempPath()) "ets2nav-ci-dataset-$Tag" }
$WorkDir = [System.IO.Path]::GetFullPath($WorkDir)
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

$url = "https://github.com/$Repo/releases/download/$Tag/$Asset"
$zip = Join-Path $WorkDir $Asset

Write-Log '========================================================================'
Write-Log 'ETS2Nav dataset preparation (P4R Batch 5 §9)'
Write-Log '========================================================================'
Write-Log "repo       : $Repo"
Write-Log "tag        : $Tag"
Write-Log "asset      : $Asset"
Write-Log "url        : $url"
Write-Log "expected   : sha256:$Sha256"
Write-Log "work dir   : $WorkDir"
Write-Log "dest       : $Dest"

# ── 1/2. 下载或复用（复用同样要验摘要） ──────────────────────────────────────
$needDownload = $true
if (Test-Path -LiteralPath $zip -PathType Leaf) {
    Write-Log ''
    Write-Log '发现本地副本，先核对摘要（缓存不是信任根，§10）…'
    $have = Get-Sha256Lower -Path $zip
    if ($have -eq $Sha256) {
        Write-Log "缓存命中且摘要一致：$have（跳过下载）"
        $needDownload = $false
    } else {
        Write-Log "缓存摘要不符（$have），丢弃并重新下载"
        Remove-Item -LiteralPath $zip -Force
    }
}

if ($needDownload) {
    Write-Log ''
    Write-Log '下载中（匿名 HTTPS，不需要任何凭据）…'
    # Windows PowerShell 5.1 默认不启用 TLS 1.2，GitHub 会直接拒绝握手。
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        Write-Log "提示: 无法调整 SecurityProtocol（$($_.Exception.Message)）"
    }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $client = New-Object System.Net.WebClient
        $client.Headers.Add('User-Agent', 'ets2nav-ci')
        $client.DownloadFile($url, $zip)
        $client.Dispose()
    } catch {
        Fail-Asset "下载失败: $url`n  $($_.Exception.Message)"
    }
    $sw.Stop()
    $size = (Get-Item -LiteralPath $zip).Length
    Write-Log ("下载完成: {0:N1} MB，耗时 {1:N1}s" -f ($size / 1MB), $sw.Elapsed.TotalSeconds)
}

# ── 3/4. 摘要核对：不符立即失败，不继续 ─────────────────────────────────────
Write-Log ''
$actual = Get-Sha256Lower -Path $zip
Write-Log "actual     : sha256:$actual"
if ($actual -ne $Sha256) {
    Fail-Asset ("数据集摘要不符。`n" +
        "  url      : $url`n" +
        "  expected : sha256:$Sha256`n" +
        "  actual   : sha256:$actual`n" +
        "  已中止，未解压、未写入目标目录。")
}
Write-Log '摘要核对通过 ✓'

# ── 5. 解压到暂存目录 ───────────────────────────────────────────────────────
$staging = Join-Path $WorkDir 'staging'
if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
New-Item -ItemType Directory -Path $staging -Force | Out-Null

Write-Log ''
Write-Log "解压到暂存目录: $staging"
$sw = [System.Diagnostics.Stopwatch]::StartNew()
try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $staging)
} catch {
    Fail-Asset "解压失败: $($_.Exception.Message)"
}
$sw.Stop()
Write-Log ("解压完成，耗时 {0:N1}s" -f $sw.Elapsed.TotalSeconds)

# ── 6. 结构校验（先定位真正的数据集根） ─────────────────────────────────────
# 归档可能把内容放在根，也可能放在单层子目录里。两种都接受，但必须唯一确定，
# 且必须能在其中找到全部必需文件；否则视为结构不完整。
$root = $null
if (Test-Path -LiteralPath (Join-Path $staging 'routing.graph') -PathType Leaf) {
    $root = $staging
} else {
    $candidates = @(Get-ChildItem -LiteralPath $staging -Directory |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'routing.graph') -PathType Leaf })
    if ($candidates.Count -eq 1) {
        $root = $candidates[0].FullName
    } elseif ($candidates.Count -eq 0) {
        Fail-Asset "解压结果中找不到 routing.graph（既不在根，也不在唯一子目录）: $staging"
    } else {
        Fail-Asset "解压结果中存在多个候选数据集根（$($candidates.Count) 个），无法唯一确定: $staging"
    }
}
Write-Log "数据集根: $root"

$entries = @(Get-ChildItem -LiteralPath $root -File | Sort-Object Name)
Write-Log "归档内容: $($entries.Count) 个文件 — $(($entries | ForEach-Object { $_.Name }) -join ', ')"

$problems = @()
foreach ($name in $REQUIRED) {
    $p = Join-Path $root $name
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
        $problems += "缺少必需文件 $name"
    } elseif ((Get-Item -LiteralPath $p).Length -le 0) {
        $problems += "必需文件 $name 字节数为 0"
    }
}
if ($problems.Count -gt 0) {
    Fail-Asset ("数据集结构不完整：`n  " + ($problems -join "`n  ") + "`n  根目录: $root")
}
Write-Log '结构校验通过 ✓（5 个必需文件均存在且非空）'

$routingHash = Get-Sha256Lower -Path (Join-Path $root 'routing.graph')
Write-Log "routing.graph sha256: $routingHash"

# ── 7. 落到目标目录 ─────────────────────────────────────────────────────────
Write-Log ''
if (Test-Path -LiteralPath $Dest) { Remove-Item -LiteralPath $Dest -Recurse -Force }
$destParent = Split-Path -Parent $Dest
if ($destParent -and -not (Test-Path -LiteralPath $destParent)) {
    New-Item -ItemType Directory -Path $destParent -Force | Out-Null
}
Move-Item -LiteralPath $root -Destination $Dest

if (-not $KeepStaging -and (Test-Path -LiteralPath $staging)) {
    Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
}

$finalSize = (Get-ChildItem -LiteralPath $Dest -Recurse -File | Measure-Object Length -Sum).Sum
Write-Log ("数据集就绪: {0}" -f $Dest)
Write-Log ("  文件数 {0}，合计 {1:N1} MB" -f (Get-ChildItem -LiteralPath $Dest -File).Count, ($finalSize / 1MB))
Write-Log "  供 harness 使用的环境变量写法: ETS2NAV_DATASET=$Dest"
Write-Log ''
Write-Log "exit 0 (0=PASS 2=USAGE 3=EXTERNAL ASSET FAILURE)"
exit $EXIT_PASS
