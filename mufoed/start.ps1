param(
    [switch]$Build,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Files
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path $PSScriptRoot -Parent
$executable = Join-Path $workspaceRoot 'target/release/mufoed.exe'
if ($Build -or -not (Test-Path -LiteralPath $executable)) {
    . (Join-Path $workspaceRoot 'image-editor/scripts/enter-cuda.ps1')
    . (Join-Path $workspaceRoot 'image-editor/scripts/enter-cudnn.ps1')
    Push-Location $workspaceRoot
    try {
        & cargo '+1.92.0' build -p mufoed --release --locked --config image-editor/cargo-performance.toml --features image-cudnn
        if ($LASTEXITCODE -ne 0) { throw 'MuFoEd build failed' }
    } finally { Pop-Location }
}
& $executable @Files | Out-Null
exit $LASTEXITCODE
