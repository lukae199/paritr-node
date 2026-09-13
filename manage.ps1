<# Unified Paritr Protocol 9 management for native and Docker installations. #>
[CmdletBinding()]
param(
    [Parameter(Position=0)][string] $Command = 'help',
    [Parameter(Position=1)][string] $Value = '',
    [Parameter(Position=2)][string] $PortalUrl = ''
)

$ErrorActionPreference = 'Stop'
$NodeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$TaskName = if ($env:PARITR_TASK_NAME) { $env:PARITR_TASK_NAME } else { 'ParitrNodeMainnet' }
$Binary = Join-Path $NodeDir 'paritr-node.exe'
$Config = Join-Path $NodeDir 'config.json'
$Log = Join-Path $NodeDir 'paritr.log'
$State = Join-Path $NodeDir '.paritr-deployment'
$Deployment = 'native'
$Source = if ($env:PARITR_SOURCE) { $env:PARITR_SOURCE } else { 'https://paritr.highactive.de/downloads' }
if (Test-Path -LiteralPath $State) {
    $settings = ConvertFrom-StringData ((Get-Content -LiteralPath $State) -join "`n")
    if ($settings.mode -in @('native','docker')) { $Deployment = $settings.mode }
    if ($settings.source -match '^https://') { $Source = $settings.source.TrimEnd('/') }
}
$ComposePrefix = @('compose','--project-directory',$NodeDir,'--env-file',(Join-Path $NodeDir '.env'),'-f',(Join-Path $NodeDir 'docker-compose.yml'))

function Invoke-Docker([string[]] $Arguments, [switch] $AllowFailure) {
    & docker.exe @Arguments
    $code = $LASTEXITCODE
    if (-not $AllowFailure -and $code -ne 0) { throw "docker $($Arguments -join ' ') failed with exit code $code" }
}

function Invoke-Compose([string[]] $Arguments, [switch] $AllowFailure) {
    Invoke-Docker ($ComposePrefix + $Arguments) -AllowFailure:$AllowFailure
}

function Assert-Installed {
    if ($Deployment -eq 'docker') {
        if (-not (Test-Path -LiteralPath (Join-Path $NodeDir 'docker-compose.yml')) -or
            -not (Test-Path -LiteralPath (Join-Path $NodeDir '.env'))) {
            throw "Incomplete Docker installation in $NodeDir"
        }
    } elseif (-not (Test-Path -LiteralPath $Binary) -or -not (Test-Path -LiteralPath $Config)) {
        throw "Paritr Protocol 9 is not installed in $NodeDir"
    }
}

function Get-NodeProcess {
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.Equals($Binary, [StringComparison]::OrdinalIgnoreCase) }
}

function Test-NodeRunning {
    if ($Deployment -eq 'docker') {
        $ids = & docker.exe @($ComposePrefix + @('ps','--status','running','--quiet','paritr-node')) 2>$null
        return $LASTEXITCODE -eq 0 -and [bool]$ids
    }
    return [bool](Get-NodeProcess | Select-Object -First 1)
}

