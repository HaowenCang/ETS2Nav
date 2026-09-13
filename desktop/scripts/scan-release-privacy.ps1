#Requires -Version 5.1
<#
.SYNOPSIS
  发布产物隐私扫描（P4R 发布工程 §5、§29）。

.DESCRIPTION
  对「bundle 暂存目录」或「解压后的产物树」做关键词级隐私扫描，逐条报告文件、行号与
  上下文，并对每条命中给出分类：

    FAIL    未经人工分类的命中——存在任一条时脚本以退出码 1 结束
    benign  命中被 desktop/scripts/privacy-allowlist.json 显式豁免（必须附真实理由）
    REVIEW  本身不构成失败、但需要人工分类的类别（例如文档中作为默认值出现的开发端口）

  文本扫描范围按**扩展名白名单**确定：.json .txt .html .js .mjs .css .md .ps1 .bat
  .cmd .yml .yaml .xml .svg .ini .cfg .toml。二进制与大型数据集文件（routing.graph /
  junction.graph / map.db / search.db / *.exe / *.dll / *.pmtiles / 字形 *.pbf）
  **按设计不进入文本扫描**：对这类文件做关键词搜索既给不出可靠的行级定位，也会在大文件上
  付出不可接受的代价；它们的完整性由发布层 SHA-256 与 verify-bundle.ps1 的树摘要承担，
  而不是靠关键词断言。无扩展名文件（例如 LICENSE）同样不进入文本扫描。

  两条刻意保留的判据冲突及其处理：

    * 驱动器绝对路径检测**排除 URL 令牌**。`http://`、`https://` 不是本地路径，且
      `https://` 里的 `s:/` 会骗过朴素正则 `[A-Za-z]:[\\/]`，因此每个模式类都显式声明
      是否先掩掉 URL 令牌。URL 内部出现的用户目录（如
      `https://example.com/?next=C:/Users/bob`）仍由 user-profile-path 类捕获——该类的
      掩码开关是关闭的，URL 不能成为泄漏的藏身处。
    * 开发端口字面量（如 `127.0.0.1:8123`）只记为 REVIEW。它在文档里是合法默认值，
      但需要人工确认它不是某个真实会话的残留。

  分类是强制环节：「零命中」不是唯一逻辑。命中必须逐条落到 FAIL / benign / REVIEW，
  benign 只能来自允许清单中「具体文件 + 具体模式类 + 真实理由」的条目。

  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。Windows PowerShell 5.1 会把无 BOM 的
  .ps1 按 ANSI 解码，最坏情况会吞掉换行并把下一条语句变成注释。

.PARAMETER Target
  待扫描的目录（bundle 暂存目录，或解压后的产物树；两者皆可）。

.PARAMETER SelfTest
  自检模式：构造临时样本树，逐个断言每个模式类仍能捕获正样本、每个负样本不被判为 FAIL，
  并断言允许清单分类路径有效。自检失败即退出码 1——它真的会因为某个模式失效而失败。

.PARAMETER AllowlistPath
  允许清单路径。缺省为脚本同目录的 privacy-allowlist.json。

.PARAMETER RepoRoot
  本仓库根目录，用于生成 repo-absolute-path 类（泄漏开发机仓库绝对路径）。缺省由脚本位置推导。

.PARAMETER WorkDir
  自检模式的临时工作目录。缺省为 %TEMP%\ets2nav-privacy-selftest。

.EXITCODES
  0 无未分类 FAIL；1 存在未分类 FAIL 或自检断言失败；3 前置条件（目标缺失、允许清单非法）；
  4 harness 失败（未预期的异常）。
#>
[CmdletBinding(DefaultParameterSetName = 'Scan')]
param(
    [Parameter(ParameterSetName = 'Scan', Mandatory, Position = 0)]
    [string]$Target,

    [Parameter(ParameterSetName = 'SelfTest', Mandatory)]
    [switch]$SelfTest,

    [string]$AllowlistPath,
    [string]$RepoRoot,
    [string]$WorkDir
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$script:TextExtensions = @(
    '.json', '.txt', '.html', '.js', '.mjs', '.css', '.md', '.ps1',
    '.bat', '.cmd', '.yml', '.yaml', '.xml', '.svg', '.ini', '.cfg', '.toml'
)

if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }
if (-not $AllowlistPath) { $AllowlistPath = Join-Path $PSScriptRoot 'privacy-allowlist.json' }
if (-not $WorkDir) { $WorkDir = Join-Path $env:TEMP 'ets2nav-privacy-selftest' }

