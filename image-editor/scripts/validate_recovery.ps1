# Native E04 crash QA. Uses only child processes created here and a fresh test directory.
param(
    [string]$Exe = "target/release/image-editor-demo.exe",
    [string]$Probe = "target/debug/examples/recovery_probe.exe",
    [string]$Source = "image-editor/artifacts/e04-fixture/original.psd",
    [string]$SettingsTemplate = "image-editor/artifacts/e04-fixture/settings.json",
    [string]$Output = ("image-editor/artifacts/e04-native-" + [guid]::NewGuid().ToString("N"))
)
$ErrorActionPreference = "Stop"
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$probePath = (Resolve-Path -LiteralPath $Probe).Path
$sourcePath = (Resolve-Path -LiteralPath $Source).Path
$testRoot = [IO.Path]::GetFullPath((Join-Path $PWD $Output))
if (Test-Path -LiteralPath $testRoot) { throw "Use a fresh output directory: $testRoot" }
New-Item -ItemType Directory -Path $testRoot | Out-Null
$settingsPath = Join-Path $testRoot "settings.json"
$settingsValue = Get-Content -Raw -LiteralPath $SettingsTemplate | ConvertFrom-Json
$settingsValue.request.steps = 23
$settingsValue.request.guidance = 7.25
$settingsValue | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
$sourceHash = (Get-FileHash -LiteralPath $sourcePath).Hash
$settingsHash = (Get-FileHash -LiteralPath $settingsPath).Hash
$names = @("IMAGE_STUDIO_SETTINGS", "IMAGE_STUDIO_RECOVERY_DIR", "IMAGE_STUDIO_RECOVERY_PROBE", "IMAGE_STUDIO_SHOT", "IMAGE_STUDIO_SHOT_DIALOG", "IMAGE_STUDIO_SHOT_SIZE", "WGPU_BACKEND", "PLAKAT_NO_SDPA")
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, "Process") }
$owner = $null
function Run-Shot([string]$name, [string]$size) {
    $env:IMAGE_STUDIO_SHOT = Join-Path $testRoot ($name + ".png")
    $env:IMAGE_STUDIO_SHOT_SIZE = $size
    & $exePath $sourcePath > (Join-Path $testRoot ($name + ".log")) 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Native screenshot failed: $name ($LASTEXITCODE)" }
    if (!(Test-Path -LiteralPath $env:IMAGE_STUDIO_SHOT)) { throw "Missing screenshot: $name" }
}
try {
    $env:WGPU_BACKEND = "dx12"
    $env:PLAKAT_NO_SDPA = "1"
    $env:IMAGE_STUDIO_SETTINGS = $settingsPath
    $env:IMAGE_STUDIO_RECOVERY_DIR = Join-Path $testRoot "drafts"
    $env:IMAGE_STUDIO_RECOVERY_PROBE = "draft"
    [Environment]::SetEnvironmentVariable("IMAGE_STUDIO_SHOT", $null, "Process")
    [Environment]::SetEnvironmentVariable("IMAGE_STUDIO_SHOT_DIALOG", $null, "Process")
    $owner = Start-Process -FilePath $exePath -ArgumentList ('"' + $sourcePath + '"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $testRoot "owner.stdout.log") -RedirectStandardError (Join-Path $testRoot "owner.stderr.log")
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    $snapshots = @()
    while ($snapshots.Count -eq 0) {
        if ($owner.HasExited) { throw "Draft owner exited before checkpoint: $($owner.ExitCode)" }
        if ([DateTime]::UtcNow -gt $deadline) { throw "Checkpoint deadline exceeded" }
        Start-Sleep -Milliseconds 100
        if (Test-Path -LiteralPath $env:IMAGE_STUDIO_RECOVERY_DIR) { $snapshots = @(Get-ChildItem -LiteralPath $env:IMAGE_STUDIO_RECOVERY_DIR -Filter "session-*.snapshot") }
    }
    [Environment]::SetEnvironmentVariable("IMAGE_STUDIO_RECOVERY_PROBE", $null, "Process")
    Run-Shot "live-window-skipped-1100" "1100x700"
    $ownerId = $owner.Id
    Stop-Process -Id $ownerId -Force
    $owner.WaitForExit()
    $owner = $null
    & $probePath verify $env:IMAGE_STUDIO_RECOVERY_DIR $sourcePath > (Join-Path $testRoot "exact-recovery.log") 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Checkpoint did not exactly restore the PSD document" }
    Run-Shot "recovery-1100" "1100x700"
    Run-Shot "recovery-1440" "1440x900"
    if (!(Test-Path -LiteralPath $snapshots[0].FullName)) { throw "Closing/defer deleted the abandoned copy" }
    $corruptDirectory = Join-Path $testRoot "corrupt"
    New-Item -ItemType Directory -Path $corruptDirectory | Out-Null
    $corruptFile = Join-Path $corruptDirectory "session-00000000000000000000000000000001.snapshot"
    [IO.File]::WriteAllText($corruptFile, "deliberately damaged isolated QA copy")
    $env:IMAGE_STUDIO_RECOVERY_DIR = $corruptDirectory
    Run-Shot "corrupt-1100" "1100x700"
    if (!(Test-Path -LiteralPath $corruptFile)) { throw "Corrupt draft deleted without explicit choice" }
    if ((Get-FileHash -LiteralPath $sourcePath).Hash -ne $sourceHash) { throw "Original PSD changed" }
    if ((Get-FileHash -LiteralPath $settingsPath).Hash -ne $settingsHash) { throw "Preferences changed" }
    [ordered]@{ status="PASS"; forced_exit=$true; owned_pid=$ownerId; source=$sourcePath; source_sha256=$sourceHash; preferences_sha256=$settingsHash; exact_recovery="pixels/layers/masks/selection/recipe/origin"; native_exits=@(0,0,0,0); abandoned_copy_preserved=$true; corrupt_copy_preserved=$true } | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $testRoot "receipt.json") -Encoding utf8NoBOM
    Write-Output "Native recovery crash/ownership/source/preferences PASS: $testRoot"
} finally {
    if ($null -ne $owner -and !$owner.HasExited) { Stop-Process -Id $owner.Id -Force; $owner.WaitForExit() }
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process") }
}
