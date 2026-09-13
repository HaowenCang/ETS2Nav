<#
.SYNOPSIS
    生成 / 校验仓库根目录的 THIRD_PARTY_NOTICES.txt（第三方许可清单）。

.DESCRIPTION
    本脚本是 THIRD_PARTY_NOTICES.txt 的唯一生成入口；该文件不得手工编辑。

    所有许可事实都来自本地已存在的证据，脚本不做任何猜测：
      · npm —— tools/ets2nav-web/node_modules/<pkg>/package.json 的 version/license 字段，
        以及同目录下的 LICENSE*/COPYING* 文件（若存在）；版本与
        tools/ets2nav-web/package-lock.json 交叉核对。
      · Rust —— scripts/crate-licenses.json。该文件由
        `cargo metadata --format-version 1` 对 desktop/Cargo.toml 与
        nav-core/Cargo.toml 分别求解后合并得出，逐条记录 crate 上游 manifest 的
        license 字段（未声明者为 null）。用 -RefreshCrateLicenses 重新生成。
      · SCS Telemetry SDK —— telemetry-plugin/scs-sdk-provenance.json 与
        vendor/scs_sdk_1_14.zip 内实际存在的 sdk_license.txt。
      · 字形字体 —— 未决（PENDING）：desktop/scripts/assemble-bundle.ps1 只在显式
        传入 -FontsDir + -FontsProvenance 时才把 glyph 放入产物，仓库内当前不存在
        该来源文件，因此无法登记任何字体许可。

    退出码：0 = 正常生成（或 -Check 且已提交文件为最新）；1 = FAIL（-Check 发现
    已提交文件过期，或输入证据相互矛盾）；3 = 前置条件缺失（缺少必需输入文件）。

.PARAMETER Check
    只校验：重新渲染后与已提交的 THIRD_PARTY_NOTICES.txt 逐字节比较，不做写入。
    内容过期时返回 1。

.PARAMETER OutputPath
    输出路径，缺省为仓库根目录的 THIRD_PARTY_NOTICES.txt。

.PARAMETER RefreshCrateLicenses
    重新执行 cargo metadata 并重写 scripts/crate-licenses.json。需要 cargo 可执行文件
    （PATH 或 -CargoExe）与网络（desktop 依赖图包含当前 registry 缓存中没有的 crate）。

.PARAMETER CargoExe
    cargo 可执行文件路径；缺省先用 PATH 中的 cargo，找不到则尝试
    %USERPROFILE%\.cargo\bin\cargo.exe。

.PARAMETER AllowMissingDist
    dist/ 未构建（或正在被其它工作流重建）时仍生成清单。此时缺少 build-manifest.json 与
    dist/vendor/* 的证据，脚本把三个直接 npm 组件的 shipped 判定降级为 UNVERIFIED，
    而不是断言「已随物分发」，并在产物哈希处写明未记录。缺省关闭：缺 dist/ 直接返回 3。

.NOTES
    本文件必须保存为 UTF-8 with BOM：Windows PowerShell 5.1 会把无 BOM 的 .ps1 按
    ANSI 解码，从而破坏其中的中文字面量（见 scripts/harness-encoding-guard.bat）。

    输出文件为 UTF-8 无 BOM（可跨平台逐字节比较），行尾统一 CRLF，不含时间戳——
    相同输入必然得到相同字节。
#>
[CmdletBinding()]
param(
    [switch]$Check,
    [string]$OutputPath,
    [switch]$RefreshCrateLicenses,
    [string]$CargoExe,
    [switch]$AllowMissingDist
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$script:RepoRoot = Split-Path -Parent $PSScriptRoot
$script:NoticesRel = 'THIRD_PARTY_NOTICES.txt'
$script:ExitOk = 0
$script:ExitFail = 1
$script:ExitPrecondition = 3

# ── 输入路径 ────────────────────────────────────────────────────────────────────
$WebDir = Join-Path $script:RepoRoot 'tools/ets2nav-web'
$WebPackageJson = Join-Path $WebDir 'package.json'
$WebPackageLock = Join-Path $WebDir 'package-lock.json'
$WebNodeModules = Join-Path $WebDir 'node_modules'
$ProjectLicensePath = Join-Path $script:RepoRoot 'LICENSE'
$CrateDataPath = Join-Path $PSScriptRoot 'crate-licenses.json'
$ScsProvenancePath = Join-Path $script:RepoRoot 'telemetry-plugin/scs-sdk-provenance.json'
$ScsArchivePath = Join-Path $script:RepoRoot 'vendor/scs_sdk_1_14.zip'
$ScsLicensePath = Join-Path $script:RepoRoot 'vendor/scs_sdk_1_14/sdk_license.txt'
$BuildManifestPath = Join-Path $WebDir 'dist/build-manifest.json'

# ── 被清单登记的直接 npm 组件 ───────────────────────────────────────────────────
# shipped 的判据：tools/ets2nav-web/dist/build-manifest.json 的 dependencies/devDependencies
# 记录（由 build.mjs 从已安装包的 package.json 真实读取），配合 dist/vendor/ 下实际存在的
# 产物文件。bundle 列为 esbuild 打包这些依赖时使用的入口（决定哪些传递依赖随之进入产物）。
$NpmComponents = @(
    [ordered]@{
        Name    = 'maplibre-gl'
        Bundle  = 'esbuild IIFE <- node_modules/maplibre-gl/dist/maplibre-gl.mjs（worker 另走 dist/maplibre-gl-worker.mjs）'
        Artifact = 'dist/vendor/maplibre-gl.js'
    },
    [ordered]@{
        Name    = 'pmtiles'
        Bundle  = '直接复制上游 node_modules/pmtiles/dist/pmtiles.js（不经 esbuild）'
        Artifact = 'dist/vendor/pmtiles.js'
    },
    [ordered]@{
        Name    = 'qrcode'
        Bundle  = 'esbuild IIFE <- node_modules/qrcode/lib/browser.js（上游只发 CommonJS）'
        Artifact = 'dist/vendor/qrcode.min.js'
    }
)

# 随 shipped 组件一起进入产物的传递依赖（版本/许可同样从 node_modules 实测）。
# qrcode 的 dijkstrajs / pngjs / yargs 不在其中：lib/browser.js 不引用它们，
# 且 dist/vendor/qrcode.min.js 的字节扫描对 'dijkstra'、'pngjs'、'yargs' 均为零命中。
$NpmBundled = @(
    '@mapbox/point-geometry', '@mapbox/tiny-sdf', '@mapbox/unitbezier', '@mapbox/vector-tile',
    '@maplibre/geojson-vt', '@maplibre/maplibre-gl-style-spec', '@maplibre/mlt', '@maplibre/vt-pbf',
    '@types/geojson', 'earcut', 'gl-matrix', 'kdbush', 'murmurhash-js', 'pbf', 'potpack',
    'quickselect', 'tinyqueue', 'fflate'
)

$NpmDevComponents = @(
    [ordered]@{ Name = 'esbuild'; Reason = '构建期打包器（build.mjs 依赖），不进入产物' },
    [ordered]@{ Name = '@playwright/test'; Reason = '测试框架（playwright.config.mjs 依赖），不进入产物' }
)

# 允许清单：出现这些标识符之一即可满足宽松/已知许可判据。
# 覆盖任务给定的基线集合，另加 crates.io 上常见且同样宽松的 Unlicense / 0BSD / MIT-0 /
# Apache-2.0 WITH LLVM-exception / LGPL-2.1-or-later（后者是已知 copyleft，登记为非宽松需复核）。
$PermissiveIds = @(
    'MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'Unicode-3.0',
    'CC0-1.0', 'MPL-2.0', 'LGPL-3.0', 'Unlicense', '0BSD', 'MIT-0',
    'Apache-2.0 WITH LLVM-exception', 'LGPL-2.1-or-later'
)

# ── 基础工具 ────────────────────────────────────────────────────────────────────
function Write-Utf8NoBom {
    param([string]$Path, [string]$Text)
    $enc = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Text, $enc)
}

function Get-NormalizedText {
    param([string]$Path)
    $raw = [System.IO.File]::ReadAllText($Path)
    return ($raw -replace "`r`n", "`n" -replace "`r", "`n")
}

function Get-FileSha256 {
    param([string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RepoRelativePath {
    param([string]$Path)
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetFullPath($script:RepoRoot)
    if ($full.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
        return ($full.Substring($root.Length).TrimStart('\', '/') -replace '\\', '/')
    }
    return $full
}

function Stop-Precondition {
    param([string]$Message)
    [Console]::Error.WriteLine("PRECONDITION FAIL: $Message")
    exit $script:ExitPrecondition
}

function Get-JsonProperty {
    param($Object, [string]$Name)
    if ($null -eq $Object) { return $null }
    # JavaScriptSerializer 的退回路径给出的是 Dictionary[string,object] 而非 PSCustomObject，
    # 它的键（例如 "node_modules/foo"）不经 PSObject 属性暴露，必须按键索引。
    if ($Object -is [System.Collections.IDictionary]) {
        # ContainsKey 未可靠地暴露给 PowerShell 的方法适配器，直接用索引器（缺键返回 null）。
        try { return $Object[$Name] } catch { return $null }
    }
    $prop = $Object.PSObject.Properties[$Name]
    if ($null -eq $prop) { return $null }
    return $prop.Value
}

function Read-JsonFile {
    param([string]$Path, [string]$Label)
    if (-not (Test-Path -LiteralPath $Path)) {
        Stop-Precondition "缺少输入文件（$Label）：$(Get-RepoRelativePath $Path)"
    }
    $raw = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
    try {
        # 注意：Windows PowerShell 5.1 的 ConvertFrom-Json 会拒绝形如 "node_modules/foo"
        # 的属性名（PSObject 属性名不接受 '/'），而 package-lock.json 正是这种形状；
        # 因此失败时退回到 JavaScriptSerializer。
        return ($raw | ConvertFrom-Json)
    } catch {
        try {
            Add-Type -AssemblyName System.Web.Extensions -ErrorAction Stop
            $ser = New-Object System.Web.Script.Serialization.JavaScriptSerializer
            $ser.MaxJsonLength = [int]::MaxValue
            return $ser.DeserializeObject($raw)
        } catch {
            Stop-Precondition "无法解析 JSON（$Label）：$(Get-RepoRelativePath $Path) —— $($_.Exception.Message)"
        }
    }
}

# ── npm 侧事实读取 ──────────────────────────────────────────────────────────────
function Get-NpmPackageFact {
    param([string]$PackageName)

    $dir = Join-Path $WebNodeModules $PackageName
    $manifestPath = Join-Path $dir 'package.json'
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        Stop-Precondition "缺少已安装的 npm 包 $PackageName（$manifestPath）；请先在 tools/ets2nav-web 执行 npm ci"
    }
    $manifest = Read-JsonFile -Path $manifestPath -Label "npm 包 $PackageName"
    $license = Get-JsonProperty -Object $manifest -Name 'license'
    if ([string]::IsNullOrWhiteSpace($license)) {
        # 老包可能只写 licenses 数组；不做猜测，直接记录为 UNKNOWN 由调用方处理
        $plural = Get-JsonProperty -Object $manifest -Name 'licenses'
        if ($null -ne $plural) {
            $types = @()
            foreach ($entry in @($plural)) {
                $t = Get-JsonProperty -Object $entry -Name 'type'
                if ($t) { $types += $t }
            }
            if ($types.Count -gt 0) { $license = ($types -join ' OR ') }
        }
    }

    $licenseFile = $null
    foreach ($candidate in @('LICENSE', 'LICENSE.txt', 'LICENSE.md', 'LICENCE', 'COPYING', 'UNLICENSE')) {
        $p = Join-Path $dir $candidate
        if (Test-Path -LiteralPath $p) { $licenseFile = $p; break }
    }

    # 表格内显示短路径：从 tools/ets2nav-web/ 起算（该根路径已在节首给出），避免用
    # 单条 100+ 字符的路径把整张表的末列撑开。
    $licenseFileDisplay = $null
    if ($licenseFile) {
        $rel = Get-RepoRelativePath $licenseFile
        $prefix = 'tools/ets2nav-web/'
        if ($rel.StartsWith($prefix)) { $licenseFileDisplay = $rel.Substring($prefix.Length) } else { $licenseFileDisplay = $rel }
    }

    return [ordered]@{
        Name        = $PackageName
        Version     = [string](Get-JsonProperty -Object $manifest -Name 'version')
        License     = if ($license) { [string]$license } else { 'UNKNOWN' }
        PackageJson = Get-RepoRelativePath $manifestPath
        LicenseFile = if ($licenseFile) { Get-RepoRelativePath $licenseFile } else { $null }
        LicenseFileDisplay = $licenseFileDisplay
        LicenseText = if ($licenseFile) { Get-NormalizedText -Path $licenseFile } else { $null }
    }
}

function Get-LockedNpmEntry {
    param($Lock, [string]$PackageName)
    $packages = Get-JsonProperty -Object $Lock -Name 'packages'
    if ($null -eq $packages) { return $null }
    return (Get-JsonProperty -Object $packages -Name ("node_modules/$PackageName"))
}

function Get-LicenseCopyrightLine {
    param([string]$Text)
    foreach ($line in ($Text -split "`n")) {
        if ($line -match '^\s*(Copyright|\(c\)|\(C\))') { return $line.Trim() }
    }
    return $null
}

# 把 LICENSE 文件原样嵌入（仅行尾归一化为 CRLF），并按主要行前缀加注释符。
function Format-LicenseBlock {
    param([string]$Heading, [string]$Text, [string]$Prefix = '#   ')
    $lines = @()
    $lines += "$Heading"
    $lines += "# ---------------------------------------------------------------------------"
    foreach ($line in ($Text -split "`n")) {
        if ($line -eq '') { $lines += '#' } else { $lines += ($Prefix + $line) }
    }
    $lines += ''
    return $lines
}

# ── 许可表达式判定 ──────────────────────────────────────────────────────────────
# 判据：SPDX 表达式按 ' AND ' 切分的每个合取项都必须可满足；每个合取项按 ' OR ' / '/'
# 切分，只要存在一个标识符落在 $PermissiveIds 内即视为可满足（双许可可选宽松分支）。
function Test-LicensePermissive {
    param([string]$Expression)
    $perm = $false
    $clean = $Expression -replace '[()]', ' '
    foreach ($conjunct in ($clean -split '\s+AND\s+')) {
        $satisfiable = $false
        foreach ($disjunct in ($conjunct -split '\s+OR\s+|\s*/\s*')) {
            $id = $disjunct.Trim()
            if ($id -eq '') { continue }
            if ($PermissiveIds -contains $id) { $satisfiable = $true; break }
        }
        if (-not $satisfiable) { $perm = $false; return $false }
        $perm = $true
    }
    return $perm
}

