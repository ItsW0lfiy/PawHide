# A bounded real-application regression check. Restores an initially visible HWND.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Application,
    [string]$Cli = (Join-Path $PSScriptRoot '../target/release/pawhide.exe')
)
$ErrorActionPreference = 'Stop'
$cliPath = (Resolve-Path -LiteralPath $Cli).Path
$windows = @((& $cliPath list $Application --json | ConvertFrom-Json))
if ($windows.Count -ne 1) { throw 'Choose an application with exactly one visible top-level window.' }
$window = $windows[0]
if ($window.affinity -ne 0) { throw 'The regression check requires an initially visible window; existing privacy choices are preserved.' }
function Invoke-Bounded([bool]$Protect) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $cliPath
    $info.Arguments = 'apply-window {0} {1} "{2}" {3} false' -f $window.hwnd, $window.pid, $window.application, $Protect.ToString().ToLowerInvariant()
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit(10000)) { $process.Kill(); $process.WaitForExit(); throw 'Helper timed out; remote affinity remains uncertain.' }
        $details = $stderr.GetAwaiter().GetResult() + $stdout.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "Helper failed ($($process.ExitCode)): $details" }
    } finally { $process.Dispose() }
}
try {
    $started = [Diagnostics.Stopwatch]::StartNew()
    Invoke-Bounded $true
    $state = & $cliPath status $window.hwnd --json | ConvertFrom-Json
    if ($state.windows[0].affinity -ne 17) { throw 'Expected WDA_EXCLUDEFROMCAPTURE readback.' }
    "PASS: $Application affinity readback 0x11, elapsed $($started.ElapsedMilliseconds) ms. Capture output still requires visual verification."
} finally {
    # A failed remote call may have applied affinity before reporting failure.
    $state = & $cliPath status $window.hwnd --json | ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -and $state.windows[0].pid -eq $window.pid -and $state.windows[0].affinity -ne 0) {
        Invoke-Bounded $false
        $restored = & $cliPath status $window.hwnd --json | ConvertFrom-Json
        if ($restored.windows[0].affinity -ne 0) { throw 'Could not verify capture restoration.' }
        'PASS: original visible affinity restored.'
    }
}
