<#
.SYNOPSIS
  Unified Paritr Protocol 9 setup for Windows 10/11 and Windows Server.
#>
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
$NodeVersion = '4.0.1-rc.3'
$ProtocolVersion = 9
$PortWasSpecified = $PSBoundParameters.ContainsKey('Port')
$Source = $Source.TrimEnd('/')
$SetupPath = $MyInvocation.MyCommand.Path
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Temporary = Join-Path ([IO.Path]::GetTempPath()) ("paritr-setup-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Temporary | Out-Null

function Invoke-Checked([string] $Program, [string[]] $Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Read-YesNo([string] $Prompt, [bool] $Default = $false) {
    if ($Yes) { return $Default }
    $suffix = if ($Default) { '[Y/n]' } else { '[y/N]' }
    $answer = (Read-Host "$Prompt $suffix").Trim()
    if (-not $answer) { return $Default }
    return $answer -match '^(?i:y|yes|j|ja)$'
}

function Get-VerifiedAsset([string] $Name, [string] $Destination) {
    $local = Join-Path $ScriptDir $Name
    if (Test-Path -LiteralPath $local) {
        Copy-Item -LiteralPath $local -Destination $Destination -Force
        return
    }
    $base = "$Source/v$NodeVersion/$Name"
    Invoke-WebRequest -Uri $base -OutFile $Destination -UseBasicParsing
    $checksum = "$Destination.sha256"
    Invoke-WebRequest -Uri "$base.sha256" -OutFile $checksum -UseBasicParsing
    $expected = ([regex]::Match((Get-Content -LiteralPath $checksum -Raw), '(?i)\b[0-9a-f]{64}\b')).Value.ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if (-not $expected -or $actual -ne $expected) { throw "Checksum mismatch for $Name" }
    Unblock-File -LiteralPath $Destination
}

function Invoke-Docker([string[]] $Arguments, [switch] $AllowFailure) {
    & docker.exe @Arguments
    $code = $LASTEXITCODE
    if (-not $AllowFailure -and $code -ne 0) { throw "docker $($Arguments -join ' ') failed with exit code $code" }
    return $code
}

function Wait-Docker {
    for ($attempt = 0; $attempt -lt 90; $attempt++) {
        & docker.exe info *> $null
        if ($LASTEXITCODE -eq 0) { return }
        Start-Sleep -Seconds 2
    }
    throw 'Docker Desktop did not become ready. Start Docker Desktop and rerun setup.ps1.'
}

function Install-DockerDesktop {
    if (-not (Get-Command docker.exe -ErrorAction SilentlyContinue)) {
        if (-not (Get-Command winget.exe -ErrorAction SilentlyContinue)) {
            throw 'winget is required for automatic Docker Desktop installation. Install Docker Desktop manually or use -Deployment native.'
        }
        & wsl.exe --status *> $null
        if ($LASTEXITCODE -ne 0) {
            if (-not (Test-Administrator)) {
                throw 'WSL2 is not enabled. Run an elevated PowerShell: wsl --install --no-distribution, restart Windows, then rerun setup.ps1.'
            }
            Invoke-Checked 'wsl.exe' @('--install','--no-distribution')
            throw 'WSL2 was enabled. Restart Windows and rerun setup.ps1.'
        }
        Invoke-Checked 'winget.exe' @('install','--exact','--id','Docker.DockerDesktop','--accept-package-agreements','--accept-source-agreements')
        $machinePath = [Environment]::GetEnvironmentVariable('Path','Machine')
        $userPath = [Environment]::GetEnvironmentVariable('Path','User')
        $env:Path = "$machinePath;$userPath"
    }
    if (-not (Get-Command docker.exe -ErrorAction SilentlyContinue)) {
        throw 'Docker CLI is not available after installation. Restart Windows and rerun setup.ps1.'
    }
    & docker.exe info *> $null
    if ($LASTEXITCODE -ne 0) {
        $desktop = @(
            "$env:LOCALAPPDATA\Programs\Docker\Docker\Docker Desktop.exe",
            "$env:ProgramFiles\Docker\Docker\Docker Desktop.exe"
        ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        if (-not $desktop) { throw 'Docker Desktop is installed but could not be located.' }
        Start-Process -FilePath $desktop
        Wait-Docker
    }
    Invoke-Docker @('compose','version') | Out-Null
}

function Install-Native {
    $installer = Join-Path $Temporary 'install.ps1'
    Get-VerifiedAsset 'install.ps1' $installer
    $parameters = @{
        Address = $Address
        Port = $Port
        PublicUrl = $PublicUrl
        PortalUrl = $PortalUrl
        PairCode = $PairCode
        Dir = $Dir
        Source = $Source
        Cores = $Cores
        Intensity = $Intensity
        AssumeYes = $true
        Fast = [bool]$Fast
        Light = [bool]$Light
        OpenFirewall = [bool]$OpenFirewall
        NoAutostart = [bool]$NoAutostart
    }
    & $installer @parameters
    if (-not $?) { throw 'Native installation failed.' }
    Copy-Item -LiteralPath $SetupPath -Destination (Join-Path $Dir 'setup.ps1') -Force
    @("mode=native", "version=$NodeVersion", "source=$Source") |
        Set-Content -LiteralPath (Join-Path $Dir '.paritr-deployment') -Encoding ASCII
}

function Resolve-ImageReference {
    if ($Image) { return $Image }
    $metadata = Join-Path $Temporary 'release.env'
    Get-VerifiedAsset 'release.env' $metadata
    $line = Get-Content -LiteralPath $metadata | Where-Object { $_ -match '^PARITR_IMAGE_REF=' } | Select-Object -First 1
    if (-not $line) { throw 'release.env does not contain PARITR_IMAGE_REF.' }
    $reference = $line.Substring('PARITR_IMAGE_REF='.Length)
    if ($reference -notmatch '^[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}$') {
        throw 'PARITR_IMAGE_REF must be pinned to a sha256 digest.'
    }
    return $reference
}

function Install-Container {
    $productType = (Get-CimInstance Win32_OperatingSystem).ProductType
    if ($productType -ne 1) {
        throw 'Docker Desktop is not supported on Windows Server. Use -Deployment native.'
    }
    Install-DockerDesktop
    $resolvedImage = Resolve-ImageReference
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $script:Dir = (Resolve-Path -LiteralPath $Dir).Path
    $composeFile = Join-Path $Dir 'docker-compose.yml'
    Get-VerifiedAsset 'docker-compose.release.yml' $composeFile
    Get-VerifiedAsset 'manage.ps1' (Join-Path $Dir 'manage.ps1')
    Copy-Item -LiteralPath $SetupPath -Destination (Join-Path $Dir 'setup.ps1') -Force
    $existingEnvironment = Join-Path $Dir '.env'
    if (-not $PortWasSpecified -and (Test-Path -LiteralPath $existingEnvironment)) {
        $portLine = Get-Content -LiteralPath $existingEnvironment | Where-Object { $_ -match '^PARITR_PUBLIC_PORT=\d+$' } | Select-Object -First 1
        if ($portLine) { $script:Port = [int]$portLine.Substring('PARITR_PUBLIC_PORT='.Length) }
    }
    if ($Port -lt 1 -or $Port -gt 65535) { throw 'Invalid stored public port.' }
    $listenHost = '127.0.0.1'
    if (Test-Path -LiteralPath $existingEnvironment) {
        $listenLine = Get-Content -LiteralPath $existingEnvironment | Where-Object { $_ -match '^PARITR_LISTEN_HOST=(127\.0\.0\.1|0\.0\.0\.0)$' } | Select-Object -First 1
        if ($listenLine) { $listenHost = $listenLine.Substring('PARITR_LISTEN_HOST='.Length) }
    }
    if ($PublicUrl -or $OpenFirewall) { $listenHost = '0.0.0.0' }
    @("PARITR_IMAGE_REF=$resolvedImage", "PARITR_LISTEN_HOST=$listenHost", "PARITR_PUBLIC_PORT=$Port", "PARITR_MANAGEMENT_HOST=0.0.0.0", "PARITR_MANAGEMENT_PORT=5051") |
        Set-Content -LiteralPath (Join-Path $Dir '.env') -Encoding ASCII
    @("mode=docker", "version=$NodeVersion", "source=$Source") |
        Set-Content -LiteralPath (Join-Path $Dir '.paritr-deployment') -Encoding ASCII

    $compose = @('compose','--project-directory',$Dir,'--env-file',(Join-Path $Dir '.env'),'-f',$composeFile)
    Invoke-Docker ($compose + @('pull','paritr-node')) | Out-Null
    & docker.exe @($compose + @('run','--rm','--no-deps','--entrypoint','/bin/sh','paritr-node','-c','test -f /var/lib/paritr/config.json')) *> $null
    $hasConfig = $LASTEXITCODE -eq 0
    if (-not $hasConfig) {
        $mode = if ($Fast -or $Address) { 'fast' } else { 'light' }
        $arguments = $compose + @('run','--rm','--no-deps','paritr-node','init','--public-bind','0.0.0.0:5050','--admin-bind','127.0.0.1:5051','--management-bind','0.0.0.0:5051','--mining-threads',[string]$Cores,'--mining-intensity',[string]$Intensity,'--randomx-mode',$mode)
        if ($Address) { $arguments += @('--miner-address',$Address,'--enable-mining') }
        if ($PublicUrl) { $arguments += @('--public-url',$PublicUrl) }
        Invoke-Docker $arguments | Out-Null
    } else {
        Write-Host 'Existing Protocol-9 container configuration retained.'
    }
    if ($PairCode) {
        Invoke-Docker ($compose + @('stop','paritr-node')) -AllowFailure | Out-Null
        try { Invoke-Docker ($compose + @('run','--rm','--no-deps','paritr-node','pair','--portal-url',$PortalUrl,'--code',$PairCode)) | Out-Null }
        catch { Write-Warning "Pairing failed. Retry later with manage.ps1: $($_.Exception.Message)" }
    }
    Invoke-Docker ($compose + @('run','--rm','--no-deps','paritr-node','check')) | Out-Null
    if (-not $NoAutostart) { Invoke-Docker ($compose + @('up','-d','paritr-node')) | Out-Null }
    $lanIp = Get-NetIPConfiguration | Where-Object { ($_.IPv4DefaultGateway -or $_.NetAdapter.HardwareInterface) -and $_.IPv4Address } | Select-Object -First 1 -ExpandProperty IPv4Address | Select-Object -First 1 -ExpandProperty IPAddress
    Invoke-Docker ($compose + @('run','--rm','--no-deps','-e',"PARITR_MANAGEMENT_HOST_IP=$lanIp",'paritr-node','admin-access'))

    if ($OpenFirewall) {
        if (-not (Test-Administrator)) {
            Write-Warning 'Run setup.ps1 as Administrator to install the firewall rule.'
        } else {
            Get-NetFirewallRule -DisplayName 'Paritr Protocol 9' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
            New-NetFirewallRule -DisplayName 'Paritr Protocol 9' -Direction Inbound -Protocol TCP -LocalPort $Port -Action Allow -Profile Any | Out-Null
        }
    }
    if (Test-Administrator) {
        Get-NetFirewallRule -DisplayName 'Paritr local management' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        Get-NetFirewallRule -DisplayName 'Paritr mDNS' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        New-NetFirewallRule -DisplayName 'Paritr local management' -Direction Inbound -Protocol TCP -LocalPort 5051 -RemoteAddress LocalSubnet -Profile Private -Action Allow | Out-Null
        New-NetFirewallRule -DisplayName 'Paritr mDNS' -Direction Inbound -Protocol UDP -LocalPort 5353 -RemoteAddress LocalSubnet -Profile Private -Action Allow | Out-Null
    }
}

try {
    if ($Fast -and $Light) { throw 'Fast and Light are mutually exclusive.' }
    if ($PublicUrl -and $PublicUrl -notmatch '^https://') { throw 'PublicUrl must use HTTPS.' }
    if ($PortalUrl -and $PortalUrl -notmatch '^https://') { throw 'PortalUrl must use HTTPS.' }

    $productType = (Get-CimInstance Win32_OperatingSystem).ProductType
    if ($Deployment -eq 'auto') { $Deployment = if ($productType -eq 1) { 'native' } else { 'native' } }

    if (-not $Yes) {
        Write-Host "Paritr Protocol $ProtocolVersion setup"
        Write-Host 'This wizard never asks for a wallet private key or seed phrase.'
        $chosen = Read-Host "Installation type [$Deployment] (native/docker)"
        if ($chosen) { $Deployment = $chosen.ToLowerInvariant() }
        Write-Host 'Mining, wallet pairing and the device name are configured in the local browser after installation.'
    }

    if ($Deployment -notin @('docker','native')) { throw "Invalid deployment: $Deployment" }
    if ([bool]$PairCode -ne [bool]$PortalUrl) { throw 'PairCode and PortalUrl must be supplied together.' }

    Write-Host "Installing Paritr $NodeVersion / Protocol $ProtocolVersion ($Deployment)"
    if ($Deployment -eq 'docker') { Install-Container } else { Install-Native }
    Write-Host "Installation complete: $Dir"
    Write-Host "Management: cd `"$Dir`"; .\manage.ps1 status"
    Write-Host "Local API: http://127.0.0.1:$Port"
    Write-Host 'Private keys and wallet seed phrases are never requested by this installer.'
} finally {
    if (Test-Path -LiteralPath $Temporary) { Remove-Item -LiteralPath $Temporary -Recurse -Force }
}