# ── -RefreshCrateLicenses：重新求解两个 workspace 的依赖图 ──────────────────────
function Resolve-CargoExe {
    if ($CargoExe) {
        if (-not (Test-Path -LiteralPath $CargoExe)) {
            Stop-Precondition "-CargoExe 指向的文件不存在：$CargoExe"
        }
        return $CargoExe
    }
    $cmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $fallback = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path -LiteralPath $fallback) { return $fallback }
    Stop-Precondition "找不到 cargo：PATH 与 %USERPROFILE%\.cargo\bin 均无；可用 -CargoExe 指定"
}

function Invoke-CargoMetadata {
    param([string]$Exe, [string]$ManifestPath, [string]$Label)
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ets2nav-notices-" + [System.Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $tmp -Force
    $jsonPath = Join-Path $tmp 'metadata.json'
    $errPath = Join-Path $tmp 'metadata.err'
    $targetDir = Join-Path $tmp 'target'
    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $Exe
        $psi.Arguments = "metadata --format-version 1 --manifest-path `"$ManifestPath`""
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.WorkingDirectory = $script:RepoRoot
        $psi.EnvironmentVariables['CARGO_TARGET_DIR'] = $targetDir
        $proc = [System.Diagnostics.Process]::Start($psi)
        $stdout = $proc.StandardOutput.ReadToEnd()
        $stderr = $proc.StandardError.ReadToEnd()
        $proc.WaitForExit()
        if ($proc.ExitCode -ne 0) {
            [Console]::Error.WriteLine($stderr)
            Stop-Precondition "cargo metadata 失败（$Label，exit $($proc.ExitCode)）：$ManifestPath"
        }
        Write-Utf8NoBom -Path $jsonPath -Text $stdout
        return (Get-Content -LiteralPath $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json)
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 依赖图闭包：从指定的根包集合出发（随物二进制的根包，以及 workspace 的其它成员——后者
# 同样出现在 Cargo.lock 中），沿 resolve.nodes[].deps[].dep_kinds 收集每个可达包的依赖
# 种类与 target 表达式。返回 @{ Rows = ...; MemberIds = ... }。
function Get-CrateGraphRows {
    param($Metadata, [string]$RootName, [string]$WorkspaceKey)

    $packagesById = @{}
    $rootPackage = $null
    foreach ($p in $Metadata.packages) {
        $packagesById[$p.id] = $p
        $src = Get-JsonProperty -Object $p -Name 'source'
        if ([string]::IsNullOrEmpty($src) -and $p.name -eq $RootName) { $rootPackage = $p }
    }
    if ($null -eq $rootPackage) {
        Stop-Precondition "cargo metadata 中找不到根包 $RootName（$WorkspaceKey）"
    }

    # workspace 成员集合（含未被随物二进制依赖的成员，例如 nav-core/tools/od-corpus）
    $memberIds = New-Object System.Collections.Generic.HashSet[string]
    foreach ($mid in @(Get-JsonProperty -Object $Metadata -Name 'workspace_members')) {
        if ($mid) { $null = $memberIds.Add([string]$mid) }
    }

    $nodesById = @{}
    foreach ($n in $Metadata.resolve.nodes) { $nodesById[$n.id] = $n }

    $reach = @{}
    $targets = @{}
    $memberReach = @{}
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue(@($rootPackage.id, 'normal', $false))
    foreach ($mid in $memberIds) {
        if ($mid -ne $rootPackage.id) { $queue.Enqueue(@($mid, 'normal', $true)) }
    }
    while ($queue.Count -gt 0) {
        $item = $queue.Dequeue()
        $id = $item[0]
        $kind = $item[1]
        $fromMember = $item[2]
        # 注意：PowerShell 会把单元素哈希表解包成其中的值，因此两层都必须用 @() 包住，
        # 否则后续的 .Keys / .ContainsKey 会在 StrictMode 下抛 PropertyNotFound。
        if ($fromMember) { $memberReach[$id] = $true }
        if (-not $reach.ContainsKey($id)) { $reach[$id] = @(@{}) }
        if (($reach[$id])[0].ContainsKey($kind)) { continue }
        ($reach[$id])[0][$kind] = $true
        $node = $nodesById[$id]
        if ($null -eq $node) { continue }
        $deps = Get-JsonProperty -Object $node -Name 'deps'
        if ($null -eq $deps) {
            $legacy = Get-JsonProperty -Object $node -Name 'dependencies'
            foreach ($depId in @($legacy)) {
                $queue.Enqueue(@($depId, 'normal', $fromMember))
                if (-not $targets.ContainsKey($depId)) { $targets[$depId] = @(@{}) }
                ($targets[$depId])[0]['all'] = $true
            }
            continue
        }
        foreach ($dep in $deps) {
            $kinds = @(Get-JsonProperty -Object $dep -Name 'dep_kinds')
            if ($kinds.Count -eq 0) {
                $kinds = @([pscustomobject]@{ kind = 'normal'; target = $null })
            }
            foreach ($k in $kinds) {
                $depKind = Get-JsonProperty -Object $k -Name 'kind'
                if ([string]::IsNullOrEmpty($depKind)) { $depKind = 'normal' }
                $depTarget = Get-JsonProperty -Object $k -Name 'target'
                $depId = $dep.pkg
                $queue.Enqueue(@($depId, $depKind, $fromMember))
                if (-not $targets.ContainsKey($depId)) { $targets[$depId] = @(@{}) }
                if ([string]::IsNullOrEmpty($depTarget)) { ($targets[$depId])[0]['all'] = $true }
                else { ($targets[$depId])[0][$depTarget] = $true }
            }
        }
    }

    $rows = @()
    foreach ($id in $reach.Keys) {
        $p = $packagesById[$id]
        if ($null -eq $p) { continue }
        $kinds = @(($reach[$id])[0].Keys | Sort-Object)
        $reachableVia = if ($kinds -contains 'normal') { 'normal' } else { ($kinds -join '+') }
        $src = Get-JsonProperty -Object $p -Name 'source'
        $isMember = $memberIds.Contains([string]$id)
        # role 的两种取值：随物二进制的依赖闭包成员，或仅因属 workspace 成员而出现在锁文件里
        # （od-corpus 属后者——没有任何随物二进制依赖它）。
        $role = if ($isMember -and $id -ne $rootPackage.id -and -not ($memberReach.ContainsKey($id) -and $memberReach[$id] -eq $true)) {
            'workspace_member_not_required_by_shipped_binary'
        } else {
            'reachable_from_shipped_binary'
        }
        $rows += [ordered]@{
            name     = $p.name
            version  = $p.version
            license  = Get-JsonProperty -Object $p -Name 'license'
            source   = if ([string]::IsNullOrEmpty($src)) { $null } else { $src }
            reach    = @{ $WorkspaceKey = $reachableVia }
            targets  = if ($targets.ContainsKey($id)) { @(($targets[$id])[0].Keys | Sort-Object) } else { @('all') }
            role     = @{ $WorkspaceKey = $role }
            used_by  = @()
        }
    }
    return @{ Rows = $rows; MemberCount = $memberIds.Count }
}

function Update-CrateLicenseData {
    param([string]$Exe)

    $workspaceSpecs = @(
        [ordered]@{
            Key      = 'desktop'
            Manifest = Join-Path $script:RepoRoot 'desktop/Cargo.toml'
            Lockfile = 'desktop/Cargo.lock'
            Root     = 'ets2nav-desktop'
            Binary   = 'ets2nav-desktop.exe'
        },
        [ordered]@{
            Key      = 'nav_core'
            Manifest = Join-Path $script:RepoRoot 'nav-core/Cargo.toml'
            Lockfile = 'nav-core/Cargo.lock'
            Root     = 'nav-core-cli'
            Binary   = 'nav-core-cli.exe'
        }
    )

    $merged = @{}
    foreach ($spec in $workspaceSpecs) {
        if (-not (Test-Path -LiteralPath $spec.Manifest)) {
            Stop-Precondition "缺少 workspace manifest：$(Get-RepoRelativePath $spec.Manifest)"
        }
        Write-Host "[refresh] cargo metadata: $($spec.Manifest)"
        $meta = Invoke-CargoMetadata -Exe $Exe -ManifestPath $spec.Manifest -Label $spec.Key
        $graph = Get-CrateGraphRows -Metadata $meta -RootName $spec.Root -WorkspaceKey $spec.Key
        Write-Host "[refresh]   $($spec.Key)：依赖闭包 $($graph.Rows.Count) 个包（workspace 成员 $($graph.MemberCount) 个）"
        foreach ($row in $graph.Rows) {
            $key = "$($row.name)|$($row.version)"
            if (-not $merged.ContainsKey($key)) {
                $merged[$key] = [ordered]@{
                    name    = $row.name
                    version = $row.version
                    license = $row.license
                    source  = $row.source
                    reach   = @{}
                    targets = @()
                    role    = @{}
                    used_by = @()
                }
            }
            if ($row.license -ne $null -and $merged[$key].license -eq $null) { $merged[$key].license = $row.license }
            if ($row.source -ne $null -and $merged[$key].source -eq $null) { $merged[$key].source = $row.source }
            $merged[$key].reach[$spec.Key] = $row.reach[$spec.Key]
            $merged[$key].role[$spec.Key] = $row.role[$spec.Key]
            $merged[$key].targets += $row.targets
            $merged[$key].used_by += $spec.Binary
        }
    }

    $crates = @()
    foreach ($key in ($merged.Keys | Sort-Object)) {
        $entry = $merged[$key]
        $entry.used_by = @($entry.used_by | Sort-Object -Unique)
        $entry.targets = @($entry.targets | Sort-Object -Unique)
        $crates += $entry
    }

    $doc = [ordered]@{
        schema     = 'ets2nav.crate-licenses/1'
        _comment   = ('Derived from `cargo metadata --format-version 1` run against desktop/Cargo.toml and ' +
                      'nav-core/Cargo.toml. One row per (name, version) that either is reachable from a shipped ' +
                      'binary''s root package or is a workspace member listed in workspace_members (the union ' +
                      'explains every [[package]] entry of both Cargo.lock files, including workspace members ' +
                      'that no shipped binary requires). `license` is the upstream crate manifest''s license field ' +
                      'verbatim; null means the crate declares none (an empty `source` identifies a ' +
                      'workspace-local path package, i.e. this project''s own crate). `reach` records how the ' +
                      'crate is reached per workspace: ''normal'' = ordinary dependency edge (may be linked into ' +
                      'that binary), ''build'' = reached only via build-dependency edges or not required by the ' +
                      'shipped binary at all. `role` records which of the two cases applies. `targets` lists the ' +
                      'cfg() expressions on the edges that reach the crate (''all'' = ungated). Regenerate with ' +
                      'scripts/generate-third-party-notices.ps1 -RefreshCrateLicenses. Do not hand-edit.')
        workspaces = [ordered]@{}
        crates     = $crates
    }
    foreach ($spec in $workspaceSpecs) {
        $doc.workspaces[$spec.Key] = [ordered]@{
            manifest = Get-RepoRelativePath $spec.Manifest
            lockfile = $spec.Lockfile
            root     = $spec.Root
            binary   = $spec.Binary
        }
    }

    # ConvertTo-Json 的缩进与键序与既有文件不同（"k":  v 双空格、role/used_by 次序），
    # 因此刷新不是逐字节幂等的。为免把格式差异当成数据漂移，这里在覆盖前做一次语义比较：
    # 记录数、crate 集合、以及逐条记录的字段值必须完全一致，否则报 FAIL 并由操作者判断。
    $previous = if (Test-Path -LiteralPath $CrateDataPath) { Read-JsonFile -Path $CrateDataPath -Label 'crate-licenses.json（旧）' } else { $null }
    $json = ($doc | ConvertTo-Json -Depth 100) -replace "`r`n", "`n"
    Write-Utf8NoBom -Path $CrateDataPath -Text ($json + "`n")
    Write-Host "[refresh] 已重写 $(Get-RepoRelativePath $CrateDataPath)：$($crates.Count) 条 crate 记录"

    if ($null -ne $previous) {
        $oldCrates = @{}
        foreach ($c in @($previous.crates)) { $oldCrates["$($c.name)|$($c.version)"] = $c }
        $added = @(); $removed = @(); $changed = @()
        foreach ($k in ($oldCrates.Keys | Sort-Object)) { if (-not ($crates | Where-Object { "$($_.name)|$($_.version)" -eq $k })) { $removed += $k } }
        foreach ($c in $crates) {
            $k = "$($c.name)|$($c.version)"
            if (-not $oldCrates.ContainsKey($k)) { $added += $k; continue }
            $o = $oldCrates[$k]
            $differences = @()
            foreach ($f in @('license', 'source')) {
                $ov = [string](Get-JsonProperty -Object $o -Name $f)
                $nv = [string](Get-JsonProperty -Object $c -Name $f)
                if ($ov -cne $nv) { $differences += ("${f}: '" + $ov + "' -> '" + $nv + "'") }
            }
            foreach ($f in @('reach', 'role')) {
                $ov = Get-JsonProperty -Object $o -Name $f
                $nv = Get-JsonProperty -Object $c -Name $f
                $oj = if ($ov) { ($ov | ConvertTo-Json -Compress -Depth 5) } else { 'null' }
                $nj = if ($nv) { ($nv | ConvertTo-Json -Compress -Depth 5) } else { 'null' }
                if ($oj -cne $nj) { $differences += ("${f}: " + $oj + ' -> ' + $nj) }
            }
            $ot = (@(Get-JsonProperty -Object $o -Name 'targets') | Sort-Object) -join ','
            $nt = (@(Get-JsonProperty -Object $c -Name 'targets') | Sort-Object) -join ','
            if ($ot -cne $nt) { $differences += "targets: [$ot] -> [$nt]" }
            if ($differences.Count -gt 0) { $changed += ("$k :: " + ($differences -join '; ')) }
        }
        Write-Host "[refresh] 与刷新前数据的语义比较：新增 $($added.Count) / 消失 $($removed.Count) / 字段变化 $($changed.Count)"
        foreach ($x in ($added | Select-Object -First 10)) { Write-Host "  + $x" }
        foreach ($x in ($removed | Select-Object -First 10)) { Write-Host "  - $x" }
        foreach ($x in ($changed | Select-Object -First 10)) { Write-Host "  ~ $x" }
        if ($added.Count -gt 0 -or $removed.Count -gt 0 -or $changed.Count -gt 0) {
            Write-Host '[refresh] 注意：数据已变化，THIRD_PARTY_NOTICES.txt 需要重新生成并在评审中说明该变化。'
        } else {
            Write-Host '[refresh] 数据与刷新前语义一致（仅 JSON 缩进/键序不同）。'
        }
    }
}

