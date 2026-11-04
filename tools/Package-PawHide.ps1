#Requires -Version 5.1
# Invoked by `cargo all`. Creates the portable ZIP + SHA-256 and NSIS online
# installer from the SAME release build. Does not publish or install anything.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'PawHide installer packaging requires Windows.'
}
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$release = Join-Path $root 'target\release'
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist 'portable'

. (Join-Path $PSScriptRoot 'Resolve-NSIS.ps1')
$nsis = Get-PawHideNsis -RepositoryRoot $root

$executables = @('pawhide.exe', 'PawHideUI.exe', 'pawhide_agent.dll')
foreach ($name in $executables) {
    if (-not (Test-Path -LiteralPath (Join-Path $release $name) -PathType Leaf)) {
        throw "Missing release artifact $name. Run cargo all from the PawHide workspace."
    }
}
New-Item -ItemType Directory -Path $dist -Force | Out-Null
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Force -Recurse }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

foreach ($name in $executables) {
    Copy-Item -LiteralPath (Join-Path $release $name) -Destination (Join-Path $stage $name)
}
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $stage 'README.md')
New-Item -ItemType Directory -Path (Join-Path $stage 'docs'), (Join-Path $stage 'ui\assets') -Force | Out-Null
foreach ($name in @('CAPTURE_BOUNDARY.md', 'RELEASE_READINESS.md')) {
    Copy-Item -LiteralPath (Join-Path $root "docs\$name") -Destination (Join-Path $stage "docs\$name")
}
Copy-Item -LiteralPath (Join-Path $root 'ui\assets\pawhide.png') -Destination (Join-Path $stage 'ui\assets\pawhide.png')

$archiveName = 'PawHide-Windows-x64.zip'
$archive = Join-Path $dist $archiveName
$manifest = Join-Path $dist "$archiveName.sha256"
$setup = Join-Path $dist 'PawHide-setup.exe'
foreach ($path in @($archive, $manifest, $setup)) {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
}

Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -CompressionLevel Optimal
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw 'Portable archive was not generated.' }

# Inspect the *actual ZIP*, not merely the staging directory.
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $entries = @($zip.Entries | Where-Object { $_.FullName -notlike '*/' } | ForEach-Object { $_.FullName.Replace('\', '/') })
    foreach ($name in $executables) {
        if ($entries -cnotcontains $name) { throw "Portable ZIP is missing $name" }
    }
    if ($entries -cnotcontains 'README.md') { throw 'Portable ZIP is missing README.md' }
} finally { $zip.Dispose() }

$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
"$hash  $archiveName" | Set-Content -LiteralPath $manifest -Encoding Ascii
$previous = [IO.File]::ReadAllText($manifest)
if ($previous.Trim() -cne "$hash  $archiveName") {
    throw 'Failed to verify the checksum manifest.'
}
Write-Host "Portable ZIP verified: $archive"

Push-Location $root
try {
    & $nsis (Join-Path $root 'installer\PawHide.nsi')
    if ($LASTEXITCODE -ne 0) { throw "NSIS compilation failed with exit code $LASTEXITCODE" }
} finally { Pop-Location }
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) {
    throw 'NSIS did not produce dist\PawHide-setup.exe'
}
$bytes = [IO.File]::ReadAllBytes($setup)
if ($bytes.Length -lt 2 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    throw 'NSIS setup does not contain a valid Windows executable header.'
}
Write-Host "Online installer verified: $setup"
Write-Host "SHA-256 manifest: $manifest"
