#Requires -Version 5.1
<#
.SYNOPSIS
  B1–B6 候选身份核对（只读 oracle）。

.DESCRIPTION
  本脚本回答一个问题：**当前磁盘上的字节，是否就是 B1–B6 应当验证的那一组字节。**

  它只读、只算、只比较。不构建、不修改、不下载、不修复、不复制、不启动任何产品进程。
  期望值以字面常量写在本文件里，而不是从被核对的对象读取——否则产物改写自己的自述就能
  让核对通过。清单只用于**交叉核对**（产物自述 vs 本脚本独立复算），不作为期望值的来源。

  核对项：
    1. ZIP 的字节数与 SHA-256
    2. 解压树全树摘要、web 树摘要、dataset 树摘要（独立实现，不引用 BundleCommon.ps1）
    3. 两个可执行文件与两个插件 DLL 的 SHA-256 与字节数
    4. bundle-manifest.json 与 release-manifest.json 的逐项一致性
    5. map.pmtiles 与字形目录确实缺席（CORE profile）
    6. 可选：ETS2 的 bin\win_x64\plugins\ 中已安装 DLL 的 SHA-256（`-GamePluginsDir`）

  树摘要算法：相对路径（`/` 分隔）按序号序排序，逐行渲染 `<rel>\0<bytes>\0<sha256>\n`，
  UTF-8 编码后取 SHA-256。本文件重新实现该算法，因此与仓库实现的「一致」是证据而不是同义反复。

.PARAMETER Zip
  冻结的发布 ZIP。必填：身份判定的对象是归档本身。

.PARAMETER ExtractDir
  ZIP 的解压根，其中应含 `ETS2Nav/`。必填。

.PARAMETER ReleaseManifest
  外层发布清单 release-manifest.json。缺省则外层清单交叉核对**不执行**，整体退出码为 3——
  「没跑」不等于「通过」。

