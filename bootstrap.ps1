<# Stable one-command bootstrap. The downloaded setup script performs all prompts. #>
[CmdletBinding()]
param(
    [ValidateSet('auto','docker','native')][string] $Deployment = 'auto',
    [string] $Image = $env:PARITR_IMAGE_REF,
    [string] $Address = '',
    [ValidateRange(1,65535)][int] $Port = 5050,
    [string] $PublicUrl = '',
    [string] $PortalUrl = $env:PARITR_PORTAL_URL,
    [Alias('Pair')][string] $PairCode = '',
    [string] $Dir = "$env:LOCALAPPDATA\Paritr\node-mainnet",
    [string] $Source = 'https://paritr.highactive.de/downloads',
    [ValidateRange(0,1024)][int] $Cores = 0,
    [ValidateRange(5,100)][int] $Intensity = 100,
    [switch] $Fast,
    [switch] $Light,
    [switch] $OpenFirewall,
    [switch] $NoAutostart,
    [Alias('AssumeYes')][switch] $Yes
)

$ErrorActionPreference = 'Stop'
$sourceBase = if ($PSBoundParameters.ContainsKey('Source')) { $Source.TrimEnd('/') } elseif ($env:PARITR_SOURCE) { $env:PARITR_SOURCE.TrimEnd('/') } else { 'https://paritr.highactive.de/downloads' }
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("paritr-bootstrap-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $setup = Join-Path $temporary 'setup.ps1'
    $checksum = "$setup.sha256"
    Invoke-WebRequest -Uri "$sourceBase/setup.ps1" -OutFile $setup -UseBasicParsing
    Invoke-WebRequest -Uri "$sourceBase/setup.ps1.sha256" -OutFile $checksum -UseBasicParsing
    $expected = ([regex]::Match((Get-Content -LiteralPath $checksum -Raw), '(?i)\b[0-9a-f]{64}\b')).Value.ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
    if (-not $expected -or $actual -ne $expected) { throw 'Setup checksum mismatch.' }
    Unblock-File -LiteralPath $setup
    $forward = @{}
    foreach ($key in $PSBoundParameters.Keys) { $forward[$key] = $PSBoundParameters[$key] }
    & $setup @forward
} finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
}
