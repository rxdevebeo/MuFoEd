param(
    [switch]$Build,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Files
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path $PSScriptRoot -Parent
$executable = Join-Path $workspaceRoot 'target/release/mufoed.exe'
$env:WGPU_BACKEND = 'dx12'
$env:PLAKAT_NO_SDPA = '1'
. (Join-Path $workspaceRoot 'image-editor/scripts/enter-cuda.ps1')
. (Join-Path $workspaceRoot 'image-editor/scripts/enter-cudnn.ps1')
if ($Build -or -not (Test-Path -LiteralPath $executable)) {
    Push-Location $workspaceRoot
    try {
        & cargo '+1.92.0' build -p mufoed --release --locked --config image-editor/cargo-performance.toml --features image-cudnn
        if ($LASTEXITCODE -ne 0) { throw 'MuFoEd build failed' }
    } finally { Pop-Location }
}
& $executable @Files | Out-Null
exit $LASTEXITCODE