.PARAMETER GamePluginsDir
  ETS2 的 `bin\win_x64\plugins\` 目录。给出时核对已安装 DLL 的 SHA-256；缺省记为 NOT VERIFIED。

.EXAMPLE
  scripts\verify-b-candidate.ps1 `
    -Zip E:\ets2nav-rc-main\A\ETS2Nav-0.7.0-rc.1-windows-x64-core.zip `
    -ExtractDir E:\ets2nav-b-validation\candidate `
    -ReleaseManifest E:\ets2nav-rc-main\A\release-manifest.json `
    -GamePluginsDir "E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2\bin\win_x64\plugins"

.EXITCODES
  0 全部核对执行且通过；1 有核对不通过（或已安装插件身份不符）；3 前置条件缺失/必要核对未执行；4 harness 失败。

.NOTES
  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Zip,
    [Parameter(Mandatory)][string]$ExtractDir,
    [string]$ReleaseManifest,
    [string]$GamePluginsDir
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

# ── 冻结身份（字面常量，不以被核对对象为来源）──────────────────────────────────
$C = [ordered]@{
    Version        = '0.7.0-rc.1'
    Profile        = 'CORE'
    ArtifactName   = 'ETS2Nav-0.7.0-rc.1-windows-x64-core.zip'
    ZipBytes       = [int64]178091258
    ZipSha256      = '20a3307b4e54a620fa6e765d36c50ce68730b6d2de5e0e111f0305591fef4b3b'
    BundleTree     = '0f5dbde33d324b2ccde7a78bcf0d35df28dd374023be9479844c99ef94f709e5'
    BundleFiles    = [int64]25
    BundleBytes    = [int64]383033438
    WebTree        = 'a330040700167ffffe8aef95d69c09edd99cd5fe4eec7f3e3ea34522da109213'
    WebFiles       = [int64]10
    WebBytes       = [int64]2173922
    DatasetTree    = '3f30272dc992d08d6831374b513647a9970c4cc484525b65db706965f474d91b'
    DatasetFiles   = [int64]7
    DatasetBytes   = [int64]373674105
    DatasetFp      = 'fa3ef5bbafe6c406e60e90985dd5c7e8f0e9fa57392ac1888fba1f4bdf5e91c1'
    DatasetArchive = '15b03a6322bff6bd800f2ca1cb7b158ae843e937b6f3204fc438fb8c482831bb'
    SourceCommit   = 'c7e0c5227cdc3d378c188eff5ee19c90f22c98a3'
    DatasetTag     = 'dataset-europe-v5'
}

# 文件身份：相对解压根的路径 → @(sha256, bytes)
$CFile = [ordered]@{
    'ETS2Nav/ets2nav-desktop.exe'         = @('0b354c7b8ad45d659b95dee95f93f6ec1c2c1423d2b5f1717e74083f667773da', [int64]4072960)
    'ETS2Nav/nav-core-cli.exe'            = @('90332b5618fd2f59fcd47d3ebb3d3b37df7ffb56e20a8dee76b2fe7de66b42b7', [int64]2648576)
    'ETS2Nav/plugins/scs-nav-bridge.dll'  = @('bc9714779f6656955a350fc49c901722e4e0c8b75b3f108e059139e1172aa568', [int64]140288)
    'ETS2Nav/plugins/semaphore-bridge.dll' = @('171434e476666c90c63dee837f76f9812d80d846d71bb1b9751ac64a2d4e4452', [int64]139264)
}
# 插件在游戏目录中的安装名（与产物内同名）
$CPluginNames = @('scs-nav-bridge.dll', 'semaphore-bridge.dll')

$script:Fails = New-Object System.Collections.ArrayList
$script:CatFails = @{}
$script:Cat = 'MANIFEST'
$script:Checks = 0
$script:Skipped = New-Object System.Collections.ArrayList

function Stop-Precondition {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host "PRECONDITION FAILURE: $Message"
    exit 3
}
function Chk {
    param([string]$Name, $Actual, $Expect)
    $script:Checks++
    $a = if ($null -eq $Actual) { '<null>' } else { [string]$Actual }
    $e = if ($null -eq $Expect) { '<null>' } else { [string]$Expect }
    if ($a -ceq $e) {
        Write-Host "  [PASS] $Name - $a"
    } else {
        Write-Host "  [FAIL] $Name - actual=$a expected=$e"
        [void]$script:Fails.Add($Name)
        $script:CatFails[$script:Cat] = 1 + [int]$script:CatFails[$script:Cat]
    }
}
function Set-Cat {
    param([Parameter(Mandatory)][string]$Name)
    $script:Cat = $Name
}
function Skip {
    param([string]$Name, [string]$Reason)
    Write-Host "  [NOT VERIFIED] $Name - $Reason"
    [void]$script:Skipped.Add($Name)
}
function Get-Sha256Of {
    param([Parameter(Mandatory)][string]$Path)
    $s = [System.IO.File]::OpenRead($Path)
    try { $h = [System.Security.Cryptography.SHA256]::Create().ComputeHash($s) }
    finally { $s.Dispose() }
    return (($h | ForEach-Object { $_.ToString('x2') }) -join '')
}
function Get-DigestOfBytes {
    param([Parameter(Mandatory)][byte[]]$Bytes)
    $h = [System.Security.Cryptography.SHA256]::Create().ComputeHash($Bytes)
    return (($h | ForEach-Object { $_.ToString('x2') }) -join '')
}
# 独立实现的树摘要（与 BundleCommon.ps1 的 Get-TreeDigest 无共享代码）。
function Get-TreeIdentity {
    param([Parameter(Mandatory)][string]$Root)
    $full = [System.IO.Path]::GetFullPath($Root).TrimEnd('\')
    $recs = New-Object System.Collections.Generic.List[object]
    foreach ($f in [System.IO.Directory]::EnumerateFiles($full, '*', [System.IO.SearchOption]::AllDirectories)) {
        $rel = $f.Substring($full.Length + 1).Replace('\', '/')
        [void]$recs.Add([pscustomobject]@{
            Rel   = $rel
            Bytes = [int64](New-Object System.IO.FileInfo $f).Length
            Sha   = (Get-Sha256Of -Path $f)
        })
    }
    $rels = New-Object 'System.Collections.Generic.List[string]'
    foreach ($r in $recs) { [void]$rels.Add($r.Rel) }
    $rels.Sort([System.StringComparer]::Ordinal)
    $byRel = @{}
    foreach ($r in $recs) { $byRel[$r.Rel] = $r }
    $sb = New-Object System.Text.StringBuilder
    foreach ($k in $rels) {
        $r = $byRel[$k]
        [void]$sb.Append($r.Rel).Append([char]0).Append([string]$r.Bytes).Append([char]0).Append($r.Sha).Append("`n")
    }
    $total = [int64]0
    foreach ($r in $recs) { $total += $r.Bytes }
    return [pscustomobject]@{
        Digest = (Get-DigestOfBytes -Bytes ([System.Text.Encoding]::UTF8.GetBytes($sb.ToString())))
        Files  = [int64]$recs.Count
        Bytes  = $total
    }
}
function Read-Json {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json)
}

