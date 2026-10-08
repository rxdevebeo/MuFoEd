#requires -Version 5.1
<#
hostq.ps1 - the host job executor for StrictLib (strict-ooxml workspace).

Ported from David's tools/hostq (work order OPS.1). It is a tool, not an
agent. It watches a file queue, validates each job against a fixed
whitelist, runs the allowed kind, and writes a report. Anything outside the
whitelist is rejected without launching a process.

Run:
  powershell -ExecutionPolicy Bypass -File tools\hostq\hostq.ps1 `
      -Repo D:\projects\StrictLib [-AllowPush]

Self-test:
  powershell -ExecutionPolicy Bypass -File tools\hostq\hostq.ps1 `
      -Repo <any> -SelfTest

This file is kept ASCII-only on purpose: Windows PowerShell 5.1 reads a
BOM-less script in the ANSI code page. Do not add kinds or fields without
updating the whitelist table in README.md.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Repo,

    [switch]$AllowPush,

    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$script:StepCounter = 0
$script:Log = New-Object System.Collections.Generic.List[string]

# Every cargo call goes through this toolchain (CI pins 1.92.0).
$script:Toolchain = '+1.92.0'
$script:Python = 'python'
$script:QueueName = 'StrictLib-hostq'

# The gitignored corpora that a worktree does not have. Repo-relative.
$script:CorpusDirs = @(
    'strict-ooxml-core/tests/docx',
    'testdata/CC0',
    'testdata/CC0_DOCX',
    'testdata/CC0_DOCX_1'
)
# What census_gate.py reads besides the tracked tests/samples (its CORPORA).
$script:CensusCorpusDirs = @(
    'strict-ooxml-core/tests/docx',
    'testdata/CC0_DOCX'
)
$script:GateStepOrder = @('fmt', 'clippy', 'test', 'deny', 'doc')
$script:ScanSets = @('CC0', 'CC0_DOCX', 'CC0_DOCX_1')
$script:XsdGates = @('xsd', 'opc', 'census')
# Criterion benches a `bench` job may run, and the package each belongs to.
$script:Benches = [ordered]@{
    'editing'   = 'strict-ooxml-edit'
    'opc'       = 'strict-ooxml-core'
    'render'    = 'strict-ooxml-render-svg'
    'wml_parse' = 'strict-ooxml-wml'
    'xml_scan'  = 'strict-ooxml-core'
}

# ---------------------------------------------------------------------------
# Small utilities (UTF-8 without BOM; never the PS 5.1 default encoding).
# ---------------------------------------------------------------------------

function Get-Utf8NoBom {
    return (New-Object System.Text.UTF8Encoding($false))
}

function Write-Utf8NoBom {
    param([string]$Path, [string]$Text)
    [System.IO.File]::WriteAllText($Path, $Text, (Get-Utf8NoBom))
}

function Read-Utf8Text {
    param([string]$Path)
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    # Strip a UTF-8 BOM if a producer wrote one, so JSON still parses.
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        if ($bytes.Length -eq 3) { return '' }
        $bytes = $bytes[3..($bytes.Length - 1)]
    }
    return [System.Text.Encoding]::UTF8.GetString($bytes)
}

function Write-JsonNoBom {
    param([string]$Path, $Object)
    $json = ConvertTo-Json -InputObject $Object -Depth 24
    Write-Utf8NoBom -Path $Path -Text $json
}

function Get-Sha256String {
    param([string]$Text)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes([string]$Text)
        return (($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) -join '')
    }
    finally { $sha.Dispose() }
}

function Get-Sha256File {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return '' }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        return (($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) -join '')
    }
    finally { $stream.Dispose(); $sha.Dispose() }
}

# A repo-relative path ('a/b') under a root, normalized to the OS separator.
function Join-RepoPath {
    param([string]$Root, [string]$Relative)
    return [System.IO.Path]::GetFullPath((Join-Path $Root $Relative))
}

# Seconds left until a deadline, never below 30.
function Get-Budget {
    param([datetime]$Deadline)
    return [int][math]::Max(30, ($Deadline - (Get-Date)).TotalSeconds)
}

