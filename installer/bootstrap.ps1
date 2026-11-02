# PawHide online-only release bootstrap. Shipped INSIDE the NSIS program,
# but application binaries are always downloaded from a GitHub Release.
# Requires Windows PowerShell 5.1+, no elevated privileges, no embedded token.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Destination,
    [Parameter(Mandatory)][string]$Repository
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$archiveName = 'PawHide-Windows-x64.zip'
$hashName = "$archiveName.sha256"
$work = Join-Path ([IO.Path]::GetTempPath()) ('PawHideInstall-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
    $archive = Join-Path $work $archiveName
    $checksum = Join-Path $work $hashName
    $useGh = $false
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        & gh auth status *> $null
        $useGh = ($LASTEXITCODE -eq 0)
    }
    if ($useGh) {
        # Enables installing from a private repository for an authenticated
        # developer without putting credentials into the installer itself.
        & gh release download --repo $Repository --pattern $archiveName --pattern $hashName --dir $work --clobber
        if ($LASTEXITCODE -ne 0) { throw 'Could not download assets from GitHub via authenticated gh CLI.' }
    } else {
        # Anonymous downloads work only when the release assets are PUBLIC.
        $base = "https://github.com/$Repository/releases/latest/download"
        try {
            Invoke-WebRequest -Uri "$base/$archiveName" -OutFile $archive -UseBasicParsing -MaximumRedirection 10
            Invoke-WebRequest -Uri "$base/$hashName" -OutFile $checksum -UseBasicParsing -MaximumRedirection 10
        } catch {
            throw "Release download failed. The GitHub repository may be private or have no published release. Authenticated developers can sign into GitHub CLI (gh auth login). $($_.Exception.Message)"
        }
    }
    $hashText = [IO.File]::ReadAllText($checksum).Trim()
    if ($hashText -notmatch '\b[0-9a-fA-F]{64}\b') { throw 'Invalid SHA-256 manifest.' }
    $expected = $Matches[0].ToUpperInvariant()
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToUpperInvariant()
    if ($actual -ne $expected) { throw 'Release ZIP failed SHA-256 verification; nothing installed.' }
    $unpack = Join-Path $work 'unpacked'
    Expand-Archive -LiteralPath $archive -DestinationPath $unpack -Force
    foreach ($name in 'pawhide.exe','PawHideUI.exe','pawhide_agent.dll') {
        if (-not (Test-Path -LiteralPath (Join-Path $unpack $name) -PathType Leaf)) {
            throw "Release is missing required component: $name"
        }
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    foreach ($name in 'pawhide.exe','PawHideUI.exe','pawhide_agent.dll') {
        Copy-Item -LiteralPath (Join-Path $unpack $name) -Destination (Join-Path $Destination $name) -Force
    }
    Write-Output "PawHide installed in $Destination"
} catch {
    Write-Error $_.Exception.Message
    exit 1
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
