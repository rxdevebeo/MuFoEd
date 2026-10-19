param([switch]$Cpu)
$ErrorActionPreference = 'Stop'
$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$plakatRoot = Join-Path (Split-Path $workspaceRoot -Parent) 'plakat'
$revision = 'ce0d67a45bc9b5f742c3772aef85afe0d15890f6'
if (-not (Test-Path -LiteralPath $plakatRoot)) {
    git clone https://github.com/vulogov/plakat.git $plakatRoot
    if ($LASTEXITCODE) { throw 'Clone failed' }
    git -C $plakatRoot checkout --detach $revision
    if ($LASTEXITCODE) { throw 'Checkout failed' }
}
$actual = git -C $plakatRoot rev-parse HEAD
if ($actual -ne $revision) { throw "Plakat checkout has another revision: $actual. Expected $revision" }
if (-not $Cpu) {
    $vswhere = 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
    $installation = & $vswhere -version '[17.0,18.0)' -latest -products '*' -property installationPath
    if (-not $installation) { throw 'MSVC 2022 is required by the installed CUDA 12.9 toolkit' }
    $vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
    $setupCommand = '"' + $vcvars + '" >nul && set'
    cmd /c $setupCommand | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
    }
    $compilerDirectory = (Get-Command cl.exe).Source | Split-Path -Parent
    $env:NVCC_CCBIN = $compilerDirectory
    # CUDA 12.9 rejects MSVC 14.44 by version despite supporting its toolchain ABI.
    $env:NVCC_PREPEND_FLAGS = '-allow-unsupported-compiler'
    $env:CUDA_COMPUTE_CAP = '120' # RTX 5070 Ti; override this script for another GPU.
}
Push-Location $plakatRoot
try {
    if ($Cpu) { cargo +1.92.0 build --release --locked --no-default-features }
    else { cargo +1.92.0 build --release --locked --no-default-features --features cuda }
    if ($LASTEXITCODE) { throw 'Plakat build failed' }
} finally { Pop-Location }