# 路径分隔符：允许单反斜杠、双反斜杠（JSON 转义形态）与前斜杠三种写法。
$SEP = '[\\/]{1,2}'

function Get-PrivacyClasses {
    param([Parameter(Mandatory)][string]$RepoRootPath)

    # 仓库绝对路径的三种字面形态：原生、正斜杠、JSON 转义（反斜杠翻倍）。
    $litNative = $RepoRootPath
    $litFwd = $RepoRootPath -replace '\\', '/'
    $litJson = $RepoRootPath -replace '\\', '\\'
    $repoRe = '(?:' + [regex]::Escape($litNative) + '|' + [regex]::Escape($litFwd) + '|' + [regex]::Escape($litJson) + ')'

    return @(
        [pscustomobject]@{
            Id = 'drive-absolute-path'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![A-Za-z0-9_.\-])[A-Za-z]:' + $SEP + '(?![\\/])'
            Note = 'Windows 驱动器绝对路径'
        }
        [pscustomobject]@{
            Id = 'user-profile-path'; Verdict = 'FAIL'; MaskUrls = $false
            Regex = '(?<![A-Za-z0-9_.\-])[A-Za-z]:' + $SEP + 'Users' + $SEP + '|\\\\Users\\'
            Note = '用户目录（含 URL 内部出现的情形）'
        }
        [pscustomobject]@{
            Id = 'home-directory-path'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![\w.:\-])/(?:home|Users)/[A-Za-z0-9._\-]+'
            Note = '类 Unix 用户主目录'
        }
        [pscustomobject]@{
            Id = 'repo-absolute-path'; Verdict = 'FAIL'; MaskUrls = $false
            Regex = $repoRe
            Note = '本仓库开发机绝对路径'
        }
        [pscustomobject]@{
            Id = 'build-artifact-residue'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![\w.\-])(?:target|playwright-report|test-results)' + $SEP +
                    '|(?<![\w.\-])\.git' + $SEP + '|[A-Za-z0-9_.\-]+\.pdb(?![A-Za-z0-9_])'
            Note = '构建残留：target/、.git/、*.pdb、playwright-report/、test-results/'
        }
        [pscustomobject]@{
            Id = 'node-modules-reference'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![\w.\-])node_modules' + $SEP
            Note = '构建残留：node_modules/ 引用（独立成类，使第三方许可声明的出处标注可与真正的目录泄漏分开分类）'
        }
        [pscustomobject]@{
            Id = 'game-archive-name'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![\w.\-])[A-Za-z0-9_][A-Za-z0-9_.\-]*\.scs(?![A-Za-z0-9_])'
            Note = '源游戏归档名（.scs）'
        }
        [pscustomobject]@{
            Id = 'sdk-archive-reference'; Verdict = 'FAIL'; MaskUrls = $true
            Regex = '(?<![\w.\-])scs_sdk_1_14(?![A-Za-z0-9_])'
            Note = 'SCS SDK 归档/目录引用（vendor/scs_sdk_1_14.zip）'
        }
        [pscustomobject]@{
            Id = 'bearer-token'; Verdict = 'FAIL'; MaskUrls = $false
            Regex = '(?i)\bBearer[ \t]+(?<![0-9A-Fa-f])[0-9A-Fa-f]{64}(?![0-9A-Fa-f])'
            Note = '会话令牌：Bearer <64 位十六进制>'
        }
        [pscustomobject]@{
            Id = 'token-literal'; Verdict = 'FAIL'; MaskUrls = $false
            Regex = '(?i)\b(?:token|access_token|auth_token|session_token|api_key|apikey|secret)' +
                    '["'']?[ \t]*[=:][ \t]*["'']?(?<![0-9A-Fa-f])[0-9A-Fa-f]{64}(?![0-9A-Fa-f])'
            Note = '会话令牌：token=<64 位十六进制> 及同形赋值'
        }
        [pscustomobject]@{
            Id = 'hex-near-token'; Verdict = 'FAIL'; MaskUrls = $false
            Regex = '(?i)token[^\r\n]{0,120}?(?<![0-9A-Fa-f])[0-9A-Fa-f]{64}(?![0-9A-Fa-f])' +
                    '|(?<![0-9A-Fa-f])[0-9A-Fa-f]{64}(?![0-9A-Fa-f])[^\r\n]{0,120}?token'
            Note = '紧邻 token 一词的 64 位十六进制串（同窗口 120 字符内）'
        }
        [pscustomobject]@{
            Id = 'dev-port-literal'; Verdict = 'REVIEW'; MaskUrls = $false
            Regex = '(?i)(?:\b(?:127\.0\.0\.1|localhost|0\.0\.0\.0|\[::1\]):(?:8123|5173|4173|1420|3000|8080)\b' +
                    '|\bport[ \t]*[=:][ \t]*["'']?(?:8123|5173|4173|1420|3000|8080)["'']?)'
            Note = '开发端口字面量：文档中的合法默认值亦落此类，须人工确认'
        }
    )
}

