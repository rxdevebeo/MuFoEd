param([int]$Runs = 6)
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path $PSScriptRoot -Parent
$workspaceRoot = Split-Path $editorRoot -Parent
Set-Location $workspaceRoot
. (Join-Path $PSScriptRoot 'enter-cuda.ps1')
. (Join-Path $PSScriptRoot 'enter-cudnn.ps1')
$artifacts = Join-Path $editorRoot 'artifacts'
$baseline = Join-Path $artifacts 'generation-bench-before-optimization.exe'
$candidate = Join-Path $workspaceRoot 'target/debug/examples/generation_bench.exe'
foreach ($name in @('CUDNN_CONV_WSCAP_DBG','BENCH_TARGET','BENCH_LORAS','BENCH_NO_TIMINGS','HIRES_SMOKE_BASE')) {
    Remove-Item -LiteralPath ('Env:' + $name) -ErrorAction SilentlyContinue
}
$env:PYTHONIOENCODING = 'utf-8'
$env:BENCH_STEPS = '50'
$env:BENCH_REFINE_STEPS = '20'
$env:BENCH_RUNS = "$Runs"
$environment = [ordered]@{
    timestamp = (Get-Date).ToUniversalTime().ToString('o')
    baseline_sha256 = (Get-FileHash -LiteralPath $baseline).Hash
    candidate_sha256 = (Get-FileHash -LiteralPath $candidate).Hash
    rust = (& rustc +1.92.0 --version)
    gpu = (& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader)
    flags = Get-Content -LiteralPath (Join-Path $editorRoot 'cargo-performance.toml') -Raw
    cudnn_workspace_cap = $env:CUDNN_CONV_WSCAP_DBG
    runs = $Runs
    source = @(Get-ChildItem (Join-Path $editorRoot 'src') -File -Recurse | Get-FileHash | Select-Object Path,Hash)
}
$environment | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $artifacts 'optimization-environment.json') -Encoding utf8
function Run-Bench([string]$Tag, [string]$Exe) {
    $env:IMAGE_STUDIO_TIMINGS = Join-Path $artifacts "$Tag.jsonl"
    $env:BENCH_RECEIPT = Join-Path $artifacts "$Tag-receipt.json"
    Write-Output "BEGIN $Tag $(Get-Date -Format o)"
    & $Exe *> (Join-Path $artifacts "$Tag-console.log")
    if ($LASTEXITCODE -ne 0) { Get-Content (Join-Path $artifacts "$Tag-console.log") -Tail 40; throw "$Tag failed: $LASTEXITCODE" }
    Copy-Item -LiteralPath (Join-Path $artifacts 'generation-bench-final.png') -Destination (Join-Path $artifacts "$Tag.png")
    Write-Output "COMPLETE $Tag $(Get-Date -Format o)"
}
Run-Bench 'optimization-before' $baseline
Run-Bench 'optimization-after' $candidate
$env:BENCH_RUNS = '2'
$env:BENCH_NO_TIMINGS = '1'
Run-Bench 'optimization-no-timings' $candidate
Remove-Item Env:BENCH_NO_TIMINGS -ErrorAction SilentlyContinue
$env:BENCH_STEPS = '10'
$env:BENCH_REFINE_STEPS = '10'
$env:BENCH_LORAS = '[{"path":"D:/Models/add-detail-xl.safetensors","weight":0.65,"enabled":true},{"path":"D:/Models/PerfectEyesXL.safetensors","weight":0.65,"enabled":true}]'
Run-Bench 'optimization-loras' $candidate
Remove-Item Env:BENCH_LORAS -ErrorAction SilentlyContinue
$env:IMAGE_STUDIO_SMOKE_STEPS = '4'
$env:IMAGE_STUDIO_TIMINGS = Join-Path $artifacts 'optimization-inpaint.jsonl'
$smoke = Join-Path $workspaceRoot 'target/debug/examples/native_smoke.exe'
& $smoke (Join-Path $editorRoot '.runtime/hub') (Join-Path $artifacts 'optimization-inpaint.png') (Join-Path $artifacts 'optimization-before.png') *> (Join-Path $artifacts 'optimization-inpaint-console.log')
if ($LASTEXITCODE -ne 0) { Get-Content (Join-Path $artifacts 'optimization-inpaint-console.log') -Tail 40; throw 'Inpaint smoke failed' }
Write-Output 'ALL GENERATION CHECKS COMPLETE'
