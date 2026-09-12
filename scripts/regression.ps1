<#
.SYNOPSIS
  ETS2Nav 回归套件（P1 / P2 / P3 / P5）的**唯一**判定实现。

.DESCRIPTION
  判定契约（Batch 4 重做）：

    1. 每个步骤的主判据是**被测进程的真实 exit code**（System.Diagnostics.Process.ExitCode）。
       不使用 `cmd | findstr`，不使用管道，不使用 $LASTEXITCODE。
    2. 需要业务语义的步骤附加**语义断言**（对已解码 stdout 的正则/子串检查）。
       两者必须同时成立才算 PASS：
           PASS  ==  (exit code ∈ AcceptExitCodes)  AND  (语义断言为真)
    3. 语义断言只做加法，不做替代。exit code != 0 时无论文本如何一律 FAIL；
       exit code == 0 但语义 marker 缺失同样 FAIL。
    4. 步骤之间**独立判定**：一个步骤失败不改变后续步骤的判定口径，
       只进入最终的 failed_steps 聚合与退出码。失败后继续执行以取得完整诊断。
    5. 结果分类（都返回非 0，但语义不同）：
           0 = PASS
           1 = TEST FAILURE        被测代码/数据不符合预期
           3 = PRECONDITION FAILURE 外部输入缺失或不可用（环境问题，不是产品缺陷）
           4 = HARNESS FAILURE      harness 自身无法可信执行（参数误用、构建产物不可信）

.PARAMETER Suite
  P1 / P2 / P3 / P5 / All / SelfTest。P2 链内含 P1，P3 链内含 P2（与原 run-p*-tests.bat 一致）；
  同一次调用内已执行过的链会在日志中标记为 reused，不重复执行，但结果沿用首次执行的真实退出码。

.PARAMETER Ets2Install
  游戏安装根目录。优先级：本参数 > $env:ETS2_INSTALL > （无 repo 内默认值，缺失即 PRECONDITION FAIL）。
  不得猜测用户磁盘位置。

.PARAMETER Ets2Extracted
  官方 scs_extractor 的解包根（含 base_map/ 与 def/）。
  优先级：本参数 > $env:ETS2NAV_EXTRACTED > repo 内 vendor/extracted（存在时）> PRECONDITION FAIL。
  P1 需要它：map-compiler 的集成测试（dotnet test）按同一约定解析该路径
  （见 map-compiler/tests/SharedTestPaths/TestPaths.cs）。

.PARAMETER Dataset
  数据集目录。优先级：本参数 > $env:ETS2NAV_DATASET > repo 内 data/europe-v5（存在时）> PRECONDITION FAIL。

.PARAMETER OdBaseline
  P5 OD 基准文件。优先级：本参数 > $env:ETS2NAV_OD_BASELINE > repo 内 od-baseline-europe-v5.txt（存在时）> PRECONDITION FAIL。
  普通回归**永不自动生成**基准；更新基准是独立的维护动作，见 -UpdateBaseline。

.PARAMETER Trace
  P2 map-match / bench 使用的 trace。缺省时在本次运行的临时工作区内现场生成 synthetic.navtrace。
  永不复用 %TEMP%\real.navtrace 之类历史文件。

.PARAMETER UpdateBaseline
  基准维护模式（不是测试）：重新生成 OD 基准。覆盖已存在的基准需要同时给出 -Force。
  不能被 -Suite All 使用，也不能被普通回归自动触发。

.PARAMETER KeepTemp
  成功时也保留临时工作区（失败时总是保留并打印路径）。

.PARAMETER StepTimeoutSeconds
  单个步骤的超时（默认 1800 s）。超时按 TEST FAILURE 处理并终止该步骤的子进程。

.EXAMPLE
  .\scripts\regression.ps1 -Suite All -Ets2Install 'D:\Steam\steamapps\common\Euro Truck Simulator 2'
.EXAMPLE
  .\scripts\regression.ps1 -Suite P5 -Dataset 'D:\ETS2 Test Data\europe-v5' -OdBaseline 'D:\ETS2 Test Data\base.txt'
.EXAMPLE
  .\scripts\regression.ps1 -Suite P5 -UpdateBaseline -Force
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('P1', 'P2', 'P3', 'P5', 'All', 'SelfTest')]
    [string[]]$Suite = @('All'),

    [string]$Ets2Install,
    [string]$Ets2Extracted,
    [string]$Dataset,
    [string]$OdBaseline,
    [string]$Trace,

    [switch]$UpdateBaseline,
    [switch]$Force,
    [switch]$KeepTemp,
    [int]$StepTimeoutSeconds = 1800,

    [string]$LogDir
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

# ─────────────────────────────────────────────────────────────────────────────
# 常量与退出码
# ─────────────────────────────────────────────────────────────────────────────
$EXIT_PASS = 0
$EXIT_TEST = 1
$EXIT_PRECONDITION = 3
$EXIT_HARNESS = 4

$script:RepoRoot = Split-Path -Parent $PSScriptRoot
$script:Results = New-Object System.Collections.ArrayList
$script:StepResults = @{}          # stepKey -> result（用于同一次调用内的链复用）
$script:RunStartUtc = [datetime]::UtcNow
$script:TempRoot = $null
$script:StepLogDir = $null

class PreconditionException : System.Exception {
    PreconditionException([string]$m) : base($m) { }
}
class HarnessException : System.Exception {
    HarnessException([string]$m) : base($m) { }
}

# ─────────────────────────────────────────────────────────────────────────────
# 输出与日志
# ─────────────────────────────────────────────────────────────────────────────
$script:LogLines = New-Object System.Collections.ArrayList
function Write-Log {
    param([string]$Text = '')
    [void]$script:LogLines.Add($Text)
    Write-Host $Text
}
function Write-Rule {
    param([string]$Text)
    Write-Log ('=' * 72)
    Write-Log $Text
    Write-Log ('=' * 72)
}

# ─────────────────────────────────────────────────────────────────────────────
# 工具解析与可执行文件白名单（§17：禁止调用本仓库之外的构建产物）
# ─────────────────────────────────────────────────────────────────────────────
$script:ToolCache = @{}
function Resolve-Tool {
    param([Parameter(Mandatory)][string]$Name)
    if ($script:ToolCache.ContainsKey($Name)) { return $script:ToolCache[$Name] }
    $cmd = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $cmd) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: 找不到可执行文件 '$Name'。请把它加入 PATH 后重试。")
    }
    $script:ToolCache[$Name] = $cmd.Source
    return $cmd.Source
}

$script:SessionTools = New-Object System.Collections.ArrayList
function Set-SessionTools {
    # 允许执行的外部工具（不在仓库内）：全部按名字从 PATH 解析，绝不硬编码路径
    foreach ($n in @('dotnet', 'cargo', 'python', 'cmd', 'powershell')) {
        try { [void]$script:SessionTools.Add((Resolve-Tool $n)) } catch { }
    }
}
function Assert-ExecutableAllowed {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$StepName)
    $full = [System.IO.Path]::GetFullPath($Path)
    if ($full.StartsWith($script:RepoRoot, [StringComparison]::OrdinalIgnoreCase)) { return $full }
    foreach ($t in $script:SessionTools) {
        if ($t -and $full.Equals($t, [StringComparison]::OrdinalIgnoreCase)) { return $full }
    }
    # fixture 允许位于本次运行的临时工作区内（H1/H2 等自检夹具）
    if ($script:TempRoot -and $full.StartsWith($script:TempRoot, [StringComparison]::OrdinalIgnoreCase)) { return $full }
    throw [HarnessException]::new(
        "HARNESS FAILURE: 步骤 $StepName 试图执行仓库外部的文件 '$full'。" +
        "回归步骤只允许执行本仓库构建产物或 PATH 上的标准工具（dotnet/cargo/python/cmd/powershell）。")
}

# ─────────────────────────────────────────────────────────────────────────────
# 配置解析（显式参数 > 环境变量 > 真实存在的 repo-relative 默认值 > PRECONDITION FAIL）
# ─────────────────────────────────────────────────────────────────────────────
function Resolve-Input {
    param(
        [Parameter(Mandatory)][string]$Label,
        [string]$Explicit,
        [string]$EnvName,
        [string[]]$RepoDefaults = @(),
        [switch]$Directory,
        # 维护模式专用：作为**写出目标**的路径允许尚不存在（基准生成的目标本来就不存在）
        [switch]$AllowMissing
    )
    $kind = if ($Directory) { '目录' } else { '文件' }
    $test = {
        param($p)
        if ($Directory) { return (Test-Path -LiteralPath $p -PathType Container) }
        return (Test-Path -LiteralPath $p -PathType Leaf)
    }
    # 显式参数与环境变量是**用户意图的明确表达**：指向不存在的路径时立即 PRECONDITION FAIL，
    # 不回落到默认值。静默换用别的输入会把「路径写错」变成「测试了另一个数据集」，正是本批次要消除的掩盖。
    if ($Explicit) {
        $p = $Explicit
        if (-not [System.IO.Path]::IsPathRooted($p)) { $p = Join-Path $script:RepoRoot $p }
        if ((& $test $p) -or ($AllowMissing -and -not $Directory)) {
            return @{ Path = [System.IO.Path]::GetFullPath($p); Source = 'parameter' }
        }
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: -$Label 指向的${kind}不存在: $([System.IO.Path]::GetFullPath($p))`n" +
            "  显式传入的路径不会被默认值替换（否则路径写错会静默测到另一个输入）。`n" +
            "  修正该路径，或去掉 -$Label 以使用默认值$(if ($EnvName) { " / 环境变量 $EnvName" })。")
    }
    if ($EnvName) {
        $v = [Environment]::GetEnvironmentVariable($EnvName)
        if ($v) {
            $p = $v
            if (-not [System.IO.Path]::IsPathRooted($p)) { $p = Join-Path $script:RepoRoot $p }
            if (& $test $p) { return @{ Path = [System.IO.Path]::GetFullPath($p); Source = "environment:$EnvName" } }
            throw [PreconditionException]::new(
                "PRECONDITION FAIL: 环境变量 $EnvName 指向的${kind}不存在: $([System.IO.Path]::GetFullPath($p))`n" +
                "  已设置的环境变量不会被默认值替换。请修正它，或清除该变量以使用 repo 默认值。")
        }
    }
    $tried = @()
    foreach ($d in $RepoDefaults) {
        $p = Join-Path $script:RepoRoot $d
        if (& $test $p) { return @{ Path = [System.IO.Path]::GetFullPath($p); Source = "repo-default:$d" } }
        $tried += "    $p"
    }
    $how = @("  -$Label 参数")
    if ($EnvName) { $how += "  环境变量 $EnvName" }
    throw [PreconditionException]::new(
        "PRECONDITION FAIL: $Label 未提供，且 repo 内没有可用的默认$kind。`n" +
        "  repo 默认值候选:`n" + (($tried) -join "`n") + "`n" +
        "  传入方式:`n" + ($how -join "`n"))
}