function Test-TextFile {
    param([Parameter(Mandatory)][string]$Path)
    $ext = [System.IO.Path]::GetExtension($Path)
    if (-not $ext) { return $false }
    return ($script:TextExtensions -contains $ext.ToLowerInvariant())
}

function ConvertTo-GlobRegex {
    param([Parameter(Mandatory)][string]$Glob)
    $g = $Glob -replace '\\', '/'
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('^')
    $i = 0
    while ($i -lt $g.Length) {
        $ch = $g[$i]
        if ($ch -eq '*') {
            if (($i + 1) -lt $g.Length -and $g[$i + 1] -eq '*') { [void]$sb.Append('.*'); $i++ }
            else { [void]$sb.Append('[^/]*') }
        } elseif ($ch -eq '?') {
            [void]$sb.Append('[^/]')
        } else {
            [void]$sb.Append([regex]::Escape([string]$ch))
        }
        $i++
    }
    [void]$sb.Append('$')
    return $sb.ToString()
}

function Get-PrivacyAllowlist {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string[]]$KnownClasses)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Write-Host "PRECONDITION FAILURE: 允许清单不存在: $Path"
        exit 3
    }
    try {
        $doc = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    } catch {
        Write-Host "PRECONDITION FAILURE: 允许清单不是合法 JSON: $Path — $($_.Exception.Message)"
        exit 3
    }
    $entries = @()
    if ($null -ne $doc.PSObject.Properties['entries']) { $entries = @($doc.entries) }
    $out = New-Object System.Collections.ArrayList
    $wholeTreeGlobs = @('', '*', '**', '**/*', '*/*', '*.*', './*')
    foreach ($e in $entries) {
        foreach ($k in @('file', 'pattern', 'reason')) {
            if ($null -eq $e.PSObject.Properties[$k] -or [string]::IsNullOrWhiteSpace([string]$e.$k)) {
                Write-Host "PRECONDITION FAILURE: 允许清单条目缺少 $k 字段: $($e | ConvertTo-Json -Compress)"
                exit 3
            }
        }
        $glob = ([string]$e.file).Trim()
        if ($wholeTreeGlobs -contains $glob) {
            Write-Host "PRECONDITION FAILURE: 允许清单不得整体豁免整棵树（file='$glob'）"
            exit 3
        }
        $cls = ([string]$e.pattern).Trim()
        if ($cls -eq '*' -or $cls -eq '**') {
            Write-Host "PRECONDITION FAILURE: 允许清单不得整体豁免一个模式类（pattern='*'）"
            exit 3
        }
        if ($KnownClasses -notcontains $cls) {
            Write-Host "PRECONDITION FAILURE: 允许清单引用了未知模式类 '$cls'（已知：$($KnownClasses -join ', ')）"
            exit 3
        }
        if (([string]$e.reason).Trim().Length -lt 20) {
            Write-Host "PRECONDITION FAILURE: 允许清单条目理由过短，无法复核: $glob / $cls"
            exit 3
        }
        [void]$out.Add([pscustomobject]@{
            Glob = $glob
            Regex = (ConvertTo-GlobRegex -Glob $glob)
            Class = $cls
            Reason = ([string]$e.reason).Trim()
        })
    }
    return , $out
}

