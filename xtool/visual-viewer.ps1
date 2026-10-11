param(
    [ValidateRange(1, 65535)][int]$Port = 8741,
    [switch]$Rebuild
)

$ErrorActionPreference = 'Stop'
$viewerRoot = Split-Path -Parent $PSScriptRoot
$viewerBinary = Join-Path $viewerRoot 'target/release/strict-ooxml-view.exe'
Push-Location $viewerRoot
try {
    if ($Rebuild -or -not (Test-Path -LiteralPath $viewerBinary)) {
        & cargo +1.92.0 build -p strict-ooxml-view --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Viewer build failed' }
    }
    & cargo +1.92.0 run -p strict-ooxml-render-svg --example graphics_gallery --locked
    if ($LASTEXITCODE -ne 0) { throw 'Gallery generation failed' }
    $viewerArguments = @('render_out/graphics-gallery')
    foreach ($viewerCorpus in @('strict-ooxml-core/tests/strict', 'strict-ooxml-core/tests/docx', 'testdata/CC0_DOCX', 'testdata/CC0_DOCX_MATH', 'testdata/CC0')) {
        if (Test-Path -LiteralPath $viewerCorpus) { $viewerArguments += $viewerCorpus }
    }
    $viewerArguments += @('--port', "$Port")
    & $viewerBinary @viewerArguments
    if ($LASTEXITCODE -ne 0) { throw 'Viewer stopped with an error' }
} finally {
    Pop-Location
}