function Get-Ets2Install {
    param([string]$Explicit)
    $r = Resolve-Input -Label 'Ets2Install' -Explicit $Explicit -EnvName 'ETS2_INSTALL' `
        -RepoDefaults @() -Directory
    $required = @('base.scs', 'def.scs', 'base_map.scs')
    $missing = @()
    foreach ($f in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $r.Path $f) -PathType Leaf)) { $missing += $f }
    }
    if ($missing.Count -gt 0) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: Ets2Install '$($r.Path)' 缺少必需的 SCS 归档: $($missing -join ', ')。`n" +
            "  该路径必须是 ETS2 游戏根目录（内含 base.scs / def.scs / base_map.scs）。`n" +
            "  传入方式: -Ets2Install '<目录>' 或 环境变量 ETS2_INSTALL")
    }
    return $r
}

function Get-Ets2Extracted {
    param([string]$Explicit)
    # scs_extractor 的解包根（含 base_map/ 与 def/）。map-compiler 的集成测试与本套件的
    # P1 依赖它；缺省用 repo 相对路径 vendor/extracted。
    $r = Resolve-Input -Label 'Ets2Extracted' -Explicit $Explicit -EnvName 'ETS2NAV_EXTRACTED' `
        -RepoDefaults @('vendor/extracted') -Directory
    $required = @('base_map', 'def')
    $missing = @()
    foreach ($d in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $r.Path $d) -PathType Container)) { $missing += $d }
    }
    if ($missing.Count -gt 0) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: Ets2Extracted '$($r.Path)' 缺少子目录: $($missing -join ', ')。`n" +
            "  该路径必须是官方 scs_extractor 的解包根（含 base_map/ 与 def/）。`n" +
            "  传入方式: -Ets2Extracted '<目录>' 或 环境变量 ETS2NAV_EXTRACTED`n" +
            "  它同时会被导出给 dotnet test（ScsSector/ScsGraph/ScsHashFs 的集成测试按同一约定解析）。")
    }
    return $r
}

function Get-DatasetPath {
    param([string]$Explicit)    $r = Resolve-Input -Label 'Dataset' -Explicit $Explicit -EnvName 'ETS2NAV_DATASET' `
        -RepoDefaults @('data/europe-v5', 'data/europe-v4') -Directory
    $required = @('routing.graph', 'junction.graph', 'map.db', 'search.db')
    $missing = @()
    foreach ($f in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $r.Path $f) -PathType Leaf)) { $missing += $f }
    }
    if ($missing.Count -gt 0) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: Dataset '$($r.Path)' 缺少必需文件: $($missing -join ', ')。`n" +
            "  该路径必须是由 map-inspector --dataset 生成的完整数据集目录。`n" +
            "  传入方式: -Dataset '<目录>' 或 环境变量 ETS2NAV_DATASET")
    }
    return $r
}

function Get-OdBaselinePath {
    param([string]$Explicit, [switch]$AllowMissing)
    $r = Resolve-Input -Label 'OdBaseline' -Explicit $Explicit -EnvName 'ETS2NAV_OD_BASELINE' `
        -RepoDefaults @('od-baseline-europe-v5.txt') -AllowMissing:$AllowMissing
    if ($AllowMissing -and -not (Test-Path -LiteralPath $r.Path -PathType Leaf)) {
        # 维护目标尚不存在：没有可校验的内容
        $r['Pairs'] = 0
        $r['Exists'] = $false
        return $r
    }
    $lines = @(Get-Content -LiteralPath $r.Path -Encoding UTF8 | Where-Object { $_.Trim() -ne '' })
    $pairs = @($lines | Where-Object { ($_ -split '\|').Count -eq 8 })
    if ($pairs.Count -eq 0) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: OdBaseline '$($r.Path)' 不含任何合法基准行（期望 8 个 '|' 分隔字段）。`n" +
            "  该文件应由维护模式生成: .\scripts\regression.ps1 -Suite P5 -UpdateBaseline")
    }
    $r['Pairs'] = $pairs.Count
    $r['Exists'] = $true
    return $r
}

# ─────────────────────────────────────────────────────────────────────────────
# 输出解码：先按 UTF-8 严格解码，失败则回退到本机 OEM 代码页
#   Rust 工具恒定写 UTF-8；.NET 控制台在重定向时使用控制台代码页（zh-CN 下为 936）。
#   两种都覆盖，且不使用任何硬编码代码页。
# ─────────────────────────────────────────────────────────────────────────────
$script:StrictUtf8 = New-Object System.Text.UTF8Encoding($false, $true)
$script:OemEncoding = [System.Text.Encoding]::GetEncoding(
    [System.Globalization.CultureInfo]::CurrentCulture.TextInfo.OEMCodePage)
function ConvertFrom-RawOutput {
    param([byte[]]$Bytes)
    if (-not $Bytes -or $Bytes.Length -eq 0) { return @{ Text = ''; Encoding = 'empty' } }
    try {
        return @{ Text = $script:StrictUtf8.GetString($Bytes); Encoding = 'utf-8' }
    } catch {
        return @{ Text = $script:OemEncoding.GetString($Bytes); Encoding = "oem-$($script:OemEncoding.CodePage)" }
    }
}

