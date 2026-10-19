param([string]$Image,[switch]$Cli)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path $PSScriptRoot -Parent
$plakatRoot = Join-Path (Split-Path $workspaceRoot -Parent) 'plakat'
$env:IMAGE_STUDIO_PLAKAT = Join-Path $plakatRoot 'target/release/plakat.exe'
$env:IMAGE_STUDIO_CACHE = Join-Path $PSScriptRoot '.runtime/hub'
$env:IMAGE_STUDIO_MODEL = 'local/image-studio-sdxl'
$env:PLAKAT_CACHE_DIR = $env:IMAGE_STUDIO_CACHE
$env:PLAKAT_NO_SDPA = '1'
$env:WGPU_BACKEND = 'dx12'
$executable = Join-Path $workspaceRoot 'target/debug/image-editor-demo.exe'
Push-Location $workspaceRoot
try {
    if ($Cli) { cargo +1.92.0 build -p image-editor --features demo --bin image-editor-demo }
    else {
        . (Join-Path $PSScriptRoot 'scripts/enter-cuda.ps1')
        cargo +1.92.0 build -p image-editor --features demo,plakat-cuda --bin image-editor-demo --config 'profile.dev.package.plakat.opt-level=2'
    }
    if ($LASTEXITCODE) { throw 'Editor build failed' }
} finally { Pop-Location }
if ($Image) { & $executable $Image } else { & $executable }
