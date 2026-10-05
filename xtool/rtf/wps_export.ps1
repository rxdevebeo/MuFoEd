param(
    [Parameter(Mandatory=$true)][string]$InputPath,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [switch]$ExportPdf
)
$ErrorActionPreference = 'Stop'
function Get-RtfExportSha256([string]$LiteralPath) {
    $rtfHashStream = [IO.File]::OpenRead($LiteralPath)
    $rtfHashAlgorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($rtfHashAlgorithm.ComputeHash($rtfHashStream)).Replace('-', '').ToLowerInvariant() }
    finally { $rtfHashStream.Dispose(); $rtfHashAlgorithm.Dispose() }
}
$rtfWpsOutput = [IO.Path]::GetFullPath($OutputDirectory)
$rtfWpsInput = [IO.Path]::GetFullPath($InputPath)
if (!(Test-Path -LiteralPath $rtfWpsOutput)) { throw 'Supervisor must create output directory' }
if (Test-Path -LiteralPath (Join-Path $rtfWpsOutput 'result.json')) { throw 'Result already exists' }
$rtfExistingIds = @(Get-Process wps -ErrorAction SilentlyContinue | ForEach-Object Id)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RtfWpsExportWindowPid {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
}
'@
$rtfWpsApplication = $null
$rtfWpsDocument = $null
$rtfOwnsApplication = $false
$rtfExportResult = [ordered]@{version=1; status='Blocked'; input=$rtfWpsInput; prog_id='KWPS.Application'}
try {
    $rtfWpsApplication = New-Object -ComObject 'KWPS.Application'
    $rtfBootstrap = Join-Path $rtfWpsOutput 'bootstrap.rtf'
    [IO.File]::WriteAllBytes($rtfBootstrap, [Text.Encoding]::ASCII.GetBytes('{\rtf1 bootstrap}'))
    $rtfWpsDocument = $rtfWpsApplication.Documents.Open($rtfBootstrap, $false, $true, $false,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, [Type]::Missing,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, $false)
    $rtfExportHwnd = [IntPtr]([long]$rtfWpsDocument.Windows.Item(1).Hwnd)
    $rtfExportPid = [uint32]0
    [void][RtfWpsExportWindowPid]::GetWindowThreadProcessId($rtfExportHwnd, [ref]$rtfExportPid)
    if ($rtfExportPid -eq 0 -or $rtfExistingIds -contains $rtfExportPid -or $rtfWpsApplication.Documents.Count -ne 1) {
        throw 'Separate empty WPS process ownership not established'
    }
    $rtfOwnsApplication = $true
    $rtfExportProcess = Get-Process -Id $rtfExportPid
    $rtfOwnership = [ordered]@{process_id=$rtfExportPid; start_ticks=$rtfExportProcess.StartTime.ToUniversalTime().Ticks;
                              executable=$rtfExportProcess.Path; before_ids=$rtfExistingIds}
    $rtfOwnership | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $rtfWpsOutput 'ownership.json') -Encoding utf8
    $rtfExportResult.oracle = [ordered]@{application='WPS Writer'; application_version=[string]$rtfWpsApplication.Version;
        file_version=(Get-Item -LiteralPath $rtfExportProcess.Path).VersionInfo.FileVersion;
        executable=$rtfExportProcess.Path; executable_sha256=(Get-RtfExportSha256 $rtfExportProcess.Path);
        save_format=12; open_readonly=$true; open_visible=$false; add_to_recent_files=$false; process_id=$rtfExportPid}
    $rtfWpsApplication.Visible = $false
    $rtfWpsApplication.DisplayAlerts = 0
    $rtfWpsApplication.AutomationSecurity = 3
    $rtfWpsDocument.Close(0)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfWpsDocument)
    $rtfWpsDocument = $null
    $rtfWpsDocument = $rtfWpsApplication.Documents.Open($rtfWpsInput, $false, $true, $false,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, [Type]::Missing,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, $false)
    $rtfRawText = [string]$rtfWpsDocument.Content.Text
    [IO.File]::WriteAllText((Join-Path $rtfWpsOutput 'content.txt'), $rtfRawText, (New-Object Text.UTF8Encoding($false)))
    $rtfExportResult.counts = [ordered]@{paragraphs=$rtfWpsDocument.Paragraphs.Count; tables=$rtfWpsDocument.Tables.Count;
        sections=$rtfWpsDocument.Sections.Count; inline_shapes=$rtfWpsDocument.InlineShapes.Count;
        shapes=$rtfWpsDocument.Shapes.Count; footnotes=$rtfWpsDocument.Footnotes.Count; endnotes=$rtfWpsDocument.Endnotes.Count}
    $rtfWpsDocxPath = Join-Path $rtfWpsOutput 'reference.docx'
    if ($ExportPdf) {
        $rtfWpsPdfPath = Join-Path $rtfWpsOutput 'reference.pdf'
        $rtfWpsDocument.ExportAsFixedFormat($rtfWpsPdfPath, 17)
        $rtfExportResult.pdf_sha256 = Get-RtfExportSha256 $rtfWpsPdfPath
        $rtfExportResult.oracle.pdf_export_format = 17
    }
    $rtfWpsDocument.SaveAs2($rtfWpsDocxPath, 12, [Type]::Missing, [Type]::Missing, $false)
    $rtfWpsDocument.Close(0)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfWpsDocument)
    $rtfWpsDocument = $null
    $rtfExportResult.docx_sha256 = Get-RtfExportSha256 $rtfWpsDocxPath
    $rtfExportResult.content_sha256 = Get-RtfExportSha256 (Join-Path $rtfWpsOutput 'content.txt')
    $rtfExportResult.status = 'Captured'
} catch {
    $rtfExportResult.reason = $_.Exception.Message
} finally {
    if ($rtfWpsDocument) { try { $rtfWpsDocument.Close(0) } catch {}; [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfWpsDocument) }
    if ($rtfWpsApplication) {
        if ($rtfOwnsApplication) { try { $rtfWpsApplication.Quit(0) } catch {} }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfWpsApplication)
    }
    $rtfExportResult | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $rtfWpsOutput 'result.json') -Encoding utf8
}
if ($rtfExportResult.status -eq 'Captured') { exit 0 } else { exit 2 }
