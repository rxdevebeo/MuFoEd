# Requires the MSVC/CUDA environment from enter-cuda.ps1. Process-local setup only.
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path $PSScriptRoot -Parent
$pythonExe = if ($env:IMAGE_STUDIO_PYTHON) { $env:IMAGE_STUDIO_PYTHON } else { Join-Path $env:LOCALAPPDATA 'Programs/Python/Python311/python.exe' }
$cudnnFolder = if ($env:IMAGE_STUDIO_CUDNN_DIR) { $env:IMAGE_STUDIO_CUDNN_DIR } else {
    $found = & $pythonExe -c "import importlib.util,pathlib; s=importlib.util.find_spec('torch'); print(pathlib.Path(s.origin).parent/'lib' if s else '')"
    if ($LASTEXITCODE -ne 0 -or -not $found) { throw 'Cannot locate cuDNN; set IMAGE_STUDIO_CUDNN_DIR to its DLL directory' }
    $found.Trim()
}
$cudnnMain = Join-Path $cudnnFolder 'cudnn64_9.dll'
if (-not (Test-Path -LiteralPath $cudnnMain -PathType Leaf)) { throw "cuDNN 9 DLL not found: $cudnnMain" }
$identity = (Get-FileHash -LiteralPath $cudnnMain -Algorithm SHA256).Hash.ToLowerInvariant()
$importRoot = Join-Path $editorRoot ('.runtime/cudnn-import/' + $identity)
$importLibrary = Join-Path $importRoot 'cudnn.lib'
if (-not (Test-Path -LiteralPath $importLibrary -PathType Leaf)) {
    New-Item -ItemType Directory -Path $importRoot -Force | Out-Null
    # PyTorch bundles runtime DLLs without an import .lib. Generate the MSVC import
    # table from this exact local DLL; no DLLs or external installations are modified.
    $exports = & dumpbin.exe /NOLOGO /EXPORTS $cudnnMain
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read cuDNN exports' }
    $symbols = @($exports | ForEach-Object {
        if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(cudnn[A-Za-z0-9_]+)') { $matches[1] }
    } | Sort-Object -Unique)
    if ($symbols.Count -lt 20 -or 'cudnnCreate' -notin $symbols) { throw 'Unexpected cuDNN export table' }
    $definition = Join-Path $importRoot 'cudnn.def'
    @('LIBRARY cudnn64_9.dll','EXPORTS') + $symbols | Set-Content -LiteralPath $definition -Encoding ascii
    & lib.exe /NOLOGO /MACHINE:X64 "/DEF:$definition" "/OUT:$importLibrary"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot generate cuDNN import library' }
}
$env:LIB = $importRoot + ';' + $env:LIB
# Keep the selected CUDA toolkit ahead of PyTorch's other CUDA DLLs.
$env:PATH = $env:PATH + ';' + $cudnnFolder
Write-Host "cuDNN 9: $cudnnFolder"
