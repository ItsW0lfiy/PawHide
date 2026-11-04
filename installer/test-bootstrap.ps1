# Offline fixtures for the real bootstrap. No network, registry or user install.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$targetRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../target')).TrimEnd('\') + '\'
$testRoot = Join-Path $targetRoot ('installer-test-' + [guid]::NewGuid().ToString('N'))
$fixtureZip = Join-Path $testRoot 'payload.zip'
$bootstrap = Join-Path $PSScriptRoot 'bootstrap.ps1'
$badHash = $false
$failCopy = $false
$copyFailures = [Collections.Generic.HashSet[string]]::new()
New-Item -ItemType Directory -Path $testRoot | Out-Null

function Invoke-WebRequest {
    param([uri]$Uri, [string]$OutFile, [switch]$UseBasicParsing, [int]$TimeoutSec, [int]$MaximumRedirection)
    if ($Uri.Host -eq 'api.github.com') {
        $assets = foreach ($name in 'PawHide-Windows-x64.zip', 'PawHide-Windows-x64.zip.sha256') {
            @{ name = $name; browser_download_url = "https://github.com/ItsW0lfiy/PawHide/releases/download/test/$name" }
        }
        return [pscustomobject]@{ Content = (@{ assets = @($assets) } | ConvertTo-Json -Depth 5) }
    }
    if ($Uri.AbsolutePath.EndsWith('.sha256')) {
        $hash = if ($badHash) { '0' * 64 } else { (Get-FileHash -LiteralPath $fixtureZip -Algorithm SHA256).Hash }
        [IO.File]::WriteAllText($OutFile, "$hash  PawHide-Windows-x64.zip")
    } else { Microsoft.PowerShell.Management\Copy-Item -LiteralPath $fixtureZip -Destination $OutFile }
}

function Copy-Item {
    param([string]$LiteralPath, [string]$Destination, [switch]$Force)
    $failTarget = $Destination -in @((Join-Path $testRoot 'rollback/PawHideUI.exe'), (Join-Path $testRoot 'upgrade/PawHideUI.exe'))
    if ($failCopy -and $failTarget -and $copyFailures.Add($Destination)) { throw 'Controlled copy failure' }
    Microsoft.PowerShell.Management\Copy-Item -LiteralPath $LiteralPath -Destination $Destination -Force:$Force
}

function New-Payload([string[]]$Names) {
    if (Test-Path -LiteralPath $fixtureZip) { Remove-Item -LiteralPath $fixtureZip }
    $zip = [IO.Compression.ZipFile]::Open($fixtureZip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($name in $Names) {
            $writer = [IO.StreamWriter]::new($zip.CreateEntry($name).Open())
            $writer.Write('controlled installer fixture'); $writer.Dispose()
        }
    } finally { $zip.Dispose() }
}

function Invoke-TestInstall([string]$Name, [bool]$Success) {
    & $bootstrap -Destination (Join-Path $testRoot $Name) -Repository 'ItsW0lfiy/PawHide' -ErrorFile (Join-Path $testRoot 'error.txt') 2>$null | Out-Null
    if (($LASTEXITCODE -eq 0) -ne $Success) { throw "Unexpected result for $Name : $(Get-Content (Join-Path $testRoot 'error.txt') -ErrorAction SilentlyContinue)" }
}