function Invoke-PrivacyScan {
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)]$Allowlist,
        [Parameter(Mandatory)]$Classes,
        [Parameter(Mandatory)][string]$RepoRootPath
    )

    $rootFull = (Resolve-Path -LiteralPath $Root).Path
    $files = New-Object System.Collections.ArrayList
    foreach ($f in (Get-ChildItem -LiteralPath $rootFull -Recurse -File -Force)) {
        if (Test-TextFile -Path $f.FullName) { [void]$files.Add($f) }
    }
    $relList = @($files | ForEach-Object { $_.FullName.Substring($rootFull.Length).TrimStart('\', '/') -replace '\\', '/' })
    [Array]::Sort($relList, [System.StringComparer]::Ordinal)

    $hits = New-Object System.Collections.ArrayList
    $urlRe = [regex]'(?i)\b[a-z][a-z0-9+.\-]*://\S+'
    foreach ($rel in $relList) {
        $full = Join-Path $rootFull ($rel -replace '/', '\')
        $text = [System.IO.File]::ReadAllText($full, [System.Text.Encoding]::UTF8)
        $lines = $text -split "`r?`n"
        for ($ln = 0; $ln -lt $lines.Length; $ln++) {
            $line = $lines[$ln]
            if ($line.Length -eq 0) { continue }
            $masked = $urlRe.Replace($line, ' ')
            foreach ($c in $Classes) {
                $subject = $line
                if ($c.MaskUrls) { $subject = $masked }
                foreach ($m in ([regex]::Matches($subject, $c.Regex))) {
                    $verdict = $c.Verdict
                    $reason = ''
                    foreach ($a in $Allowlist) {
                        if ($a.Class -ne $c.Id) { continue }
                        if ([regex]::IsMatch($rel, $a.Regex)) { $verdict = 'benign'; $reason = $a.Reason; break }
                    }
                    [void]$hits.Add([pscustomobject]@{
                        Class = $c.Id
                        Verdict = $verdict
                        File = $rel
                        Line = $ln + 1
                        Match = $m.Value
                        Text = $line.Trim()
                        Index = $m.Index
                        Reason = $reason
                        Note = $c.Note
                    })
                }
            }
        }
    }
    return [pscustomobject]@{
        Root = $rootFull
        Files = $relList.Count
        Hits = $hits
    }
}

function Write-ScanReport {
    param([Parameter(Mandatory)]$Result)
    $maxContext = 180
    foreach ($h in $Result.Hits) {
        $ctx = $h.Text
        if ($ctx.Length -gt $maxContext) { $ctx = $ctx.Substring(0, $maxContext) + ' …' }
        $tag = switch ($h.Verdict) {
            'FAIL' { '[FAIL]' }
            'benign' { '[benign]' }
            default { '[REVIEW]' }
        }
        Write-Host ("  {0} {1}  {2}:{3}" -f $tag, $h.Class, $h.File, $h.Line)
        Write-Host ("        | {0}" -f $ctx)
        Write-Host ("        | match: {0}" -f $h.Match)
        if ($h.Verdict -eq 'benign') { Write-Host ("        | reason: {0}" -f $h.Reason) }
        if ($h.Verdict -eq 'REVIEW') { Write-Host ("        | 说明: {0}" -f $h.Note) }
    }
}

function Invoke-SelfTest {
    $classes = Get-PrivacyClasses -RepoRootPath $RepoRoot
    $known = @($classes | ForEach-Object { $_.Id })

    $hex64a = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'
    $hex64b = '0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0'
    $repo = $RepoRoot

    # 正样本：每个模式类至少一条；Expect 为 FAIL:<class> 或 REVIEW:<class>。
    $cases = @(
        [pscustomobject]@{ File = 'pos/drive.txt'; Expect = 'FAIL:drive-absolute-path'
            Text = 'staging 目录 D:/builds/ets2nav/staging 由打包脚本写入' }
        [pscustomobject]@{ File = 'pos/user-forward.json'; Expect = 'FAIL:user-profile-path'
            Text = '{ "cache": "C:/Users/developer/AppData/Local/Temp/ets2nav" }' }
        [pscustomobject]@{ File = 'pos/user-jsonescaped.json'; Expect = 'FAIL:user-profile-path'
            Text = '{ "cache": "C:\\\\Users\\\\developer\\\\AppData" }' }
        [pscustomobject]@{ File = 'pos/user-unc.txt'; Expect = 'FAIL:user-profile-path'
            Text = '共享目录 \\Users\public\ets2nav 不应出现在产物中' }
        [pscustomobject]@{ File = 'pos/home.txt'; Expect = 'FAIL:home-directory-path'
            Text = 'built at /home/developer/ets2nav/dist/app.js' }
        [pscustomobject]@{ File = 'pos/url-user.txt'; Expect = 'FAIL:user-profile-path'
            Text = 'https://example.com/?next=C:/Users/bob/ets2nav' }
        [pscustomobject]@{ File = 'pos/repo.txt'; Expect = 'FAIL:repo-absolute-path'
            Text = ('日志: ' + $repo + '\desktop\target\release\ets2nav-desktop.exe') }
        [pscustomobject]@{ File = 'pos/repo-json.json'; Expect = 'FAIL:repo-absolute-path'
            Text = ('{ "root": "' + ($repo -replace '\\', '\\') + '\\\\desktop" }') }
        [pscustomobject]@{ File = 'pos/node-modules.txt'; Expect = 'FAIL:node-modules-reference'
            Text = 'copied node_modules/maplibre-gl/dist/maplibre-gl.js into web/vendor' }
        [pscustomobject]@{ File = 'pos/target.txt'; Expect = 'FAIL:build-artifact-residue'
            Text = 'staged from desktop\target\release\ets2nav-desktop.exe' }
        [pscustomobject]@{ File = 'pos/test-results.txt'; Expect = 'FAIL:build-artifact-residue'
            Text = 'artifacts written to test-results/run-1.json' }
        [pscustomobject]@{ File = 'pos/pdb.txt'; Expect = 'FAIL:build-artifact-residue'
            Text = 'symbols left behind: nav-core-cli.pdb' }
        [pscustomobject]@{ File = 'pos/git.txt'; Expect = 'FAIL:build-artifact-residue'
            Text = 'staged from .git\objects\pack' }
        [pscustomobject]@{ File = 'pos/archive.txt'; Expect = 'FAIL:game-archive-name'
            Text = 'sources: base.scs, dlc_balt.scs, dlc_iberia.scs' }
        [pscustomobject]@{ File = 'pos/sdk.txt'; Expect = 'FAIL:sdk-archive-reference'
            Text = 'precondition: vendor/scs_sdk_1_14.zip 必须存在' }
        [pscustomobject]@{ File = 'pos/bearer.txt'; Expect = 'FAIL:bearer-token'
            Text = ('Authorization: Bearer ' + $hex64a) }
        [pscustomobject]@{ File = 'pos/token-query.txt'; Expect = 'FAIL:token-literal'
            Text = ('ws://127.0.0.1:8123/ws?token=' + $hex64a) }
        [pscustomobject]@{ File = 'pos/token-json.json'; Expect = 'FAIL:token-literal'
            Text = ('{ "token": "' + $hex64b + '" }') }
        [pscustomobject]@{ File = 'pos/hex-near-token.js'; Expect = 'FAIL:hex-near-token'
            Text = ('const t = readSessionToken(); const fp = "' + $hex64a + '";') }
        [pscustomobject]@{ File = 'pos/port.md'; Expect = 'REVIEW:dev-port-literal'
            Text = '默认端口 127.0.0.1:8123，可用 --port 覆盖' }
    )

    # 负样本：Expect 为 none（任何 FAIL 都算自检失败）。
    $negatives = @(
        [pscustomobject]@{ File = 'neg/http.txt'; Expect = 'none'
            Text = 'see http://example.com/docs for the protocol description' }
        [pscustomobject]@{ File = 'neg/https.json'; Expect = 'none'
            Text = '{ "repository": "https://github.com/maplibre/maplibre-gl-js" }' }
        [pscustomobject]@{ File = 'neg/scheme-trap.txt'; Expect = 'none'
            Text = 'wss://s.example.com/stream and ftp://f.example.com/pub' }
        [pscustomobject]@{ File = 'neg/url-query-path.txt'; Expect = 'none'
            Text = 'https://example.com/?next=C:/path&depth=2' }
        [pscustomobject]@{ File = 'neg/hex-literal.js'; Expect = 'none'
            Text = 'const mask = 0x8123; const v = 0xdeadbeef; const u = 0x1234abcd;' }
        [pscustomobject]@{ File = 'neg/port-zero.txt'; Expect = 'none'
            Text = '生命周期用例传 --port=0，由系统分配空闲端口；不是文档默认值' }
    )

    if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
    $null = New-Item -ItemType Directory -Path $WorkDir -Force
    foreach ($c in ($cases + $negatives)) {
        $p = Join-Path $WorkDir ($c.File -replace '/', '\')
        $dir = Split-Path -Parent $p
        if (-not (Test-Path -LiteralPath $dir)) { $null = New-Item -ItemType Directory -Path $dir -Force }
        [System.IO.File]::WriteAllText($p, $c.Text + "`n", (New-Object System.Text.UTF8Encoding($false)))
    }

    # 允许清单路径的样本：同一份驱动器路径，一处被豁免、一处不被豁免。
    $allowedText = 'staging 目录 D:/builds/ets2nav/staging 由打包脚本写入'
    $null = New-Item -ItemType Directory -Path (Join-Path $WorkDir 'neg') -Force
    [System.IO.File]::WriteAllText((Join-Path $WorkDir 'neg\allowed.md'), $allowedText + "`n", (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText((Join-Path $WorkDir 'neg\not-allowed.md'), $allowedText + "`n", (New-Object System.Text.UTF8Encoding($false)))
    $tmpAllowPath = Join-Path $WorkDir 'selftest-allowlist.json'
    $tmpAllow = [ordered]@{
        schema  = 'ets2nav.privacy-allowlist/1'
        entries = @(
            [ordered]@{
                file    = 'neg/allowed.md'
                pattern = 'drive-absolute-path'
                reason  = '自检样本：该文件被显式豁免，用于验证 benign 分类路径本身有效'
            }
        )
    }
    [System.IO.File]::WriteAllText($tmpAllowPath, ($tmpAllow | ConvertTo-Json -Depth 6) + "`n", (New-Object System.Text.UTF8Encoding($false)))
    $allow = Get-PrivacyAllowlist -Path $tmpAllowPath -KnownClasses $known

    $result = Invoke-PrivacyScan -Root $WorkDir -Allowlist $allow -Classes $classes -RepoRootPath $RepoRoot

    $fail = New-Object System.Collections.ArrayList
    $posCaught = 0
    $negClean = 0
    Write-Host '=== 隐私扫描自检 ==='
    Write-Host "样本树 : $WorkDir"
    Write-Host ''
    foreach ($c in $cases) {
        $hits = @($result.Hits | Where-Object { $_.File -eq $c.File })
        $wantClass = $c.Expect.Substring($c.Expect.IndexOf(':') + 1)
        $wantVerdict = $c.Expect.Substring(0, $c.Expect.IndexOf(':'))
        $hit = @($hits | Where-Object { $_.Class -eq $wantClass -and $_.Verdict -eq $wantVerdict })
        if ($hit.Count -gt 0) {
            $posCaught++
            Write-Host ("  [PASS] 正样本 {0} → {1} (line {2})" -f $c.File, $c.Expect, $hit[0].Line)
        } else {
            [void]$fail.Add("正样本未被捕获: $($c.File) 期望 $($c.Expect) 实际 [$(($hits | ForEach-Object { "$($_.Class)/$($_.Verdict)" }) -join ', ')]")
            Write-Host ("  [FAIL] 正样本 {0} 期望 {1}，实际 [{2}]" -f $c.File, $c.Expect, (($hits | ForEach-Object { "$($_.Class)/$($_.Verdict)" }) -join ', '))
        }
    }
    foreach ($c in $negatives) {
        $bad = @($result.Hits | Where-Object { $_.File -eq $c.File -and $_.Verdict -eq 'FAIL' })
        if ($bad.Count -eq 0) {
            $negClean++
            Write-Host ("  [PASS] 负样本 {0} 未被判为 FAIL" -f $c.File)
        } else {
            [void]$fail.Add("负样本被误判为 FAIL: $($c.File) → $($bad | ForEach-Object { $_.Class })")
            Write-Host ("  [FAIL] 负样本 {0} 被误判为 FAIL: {1}" -f $c.File, (($bad | ForEach-Object { $_.Class }) -join ', '))
        }
    }

    $allowHits = @($result.Hits | Where-Object { $_.File -eq 'neg/allowed.md' })
    $allowOk = (@($allowHits | Where-Object { $_.Verdict -eq 'benign' -and $_.Reason.Length -ge 20 }).Count -gt 0)
    $notAllowHits = @($result.Hits | Where-Object { $_.File -eq 'neg/not-allowed.md' -and $_.Verdict -eq 'FAIL' })
    $notAllowOk = ($notAllowHits.Count -gt 0)
    if ($allowOk) { Write-Host '  [PASS] 允许清单命中被分类为 benign 并带理由' }
    else { [void]$fail.Add('允许清单命中未分类为 benign'); Write-Host '  [FAIL] 允许清单命中未分类为 benign' }
    if ($notAllowOk) { Write-Host '  [PASS] 未列入允许清单的同形样本仍为 FAIL（豁免是逐文件生效的）' }
    else { [void]$fail.Add('未列入允许清单的同形样本未被判为 FAIL'); Write-Host '  [FAIL] 未列入允许清单的同形样本未被判为 FAIL' }
    $allowCases = 2

    Write-Host ''
    Write-Host ("自检统计: 正样本 {0}/{1} 捕获；负样本 {2}/{3} 未被误判；允许清单用例 {4}/{4}" -f `
        $posCaught, $cases.Count, $negClean, $negatives.Count, $allowCases)
    if ($fail.Count -gt 0) {
        Write-Host ("PRIVACY SELF-TEST: FAIL ({0}) - {1}" -f $fail.Count, ($fail -join '; '))
        return 1
    }
    Write-Host ("PRIVACY SELF-TEST: PASS ({0} 项断言)" -f ($cases.Count + $negatives.Count + $allowCases))
    return 0
}

try {
    if ($SelfTest) {
        exit (Invoke-SelfTest)
    }

    if (-not (Test-Path -LiteralPath $Target -PathType Container)) {
        Write-Host "PRECONDITION FAILURE: 目标目录不存在: $Target"
        exit 3
    }
    $classes = Get-PrivacyClasses -RepoRootPath $RepoRoot
    $known = @($classes | ForEach-Object { $_.Id })
    $allow = Get-PrivacyAllowlist -Path $AllowlistPath -KnownClasses $known

    $result = Invoke-PrivacyScan -Root $Target -Allowlist $allow -Classes $classes -RepoRootPath $RepoRoot
    Write-Host '=== 发布产物隐私扫描（§5、§29）==='
    Write-Host "目标   : $($result.Root)"
    Write-Host "允许清单: $AllowlistPath（$($allow.Count) 条）"
    Write-Host "文本文件: $($result.Files) 个（扩展名白名单；二进制与数据集文件按设计不扫描）"
    Write-Host ''
    if ($result.Hits.Count -gt 0) { Write-ScanReport -Result $result; Write-Host '' }

    $fails = @($result.Hits | Where-Object { $_.Verdict -eq 'FAIL' })
    $benign = @($result.Hits | Where-Object { $_.Verdict -eq 'benign' })
    $review = @($result.Hits | Where-Object { $_.Verdict -eq 'REVIEW' })
    Write-Host ("命中统计: 合计 {0} = FAIL {1} + benign {2} + REVIEW {3}" -f `
        $result.Hits.Count, $fails.Count, $benign.Count, $review.Count)
    if ($fails.Count -gt 0) {
        Write-Host ("PRIVACY SCAN: FAIL ({0} 条未分类命中) - {1}" -f $fails.Count, (($fails | ForEach-Object { "$($_.Class)@$($_.File):$($_.Line)" }) -join '; '))
        exit 1
    }
    Write-Host ("PRIVACY SCAN: PASS（未分类命中 0；benign {0}；REVIEW {1} 待人工分类）" -f $benign.Count, $review.Count)
    exit 0
} catch {
    Write-Host ("HARNESS FAILURE: {0}" -f $_.Exception.Message)
    Write-Host $_.ScriptStackTrace
    exit 4
}
