#Requires -Version 5.1
<#
.SYNOPSIS
  从 bundle 暂存目录生成发布 ZIP（P4R 发布工程 §14、§17）。

.DESCRIPTION
  产物：`ETS2Nav-<version>-windows-x64-<profile>.zip`，`<profile>` 为清单 profile 的小写形态
  （core / full）。归档内部的单一顶层目录 `ETS2Nav/` 就是 **bundle 根**：

    ETS2Nav/
      ets2nav-desktop.exe      产品入口
      nav-core-cli.exe         sidecar
      bundle-manifest.json     身份
      LICENSE / THIRD_PARTY_NOTICES.txt
      plugins/                 两个遥测插件 DLL + README.txt
      data/europe-v5/          导航数据集
      web/                     前端产物

  为什么是这一层而不是更深一层：`desktop/src/bundle.rs::bundle_root()` 的实现是
  `std::env::current_exe()` 的父目录，`desktop/src/main.rs::resolve_bundle()` 直接用该目录
  解析 sidecar、数据集与前端。因此解压后的 `ETS2Nav/` 必须**正好**是 exe 所在的目录，
  多套一层会让 Desktop 把上层目录当根、找不到 `bundle-manifest.json` 而以打包错误退出。
  本布局已按此实现核对，不需要额外嵌套。

  归档可复现性（§17）由三件事保证，而不是「看起来一样」：
    1. 用 System.IO.Compression.ZipArchive 直接写条目。**不使用 Compress-Archive**——
       后者不提供条目顺序与时间戳控制，两次运行的字节序列可能不同。
    2. 条目按**序数序**（[StringComparer]::Ordinal）排序后依次写入；不写目录条目。
    3. 每个条目的 LastWriteTime 统一置为 -SourceDateEpoch 对应的时刻，压缩级别固定为
       Optimal。
  注意 .NET 把 LastWriteTime 存成 DOS 时间（2 秒分辨率、本地墙钟字段），因此对同一
  epoch，两次打包写入的时间字段逐字节相同——这正是 A/B 实验要判定的性质。

  plugins/README.txt 由本脚本生成。其内容依据仓库既有文档写成，而非臆测：
    * README.md §telemetry plugin（第 148–150 行）：两个 DLL 由 telemetry-plugin/*/build.bat
      用 MSVC 构建，SDK 头不随仓库分发，取回与摘要校验由 scripts/prepare-scs-sdk.ps1 承担。
    * docs/validation/p2-gameplay-test-checklist-2026-08.md §一：安装方法是把两个 DLL 复制到
      游戏的 `bin\win_x64\plugins\`（目录不存在则创建），并用 `nav-core-cli live` 验证就绪。
    * docs/validation/p4r-batch5-2026-09.md §13：唯一可用的用户二进制是 `out/*.dll`
      （历史文档里写的 `telemetry-plugin\<name>\<name>.dll` 路径是已知的过时写法）；
      同一节记录 semaphore 数组激活依赖游戏侧已安装的 `ets2la_plugin.dll`。

  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。

.PARAMETER StagingDir
  由 assemble-bundle.ps1 产出的暂存目录。

.PARAMETER ZipPath
  输出 ZIP 路径。A/B 可复现性实验需要两次打包写到两个不同路径，因此这里不做命名强制；
  当文件名与约定名不一致时打印 WARN（约定名由清单版本与 profile 推导）。

.PARAMETER SourceDateEpoch
  Unix 秒。默认 1767225600 = 2026-01-01T00:00:00Z，与归档条目时间戳的固定取值一致。

.PARAMETER RepoRoot
  仓库根目录，用于取 LICENSE / THIRD_PARTY_NOTICES.txt 与插件 DLL。缺省由脚本位置推导。

.EXITCODES
  0 打包完成；1 FAIL；3 前置条件（暂存目录/清单/许可文件/插件 DLL 缺失，或暂存目录含禁止内容）。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$StagingDir,
    [Parameter(Mandatory)][string]$ZipPath,
    [int64]$SourceDateEpoch = 1767225600,
    [string]$RepoRoot
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null

$script:ArtifactRoot = 'ETS2Nav'
if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }

# 复用仓库既有的身份/摘要实现（BundleCommon.ps1 的 Get-FileIdentity 等），
# 不在这里另写一份哈希逻辑：发布层与 bundle 层必须用同一套定义。
. (Join-Path $PSScriptRoot 'BundleCommon.ps1')

function Stop-Precondition {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host "PRECONDITION FAILURE: $Message"
    exit 3
}

function New-PluginsReadme {
    return @'
ETS2Nav 遥测插件安装说明
=======================

本目录中的两个 DLL 是 ETS2Nav 与 Euro Truck Simulator 2 之间的遥测桥接插件。它们不属于
导航程序本体：不安装时导航程序仍可启动与运行，只是拿不到实时车辆数据。

安装步骤
--------
1. 打开 ETS2 安装目录下的 bin\win_x64\plugins\ 子目录；该目录不存在时自行创建。
   典型形态为 <Steam 库目录>\steamapps\common\Euro Truck Simulator 2\bin\win_x64\plugins\。
2. 把本目录中的两个 DLL 复制进去，不要改名：
     scs-nav-bridge.dll      遥测桥接：位置、速度、档位等通道
     semaphore-bridge.dll    信号灯相位读取
3. 复制完成后，插件目录中应同时存在这两个文件。

就绪判据
--------
启动游戏后运行 nav-core-cli live：若持续输出遥测帧（而不是报告无法连接），即为就绪。

需要如实说明的限制
------------------
* 插件由游戏宿主进程加载，导航程序本身不加载它们，因此插件缺失不会被导航程序检测为错误。
* semaphore-bridge 读取信号灯数组依赖游戏侧已安装 ets2la_plugin.dll；该插件缺失时
  scs-nav-bridge 仍可工作，但信号灯相位不可用。
* 本目录中 DLL 的字节身份记录在发布层的 release-manifest.json（plugins 数组）中，
  可用 SHA-256 核对复制后的文件与发布件是否一致。
* 游戏版本更新后若插件不再被加载，需要用与游戏版本匹配的 SCS Telemetry SDK 重新构建
  telemetry-plugin 下的插件；SDK 不随本发布件分发。
'@
}

# ── 前置条件 ─────────────────────────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $StagingDir -PathType Container)) {
    Stop-Precondition "暂存目录不存在: $StagingDir"
}
$stagingFull = (Resolve-Path -LiteralPath $StagingDir).Path
$manifestPath = Join-Path $stagingFull 'bundle-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Stop-Precondition "暂存目录缺少 bundle-manifest.json: $stagingFull"
}
try {
    $bm = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
} catch {
    Stop-Precondition "bundle-manifest.json 不是合法 JSON: $($_.Exception.Message)"
}
if ($bm.schema -ne 2) { Stop-Precondition "bundle-manifest.json schema 必须是 2（实际 $($bm.schema)）" }
foreach ($k in @('app_version', 'profile', 'desktop_exe', 'sidecar', 'web', 'dataset', 'basemap', 'fonts')) {
    if ($null -eq $bm.PSObject.Properties[$k]) { Stop-Precondition "bundle-manifest.json 缺键 '$k'" }
}
$version = [string]$bm.app_version
$profileName = ([string]$bm.profile).ToLowerInvariant()
if ($profileName -ne 'core' -and $profileName -ne 'full') {
    Stop-Precondition "bundle-manifest.json profile 必须是 CORE 或 FULL（实际 '$($bm.profile)'）"
}

foreach ($rel in @('ets2nav-desktop.exe', 'nav-core-cli.exe')) {
    if (-not (Test-Path -LiteralPath (Join-Path $stagingFull $rel) -PathType Leaf)) {
        Stop-Precondition "暂存目录缺少 $rel"
    }
}
foreach ($rel in @('web', 'data/europe-v5')) {
    if (-not (Test-Path -LiteralPath (Join-Path $stagingFull ($rel -replace '/', '\')) -PathType Container)) {
        Stop-Precondition "暂存目录缺少 $rel/"
    }
}

# 许可文件：仓库根，缺失即前置条件失败（另一工作流负责生成 THIRD_PARTY_NOTICES.txt）
$licenseSrc = Join-Path $RepoRoot 'LICENSE'
$noticesSrc = Join-Path $RepoRoot 'THIRD_PARTY_NOTICES.txt'
foreach ($p in @($licenseSrc, $noticesSrc)) {
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
        Stop-Precondition "仓库根缺少 $(Split-Path -Leaf $p)（发布件必须随包分发许可与第三方声明）"
    }
    if ((Get-Item -LiteralPath $p).Length -le 0) {
        Stop-Precondition "$(Split-Path -Leaf $p) 是空文件"
    }
}

$plugins = @(
    [pscustomobject]@{ Name = 'scs-nav-bridge.dll';   Src = Join-Path $RepoRoot 'telemetry-plugin/scs-nav-bridge/out/scs-nav-bridge.dll' }
    [pscustomobject]@{ Name = 'semaphore-bridge.dll'; Src = Join-Path $RepoRoot 'telemetry-plugin/semaphore-bridge/out/semaphore-bridge.dll' }
)
foreach ($p in $plugins) {
    if (-not (Test-Path -LiteralPath $p.Src -PathType Leaf)) {
        Stop-Precondition "遥测插件 DLL 缺失: $($p.Src)"
    }
    if ((Get-Item -LiteralPath $p.Src).Length -le 0) { Stop-Precondition "遥测插件 DLL 为空: $($p.Src)" }
}

# ── 禁止内容（§14）────────────────────────────────────────────────────────────
# 这些路径形态一旦进入归档，就说明打包源头选错了，必须在写 ZIP 之前失败，
# 而不是「打进去再扫描」。
$forbiddenRegex = '(?i)(^|/)(target|node_modules|\.git|playwright-report|test-results)(/|$)|\.pdb$|(^|/)vendor/scs_sdk_1_14\.zip$'
$stagingFiles = New-Object System.Collections.ArrayList
foreach ($f in (Get-ChildItem -LiteralPath $stagingFull -Recurse -File -Force)) {
    [void]$stagingFiles.Add($f)
}
$forbidden = New-Object System.Collections.ArrayList
foreach ($f in $stagingFiles) {
    $rel = $f.FullName.Substring($stagingFull.Length).TrimStart('\', '/') -replace '\\', '/'
    if ($rel -match $forbiddenRegex) { [void]$forbidden.Add($rel) }
}
if ($forbidden.Count -gt 0) {
    Write-Host 'PRECONDITION FAILURE: 暂存目录含禁止内容（target/、node_modules/、.git/、*.pdb、playwright-report/、test-results/、vendor/scs_sdk_1_14.zip）：'
    $forbidden | Sort-Object | ForEach-Object { Write-Host "  - $_" }
    exit 3
}

# 暂存目录不得已经包含由本脚本提供的条目，否则会出现同名条目。
foreach ($rel in @('LICENSE', 'THIRD_PARTY_NOTICES.txt', 'plugins')) {
    if (Test-Path -LiteralPath (Join-Path $stagingFull $rel)) {
        Stop-Precondition "暂存目录已包含 $rel；该条目由 package-release.ps1 提供，不应出现在 assemble-bundle 的输出中"
    }
}

# ── 条目集合 ─────────────────────────────────────────────────────────────────
$entries = New-Object System.Collections.ArrayList
foreach ($f in $stagingFiles) {
    $rel = $f.FullName.Substring($stagingFull.Length).TrimStart('\', '/') -replace '\\', '/'
    [void]$entries.Add([pscustomobject]@{ Name = "$($script:ArtifactRoot)/$rel"; Kind = 'file'; Path = $f.FullName })
}
[void]$entries.Add([pscustomobject]@{ Name = "$($script:ArtifactRoot)/LICENSE"; Kind = 'file'; Path = $licenseSrc })
[void]$entries.Add([pscustomobject]@{ Name = "$($script:ArtifactRoot)/THIRD_PARTY_NOTICES.txt"; Kind = 'file'; Path = $noticesSrc })
[void]$entries.Add([pscustomobject]@{ Name = "$($script:ArtifactRoot)/plugins/README.txt"; Kind = 'text'; Path = $null })
foreach ($p in $plugins) {
    [void]$entries.Add([pscustomobject]@{ Name = "$($script:ArtifactRoot)/plugins/$($p.Name)"; Kind = 'file'; Path = $p.Src })
}

$names = @($entries | ForEach-Object { $_.Name })
$dup = @($names | Group-Object | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Name })
if ($dup.Count -gt 0) { Stop-Precondition "归档条目重名: $($dup -join ', ')" }
[Array]::Sort($names, [System.StringComparer]::Ordinal)

$byName = @{}
foreach ($e in $entries) { $byName[$e.Name] = $e }

# ── 写 ZIP ───────────────────────────────────────────────────────────────────
$zipFull = [System.IO.Path]::GetFullPath($ZipPath)
$zipDir = Split-Path -Parent $zipFull
if ($zipDir -and -not (Test-Path -LiteralPath $zipDir)) { $null = New-Item -ItemType Directory -Path $zipDir -Force }
if (Test-Path -LiteralPath $zipFull) { Remove-Item -LiteralPath $zipFull -Force }

$stampText = [DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch).UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ssZ')
Write-Host '=== 打包发布件（§14、§17）==='
Write-Host "staging  : $stagingFull"
Write-Host "zip      : $zipFull"
Write-Host "version  : $version  profile=$profileName"
Write-Host "条目     : $($names.Count) 个（序数序写入，无目录条目）"
Write-Host "时间戳   : $stampText（DOS 时间字段，2 秒分辨率）"
Write-Host ''
Write-Host '== 写入归档'
$stamp = [DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$fs = [System.IO.File]::Open($zipFull, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
    $archive = New-Object System.IO.Compression.ZipArchive($fs, [System.IO.Compression.ZipArchiveMode]::Create, $true)
    try {
        $readmeBytes = $null
        if ($names -contains "$($script:ArtifactRoot)/plugins/README.txt") {
            $readmeBytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes((New-PluginsReadme) + "`n")
        }
        foreach ($n in $names) {
            $e = $byName[$n]
            $entry = $archive.CreateEntry($n, [System.IO.Compression.CompressionLevel]::Optimal)
            # 先置时间戳再写内容：本地头在条目首次被写入时落盘，顺序反了会留下未规范化的字段。
            $entry.LastWriteTime = $stamp
            $out = $entry.Open()
            try {
                if ($e.Kind -eq 'text') {
                    $out.Write($readmeBytes, 0, $readmeBytes.Length)
                } else {
                    $in = [System.IO.File]::Open($e.Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
                    try { $in.CopyTo($out) } finally { $in.Dispose() }
                }
            } finally { $out.Dispose() }
        }
    } finally { $archive.Dispose() }
} finally { $fs.Dispose() }
$sw.Stop()

$zipId = Get-FileIdentity -Path $zipFull
$canonical = "ETS2Nav-$version-windows-x64-$profileName.zip"
$leaf = Split-Path -Leaf $zipFull
if ($leaf -ne $canonical) {
    Write-Host "WARN: 产物文件名 '$leaf' 与约定名 '$canonical' 不一致（-ZipPath 允许任意路径，约定名由版本与 profile 推导）"
}

# 归档自检：条目名集合与排序是本次写入的实际结果，不是意图。
$verifyNames = New-Object System.Collections.ArrayList
$zipRead = [System.IO.Compression.ZipFile]::OpenRead($zipFull)
try {
    foreach ($entry in $zipRead.Entries) { [void]$verifyNames.Add($entry.FullName) }
} finally { $zipRead.Dispose() }
$sortedOk = $true
for ($i = 1; $i -lt $verifyNames.Count; $i++) {
    if ([System.StringComparer]::Ordinal.Compare($verifyNames[$i - 1], $verifyNames[$i]) -ge 0) { $sortedOk = $false; break }
}
$setOk = (@($verifyNames | Sort-Object) -join "`n") -eq (@($names | Sort-Object) -join "`n")

Write-Host ''
Write-Host '=== 发布件已生成 ==='
Write-Host "artifact  : $leaf"
Write-Host "bytes     : $($zipId.Bytes)"
Write-Host "sha256    : $($zipId.Sha256)"
Write-Host "entries   : $($verifyNames.Count)"
Write-Host "序数有序  : $sortedOk"
Write-Host "条目集合  : $setOk"
Write-Host "wall      : $([math]::Round($sw.Elapsed.TotalSeconds, 1)) s"
Write-Host "ZIP_SHA256=$($zipId.Sha256)"
Write-Host "ZIP_BYTES=$($zipId.Bytes)"
Write-Host "ARTIFACT_FILENAME=$leaf"
Write-Host "ARTIFACT_CANONICAL=$canonical"
Write-Host "ARTIFACT_PROFILE=$profileName"
Write-Host "ARTIFACT_VERSION=$version"

if (-not $sortedOk -or -not $setOk) {
    Write-Host 'PACKAGE RELEASE: FAIL（归档条目顺序或集合与预期不符）'
    exit 1
}
Write-Host 'PACKAGE RELEASE: PASS'
exit 0