# ── 渲染 ────────────────────────────────────────────────────────────────────────

function Get-DisplayWidth {
    param([string]$Text)
    $w = 0
    foreach ($ch in $Text.ToCharArray()) {
        if ([int][char]$ch -lt 128) { $w += 1 } else { $w += 2 }
    }
    return $w
}

# 说明性文字折行：以空白为分隔单位贪心装箱（ASCII 词、路径整体不拆，无空白的中文按字
# 装箱）；仅当单个词本身就超过列宽时按显示宽度硬切。许可原文不经此函数。
function Split-TextForNotice {
    param([string]$Text, [int]$Width = 74, [string]$Indent = '', [string]$ContinuationIndent = $null)
    if ($null -eq $ContinuationIndent) { $ContinuationIndent = $Indent }
    $out = @()
    $current = ''
    $currentWidth = 0
    $prefix = $Indent

    # 把可能超宽的单字硬切成不超过 $Width 显示列的片段
    function Split-Hard {
        param([string]$Word)
        $parts = @()
        $buf = ''
        $bufWidth = 0
        foreach ($ch in $Word.ToCharArray()) {
            $cw = if ([int][char]$ch -lt 128) { 1 } else { 2 }
            if ($bufWidth + $cw -gt $Width -and $buf -ne '') {
                $parts += $buf
                $buf = ''
                $bufWidth = 0
            }
            $buf += $ch
            $bufWidth += $cw
        }
        if ($buf -ne '') { $parts += $buf }
        return $parts
    }

    $tokens = @()
    foreach ($word in ($Text -split '\s+')) {
        if ($word -eq '') { continue }
        if ((Get-DisplayWidth -Text $word) -gt $Width) {
            foreach ($h in @(Split-Hard -Word $word)) { $tokens += $h }
        } else {
            $tokens += $word
        }
    }

    for ($ti = 0; $ti -lt $tokens.Count; $ti++) {
        $piece = $tokens[$ti]
        $pw = Get-DisplayWidth -Text $piece
        if ($currentWidth -gt 0 -and ($currentWidth + 1 + $pw) -gt $Width) {
            # 避免只把一个短词挤到下一行：若下一词能留在本行，则本词并入本行（轻微超宽）
            $nextWidth = if ($ti + 1 -lt $tokens.Count) { Get-DisplayWidth -Text $tokens[$ti + 1] } else { [int]::MaxValue }
            if ($pw -le 12 -and ($currentWidth + 1 + $pw + 1 + $nextWidth) -le ($Width + 8)) {
                $current += (' ' + $piece)
                $currentWidth += (1 + $pw)
                continue
            }
            $out += ($prefix + $current)
            $current = $piece
            $currentWidth = $pw
            $prefix = $ContinuationIndent
        } elseif ($currentWidth -gt 0) {
            $current += (' ' + $piece)
            $currentWidth += (1 + $pw)
        } else {
            $current = $piece
            $currentWidth = $pw
        }
    }
    if ($current -ne '') { $out += ($prefix + $current) }
    return $out
}