# ─────────────────────────────────────────────────────────────────────────────
# Windows 命令行参数转义（用于 ProcessStartInfo.Arguments 单一字符串形式）
# ─────────────────────────────────────────────────────────────────────────────
function ConvertTo-CommandLineArgument {
    param([string]$Value)
    if ($null -eq $Value) { return '""' }
    if ($Value -ne '' -and $Value -notmatch '[\s"]') { return $Value }
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    $backslashes = 0
    foreach ($ch in $Value.ToCharArray()) {
        if ($ch -eq '\') { $backslashes++; continue }
        if ($ch -eq '"') {
            [void]$sb.Append(('\' * (2 * $backslashes + 1)))
            [void]$sb.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) { [void]$sb.Append(('\' * $backslashes)); $backslashes = 0 }
        [void]$sb.Append($ch)
    }
    if ($backslashes -gt 0) { [void]$sb.Append(('\' * (2 * $backslashes))) }
    [void]$sb.Append('"')
    return $sb.ToString()
}

# ─────────────────────────────────────────────────────────────────────────────
# 步骤执行器：真实 exit code + 独立 stdout/stderr + 可选语义断言
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-NativeStep {
    param(
        [Parameter(Mandatory)][string]$Suite,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory,
        [scriptblock]$Semantic,
        [string]$SemanticText,
        [int[]]$AcceptExitCodes = @(0),
        [int]$TimeoutSeconds = 0,
        [switch]$ProbeOnly
    )
    $key = "$Suite/$Name"
    $label = "[$Suite`:$Name]"
    if (-not $WorkingDirectory) { $WorkingDirectory = $script:RepoRoot }
    $exe = Assert-ExecutableAllowed -Path $FilePath -StepName $key
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
        return (Add-StepResult -Suite $Suite -Name $Name -Result 'HARNESS FAILURE' `
                -ExitCode $null -SemanticPassed $null -Duration 0 -ProbeOnly:$ProbeOnly `
                -Detail "可执行文件不存在: $exe" -HarnessFailure)
    }
    if ($TimeoutSeconds -le 0) { $TimeoutSeconds = $StepTimeoutSeconds }

    $argString = (($Arguments | ForEach-Object { ConvertTo-CommandLineArgument $_ }) -join ' ')
    Write-Log "$label exec: $exe"
    if ($argString) { Write-Log "$label args: $argString" }
    Write-Log "$label cwd : $WorkingDirectory"

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $argString
    $psi.WorkingDirectory = $WorkingDirectory
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true
    $psi.EnvironmentVariables['DOTNET_CLI_UI_LANGUAGE'] = 'en'
    $psi.EnvironmentVariables['DOTNET_NOLOGO'] = '1'
    $psi.EnvironmentVariables['DOTNET_SKIP_FIRST_TIME_EXPERIENCE'] = '1'
    $psi.EnvironmentVariables['CARGO_TERM_COLOR'] = 'never'
    $psi.EnvironmentVariables['RUST_BACKTRACE'] = '0'
    $psi.EnvironmentVariables['NO_COLOR'] = '1'

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $outMs = New-Object System.IO.MemoryStream
    $errMs = New-Object System.IO.MemoryStream
    $exitCode = $null
    $timedOut = $false
    try {
        [void]$proc.Start()
        $outTask = $proc.StandardOutput.BaseStream.CopyToAsync($outMs)
        $errTask = $proc.StandardError.BaseStream.CopyToAsync($errMs)
        if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
            $timedOut = $true
            try { $proc.Kill() } catch { }
            try { [void]$proc.WaitForExit(15000) } catch { }
        } else {
            $proc.WaitForExit()
        }
        if (-not $timedOut) { $exitCode = $proc.ExitCode }
        try { [void]$outTask.Wait(10000) } catch { }
        try { [void]$errTask.Wait(10000) } catch { }
    } finally {
        $sw.Stop()
        if ($proc) { $proc.Dispose() }
    }
    $decOut = ConvertFrom-RawOutput -Bytes $outMs.ToArray()
    $decErr = ConvertFrom-RawOutput -Bytes $errMs.ToArray()
    $stdout = $decOut.Text
    $stderr = $decErr.Text

    $outFile = Join-Path $script:StepLogDir "$Suite-$Name.out.txt"
    $errFile = Join-Path $script:StepLogDir "$Suite-$Name.err.txt"
    [System.IO.File]::WriteAllText($outFile, $stdout, $script:StrictUtf8)
    [System.IO.File]::WriteAllText($errFile, $stderr, $script:StrictUtf8)
    $meta = @(
        "step   : $key",
        "exe    : $exe",
        "args   : $argString",
        "cwd    : $WorkingDirectory",
        "exit   : $(if ($timedOut) { 'TIMEOUT' } else { $exitCode })",
        "stdout : $($decOut.Encoding), $($stdout.Length) chars",
        "stderr : $($decErr.Encoding), $($stderr.Length) chars",
        "log    : $outFile"
    ) -join "`r`n"
    [System.IO.File]::WriteAllText((Join-Path $script:StepLogDir "$Suite-$Name.meta.txt"), $meta, $script:StrictUtf8)

    if ($timedOut) {
        Write-Log "$label TIMEOUT after ${TimeoutSeconds}s (killed)"
        Write-Log (Get-OutputTail -Stdout $stdout -Stderr $stderr)
        return (Add-StepResult -Suite $Suite -Name $Name -Result 'TEST FAILURE' -ExitCode $null `
                -SemanticPassed $null -Duration $sw.Elapsed.TotalSeconds -ProbeOnly:$ProbeOnly `
                -Detail "超时 ${TimeoutSeconds}s" -LogPath $outFile)
    }

    $exitOk = $AcceptExitCodes -contains $exitCode
    $semOk = $null
    $semDetail = ''
    if ($Semantic) {
        $r = & $Semantic $stdout $stderr
        if ($r -is [bool]) { $semOk = $r }
        elseif ($r -is [hashtable] -and $r.ContainsKey('Ok')) { $semOk = [bool]$r['Ok']; $semDetail = [string]$r['Detail'] }
        else { $semOk = [bool]$r }
        if (-not $semDetail -and -not $semOk) { $semDetail = "语义断言未满足: $SemanticText" }
    }

    $result = if ($exitOk -and ($semOk -ne $false)) { 'PASS' } else { 'TEST FAILURE' }
    $parts = @()
    $parts += if ($exitCode -ne $null) { "exit=$exitCode" } else { 'exit=n/a' }
    if ($Semantic) { $parts += "semantic=$(if ($semOk) { 'PASS' } else { 'FAIL' })" }
    $parts += ('{0:N1}s' -f $sw.Elapsed.TotalSeconds)
    Write-Log "$label $result ($($parts -join ', '))"
    if ($result -ne 'PASS') {
        if (-not $exitOk) {
            Write-Log "$label expected exit in [$($AcceptExitCodes -join ',')] but got $exitCode"
        }
        if ($semDetail) { Write-Log "$label $semDetail" }
        Write-Log (Get-OutputTail -Stdout $stdout -Stderr $stderr)
    }

    return (Add-StepResult -Suite $Suite -Name $Name -Result $result -ExitCode $exitCode `
            -SemanticPassed $semOk -Duration $sw.Elapsed.TotalSeconds -Detail $semDetail `
            -LogPath $outFile -SemanticText $SemanticText -ProbeOnly:$ProbeOnly)
}

function Get-OutputTail {
    param([string]$Stdout, [string]$Stderr, [int]$Lines = 25)
    $sb = New-Object System.Text.StringBuilder
    if ($Stderr -and $Stderr.Trim()) {
        [void]$sb.AppendLine('  --- stderr (tail) ---')
        foreach ($l in (($Stderr -split "`r?`n") | Where-Object { $_ -ne '' } | Select-Object -Last $Lines)) {
            [void]$sb.AppendLine("  | $l")
        }
    }
    if ($Stdout -and $Stdout.Trim()) {
        [void]$sb.AppendLine('  --- stdout (tail) ---')
        foreach ($l in (($Stdout -split "`r?`n") | Where-Object { $_ -ne '' } | Select-Object -Last $Lines)) {
            [void]$sb.AppendLine("  | $l")
        }
    }
    return $sb.ToString().TrimEnd()
}

function Add-StepResult {
    param(
        [string]$Suite, [string]$Name, [string]$Result,
        [object]$ExitCode, [object]$SemanticPassed, [double]$Duration,
        [string]$Detail = '', [string]$LogPath = '', [string]$SemanticText = '',
        [switch]$HarnessFailure, [switch]$ProbeOnly
    )
    $r = [pscustomobject]@{
        Suite          = $Suite
        Name           = $Name
        Result         = $Result
        ExitCode       = $ExitCode
        SemanticPassed = $SemanticPassed
        Duration       = $Duration
        Detail         = $Detail
        LogPath        = $LogPath
        SemanticText   = $SemanticText
        HarnessFailure = [bool]$HarnessFailure
    }
    # ProbeOnly：自检夹具的探针结果不进入套件判定（H1/H2 的「预期失败」不是套件失败）
    if (-not $ProbeOnly) {
        [void]$script:Results.Add($r)
        $script:StepResults["$Suite/$Name"] = $r
    }
    return $r
}

# harness 自身检查（不启动进程）：用于 determinism 比对、diagnostics.json 断言等
function Invoke-HarnessCheck {
    param(
        [Parameter(Mandatory)][string]$Suite,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$Check,
        [Parameter(Mandatory)][string]$Description
    )
    $label = "[$Suite`:$Name]"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $ok = $false
    $detail = ''
    try {
        $r = & $Check
        if ($r -is [bool]) { $ok = $r }
        elseif ($r -is [hashtable] -and $r.ContainsKey('Ok')) { $ok = [bool]$r['Ok']; $detail = [string]$r['Detail'] }
        else { $ok = [bool]$r }
    } catch {
        $ok = $false
        $detail = "检查抛出异常: $($_.Exception.Message)"
    }
    $sw.Stop()
    if (-not $detail -and -not $ok) { $detail = "断言未满足: $Description" }
    $result = if ($ok) { 'PASS' } else { 'TEST FAILURE' }
    Write-Log "$label $result (check, $('{0:N1}s' -f $sw.Elapsed.TotalSeconds))"
    if (-not $ok) { Write-Log "$label $detail" }
    return (Add-StepResult -Suite $Suite -Name $Name -Result $result -ExitCode $null `
            -SemanticPassed $ok -Duration $sw.Elapsed.TotalSeconds -Detail $detail -SemanticText $Description)
}

# ─────────────────────────────────────────────────────────────────────────────
# 语义断言工具
# ─────────────────────────────────────────────────────────────────────────────
function Test-Contains {
    param([string]$Text, [string]$Needle, [string]$Label)
    $hit = $Text -and $Text.Contains($Needle)
    return @{ Ok = $hit; Detail = if ($hit) { '' } else { "stdout 未包含 '$Needle'（$Label）" } }
}
function Test-Match {
    param([string]$Text, [string]$Pattern, [string]$Label)
    $hit = $Text -and ([regex]::IsMatch($Text, $Pattern))
    return @{ Ok = [bool]$hit; Detail = if ($hit) { '' } else { "stdout 未匹配 /$Pattern/（$Label）" } }
}
function Test-All {
    param([object[]]$Checks)
    foreach ($c in $Checks) { if (-not $c.Ok) { return $c } }
    return @{ Ok = $true; Detail = '' }
}
function Get-CapturedGroup {
    param([string]$Text, [string]$Pattern)
    $m = [regex]::Match($Text, $Pattern)
    if ($m.Success -and $m.Groups.Count -gt 1) { return $m.Groups[1].Value }
    return $null
}

# ─────────────────────────────────────────────────────────────────────────────
# 构建产物可信性：产物必须比其全部源码更新（捕获「源码已恢复但二进制陈旧」）
# ─────────────────────────────────────────────────────────────────────────────
$script:SourceExtensions = @('.rs', '.toml', '.lock', '.cs', '.csproj', '.csproj.user', '.sln')
function Get-NewestSourceFile {
    param([Parameter(Mandatory)][string[]]$Roots)
    $newest = $null
    $skip = '\\(target|obj|bin|node_modules|\.git)\\'
    foreach ($root in $Roots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        $stack = New-Object System.Collections.Stack
        $stack.Push($root)
        while ($stack.Count -gt 0) {
            $dir = $stack.Pop()
            try { $entries = [System.IO.Directory]::GetFileSystemEntries($dir) } catch { continue }
            foreach ($e in $entries) {
                if ([System.IO.Directory]::Exists($e)) {
                    if ($e -match $skip) { continue }
                    $stack.Push($e)
                } else {
                    $ext = [System.IO.Path]::GetExtension($e)
                    if ($script:SourceExtensions -notcontains $ext) { continue }
                    if ($e -match $skip) { continue }
                    $t = [System.IO.File]::GetLastWriteTimeUtc($e)
                    if (-not $newest -or $t -gt $newest.Time) { $newest = [pscustomobject]@{ Path = $e; Time = $t } }
                }
            }
        }
    }
    return $newest
}

function Get-CSharpSourceRoots {
    <#
      从 csproj 出发递归收集 ProjectReference 的项目目录（含自身）。
      必要性：产物新鲜度必须对「真正决定该产物的源码集合」比较。用整个 map-compiler/src
      当根会把未被引用的项目也算进来，因此一次与 map-inspector 无关的编辑会让
      `dotnet build map-inspector` 之后仍报「产物陈旧」——那是假失败，与漏报同样有害。

      实现注意：StrictMode 2.0 下访问 XML 适配器上**不存在**的属性会抛异常
      （`$x.Project.ItemGroup` 在只有 PropertyGroup 的项目上抛；`$g.ProjectReference`
      在没有该引用的 ItemGroup 上抛，实测 ScsHashFs.csproj / ScsSii.csproj 均如此）。
      因此这里一律走 ChildNodes + LocalName：既避免属性缺失异常，也不受 MSBuild 命名空间影响。
    #>
    param([Parameter(Mandatory)][string]$Csproj)
    $dirs = New-Object System.Collections.ArrayList
    $seen = @{}
    $walk = {
        param([string]$proj)
        $full = [System.IO.Path]::GetFullPath($proj)
        if ($seen.ContainsKey($full)) { return }
        $seen[$full] = $true
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { return }
        [void]$dirs.Add((Split-Path -Parent $full))
        try { [xml]$x = Get-Content -LiteralPath $full -Raw } catch { return }
        $root = $x.DocumentElement
        if (-not $root) { return }
        foreach ($node in $root.ChildNodes) {
            if ($node.LocalName -ne 'ItemGroup') { continue }
            foreach ($child in $node.ChildNodes) {
                if ($child.LocalName -ne 'ProjectReference') { continue }
                $inc = $child.GetAttribute('Include')
                if ($inc) { & $walk (Join-Path (Split-Path -Parent $full) $inc) }
            }
        }
    }
    & $walk $Csproj
    return @($dirs | Select-Object -Unique)
}

$script:CargoMetadataCache = @{}
function Get-RustSourceRoots {
    <#
      用 `cargo metadata` 计算某包的依赖闭包（只取仓库内的路径依赖），作为产物新鲜度的
      比较根。必要性：od-corpus 与 nav-core-cli 是同一 workspace 内的两个独立二进制，
      用整个 nav-core 当根时，编辑 nav-core-cli 会把 od-corpus 的产物误判为陈旧——
      本批次实测发生过（案例 4/5）。闭包是精确的：只有真正参与该产物构建的源码才会
      影响判定。注册表依赖的 manifest 在仓库外，一律排除。metadata 失败时退化为包目录。
    #>
    param(
        [Parameter(Mandatory)][string]$WorkspaceManifest,
        [Parameter(Mandatory)][string]$PackageName,
        [Parameter(Mandatory)][string]$FallbackRoot
    )
    try {
        if (-not $script:CargoMetadataCache.ContainsKey($WorkspaceManifest)) {
            $cargo = Resolve-Tool 'cargo'
            $json = & $cargo metadata --format-version 1 --locked --manifest-path $WorkspaceManifest 2>$null | Out-String
            $code = $LASTEXITCODE
            if ($code -ne 0) { throw "cargo metadata exited $code" }
            $script:CargoMetadataCache[$WorkspaceManifest] = ($json | ConvertFrom-Json)
        }
        $md = $script:CargoMetadataCache[$WorkspaceManifest]
        $byId = @{}
        foreach ($p in $md.packages) { $byId[$p.id] = $p }
        $nodeById = @{}
        foreach ($n in $md.resolve.nodes) { $nodeById[$n.id] = $n }
        $target = @($md.packages | Where-Object {
                $_.name -eq $PackageName -and
                $_.manifest_path.StartsWith($script:RepoRoot, [StringComparison]::OrdinalIgnoreCase)
            }) | Select-Object -First 1
        if (-not $target) { throw "包 '$PackageName' 不在 $WorkspaceManifest 的 workspace 内" }
        $dirs = New-Object System.Collections.ArrayList
        $seen = @{}
        $queue = New-Object System.Collections.Queue
        $seen[$target.id] = $true
        $queue.Enqueue($target.id)
        while ($queue.Count -gt 0) {
            $id = $queue.Dequeue()
            $pk = $byId[$id]
            if ($pk -and $pk.manifest_path.StartsWith($script:RepoRoot, [StringComparison]::OrdinalIgnoreCase)) {
                [void]$dirs.Add((Split-Path -Parent $pk.manifest_path))
            }
            $n = $nodeById[$id]
            if ($n -and $n.deps) {
                foreach ($d in $n.deps) {
                    if (-not $seen.ContainsKey($d.pkg)) {
                        $seen[$d.pkg] = $true
                        $queue.Enqueue($d.pkg)
                    }
                }
            }
        }
        if ($dirs.Count -eq 0) { throw '依赖闭包为空' }
        return @($dirs | Select-Object -Unique)
    } catch {
        Write-Log "[harness] 警告: 无法计算 '$PackageName' 的 cargo 依赖闭包（$($_.Exception.Message)）；新鲜度比较退化到包目录 $FallbackRoot"
        return @($FallbackRoot)
    }
}

function Get-ArtifactInfo {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $fi = Get-Item -LiteralPath $Path
    $h = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    return [pscustomobject]@{
        Path     = $fi.FullName
        Length   = $fi.Length
        Written  = $fi.LastWriteTimeUtc
        Sha256   = $h
    }
}

function Assert-ArtifactFresh {
    param(
        [Parameter(Mandatory)][string]$Suite,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Artifact,
        [Parameter(Mandatory)][string[]]$SourceRoots,
        [int]$GraceSeconds = 2,
        [switch]$ProbeOnly
    )
    $info = Get-ArtifactInfo -Path $Artifact
    $desc = "产物存在、非空、且不早于其全部源码（$([System.IO.Path]::GetFileName($Artifact))）"
    if (-not $info -or $info.Length -le 0) {
        return (Add-StepResult -Suite $Suite -Name $Name -Result 'HARNESS FAILURE' -ExitCode $null `
                -SemanticPassed $false -Duration 0 -HarnessFailure -ProbeOnly:$ProbeOnly `
                -Detail "构建步骤报告成功，但产物缺失或为空: $Artifact" -SemanticText $desc)
    }
    $newest = Get-NewestSourceFile -Roots $SourceRoots
    if ($newest -and $newest.Time -gt $info.Written.AddSeconds($GraceSeconds)) {
        return (Add-StepResult -Suite $Suite -Name $Name -Result 'HARNESS FAILURE' -ExitCode $null `
                -SemanticPassed $false -Duration 0 -HarnessFailure -ProbeOnly:$ProbeOnly -SemanticText $desc `
                -Detail ("产物陈旧: $($info.Path) 写于 $($info.Written.ToString('o'))，" +
                          "但源码 $($newest.Path) 更新于 $($newest.Time.ToString('o'))"))
    }
    Write-Log "[$Suite`:$Name] artifact: $($info.Path)"
    Write-Log "[$Suite`:$Name] artifact: $($info.Length) bytes, written $($info.Written.ToString('o')), sha256 $($info.Sha256.Substring(0,16))"
    return (Add-StepResult -Suite $Suite -Name $Name -Result 'PASS' -ExitCode 0 `
            -SemanticPassed $true -Duration 0 -SemanticText $desc -ProbeOnly:$ProbeOnly `
            -Detail "sha256=$($info.Sha256.Substring(0,16)) bytes=$($info.Length) path=$($info.Path)")
}

# 链复用：同一次调用内已执行过的套件结果直接沿用（不重复执行，但保留原始退出码证据）
function Add-ChainResult {
    param([string]$Suite, [string]$ChainName, [string]$ChainedSuite)
    $child = @($script:Results | Where-Object { $_.Suite -eq $ChainedSuite })
    if ($child.Count -eq 0) {
        return (Add-StepResult -Suite $Suite -Name $ChainName -Result 'HARNESS FAILURE' -ExitCode $null `
                -SemanticPassed $null -Duration 0 -HarnessFailure `
                -Detail "链套件 $ChainedSuite 没有产生任何步骤结果")
    }
    $failed = @($child | Where-Object { $_.Result -ne 'PASS' })
    $dur = ($child | Measure-Object -Property Duration -Sum).Sum
    $result = if ($failed.Count -eq 0) { 'PASS' } else { 'TEST FAILURE' }
    $label = "[$Suite`:$ChainName]"
    Write-Log "$label $result (chain $ChainedSuite, steps=$($child.Count), failed=$($failed.Count), reused)"
    if ($failed.Count -gt 0) {
        Write-Log "$label 失败步骤: $((($failed | ForEach-Object { $_.Name }) -join ', '))"
    }
    return (Add-StepResult -Suite $Suite -Name $ChainName -Result $result `
            -ExitCode $(if ($failed.Count -eq 0) { 0 } else { 1 }) -SemanticPassed ($failed.Count -eq 0) `
            -Duration $dur -SemanticText "$ChainedSuite 全部步骤 PASS" `
            -Detail "reused; steps=$($child.Count); failed=$($failed.Count)")
}

# ─────────────────────────────────────────────────────────────────────────────
# 临时工作区（§7：每轮唯一，不复用历史文件）
# ─────────────────────────────────────────────────────────────────────────────
function New-RunTempRoot {
    $stamp = (Get-Date).ToString('yyyyMMdd-HHmmss')
    $rand = [guid]::NewGuid().ToString('N').Substring(0, 8)
    $root = Join-Path $env:TEMP "ets2nav-regression-$stamp-$PID-$rand"
    if (Test-Path -LiteralPath $root) {
        throw [HarnessException]::new("HARNESS FAILURE: 临时工作区已存在（不应发生）: $root")
    }
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    return $root
}

# ─────────────────────────────────────────────────────────────────────────────
# P1 套件
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-P1Suite {
    param([hashtable]$Cfg)
    Write-Rule 'P1 Regression Suite（map-compiler / map-inspector / determinism / dataset-reader / europe scale）'
    $install = $Cfg.Ets2Install.Path
    $tmp = Join-Path $script:TempRoot 'p1'
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    $det1 = Join-Path $tmp 'det-1'
    $det2 = Join-Path $tmp 'det-2'
    $europe = Join-Path $tmp 'europe'
    $sectors = 'sec+0002-0002,sec+0002-0003,sec+0003-0002,sec+0003-0003,sec+0002-0001,sec+0002-0004,sec+0003-0001,sec+0003-0004'

    $dotnet = Resolve-Tool 'dotnet'
    $cargo = Resolve-Tool 'cargo'

    # [1] 单元测试
    [void](Invoke-NativeStep -Suite P1 -Name 'dotnet-test' -FilePath $dotnet `
        -Arguments @('test', 'map-compiler/MapCompiler.sln', '-v', 'q', '--nologo') `
        -SemanticText 'exit 0 且 stdout 含 "Passed!" 与 "Failed: 0"' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out 'Passed!' 'dotnet test 汇总行'),
                (Test-Match $out 'Failed:\s*0\b' 'dotnet test 失败数为 0')
            )
        })

    # [2] 构建 map-inspector（不在 MapCompiler.sln 内，必须显式构建）
    $miCsproj = Join-Path $script:RepoRoot 'tools/map-inspector/MapInspector/MapInspector.csproj'
    $miExe = Join-Path $script:RepoRoot 'tools/map-inspector/MapInspector/bin/Debug/net9.0/map-inspector.exe'
    $miRoots = Get-CSharpSourceRoots -Csproj $miCsproj
    $r = Invoke-NativeStep -Suite P1 -Name 'build-map-inspector' -FilePath $dotnet `
        -Arguments @('build', $miCsproj, '-c', 'Debug', '-v', 'q', '--nologo') `
        -SemanticText 'exit 0 且产物不早于源码'
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P1 -Name 'build-map-inspector-artifact' -Artifact $miExe `
            -SourceRoots $miRoots)
    }

    # [3] Berlin gate
    [void](Invoke-NativeStep -Suite P1 -Name 'gate-berlin' -FilePath $miExe `
        -Arguments @('--install', $install, '--sectors', $sectors, '--gate') `
        -SemanticText 'exit 0 且 stdout 含 "GATE 通过"' `
        -Semantic { param($out, $err) Test-Contains $out 'GATE 通过' 'Berlin gate 结论行' })

    # [4] Germany gate
    [void](Invoke-NativeStep -Suite P1 -Name 'gate-germany' -FilePath $miExe `
        -Arguments @('--install', $install, '--region', 'germany', '--gate') `
        -SemanticText 'exit 0 且 stdout 含 "GATE 通过"' `
        -Semantic { param($out, $err) Test-Contains $out 'GATE 通过' 'Germany gate 结论行' })

    # [5][6] determinism：两个全新目录各构建一次
    $required = @('routing.graph', 'junction.graph', 'map.db', 'search.db')
    foreach ($pair in @(@('det-build-1', $det1), @('det-build-2', $det2))) {
        $name = $pair[0]; $dir = $pair[1]
        [void](Invoke-NativeStep -Suite P1 -Name $name -FilePath $miExe `
            -Arguments @('--install', $install, '--sectors', 'sec+0002-0002,sec+0002-0003', '--dataset', $dir) `
            -SemanticText 'exit 0 且四个产物文件均存在且非空' `
            -Semantic {
                param($out, $err)
                $missing = @()
                foreach ($f in $required) {
                    $p = Join-Path $dir $f
                    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { $missing += $f; continue }
                    if ((Get-Item -LiteralPath $p).Length -le 0) { $missing += "$f(empty)" }
                }
                if ($missing.Count -gt 0) { return @{ Ok = $false; Detail = "缺少产物: $($missing -join ', ')" } }
                return @{ Ok = $true; Detail = '' }
            })
        [void](Assert-ArtifactFresh -Suite P1 -Name "$name-artifact" -Artifact (Join-Path $dir 'routing.graph') `
            -SourceRoots $miRoots)
    }

    # [7] determinism 比对（harness 检查，无进程）
    [void](Invoke-HarnessCheck -Suite P1 -Name 'determinism' -Description '两次独立构建的四个产物 SHA-256 完全一致' -Check {
            $bad = @()
            $lines = @()
            foreach ($f in $required) {
                $a = (Get-FileHash -LiteralPath (Join-Path $det1 $f) -Algorithm SHA256).Hash
                $b = (Get-FileHash -LiteralPath (Join-Path $det2 $f) -Algorithm SHA256).Hash
                $lines += ("  {0,-16} {1}  {2}" -f $f, $a.Substring(0, 16), $(if ($a -eq $b) { 'same' } else { 'DIFF' }))
                if ($a -ne $b) { $bad += $f }
            }
            Write-Log ($lines -join "`n")
            if ($bad.Count -gt 0) { return @{ Ok = $false; Detail = "产物不一致: $($bad -join ', ')" } }
            return @{ Ok = $true; Detail = '' }
        })

    # [8] 构建 Rust dataset reader（独立 crate，不在 nav-core workspace 内）
    $smokeManifest = Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/Cargo.toml'
    $smokeExe = Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/target/release/dataset-reader-smoke.exe'
    $smokeRoots = Get-RustSourceRoots -WorkspaceManifest $smokeManifest -PackageName 'dataset-reader-smoke' `
        -FallbackRoot (Join-Path $script:RepoRoot 'tools/dataset-reader-smoke')
    $r = Invoke-NativeStep -Suite P1 -Name 'build-dataset-reader' -FilePath $cargo `
        -Arguments @('build', '--release', '--manifest-path', $smokeManifest) `
        -WorkingDirectory $script:RepoRoot -SemanticText 'exit 0 且产物不早于源码'
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P1 -Name 'build-dataset-reader-artifact' -Artifact $smokeExe `
            -SourceRoots $smokeRoots)
    }

    # [9] 运行 dataset reader（对本轮新建的 det-1）
    [void](Invoke-NativeStep -Suite P1 -Name 'dataset-reader' -FilePath $smokeExe -Arguments @($det1))

    # [10] Europe scale 全量构建
    [void](Invoke-NativeStep -Suite P1 -Name 'europe-scale' -FilePath $miExe `
        -Arguments @('--install', $install, '--all-sectors', '--dataset', $europe) `
        -SemanticText 'exit 0 且 stdout 含 "diagnostics.json 已写"' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out 'diagnostics.json 已写' '数据集写出结论行'),
                (Test-Match $out '已加载\s+(\d+)\s+个 sector' 'sector 加载统计存在')
            )
        })
    # [11] diagnostics.json 断言（harness 检查）
    [void](Invoke-HarnessCheck -Suite P1 -Name 'europe-diagnostics' `
        -Description 'diagnostics.json 存在、含 failed_prefabs 键且为 0' -Check {
            $p = Join-Path $europe 'diagnostics.json'
            if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
                return @{ Ok = $false; Detail = "缺少 $p" }
            }
            $j = Get-Content -LiteralPath $p -Raw -Encoding UTF8 | ConvertFrom-Json
            if (-not ($j.PSObject.Properties.Name -contains 'failed_prefabs')) {
                return @{ Ok = $false; Detail = "diagnostics.json 无 failed_prefabs 键（键: $($j.PSObject.Properties.Name -join ', ')）" }
            }
            $n = @($j.failed_prefabs).Count
            Write-Log "  failed_prefabs: $n"
            if ($n -ne 0) { return @{ Ok = $false; Detail = "failed_prefabs = $n（要求 0）" } }
            return @{ Ok = $true; Detail = '' }
        })
}

# ─────────────────────────────────────────────────────────────────────────────
# P2 套件
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-P2Suite {
    param([hashtable]$Cfg)
    Write-Rule 'P2 Regression Suite（P1 链 + cargo 门 + dataset smoke + route regression + match replay + signal link + perf smoke）'
    $dataset = $Cfg.Dataset.Path
    $trace = $Cfg.Trace

    # [1] P1 链
    if (-not $Cfg.P1Done) {
        Invoke-P1Suite -Cfg $Cfg
        $Cfg.P1Done = $true
    }
    [void](Add-ChainResult -Suite P2 -ChainName 'chain-p1' -ChainedSuite P1)

    $cargo = Resolve-Tool 'cargo'
    $navCore = Join-Path $script:RepoRoot 'nav-core'

    # [2][3][4] cargo 门：真实 exit code 为准
    [void](Invoke-NativeStep -Suite P2 -Name 'cargo-fmt' -FilePath $cargo `
        -Arguments @('fmt', '--check') -WorkingDirectory $navCore)
    [void](Invoke-NativeStep -Suite P2 -Name 'cargo-clippy' -FilePath $cargo `
        -Arguments @('clippy', '--all-targets', '--', '-D', 'warnings') -WorkingDirectory $navCore)
    [void](Invoke-NativeStep -Suite P2 -Name 'cargo-test' -FilePath $cargo `
        -Arguments @('test') -WorkingDirectory $navCore `
        -SemanticText 'exit 0 且 stdout 至少出现一次 "test result: ok."，且无 "test result: FAILED"' `
        -Semantic {
            param($out, $err)
            $okCount = ([regex]::Matches($out, 'test result: ok\.')).Count
            Write-Log "  test result: ok. x$okCount"
            Test-All @(
                @{ Ok = ($okCount -ge 1); Detail = 'stdout 中没有任何 "test result: ok."，测试可能未真正执行' },
                @{ Ok = (-not [regex]::IsMatch($out, 'test result: FAILED')); Detail = 'stdout 含 "test result: FAILED"' }
            )
        })

    # [5] dataset v2 smoke：显式构建后执行（不在 nav-core workspace 内）
    $smokeManifest = Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/Cargo.toml'
    $smokeExe = Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/target/release/dataset-reader-smoke.exe'
    $smokeRoots = Get-RustSourceRoots -WorkspaceManifest $smokeManifest -PackageName 'dataset-reader-smoke' `
        -FallbackRoot (Join-Path $script:RepoRoot 'tools/dataset-reader-smoke')
    $r = Invoke-NativeStep -Suite P2 -Name 'build-dataset-reader' -FilePath $cargo `
        -Arguments @('build', '--release', '--manifest-path', $smokeManifest) `
        -WorkingDirectory $script:RepoRoot
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P2 -Name 'build-dataset-reader-artifact' -Artifact $smokeExe `
            -SourceRoots $smokeRoots)
    }
    [void](Invoke-NativeStep -Suite P2 -Name 'dataset-smoke' -FilePath $smokeExe -Arguments @($dataset))

    # [6] 构建 nav-core-cli 后执行（release 产物必须本轮构建）
    $navManifest = Join-Path $navCore 'Cargo.toml'
    $cliExe = Join-Path $navCore 'target/release/nav-core-cli.exe'
    $cliRoots = Get-RustSourceRoots -WorkspaceManifest $navManifest -PackageName 'nav-core-cli' `
        -FallbackRoot (Join-Path $navCore 'tools/nav-core-cli')
    $r = Invoke-NativeStep -Suite P2 -Name 'build-nav-core-cli' -FilePath $cargo `
        -Arguments @('build', '--release', '-p', 'nav-core-cli') -WorkingDirectory $navCore
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P2 -Name 'build-nav-core-cli-artifact' -Artifact $cliExe `
            -SourceRoots $cliRoots)
    }

    # [7] 合成 trace：每轮现场生成到本次运行的临时工作区（永不复用 %TEMP%\real.navtrace）
    [void](Invoke-NativeStep -Suite P2 -Name 'syntrace' -FilePath $cliExe `
        -Arguments @('syntrace', '-58456,32832:-52925,36510', $dataset, $trace) `
        -SemanticText 'exit 0 且 stdout 含 "SYNTRACE OK"' `
        -Semantic { param($out, $err) Test-Contains $out 'SYNTRACE OK' 'syntrace 结论行' })
    [void](Invoke-HarnessCheck -Suite P2 -Name 'trace-freshness' `
        -Description 'trace 位于本次运行的临时工作区内、非空、且命名为 synthetic.navtrace' -Check {
            if (-not (Test-Path -LiteralPath $trace -PathType Leaf)) {
                return @{ Ok = $false; Detail = "trace 不存在: $trace" }
            }
            if (-not $trace.StartsWith($script:TempRoot, [StringComparison]::OrdinalIgnoreCase)) {
                return @{ Ok = $false; Detail = "trace 不在本次运行的临时工作区内: $trace" }
            }
            if ([System.IO.Path]::GetFileName($trace) -ne 'synthetic.navtrace') {
                return @{ Ok = $false; Detail = "trace 文件名应为 synthetic.navtrace，实际 $([System.IO.Path]::GetFileName($trace))" }
            }
            $len = (Get-Item -LiteralPath $trace).Length
            if ($len -le 0) { return @{ Ok = $false; Detail = 'trace 为空' } }
            Write-Log "  trace: $trace ($len bytes)"
            return @{ Ok = $true; Detail = '' }
        })

    # [8] 路线回归
    [void](Invoke-NativeStep -Suite P2 -Name 'route-regression' -FilePath $cliExe `
        -Arguments @('regression', $dataset) `
        -SemanticText 'exit 0 且 stdout 含 "P2-18 Regression PASS" 与 "不一致 0"' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out 'P2-18 Regression PASS' 'P2-18 结论行'),
                (Test-Match $out '不一致\s+0\b' '区域化回归不一致数为 0')
            )
        })

    # [9] map-match 回放
    [void](Invoke-NativeStep -Suite P2 -Name 'match-replay' -FilePath $cliExe `
        -Arguments @('match', $trace, $dataset) `
        -SemanticText 'exit 0 且 stdout 的 "匹配: HIGH N" 中 N >= 1' `
        -Semantic {
            param($out, $err)
            $hi = Get-CapturedGroup $out '匹配:\s*HIGH\s+(\d+)'
            if ($null -eq $hi) { return @{ Ok = $false; Detail = 'stdout 未出现 "匹配: HIGH N" 统计行' } }
            Write-Log "  HIGH frames: $hi"
            if ([int]$hi -lt 1) { return @{ Ok = $false; Detail = "HIGH 匹配帧数为 0（全帧未匹配）" } }
            return @{ Ok = $true; Detail = '' }
        })

    # [10] 信号关联
    [void](Invoke-NativeStep -Suite P2 -Name 'signal-link' -FilePath $cliExe `
        -Arguments @('signal', '-58456,32832:-58456,35000', $dataset) `
        -SemanticText 'exit 0 且 "共 N 个受控 movement" 中 N >= 1（区别「无受控 movement」分支）' `
        -Semantic {
            param($out, $err)
            $n = Get-CapturedGroup $out '共\s*(\d+)\s*个受控 movement'
            if ($null -eq $n) { return @{ Ok = $false; Detail = 'stdout 未出现 "共 N 个受控 movement" 统计行' } }
            Write-Log "  controlled movements: $n"
            if ([int]$n -lt 1) { return @{ Ok = $false; Detail = '受控 movement 数为 0' } }
            return @{ Ok = $true; Detail = '' }
        })

    # [11] 性能冒烟
    # 语义断言不只看 "Bench PASS"：该行在 bench 正常走完时必然打印，单靠它无法区分
    # 「所有测量段都执行了」与「提前返回」。因此同时要求各测量段的统计行存在。
    # 注意：这里刻意不断言墙钟阈值（p99<500ms / <10ms / <10us）。把墙钟阈值写成门会引入
    # 依赖机器的概率判定，与本批次「消除概率测试」的目标相反；阈值仍由 bench 自身打印，
    # 供人工与报告核对。
    [void](Invoke-NativeStep -Suite P2 -Name 'perf-smoke' -FilePath $cliExe `
        -Arguments @('bench', $dataset, '--trace', $trace) `
        -SemanticText 'exit 0 且 stdout 同时含 "Bench PASS"、路线统计行、匹配统计行、限速查询段、进程内存段' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out 'Bench PASS' 'bench 结论行'),
                (Test-Match $out '\[路线\]\s+\d+\s+条' '路线时延统计段'),
                (Test-Match $out '\[匹配\]\s+\d+\s+帧' '匹配 p99 统计段（需要 --trace 生效）'),
                (Test-Contains $out '[限速查询] 10000 次' '限速查询统计段'),
                (Test-Match $out '\[内存-进程\]\s+工作集\s+\d+MB' '进程工作集统计段')
            )
        })
}

# ─────────────────────────────────────────────────────────────────────────────
# P3 套件
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-P3Suite {
    param([hashtable]$Cfg)
    Write-Rule 'P3 Regression Suite（P2 链 + speed lookahead + camera verdict）'
    $dataset = $Cfg.Dataset.Path
    if (-not $Cfg.P2Done) {
        Invoke-P2Suite -Cfg $Cfg
        $Cfg.P2Done = $true
    }
    [void](Add-ChainResult -Suite P3 -ChainName 'chain-p2' -ChainedSuite P2)

    $navCore = Join-Path $script:RepoRoot 'nav-core'
    $cliExe = Join-Path $navCore 'target/release/nav-core-cli.exe'

    [void](Invoke-NativeStep -Suite P3 -Name 'speed-lookahead' -FilePath $cliExe `
        -Arguments @('speed', '-58456,32832:-52925,36510', $dataset, '3000') `
        -SemanticText 'exit 0 且 stdout 含数据集金值 "breaks=2"' `
        -Semantic { param($out, $err) Test-Contains $out 'breaks=2 (machine-readable)' 'speed 断点金值' })

    $install = $Cfg.Ets2Install.Path
    $dotnet = Resolve-Tool 'dotnet'
    $probeCsproj = Join-Path $script:RepoRoot 'tools/camera-probe/CameraProbe/CameraProbe.csproj'
    [void](Invoke-NativeStep -Suite P3 -Name 'camera-verdict' -FilePath $dotnet `
        -Arguments @('run', '-c', 'Release', '--project', $probeCsproj, '--', '--install', $install) `
        -SemanticText 'exit 0 且 stdout 恰含 "VERDICT=NO-GO (machine-readable)"' `
        -Semantic {
            param($out, $err)
            $vs = [regex]::Matches($out, 'VERDICT=(\S+)')
            if ($vs.Count -eq 0) { return @{ Ok = $false; Detail = 'stdout 未出现 VERDICT 行' } }
            Write-Log "  VERDICT: $($vs[0].Value)"
            if ($vs.Count -ne 1) { return @{ Ok = $false; Detail = "VERDICT 行出现 $($vs.Count) 次（应为 1）" } }
            return (Test-Contains $out 'VERDICT=NO-GO (machine-readable)' 'camera-probe 结论值')
        })
}

# ─────────────────────────────────────────────────────────────────────────────
# P5 套件
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-P5Suite {
    param([hashtable]$Cfg)
    Write-Rule 'P5 Regression Suite（od-corpus 门 + 基准 diff + 随机 OD 检查）'
    $dataset = $Cfg.Dataset.Path
    $baseline = $Cfg.OdBaseline.Path
    $cargo = Resolve-Tool 'cargo'
    $navCore = Join-Path $script:RepoRoot 'nav-core'

    [void](Invoke-NativeStep -Suite P5 -Name 'cargo-test-od-corpus' -FilePath $cargo `
        -Arguments @('test', '-p', 'od-corpus') -WorkingDirectory $navCore `
        -SemanticText 'exit 0 且 stdout 至少出现一次 "test result: ok."' `
        -Semantic {
            param($out, $err)
            $okCount = ([regex]::Matches($out, 'test result: ok\.')).Count
            Write-Log "  test result: ok. x$okCount"
            if ($okCount -lt 1) { return @{ Ok = $false; Detail = 'stdout 中没有任何 "test result: ok."' } }
            return @{ Ok = $true; Detail = '' }
        })
    [void](Invoke-NativeStep -Suite P5 -Name 'cargo-test-nav-router' -FilePath $cargo `
        -Arguments @('test', '-p', 'nav-router') -WorkingDirectory $navCore `
        -SemanticText 'exit 0 且 stdout 至少出现一次 "test result: ok."' `
        -Semantic {
            param($out, $err)
            $okCount = ([regex]::Matches($out, 'test result: ok\.')).Count
            Write-Log "  test result: ok. x$okCount"
            if ($okCount -lt 1) { return @{ Ok = $false; Detail = 'stdout 中没有任何 "test result: ok."' } }
            return @{ Ok = $true; Detail = '' }
        })
    [void](Invoke-NativeStep -Suite P5 -Name 'cargo-fmt-od-corpus' -FilePath $cargo `
        -Arguments @('fmt', '-p', 'od-corpus', '--check') -WorkingDirectory $navCore)
    [void](Invoke-NativeStep -Suite P5 -Name 'cargo-clippy-od-corpus' -FilePath $cargo `
        -Arguments @('clippy', '-p', 'od-corpus', '--all-targets', '--', '-D', 'warnings') `
        -WorkingDirectory $navCore)

    $odExe = Join-Path $navCore 'target/release/od-corpus.exe'
    $odRoots = Get-RustSourceRoots -WorkspaceManifest (Join-Path $navCore 'Cargo.toml') `
        -PackageName 'od-corpus' -FallbackRoot (Join-Path $navCore 'tools/od-corpus')
    $r = Invoke-NativeStep -Suite P5 -Name 'build-od-corpus' -FilePath $cargo `
        -Arguments @('build', '--release', '-p', 'od-corpus') -WorkingDirectory $navCore
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P5 -Name 'build-od-corpus-artifact' -Artifact $odExe `
            -SourceRoots $odRoots)
    }

    # 基准语义断言：diff 必须为 0（基准绝不自动生成）
    [void](Invoke-NativeStep -Suite P5 -Name 'od-regress' -FilePath $odExe `
        -Arguments @('od-regress', $dataset, $baseline) `
        -SemanticText 'exit 0 且 stdout 含 "OD-REGRESS PASS" 与 "diff=0"' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out 'OD-REGRESS PASS' 'OD-REGRESS 结论行'),
                (Test-Match $out 'OD-REGRESS pairs=\d+ diff=0\b' 'diff 为 0')
            )
        })
    [void](Invoke-NativeStep -Suite P5 -Name 'od-check' -FilePath $odExe `
        -Arguments @('od-check', $dataset, '500') `
        -SemanticText 'exit 0 且 stdout 含 "OD-CHECK PASS"' `
        -Semantic { param($out, $err) Test-Contains $out 'OD-CHECK PASS' 'OD-CHECK 结论行' })
}

