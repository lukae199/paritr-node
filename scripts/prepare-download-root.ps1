<# Validate GitHub release assets and create an upload-ready download tree. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string] $Version,
    [Parameter(Mandatory=$true)][string] $ArtifactDirectory,
    [Parameter(Mandatory=$true)][string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
$Version = $Version.TrimStart('v')
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$') { throw 'Invalid version.' }
if (-not (Test-Path -LiteralPath $ArtifactDirectory -PathType Container)) { throw "Artifact directory not found: $ArtifactDirectory" }
if (Test-Path -LiteralPath $OutputDirectory) { throw "Output already exists: $OutputDirectory" }
$ArtifactDirectory = (Resolve-Path -LiteralPath $ArtifactDirectory).Path

$required = @('bootstrap.sh','bootstrap.ps1','setup.sh','setup.ps1','install.sh','install.ps1','manage.sh','manage.ps1','docker-compose.release.yml','release.env','SHA256SUMS')
foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $ArtifactDirectory $name) -PathType Leaf)) { throw "Missing release asset: $name" }
}

foreach ($line in Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'SHA256SUMS')) {
    if ($line -notmatch '^([0-9a-f]{64})\s+\*?(.+)$') { throw "Invalid SHA256SUMS line: $line" }
    $path = Join-Path $ArtifactDirectory $Matches[2]
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Checksummed file missing: $($Matches[2])" }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $Matches[1]) { throw "Checksum mismatch: $($Matches[2])" }
}

$versionDirectory = Join-Path $OutputDirectory "v$Version"
New-Item -ItemType Directory -Path $versionDirectory | Out-Null
Copy-Item -Path (Join-Path $ArtifactDirectory '*') -Destination $versionDirectory -Recurse
foreach ($name in @('bootstrap.sh','bootstrap.ps1','setup.sh','setup.ps1')) {
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory $name) -Destination (Join-Path $OutputDirectory $name)
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory "$name.sha256") -Destination (Join-Path $OutputDirectory "$name.sha256")
}
$Version | Set-Content -LiteralPath (Join-Path $OutputDirectory 'STABLE') -Encoding ASCII
Write-Host "Prepared: $OutputDirectory"
