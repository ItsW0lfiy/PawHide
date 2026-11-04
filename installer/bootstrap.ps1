# Public online release bootstrap. No credentials or embedded app binaries.
# Windows PowerShell 5.1+, per-user installation, SHA-256 checked before writes.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Destination,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repository,
    [string]$ErrorFile
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$names = @('pawhide.exe', 'PawHideUI.exe', 'pawhide_agent.dll')
$optionalEntries = @('README.md', 'docs/', 'docs/CAPTURE_BOUNDARY.md', 'docs/RELEASE_READINESS.md', 'ui/', 'ui/assets/', 'ui/assets/pawhide.png')
$archiveName = 'PawHide-Windows-x64.zip'
$preserveWork = $false
$work = Join-Path ([IO.Path]::GetTempPath()) ('PawHideInstall-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $archive = Join-Path $work $archiveName
    # Pin both assets to one release, avoiding a latest-release promotion race.
    try {
        $response = Invoke-WebRequest -Uri "https://api.github.com/repos/$Repository/releases/latest" -UseBasicParsing -TimeoutSec 90
        $release = $response.Content | ConvertFrom-Json
        foreach ($name in @($archiveName, "$archiveName.sha256")) {
            $asset = @($release.assets | Where-Object { $_.name -ceq $name })
            if ($asset.Count -ne 1) { throw "Release is missing asset: $name" }
            $uri = [uri]$asset[0].browser_download_url
            if ($uri.Scheme -ne 'https' -or $uri.Host -ne 'github.com' -or -not $uri.AbsolutePath.StartsWith("/$Repository/releases/download/", [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Release asset URL is outside the expected GitHub repository.'
            }
            Invoke-WebRequest -Uri $uri.AbsoluteUri -OutFile (Join-Path $work $name) -UseBasicParsing -TimeoutSec 90 -MaximumRedirection 10
        }
    } catch {
        throw "Cannot download a public PawHide release. This repository is private, has no accessible release, or the connection failed. No GitHub login is required for public releases. $($_.Exception.Message)"
    }
    $hashText = [IO.File]::ReadAllText((Join-Path $work "$archiveName.sha256")).Trim()
    if ($hashText -notmatch '^(?<Hash>[0-9a-fA-F]{64})[ \t]+\*?PawHide-Windows-x64\.zip$') { throw 'Invalid SHA-256 manifest or wrong archive filename.' }
    $expected = $Matches.Hash
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ine $expected) { throw 'Release ZIP failed SHA-256 verification; nothing installed.' }
    $unpack = Join-Path $work 'unpacked'
    Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $seen = @{}
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -cnotin ($names + $optionalEntries)) { throw "Unexpected portable ZIP entry: $($entry.FullName)" }
            if ($seen.ContainsKey($entry.FullName.ToLowerInvariant())) { throw 'Duplicate ZIP entry.' }
            $seen[$entry.FullName.ToLowerInvariant()] = $true
            if ($entry.Length -gt 150MB -or (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Invalid portable ZIP entry.' }
        }
        foreach ($name in $names) { if (-not $seen.ContainsKey($name.ToLowerInvariant())) { throw "Missing component: $name" } }
    } finally { $zip.Dispose() }
    Expand-Archive -LiteralPath $archive -DestinationPath $unpack
    & (Join-Path $PSScriptRoot 'maintenance.ps1') -Destination $Destination -ErrorFile $ErrorFile
    if ($LASTEXITCODE -ne 0) {
        if ($ErrorFile -and (Test-Path -LiteralPath $ErrorFile)) { throw [IO.File]::ReadAllText($ErrorFile) }
        throw 'Upgrade preflight failed. Close PawHide and protected applications, then retry.'
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $backup = Join-Path $work 'previous'
    New-Item -ItemType Directory -Path $backup | Out-Null
    $replaced = @()
    try {
        foreach ($name in $names) {
            $target = Join-Path $Destination $name
            if (Test-Path -LiteralPath $target -PathType Leaf) { Copy-Item -LiteralPath $target -Destination (Join-Path $backup $name) }
            $replaced += $name
            Copy-Item -LiteralPath (Join-Path $unpack $name) -Destination $target -Force
        }
    } catch {
        $failure = $_
        $rollbackErrors = @()
        foreach ($name in $replaced) {
            $target = Join-Path $Destination $name
            $previous = Join-Path $backup $name
            try {
                if (Test-Path -LiteralPath $previous) { Copy-Item -LiteralPath $previous -Destination $target -Force }
                elseif (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
            } catch { $rollbackErrors += "$name : $($_.Exception.Message)" }
        }
        if ($rollbackErrors.Count -gt 0) {
            $preserveWork = $true
            throw "Installation and rollback failed. Previous files are retained at $backup. $($rollbackErrors -join '; ')"
        }
        throw "Installation failed; previous files restored. $($failure.Exception.Message)"
    }
    Write-Output "PawHide installed in $Destination"
} catch {
    if ($ErrorFile) { [IO.File]::WriteAllText($ErrorFile, $_.Exception.Message) }
    Write-Error $_.Exception.Message -ErrorAction Continue
    exit 1
} finally {
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    $resolvedWork = [IO.Path]::GetFullPath($work)
    if (-not $preserveWork -and $resolvedWork.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($resolvedWork) -match '^PawHideInstall-[0-9a-f]{32}$') {
        Remove-Item -LiteralPath $resolvedWork -Recurse -Force -ErrorAction SilentlyContinue
    }
}