function Stop-Node {
    if ($Deployment -eq 'docker') { Invoke-Compose @('stop','paritr-node'); return }
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Get-NodeProcess | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Start-Node {
    Assert-Installed
    if ($Deployment -eq 'docker') { Invoke-Compose @('up','-d','paritr-node'); return }
    $task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($task) { Start-ScheduledTask -TaskName $TaskName; return }
    Start-Process -FilePath $Binary -ArgumentList @('--config', $Config, 'run') -WorkingDirectory $NodeDir -WindowStyle Hidden
}

function Invoke-Node([string[]] $Arguments) {
    if ($Deployment -eq 'docker') { Invoke-Compose (@('run','--rm','--no-deps','paritr-node') + $Arguments) }
    else {
        & $Binary --config $Config @Arguments
        if ($LASTEXITCODE -ne 0) { throw "paritr-node failed with exit code $LASTEXITCODE" }
    }
}

function Get-PublicPort {
    if ($Deployment -eq 'docker') {
        $line = Get-Content -LiteralPath (Join-Path $NodeDir '.env') | Where-Object { $_ -match '^PARITR_PUBLIC_PORT=\d+$' } | Select-Object -First 1
        if ($line) { return [int]$line.Substring('PARITR_PUBLIC_PORT='.Length) }
        return 5050
    }
    return [int](([string](Get-Content -LiteralPath $Config -Raw | ConvertFrom-Json).public_bind).Split(':')[-1].TrimEnd(']'))
}

function Show-StatusApi {
    try {
        Invoke-RestMethod -Uri "http://127.0.0.1:$(Get-PublicPort)/status" -TimeoutSec 5 | ConvertTo-Json -Depth 8
    } catch { Write-Host "Status API unavailable: $($_.Exception.Message)" }
}

function Update-Installation {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ("paritr-update-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temporary | Out-Null
    try {
        $installer = Join-Path $temporary 'setup.ps1'
        $checksum = "$installer.sha256"
        Invoke-WebRequest -Uri "$Source/setup.ps1" -OutFile $installer -UseBasicParsing
        Invoke-WebRequest -Uri "$Source/setup.ps1.sha256" -OutFile $checksum -UseBasicParsing
        $expected = ([regex]::Match((Get-Content -LiteralPath $checksum -Raw), '(?i)\b[0-9a-f]{64}\b')).Value.ToLowerInvariant()
        $actual = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not $expected -or $actual -ne $expected) { throw 'Setup checksum mismatch.' }
        Unblock-File -LiteralPath $installer
        & $installer -Deployment $Deployment -Dir $NodeDir -Source $Source -Port (Get-PublicPort) -Yes
        if (-not $?) { throw 'Update failed.' }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
    }
}

function Backup-Installation {
    $backupDir = if ($env:PARITR_BACKUP_DIR) { $env:PARITR_BACKUP_DIR } else { Join-Path $NodeDir 'backups' }
    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $wasRunning = Test-NodeRunning
    if ($wasRunning) { Stop-Node }
    try {
        if ($Deployment -eq 'docker') {
            $archive = Join-Path $backupDir "paritr-mainnet-$stamp.tar.gz"
            $imageLine = Get-Content -LiteralPath (Join-Path $NodeDir '.env') | Where-Object { $_ -match '^PARITR_IMAGE_REF=' } | Select-Object -First 1
            if (-not $imageLine) { throw 'PARITR_IMAGE_REF is missing.' }
            $image = $imageLine.Substring('PARITR_IMAGE_REF='.Length)
            $container = "paritr-backup-$([Guid]::NewGuid().ToString('N'))"
            try {
                Invoke-Docker @('run','--name',$container,'--volume','paritr-mainnet-data:/data','--entrypoint','/bin/sh',$image,'-c','tar -C /data -czf /tmp/paritr-backup.tar.gz .')
                Invoke-Docker @('cp',"${container}:/tmp/paritr-backup.tar.gz",$archive)
            } finally {
                Invoke-Docker @('rm','-f',$container) -AllowFailure 2>$null
            }
        } else {
            $archive = Join-Path $backupDir "paritr-mainnet-$stamp.zip"
            Compress-Archive -LiteralPath $Config,(Join-Path $NodeDir 'data') -DestinationPath $archive
        }
    } finally { if ($wasRunning) { Start-Node } }
    Write-Host "Backup written: $archive"
}

function Invoke-Doctor {
    Assert-Installed
    Write-Host "Deployment : $Deployment"
    Write-Host "Directory  : $NodeDir"
    if ($Deployment -eq 'docker') {
        Invoke-Docker @('version','--format','Docker: {{.Server.Version}}')
        Invoke-Compose @('config','--quiet')
        Invoke-Compose @('ps')
    } else { & $Binary --version }
    Invoke-Node @('check')
    Show-StatusApi
}

switch ($Command.ToLowerInvariant()) {
    'start' { Start-Node }
    'stop' { Assert-Installed; Stop-Node }
    'restart' { Assert-Installed; Stop-Node; Start-Sleep -Seconds 1; Start-Node }
    'status' {
        Assert-Installed
        Write-Host "Service : $(if (Test-NodeRunning) { 'running' } else { 'stopped' })"
        if ($Deployment -eq 'docker') { Invoke-Compose @('ps') }
        Show-StatusApi
    }
    'logs' {
        Assert-Installed
        if ($Deployment -eq 'docker') { Invoke-Compose @('logs','--tail','200','-f','paritr-node') }
        else {
            if (-not (Test-Path -LiteralPath $Log)) { New-Item -ItemType File -Path $Log | Out-Null }
            Get-Content -LiteralPath $Log -Tail 200 -Wait
        }
    }
    'check' { Assert-Installed; Invoke-Node @('check') }
    'doctor' { Invoke-Doctor }
    'update' { Assert-Installed; Update-Installation }
    'backup' { Assert-Installed; Backup-Installation }
    'pair' {
        Assert-Installed
        $code = if ($Value) { $Value } else { Read-Host 'Pairing code' }
        $portal = if ($PortalUrl) { $PortalUrl } elseif ($env:PARITR_PORTAL_URL) { $env:PARITR_PORTAL_URL } else { Read-Host 'Portal HTTPS URL' }
        if ($portal -notmatch '^https://') { throw 'An HTTPS portal URL is required.' }
        $wasRunning = Test-NodeRunning
        if ($wasRunning) { Stop-Node }
        try { Invoke-Node @('pair','--portal-url',$portal,'--code',$code) }
        finally { if ($wasRunning) { Start-Node } }
    }
    'unpair' {
        Assert-Installed
        $wasRunning = Test-NodeRunning
        if ($wasRunning) { Stop-Node }
        try { Invoke-Node @('unpair') }
        finally { if ($wasRunning) { Start-Node } }
    }
    'config' { Assert-Installed; Invoke-Node @('show-config') }
    'access' { Assert-Installed; Invoke-Node @('admin-access') }
    default {
        Write-Host @'
Paritr Protocol 9 management

  .\manage.ps1 status
  .\manage.ps1 start|stop|restart
  .\manage.ps1 logs
  .\manage.ps1 check|doctor
  .\manage.ps1 pair PRTR-... https://portal.example
  .\manage.ps1 unpair
  .\manage.ps1 config
  .\manage.ps1 access
  .\manage.ps1 update
  .\manage.ps1 backup
'@
    }
}
