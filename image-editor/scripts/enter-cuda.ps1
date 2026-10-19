$ErrorActionPreference = 'Stop'
$vswhere = 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -version '[17.0,18.0)' -latest -products '*' -property installationPath
if (-not $installation) { throw 'MSVC 2022 is required by the installed CUDA 12.9 toolkit' }
$vcvars = Join-Path $installation 'VC/Auxiliary/Build/vcvars64.bat'
$setupCommand = '"' + $vcvars + '" >nul && set'
cmd /c $setupCommand | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$env:NVCC_CCBIN = (Get-Command cl.exe).Source | Split-Path -Parent
$env:NVCC_PREPEND_FLAGS = '-allow-unsupported-compiler'
if (-not $env:CUDA_COMPUTE_CAP) { $env:CUDA_COMPUTE_CAP = '120' } # This machine: RTX 5070 Ti.