# Windows command-line quoting (the inverse of CommandLineToArgvW).
function Quote-Arg {
    param([string]$Arg)
    if ($Arg -eq '') { return '""' }
    if ($Arg -notmatch '[\s"]') { return $Arg }
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    $backslashes = 0
    foreach ($ch in $Arg.ToCharArray()) {
        if ($ch -eq '\') { $backslashes++; continue }
        if ($ch -eq '"') {
            [void]$sb.Append('\' * ($backslashes * 2 + 1))
            [void]$sb.Append('"')
            $backslashes = 0
        }
        else {
            [void]$sb.Append('\' * $backslashes)
            [void]$sb.Append($ch)
            $backslashes = 0
        }
    }
    [void]$sb.Append('\' * ($backslashes * 2))
    [void]$sb.Append('"')
    return $sb.ToString()
}

function Invoke-Git {
    param([string]$WorkDir, [string[]]$Arguments)
    $prevEap = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        # `git -C <dir>` rather than Set-Location: a missing directory must
        # fail loudly, never silently run git in the executor's own cwd (which
        # would let `clean -ffdx` eat the main copy's untracked corpora).
        $out = & git -C $WorkDir @Arguments 2>&1
        $code = $LASTEXITCODE
        $text = (($out | ForEach-Object { "$_" }) -join "`n")
        return [pscustomobject]@{ exit = $code; text = $text }
    }
    finally {
        $ErrorActionPreference = $prevEap
    }
}

# ---------------------------------------------------------------------------
# Logged process execution with a timeout that kills the whole process tree.
# ---------------------------------------------------------------------------

function Invoke-Logged {
    param(
        [string]$Name,
        [string]$Exe,
        [string[]]$Arguments,
        [string]$WorkDir,
        [int]$TimeoutSec,
        [hashtable]$Env,
        [string]$OutDir
    )
    $argStr = (($Arguments | ForEach-Object { Quote-Arg $_ }) -join ' ')
    $cmdDisplay = ($Exe + ' ' + $argStr).Trim()

    # ProcessStartInfo + async reads: PS 5.1's Start-Process -PassThru leaves
    # ExitCode null, and redirect-to-file loses output ordering. This reads
    # both streams without deadlocking and never lies about the exit code.
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    if ($argStr) { $psi.Arguments = $argStr }
    $psi.WorkingDirectory = $WorkDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true
    $utf8 = Get-Utf8NoBom
    $psi.StandardOutputEncoding = $utf8
    $psi.StandardErrorEncoding = $utf8

    if ($Env) {
        foreach ($key in $Env.Keys) {
            $psi.EnvironmentVariables[$key] = [string]$Env[$key]
        }
    }

    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $exit = -1
    $timedOut = $false
    $outText = ''
    $errText = ''
    try {
        $proc = [System.Diagnostics.Process]::Start($psi)
        $outTask = $proc.StandardOutput.ReadToEndAsync()
        $errTask = $proc.StandardError.ReadToEndAsync()
        if (-not $proc.WaitForExit($TimeoutSec * 1000)) {
            $timedOut = $true
            & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
            try { [void]$proc.WaitForExit(10000) } catch { }
            $exit = 124
        }
        else {
            $proc.WaitForExit()
            $exit = [int]$proc.ExitCode
        }
        try { [void]$outTask.Wait(5000); $outText = $outTask.Result } catch { }
        try { [void]$errTask.Wait(5000); $errText = $errTask.Result } catch { }
    }
    finally {
        $watch.Stop()
    }

    $combined = New-Object System.Collections.Generic.List[string]
    foreach ($line in ($outText -split "`r?`n")) { if ($line -ne '') { $combined.Add($line) } }
    foreach ($line in ($errText -split "`r?`n")) { if ($line -ne '') { $combined.Add($line) } }
    $tail = @()
    if ($combined.Count -gt 0) {
        $start = [math]::Max(0, $combined.Count - 40)
        for ($i = $start; $i -lt $combined.Count; $i++) { $tail += $combined[$i] }
    }

    $script:Log.Add(('$ ' + $cmdDisplay))
    $script:Log.Add(('  exit {0} in {1:N2}s{2}' -f $exit, $watch.Elapsed.TotalSeconds, $(if ($timedOut) { ' (TIMEOUT, tree killed)' } else { '' })))
    foreach ($line in ($outText -split "`r?`n")) { if ($line -ne '') { $script:Log.Add('  | ' + $line) } }
    foreach ($line in ($errText -split "`r?`n")) { if ($line -ne '') { $script:Log.Add('  ! ' + $line) } }

    return [pscustomobject]@{
        name     = $Name
        cmd      = $cmdDisplay
        exit     = $exit
        seconds  = [math]::Round($watch.Elapsed.TotalSeconds, 2)
        tail     = ($tail -join "`n")
        timedout = $timedOut
        stdout   = $outText
        stderr   = $errText
    }
}

# A step that did not launch a process (a probe, a skipped copy, a summary).
function New-InfoStep {
    param([string]$Name, [string]$Cmd, [int]$Exit, [string]$Tail)
    $script:Log.Add(('$ ' + $Cmd))
    foreach ($line in ($Tail -split "`r?`n")) { if ($line -ne '') { $script:Log.Add('  ' + $line) } }
    return [pscustomobject]@{ name = $Name; cmd = $Cmd; exit = $Exit; seconds = 0; tail = $Tail; timedout = $false; stdout = $Tail; stderr = '' }
}

# Invoke-Logged that turns "the program does not exist" into a failed step
# instead of an exception (ping probes optional tools).
function Invoke-Probe {
    param([string]$Name, [string]$Exe, [string[]]$Arguments, [string]$WorkDir, [int]$TimeoutSec, [string]$OutDir)
    try {
        return (Invoke-Logged -Name $Name -Exe $Exe -Arguments $Arguments -WorkDir $WorkDir -TimeoutSec $TimeoutSec -OutDir $OutDir)
    }
    catch {
        return (New-InfoStep -Name $Name -Cmd (($Exe + ' ' + ($Arguments -join ' ')).Trim()) -Exit 127 -Tail ('could not start: ' + $_.Exception.Message))
    }
}

# Projects an internal step onto the report schema:
# `name`, `cmd`, `exit`, `seconds`, `tail` and nothing else.
function ConvertTo-ReportStep {
    param($Step)
    return [ordered]@{
        name    = $Step.name
        cmd     = $Step.cmd
        exit    = $Step.exit
        seconds = $Step.seconds
        tail    = $Step.tail
    }
}

function New-Result {
    param([string]$Status, [string]$Reason, $Steps, $Artifacts)
    return [ordered]@{ status = $Status; reason = $Reason; steps = @($Steps); artifacts = @($Artifacts) }
}

# ok / failed / timeout from a list of steps.
function Get-StepsStatus {
    param($Steps)
    if (@($Steps | Where-Object { $_.timedout }).Count -gt 0) { return 'timeout' }
    if (@($Steps | Where-Object { $_.exit -ne 0 }).Count -gt 0) { return 'failed' }
    return 'ok'
}

# ---------------------------------------------------------------------------
# Main-copy state: HEAD, branch, status hash, corpus listing hash.
# ---------------------------------------------------------------------------

function Get-CorpusListingHash {
    param([string]$Repo)
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($rel in $script:CorpusDirs) {
        $root = Join-RepoPath -Root $Repo -Relative $rel
        if (-not (Test-Path -LiteralPath $root)) {
            $lines.Add(("{0}`tabsent" -f $rel))
            continue
        }
        $files = Get-ChildItem -LiteralPath $root -Recurse -File -Force | Sort-Object FullName
        foreach ($file in $files) {
            $sub = $file.FullName.Substring($root.Length).TrimStart('\', '/').Replace('\', '/')
            $lines.Add(("{0}/{1}`t{2}`t{3:o}" -f $rel, $sub, $file.Length, $file.LastWriteTimeUtc))
        }
    }
    return (Get-Sha256String ($lines -join "`n"))
}

function Get-MainState {
    param([string]$Repo)
    $head = (Invoke-Git -WorkDir $Repo -Arguments @('rev-parse', 'HEAD')).text.Trim()
    $branch = (Invoke-Git -WorkDir $Repo -Arguments @('rev-parse', '--abbrev-ref', 'HEAD')).text.Trim()
    $porcelain = (Invoke-Git -WorkDir $Repo -Arguments @('status', '--porcelain')).text
    $statusHash = Get-Sha256String $porcelain

    return [ordered]@{
        head          = $head
        branch        = $branch
        status_sha256 = $statusHash
        corpus_sha256 = (Get-CorpusListingHash -Repo $Repo)
    }
}

# ---------------------------------------------------------------------------
# Whitelist validation. Rejection must never launch a job process.
# ---------------------------------------------------------------------------

$script:AllowedFields = @{
    'ping'        = @('id', 'kind')
    'git-info'    = @('id', 'kind', 'rev', 'range')
    'gate'        = @('id', 'kind', 'sha', 'steps')
    'pixels'      = @('id', 'kind', 'sha')
    'xsd'         = @('id', 'kind', 'sha', 'gate')
    'corpus-scan' = @('id', 'kind', 'sha', 'set')
    'bench'       = @('id', 'kind', 'sha', 'bench')
    'wps'         = @('id', 'kind', 'sha')
    'commit'      = @('id', 'kind', 'branch', 'paths', 'message')
    'push'        = @('id', 'kind', 'branch')
}

function Test-SafeRef {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    if ($Value.StartsWith('-')) { return $false }
    return ($Value -match '^[A-Za-z0-9._/@{}^~:-]{1,200}$')
}

function Test-SafePath {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    if ($Value.StartsWith('-')) { return $false }
    if ($Value -match '\.\.') { return $false }
    if ($Value -match '[:<>"|?*]') { return $false }
    if ([System.IO.Path]::IsPathRooted($Value)) { return $false }
    return $true
}

function Test-JobRecord {
    param($Job, [string]$FileId, [string]$Repo, [switch]$AllowPush)

    $reject = { param($Reason) return [ordered]@{ ok = $false; reason = $Reason; full_sha = '' } }

    if ($null -eq $Job -or $Job -isnot [System.Management.Automation.PSCustomObject]) {
        return (& $reject 'job is not a JSON object')
    }

    $fields = @($Job.PSObject.Properties.Name)
    if ($fields -notcontains 'kind') { return (& $reject 'missing kind') }
    $kind = [string]$Job.kind
    if (@($script:AllowedFields.Keys) -cnotcontains $kind) {
        return (& $reject ("kind '{0}' is outside the whitelist" -f $kind))
    }

    $allowed = $script:AllowedFields[$kind]
    foreach ($field in $fields) {
        if ($allowed -cnotcontains $field) {
            return (& $reject ("field '{0}' is not allowed for kind '{1}'" -f $field, $kind))
        }
    }

    $id = $FileId
    if ($fields -contains 'id') {
        $id = [string]$Job.id
        if ($id -ne $FileId) { return (& $reject 'id does not match the file name') }
    }
    if ($id -notmatch '^\d{8}-\d{4}-[a-z0-9-]{1,32}$') {
        return (& $reject ("bad id '{0}'" -f $id))
    }

    $required = @()
    switch ($kind) {
        'git-info' { $required = @('rev') }
        'gate' { $required = @('sha') }
        'pixels' { $required = @('sha') }
        'xsd' { $required = @('sha', 'gate') }
        'corpus-scan' { $required = @('sha', 'set') }
        'bench' { $required = @('sha', 'bench') }
        'wps' { $required = @('sha') }
        'commit' { $required = @('branch', 'paths', 'message') }
        'push' { $required = @('branch') }
    }
    foreach ($name in $required) {
        if ($fields -notcontains $name -or $null -eq $Job.$name -or [string]::IsNullOrWhiteSpace([string]$Job.$name)) {
            return (& $reject ("missing required parameter '{0}'" -f $name))
        }
    }

    $fullSha = ''
    if ($fields -contains 'sha') {
        $sha = [string]$Job.sha
        if ($sha -notmatch '^[0-9a-f]{7,40}$') {
            return (& $reject ("sha '{0}' is not a hex commit id" -f $sha))
        }
        $resolved = Invoke-Git -WorkDir $Repo -Arguments @('rev-parse', '--verify', '--quiet', ($sha + '^{commit}'))
        if ($resolved.exit -ne 0 -or -not ($resolved.text.Trim() -match '^[0-9a-f]{40}$')) {
            return (& $reject ("sha '{0}' does not resolve to a commit" -f $sha))
        }
        $fullSha = $resolved.text.Trim()
    }

    switch ($kind) {
        'git-info' {
            if (-not (Test-SafeRef ([string]$Job.rev))) {
                return (& $reject 'rev is not a safe ref')
            }
            if ($fields -contains 'range' -and -not [string]::IsNullOrWhiteSpace([string]$Job.range)) {
                if (-not (Test-SafeRef ([string]$Job.range))) {
                    return (& $reject 'range is not a safe revision range')
                }
            }
        }
        'gate' {
            if ($fields -contains 'steps') {
                if ($null -eq $Job.steps -or $Job.steps -isnot [array]) {
                    return (& $reject 'steps must be an array')
                }
                $list = @($Job.steps)
                if ($list.Count -eq 0) { return (& $reject 'steps must not be empty') }
                $seen = @()
                foreach ($entry in $list) {
                    $text = [string]$entry
                    if ($entry -isnot [string] -or $script:GateStepOrder -cnotcontains $text) {
                        return (& $reject ("steps entry '{0}' is not fmt|clippy|test|deny|doc" -f $text))
                    }
                    if ($seen -ccontains $text) {
                        return (& $reject ("steps entry '{0}' is duplicated" -f $text))
                    }
                    $seen += $text
                }
            }
        }
        'xsd' {
            if ($Job.gate -isnot [string] -or $script:XsdGates -cnotcontains [string]$Job.gate) {
                return (& $reject ("gate '{0}' is not xsd|opc|census" -f $Job.gate))
            }
        }
        'bench' {
            if ($Job.bench -isnot [string] -or -not $script:Benches.Contains([string]$Job.bench)) {
                return (& $reject ("bench '{0}' is not {1}" -f $Job.bench, ($script:Benches.Keys -join '|')))
            }
        }
        'corpus-scan' {
            if ($Job.set -isnot [string] -or $script:ScanSets -cnotcontains [string]$Job.set) {
                return (& $reject ("set '{0}' is not CC0|CC0_DOCX|CC0_DOCX_1" -f $Job.set))
            }
        }
        'commit' {
            if (-not (Test-SafeRef ([string]$Job.branch))) {
                return (& $reject 'branch is not a safe ref')
            }
            $paths = @($Job.paths)
            if ($paths.Count -eq 0) { return (& $reject 'paths must not be empty') }
            foreach ($path in $paths) {
                $text = [string]$path
                if (-not (Test-SafePath $text)) {
                    return (& $reject ("path '{0}' is not a safe repo-relative path" -f $text))
                }
                $full = [System.IO.Path]::GetFullPath((Join-Path $Repo $text))
                $root = [System.IO.Path]::GetFullPath($Repo).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
                if (-not $full.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
                    return (& $reject ("path '{0}' escapes the repo" -f $text))
                }
            }
            $message = [string]$Job.message
            if (-not (Test-SafePath $message)) {
                return (& $reject 'message must be a repo-relative file path')
            }
            $messageFile = [System.IO.Path]::GetFullPath((Join-Path $Repo $message))
            if (-not (Test-Path -LiteralPath $messageFile -PathType Leaf)) {
                return (& $reject ("message file '{0}' does not exist" -f $message))
            }
        }
        'push' {
            if (-not $AllowPush) { return (& $reject 'push requires -AllowPush') }
            $branch = [string]$Job.branch
            if ($branch -eq 'master' -or $branch -eq 'main') {
                return (& $reject ("push to '{0}' is never allowed" -f $branch))
            }
            # A plain source ref only: a `:` would make this a refspec and
            # `task/x:master` would push straight into master. Test-SafeRef
            # permits `:` for revision syntax, so it must not be the push check.
            if ($branch -notmatch '^(task|docs|codex)/[A-Za-z0-9._/-]+$' -or $branch -match '\.\.') {
                return (& $reject 'push branch must be a plain task/*, docs/* or codex/* branch name')
            }
        }
    }

    return [ordered]@{ ok = $true; reason = ''; full_sha = $fullSha }
}

# ---------------------------------------------------------------------------
# Worktree, corpus copy, artifacts.
# ---------------------------------------------------------------------------

function Ensure-Worktree {
    param([string]$Repo, [string]$Wt, [string]$Sha)
    $gitFile = Join-Path $Wt '.git'
    if (-not (Test-Path -LiteralPath $gitFile)) {
        if (Test-Path -LiteralPath $Wt) { Remove-Item -LiteralPath $Wt -Recurse -Force }
        # A worktree directory deleted by hand leaves a stale registration
        # that makes `worktree add` refuse the path.
        [void](Invoke-Git -WorkDir $Repo -Arguments @('worktree', 'prune'))
        $created = Invoke-Git -WorkDir $Repo -Arguments @('worktree', 'add', '--detach', $Wt, $Sha)
        if ($created.exit -ne 0) { throw "worktree add failed: $($created.text)" }
    }
    else {
        $co = Invoke-Git -WorkDir $Wt -Arguments @('checkout', '--detach', '--force', $Sha)
        if ($co.exit -ne 0) { throw "worktree checkout failed: $($co.text)" }
    }
    # Removes everything untracked, including the corpora copied by the
    # previous job: they are copied again below, after this clean.
    $clean = Invoke-Git -WorkDir $Wt -Arguments @('clean', '-ffdx')
    if ($clean.exit -ne 0) { throw "worktree clean failed: $($clean.text)" }
}

function Copy-Tree {
    param([string]$Source, [string]$Destination, [int]$TimeoutSec, [string]$OutDir, [scriptblock]$AfterCopy)
    if (-not (Test-Path -LiteralPath $Source)) {
        return (New-InfoStep -Name 'copy' -Cmd 'skip (absent source)' -Exit 0 -Tail "source absent: $Source")
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $result = Invoke-Logged -Name 'copy' -Exe 'robocopy.exe' -Arguments @($Source, $Destination, '/E', '/NFL', '/NDL', '/NJH', '/NJS', '/NP', '/R:1', '/W:1') -WorkDir $Destination -TimeoutSec $TimeoutSec -OutDir $OutDir
    # robocopy: 0-7 are success codes (files copied / extra / mismatched).
    if ($result.exit -lt 8 -and -not $result.timedout) { $result.exit = 0 }
    # Self-test-only hook; it is not part of the job schema. It lets the
    # self-test change the source between the two inventories.
    if ($AfterCopy) { & $AfterCopy $Source }
    return $result
}

# Inventory of a source tree: relative path -> "size|mtime". Used to prove
# the copied corpus did not change under the executor.
function Get-SourceInventory {
    param([string]$Root)
    $map = @{}
    if (Test-Path -LiteralPath $Root) {
        foreach ($file in (Get-ChildItem -LiteralPath $Root -Recurse -File -Force)) {
            $rel = $file.FullName.Substring($Root.Length).TrimStart('\', '/').Replace('\', '/')
            $map[$rel] = ('{0}|{1:o}' -f $file.Length, $file.LastWriteTimeUtc)
        }
    }
    return $map
}

# Returns the sorted list of entries that differ between two inventories.
function Compare-Inventory {
    param($Before, $After)
    $changed = New-Object System.Collections.Generic.List[string]
    foreach ($key in (@($Before.Keys) + @($After.Keys) | Sort-Object -Unique)) {
        if (-not $Before.ContainsKey($key) -or -not $After.ContainsKey($key) -or $Before[$key] -ne $After[$key]) {
            $changed.Add($key)
        }
    }
    return , $changed
}

# Copies the named repo-relative corpus dirs from the main copy into the
# worktree, each with an inventory before and after the copy. Absent dirs
# are skipped. Returns @{ ok; reason; steps }.
function Copy-Corpora {
    param([string]$Repo, [string]$Wt, [string[]]$RelDirs, [string]$OutDir, [scriptblock]$AfterCopyHook)
    $steps = @()
    foreach ($rel in $RelDirs) {
        $source = Join-RepoPath -Root $Repo -Relative $rel
        if (-not (Test-Path -LiteralPath $source)) {
            $steps += New-InfoStep -Name ('copy ' + $rel) -Cmd ('skip ' + $rel) -Exit 0 -Tail ('absent in the main copy: ' + $rel)
            continue
        }
        $dest = Join-RepoPath -Root $Wt -Relative $rel
        $before = Get-SourceInventory -Root $source
        $copy = Copy-Tree -Source $source -Destination $dest -TimeoutSec (10 * 60) -OutDir $OutDir -AfterCopy $AfterCopyHook
        $copy.name = 'copy ' + $rel
        $steps += $copy
        if ($copy.timedout) {
            return @{ ok = $false; reason = ('copy of {0} exceeded 10 minutes' -f $rel); steps = $steps }
        }
        if ($copy.exit -ne 0) {
            return @{ ok = $false; reason = ('copy of {0} failed' -f $rel); steps = $steps }
        }
        $after = Get-SourceInventory -Root $source
        $changed = Compare-Inventory -Before $before -After $after
        if ($changed.Count -gt 0) {
            $script:Log.Add(('source changed during copy: {0} file(s) under {1}' -f $changed.Count, $rel))
            foreach ($path in ($changed | Select-Object -First 20)) { $script:Log.Add('  ' + $rel + '/' + $path) }
            return @{ ok = $false; reason = ('source changed during copy: {0} file(s)' -f $changed.Count); steps = $steps }
        }
    }
    return @{ ok = $true; reason = ''; steps = $steps }
}

function Get-RelativeArtifactPath {
    param([string]$QueueDir, [string]$Path)
    $root = [System.IO.Path]::GetFullPath($QueueDir).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    $full = [System.IO.Path]::GetFullPath($Path)
    if ($full.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $full.Substring($root.Length).Replace('\', '/')
    }
    return $full
}

function New-Artifact {
    param([string]$QueueDir, [string]$Path)
    return [ordered]@{ path = (Get-RelativeArtifactPath -QueueDir $QueueDir -Path $Path); sha256 = (Get-Sha256File $Path) }
}

# Writes a step's full stdout+stderr as an artifact file.
function Save-StepOutput {
    param($Step, [string]$Path)
    Write-Utf8NoBom -Path $Path -Text ([string]$Step.stdout + "`n" + [string]$Step.stderr)
}

function Get-CargoEnv {
    param([string]$Target)
    # CARGO_TERM_COLOR: CI sets `always`; the executor sets `never` so the
    # log and the `test result:` parsing see no ANSI escapes.
    return @{ CARGO_TARGET_DIR = $Target; CARGO_TERM_COLOR = 'never' }
}

# ---------------------------------------------------------------------------
# Test-output summary for gates.txt. Reads the FULL output, never the tail.
# ---------------------------------------------------------------------------

function Get-TestSummary {
    param([string]$Text)
    $resultLines = New-Object System.Collections.Generic.List[string]
    $failing = New-Object System.Collections.Generic.List[string]
    $failedTargets = New-Object System.Collections.Generic.List[string]
    $passed = 0
    $failed = 0
    $ignored = 0
    $lines = @($Text -split "`r?`n")
    $mode = ''
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line -match 'test result:\s') {
            $resultLines.Add($line.Trim())
            if ($line -match '(\d+)\s+passed') { $passed += [int]$Matches[1] }
            if ($line -match '(\d+)\s+failed') { $failed += [int]$Matches[1] }
            if ($line -match '(\d+)\s+ignored') { $ignored += [int]$Matches[1] }
            $mode = ''
            continue
        }
        if ($line -match '^---- (.+) stdout ----\s*$') {
            if (-not $failing.Contains($Matches[1])) { $failing.Add($Matches[1]) }
            continue
        }
        # cargo's own verdicts: `error: test failed, to rerun pass `-p x --test y``
        # and, under --no-fail-fast, `error: N targets failed:` + "    `-p x --test y`".
        if ($line -match '`(-p [^`]+)`') {
            if (-not $failedTargets.Contains($Matches[1])) { $failedTargets.Add($Matches[1]) }
            continue
        }
        if ($line -match '^failures:\s*$') {
            # Two `failures:` sections: the first holds `---- name stdout ----`
            # blocks, the second is the plain list of names.
            $mode = 'list'
            for ($j = $i + 1; $j -lt $lines.Count; $j++) {
                if ($lines[$j].Trim() -eq '') { continue }
                if ($lines[$j] -match '^---- ') { $mode = '' }
                break
            }
            continue
        }
        if ($mode -eq 'list') {
            if ($line.Trim() -eq '') { continue }
            if ($line -match '^    (\S.*)$') {
                $name = $Matches[1].TrimEnd()
                if (-not $failing.Contains($name)) { $failing.Add($name) }
                continue
            }
            $mode = ''
        }
    }
    return [pscustomobject]@{
        saw            = ($resultLines.Count -gt 0)
        lines          = $resultLines
        passed         = $passed
        failed         = $failed
        ignored        = $ignored
        failing        = $failing
        failed_targets = $failedTargets
    }
}

# ---------------------------------------------------------------------------
# Kinds.
# ---------------------------------------------------------------------------

function Get-GateStepSpecs {
    param([string[]]$OnlySteps)
    $tc = $script:Toolchain
    $all = @(
        @{ name = 'fmt'; args = @($tc, 'fmt', '--all', '--', '--check') },
        @{ name = 'clippy'; args = @($tc, 'clippy', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings') },
        @{ name = 'test'; args = @($tc, 'test', '--workspace', '--all-features', '--no-fail-fast') },
        @{ name = 'deny'; args = @($tc, 'deny', 'check') },
        @{ name = 'doc'; args = @($tc, 'doc', '--workspace', '--no-deps') }
    )
    # Canonical order is kept whatever order the job lists its subset in.
    if ($OnlySteps -and $OnlySteps.Count -gt 0) { $all = @($all | Where-Object { $OnlySteps -contains $_.name }) }
    return , $all
}

function Invoke-Gate {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, [string]$Id, [string]$OutDir, [string[]]$OnlySteps, [scriptblock]$AfterCopyHook)

    $deadline = (Get-Date).AddMinutes(120)
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    $copied = Copy-Corpora -Repo $Repo -Wt $wt -RelDirs $script:CorpusDirs -OutDir $OutDir -AfterCopyHook $AfterCopyHook
    $steps = @($copied.steps)
    if (-not $copied.ok) { return (New-Result 'error' $copied.reason $steps @()) }
    $copyCount = $steps.Count

    $cargoEnv = Get-CargoEnv -Target $target
    foreach ($spec in (Get-GateStepSpecs -OnlySteps $OnlySteps)) {
        $steps += Invoke-Logged -Name $spec.name -Exe 'cargo' -Arguments $spec.args -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $cargoEnv -OutDir $OutDir
    }
    $runSteps = @($steps | Select-Object -Skip $copyCount)

    $gatesPath = Join-Path $OutDir 'gates.txt'
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add(('# hostq gate {0} @ {1}' -f $Id, $Sha))
    $lines.Add('')
    $failedCount = 0
    foreach ($step in $runSteps) {
        $lines.Add(('{0,-72} exit {1}' -f $step.cmd, $step.exit))
        if ($step.exit -ne 0) { $failedCount++ }
        if ($step.name -eq 'test') {
            # The count must be read from the step's FULL output, not the
            # 40-line tail: cargo test prints the `test result:` lines to
            # stdout while compiler noise goes to stderr, so the tail can
            # contain no verdicts at all.
            $summary = Get-TestSummary -Text ($step.stdout + "`n" + $step.stderr)
            foreach ($l in $summary.lines) { $lines.Add('  ' + $l) }
            if ($summary.saw) {
                $lines.Add(('  total: {0} passed, {1} failed, {2} ignored' -f $summary.passed, $summary.failed, $summary.ignored))
            }
            else {
                $lines.Add('  total: no `test result:` line in the output')
            }
            if ($summary.failing.Count -gt 0) {
                $lines.Add(('  failing tests ({0}):' -f $summary.failing.Count))
                foreach ($name in $summary.failing) { $lines.Add('    ' + $name) }
            }
            if ($summary.failed_targets.Count -gt 0) {
                $lines.Add(('  failing targets ({0}):' -f $summary.failed_targets.Count))
                foreach ($t in $summary.failed_targets) { $lines.Add('    ' + $t) }
            }
        }
    }
    $lines.Add('')
    $lines.Add('summary: ' + (($runSteps | ForEach-Object { '{0}={1}' -f $_.name, $_.exit }) -join ' '))
    Write-Utf8NoBom -Path $gatesPath -Text (($lines -join "`n") + "`n")

    $artifacts = @(New-Artifact -QueueDir $QueueDir -Path $gatesPath)
    $status = Get-StepsStatus $runSteps
    $reason = ''
    if ($status -ne 'ok') { $reason = ('gate: {0} of {1} step(s) non-zero' -f $failedCount, $runSteps.Count) }
    return (New-Result $status $reason $steps $artifacts)
}

function Invoke-Pixels {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, [string]$OutDir)
    $deadline = (Get-Date).AddMinutes(60)
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    $tc = $script:Toolchain
    $cargoEnv = Get-CargoEnv -Target $target
    $steps = @()
    # Both run, as in ci.yml ("Render fidelity" and "PDF render fidelity").
    $steps += Invoke-Logged -Name 'ssim' -Exe 'cargo' -Arguments @($tc, 'test', '-p', 'strict-ooxml-render-svg', '--all-features', '--test', 'ssim') -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $cargoEnv -OutDir $OutDir
    $steps += Invoke-Logged -Name 'pdf_pixels' -Exe 'cargo' -Arguments @($tc, 'test', '-p', 'strict-ooxml-pdf', '--features', 'raster', '--test', 'pdf_pixels') -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $cargoEnv -OutDir $OutDir

    $artifacts = @()
    foreach ($step in $steps) {
        $p = Join-Path $OutDir ($step.name + '.txt')
        Save-StepOutput -Step $step -Path $p
        $artifacts += New-Artifact -QueueDir $QueueDir -Path $p
    }
    $status = Get-StepsStatus $steps
    $reason = ''
    if ($status -ne 'ok') { $reason = ('pixels: {0} of 2 step(s) non-zero' -f @($steps | Where-Object { $_.exit -ne 0 }).Count) }
    return (New-Result $status $reason $steps $artifacts)
}

function Invoke-Xsd {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, $Job, [string]$OutDir, [scriptblock]$AfterCopyHook)
    $deadline = (Get-Date).AddMinutes(120)
    $gate = [string]$Job.gate
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    $steps = @()
    if ($gate -eq 'census') {
        $copied = Copy-Corpora -Repo $Repo -Wt $wt -RelDirs $script:CensusCorpusDirs -OutDir $OutDir -AfterCopyHook $AfterCopyHook
        $steps += $copied.steps
        if (-not $copied.ok) { return (New-Result 'error' $copied.reason $steps @()) }
    }

    # ci.yml xsd-gate job: "Build the writer" (release strict-ooxml-cli).
    $cargoEnv = Get-CargoEnv -Target $target
    $build = Invoke-Logged -Name 'build-writer' -Exe 'cargo' -Arguments @($script:Toolchain, 'build', '--release', '-p', 'strict-ooxml-cli') -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $cargoEnv -OutDir $OutDir
    $steps += $build
    if ($build.timedout) { return (New-Result 'timeout' 'writer build timed out' $steps @()) }
    if ($build.exit -ne 0) { return (New-Result 'failed' 'writer build failed' $steps @()) }
    # CARGO_TARGET_DIR is the queue's target, so the gates' own lookup of
    # <repo>/target/release would miss it; the binary is named explicitly.
    $cli = Join-Path $target 'release\strict-ooxml.exe'
    if (-not (Test-Path -LiteralPath $cli -PathType Leaf)) {
        return (New-Result 'error' ('built writer not found at ' + $cli) $steps @())
    }

    $pyEnv = @{ PYTHONUTF8 = '1'; PYTHONIOENCODING = 'utf-8'; CARGO_TARGET_DIR = $target; CARGO_TERM_COLOR = 'never' }
    $artifacts = @()
    $gateArgs = @()
    if ($gate -eq 'xsd' -or $gate -eq 'opc') {
        # ci.yml "Write the corpus": every tests/strict/*.docx through the
        # binary just built; exit 1 is "written with losses", not a failure.
        $written = Join-Path $OutDir 'written'
        New-Item -ItemType Directory -Force -Path $written | Out-Null
        $corpus = Join-RepoPath -Root $wt -Relative 'strict-ooxml-core/tests/strict'
        $summary = New-Object System.Collections.Generic.List[string]
        $writtenCount = 0
        $docs = @(Get-ChildItem -LiteralPath $corpus -File -Filter '*.docx' -ErrorAction SilentlyContinue | Sort-Object Name)
        foreach ($doc in $docs) {
            $dst = Join-Path $written $doc.Name
            $w = Invoke-Logged -Name 'write' -Exe $cli -Arguments @('write', $doc.FullName, '--out', $dst) -WorkDir $wt -TimeoutSec ([math]::Min(600, (Get-Budget $deadline))) -Env $cargoEnv -OutDir $OutDir
            if ($w.timedout) {
                $steps += $w
                return (New-Result 'timeout' ('write timed out on ' + $doc.Name) $steps @())
            }
            if (Test-Path -LiteralPath $dst -PathType Leaf) { $writtenCount++ }
            $summary.Add(('{0} exit {1}' -f $doc.Name, $w.exit))
        }
        $writeExit = 0
        if ($writtenCount -eq 0) { $writeExit = 1 }
        $summary.Add(('written: {0} of {1}' -f $writtenCount, $docs.Count))
        $steps += New-InfoStep -Name 'write-corpus' -Cmd ('strict-ooxml write <tests/strict/*.docx> --out ' + $written) -Exit $writeExit -Tail ($summary -join "`n")
        if ($writeExit -ne 0) { return (New-Result 'failed' 'no package was written' $steps @()) }

        if ($gate -eq 'xsd') {
            $gateArgs = @('xtool/xsd-gate/xsd_gate.py', '--written', $written, '--quiet-messages')
        }
        else {
            $gateArgs = @('xtool/xsd-gate/opc_gate.py', '--written', $written)
        }
    }
    else {
        $reports = Join-Path $OutDir 'census-reports'
        New-Item -ItemType Directory -Force -Path $reports | Out-Null
        $inventory = Join-Path $OutDir 'census-inventory.json'
        $gateArgs = @('xtool/xsd-gate/census_gate.py', '--cli', $cli, '--quiet-messages', '--write-reports', $reports, '--inventory-out', $inventory)
    }

    $run = Invoke-Logged -Name ($gate + '-gate') -Exe $script:Python -Arguments $gateArgs -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $pyEnv -OutDir $OutDir
    $steps += $run
    $outFile = Join-Path $OutDir ($gate + '-gate.txt')
    Save-StepOutput -Step $run -Path $outFile
    $artifacts += New-Artifact -QueueDir $QueueDir -Path $outFile
    if ($gate -eq 'census') {
        if (Test-Path -LiteralPath $inventory -PathType Leaf) { $artifacts += New-Artifact -QueueDir $QueueDir -Path $inventory }
        foreach ($file in @(Get-ChildItem -LiteralPath $reports -Recurse -File -ErrorAction SilentlyContinue | Sort-Object FullName)) {
            $artifacts += New-Artifact -QueueDir $QueueDir -Path $file.FullName
        }
    }

    $status = Get-StepsStatus @($run)
    $reason = ''
    if ($status -ne 'ok') { $reason = ('{0} gate exited {1}' -f $gate, $run.exit) }
    return (New-Result $status $reason $steps $artifacts)
}

function Invoke-CorpusScan {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, $Job, [string]$OutDir, [scriptblock]$AfterCopyHook)
    $deadline = (Get-Date).AddMinutes(120)
    $set = [string]$Job.set
    $rel = 'testdata/' + $set
    if (-not (Test-Path -LiteralPath (Join-RepoPath -Root $Repo -Relative $rel))) {
        return (New-Result 'error' ('corpus set {0} is absent in the main copy' -f $rel) @() @())
    }
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    $copied = Copy-Corpora -Repo $Repo -Wt $wt -RelDirs @($rel) -OutDir $OutDir -AfterCopyHook $AfterCopyHook
    $steps = @($copied.steps)
    if (-not $copied.ok) { return (New-Result 'error' $copied.reason $steps @()) }

    $scanArgs = @($script:Toolchain, 'run', '-p', 'strict-ooxml', '--features', 'write,svg', '--example', 'corpus_scan', '--release', '--', $rel)
    $step = Invoke-Logged -Name 'corpus-scan' -Exe 'cargo' -Arguments $scanArgs -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env (Get-CargoEnv -Target $target) -OutDir $OutDir
    $steps += $step

    # corpus_scan writes nothing to disk: the per-file OK/FAIL rows go to
    # stdout, the progress and the by-stage summary to stderr.
    $tsv = Join-Path $OutDir ('corpus-scan-' + $set + '.tsv')
    $sum = Join-Path $OutDir ('corpus-scan-' + $set + '-summary.txt')
    Write-Utf8NoBom -Path $tsv -Text ([string]$step.stdout)
    Write-Utf8NoBom -Path $sum -Text ([string]$step.stderr)
    $artifacts = @((New-Artifact -QueueDir $QueueDir -Path $tsv), (New-Artifact -QueueDir $QueueDir -Path $sum))

    $status = Get-StepsStatus @($step)
    $reason = ''
    if ($status -ne 'ok') { $reason = ('corpus-scan exited {0}' -f $step.exit) }
    return (New-Result $status $reason $steps $artifacts)
}

function Invoke-Bench {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, $Job, [string]$OutDir)
    $deadline = (Get-Date).AddMinutes(90)
    $bench = [string]$Job.bench
    $package = [string]$script:Benches[$bench]
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    # Release by construction (`cargo bench`); `--noplot` keeps criterion from
    # rendering HTML into the shared target directory.
    $benchArgs = @($script:Toolchain, 'bench', '-p', $package, '--bench', $bench, '--', '--noplot')
    $step = Invoke-Logged -Name 'bench' -Exe 'cargo' -Arguments $benchArgs -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env (Get-CargoEnv -Target $target) -OutDir $OutDir

    # Criterion prints its estimates to stdout; that is the report.
    $report = Join-Path $OutDir ('bench-' + $bench + '.txt')
    Write-Utf8NoBom -Path $report -Text ([string]$step.stdout)
    $artifacts = @(New-Artifact -QueueDir $QueueDir -Path $report)

    $status = Get-StepsStatus @($step)
    $reason = ''
    if ($status -ne 'ok') { $reason = ('bench exited {0}' -f $step.exit) }
    return (New-Result $status $reason @($step) $artifacts)
}

function Invoke-Wps {
    param([string]$Repo, [string]$QueueDir, [string]$Sha, [string]$OutDir, [scriptblock]$AfterCopyHook)
    $deadline = (Get-Date).AddMinutes(60)
    $wt = Join-Path $QueueDir 'wt'
    $target = Join-Path $QueueDir 'target'
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Ensure-Worktree -Repo $Repo -Wt $wt -Sha $Sha

    # Clio lives in the gitignored corpus, like the gate's documents.
    $copied = Copy-Corpora -Repo $Repo -Wt $wt -RelDirs @('strict-ooxml-core/tests/docx') -OutDir $OutDir -AfterCopyHook $AfterCopyHook
    $steps = @($copied.steps)
    if (-not $copied.ok) { return (New-Result 'error' $copied.reason $steps @()) }

    # The P1 protocol (docs/audit-remediation-2026-10-06/P01-wps-geometry/tests.md),
    # once with the WPS Times calibration the gate was accepted under and once
    # with the renderer's defaults; each render replaces the SVGs the ledger reads.
    $clio = 'strict-ooxml-core/tests/docx/Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx'
    $svg = 'target/remediation-2026-10-06/clio-svg'
    $cargoEnv = Get-CargoEnv -Target $target
    $artifacts = @()
    foreach ($variant in @('wps-times', 'default')) {
        $svgDir = Join-Path $wt $svg
        if (Test-Path -LiteralPath $svgDir) { Remove-Item -LiteralPath $svgDir -Recurse -Force }
        $render = @($script:Toolchain, 'run', '-p', 'strict-ooxml-cli', '--release', '--', 'render', '--transitional', '--pages', '54-104', '--out', $svg)
        if ($variant -eq 'wps-times') { $render += '--wps-times' }
        $render += $clio
        $steps += Invoke-Logged -Name ('render-' + $variant) -Exe 'cargo' -Arguments $render -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -Env $cargoEnv -OutDir $OutDir
        $ledger = Join-Path $OutDir ('wps-ledger-' + $variant + '.json')
        $steps += Invoke-Logged -Name ('ledger-' + $variant) -Exe $script:Python -Arguments @('xtool/wps-gate/wps_ledger.py', '--root', $wt, '--out', $ledger) -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -OutDir $OutDir
        $steps += Invoke-Logged -Name ('p1-selftest-' + $variant) -Exe $script:Python -Arguments @('xtool/wps-gate/wps_p1_gate_selftest.py') -WorkDir $wt -TimeoutSec (Get-Budget $deadline) -OutDir $OutDir
        foreach ($step in @($steps | Select-Object -Last 2)) {
            $p = Join-Path $OutDir ($step.name + '.txt')
            Save-StepOutput -Step $step -Path $p
            $artifacts += New-Artifact -QueueDir $QueueDir -Path $p
        }
        if (Test-Path -LiteralPath $ledger) { $artifacts += New-Artifact -QueueDir $QueueDir -Path $ledger }
    }
    $status = Get-StepsStatus $steps
    $reason = ''
    if ($status -ne 'ok') { $reason = ('wps: {0} step(s) non-zero' -f @($steps | Where-Object { $_.exit -ne 0 }).Count) }
    return (New-Result $status $reason $steps $artifacts)
}

function Invoke-GitInfo {
    param([string]$Repo, $Job, [string]$OutDir)
    $steps = @()
    $rev = [string]$Job.rev
    $steps += Invoke-Logged -Name 'rev-parse' -Exe 'git' -Arguments @('rev-parse', $rev) -WorkDir $Repo -TimeoutSec 120 -OutDir $OutDir
    if ($null -ne $Job.PSObject.Properties['range'] -and -not [string]::IsNullOrWhiteSpace([string]$Job.range)) {
        $range = [string]$Job.range
        $steps += Invoke-Logged -Name 'log' -Exe 'git' -Arguments @('log', '--oneline', '-n', '50', $range) -WorkDir $Repo -TimeoutSec 120 -OutDir $OutDir
        $steps += Invoke-Logged -Name 'diff-stat' -Exe 'git' -Arguments @('diff', '--stat', $range) -WorkDir $Repo -TimeoutSec 120 -OutDir $OutDir
    }
    $steps += Invoke-Logged -Name 'status' -Exe 'git' -Arguments @('status', '--porcelain') -WorkDir $Repo -TimeoutSec 120 -OutDir $OutDir
    $status = Get-StepsStatus $steps
    $reason = ''
    if ($status -ne 'ok') { $reason = 'git-info step failed' }
    return (New-Result $status $reason $steps @())
}

function Invoke-Ping {
    param([string]$Repo, [string]$OutDir)
    $tc = $script:Toolchain
    $required = @()
    $required += Invoke-Probe -Name 'git-version' -Exe 'git' -Arguments @('--version') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    $required += Invoke-Probe -Name 'rustup-toolchain' -Exe 'rustup' -Arguments @('show', 'active-toolchain') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    $required += Invoke-Probe -Name 'cargo-version' -Exe 'cargo' -Arguments @($tc, '--version') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    $required += Invoke-Probe -Name 'rustc-version' -Exe 'rustc' -Arguments @($tc, '--version') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    $required += Invoke-Probe -Name 'components' -Exe 'rustup' -Arguments @('component', 'list', '--installed', '--toolchain', $tc.TrimStart('+')) -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    $required += Invoke-Probe -Name 'python-version' -Exe $script:Python -Arguments @('--version') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir

    # Optional: cargo-deny is only needed by the gate's `deny` step.
    $optional = @()
    $denyExe = Get-Command 'cargo-deny' -ErrorAction SilentlyContinue
    if ($denyExe) {
        $optional += Invoke-Probe -Name 'cargo-deny-version' -Exe 'cargo' -Arguments @($tc, 'deny', '--version') -WorkDir $Repo -TimeoutSec 60 -OutDir $OutDir
    }
    else {
        $optional += New-InfoStep -Name 'cargo-deny-version' -Cmd 'cargo-deny' -Exit 0 -Tail 'cargo-deny is not installed (the gate deny step will fail)'
    }

    $free = 'unknown'
    try {
        $root = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath($Repo))
        $drive = New-Object System.IO.DriveInfo($root)
        $free = ('{0} bytes free of {1} ({2:N1} GiB free)' -f $drive.AvailableFreeSpace, $drive.TotalSize, ($drive.AvailableFreeSpace / 1GB))
    }
    catch { $free = ('unavailable: ' + $_.Exception.Message) }
    $optional += New-InfoStep -Name 'disk' -Cmd 'DriveInfo' -Exit 0 -Tail $free
    $optional += New-InfoStep -Name 'repo' -Cmd 'repo path' -Exit 0 -Tail $Repo

    $corpusLines = New-Object System.Collections.Generic.List[string]
    foreach ($rel in $script:CorpusDirs) {
        $path = Join-RepoPath -Root $Repo -Relative $rel
        if (Test-Path -LiteralPath $path) {
            $count = @(Get-ChildItem -LiteralPath $path -Recurse -File -Force -ErrorAction SilentlyContinue).Count
            $corpusLines.Add(('{0}: present, {1} file(s)' -f $rel, $count))
        }
        else {
            $corpusLines.Add(('{0}: absent' -f $rel))
        }
    }
    $optional += New-InfoStep -Name 'corpora' -Cmd 'corpus dirs' -Exit 0 -Tail ($corpusLines -join "`n")
    $optional += New-InfoStep -Name 'cargo-build-jobs' -Cmd 'CARGO_BUILD_JOBS' -Exit 0 -Tail ('CARGO_BUILD_JOBS=' + [string]$env:CARGO_BUILD_JOBS)

    $steps = @($required) + @($optional)
    $status = Get-StepsStatus $required
    $reason = ''
    if ($status -ne 'ok') { $reason = 'ping step failed' }
    return (New-Result $status $reason $steps @())
}

function Invoke-Commit {
    param([string]$Repo, $Job, [string]$OutDir)
    $branch = [string]$Job.branch
    $current = (Invoke-Git -WorkDir $Repo -Arguments @('rev-parse', '--abbrev-ref', 'HEAD')).text.Trim()
    if ($current -ne $branch) {
        return (New-Result 'rejected' ("main copy is on '{0}', job wants '{1}'" -f $current, $branch) @() @())
    }
    $paths = @($Job.paths | ForEach-Object { [string]$_ })
    $messageFile = [System.IO.Path]::GetFullPath((Join-Path $Repo ([string]$Job.message)))
    $steps = @()
    # `git commit --only -- <paths>` refuses an untracked path, and the
    # acceptance's review/order files are often new; stage the exact paths
    # first so `--only` can find them.
    $add = Invoke-Logged -Name 'add' -Exe 'git' -Arguments (@('add', '--') + $paths) -WorkDir $Repo -TimeoutSec (10 * 60) -OutDir $OutDir
    $steps += $add
    if ($add.timedout -or $add.exit -ne 0) {
        $st = 'failed'
        if ($add.timedout) { $st = 'timeout' }
        return (New-Result $st 'git add failed' $steps @())
    }
    $commitArgs = @('commit', '--only', '-F', $messageFile) + @('--') + $paths
    $commit = Invoke-Logged -Name 'commit' -Exe 'git' -Arguments $commitArgs -WorkDir $Repo -TimeoutSec (10 * 60) -OutDir $OutDir
    $steps += $commit
    $show = Invoke-Logged -Name 'show' -Exe 'git' -Arguments @('show', '--stat', 'HEAD') -WorkDir $Repo -TimeoutSec 120 -OutDir $OutDir
    $steps += $show
    $status = Get-StepsStatus @($commit)
    $reason = ''
    if ($status -ne 'ok') { $reason = 'commit failed' }
    return (New-Result $status $reason $steps @())
}

function Invoke-Push {
    param([string]$Repo, $Job, [string]$OutDir)
    $branch = [string]$Job.branch
    $steps = @()
    # Never --force. The branch was validated as a plain task/docs/codex name.
    $steps += Invoke-Logged -Name 'push' -Exe 'git' -Arguments @('push', 'origin', $branch) -WorkDir $Repo -TimeoutSec (10 * 60) -OutDir $OutDir
    $status = Get-StepsStatus $steps
    $reason = ''
    if ($status -ne 'ok') { $reason = 'push failed' }
    return (New-Result $status $reason $steps @())
}

# ---------------------------------------------------------------------------
# One job: validate, run, capture main state, write log then report.
# ---------------------------------------------------------------------------

function Invoke-JobFile {
    param(
        [string]$JobFile,
        [string]$Repo,
        [string]$QueueDir,
        [switch]$AllowPush,
        # Self-test-only hooks; they are not part of the job schema.
        # `TestHook` runs right after `main_before`; `AfterCopyHook` reaches
        # Copy-Tree for the kinds that copy corpora.
        [scriptblock]$TestHook,
        [scriptblock]$AfterCopyHook
    )

    $id = [System.IO.Path]::GetFileNameWithoutExtension($JobFile)
    $outDir = Join-Path (Join-Path $QueueDir 'out') $id
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $script:Log = New-Object System.Collections.Generic.List[string]
    $script:StepCounter = 0

    $started = (Get-Date).ToUniversalTime().ToString('o')
    $mainBefore = Get-MainState -Repo $Repo
    if ($TestHook) { & $TestHook $Repo }

    $kind = ''
    $fullSha = ''
    $steps = @()
    $artifacts = @()
    $status = 'error'
    $reason = ''

    try {
        $job = ConvertFrom-Json -InputObject (Read-Utf8Text -Path $JobFile)
        $kind = ''
        if ($null -ne $job -and $null -ne $job.PSObject.Properties['kind'] -and $null -ne $job.kind) { $kind = [string]$job.kind }

        $valid = Test-JobRecord -Job $job -FileId $id -Repo $Repo -AllowPush:$AllowPush
        if (-not $valid.ok) {
            $status = 'rejected'
            $reason = $valid.reason
        }
        else {
            $fullSha = $valid.full_sha
            $r = $null
            switch ($kind) {
                'ping' { $r = Invoke-Ping -Repo $Repo -OutDir $outDir }
                'git-info' { $r = Invoke-GitInfo -Repo $Repo -Job $job -OutDir $outDir }
                'gate' {
                    $only = @()
                    if ($null -ne $job.PSObject.Properties['steps']) { $only = @($job.steps | ForEach-Object { [string]$_ }) }
                    $r = Invoke-Gate -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -Id $id -OutDir $outDir -OnlySteps $only -AfterCopyHook $AfterCopyHook
                }
                'pixels' { $r = Invoke-Pixels -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -OutDir $outDir }
                'xsd' { $r = Invoke-Xsd -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -Job $job -OutDir $outDir -AfterCopyHook $AfterCopyHook }
                'corpus-scan' { $r = Invoke-CorpusScan -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -Job $job -OutDir $outDir -AfterCopyHook $AfterCopyHook }
                'bench' { $r = Invoke-Bench -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -Job $job -OutDir $outDir }
                'wps' { $r = Invoke-Wps -Repo $Repo -QueueDir $QueueDir -Sha $fullSha -OutDir $outDir -AfterCopyHook $AfterCopyHook }
                'commit' { $r = Invoke-Commit -Repo $Repo -Job $job -OutDir $outDir }
                'push' { $r = Invoke-Push -Repo $Repo -Job $job -OutDir $outDir }
                default { $r = New-Result 'rejected' 'unknown kind' @() @() }
            }
            $status = $r.status
            $reason = $r.reason
            $steps = $r.steps
            $artifacts = $r.artifacts
        }
    }
    catch {
        $status = 'error'
        $reason = $_.Exception.Message
        $script:Log.Add('EXCEPTION: ' + $_.Exception.Message)
    }

    $mainAfter = Get-MainState -Repo $Repo
    # The submitter works in the main copy while the executor runs, so a
    # change there is not a finding: the executor cannot tell its own write
    # from theirs. It is only recorded. The only source whose change matters
    # is the one the job reads (the copied corpora), checked in Copy-Corpora.
    $mainChanged = [ordered]@{
        head   = ($mainBefore.head -ne $mainAfter.head)
        branch = ($mainBefore.branch -ne $mainAfter.branch)
        status = ($mainBefore.status_sha256 -ne $mainAfter.status_sha256)
        corpus = ($mainBefore.corpus_sha256 -ne $mainAfter.corpus_sha256)
    }

    $finished = (Get-Date).ToUniversalTime().ToString('o')
    $report = [ordered]@{
        id           = $id
        kind         = $kind
        status       = $status
        reason       = $reason
        started      = $started
        finished     = $finished
        sha          = $fullSha
        steps        = @($steps | ForEach-Object { ConvertTo-ReportStep $_ })
        artifacts    = @($artifacts)
        main_before  = $mainBefore
        main_after   = $mainAfter
        main_changed = $mainChanged
    }

    $logPath = Join-Path (Join-Path $QueueDir 'out') ($id + '.log')
    Write-Utf8NoBom -Path $logPath -Text (($script:Log -join "`n") + "`n")

    # The report is written last, through a temp file and a rename.
    $reportPath = Join-Path (Join-Path $QueueDir 'out') ($id + '.json')
    $tmp = $reportPath + '.tmp'
    Write-JsonNoBom -Path $tmp -Object $report
    Move-Item -LiteralPath $tmp -Destination $reportPath -Force

    Write-Host ("hostq: {0} {1} {2}" -f $id, $kind, $status)
}

# ---------------------------------------------------------------------------
# Queue layout, heartbeat, pending jobs, main loop.
# ---------------------------------------------------------------------------

function New-QueueLayout {
    param([string]$QueueDir)
    foreach ($name in @('inbox', 'running', 'done', 'out', 'target')) {
        New-Item -ItemType Directory -Force -Path (Join-Path $QueueDir $name) | Out-Null
    }
    # `wt` is created by `git worktree add` on first use; an empty directory
    # there is removed by Ensure-Worktree anyway.
}

function Get-PendingJobs {
    param([string]$QueueDir)
    $inbox = Join-Path $QueueDir 'inbox'
    if (-not (Test-Path -LiteralPath $inbox)) { return @() }
    # -Filter '*.json' would also match '*.json.tmp' on Windows' 8.3 rules
    # in some cases; the extension is checked exactly.
    return @(
        Get-ChildItem -LiteralPath $inbox -File |
            Where-Object { $_.Extension -eq '.json' } |
            Sort-Object Name |
            ForEach-Object { $_.FullName }
    )
}

# Job files left in running\ by a crash. They are never retried or deleted.
function Get-StaleRunningJobs {
    param([string]$QueueDir)
    $running = Join-Path $QueueDir 'running'
    if (-not (Test-Path -LiteralPath $running)) { return @() }
    return @(Get-ChildItem -LiteralPath $running -File | Sort-Object Name | ForEach-Object { $_.Name })
}

function Write-StaleRunningHint {
    param([string]$QueueDir)
    $stale = @(Get-StaleRunningJobs -QueueDir $QueueDir)
    if ($stale.Count -eq 0) { return }
    $running = Join-Path $QueueDir 'running'
    Write-Host ''
    Write-Host ('hostq: WARNING: {0} stale job file(s) in {1} (left by a crash or a closed window):' -f $stale.Count, $running)
    foreach ($name in $stale) { Write-Host ('hostq:   ' + $name) }
    Write-Host 'hostq: they are NOT retried and NOT deleted. Check out\<id>.json / out\<id>.log,'
    Write-Host 'hostq: then remove the file (or move it back to inbox\ to run it again).'
    Write-Host ''
}

function Start-Heartbeat {
    param([string]$QueueDir)
    $parent = $PID
    return Start-Job -ScriptBlock {
        param($queueDir, $executorPid)
        $enc = New-Object System.Text.UTF8Encoding($false)
        $currentFile = Join-Path $queueDir 'current'
        while ($true) {
            # The executor's own current job, written to `current` when it is
            # taken and cleared when it finishes - never a scan of running\.
            $current = $null
            if (Test-Path -LiteralPath $currentFile) {
                $text = [System.IO.File]::ReadAllText($currentFile).Trim()
                if ($text -ne '') { $current = $text }
            }
            $obj = [ordered]@{
                pid      = $executorPid
                time_utc = (Get-Date).ToUniversalTime().ToString('o')
                job      = $current
            }
            $tmp = Join-Path $queueDir 'heartbeat.json.tmp'
            [System.IO.File]::WriteAllText($tmp, (ConvertTo-Json -InputObject $obj -Compress), $enc)
            Move-Item -LiteralPath $tmp -Destination (Join-Path $queueDir 'heartbeat.json') -Force
            Start-Sleep -Seconds 30
        }
    } -ArgumentList $QueueDir, $parent
}

# Takes the queue's exclusive lock. The returned stream is held for the whole
# process lifetime; a second instance gets $null. `lock` holds the pid and
# start time so the loser can name the winner (read from heartbeat.json,
# since the lock itself is FileShare.None).
function Acquire-QueueLock {
    param([string]$QueueDir)
    $lockPath = Join-Path $QueueDir 'lock'
    try {
        $stream = [System.IO.File]::Open($lockPath, [System.IO.FileMode]::OpenOrCreate, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    }
    catch [System.IO.IOException] {
        return $null
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(('{0} {1:o}' -f $PID, (Get-Date).ToUniversalTime()))
    $stream.SetLength(0)
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush()
    return $stream
}

function Write-CurrentJob {
    param([string]$QueueDir, [string]$Id)
    $currentFile = Join-Path $QueueDir 'current'
    if ([string]::IsNullOrEmpty($Id)) {
        if (Test-Path -LiteralPath $currentFile) { Remove-Item -LiteralPath $currentFile -Force -ErrorAction SilentlyContinue }
    }
    else {
        Write-Utf8NoBom -Path $currentFile -Text $Id
    }
}

# Ctrl+C handling: with TreatControlCAsInput the key is buffered instead of
# killing the script mid-job, and the loop reads it between jobs. If the
# console cannot do that (redirected input), PowerShell's default applies.
function Enable-CtrlCAsInput {
    try {
        [Console]::TreatControlCAsInput = $true
        return $true
    }
    catch {
        return $false
    }
}

function Test-CtrlCPressed {
    try {
        while ([Console]::KeyAvailable) {
            $key = [Console]::ReadKey($true)
            if ($key.Key -eq [ConsoleKey]::C -and ($key.Modifiers -band [ConsoleModifiers]::Control)) { return $true }
        }
    }
    catch { }
    return $false
}

function Start-Executor {
    param([string]$Repo, [string]$QueueDir, [switch]$AllowPush)
    New-QueueLayout -QueueDir $QueueDir
    $inbox = Join-Path $QueueDir 'inbox'
    $running = Join-Path $QueueDir 'running'
    $done = Join-Path $QueueDir 'done'

    $ctrlC = Enable-CtrlCAsInput
    $heartbeat = Start-Heartbeat -QueueDir $QueueDir
    try {
        Write-Host ("hostq: watching {0} (repo {1}, push {2})" -f $inbox, $Repo, $(if ($AllowPush) { 'allowed' } else { 'off' }))
        if ($ctrlC) { Write-Host 'hostq: Ctrl+C stops after the current job (close the window to abort a job).' }
        while ($true) {
            if (Test-CtrlCPressed) {
                Write-Host 'hostq: Ctrl+C - stopping.'
                break
            }
            $pending = @(Get-PendingJobs -QueueDir $QueueDir)
            if ($pending.Count -eq 0) {
                Start-Sleep -Seconds 2
                continue
            }
            $jobFile = $pending[0]
            $id = [System.IO.Path]::GetFileNameWithoutExtension($jobFile)
            $runningFile = Join-Path $running ($id + '.json')
            Move-Item -LiteralPath $jobFile -Destination $runningFile -Force
            Write-CurrentJob -QueueDir $QueueDir -Id $id
            Write-Host ("hostq: {0} started" -f $id)
            Invoke-JobFile -JobFile $runningFile -Repo $Repo -QueueDir $QueueDir -AllowPush:$AllowPush
            Move-Item -LiteralPath $runningFile -Destination (Join-Path $done ($id + '.json')) -Force
            Write-CurrentJob -QueueDir $QueueDir -Id ''
        }
    }
    finally {
        Write-CurrentJob -QueueDir $QueueDir -Id ''
        Stop-Job -Job $heartbeat -ErrorAction SilentlyContinue
        Remove-Job -Job $heartbeat -Force -ErrorAction SilentlyContinue
        if ($ctrlC) { try { [Console]::TreatControlCAsInput = $false } catch { } }
    }
}

# ---------------------------------------------------------------------------
# Self-test: the falsifiers, against a temporary repository and a temporary
# queue; the real main copy is never touched. Heavy kinds are not built:
# only `cargo fmt` / `cargo test` on a one-file crate run, as in David.
# ---------------------------------------------------------------------------

function New-TempRepo {
    param([string]$Root)
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    $init = Invoke-Git -WorkDir $Root -Arguments @('init')
    if ($init.exit -ne 0) { throw "git init failed: $($init.text)" }
    # `init -b` needs git 2.28; a symbolic-ref works everywhere.
    Invoke-Git -WorkDir $Root -Arguments @('symbolic-ref', 'HEAD', 'refs/heads/master') | Out-Null
    Invoke-Git -WorkDir $Root -Arguments @('config', 'user.email', 'hostq@selftest.local') | Out-Null
    Invoke-Git -WorkDir $Root -Arguments @('config', 'user.name', 'hostq selftest') | Out-Null
    Invoke-Git -WorkDir $Root -Arguments @('config', 'commit.gpgsign', 'false') | Out-Null

    Write-Utf8NoBom -Path (Join-Path $Root 'Cargo.toml') -Text @"
[package]
name = "hostq-selftest"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"@
    # The queue lives inside the repo; keep it out of `git status`.
    Write-Utf8NoBom -Path (Join-Path $Root '.gitignore') -Text "/StrictLib-hostq/`n/target/`n/testdata/`n"
    New-Item -ItemType Directory -Force -Path (Join-Path $Root 'src') | Out-Null
    Write-Utf8NoBom -Path (Join-Path $Root 'src/lib.rs') -Text "pub fn answer() -> u32 {`n    42`n}`n`n#[cfg(test)]`nmod tests {`n    #[test]`n    fn answer_is_42() {`n        assert_eq!(super::answer(), 42);`n    }`n}`n"
    Invoke-Git -WorkDir $Root -Arguments @('add', '-A') | Out-Null
    Invoke-Git -WorkDir $Root -Arguments @('commit', '-m', 'clean commit') | Out-Null
    $clean = (Invoke-Git -WorkDir $Root -Arguments @('rev-parse', 'HEAD')).text.Trim()

    Write-Utf8NoBom -Path (Join-Path $Root 'src/lib.rs') -Text "pub fn answer( )->u32{`n42`n}`n"
    Invoke-Git -WorkDir $Root -Arguments @('add', '-A') | Out-Null
    Invoke-Git -WorkDir $Root -Arguments @('commit', '-m', 'misformatted commit') | Out-Null
    $dirty = (Invoke-Git -WorkDir $Root -Arguments @('rev-parse', 'HEAD')).text.Trim()

    return [pscustomobject]@{ clean = $clean; dirty = $dirty }
}

function Invoke-SelfTest {
    param([string]$Repo)

    $root = Join-Path ([System.IO.Path]::GetTempPath()) ('hostq-selftest-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $tempRepo = Join-Path $root 'repo'
    # Match the path the executor computes from -Repo, so the second-instance
    # check contends for the same lock.
    $tempQueue = Join-Path $tempRepo $script:QueueName
    $lockStream = $null
    $checks = New-Object System.Collections.Generic.List[object]

    $add = {
        param($Name, $Pass, $Detail)
        $checks.Add([pscustomobject]@{ check = $Name; result = $(if ($Pass) { 'PASS' } else { 'FAIL' }); detail = $Detail })
    }

    try {
        $commits = New-TempRepo -Root $tempRepo
        New-QueueLayout -QueueDir $tempQueue
        $inbox = Join-Path $tempQueue 'inbox'
        & $add '(pre) temp repo has two commits' ($commits.clean -ne $commits.dirty) ("clean=" + $commits.clean.Substring(0, 7) + " dirty=" + $commits.dirty.Substring(0, 7))
        $short = $commits.clean.Substring(0, 7)

        # Drives one job through the same queue moves as Start-Executor.
        $submit = {
            param($Id, $Object, $TestHook, $AfterCopyHook)
            $file = Join-Path $inbox ($Id + '.json')
            Write-JsonNoBom -Path $file -Object $Object
            $running = Join-Path $tempQueue ('running/' + $Id + '.json')
            Move-Item -LiteralPath $file -Destination $running -Force
            Invoke-JobFile -JobFile $running -Repo $tempRepo -QueueDir $tempQueue -TestHook $TestHook -AfterCopyHook $AfterCopyHook
            Move-Item -LiteralPath $running -Destination (Join-Path $tempQueue ('done/' + $Id + '.json')) -Force
        }
        $report = {
            param($Id)
            ConvertFrom-Json -InputObject (Read-Utf8Text -Path (Join-Path $tempQueue ('out/' + $Id + '.json')))
        }
        # A job that must be rejected through the full pipeline, no process.
        $expectReject = {
            param($Label, $Id, $Object)
            [void](& $submit $Id $Object)
            $r = & $report $Id
            & $add $Label ($r.status -eq 'rejected' -and @($r.steps).Count -eq 0) ("status=" + $r.status + " reason=" + $r.reason)
        }

        # --- rejections (no process is launched) ---------------------------
        & $expectReject '(rej) unknown kind' '20260101-0001-rmkind' ([ordered]@{ id = '20260101-0001-rmkind'; kind = 'rm'; path = 'x' })
        & $expectReject '(rej) extra field' '20260101-0002-extra' ([ordered]@{ id = '20260101-0002-extra'; kind = 'ping'; cmd = 'calc' })
        & $expectReject '(rej) bad id' 'bad_id' ([ordered]@{ id = 'bad_id'; kind = 'ping' })
        & $expectReject '(rej) id != file name' '20260101-0003-idmismatch' ([ordered]@{ id = '20260101-0003-other'; kind = 'ping' })
        & $expectReject '(rej) bad sha' '20260101-0004-badsha' ([ordered]@{ id = '20260101-0004-badsha'; kind = 'gate'; sha = 'HEAD;x' })
        & $expectReject '(rej) unknown sha' '20260101-0005-nosha' ([ordered]@{ id = '20260101-0005-nosha'; kind = 'pixels'; sha = 'deadbeefdeadbeef' })
        & $expectReject '(rej) bad steps entry' '20260101-0006-badstep' ([ordered]@{ id = '20260101-0006-badstep'; kind = 'gate'; sha = $short; steps = @('fmt', 'bench') })
        & $expectReject '(rej) duplicate steps' '20260101-0007-dupstep' ([ordered]@{ id = '20260101-0007-dupstep'; kind = 'gate'; sha = $short; steps = @('fmt', 'fmt') })
        & $expectReject '(rej) steps not an array' '20260101-0008-strstep' ([ordered]@{ id = '20260101-0008-strstep'; kind = 'gate'; sha = $short; steps = 'fmt' })
        & $expectReject '(rej) bad set' '20260101-0009-badset' ([ordered]@{ id = '20260101-0009-badset'; kind = 'corpus-scan'; sha = $short; set = '../CC0' })
        & $expectReject '(rej) unknown bench' '20260101-0040-badbench' ([ordered]@{ id = '20260101-0040-badbench'; kind = 'bench'; sha = $short; bench = 'editing --all' })
        & $expectReject '(rej) bad xsd gate' '20260101-0010-badgate' ([ordered]@{ id = '20260101-0010-badgate'; kind = 'xsd'; sha = $short; gate = 'lint' })
        & $expectReject '(rej) pixels extra field' '20260101-0011-pixextra' ([ordered]@{ id = '20260101-0011-pixextra'; kind = 'pixels'; sha = $short; steps = @('fmt') })
        & $expectReject '(rej) push without -AllowPush' '20260101-0012-pushmaster' ([ordered]@{ id = '20260101-0012-pushmaster'; kind = 'push'; branch = 'master' })

        $pushMaster = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0013-pushmaster'; kind = 'push'; branch = 'master' }) -FileId '20260101-0013-pushmaster' -Repo $tempRepo -AllowPush
        & $add '(rej) push master with -AllowPush' (-not $pushMaster.ok) ("reason=" + $pushMaster.reason)
        $pushRef = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0014-pushref'; kind = 'push'; branch = 'task/x:master' }) -FileId '20260101-0014-pushref' -Repo $tempRepo -AllowPush
        & $add '(rej) push refspec' (-not $pushRef.ok) ("reason=" + $pushRef.reason)
        $pushOk = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0015-pushok'; kind = 'push'; branch = 'codex/hostq' }) -FileId '20260101-0015-pushok' -Repo $tempRepo -AllowPush
        & $add '(ok) push codex/* validates' ($pushOk.ok) ("reason=" + $pushOk.reason)

        # Positive validation of the heavy kinds (not run here).
        $vPixels = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0016-pix'; kind = 'pixels'; sha = $short }) -FileId '20260101-0016-pix' -Repo $tempRepo
        $vXsd = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0017-xsd'; kind = 'xsd'; sha = $short; gate = 'census' }) -FileId '20260101-0017-xsd' -Repo $tempRepo
        $vScan = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0018-scan'; kind = 'corpus-scan'; sha = $short; set = 'CC0_DOCX_1' }) -FileId '20260101-0018-scan' -Repo $tempRepo
        $vGate = Test-JobRecord -Job ([pscustomobject]@{ id = '20260101-0019-gate'; kind = 'gate'; sha = $short; steps = @('doc', 'fmt') }) -FileId '20260101-0019-gate' -Repo $tempRepo
        $vOk = ($vPixels.ok -and $vXsd.ok -and $vScan.ok -and $vGate.ok -and $vPixels.full_sha -eq $commits.clean)
        & $add '(ok) pixels/xsd/scan/gate validate' $vOk ("pixels=" + $vPixels.ok + " xsd=" + $vXsd.ok + " scan=" + $vScan.ok + " gate=" + $vGate.ok)

        # .tmp is not picked up.
        Write-JsonNoBom -Path (Join-Path $inbox '20260101-0020-half.json.tmp') -Object ([ordered]@{ id = '20260101-0020-half'; kind = 'ping' })
        $pending = @(Get-PendingJobs -QueueDir $tempQueue)
        & $add '(q) .tmp ignored' ($pending.Count -eq 0) ("pending=" + $pending.Count)
        Remove-Item -LiteralPath (Join-Path $inbox '20260101-0020-half.json.tmp') -Force

        # --- commit -----------------------------------------------------------
        $msgFile = Join-Path $tempRepo 'MSG.txt'
        Write-Utf8NoBom -Path $msgFile -Text "selftest`n"
        $wrong = Join-Path $tempRepo 'WRONG.txt'
        Write-Utf8NoBom -Path $wrong -Text "wrong`n"
        [void](& $submit '20260101-0021-wrongbranch' ([ordered]@{ id = '20260101-0021-wrongbranch'; kind = 'commit'; branch = 'task/other'; paths = @('WRONG.txt'); message = 'MSG.txt' }))
        $rWrong = & $report '20260101-0021-wrongbranch'
        & $add '(commit) wrong branch rejected' ($rWrong.status -eq 'rejected' -and @($rWrong.steps).Count -eq 0) ("status=" + $rWrong.status + " reason=" + $rWrong.reason)
        & $expectReject '(rej) path with ..' '20260101-0022-badpath' ([ordered]@{ id = '20260101-0022-badpath'; kind = 'commit'; branch = 'master'; paths = @('../outside.txt'); message = 'MSG.txt' })
        Remove-Item -LiteralPath $wrong -Force

        $newFile = Join-Path $tempRepo 'NEW.txt'
        Write-Utf8NoBom -Path $newFile -Text "new file`n"
        [void](& $submit '20260101-0023-newcommit' ([ordered]@{ id = '20260101-0023-newcommit'; kind = 'commit'; branch = 'master'; paths = @('NEW.txt'); message = 'MSG.txt' }))
        $rCommit = & $report '20260101-0023-newcommit'
        $showStep = $rCommit.steps | Where-Object { $_.name -eq 'show' }
        $showTail = ''
        if ($showStep) { $showTail = [string]$showStep.tail }
        & $add '(commit) ok, stages a new file' ($rCommit.status -eq 'ok' -and $showTail -match 'NEW.txt' -and $rCommit.main_changed.head -eq $true) ("status=" + $rCommit.status + " shows_new=" + [bool]($showTail -match 'NEW.txt'))
        Remove-Item -LiteralPath $msgFile -Force

        # --- ping, git-info ---------------------------------------------------
        [void](& $submit '20260101-0024-ping' ([ordered]@{ id = '20260101-0024-ping'; kind = 'ping' }))
        $rPing = & $report '20260101-0024-ping'
        $failedProbes = (@($rPing.steps | Where-Object { $_.exit -ne 0 } | ForEach-Object { $_.name }) -join ',')
        & $add '(ping) ok' ($rPing.status -eq 'ok') ("status=" + $rPing.status + " failed=" + $failedProbes)

        [void](& $submit '20260101-0025-info' ([ordered]@{ id = '20260101-0025-info'; kind = 'git-info'; rev = 'HEAD'; range = 'HEAD~1..HEAD' }))
        $rInfo = & $report '20260101-0025-info'
        $same = (($rInfo.main_before | ConvertTo-Json -Compress) -eq ($rInfo.main_after | ConvertTo-Json -Compress))
        & $add '(git-info) ok, main unchanged' ($rInfo.status -eq 'ok' -and $same) ("status=" + $rInfo.status + " same=" + $same)

        # --- gate: fmt fails on misformatted, passes on clean ------------------
        $fmtOut = Join-Path $tempQueue 'out/00000000-0000-fmt'
        New-Item -ItemType Directory -Force -Path $fmtOut | Out-Null
        $dirtyGate = Invoke-Gate -Repo $tempRepo -QueueDir $tempQueue -Sha $commits.dirty -Id '00000000-0000-fmt' -OutDir $fmtOut -OnlySteps @('fmt')
        $cleanGate = Invoke-Gate -Repo $tempRepo -QueueDir $tempQueue -Sha $commits.clean -Id '00000000-0000-fmt2' -OutDir $fmtOut -OnlySteps @('fmt')
        $dirtyFmt = @($dirtyGate.steps | Where-Object { $_.name -eq 'fmt' })[0]
        $cleanFmt = @($cleanGate.steps | Where-Object { $_.name -eq 'fmt' })[0]
        & $add '(gate) fmt fails on misformatted' ($dirtyGate.status -eq 'failed' -and $dirtyFmt.exit -ne 0) ("status=" + $dirtyGate.status + " exit=" + $dirtyFmt.exit)
        & $add '(gate) fmt passes on clean' ($cleanGate.status -eq 'ok' -and $cleanFmt.exit -eq 0) ("status=" + $cleanGate.status + " exit=" + $cleanFmt.exit + " tail=" + (($cleanFmt.tail -replace "`n", ' ').Trim()))

        # --- gate: gates.txt carries the test count from the full output -----
        $testOut = Join-Path $tempQueue 'out/00000000-0000-test'
        New-Item -ItemType Directory -Force -Path $testOut | Out-Null
        $testGate = Invoke-Gate -Repo $tempRepo -QueueDir $tempQueue -Sha $commits.clean -Id '00000000-0000-test' -OutDir $testOut -OnlySteps @('test')
        $gatesText = Read-Utf8Text -Path (Join-Path $testOut 'gates.txt')
        $hasTotal = ($gatesText -match 'total: 1 passed, 0 failed, 0 ignored')
        & $add '(gate) gates.txt test total' ($testGate.status -eq 'ok' -and $hasTotal) ("status=" + $testGate.status + " total=" + $hasTotal)

        # --- gates.txt parser on a synthetic failing run (no cargo) ----------
        $synthetic = @(
            'running 3 tests',
            'test a::ok ... ok',
            'test a::bad ... FAILED',
            'test a::skip ... ignored',
            '',
            'failures:',
            '',
            '---- a::bad stdout ----',
            "thread 'a::bad' panicked at src/lib.rs:9:5:",
            '    left: 1',
            '',
            'failures:',
            '    a::bad',
            '    b::worse',
            '',
            'test result: FAILED. 1 passed; 2 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s',
            'error: 1 target failed:',
            '    `-p demo --lib`'
        ) -join "`n"
        $sum = Get-TestSummary -Text $synthetic
        $namesOk = (($sum.failing -join ',') -eq 'a::bad,b::worse')
        $okParse = ($sum.passed -eq 1 -and $sum.failed -eq 2 -and $sum.ignored -eq 1 -and $namesOk -and $sum.failed_targets.Count -eq 1)
        & $add '(gate) failing tests parsed' $okParse ("p/f/i=" + $sum.passed + "/" + $sum.failed + "/" + $sum.ignored + " names=" + ($sum.failing -join ',') + " targets=" + ($sum.failed_targets -join ','))

        # --- a change to the main copy is recorded, not fatal ----------------
        $mutate = { param($repo) Add-Content -LiteralPath (Join-Path $repo 'src/lib.rs') -Value '// touched by selftest' }
        [void](& $submit '20260101-0026-mainchange' ([ordered]@{ id = '20260101-0026-mainchange'; kind = 'git-info'; rev = 'HEAD' }) $mutate $null)
        $rMain = & $report '20260101-0026-mainchange'
        [void](& $submit '20260101-0027-next' ([ordered]@{ id = '20260101-0027-next'; kind = 'git-info'; rev = 'HEAD' }))
        $rNext = & $report '20260101-0027-next'
        $okMain = ($rMain.status -eq 'ok' -and $rMain.main_changed.status -eq $true -and $rNext.status -eq 'ok')
        & $add '(main) change recorded, next runs' $okMain ("status=" + $rMain.status + " changed.status=" + $rMain.main_changed.status + " next=" + $rNext.status)
        [void](Invoke-Git -WorkDir $tempRepo -Arguments @('checkout', '--', 'src/lib.rs'))

        # --- corpus source changing during the copy is an error -------------
        # corpus-scan and xsd census reach the copy before any cargo call, so
        # a source change stops them with no build step in the report.
        $setDir = Join-Path $tempRepo 'testdata/CC0_DOCX'
        New-Item -ItemType Directory -Force -Path $setDir | Out-Null
        Write-Utf8NoBom -Path (Join-Path $setDir 'a.docx') -Text "not really a docx`n"
        $copyMutate = { param($source) if (Test-Path -LiteralPath (Join-Path $source 'a.docx')) { Add-Content -LiteralPath (Join-Path $source 'a.docx') -Value 'changed' } }
        [void](& $submit '20260101-0028-scanchange' ([ordered]@{ id = '20260101-0028-scanchange'; kind = 'corpus-scan'; sha = $short; set = 'CC0_DOCX' }) $null $copyMutate)
        $rScan = & $report '20260101-0028-scanchange'
        $scanCargo = @($rScan.steps | Where-Object { $_.name -eq 'corpus-scan' }).Count
        $okScan = ($rScan.status -eq 'error' -and $rScan.reason -like 'source changed during copy*' -and $scanCargo -eq 0 -and $rScan.main_changed.corpus -eq $true)
        & $add '(corpus-scan) source change errors' $okScan ("status=" + $rScan.status + " reason=" + $rScan.reason + " cargo_steps=" + $scanCargo)

        [void](& $submit '20260101-0029-censuschange' ([ordered]@{ id = '20260101-0029-censuschange'; kind = 'xsd'; sha = $short; gate = 'census' }) $null $copyMutate)
        $rCensus = & $report '20260101-0029-censuschange'
        $censusBuild = @($rCensus.steps | Where-Object { $_.name -eq 'build-writer' }).Count
        $okCensus = ($rCensus.status -eq 'error' -and $rCensus.reason -like 'source changed during copy*' -and $censusBuild -eq 0)
        & $add '(xsd census) source change errors' $okCensus ("status=" + $rCensus.status + " reason=" + $rCensus.reason + " build_steps=" + $censusBuild)

        # The worktree got the copy (after `clean -ffdx`, not before).
        $copiedFile = Join-Path $tempQueue 'wt/testdata/CC0_DOCX/a.docx'
        & $add '(copy) corpus copied into wt' (Test-Path -LiteralPath $copiedFile) ("exists=" + (Test-Path -LiteralPath $copiedFile))

        # --- pixels dispatches both steps (packages absent -> fast failure) --
        [void](& $submit '20260101-0030-pixels' ([ordered]@{ id = '20260101-0030-pixels'; kind = 'pixels'; sha = $short }))
        $rPix = & $report '20260101-0030-pixels'
        $pixNames = (@($rPix.steps | ForEach-Object { $_.name }) -join ',')
        & $add '(pixels) both steps run' ($rPix.status -eq 'failed' -and $pixNames -eq 'ssim,pdf_pixels') ("status=" + $rPix.status + " steps=" + $pixNames)

        # --- stale running job is reported, not deleted ----------------------
        $staleFile = Join-Path $tempQueue 'running/20260101-0031-stale.json'
        Write-JsonNoBom -Path $staleFile -Object ([ordered]@{ id = '20260101-0031-stale'; kind = 'ping' })
        $stale = @(Get-StaleRunningJobs -QueueDir $tempQueue)
        & $add '(q) stale running job detected' ($stale.Count -eq 1 -and $stale[0] -eq '20260101-0031-stale.json') ("stale=" + ($stale -join ','))
        Remove-Item -LiteralPath $staleFile -Force

        # --- a missing workdir fails loudly ---------------------------------
        $missing = Invoke-Git -WorkDir (Join-Path $root 'no-such-dir') -Arguments @('rev-parse', 'HEAD')
        & $add '(git) missing workdir fails loudly' ($missing.exit -ne 0) ("exit=" + $missing.exit)

        # --- a second instance on the same queue exits 3, inbox intact ------
        $lockStream = Acquire-QueueLock -QueueDir $tempQueue
        Write-Utf8NoBom -Path (Join-Path $tempQueue 'heartbeat.json') -Text (ConvertTo-Json -InputObject ([ordered]@{ pid = $PID; time_utc = (Get-Date).ToUniversalTime().ToString('o'); job = $null }) -Compress)
        $stillFile = Join-Path $inbox '20260101-0032-untouched.json'
        Write-JsonNoBom -Path $stillFile -Object ([ordered]@{ id = '20260101-0032-untouched'; kind = 'ping' })
        $second = Invoke-Logged -Name 'second-instance' -Exe 'powershell.exe' -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Repo', $tempRepo) -WorkDir $root -TimeoutSec 60 -OutDir (Join-Path $tempQueue 'out')
        $runningLeft = @(Get-ChildItem -LiteralPath (Join-Path $tempQueue 'running') -File -ErrorAction SilentlyContinue).Count
        $inboxIntact = (Test-Path -LiteralPath $stillFile) -and ($runningLeft -eq 0)
        $saidPid = $second.stdout -match ("already running \(pid {0}\)" -f $PID)
        $okSecond = (($null -ne $lockStream) -and $second.exit -eq 3 -and $saidPid -and $inboxIntact)
        & $add '(lock) second instance exits 3' $okSecond ("held=" + ($null -ne $lockStream) + " exit=" + $second.exit + " saidPid=" + $saidPid + " inboxIntact=" + $inboxIntact + " out=" + (($second.stdout -replace "`n", ' ').Trim()))
    }
    catch {
        & $add 'selftest harness' $false ($_.Exception.Message + ' @ ' + $_.InvocationInfo.PositionMessage)
    }
    finally {
        if ($lockStream) { $lockStream.Dispose() }
        # The worktree registration lives in the temp repo; removing the whole
        # temp root removes both.
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ''
    Write-Host 'hostq -SelfTest'
    Write-Host ('{0,-38} {1,-6} {2}' -f 'check', 'result', 'detail')
    $failed = 0
    foreach ($check in $checks) {
        if ($check.result -ne 'PASS') { $failed++ }
        Write-Host ('{0,-38} {1,-6} {2}' -f $check.check, $check.result, $check.detail)
    }
    Write-Host ''
    if ($failed -gt 0) {
        Write-Host ("SelfTest FAILED: {0} of {1} check(s)" -f $failed, $checks.Count)
        return 1
    }
    Write-Host ("SelfTest PASSED: {0} check(s)" -f $checks.Count)
    return 0
}

# ---------------------------------------------------------------------------
# Entry point.
# ---------------------------------------------------------------------------

$repoPath = [System.IO.Path]::GetFullPath($Repo)

if ($SelfTest) {
    exit (Invoke-SelfTest -Repo $repoPath)
}

if (-not (Test-Path -LiteralPath (Join-Path $repoPath '.git'))) {
    throw "not a git repository: $repoPath"
}

$queue = Join-Path $repoPath $script:QueueName
New-QueueLayout -QueueDir $queue

$script:LockStream = Acquire-QueueLock -QueueDir $queue
if ($null -eq $script:LockStream) {
    $pidText = 'unknown'
    $heartbeatPath = Join-Path $queue 'heartbeat.json'
    if (Test-Path -LiteralPath $heartbeatPath) {
        try { $pidText = (ConvertFrom-Json -InputObject (Read-Utf8Text -Path $heartbeatPath)).pid } catch { }
    }
    Write-Host ("hostq: already running (pid {0})" -f $pidText)
    exit 3
}
# Clear any stale current-job marker and publish the pid immediately, before
# the loop writes its first heartbeat, so a competing launch can name this
# process.
Write-CurrentJob -QueueDir $queue -Id ''
Write-Utf8NoBom -Path (Join-Path $queue 'heartbeat.json') -Text (ConvertTo-Json -InputObject ([ordered]@{ pid = $PID; time_utc = (Get-Date).ToUniversalTime().ToString('o'); job = $null }) -Compress)
Write-StaleRunningHint -QueueDir $queue

Start-Executor -Repo $repoPath -QueueDir $queue -AllowPush:$AllowPush