# ─────────────────────────────────────────────────────────────────────────────
# P5 基准维护模式（显式、独立；不是回归判据）
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-P5BaselineMaintenance {
    param([hashtable]$Cfg)
    Write-Rule 'P5 OD 基准维护（BASELINE MAINTENANCE —— 这是审查动作，不是测试）'
    Write-Log '本模式重新生成 OD 基准文件。它不构成任何回归判据：'
    Write-Log '  生成后与自身比较必然一致，因此「通过」在这里没有验证意义。'
    Write-Log '基准更新后必须人工审查 diff 并提交，才形成新的回归 oracle。'
    $dataset = $Cfg.Dataset.Path
    $target = $Cfg.OdBaseline.Path
    $exists = Test-Path -LiteralPath $target -PathType Leaf
    if ($exists -and -not $Force) {
        throw [PreconditionException]::new(
            "PRECONDITION FAIL: 基准已存在且未给出 -Force，拒绝覆盖: $target`n" +
            "  覆盖既有基准会丢弃历史 oracle，必须显式确认: -UpdateBaseline -Force")
    }
    $cargo = Resolve-Tool 'cargo'
    $navCore = Join-Path $script:RepoRoot 'nav-core'
    $odExe = Join-Path $navCore 'target/release/od-corpus.exe'
    $odRoots = Get-RustSourceRoots -WorkspaceManifest (Join-Path $navCore 'Cargo.toml') `
        -PackageName 'od-corpus' -FallbackRoot (Join-Path $navCore 'tools/od-corpus')
    $r = Invoke-NativeStep -Suite P5M -Name 'build-od-corpus' -FilePath $cargo `
        -Arguments @('build', '--release', '-p', 'od-corpus') -WorkingDirectory $navCore
    if ($r.Result -eq 'PASS') {
        [void](Assert-ArtifactFresh -Suite P5M -Name 'build-od-corpus-artifact' -Artifact $odExe `
            -SourceRoots $odRoots)
    }
    $staged = Join-Path $script:TempRoot 'od-baseline-candidate.txt'
    [void](Invoke-NativeStep -Suite P5M -Name 'od-baseline-generate' -FilePath $odExe `
        -Arguments @('od-baseline', $dataset, '--write', $staged) `
        -SemanticText 'exit 0 且 stdout 含 "OD-BASELINE PASS"' `
        -Semantic { param($out, $err) Test-Contains $out 'OD-BASELINE PASS' 'OD-BASELINE 结论行' })
    [void](Invoke-HarnessCheck -Suite P5M -Name 'baseline-diff' -Description '候选基准与现有基准的差异摘要' -Check {
            # 注意：不可写成 `$old = if (...) { @(...) } else { @() }`——语句作为表达式赋值时
            # PowerShell 会把单元素数组展开成标量，随后 $old.Count 在 StrictMode 下抛异常。
            # 这是 Batch 4 自检案例 4 实测到的 harness 缺陷（已修）。
            $new = @(Get-Content -LiteralPath $staged -Encoding UTF8 | Where-Object { $_.Trim() -ne '' })
            $old = @()
            if ($exists) {
                $old = @(Get-Content -LiteralPath $target -Encoding UTF8 | Where-Object { $_.Trim() -ne '' })
            }
            $added = @($new | Where-Object { $old -notcontains $_ })
            $removed = @($old | Where-Object { $new -notcontains $_ })
            Write-Log "  现有基准行数: $($old.Count)  候选基准行数: $($new.Count)"
            Write-Log "  新增/变化: $($added.Count)  移除/变化: $($removed.Count)"
            foreach ($l in ($added | Select-Object -First 10)) { Write-Log "    + $l" }
            foreach ($l in ($removed | Select-Object -First 10)) { Write-Log "    - $l" }
            return @{ Ok = $true; Detail = "added=$($added.Count) removed=$($removed.Count)" }
        })
    Copy-Item -LiteralPath $staged -Destination $target -Force
    # Copy-Item 会把目标文件的 mtime 设为源文件的 mtime（Batch 3 教训：mtime 回退会让构建系统复用陈旧产物）
    (Get-Item -LiteralPath $target).LastWriteTime = Get-Date
    Write-Log "基准已写入: $target"
    Write-Log '提醒：请审查上面的差异并提交该文件；提交后的基准才成为下一次回归的 oracle。'
}

# ─────────────────────────────────────────────────────────────────────────────
# SelfTest：H1–H6 假绿检测（fixture 驱动，不触碰真实产品代码）
# ─────────────────────────────────────────────────────────────────────────────
function Invoke-SelfTest {
    Write-Rule 'Harness SelfTest（H1–H6：证明 harness 抓得住假绿）'
    $cmd = Resolve-Tool 'cmd'
    $fx = Join-Path $script:RepoRoot 'scripts/harness-fixtures'
    $spaced = Join-Path $script:TempRoot 'space test dir'
    New-Item -ItemType Directory -Path $spaced -Force | Out-Null

    Write-Log "fixtures: $fx"
    Write-Log "spaced  : $spaced"
    Write-Log '探针步骤标记为 probe，不进入套件判定（它们的「预期失败」不是套件失败）。'

    # H1 非零 exit + 打印 PASS  → 必须 FAIL，且必须读到真实退出码 7
    $r1 = Invoke-NativeStep -Suite H1 -Name 'exit7-prints-pass' -FilePath $cmd `
        -Arguments @('/c', (Join-Path $fx 'h1-exit7-prints-pass.cmd')) `
        -ProbeOnly `
        -SemanticText 'exit 7 且 stdout 含 PASS' `
        -Semantic { param($out, $err) Test-Contains $out 'PASS' 'fixture 声称成功' }
    $h1 = (($r1.Result -eq 'TEST FAILURE') -and ($r1.ExitCode -eq 7))

    # H2 exit 0 + 语义 marker 缺失 → 必须 FAIL（exit 0 不能替代语义断言）
    $r2 = Invoke-NativeStep -Suite H2 -Name 'exit0-marker-absent' -FilePath $cmd `
        -Arguments @('/c', (Join-Path $fx 'h2-exit0-no-marker.cmd')) `
        -ProbeOnly `
        -SemanticText 'exit 0 且 stdout 含 REQUIRED-MARKER' `
        -Semantic { param($out, $err) Test-Contains $out 'REQUIRED-MARKER' '必需的业务 marker' }
    $h2 = (($r2.Result -eq 'TEST FAILURE') -and ($r2.ExitCode -eq 0) -and ($r2.SemanticPassed -eq $false))

    # H3 基准缺失 → PRECONDITION FAIL，且不得自动创建
    $ghost = Join-Path $spaced 'missing-baseline.txt'
    $h3ok = $false; $h3detail = ''
    try {
        [void](Get-OdBaselinePath -Explicit $ghost)
        $h3detail = 'Get-OdBaselinePath 未对缺失基准报错'
    } catch [PreconditionException] {
        $created = Test-Path -LiteralPath $ghost -PathType Leaf
        $h3ok = (-not $created)
        $h3detail = if ($created) { '缺失的基准被自动创建了' } else { 'PRECONDITION FAIL，且未创建文件（异常类型正确）' }
    }

    # H4 产物缺失 → 必须自动重建并可通过新鲜度检查，而不是 file-not-found
    $probe = Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/target/release/dataset-reader-smoke.exe'
    $h4ok = $false; $h4detail = ''
    if (Test-Path -LiteralPath $probe) {
        Remove-Item -LiteralPath $probe -Force
        $cargo = Resolve-Tool 'cargo'
        $r = Invoke-NativeStep -Suite H4 -Name 'rebuild-after-delete' -FilePath $cargo `
            -Arguments @('build', '--release', '--manifest-path',
                (Join-Path $script:RepoRoot 'tools/dataset-reader-smoke/Cargo.toml')) `
            -WorkingDirectory $script:RepoRoot -ProbeOnly
        if ($r.Result -eq 'PASS') {
            $a = Assert-ArtifactFresh -Suite H4 -Name 'rebuilt-artifact' -Artifact $probe `
                -SourceRoots @((Join-Path $script:RepoRoot 'tools/dataset-reader-smoke')) -ProbeOnly
            $h4ok = ($a.Result -eq 'PASS')
            $h4detail = if ($h4ok) { '产物被自动重建并通过新鲜度检查' } else { $a.Detail }
        } else {
            $h4detail = "重建失败 exit=$($r.ExitCode)"
        }
    } else {
        $h4detail = "产物预先不存在，无法验证删除后重建: $probe"
    }

    # H5 含空格路径：参数转义单元检查 + 真实夹具在带空格路径下运行
    $qcases = @(
        @{ In = 'abc';                          Want = 'abc' },
        @{ In = 'a b';                          Want = '"a b"' },
        @{ In = '';                             Want = '""' },
        @{ In = 'a"b';                          Want = '"a\"b"' },
        @{ In = 'a\';                           Want = 'a\' },
        @{ In = 'a b\';                         Want = '"a b\\"' },
        @{ In = 'a b"c';                        Want = '"a b\"c"' },
        @{ In = 'D:\ETS2 Test Data\europe-v5';  Want = '"D:\ETS2 Test Data\europe-v5"' }
    )
    $qbad = @()
    foreach ($c in $qcases) {
        $got = ConvertTo-CommandLineArgument $c.In
        if ($got -ne $c.Want) { $qbad += ("in=[{0}] want=[{1}] got=[{2}]" -f $c.In, $c.Want, $got) }
    }
    if ($qbad.Count -gt 0) {
        Write-Log '  引号规则不符:'
        foreach ($b in $qbad) { Write-Log "    $b" }
    }

    $spacedFixture = Join-Path $fx 'h5-echo-args.ps1'
    $spacedScript = Join-Path $spaced 'h5-spaced-fixture.ps1'
    Copy-Item -LiteralPath $spacedFixture -Destination $spacedScript -Force
    $r5 = Invoke-NativeStep -Suite H5 -Name 'path-with-spaces' -FilePath (Resolve-Tool 'powershell') `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $spacedScript,
                     'arg with spaces', 'second arg') `
        -WorkingDirectory $spaced -ProbeOnly `
        -SemanticText 'exit 0 且 stdout 逐字回显含空格的参数与工作目录' `
        -Semantic {
            param($out, $err)
            Test-All @(
                (Test-Contains $out '[arg with spaces]' '带空格参数原样传递'),
                (Test-Contains $out 'space test dir' '带空格工作目录')
            )
        }
    $h5 = ($qbad.Count -eq 0) -and ($r5.Result -eq 'PASS')
    $h5detail = if ($h5) { '引号规则 8/8，且带空格路径的夹具 PASS' } else { "引号不符 $($qbad.Count) 例；夹具判定 $($r5.Result): $($r5.Detail)" }

    # H6 错误的 dataset 路径 → preflight 快速失败，且不启动任何构建
    $h6ok = $false; $h6detail = ''
    $before = $script:Results.Count
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        [void](Get-DatasetPath -Explicit (Join-Path $spaced 'no-such-dataset'))
        $h6detail = 'Get-DatasetPath 未对错误路径报错'
    } catch [PreconditionException] {
        $sw.Stop()
        $noSteps = ($script:Results.Count -eq $before)
        $h6ok = $noSteps -and ($sw.Elapsed.TotalSeconds -lt 10)
        $h6detail = "PRECONDITION FAIL，耗时 $('{0:N2}s' -f $sw.Elapsed.TotalSeconds)，未启动任何步骤=$noSteps"
    }
    $h6 = $h6ok

    $checks = @(
        @{ Id = 'H1'; Ok = $h1; Expect = 'TEST FAILURE'; Detail = "exit=$($r1.ExitCode)（真实读到非零）且 stdout 含 PASS → 判定 $($r1.Result)" },
        @{ Id = 'H2'; Ok = $h2; Expect = 'TEST FAILURE'; Detail = "exit=$($r2.ExitCode) 且 marker 缺失 → 判定 $($r2.Result)，semantic=$($r2.SemanticPassed)" },
        @{ Id = 'H3'; Ok = $h3ok; Expect = 'PRECONDITION FAIL'; Detail = $h3detail },
        @{ Id = 'H4'; Ok = $h4ok; Expect = 'PASS (rebuild)'; Detail = $h4detail },
        @{ Id = 'H5'; Ok = $h5; Expect = 'PASS'; Detail = $h5detail },
        @{ Id = 'H6'; Ok = $h6; Expect = 'PRECONDITION FAIL (fast)'; Detail = $h6detail }
    )
    Write-Log ''
    Write-Log '  H#   期望                     实测'
    foreach ($c in $checks) {
        $mark = if ($c.Ok) { 'ok  ' } else { 'FAIL' }
        Write-Log ("  {0}  {1}  {2,-24} {3}" -f $c.Id, $mark, $c.Expect, $c.Detail)
    }
    [void](Invoke-HarnessCheck -Suite SelfTest -Name 'H1-H6' `
        -Description 'H1..H6 全部按预期判定' -Check {
            $bad = @($checks | Where-Object { -not $_.Ok } | ForEach-Object { $_.Id })
            if ($bad.Count -gt 0) { return @{ Ok = $false; Detail = "未按预期判定: $($bad -join ', ')" } }
            return @{ Ok = $true; Detail = '' }
        })
}

# ─────────────────────────────────────────────────────────────────────────────
# 主流程
# ─────────────────────────────────────────────────────────────────────────────
function Get-FailedSteps {
    return @($script:Results | Where-Object { $_.Result -ne 'PASS' })
}

function Write-Summary {
    param([string]$SuiteLabel)
    Write-Log ''
    Write-Rule "$SuiteLabel 步骤汇总"
    foreach ($r in $script:Results) {
        $exit = if ($null -eq $r.ExitCode) { 'n/a' } else { [string]$r.ExitCode }
        $sem = if ($null -eq $r.SemanticPassed) { '-' } else { if ($r.SemanticPassed) { 'ok' } else { 'FAIL' } }
        Write-Log ("  [{0}:{1}] {2} (exit={3}, semantic={4}, {5:N1}s)" -f `
                $r.Suite, $r.Name, $r.Result, $exit, $sem, $r.Duration)
        if ($r.Detail) { Write-Log "        $($r.Detail)" }
    }
    $failed = @(Get-FailedSteps)
    Write-Log ''
    if ($failed.Count -eq 0) {
        Write-Log "$SuiteLabel : PASS (steps=$($script:Results.Count))"
    } else {
        $names = ($failed | ForEach-Object { "[$($_.Suite):$($_.Name)]=$($_.Result)" }) -join ', '
        Write-Log "$SuiteLabel : FAIL"
        Write-Log "suite failed_steps = [$names]"
    }
}

function Get-ExitCode {
    $failed = @(Get-FailedSteps)
    if ($failed.Count -eq 0) { return $EXIT_PASS }
    if (@($failed | Where-Object { $_.HarnessFailure }).Count -gt 0) { return $EXIT_HARNESS }
    return $EXIT_TEST
}

# ── 启动 ────────────────────────────────────────────────────────────────────
Set-SessionTools
$script:TempRoot = New-RunTempRoot
$script:StepLogDir = Join-Path $script:TempRoot 'logs'
New-Item -ItemType Directory -Path $script:StepLogDir -Force | Out-Null
if ($LogDir) {
    New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
    $script:StepLogDir = (Resolve-Path -LiteralPath $LogDir).Path
}

Write-Rule 'ETS2Nav Regression Harness (Batch 4)'
Write-Log "repo root   : $($script:RepoRoot)"
Write-Log "temp root   : $($script:TempRoot)"
Write-Log "step logs   : $($script:StepLogDir)"
Write-Log "suite       : $($Suite -join ', ')"
Write-Log ("started     : {0}" -f $script:RunStartUtc.ToString('o'))
Write-Log "powershell  : $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition))"
Write-Log "step timeout: ${StepTimeoutSeconds}s"

$exit = $EXIT_PASS
try {
    if ($UpdateBaseline) {
        if ($Suite -contains 'All') {
            throw [HarnessException]::new(
                "HARNESS FAILURE: -UpdateBaseline 不能与 -Suite All 组合。" +
                "基准维护是独立的显式动作，不属于回归。请使用: -Suite P5 -UpdateBaseline")
        }
        $Cfg = @{
            Ets2Install   = $null
            Ets2Extracted = $null
            Dataset       = Get-DatasetPath -Explicit $Dataset
            OdBaseline    = Get-OdBaselinePath -Explicit $OdBaseline -AllowMissing
            Trace         = $null
        }
        Write-Log "dataset     : $($Cfg.Dataset.Path)  [$($Cfg.Dataset.Source)]"
        Write-Log "baseline    : $($Cfg.OdBaseline.Path)  [$($Cfg.OdBaseline.Source)]"
        Invoke-P5BaselineMaintenance -Cfg $Cfg
        Write-Summary 'P5 baseline maintenance'
        $exit = Get-ExitCode
    } elseif ($Suite -contains 'SelfTest') {
        Invoke-SelfTest
        Write-Summary 'Harness SelfTest'
        $exit = Get-ExitCode
    } else {
        $wantP1 = ($Suite -contains 'P1') -or ($Suite -contains 'All')
        $wantP2 = ($Suite -contains 'P2') -or ($Suite -contains 'All')
        $wantP3 = ($Suite -contains 'P3') -or ($Suite -contains 'All')
        $wantP5 = ($Suite -contains 'P5') -or ($Suite -contains 'All')
        if ($Suite -contains 'P2' -or $Suite -contains 'P3') { $wantP1 = $true }
        if ($Suite -contains 'P3') { $wantP2 = $true }
        if (-not ($wantP1 -or $wantP2 -or $wantP3 -or $wantP5)) {
            throw [HarnessException]::new('HARNESS FAILURE: 没有可执行的套件。')
        }

        # StrictMode 2.0 下访问不存在的 hashtable 键会抛异常，故所有键在此显式占位
        $Cfg = @{
            P1Done        = $false
            P2Done        = $false
            Ets2Install   = $null
            Ets2Extracted = $null
            Dataset       = $null
            OdBaseline    = $null
            Trace         = $null
        }

        # preflight：先验证本套件真正依赖的输入，缺什么立刻 PRECONDITION FAIL
        Write-Log ''
        Write-Log '--- preflight ---'
        if ($wantP1 -or $wantP3) {
            $Cfg.Ets2Install = Get-Ets2Install -Explicit $Ets2Install
            Write-Log "ets2 install: $($Cfg.Ets2Install.Path)  [$($Cfg.Ets2Install.Source)]"
            $Cfg.Ets2Extracted = Get-Ets2Extracted -Explicit $Ets2Extracted
            Write-Log "extracted   : $($Cfg.Ets2Extracted.Path)  [$($Cfg.Ets2Extracted.Source)]"
        }
        if ($wantP2 -or $wantP3 -or $wantP5) {
            $Cfg.Dataset = Get-DatasetPath -Explicit $Dataset
            Write-Log "dataset     : $($Cfg.Dataset.Path)  [$($Cfg.Dataset.Source)]"
        }
        if ($wantP5) {
            $Cfg.OdBaseline = Get-OdBaselinePath -Explicit $OdBaseline
            Write-Log "baseline    : $($Cfg.OdBaseline.Path)  [$($Cfg.OdBaseline.Source)]  pairs=$($Cfg.OdBaseline.Pairs)"
        }
        if ($wantP2 -or $wantP3) {
            if ($Trace) {
                if (-not (Test-Path -LiteralPath $Trace -PathType Leaf)) {
                    throw [PreconditionException]::new("PRECONDITION FAIL: -Trace '$Trace' 不存在。")
                }
                $Cfg.Trace = [System.IO.Path]::GetFullPath($Trace)
                Write-Log "trace       : $($Cfg.Trace)  [parameter]"
            } else {
                $Cfg.Trace = Join-Path $script:TempRoot 'synthetic.navtrace'
                Write-Log "trace       : $($Cfg.Trace)  [generated this run]"
            }
        }
        foreach ($t in @('dotnet', 'cargo')) {
            $p = Resolve-Tool $t
            Write-Log ("tool {0,-7}: {1}" -f $t, $p)
        }
        # 把解析后的输入导出为环境变量供子进程使用。两个理由：
        #   1. map-compiler 的集成测试（dotnet test）按同一套约定解析路径，见
        #      map-compiler/tests/SharedTestPaths/TestPaths.cs；
        #   2. 子进程看到的就是本次实际使用的输入，避免「参数传了一个、环境变量里是另一个」。
        if ($Cfg.Ets2Install) { $env:ETS2_INSTALL = $Cfg.Ets2Install.Path }
        if ($Cfg.Ets2Extracted) { $env:ETS2NAV_EXTRACTED = $Cfg.Ets2Extracted.Path }
        if ($Cfg.Dataset) { $env:ETS2NAV_DATASET = $Cfg.Dataset.Path }
        if ($Cfg.OdBaseline) { $env:ETS2NAV_OD_BASELINE = $Cfg.OdBaseline.Path }
        Write-Log '--- preflight OK ---'

        if ($wantP1 -and -not $Cfg.P1Done) {
            Invoke-P1Suite -Cfg $Cfg
            $Cfg.P1Done = $true
        }
        if ($wantP2 -and -not $Cfg.P2Done) {
            Invoke-P2Suite -Cfg $Cfg
            $Cfg.P2Done = $true
        }
        if ($wantP3) { Invoke-P3Suite -Cfg $Cfg }
        if ($wantP5) { Invoke-P5Suite -Cfg $Cfg }

        Write-Summary ($Suite -join '+')
        $exit = Get-ExitCode
    }
} catch [PreconditionException] {
    Write-Log ''
    Write-Log $_.Exception.Message
    Write-Log ''
    Write-Log 'PRECONDITION FAILURE —— 外部输入缺失或不可用；这不是产品测试失败。'
    $exit = $EXIT_PRECONDITION
} catch [HarnessException] {
    Write-Log ''
    Write-Log $_.Exception.Message
    Write-Log ''
    Write-Log 'HARNESS FAILURE —— harness 自身无法可信执行。'
    $exit = $EXIT_HARNESS
} catch {
    Write-Log ''
    Write-Log "HARNESS FAILURE: 未预期的异常: $($_.Exception.GetType().Name): $($_.Exception.Message)"
    Write-Log $_.ScriptStackTrace
    $exit = $EXIT_HARNESS
}

# 临时工作区清理：成功且未要求保留时删除；失败时保留并打印位置
$failed = @(Get-FailedSteps)
$keep = $KeepTemp -or ($failed.Count -gt 0) -or ($exit -ne $EXIT_PASS)
if (-not $keep) {
    try { Remove-Item -LiteralPath $script:TempRoot -Recurse -Force } catch { }
} else {
    Write-Log ''
    Write-Log "临时工作区保留: $($script:TempRoot)"
    Write-Log "  步骤日志: $($script:StepLogDir)"
}

$elapsed = ([datetime]::UtcNow - $script:RunStartUtc).TotalSeconds
Write-Log ''
Write-Log ("exit {0}   (0=PASS 1=TEST FAILURE 3=PRECONDITION FAILURE 4=HARNESS FAILURE)" -f $exit)
Write-Log ("duration {0:N1}s" -f $elapsed)
exit $exit
