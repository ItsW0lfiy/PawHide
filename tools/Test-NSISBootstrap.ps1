#Requires -Version 5.1
# Offline dependency-provisioning fixtures. Never execute the fixture compiler.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Resolve-NSIS.ps1')
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$testParent = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../target'))
$fixtureRoot = Join-Path $testParent ('nsis-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$fixtureZip = Join-Path $fixtureRoot 'fixture.zip'
$fixturePackage = Join-Path $fixtureRoot 'fixture.nupkg'
$savedOverride = $env:PAWHIDE_MAKENSIS
$downloadMode = 'direct'
$requests = 0

function Invoke-WebRequest {
    param([uri]$Uri, [string]$OutFile, [switch]$UseBasicParsing, [int]$TimeoutSec)
    $script:requests++
    if ($downloadMode -eq 'offline') { throw 'Controlled offline failure' }
    if ($downloadMode -eq 'bad') { [IO.File]::WriteAllText($OutFile, 'untrusted download'); return }
    if ($Uri.Host -eq 'community.chocolatey.org') {
        Copy-Item -LiteralPath $fixturePackage -Destination $OutFile
    } elseif ($downloadMode -eq 'mirror') {
        [IO.File]::WriteAllText($OutFile, '<html>download landing page</html>')
    } else { Copy-Item -LiteralPath $fixtureZip -Destination $OutFile }
}

function Expect-Failure([scriptblock]$Action, [string]$Message) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    if (-not $failed) { throw $Message }
}

try {
    $zip = [IO.Compression.ZipFile]::Open($fixtureZip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($name in @('makensis.exe', 'Bin/makensis.exe', 'Include/MUI2.nsh', 'nsisconf.nsh')) {
            $writer = [IO.StreamWriter]::new($zip.CreateEntry("nsis-3.13/$name").Open())
            $writer.Write("controlled fixture $name"); $writer.Dispose()
        }
    } finally { $zip.Dispose() }
    $fixtureHash = (Get-FileHash -LiteralPath $fixtureZip -Algorithm SHA256).Hash
    $package = [IO.Compression.ZipFile]::Open($fixturePackage, [IO.Compression.ZipArchiveMode]::Create)
    try { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($package, $fixtureZip, 'tools/nsis-3.13.zip') | Out-Null }
    finally { $package.Dispose() }

    $cache = Join-Path $fixtureRoot 'direct'
    $compiler = Get-PawHideCachedNsis -CacheRoot $cache -Version '3.13' -ExpectedHash $fixtureHash
    if (-not (Test-Path -LiteralPath $compiler) -or $requests -ne 1) { throw 'Fresh provisioning failed' }
    'PASS: missing compiler downloads, verifies and extracts automatically.'

    $downloadMode = 'offline'; $requests = 0
    $reused = Get-PawHideCachedNsis -CacheRoot $cache -Version '3.13' -ExpectedHash $fixtureHash
    if ($reused -ne $compiler -or $requests -ne 0) { throw 'Offline cache reuse failed' }
    [IO.File]::WriteAllText($compiler, 'corrupt compiler')
    Get-PawHideCachedNsis -CacheRoot $cache -Version '3.13' -ExpectedHash $fixtureHash | Out-Null
    if ([IO.File]::ReadAllText($compiler) -ne 'controlled fixture makensis.exe' -or $requests -ne 0) { throw 'Cached extraction was not repaired offline' }
    'PASS: subsequent build reuses cache offline and repairs modified files from the verified archive.'

    $downloadMode = 'mirror'; $requests = 0
    Get-PawHideCachedNsis -CacheRoot (Join-Path $fixtureRoot 'mirror') -Version '3.13' -ExpectedHash $fixtureHash | Out-Null
    if ($requests -ne 2) { throw 'HTML response did not trigger the explicit verified mirror' }
    'PASS: HTML response rejected; mirror inner ZIP verified before extraction.'

    $downloadMode = 'bad'
    $badCache = Join-Path $fixtureRoot 'bad'
    Expect-Failure { Get-PawHideCachedNsis -CacheRoot $badCache -Version '3.13' -ExpectedHash $fixtureHash } 'Bad checksum was accepted'
    if (Test-Path -LiteralPath (Join-Path $badCache 'nsis-3.13')) { throw 'Untrusted archive was extracted' }
    [IO.File]::WriteAllText((Join-Path $cache 'nsis-3.13.zip'), 'corrupt cached archive')
    $downloadMode = 'offline'
    Expect-Failure { Get-PawHideCachedNsis -CacheRoot $cache -Version '3.13' -ExpectedHash $fixtureHash } 'Corrupt archive was reused offline'
    if (@(Get-ChildItem -LiteralPath $cache -Force | Where-Object { $_.Name -like '.nsis-bootstrap-*' }).Count -ne 0) { throw 'Failed provisioning left temporary content' }
    'PASS: bad downloads and corrupt offline cache fail without using an unverified compiler.'

    $env:PAWHIDE_MAKENSIS = $compiler
    if ((Get-PawHideNsis -RepositoryRoot $fixtureRoot) -ne $compiler) { throw 'Explicit override was ignored' }
    $env:PAWHIDE_MAKENSIS = Join-Path $fixtureRoot 'missing.exe'
    Expect-Failure { Get-PawHideNsis -RepositoryRoot $fixtureRoot } 'Missing explicit override was silently ignored'
    'PASS: explicit override is respected and a broken override fails clearly.'
} finally {
    $env:PAWHIDE_MAKENSIS = $savedOverride
    $resolved = [IO.Path]::GetFullPath($fixtureRoot)
    if ([IO.Path]::GetDirectoryName($resolved) -eq $testParent -and [IO.Path]::GetFileName($resolved) -match '^nsis-test-[0-9a-f]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