try {
    Write-Host '=== B1–B6 候选身份核对（只读）==='
    Write-Host "zip        : $Zip"
    Write-Host "extract    : $ExtractDir"
    Write-Host "manifest   : $(if ($ReleaseManifest) { $ReleaseManifest } else { '<未提供>' })"
    Write-Host "game plugs : $(if ($GamePluginsDir) { $GamePluginsDir } else { '<未提供>' })"
    Write-Host ''

    # ── 前置条件 ─────────────────────────────────────────────────────────────
    if (-not (Test-Path -LiteralPath $Zip -PathType Leaf)) { Stop-Precondition "ZIP 不存在: $Zip" }
    if (-not (Test-Path -LiteralPath $ExtractDir -PathType Container)) { Stop-Precondition "解压根不存在: $ExtractDir" }
    $zipFull = (Resolve-Path -LiteralPath $Zip).Path
    $extractFull = (Resolve-Path -LiteralPath $ExtractDir).Path
    $bundleRoot = Join-Path $extractFull 'ETS2Nav'
    if (-not (Test-Path -LiteralPath $bundleRoot -PathType Container)) {
        Stop-Precondition "解压根下缺少 ETS2Nav/: $extractFull"
    }

    # ── 1. ZIP ──────────────────────────────────────────────────────────────
    Set-Cat 'ZIP'
    Write-Host '-- ZIP --'
    Chk 'ZIP 文件名' (Split-Path -Leaf $zipFull) $C.ArtifactName
    Chk 'ZIP 字节数' ([int64](Get-Item -LiteralPath $zipFull).Length) $C.ZipBytes
    Chk 'ZIP SHA-256' (Get-Sha256Of -Path $zipFull) $C.ZipSha256

    # ── 2. 解压树 ────────────────────────────────────────────────────────────
    Write-Host ''
    Set-Cat 'TREE'
    Write-Host '-- 解压树（本脚本独立复算）--'
    $bt = Get-TreeIdentity -Root $bundleRoot
    Chk 'bundle 全树 SHA-256' $bt.Digest $C.BundleTree
    Chk 'bundle 文件数' $bt.Files $C.BundleFiles
    Chk 'bundle 字节数' $bt.Bytes $C.BundleBytes
    $wt = Get-TreeIdentity -Root (Join-Path $bundleRoot 'web')
    Chk 'web 树 SHA-256' $wt.Digest $C.WebTree
    Chk 'web 文件数' $wt.Files $C.WebFiles
    Chk 'web 字节数' $wt.Bytes $C.WebBytes
    $dt = $null
    $dsDir = Join-Path $bundleRoot 'data\europe-v5'
    if (-not (Test-Path -LiteralPath $dsDir -PathType Container)) {
        Chk 'dataset 目录存在' $false $true
    } else {
        $dt = Get-TreeIdentity -Root $dsDir
        Chk 'dataset 树 SHA-256' $dt.Digest $C.DatasetTree
        Chk 'dataset 文件数' $dt.Files $C.DatasetFiles
        Chk 'dataset 字节数' $dt.Bytes $C.DatasetBytes
    }

    # ── 3. 运行时二进制 ──────────────────────────────────────────────────────
    Write-Host ''
    Set-Cat 'BINARY'
    Write-Host '-- 运行时二进制 --'
    foreach ($rel in $CFile.Keys) {
        $p = Join-Path $extractFull ($rel -replace '/', '\')
        if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
            Chk "$rel 存在" $false $true
            continue
        }
        Chk "$rel SHA-256" (Get-Sha256Of -Path $p) $CFile[$rel][0]
        Chk "$rel 字节数" ([int64](Get-Item -LiteralPath $p).Length) $CFile[$rel][1]
    }

    # ── 4. 可选资源缺席 ──────────────────────────────────────────────────────
    Write-Host ''
    Set-Cat 'MANIFEST'
    Write-Host '-- 可选资源（CORE profile）--'
    Chk 'web/map.pmtiles 缺席' ([bool](Test-Path -LiteralPath (Join-Path $bundleRoot 'web\map.pmtiles'))) $false
    Chk 'web/vendor/fonts 缺席' ([bool](Test-Path -LiteralPath (Join-Path $bundleRoot 'web\vendor\fonts'))) $false
    Chk '解压树内无任何 *.pmtiles' (@(Get-ChildItem -LiteralPath $bundleRoot -Recurse -File -Filter '*.pmtiles').Count) 0

    # ── 5. 产物自述交叉核对 ──────────────────────────────────────────────────
    Write-Host ''
    Set-Cat 'MANIFEST'
    Write-Host '-- bundle-manifest.json（产物自述 vs 本脚本复算）--'
    $bmp = Join-Path $bundleRoot 'bundle-manifest.json'
    if (-not (Test-Path -LiteralPath $bmp -PathType Leaf)) {
        Chk 'bundle-manifest.json 存在' $false $true
    } else {
        $bm = Read-Json -Path $bmp
        Chk 'schema' ([string]$bm.schema) '2'
        Chk 'app_version' ([string]$bm.app_version) $C.Version
        Chk 'profile' ([string]$bm.profile) $C.Profile
        Chk 'source.commit' ([string]$bm.source.commit) $C.SourceCommit
        Chk 'source.dirty' ([bool]$bm.source.dirty) $false
        Chk 'basemap.present' ([bool]$bm.basemap.present) $false
        Chk 'fonts.present' ([bool]$bm.fonts.present) $false
        Set-Cat 'BINARY'
        Chk 'desktop_exe 与解压树一致' ([string]$bm.desktop_exe.sha256) $CFile['ETS2Nav/ets2nav-desktop.exe'][0]
        Chk 'sidecar 与解压树一致' ([string]$bm.sidecar.sha256) $CFile['ETS2Nav/nav-core-cli.exe'][0]
        Set-Cat 'TREE'
        Chk 'web.tree_sha256 与复算一致' ([string]$bm.web.tree_sha256) $wt.Digest
        if ($null -ne $dt) {
            Set-Cat 'DATASET'
            Chk 'dataset.tree_sha256 与复算一致' ([string]$bm.dataset.tree_sha256) $dt.Digest
        } else {
            Skip 'bundle-manifest.dataset.tree_sha256 核对' '解压树内没有 dataset 目录'
        }
    }

    if (-not $ReleaseManifest) {
        Skip 'release-manifest.json 交叉核对' '未提供 -ReleaseManifest（不能跳过，故整体退出码为 3）'
    } else {
        Write-Host ''
        Write-Host '-- release-manifest.json --'
        if (-not (Test-Path -LiteralPath $ReleaseManifest -PathType Leaf)) {
            Stop-Precondition "release-manifest.json 不存在: $ReleaseManifest"
        }
        $rm = Read-Json -Path $ReleaseManifest
        Chk 'rm.version' ([string]$rm.version) $C.Version
        Chk 'rm.profile' ([string]$rm.profile) $C.Profile
        Chk 'rm.source_commit' ([string]$rm.source_commit) $C.SourceCommit
        Chk 'rm.source_dirty' ([bool]$rm.source_dirty) $false
        Set-Cat 'ZIP'
        Chk 'rm.artifact_filename' ([string]$rm.artifact_filename) $C.ArtifactName
        Chk 'rm.artifact_bytes' ([int64]$rm.artifact_bytes) $C.ZipBytes
        Chk 'rm.artifact_sha256' ([string]$rm.artifact_sha256) $C.ZipSha256
        Set-Cat 'TREE'
        Chk 'rm.bundle_tree_sha256' ([string]$rm.bundle_tree_sha256) $bt.Digest
        Chk 'rm.bundle_files' ([int64]$rm.bundle_files) $bt.Files
        Chk 'rm.bundle_bytes' ([int64]$rm.bundle_bytes) $bt.Bytes
        Set-Cat 'BINARY'
        Chk 'rm.desktop_sha256' ([string]$rm.desktop_sha256) $CFile['ETS2Nav/ets2nav-desktop.exe'][0]
        Chk 'rm.sidecar_sha256' ([string]$rm.sidecar_sha256) $CFile['ETS2Nav/nav-core-cli.exe'][0]
        Set-Cat 'TREE'
        Chk 'rm.web_tree_sha256' ([string]$rm.web_tree_sha256) $wt.Digest
        Set-Cat 'MANIFEST'
        $mp = if ($null -eq $rm.map_pmtiles_sha256) { 'null' } else { [string]$rm.map_pmtiles_sha256 }
        $ft = if ($null -eq $rm.fonts_tree_sha256) { 'null' } else { [string]$rm.fonts_tree_sha256 }
        Chk 'rm.map_pmtiles_sha256 为 null' $mp 'null'
        Chk 'rm.fonts_tree_sha256 为 null' $ft 'null'
        Chk 'rm.plugins 数量' (@($rm.plugins).Count) 2
        Set-Cat 'BINARY'
        foreach ($pl in @($rm.plugins)) {
            $n = [string]$pl.name
            $key = "ETS2Nav/plugins/$n"
            if (-not $CFile.Contains($key)) {
                Chk "rm.plugins 含未知条目 $n" $false $true
                continue
            }
            Chk "rm.plugins[$n].sha256" ([string]$pl.sha256) $CFile[$key][0]
            Chk "rm.plugins[$n].bytes" ([int64]$pl.bytes) $CFile[$key][1]
        }
        Write-Host ''
        Set-Cat 'DATASET'
    Write-Host '-- dataset 身份 --'
        Chk 'rm.dataset.release_tag' ([string]$rm.dataset.release_tag) $C.DatasetTag
        if ($null -ne $dt) {
            Chk 'rm.dataset.tree_sha256' ([string]$rm.dataset.tree_sha256) $dt.Digest
            Chk 'rm.dataset.files' ([int64]$rm.dataset.files) $dt.Files
            Chk 'rm.dataset.bytes' ([int64]$rm.dataset.bytes) $dt.Bytes
        } else {
            Skip 'rm.dataset 树摘要核对' '解压树内没有 dataset 目录'
        }
        Chk 'rm.dataset.content_fingerprint' ([string]$rm.dataset.content_fingerprint) $C.DatasetFp
        Chk 'rm.dataset.archive_sha256' ([string]$rm.dataset.archive_sha256) $C.DatasetArchive
        $dsp = Join-Path $dsDir 'manifest.json'
        if (Test-Path -LiteralPath $dsp -PathType Leaf) {
            $dsm = Read-Json -Path $dsp
            Chk 'dataset manifest.json 的 content_fingerprint' ([string]$dsm.content_fingerprint) $C.DatasetFp
        } else {
            Chk 'dataset manifest.json 存在' $false $true
        }
        Chk 'rm.real_game_validation 陈述 B1–B6 未验证' ([bool]([string]$rm.real_game_validation -match '^REAL-GAME B1-B6 NOT YET VERIFIED')) $true
        $sv = [string]$rm.code_signing.verdict
        Chk 'rm.code_signing.verdict 已披露（UNSIGNED）' $sv 'UNSIGNED'
    }

    # ── 6. 已安装插件（可选）─────────────────────────────────────────────────
    Write-Host ''
    Set-Cat 'PLUGIN'
    Write-Host '-- 已安装插件（游戏 bin\win_x64\plugins\）--'
    $pluginVerdict = 'NOT VERIFIED'
    if (-not $GamePluginsDir) {
        Skip 'Installed plugin identity' '未提供 -GamePluginsDir；B1 启动游戏前必须核对'
    } elseif (-not (Test-Path -LiteralPath $GamePluginsDir -PathType Container)) {
        Skip 'Installed plugin identity' "目录不存在: $GamePluginsDir"
    } else {
        $pluginVerdict = 'PASS'
        foreach ($n in $CPluginNames) {
            $gp = Join-Path $GamePluginsDir $n
            if (-not (Test-Path -LiteralPath $gp -PathType Leaf)) {
                Write-Host "  [FAIL] 已安装 $n 存在 - 文件缺失: $gp"
                [void]$script:Fails.Add("installed:$n")
                $pluginVerdict = 'FAILED'
                continue
            }
            $exp = $CFile["ETS2Nav/plugins/$n"][0]
            $act = Get-Sha256Of -Path $gp
            if ($act -ceq $exp) {
                Write-Host "  [PASS] 已安装 $n SHA-256 - $act"
            } else {
                Write-Host "  [FAIL] 已安装 $n SHA-256 - actual=$act expected=$exp"
                [void]$script:Fails.Add("installed:$n")
                $pluginVerdict = 'FAILED'
            }
            Write-Host ("         （%ETS2NAV_RC_ROOT%\plugins\{0} 的字节数 = {1}）" -f $n, $CFile["ETS2Nav/plugins/$n"][1])
        }
    }

    # ── 汇总 ────────────────────────────────────────────────────────────────
    Write-Host ''
    Write-Host '=== 汇总 ==='
    function Get-CatVerdict {
        param([Parameter(Mandatory)][string]$Name)
        if ($script:CatFails.ContainsKey($Name) -and $script:CatFails[$Name] -gt 0) { return 'FAILED' }
        return 'PASS'
    }
    Write-Host ("核对项 {0} 个，未通过 {1} 个，未执行 {2} 个" -f $script:Checks, $script:Fails.Count, $script:Skipped.Count)
    foreach ($f in $script:Fails) { Write-Host "  未通过: $f" }
    foreach ($s in $script:Skipped) { Write-Host "  未执行: $s" }
    Write-Host ''
    Write-Host ("FINAL_RC_ZIP_IDENTITY=" + (Get-CatVerdict 'ZIP'))
    Write-Host ("FINAL_RC_EXTRACTED_TREE=" + (Get-CatVerdict 'TREE'))
    Write-Host ("RUNTIME_BINARY_IDENTITY=" + (Get-CatVerdict 'BINARY'))
    Write-Host ("DATASET_IDENTITY=" + (Get-CatVerdict 'DATASET'))
    Write-Host ("RELEASE_MANIFEST_CROSSCHECK=" + (Get-CatVerdict 'MANIFEST'))
    Write-Host "INSTALLED_PLUGIN_IDENTITY=$pluginVerdict"

    if ($script:Fails.Count -gt 0) {
        Write-Host 'PRE-B0 CANDIDATE GATE: NOT CLOSED'
        exit 1
    }
    if (-not $ReleaseManifest) {
        Write-Host 'PRE-B0 CANDIDATE GATE: NOT CLOSED（外层清单核对未执行）'
        exit 3
    }
    Write-Host 'PRE-B0 CANDIDATE GATE: PASS'
    exit 0
} catch {
    Write-Host ("HARNESS FAILURE: {0}" -f $_.Exception.Message)
    Write-Host $_.ScriptStackTrace
    exit 4
}