try {
    $names = @('pawhide.exe', 'PawHideUI.exe', 'pawhide_agent.dll', 'README.md')
    New-Payload $names
    Invoke-TestInstall 'fresh' $true
    foreach ($name in $names[0..2]) { if (-not (Test-Path -LiteralPath (Join-Path $testRoot "fresh/$name"))) { throw 'Incomplete install' } }
    $badHash = $true
    Invoke-TestInstall 'tampered' $false
    if (Test-Path -LiteralPath (Join-Path $testRoot 'tampered')) { throw 'Checksum failure wrote destination files' }
    $badHash = $false
    New-Payload ($names + '../outside.txt')
    Invoke-TestInstall 'traversal' $false
    if (Test-Path -LiteralPath (Join-Path $testRoot 'outside.txt')) { throw 'ZIP traversal escaped extraction' }
    New-Payload @('pawhide.exe', 'PawHideUI.exe')
    Invoke-TestInstall 'missing' $false
    New-Payload $names
    New-Item -ItemType Directory -Path (Join-Path $testRoot 'rollback') | Out-Null
    [IO.File]::WriteAllText((Join-Path $testRoot 'rollback/keep.txt'), 'unrelated user file')
    $failCopy = $true
    Invoke-TestInstall 'rollback' $false
    if (Test-Path -LiteralPath (Join-Path $testRoot 'rollback/pawhide.exe')) { throw 'Partial first install was not rolled back' }
    if ([IO.File]::ReadAllText((Join-Path $testRoot 'rollback/keep.txt')) -ne 'unrelated user file') { throw 'Unrelated file changed' }
    'PASS: fresh install, SHA-256 rejection, traversal rejection, missing component, and copy rollback.'
    $monitorRunning = $false
    try { $event = [Threading.EventWaitHandle]::OpenExisting('Local\PawHide.Auto.Stop'); $event.Dispose(); $monitorRunning = $true }
    catch [Threading.WaitHandleCannotBeOpenedException] { }
    if ($monitorRunning) {
        'SKIP: upgrade rollback fixture requires AutoHide stopped; the existing monitor was left untouched.'
    } else {
        New-Item -ItemType Directory -Path (Join-Path $testRoot 'upgrade') | Out-Null
        # This local fixture executable never signals the real monitor event.
        Add-Type -TypeDefinition 'public class MaintenanceFixture { public static int Main(string[] args) { return 1; } }' -OutputAssembly (Join-Path $testRoot 'upgrade/pawhide.exe') -OutputType ConsoleApplication
        [IO.File]::WriteAllText((Join-Path $testRoot 'upgrade/PawHideUI.exe'), 'previous UI')
        [IO.File]::WriteAllText((Join-Path $testRoot 'upgrade/pawhide_agent.dll'), 'previous agent')
        $before = @{}
        foreach ($name in $names[0..2]) { $before[$name] = (Get-FileHash -LiteralPath (Join-Path $testRoot "upgrade/$name") -Algorithm SHA256).Hash }
        Invoke-TestInstall 'upgrade' $false
        foreach ($name in $names[0..2]) {
            if ((Get-FileHash -LiteralPath (Join-Path $testRoot "upgrade/$name") -Algorithm SHA256).Hash -ne $before[$name]) { throw "Upgrade rollback changed $name" }
        }
        'PASS: failed upgrade restored all three previous binaries byte for byte.'
    }
    $startupValue = ''
    $removedStartup = [Collections.Generic.List[string]]::new()
    function Get-ItemProperty {
        param([string]$LiteralPath, [string]$Name, [string]$ErrorAction)
        return [pscustomobject]@{ 'PawHide Auto' = $startupValue }
    }
    function Remove-ItemProperty {
        param([string]$LiteralPath, [string]$Name)
        $removedStartup.Add($Name)
    }
    $startupDirectory = Join-Path $testRoot 'startup'
    New-Item -ItemType Directory -Path $startupDirectory | Out-Null
    $startupExe = Join-Path $startupDirectory 'pawhide.exe'
    foreach ($case in @(
        @{ Value = ('"' + $startupExe + '" auto start'); Expected = 1 },
        @{ Value = ('"\\?\' + $startupExe + '" auto start'); Expected = 1 },
        @{ Value = '"C:\Unrelated\other.exe" auto start'; Expected = 0 },
        @{ Value = ('"' + $startupExe + '" auto start --unrecognized'); Expected = 0 }
    )) {
        $startupValue = $case.Value
        $beforeCount = $removedStartup.Count
        & (Join-Path $PSScriptRoot 'maintenance.ps1') -Destination $startupDirectory -RemoveStartup
        if ($LASTEXITCODE -ne 0 -or $removedStartup.Count - $beforeCount -ne $case.Expected) { throw 'Startup ownership matching failed' }
    }
    'PASS: normal/legacy startup paths removed; unrelated startup commands preserved (mock registry).'

} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if ($resolved.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($resolved) -match '^installer-test-[0-9a-f]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