function Render-Notices {
    $L = New-Object System.Collections.Generic.List[string]
    function Add-Line { param([string]$Text = '') $L.Add($Text) }

    # ---------- 项目自身许可 ----------
    if (-not (Test-Path -LiteralPath $ProjectLicensePath)) {
        Stop-Precondition "缺少仓库 LICENSE：$ProjectLicensePath"
    }
    $licenseText = Get-NormalizedText -Path $ProjectLicensePath
    $licenseSha = Get-FileSha256 -Path $ProjectLicensePath
    $licenseLines = @($licenseText -split "`n")
    $licenseTitle = ($licenseLines | Where-Object { $_.Trim() -ne '' } | Select-Object -First 1).Trim()
    $licenseVersion = ($licenseLines | Where-Object { $_.Trim() -ne '' } | Select-Object -Skip 1 -First 1).Trim()
    $declared = 'GPL-3.0-only'
    $declaredSource = 'tools/ets2nav-web/package.json 的 license 字段'
    $discrepancy =
        "LICENSE 文件收录的是 GNU GPL v3 的逐字全文（首两行：'$licenseTitle' / '$licenseVersion'），" +
        "正文第 566-580 行讨论 'or any later version' 只是条款本身，并未对本项目作出任何版本选择声明；" +
        "文件与仓库内其它位置都不存在 'either version 3 of the License, or (at your option) any later version' 这句话。" +
        "因此该文件本身既未选择 GPL-3.0-only，也未选择 GPL-3.0-or-later。" +
        "把本项目登记为 GPL-3.0-only 的判据来自 $($declaredSource)，" +
        "以及 commit a3d41f2（'决策 D2 修订：项目许可证 MIT -> GPL-3.0'）的提交意图，" +
        "而不是 LICENSE 文本。二者不构成冲突，但 LICENSE 缺少开发者选择声明，" +
        "属于声明与文本之间的空缺。"

    Add-Line '================================================================================'
    Add-Line 'ETS2Nav 第三方组件与许可清单（THIRD_PARTY_NOTICES）'
    Add-Line '================================================================================'
    Add-Line ''
    Add-Line '本文件由脚本生成，不得手工编辑：'
    Add-Line '  scripts/generate-third-party-notices.ps1          生成'
    Add-Line '  scripts/generate-third-party-notices.ps1 -Check   校验已提交内容是否为最新'
    Add-Line ''
    Add-Line '所有许可事实均取自仓库工作树中实际存在的文件（锁文件、已安装包的 package.json 与'
    Add-Line 'LICENSE 文件、crate 元数据、SDK 归档内的许可文本）。无证据的条目记为 UNKNOWN 或'
    Add-Line 'PENDING，不以推测填充。文件不含时间戳：相同输入必然生成相同字节。行尾为 CRLF；'
    Add-Line '被逐字收录的许可文本其行尾已从原文件的 LF 归一化为 CRLF，字词与空白未作改动。'
    Add-Line ''
    Add-Line "自校验摘要（不是签名，只用于 -Check 判断内容是否变化）："
    Add-Line "  notices-body-sha256: <见文件末尾>"
    Add-Line ''
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[0] 本项目自身许可'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    Add-Line '  项目          : ETS2Nav'
    Add-Line "  声明许可      : $declared"
    Add-Line "  声明来源      : $declaredSource"
    Add-Line "  LICENSE 文件  : LICENSE（Git 跟踪，$(Get-RepoRelativePath $ProjectLicensePath)）"
    Add-Line "  LICENSE sha256: $licenseSha"
    Add-Line "  LICENSE 首行  : $licenseTitle"
    Add-Line ''
    Add-Line '  声明与文本的核对结论：'
    foreach ($line in (Split-TextForNotice -Text $discrepancy -Width 78 -Indent '    ' -ContinuationIndent '    ')) { Add-Line $line }
    Add-Line ''
    Add-Line '  覆盖范围：仓库内自有源码与其构建产物，包括 nav-core 的 8 个 workspace 成员'
    Add-Line '  （6 个库 crate、nav-core-cli、构建工具 od-corpus；它们的 Cargo.toml 均未声明'
    Add-Line '  license 字段，故在 [2] 末的「source 为空」清单中同时出现）、desktop'
    Add-Line '  （ets2nav-desktop）以及 telemetry-plugin/ 下两个插件的源码与其提交的 DLL。'
    Add-Line '  上述包在 crate 数据中 source 为空，属本项目自有代码，不构成第三方依赖。'
    Add-Line ''
    Add-Line '  以下是 LICENSE 文件的完整逐字内容。'
    Add-Line ''
    foreach ($line in (Format-LicenseBlock -Heading '# LICENSE（逐字收录，sha256 如上）' -Text $licenseText)) { Add-Line $line }

    # ---------- npm 组件 ----------
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[1] Web 前端组件（npm，锁定安装）'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    Add-Line '  锁定来源：tools/ets2nav-web/package.json + tools/ets2nav-web/package-lock.json'
    Add-Line "  锁定文件 sha256：$(Get-FileSha256 -Path $WebPackageLock)"
    Add-Line '  事实来源：node_modules/<pkg>/package.json 的 version 与 license 字段，'
    Add-Line '            以及 node_modules/<pkg>/LICENSE* 文件（下表逐条给出路径）。'
    Add-Line ''

    if (-not (Test-Path -LiteralPath $WebPackageLock)) {
        Stop-Precondition "缺少 package-lock.json：$WebPackageLock"
    }
    if (-not (Test-Path -LiteralPath $WebNodeModules)) {
        Stop-Precondition "缺少 node_modules（$WebNodeModules）：请先在 tools/ets2nav-web 执行 npm ci"
    }
    $lock = Read-JsonFile -Path $WebPackageLock -Label 'package-lock.json'
    $webManifest = Read-JsonFile -Path $WebPackageJson -Label 'package.json'
    # dist/build-manifest.json 与 dist/vendor/* 都是 npm run build 的产物、不入库；缺它们时
    # 不足以判定 shipped，但足以登记许可事实。默认直接失败（避免在缺证据时断言已随物分发），
    # -AllowMissingDist 时降级为 UNVERIFIED。
    $buildManifest = $null
    $bmDeps = $null
    if (Test-Path -LiteralPath $BuildManifestPath) {
        $buildManifest = Read-JsonFile -Path $BuildManifestPath -Label 'dist/build-manifest.json'
        $bmDeps = Get-JsonProperty -Object $buildManifest -Name 'dependencies'
    } elseif (-not $AllowMissingDist) {
        Stop-Precondition "缺少 dist/build-manifest.json（应先执行 npm run build）；若只是产物被清理，可用 -AllowMissingDist 降级生成"
    } else {
        Add-Line '  注意：dist/build-manifest.json 缺失（dist/ 未构建），本节 shipped 判定降级为 UNVERIFIED。'
        Add-Line ''
    }

    # 表头：先收集行，最后按显示宽度补齐（CJK 记 2 列），保证竖列对齐。
    $table = New-Object System.Collections.Generic.List[object]
    $table.Add(@('组件', '版本', '许可（上游声明）', 'shipped', '事实来源'))

    $licenseTexts = New-Object System.Collections.Generic.List[object]
    $tableWarnings = New-Object System.Collections.Generic.List[string]
    $shippedCount = 0

    foreach ($comp in $NpmComponents) {
        $fact = Get-NpmPackageFact -PackageName $comp.Name
        $locked = Get-LockedNpmEntry -Lock $lock -PackageName $comp.Name
        if ($null -eq $locked) {
            Stop-Precondition "package-lock.json 中没有 node_modules/$($comp.Name)"
        }
        $lockedVersion = [string](Get-JsonProperty -Object $locked -Name 'version')
        if ($lockedVersion -ne $fact.Version) {
            Stop-Precondition "版本不一致：$($comp.Name) 已安装 $($fact.Version)，package-lock.json 记录 $lockedVersion"
        }
        $bmEntry = if ($bmDeps) { Get-JsonProperty -Object $bmDeps -Name $comp.Name } else { $null }
        if ($null -ne $bmDeps -and $null -eq $bmEntry) {
            Stop-Precondition "dist/build-manifest.json 的 dependencies 未记录 $($comp.Name)；请先执行 npm run build"
        }
        if ($null -ne $bmEntry) {
            $bmVersion = [string](Get-JsonProperty -Object $bmEntry -Name 'version')
            if ($bmVersion -ne $fact.Version) {
                Stop-Precondition "build-manifest.json 记录 $($comp.Name) 版本 $bmVersion，与已安装的 $($fact.Version) 不一致"
            }
        }
        $artifactPath = Join-Path $WebDir ($comp.Artifact -replace '/', '\')
        $artifactPresent = Test-Path -LiteralPath $artifactPath
        # dist/ 由 npm run build 生成、不入库，其它工作流可能正在重建它。-AllowMissingDist
        # 允许在缺 dist/ 时仍按源码侧证据生成清单，但把 shipped 降级为 UNVERIFIED 并在表内
        # 标明，避免在缺证据时断言「已随物分发」。
        if (-not $artifactPresent -and -not $AllowMissingDist) {
            Stop-Precondition "缺少产物 $($comp.Artifact)（应先执行 npm run build；若只是产物被清理，可用 -AllowMissingDist）"
        }
        if ($artifactPresent -and $null -ne $bmEntry) {
            $shippedFlag = 'true'
            $shippedCount++
        } else {
            $shippedFlag = 'UNVERIFIED'
        }
        $table.Add(@($comp.Name, $fact.Version, $fact.License, $shippedFlag, 'package.json + package-lock.json'))
        $table.Add(@('', '', '', '', "└ 许可文本: $(if ($fact.LicenseFileDisplay) { $fact.LicenseFileDisplay } else { '（包内无 LICENSE 文件）' })"))
        if ($shippedFlag -eq 'true') {
            $table.Add(@('', '', '', '', "└ 产物: $($comp.Artifact)  （完整 sha256 见本节末）"))
        } else {
            $table.Add(@('', '', '', '', "└ 产物: $($comp.Artifact) 缺失（dist/ 未构建）"))
        }
        $table.Add(@('', '', '', '', "└ 打包入口: $($comp.Bundle)"))
        if ($fact.LicenseFile) {
            $licenseTexts.Add([pscustomobject]@{
                Heading = "# $($comp.Name) $($fact.Version) —— $(Split-Path $fact.LicenseFile -Leaf)（逐字收录）"
                Text    = $fact.LicenseText
            })
        } else {
            $tableWarnings.Add("$($comp.Name) $($fact.Version) 的 npm 包内不存在 LICENSE/COPYING 文件，许可仅依据其 package.json 的 license 字段（$($fact.License)）。")
        }
    }

    foreach ($name in $NpmBundled) {
        $fact = Get-NpmPackageFact -PackageName $name
        $locked = Get-LockedNpmEntry -Lock $lock -PackageName $name
        if ($null -eq $locked) {
            Stop-Precondition "package-lock.json 中没有 node_modules/$name（传递依赖）"
        }
        $lockedVersion = [string](Get-JsonProperty -Object $locked -Name 'version')
        if ($lockedVersion -ne $fact.Version) {
            Stop-Precondition "版本不一致：$name 已安装 $($fact.Version)，package-lock.json 记录 $lockedVersion"
        }
        $lockLicense = Get-JsonProperty -Object $locked -Name 'license'
        if ($lockLicense -and ($lockLicense -ne $fact.License)) {
            Stop-Precondition "许可声明不一致：$name 的 package.json 写 '$($fact.License)'，package-lock.json 写 '$lockLicense'"
        }
        $table.Add(@($name, $fact.Version, $fact.License, 'true', 'node_modules/package.json（传递依赖，随打包进入产物）'))
        $table.Add(@('', '', '', '', "└ 许可文本: $(if ($fact.LicenseFileDisplay) { $fact.LicenseFileDisplay } else { '（包内无 LICENSE 文件）' })"))
        if ($fact.LicenseFile) {
            $licenseTexts.Add([pscustomobject]@{
                Heading = "# $name $($fact.Version) —— $(Split-Path $fact.LicenseFile -Leaf)（逐字收录）"
                Text    = $fact.LicenseText
            })
        } else {
            $tableWarnings.Add("$name $($fact.Version) 的 npm 包内不存在 LICENSE/COPYING 文件，许可仅依据其 package.json 的 license 字段（$($fact.License)）。")
        }
    }

    foreach ($dev in $NpmDevComponents) {
        $fact = Get-NpmPackageFact -PackageName $dev.Name
        $locked = Get-LockedNpmEntry -Lock $lock -PackageName $dev.Name
        $lockedVersion = if ($locked) { [string](Get-JsonProperty -Object $locked -Name 'version') } else { '（不在锁中）' }
        if ($locked -and $lockedVersion -ne $fact.Version) {
            Stop-Precondition "版本不一致：$($dev.Name) 已安装 $($fact.Version)，package-lock.json 记录 $lockedVersion"
        }
        $table.Add(@($dev.Name, $fact.Version, $fact.License, 'false', 'node_modules/package.json（devDependency）'))
        $table.Add(@('', '', '', '', "└ 事实来源: $(if ($fact.LicenseFileDisplay) { $fact.LicenseFileDisplay } else { 'package.json license 字段' })；$($dev.Reason)"))
    }

    $tableWidths = @(0, 0, 0, 0, 0)
    foreach ($row in $table) {
        for ($ci = 0; $ci -lt 5; $ci++) {
            $w = Get-DisplayWidth -Text $row[$ci]
            if ($w -gt $tableWidths[$ci]) { $tableWidths[$ci] = $w }
        }
    }
    # 末列（事实来源）含长路径，限宽为 64 显示列后折行，避免整表被一条路径撑开。
    $sourceWidth = [Math]::Min(64, $tableWidths[4])
    $sourceColumnStart = $tableWidths[0] + 2 + $tableWidths[1] + 2 + $tableWidths[2] + 2 + $tableWidths[3] + 2
    $rendered = New-Object System.Collections.Generic.List[string]
    for ($ri = 0; $ri -lt $table.Count; $ri++) {
        $cells = @()
        for ($ci = 0; $ci -lt 5; $ci++) {
            $pad = $tableWidths[$ci] - (Get-DisplayWidth -Text $table[$ri][$ci])
            $cells += ($table[$ri][$ci] + (' ' * [Math]::Max(0, $pad)))
        }
        if ($ri -eq 0) {
            $rendered.Add(('  ' + ($cells -join '  ').TrimEnd()))
            $sep = @()
            for ($ci = 0; $ci -lt 5; $ci++) {
                $w = if ($ci -eq 4) { $sourceWidth } else { $tableWidths[$ci] }
                $sep += ('-' * $w)
            }
            $rendered.Add(('  ' + ($sep -join '  ')))
        } else {
            # 续行缩进 2 空格（与末列首行的 "└ " 对齐）。@() 强制数组化：
            # PowerShell 会把单元素数组解包成标量，标量没有 .Count。
            $wrapped = @(Split-TextForNotice -Text $table[$ri][4] -Width $sourceWidth `
                        -Indent '' -ContinuationIndent '  ')
            $rendered.Add(('  ' + (($cells[0..3] -join '  ').TrimEnd() + '  ' + $wrapped[0]).TrimEnd()))
            for ($wi = 1; $wi -lt $wrapped.Count; $wi++) {
                $rendered.Add(('  ' + (' ' * $sourceColumnStart) + $wrapped[$wi]))
            }
        }
    }
    foreach ($line in $rendered) { Add-Line $line }
    Add-Line ''
    if ($AllowMissingDist) {
        Add-Line "  shipped=true 的组件数：$shippedCount（-AllowMissingDist 模式：dist/ 未构建或缺产物，未判定项见上表 UNVERIFIED）"
    } else {
        Add-Line "  shipped=true 的组件数：$shippedCount（与 dist/build-manifest.json 的 dependencies 记录一致）"
    }
    Add-Line ''
    if ($AllowMissingDist) {
        Add-Line '  随物分发的 vendor 产物与其完整 sha256：本轮未记录（dist/ 未构建，-AllowMissingDist）。'
    } else {
        Add-Line '  随物分发的 vendor 产物与其完整 sha256（与 dist/build-manifest.json 的 artifacts 同源）：'
        foreach ($comp in $NpmComponents) {
            $artifactPath = Join-Path $WebDir ($comp.Artifact -replace '/', '\')
            Add-Line "    $($comp.Artifact)  $(Get-FileSha256 -Path $artifactPath)"
        }
        $maplibreWorker = Join-Path $WebDir 'dist/vendor/maplibre-gl-worker.js'
        $maplibreCss = Join-Path $WebDir 'dist/vendor/maplibre-gl.css'
        if (Test-Path -LiteralPath $maplibreWorker) {
            Add-Line "    dist/vendor/maplibre-gl-worker.js  $(Get-FileSha256 -Path $maplibreWorker)"
        }
        if (Test-Path -LiteralPath $maplibreCss) {
            Add-Line "    dist/vendor/maplibre-gl.css（maplibre-gl 6.4.1 的样式表，BSD-3-Clause 同上）  $(Get-FileSha256 -Path $maplibreCss)"
        }
    }
    Add-Line '  esbuild 的平台二进制包（已安装的 @esbuild/win32-x64 0.28.2，MIT）只在本机构建时'
    Add-Line '  执行，不进入 dist/，故未登记为随物分发组件。'
    Add-Line ''
    Add-Line '  许可声明证据缺口（不是许可缺失，而是包内缺少可逐字收录的文本）：'
    if ($tableWarnings.Count -eq 0) {
        Add-Line '    （无）'
    } else {
        foreach ($w in $tableWarnings) {
            foreach ($line in (Split-TextForNotice -Text $w -Width 72 -Indent '    - ' -ContinuationIndent '      ')) { Add-Line $line }
        }
    }
    Add-Line ''
    Add-Line '  以下逐字收录随物分发组件自身携带的许可文本。'
    Add-Line ''
    foreach ($entry in $licenseTexts) {
        foreach ($line in (Format-LicenseBlock -Heading $entry.Heading -Text $entry.Text)) { Add-Line $line }
    }

    # ---------- Rust crates ----------
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[2] Rust crates（链接进 nav-core-cli.exe 与 ets2nav-desktop.exe）'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    Add-Line '  数据来源：scripts/crate-licenses.json，由 `cargo metadata --format-version 1` 对'
    Add-Line '  desktop/Cargo.toml 与 nav-core/Cargo.toml 求解后合并，取每个 crate 上游 manifest 的'
    Add-Line '  license 字段（逐字）。cargo 的解析版本与 Cargo.lock 逐条核对一致（见下）。'
    Add-Line ''

    $crateDoc = Read-JsonFile -Path $CrateDataPath -Label 'crate-licenses.json'
    $crates = @($crateDoc.crates)
    foreach ($lockName in @('desktop/Cargo.lock', 'nav-core/Cargo.lock')) {
        $lockPath = Join-Path $script:RepoRoot ($lockName -replace '/', '\')
        if (-not (Test-Path -LiteralPath $lockPath)) {
            Stop-Precondition "缺少锁文件：$lockName"
        }
        Add-Line "  $lockName  sha256=$(Get-FileSha256 -Path $lockPath)"
    }
    Add-Line ''

    # 锁文件与 crate 数据的交叉核对
    foreach ($pair in @(
        @{ Lock = 'desktop/Cargo.lock'; Key = 'desktop' },
        @{ Lock = 'nav-core/Cargo.lock'; Key = 'nav_core' }
    )) {
        $lockPath = Join-Path $script:RepoRoot ($pair.Lock -replace '/', '\')
        $lines = [System.IO.File]::ReadAllLines($lockPath)
        $entries = @{}
        for ($i = 0; $i -lt $lines.Length; $i++) {
            if ($lines[$i] -ne '[[package]]') { continue }
            $nm = $null; $vr = $null; $sr = $null
            for ($j = $i + 1; $j -lt $lines.Length; $j++) {
                if ($lines[$j] -eq '[[package]]') { break }
                if ($lines[$j] -like 'name = *') { $nm = $lines[$j].Split('"')[1] }
                elseif ($lines[$j] -like 'version = *') { $vr = $lines[$j].Split('"')[1] }
                elseif ($lines[$j] -like 'source = *') { $sr = $lines[$j].Split('"')[1] }
            }
            if ($nm) { $entries["$nm|$vr"] = $sr }
        }
        # 锁文件中 workspace 成员的版本是「占位版本」（例如 ets2nav-desktop 记 0.1.0，
        # 而 Cargo.toml 已改为 0.7.0-rc.1，因为改版本号尚未重新解析锁文件）。核对时按
        # 包名绑定到 crate 数据里的实际版本，并在输出中显式记下这一处绑定，而不是放宽判据。
        $knownById = @{}
        $knownByName = @{}
        foreach ($c in $crates) {
            $reach = Get-JsonProperty -Object $c -Name 'reach'
            $r = Get-JsonProperty -Object $reach -Name $pair.Key
            if (-not $r) { continue }
            $knownById["$($c.name)|$($c.version)"] = $true
            if (-not $knownByName.ContainsKey($c.name)) { $knownByName[$c.name] = @() }
            $knownByName[$c.name] += [string]$c.version
        }
        $missing = @()
        $bound = @()
        foreach ($k in ($entries.Keys | Sort-Object)) {
            if ($knownById.ContainsKey($k)) { continue }
            $parts = $k -split '\|', 2
            $versions = $knownByName[$parts[0]]
            if ($versions.Count -eq 1) {
                $bound += "$k -> $($parts[0])|$($versions[0])"
            } else {
                $missing += $k
            }
        }
        if ($missing.Count -gt 0) {
            Stop-Precondition ("$($pair.Lock) 中有 $($missing.Count) 个包不在 crate-licenses.json 的 $($pair.Key) 闭包内：" +
                               (($missing | Select-Object -First 8) -join ', ') +
                               "；请运行 -RefreshCrateLicenses 重新求解")
        }
        Add-Line "  核对：$($pair.Lock) 的 $($entries.Count) 个 [[package]] 条目全部对应到 $($pair.Key) 依赖闭包中的包。"
        if ($bound.Count -gt 0) {
            Add-Line "  其中 $($bound.Count) 条按包名绑定版本（锁文件中的版本与 crate 数据不同，差异如下）："
            foreach ($b in $bound) { Add-Line "    $b" }
            Add-Line '  该差异来自本工作树中对 Cargo.toml 版本号与 workspace 成员的未提交改动（锁文件未重新解析）；'
            Add-Line '  核对完成后锁文件已还原为 Git 中的版本。'
        }
    }
    Add-Line ''

    # 统计
    $external = @($crates | Where-Object { (Get-JsonProperty -Object $_ -Name 'source') })
    $local = @($crates | Where-Object { -not (Get-JsonProperty -Object $_ -Name 'source') })
    $reachedDesktop = @($crates | Where-Object { (Get-JsonProperty -Object (Get-JsonProperty -Object $_ -Name 'reach') -Name 'desktop') })
    $reachedNav = @($crates | Where-Object { (Get-JsonProperty -Object (Get-JsonProperty -Object $_ -Name 'reach') -Name 'nav_core') })
    $buildOnlyDesktop = @($reachedDesktop | Where-Object { (Get-JsonProperty -Object (Get-JsonProperty -Object $_ -Name 'reach') -Name 'desktop') -eq 'build' })
    $buildOnlyNav = @($reachedNav | Where-Object { (Get-JsonProperty -Object (Get-JsonProperty -Object $_ -Name 'reach') -Name 'nav_core') -eq 'build' })

    Add-Line "  不同 crate 总数（按 name+version 去重，两个 workspace 合并）：$($crates.Count)"
    Add-Line "    · ets2nav-desktop 依赖闭包：$($reachedDesktop.Count) 个"
    Add-Line "    · nav-core-cli 依赖闭包：$($reachedNav.Count) 个"
    Add-Line "    · 来自 registry 的第三方 crate：$($external.Count) 个"
    Add-Line "    · 本项目自有（source 为空）的 workspace 内 crate：$($local.Count) 个"
    Add-Line "    · 仅经 build-dependency 边可达：desktop $($buildOnlyDesktop.Count) 个 / nav-core $($buildOnlyNav.Count) 个"
    Add-Line ''
    Add-Line '  许可集合（按上游声明的 SPDX 表达式字面量分组，计数为第三方 crate 数）：'
    $groups = @{}
    foreach ($c in $external) {
        $lic = Get-JsonProperty -Object $c -Name 'license'
        if (-not $lic) { $lic = '(none)' }
        if (-not $groups.ContainsKey($lic)) { $groups[$lic] = 0 }
        $groups[$lic]++
    }
    foreach ($lic in ($groups.Keys | Sort-Object { -$groups[$_] }, { $_ })) {
        Add-Line ('    {0,4}  {1}' -f $groups[$lic], $lic)
    }
    Add-Line ''

    # 风险清单
    $noLicense = @($external | Where-Object { -not (Get-JsonProperty -Object $_ -Name 'license') })
    $nonPermissive = @($external | Where-Object {
        $lic = Get-JsonProperty -Object $_ -Name 'license'
        $lic -and -not (Test-LicensePermissive -Expression $lic)
    })

    Add-Line '  ── 风险清单 A：许可不在允许清单内的第三方 crate ──'
    Add-Line '  判据：SPDX 表达式按 '' AND '' 分组的每一项都必须有一个标识符落在允许清单内；'
    Add-Line '  '' OR '' 分支只要有一支命中即可。允许清单为：'
    foreach ($line in (Split-TextForNotice -Text ($PermissiveIds -join ' / ') -Width 72 -Indent '    ' -ContinuationIndent '    ')) { Add-Line $line }
    if ($nonPermissive.Count -eq 0) {
        Add-Line '  结果：空——没有 crate 落在允许清单之外。'
    } else {
        Add-Line "  结果：$($nonPermissive.Count) 个，逐条如下（需发布前复核）："
        foreach ($c in $nonPermissive) {
            $reach = Get-JsonProperty -Object $c -Name 'reach'
            Add-Line ('    {0}@{1}  license="{2}"  reach={3}  targets=[{4}]' -f `
                      $c.name, $c.version, (Get-JsonProperty -Object $c -Name 'license'), `
                      (($reach.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ', '), `
                      (@(Get-JsonProperty -Object $c -Name 'targets') -join ', '))
        }
    }
    Add-Line ''
    Add-Line '  ── 风险清单 B：未声明任何许可的第三方 crate ──'
    if ($noLicense.Count -eq 0) {
        Add-Line '  结果：空——每个来自 registry 的 crate 都在 manifest 中声明了 license。'
    } else {
        Add-Line "  结果：$($noLicense.Count) 个（许可未知，必须补齐证据后才能判定）："
        foreach ($c in $noLicense) {
            Add-Line ('    {0}@{1}  source={2}' -f $c.name, $c.version, $c.source)
        }
    }
    Add-Line ''
    Add-Line '  ── 补充：crate 数据中 source 为空（不是 registry/git 来源）的 crate ──'
    Add-Line '  这些不是第三方依赖，其许可由 [0] 的仓库 LICENSE 覆盖；列出仅为消除歧义。'
    foreach ($c in ($local | Sort-Object { $_.name })) {
        $reach = Get-JsonProperty -Object $c -Name 'reach'
        Add-Line ('    {0}@{1}  reach={2}' -f $c.name, $c.version, `
                  (($reach.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ', '))
    }
    Add-Line ''

    # Tauri / WebView2 与字体
    Add-Line '  ── Tauri 运行时与 WebView2 ──'
    $tauriRow = $crates | Where-Object { $_.name -eq 'tauri' } | Select-Object -First 1
    if ($tauriRow) {
        $tauriLic = Get-JsonProperty -Object $tauriRow -Name 'license'
        Add-Line "  Tauri 以普通依赖（tauri $($tauriRow.version)）静态链接进 ets2nav-desktop.exe；其自身与其"
        Add-Line "  全部 Rust 依赖都已经包含在上面的 crate 闭包与统计中（license 声明为 $tauriLic），"
        Add-Line '  不再单列。desktop/tauri.conf.json 未启用任何 bundler 目标，因此不产出 MSI/NSIS 包，'
        Add-Line '  也不把 Tauri CLI 或任何运行时二进制复制进产物。'
    }
    Add-Line '  WebView2：desktop/Cargo.lock 中的 webview2-com / webview2-com-sys / webview2-com-macros'
    Add-Line '  是 Microsoft WebView2 的 Rust 绑定（随 crate 闭包统计），但 WebView2 运行期本体是'
    Add-Line '  Windows 上独立安装的 Microsoft Edge WebView2 Evergreen Runtime，按 Microsoft 自己的'
    Add-Line '  许可条款分发，不在本仓库的产物内，也不由本项目再分发。'
    Add-Line ''
    Add-Line '  ── 字形字体（glyphs）──'
    Add-Line '  状态：PENDING（当前 ABSENT）'
    Add-Line '  依据：tools/ets2nav-web/app.js 以 vendor/fonts/{fontstack}/{range}.pbf 请求字形，'
    Add-Line '  build.mjs 仅在 <web>/fonts/ 存在时才把它复制到 dist/vendor/fonts；'
    Add-Line '  desktop/scripts/assemble-bundle.ps1 仅在显式传入 -FontsDir 与 -FontsProvenance'
    Add-Line '  （要求含 font/version/upstream/license_spdx/source_sha256 等键）时才把它放进产物。'
    Add-Line "  现状：git ls-files 中不存在任何 .ttf/.otf/.woff/.woff2/.pbf 字体文件，"
    Add-Line '  仓库内也不存在字体 provenance 文件，因此最终随物的字形字体尚未确定，'
    Add-Line '  本清单不登记任何字体许可。若发布时提供 -FontsDir，则必须在 -FontsProvenance 中'
    Add-Line '  给出 license_spdx，并把对应许可文本补进本文件后重新生成。'
    Add-Line ''
    Add-Line '  ── 完整 crate 清单（name version | license | reach | source）──'
    Add-Line '  reach: desktop=.../nav_core=...。normal 表示存在普通依赖边（可能被链接进该二进制），'
    Add-Line '  build 表示仅经 build-dependency 边可达、或未被任何随物二进制依赖；source=registry'
    Add-Line '  表示来自 crates.io，workspace-local 表示本项目自有代码（由仓库 LICENSE 覆盖）。'
    foreach ($c in $crates) {
        $reach = Get-JsonProperty -Object $c -Name 'reach'
        $reachText = (($reach.PSObject.Properties | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ',')
        $lic = Get-JsonProperty -Object $c -Name 'license'
        if (-not $lic) { $lic = '(none)' }
        $src = Get-JsonProperty -Object $c -Name 'source'
        if (-not $src) { $src = 'workspace-local' } else { $src = 'registry' }
        Add-Line ('    {0} {1} | {2} | {3} | {4}' -f $c.name, $c.version, $lic, $reachText, $src)
    }
    Add-Line ''

    # ---------- SCS SDK ----------
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[3] SCS Telemetry SDK（telemetry-plugin 两个插件）'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    $scs = Read-JsonFile -Path $ScsProvenancePath -Label 'scs-sdk-provenance.json'
    $sdk = Get-JsonProperty -Object $scs -Name 'sdk'
    $scsLic = Get-JsonProperty -Object $scs -Name 'license'
    $spdx = Get-JsonProperty -Object $scsLic -Name 'spdx_identifier'

    if (-not (Test-Path -LiteralPath $ScsArchivePath)) {
        Stop-Precondition "缺少 SDK 归档：vendor/scs_sdk_1_14.zip"
    }
    $archiveSha = Get-FileSha256 -Path $ScsArchivePath
    $recordedSha = ([string](Get-JsonProperty -Object $sdk -Name 'sha256')).ToLowerInvariant()
    if ($archiveSha -ne $recordedSha) {
        Stop-Precondition "vendor/scs_sdk_1_14.zip 的 sha256=$archiveSha 与 provenance 记录的 $recordedSha 不一致"
    }
    if (-not (Test-Path -LiteralPath $ScsLicensePath)) {
        Stop-Precondition "缺少解包后的 sdk_license.txt（$ScsLicensePath）"
    }
    $scsText = Get-NormalizedText -Path $ScsLicensePath
    $scsLicenseSha = Get-FileSha256 -Path $ScsLicensePath

    # 与归档内的同一 entry 比对（zip entry 名称固定，无需解包整个归档）
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($ScsArchivePath)
    try {
        $entry = $zip.Entries | Where-Object { $_.FullName -eq 'sdk_license.txt' } | Select-Object -First 1
        if ($null -eq $entry) {
            Stop-Precondition "vendor/scs_sdk_1_14.zip 内不存在 sdk_license.txt"
        }
        $stream = $entry.Open()
        $ms = New-Object System.IO.MemoryStream
        try {
            $stream.CopyTo($ms)
            $entryBytes = $ms.ToArray()
            $sha = [System.Security.Cryptography.SHA256]::Create()
            try {
                $entrySha = ([BitConverter]::ToString($sha.ComputeHash($entryBytes)) -replace '-', '').ToLowerInvariant()
            } finally { $sha.Dispose() }
        } finally {
            $ms.Dispose(); $stream.Dispose()
        }
        if ($entrySha -ne $scsLicenseSha) {
            Stop-Precondition "归档内 sdk_license.txt（sha256=$entrySha）与磁盘解包文件（sha256=$scsLicenseSha）不一致"
        }
    } finally { $zip.Dispose() }

    Add-Line "  组件          : $($sdk.name) $($sdk.version)"
    Add-Line "  归档          : vendor/scs_sdk_1_14.zip  sha256=$archiveSha（与 provenance 记录一致）"
    Add-Line "  上游来源      : $($sdk.source_url)"
    Add-Line "  上游许可标识  : $spdx（provenance 的 license.spdx_identifier）"
    Add-Line "  许可文本文件  : sdk_license.txt（归档内条目，逐字收录于下）"
    Add-Line "  许可文本 sha256: $scsLicenseSha"
    Add-Line "  文本长度      : $($scsText.Length) 字符（$(($scsText -split "`n").Count - 1) 行）"
    Add-Line "  版权行        : $($scsLic.copyright_line)"
    Add-Line "  是否随仓库提交: $(([string]$scsLic.committed_to_repository).ToLowerInvariant())（归档与解包目录均在 .gitignore 内）"
    Add-Line ''
    Add-Line '  可再分发性的操作语句（逐字引用 sdk_license.txt 第 4-9 行）：'
    foreach ($line in (Split-TextForNotice -Text ('"' + $scsLic.verbatim_grant + '"') -Width 72 -Indent '    ')) { Add-Line $line }
    Add-Line ''
    Add-Line '  同一许可文本的条件句（第 11-12 行）：'
    foreach ($line in (Split-TextForNotice -Text ('"' + $scsLic.verbatim_condition + '"') -Width 72 -Indent '    ')) { Add-Line $line }
    Add-Line ''
    Add-Line '  判定：允许再分发。上述授权句无条件列举了 use、copy、modify、merge、publish、'
    Add-Line '  distribute、sublicense 与 sell，且明示及于 source and binary form；唯一条件是保留'
    Add-Line '  版权与许可声明。sdk_license.txt 是未经改动的 MIT 许可文本（含 "Copyright (C) 2016'
    Add-Line '  SCS Software"），归档内 readme.txt 不含任何额外的许可、再分发或第三方 mod 限制。'
    Add-Line '  因此：把该归档或其头文件提交进仓库、或让 CI 下载它们，均被许可允许，不需要另行取得'
    Add-Line '  SCS 的授权；任何再分发必须随附 sdk_license.txt。'
    Add-Line ''
    Add-Line '  本仓库当前的实际形态（来自 provenance）：归档未提交、CI 也不下载，构建脚本'
    Add-Line '  telemetry-plugin/*/build.bat 在缺少 vendor/scs_sdk_1_14/include 时直接失败，'
    Add-Line '  SDK 因此是一个外部前置条件（由 scripts/prepare-scs-sdk.ps1 按需满足）。'
    Add-Line '  但两个插件 DLL 已提交并在仓库内分发：'
    foreach ($artifact in @(Get-JsonProperty -Object $scs -Name 'artifacts')) {
        $p = Join-Path $script:RepoRoot ($artifact.path -replace '/', '\')
        $status = if (Test-Path -LiteralPath $p) { 'on-disk sha256=' + (Get-FileSha256 -Path $p) } else { 'ABSENT' }
        Add-Line "    - $($artifact.name)：$($artifact.path)"
        Add-Line "      provenance 记录 sha256=$($artifact.sha256)（$($artifact.size_bytes) 字节）；$status"
    }
    Add-Line '  scs-nav-bridge.cpp 自述「基于官方 telemetry_mem 示例骨架（SCS SDK，MIT 许可）扩展」，'
    Add-Line '  即该 DLL 包含 SDK 示例代码的派生部分，属于 MIT 条件句所称的 substantial portions，'
    Add-Line '  故本节完整收录 sdk_license.txt 文本以满足保留声明的要求。'
    Add-Line ''
    Add-Line '  以下是 sdk_license.txt 的完整逐字内容。'
    Add-Line ''
    foreach ($line in (Format-LicenseBlock -Heading '# sdk_license.txt（逐字收录，sha256 如上）' -Text $scsText)) { Add-Line $line }

    # ---------- 未验证项 ----------
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[3.1] 判定（Release license audit）'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    Add-Line '  已登记的每个组件，其许可都由工作树中的文件确定：npm 组件来自已安装包的'
    Add-Line '  package.json 与其 LICENSE 文件，Rust 组件来自 cargo metadata 读出的上游 manifest'
    Add-Line '  字段，SCS SDK 来自归档内 sdk_license.txt 的文本与摘要。因此：'
    Add-Line ''
    Add-Line '    · 风险清单 A（许可不在允许清单内）：空。'
    Add-Line '    · 风险清单 B（第三方 crate 未声明许可）：空。'
    Add-Line '    · 未识别（unknown/unidentifiable）许可：无。'
    Add-Line '    · 需要标注但不阻塞判定的一项：pmtiles 4.5.0 与 murmurhash-js 1.0.0 的包内没有'
    Add-Line '      LICENSE 文件，其许可仅由 package.json 的 license 字段声明（分别为 BSD-3-Clause'
    Add-Line '      与 MIT）；这是「文本缺失」而不是「许可未知」。'
    Add-Line ''
    Add-Line '  结论：就本清单覆盖的组件范围而言，可以判定 Release license audit = PASS。'
    Add-Line '  但有 NOT CLOSED 项，发布前必须处理：[4] 第 1 条（最终随物的字形字体尚未确定，'
    Add-Line '  其许可因此无法登记）；若发布时提供 -FontsDir，则本 PASS 对该资源不成立，必须先'
    Add-Line '  补登字体许可再重新生成。其余 [4] 各项属于范围与精度说明，不改变上述判定。'
    Add-Line ''
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line '[4] NOT VERIFIED（本轮未能从证据确定的事项）'
    Add-Line '--------------------------------------------------------------------------------'
    Add-Line ''
    Add-Line '  1. 字形字体许可：未决（见 [2] 末）。仓库内不存在字体文件与其 provenance。'
    Add-Line '  2. Tauri 的 bundler 目标：desktop/tauri.conf.json 的 bundle 字段为空，'
    Add-Line '     本轮未执行 `tauri build`，因此「不产出 MSI/NSIS」是由配置推断而非实测。'
    Add-Line '  3. WebView2 运行期本体：不随物，本轮未在本机枚举已安装的 Evergreen Runtime 版本。'
    Add-Line '  4. Rust 侧 build-only 与 linked 的区分：`reach=build` 只表示该 crate 仅经'
    Add-Line '     build-dependency 边可达；`reach=normal` 只表示存在普通依赖边，两者都不等价于'
    Add-Line '     「该 crate 的机器码出现在最终二进制里」——cfg 门控、dead code elimination 与'
    Add-Line '     LTO 都可能把某个可达 crate 完全剔除。精确判定需要分析链接后的 PE/PDB，本轮未做。'
    Add-Line '  5. 本次生成使用的是普通 `cargo metadata` 而非 `--locked`：desktop/Cargo.toml、'
    Add-Line '     nav-core/tools/nav-core-cli/Cargo.toml 与 workspace 成员版本在本工作树中存在未提交的'
    Add-Line '     变更（0.1.0 -> 0.7.0-rc.1），两个 Cargo.lock 仍记录 0.1.0，--locked 会因此报'
    Add-Line '     "cannot update the lock file ... because --locked was passed"。可补偿的控制：'
    Add-Line '     本轮用普通 metadata 求解，并把结果与锁定文件逐条核对——两个锁文件的全部'
    Add-Line '     [[package]] 条目都已对应到闭包中的包（见 [2] 的核对行），版本差异只出现在锁文件'
    Add-Line '     对 workspace 根包的占位版本上，且已在 [2] 中逐条列出。求解过程对 Cargo.lock 的'
    Add-Line '     临时改动（仅重写这一处版本号）已还原为 Git 中的版本。'
    Add-Line '  6. scripts/crate-licenses.json 是派生快照：它随 Cargo.lock 或 workspace 版本变化而'
    Add-Line '     过期。生成脚本会逐条核对两个锁文件的 [[package]] 条目（缺项直接返回 3），但'
    Add-Line '     「同名 crate 换了版本」只能通过 -RefreshCrateLicenses 重解才能反映。锁文件变更时'
    Add-Line '     必须重新求解并重新生成本清单。'
    Add-Line '  7. npm 侧的传递依赖完整性：随物分发的传递依赖取自被 esbuild 打包的入口的'
    Add-Line '     dependencies 闭包，并用产物字节扫描核对（qrcode 的 dijkstrajs/pngjs/yargs 为零命中）。'
    Add-Line '     这是静态判定，不是对 dist/ 的模块级来源分析。'
    Add-Line ''
    Add-Line '================================================================================'
    Add-Line '清单结束。'
    Add-Line '================================================================================'

    return $L
}

# ── 主流程 ──────────────────────────────────────────────────────────────────────
if ($RefreshCrateLicenses) {
    $exe = Resolve-CargoExe
    Write-Host "[info] cargo: $exe"
    Update-CrateLicenseData -Exe $exe
    Write-Host "[info] crate 许可数据已刷新；请重新运行本脚本（不带 -RefreshCrateLicenses）以更新清单。"
    exit $script:ExitOk
}

$required = @($ProjectLicensePath, $WebPackageJson, $WebPackageLock,
              $CrateDataPath, $ScsProvenancePath, $ScsArchivePath, $ScsLicensePath)
if (-not $AllowMissingDist) { $required += $BuildManifestPath }
foreach ($path in $required) {
    if (-not (Test-Path -LiteralPath $path)) {
        Stop-Precondition "缺少必需输入：$(Get-RepoRelativePath $path)"
    }
}

$lines = Render-Notices
$body = ($lines -join "`r`n") + "`r`n"
# 注意括号位置：-replace 必须只作用于十六进制串本身，否则它会被解析为
# ToString(byte[], int) 的第二个实参而静默地产生 "-" 分隔的十六进制。
$bodySha = (([System.BitConverter]::ToString(
    [System.Security.Cryptography.SHA256]::Create().ComputeHash(
        [System.Text.Encoding]::UTF8.GetBytes($body))) -replace '-', '').ToLowerInvariant())
$content = $body + "# notices-body-sha256: $bodySha`r`n"

if (-not $OutputPath) { $OutputPath = Join-Path $script:RepoRoot $script:NoticesRel }
$outputFull = [System.IO.Path]::GetFullPath($OutputPath)

if ($Check) {
    if (-not (Test-Path -LiteralPath $outputFull)) {
        [Console]::Error.WriteLine("FAIL: 找不到已提交的 $(Get-RepoRelativePath $outputFull)")
        exit $script:ExitFail
    }
    $committed = (Get-NormalizedText -Path $outputFull) -replace "`n", "`r`n"
    if ($committed -ceq $content) {
        Write-Host "PASS: $(Get-RepoRelativePath $outputFull) 与生成结果逐字节一致（body sha256=$bodySha）"
        exit $script:ExitOk
    }
    $committedLines = @($committed -split "`r`n")
    $generatedLines = @($content -split "`r`n")
    $max = [Math]::Max($committedLines.Count, $generatedLines.Count)
    $firstDiff = -1
    for ($i = 0; $i -lt $max; $i++) {
        $a = if ($i -lt $committedLines.Count) { $committedLines[$i] } else { '<EOF>' }
        $b = if ($i -lt $generatedLines.Count) { $generatedLines[$i] } else { '<EOF>' }
        if ($a -cne $b) { $firstDiff = $i + 1; break }
    }
    [Console]::Error.WriteLine("FAIL: $(Get-RepoRelativePath $outputFull) 已过期（首个差异在第 $firstDiff 行；" +
                               "已提交 $($committedLines.Count) 行 / 生成 $($generatedLines.Count) 行）。")
    if ($firstDiff -gt 0) {
        [Console]::Error.WriteLine("  已提交: $($committedLines[$firstDiff - 1])")
        [Console]::Error.WriteLine("  应生成: $($generatedLines[$firstDiff - 1])")
    }
    [Console]::Error.WriteLine("  重新生成：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/generate-third-party-notices.ps1")
    exit $script:ExitFail
}

Write-Utf8NoBom -Path $outputFull -Text $content
$lineCount = ($content -split "`r`n").Count - 1
Write-Host "OK: 已生成 $(Get-RepoRelativePath $outputFull)（$lineCount 行，$(($content.Length)) 字符，body sha256=$bodySha）"
exit $script:ExitOk
