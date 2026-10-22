param([string]$Image,[switch]$Cli,[switch]$LegacyCuda,[switch]$Release)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path $PSScriptRoot -Parent
$plakatRoot = Join-Path (Split-Path $workspaceRoot -Parent) 'plakat'
$env:IMAGE_STUDIO_PLAKAT = Join-Path $plakatRoot 'target/release/plakat.exe'
$env:IMAGE_STUDIO_CACHE = Join-Path $PSScriptRoot '.runtime/hub'
$env:IMAGE_STUDIO_MODEL = 'local/image-studio-sdxl'
$env:PLAKAT_CACHE_DIR = $env:IMAGE_STUDIO_CACHE
$env:PLAKAT_NO_SDPA = '1'
$env:WGPU_BACKEND = 'dx12'
$buildProfile = if ($Release) { 'release' } else { 'debug' }
$executable = Join-Path $workspaceRoot ('target/' + $buildProfile + '/image-editor-demo.exe')
$performanceConfig = Join-Path $PSScriptRoot 'cargo-performance.toml'
Push-Location $workspaceRoot
try {
    $backendFeatures = 'demo'
    if (-not $Cli) {
        . (Join-Path $PSScriptRoot 'scripts/enter-cuda.ps1')
        $backendFeatures = 'demo,plakat-cuda'
        if (-not $LegacyCuda) {
            try {
                . (Join-Path $PSScriptRoot 'scripts/enter-cudnn.ps1')
                $backendFeatures = 'demo,plakat-cudnn'
            } catch {
                Write-Warning ("cuDNN setup unavailable; using CUDA: " + $_.Exception.Message)
            }
        }
    }
    Write-Host "Image Studio: $backendFeatures, optimized inference build"
    $buildArguments = @('+1.92.0','build','-p','image-editor','--features',$backendFeatures,'--bin','image-editor-demo','--config',$performanceConfig)
    if ($Release) { $buildArguments += '--release' }
    & cargo @buildArguments
    if ($LASTEXITCODE) { throw 'Editor build failed' }
} finally { Pop-Location }
if ($Image) { & $executable $Image } else { & $executable }
exit $LASTEXITCODE
