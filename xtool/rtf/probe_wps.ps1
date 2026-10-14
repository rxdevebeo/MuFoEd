param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$rtfProbeOutput = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $rtfProbeOutput) { throw 'Probe output must be a new directory' }
New-Item -ItemType Directory -Path $rtfProbeOutput -Force | Out-Null
$rtfExistingWpsIds = @(Get-Process wps -ErrorAction SilentlyContinue | ForEach-Object Id)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RtfOracleWindowPid {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
}
'@
$rtfWpsApp = $null
$rtfProbeDocument = $null
$rtfProbeOwnsProcess = $false
$rtfProbeReport = [ordered]@{version=1; status='Blocked'; existing_processes=$rtfExistingWpsIds; prog_id='KWPS.Application'}
try {
    $rtfWpsApp = New-Object -ComObject 'KWPS.Application'
    $rtfProbeInput = Join-Path $rtfProbeOutput 'fixture.rtf'
    [IO.File]::WriteAllBytes($rtfProbeInput, [Text.Encoding]::ASCII.GetBytes('{\rtf1\ansi Hello \b bold\b0\par Unicode \uc1\u1046?\par}'))
    # A hidden, uniquely named fixture gives a window handle without modifying
    # application-wide settings. Only this document may be closed before ownership.
    $rtfProbeDocument = $rtfWpsApp.Documents.Open($rtfProbeInput, $false, $true, $false,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, [Type]::Missing,
        [Type]::Missing, [Type]::Missing, [Type]::Missing, $false)
    $rtfProbeHwnd = [IntPtr]([long]$rtfWpsApp.Hwnd)
    if ($rtfProbeHwnd -eq [IntPtr]::Zero) { $rtfProbeHwnd = [IntPtr]([long]$rtfProbeDocument.Windows.Item(1).Hwnd) }
    $rtfWpsProcessId = [uint32]0
    [void][RtfOracleWindowPid]::GetWindowThreadProcessId($rtfProbeHwnd, [ref]$rtfWpsProcessId)
    $rtfProbeReport.hwnd = $rtfProbeHwnd.ToInt64()
    $rtfProbeReport.process_id = $rtfWpsProcessId
    $rtfProbeReport.document_count = $rtfWpsApp.Documents.Count
    $rtfProbeReport.application_version = [string]$rtfWpsApp.Version
    if ($rtfWpsProcessId -eq 0 -or $rtfExistingWpsIds -contains $rtfWpsProcessId -or $rtfWpsApp.Documents.Count -ne 1) {
        $rtfProbeReport.reason = 'COM process ownership not established; existing applications left untouched'
    } else {
        $rtfProbeOwnsProcess = $true
        $rtfProbeProcess = Get-Process -Id $rtfWpsProcessId
        $rtfProbeReport.executable = $rtfProbeProcess.Path
        $rtfProbeReport.file_version = (Get-Item -LiteralPath $rtfProbeProcess.Path).VersionInfo.FileVersion
        $rtfProbeReport.executable_sha256 = (Get-FileHash -LiteralPath $rtfProbeProcess.Path -Algorithm SHA256).Hash.ToLowerInvariant()
        $rtfWpsApp.Visible = $false
        $rtfWpsApp.DisplayAlerts = 0
        $rtfWpsApp.AutomationSecurity = 3
        try {
            $rtfProbeReport.direct_text = [string]$rtfProbeDocument.Content.Text
            $rtfProbeDocx = Join-Path $rtfProbeOutput 'fixture.docx'
            $rtfProbeDocument.SaveAs2($rtfProbeDocx, 12)
        } finally { $rtfProbeDocument.Close(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfProbeDocument); $rtfProbeDocument = $null }
        $rtfProbeReport.output_sha256 = (Get-FileHash -LiteralPath $rtfProbeDocx -Algorithm SHA256).Hash.ToLowerInvariant()
        $rtfProbeReport.status = 'Captured'
    }
} catch {
    $rtfProbeReport.reason = $_.Exception.Message
} finally {
    if ($rtfProbeDocument) { try { $rtfProbeDocument.Close(0) } catch {}; [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfProbeDocument) }
    if ($rtfWpsApp) {
        if ($rtfProbeOwnsProcess) { try { $rtfWpsApp.Quit(0) } catch {} }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($rtfWpsApp)
    }
    $rtfProbeReport | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $rtfProbeOutput 'probe.json') -Encoding utf8
}
$rtfProbeReport | ConvertTo-Json -Depth 6
if ($rtfProbeReport.status -eq 'Captured') { exit 0 } else { exit 2 }
