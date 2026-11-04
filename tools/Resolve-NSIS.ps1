#Requires -Version 5.1
# Portable build dependency only: no installer, registry, PATH or elevation changes.
function Get-PawHideNsisArchive {
    param([string]$Destination, [string]$Version, [string]$ExpectedHash)
    $source = "https://downloads.sourceforge.net/project/nsis/NSIS%203/$Version/nsis-$Version.zip"
    try {
        Write-Host "Downloading NSIS $Version from SourceForge..."
        Invoke-WebRequest -Uri $source -OutFile $Destination -UseBasicParsing -TimeoutSec 60
        if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ine $ExpectedHash) {
            throw 'The download did not match the pinned NSIS SHA-256 (it may be an HTML download page).'
        }
    } catch {
        Write-Host "SourceForge download unavailable: $($_.Exception.Message)"
        Write-Host 'Trying the Chocolatey portable mirror; the inner ZIP must match the SAME pinned NSIS SHA-256.'
        $mirror = "$Destination.nupkg"
        try {
            Invoke-WebRequest -Uri "https://community.chocolatey.org/api/v2/package/nsis.portable/$Version.0" -OutFile $mirror -UseBasicParsing -TimeoutSec 60
            $package = [IO.Compression.ZipFile]::OpenRead($mirror)
            try {
                $entries = @($package.Entries | Where-Object { $_.FullName -ceq "tools/nsis-$Version.zip" })
                if ($entries.Count -ne 1 -or $entries[0].Length -gt 20MB) { throw 'Portable mirror contains no unique, valid NSIS archive.' }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $Destination, $true)
            } finally { $package.Dispose() }
            if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ine $ExpectedHash) {
                throw 'Mirrored NSIS archive failed pinned SHA-256 verification.'
            }
        } catch {
            throw "Cannot acquire verified NSIS $Version. Check network access to SourceForge or community.chocolatey.org and rerun cargo all. $($_.Exception.Message)"
        } finally {
            if (Test-Path -LiteralPath $mirror) { Remove-Item -LiteralPath $mirror -Force }
        }
    }
}

function Test-PawHideNsisTree {
    param([IO.Compression.ZipArchive]$Archive, [string]$Directory, [string]$Version)
    foreach ($entry in $Archive.Entries) {
        if ($entry.FullName.EndsWith('/')) { continue }
        $relative = $entry.FullName.Substring("nsis-$Version/".Length)
        $path = Join-Path $Directory $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
        if ((Get-Item -LiteralPath $path).Length -ne $entry.Length) { return $false }
        $stream = $entry.Open()
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $expected = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
        finally { $sha.Dispose(); $stream.Dispose() }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -cne $expected) { return $false }
    }
    return $true
}

function Get-PawHideCachedNsis {
    param([string]$CacheRoot, [string]$Version, [string]$ExpectedHash)
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $CacheRoot = [IO.Path]::GetFullPath($CacheRoot)
    New-Item -ItemType Directory -Path $CacheRoot -Force | Out-Null
    $work = Join-Path $CacheRoot ('.nsis-bootstrap-' + [guid]::NewGuid().ToString('N'))
    $archivePath = Join-Path $CacheRoot "nsis-$Version.zip"
    $toolRoot = Join-Path $CacheRoot "nsis-$Version"
    $lock = $null
    $timer = [Diagnostics.Stopwatch]::StartNew()
    try {
        while (-not $lock) {
            try { $lock = [IO.File]::Open((Join-Path $CacheRoot '.bootstrap.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
            catch [IO.IOException] {
                if ($timer.Elapsed.TotalSeconds -ge 20) { throw 'Another build is provisioning NSIS. Wait for it to finish and rerun cargo all.' }
                Start-Sleep -Milliseconds 200
            }
        }
        New-Item -ItemType Directory -Path $work | Out-Null
        if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf) -or (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ine $ExpectedHash) {
            $download = Join-Path $work 'download.zip'
            Get-PawHideNsisArchive -Destination $download -Version $Version -ExpectedHash $ExpectedHash
            Move-Item -LiteralPath $download -Destination $archivePath -Force
        }
        $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            $seen = @{}
            foreach ($entry in $archive.Entries) {
                $name = $entry.FullName
                if (-not $name.StartsWith("nsis-$Version/", [StringComparison]::Ordinal) -or $name.Contains('\') -or $name.Contains(':') -or $name.Split('/') -contains '..' -or $entry.Length -gt 30MB) {
                    throw "Invalid NSIS archive entry: $name"
                }
                if ($seen.ContainsKey($name)) { throw 'Duplicate NSIS archive entry.' }
                $seen[$name] = $true
            }
            foreach ($required in @('makensis.exe', 'Bin/makensis.exe', 'Include/MUI2.nsh', 'nsisconf.nsh')) {
                if (-not $seen.ContainsKey("nsis-$Version/$required")) { throw "Incomplete NSIS archive: $required" }
            }
            if (-not (Test-PawHideNsisTree -Archive $archive -Directory $toolRoot -Version $Version)) {
                [IO.Compression.ZipFile]::ExtractToDirectory($archivePath, $work)
                # Only this versioned, generated cache directory can be replaced.
                if (Test-Path -LiteralPath $toolRoot) {
                    if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($toolRoot)) -ne $CacheRoot -or (Get-Item -LiteralPath $toolRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                        throw 'Refusing to replace an unsafe NSIS cache directory.'
                    }
                    Remove-Item -LiteralPath $toolRoot -Recurse -Force
                }
                Move-Item -LiteralPath (Join-Path $work "nsis-$Version") -Destination $toolRoot
            }
        } finally { $archive.Dispose() }
        Write-Host "Using verified portable NSIS ${Version}: $toolRoot"
        return (Join-Path $toolRoot 'makensis.exe')
    } finally {
        if (Test-Path -LiteralPath $work) {
            $resolved = [IO.Path]::GetFullPath($work)
            if ([IO.Path]::GetDirectoryName($resolved) -eq $CacheRoot -and [IO.Path]::GetFileName($resolved) -match '^\.nsis-bootstrap-[0-9a-f]{32}$') {
                Remove-Item -LiteralPath $resolved -Recurse -Force
            }
        }
        if ($lock) { $lock.Dispose() }
    }
}

function Get-PawHideNsis {
    param([string]$RepositoryRoot)
    if ($env:PAWHIDE_MAKENSIS) {
        if (-not (Test-Path -LiteralPath $env:PAWHIDE_MAKENSIS -PathType Leaf)) { throw "PAWHIDE_MAKENSIS points to a missing file: $env:PAWHIDE_MAKENSIS" }
        return $env:PAWHIDE_MAKENSIS
    }
    $command = Get-Command makensis.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { return $command.Source }
    foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, $env:ChocolateyInstall)) {
        if (-not $base) { continue }
        foreach ($relative in @('NSIS\makensis.exe', 'bin\makensis.exe')) {
            $candidate = Join-Path $base $relative
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        }
    }
    # Published checksum: https://sourceforge.net/projects/nsis/files/NSIS%203/3.13/nsis-3.13.zip/download
    return Get-PawHideCachedNsis -CacheRoot (Join-Path $RepositoryRoot 'target\tools\nsis') -Version '3.13' -ExpectedHash 'BA63DFFC4410EE89193E1CB5A41989991BD77C61068DA17E3156D136B7B0B3D8'
}
