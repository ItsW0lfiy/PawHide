# Per-user upgrade/uninstall preflight. Never terminate user applications.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Destination,
    [switch]$RemoveStartup,
    [string]$ErrorFile
)
$ErrorActionPreference = 'Stop'
try {
    $cli = Join-Path $Destination 'pawhide.exe'
    if (Test-Path -LiteralPath $cli -PathType Leaf) {
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $cli
        $info.Arguments = 'auto stop'
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $process = [Diagnostics.Process]::Start($info)
        if (-not $process.WaitForExit(10000)) {
            $process.Kill()
            throw 'The monitor did not stop within 10 seconds. Close PawHide, then retry.'
        }
        $process.Dispose()
        try {
            $event = [Threading.EventWaitHandle]::OpenExisting('Local\PawHide.Auto.Stop')
            $event.Dispose()
            throw 'The background monitor is still running. Stop it before continuing.'
        } catch [Threading.WaitHandleCannotBeOpenedException] { }
    }
    foreach ($name in 'pawhide.exe', 'PawHideUI.exe', 'pawhide_agent.dll') {
        $path = Join-Path $Destination $name
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            try {
                $handle = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
                $handle.Dispose()
            } catch { throw "Cannot replace/remove $name. Close PawHide and protected applications, then retry." }
        }
    }
    if ($RemoveStartup) {
        $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
        $value = (Get-ItemProperty -LiteralPath $key -Name 'PawHide Auto' -ErrorAction SilentlyContinue).'PawHide Auto'
        $owned = $false
        if ($value -match '^"(?<Executable>[^"]+)"\s+auto\s+start$') {
            $registered = $Matches.Executable
            if ($registered.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) { $registered = '\\' + $registered.Substring(8) }
            elseif ($registered.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) { $registered = $registered.Substring(4) }
            try { $owned = [IO.Path]::GetFullPath($registered).Equals([IO.Path]::GetFullPath($cli), [StringComparison]::OrdinalIgnoreCase) }
            catch { $owned = $false } # Preserve malformed/unrecognized commands.
        }
        if ($owned) {
            Remove-ItemProperty -LiteralPath $key -Name 'PawHide Auto'
        }
    }
} catch {
    if ($ErrorFile) { [IO.File]::WriteAllText($ErrorFile, $_.Exception.Message) }
    Write-Error $_.Exception.Message -ErrorAction Continue
    exit 1
}

exit 0
