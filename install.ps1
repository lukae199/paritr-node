<#
.SYNOPSIS
  Checksummed Paritr Protocol 10 installer for 64-bit Windows.
#>
[CmdletBinding()]
param(
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
    [switch] $AssumeYes
)

$ErrorActionPreference = 'Stop'
$NodeVersion = '4.1.0-rc.1'
$ProtocolVersion = 10
$ChainId = 'paritr-mainnet'
$RandomXTag = 'v1.2.3'
$RandomXCommit = '12f2c2ffe2108d6cf54c391fee33c8bc3646cdab'
$TaskName = 'ParitrNodeMainnet'
$Target = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -eq 'Arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$Source = $Source.TrimEnd('/')
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ($Fast -and $Light) { throw 'Fast and Light are mutually exclusive.' }
if ($PortalUrl -and $PortalUrl -notmatch '^https://') { throw 'PortalUrl must use HTTPS.' }
if ($PublicUrl -and $PublicUrl -notmatch '^https://') { throw 'PublicUrl must use HTTPS.' }
if ([bool]$PairCode -ne [bool]$PortalUrl) { throw 'PairCode and PortalUrl must be supplied together.' }

$osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($osArchitecture -notin @('X64','Arm64')) {
    throw "Unsupported Windows architecture $osArchitecture. A 64-bit x64 or ARM64 operating system is required."
}
if ($osArchitecture -eq 'Arm64') { Write-Host 'Windows ARM64 detected: installing the native ARM64 bundle.' }

if (-not $AssumeYes) {
    if (-not $Address) { $Address = Read-Host 'Mining reward address (optional)' }
    if (-not $PairCode) {
        $PairCode = Read-Host 'Portal pairing code (optional)'
        if ($PairCode -and -not $PortalUrl) { $PortalUrl = Read-Host 'Portal HTTPS URL' }
    }
}
if ([bool]$PairCode -ne [bool]$PortalUrl) { throw 'PairCode and PortalUrl must be supplied together.' }

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$Dir = (Resolve-Path -LiteralPath $Dir).Path
$Binary = Join-Path $Dir 'paritr-node.exe'
$Config = Join-Path $Dir 'config.json'
$Manage = Join-Path $Dir 'manage.ps1'
$Data = Join-Path $Dir 'data'
$Dll = Join-Path $Dir 'randomx.dll'
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$Temporary = Join-Path ([IO.Path]::GetTempPath()) ("paritr-mainnet-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Temporary | Out-Null

function Invoke-Checked([string] $Program, [string[]] $Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

function Stop-InstalledNode {
    foreach ($name in @($TaskName, 'ParitrNode')) {
        Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    }
    $legacyNode = Join-Path $Dir 'node.py'
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        ($_.ExecutablePath -and $_.ExecutablePath.Equals($Binary, [StringComparison]::OrdinalIgnoreCase)) -or
        ($_.CommandLine -and $_.CommandLine.IndexOf($legacyNode, [StringComparison]::OrdinalIgnoreCase) -ge 0)
    } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

try {
    Stop-InstalledNode

    if (Test-Path -LiteralPath $Config) {
        $isP9 = $false
        try { $isP9 = (Get-Content -LiteralPath $Config -Raw | ConvertFrom-Json).network -eq $ChainId } catch { }
        if (-not $isP9) {
            $legacy = Join-Path $Dir "protocol8-backup-$Stamp"
            New-Item -ItemType Directory -Path $legacy | Out-Null
            foreach ($item in @('config.json','data','node.py','venv','start-node.ps1')) {
                $path = Join-Path $Dir $item
                if (Test-Path -LiteralPath $path) { Move-Item -LiteralPath $path -Destination $legacy }
            }
            Write-Host "Protocol-8 files preserved in $legacy"
        }
    }

    Write-Host "Installing Paritr $NodeVersion / Protocol $ProtocolVersion"
    $localSource = (Test-Path -LiteralPath (Join-Path $ScriptDir 'Cargo.toml')) -and (Test-Path -LiteralPath (Join-Path $ScriptDir 'src'))
    if ($localSource) {
        foreach ($tool in @('cargo.exe','git.exe','cmake.exe')) {
            if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is required for a source install." }
        }
        Push-Location $ScriptDir
        try { Invoke-Checked 'cargo.exe' @('build','--release','--locked','--target',$Target) } finally { Pop-Location }
        Copy-Item -LiteralPath (Join-Path $ScriptDir "target\$Target\release\paritr-node.exe") -Destination $Binary -Force

        $rxSource = Join-Path $Temporary 'RandomX'
        Invoke-Checked 'git.exe' @('clone','--quiet','--depth','1','--branch',$RandomXTag,'https://github.com/tevador/RandomX.git',$rxSource)
        $actual = (& git.exe -C $rxSource rev-parse HEAD).Trim()
        if ($actual -ne $RandomXCommit) { throw "RandomX tag identity mismatch: $actual" }
        $rxPatch = Join-Path $ScriptDir 'packaging/randomx-msvc-ssse3.patch'
        Invoke-Checked 'git.exe' @('-C',$rxSource,'apply','--check',$rxPatch)
        Invoke-Checked 'git.exe' @('-C',$rxSource,'apply',$rxPatch)
        $rxBuild = Join-Path $rxSource 'build'
        $rxArch = if ($Target -eq 'aarch64-pc-windows-msvc') { 'ARM64' } else { 'x64' }
        $rxOptions = @('-S',$rxSource,'-B',$rxBuild,'-A',$rxArch,'-DCMAKE_BUILD_TYPE=Release','-DCMAKE_WINDOWS_EXPORT_ALL_SYMBOLS=ON','-DBUILD_SHARED_LIBS=ON','-DCMAKE_POLICY_DEFAULT_CMP0091=NEW','-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded','-DCMAKE_CXX_FLAGS=/fp:strict')
        if ($rxArch -eq 'ARM64') { $rxOptions += '-DARM_ID=portable' }
        Invoke-Checked 'cmake.exe' $rxOptions
        Invoke-Checked 'cmake.exe' @('--build',$rxBuild,'--config','Release','--target','randomx','--parallel')
        $rxDll = Get-ChildItem -LiteralPath $rxBuild -Recurse -File | Where-Object { $_.Name -in @('randomx.dll','librandomx.dll') } | Select-Object -First 1
        if (-not $rxDll) { throw 'RandomX shared library build did not produce a DLL.' }
        Copy-Item -LiteralPath $rxDll.FullName -Destination $Dll -Force
        Copy-Item -LiteralPath (Join-Path $ScriptDir 'manage.ps1') -Destination $Manage -Force
    } else {
        $archiveName = "paritr-node-$Target.zip"
        $archive = Join-Path $Temporary $archiveName
        $checksum = "$archive.sha256"
        Invoke-WebRequest -Uri "$Source/v$NodeVersion/$archiveName" -OutFile $archive -UseBasicParsing
        Invoke-WebRequest -Uri "$Source/v$NodeVersion/$archiveName.sha256" -OutFile $checksum -UseBasicParsing
        $expected = ([regex]::Match((Get-Content -LiteralPath $checksum -Raw), '(?i)\b[0-9a-f]{64}\b')).Value.ToLowerInvariant()
        $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not $expected -or $actual -ne $expected) { throw 'Release checksum mismatch.' }
        $unpacked = Join-Path $Temporary 'bundle'
        Expand-Archive -LiteralPath $archive -DestinationPath $unpacked
        $bundleBinary = Get-ChildItem -LiteralPath $unpacked -Filter 'paritr-node.exe' -Recurse -File | Select-Object -First 1
        $bundleDll = Get-ChildItem -LiteralPath $unpacked -Recurse -File | Where-Object { $_.Name -in @('randomx.dll','librandomx.dll') } | Select-Object -First 1
        $bundleManage = Get-ChildItem -LiteralPath $unpacked -Filter 'manage.ps1' -Recurse -File | Select-Object -First 1
        if (-not $bundleBinary -or -not $bundleDll -or -not $bundleManage) { throw 'Release bundle is incomplete.' }
        Copy-Item -LiteralPath $bundleBinary.FullName -Destination $Binary -Force
        Copy-Item -LiteralPath $bundleDll.FullName -Destination $Dll -Force
        Copy-Item -LiteralPath $bundleManage.FullName -Destination $Manage -Force
    }

    $mode = if ($Light) { 'light' } elseif ($Fast -or $Address) { 'fast' } else { 'light' }
    if (-not (Test-Path -LiteralPath $Config)) {
        $publicBindHost = if ($PublicUrl -or $OpenFirewall) { '0.0.0.0' } else { '127.0.0.1' }
        $arguments = @('--config',$Config,'init','--public-bind',"${publicBindHost}:$Port",'--admin-bind','127.0.0.1:5051','--management-bind','0.0.0.0:5051','--mining-threads',[string]$Cores,'--mining-intensity',[string]$Intensity,'--randomx-mode',$mode)
        if ($Address) { $arguments += @('--miner-address',$Address,'--enable-mining') }
        if ($PublicUrl) { $arguments += @('--public-url',$PublicUrl) }
        Invoke-Checked $Binary $arguments
    } else { Write-Host "Existing Protocol-9 configuration retained: $Config" }

    if ($PairCode) {
        try { Invoke-Checked $Binary @('--config',$Config,'pair','--portal-url',$PortalUrl,'--code',$PairCode) }
        catch { Write-Warning "Pairing failed; retry with manage.ps1: $($_.Exception.Message)" }
    }

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $sid = $identity.User.Value
    & icacls.exe $Dir '/inheritance:r' '/grant:r' "*$($sid):(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not apply a private ACL to the node directory.' }
    Invoke-Checked $Binary @('--config',$Config,'check')

    $startScript = Join-Path $Dir 'start-node.ps1'
    $escapedDir = $Dir.Replace("'", "''")
    $escapedBinary = $Binary.Replace("'", "''")
    $escapedConfig = $Config.Replace("'", "''")
    $escapedLog = (Join-Path $Dir 'paritr.log').Replace("'", "''")
    @"
`$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath '$escapedDir'
& '$escapedBinary' --config '$escapedConfig' run 1>> '$escapedLog' 2>> '$escapedLog'
"@ | Set-Content -LiteralPath $startScript -Encoding UTF8

    if (-not $NoAutostart) {
        try {
            $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$startScript`""
            $trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity.Name
            $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero)
            $principal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Limited
            Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null
            Start-ScheduledTask -TaskName $TaskName
        } catch {
            Write-Warning "Autostart could not be registered: $($_.Exception.Message)"
            Start-Process -FilePath $Binary -ArgumentList @('--config',$Config,'run') -WorkingDirectory $Dir -WindowStyle Hidden
        }
    } else {
        Disable-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue | Out-Null
    }

    if ($OpenFirewall) {
        try {
            Get-NetFirewallRule -DisplayName 'Paritr Protocol 10' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
            New-NetFirewallRule -DisplayName 'Paritr Protocol 10' -Direction Inbound -Protocol TCP -LocalPort $Port -Action Allow -Profile Any | Out-Null
        } catch { Write-Warning 'Firewall rule could not be installed; run this script as Administrator or open the port manually.' }
    }

    if (Test-Administrator) {
        Get-NetFirewallRule -DisplayName 'Paritr local management' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        Get-NetFirewallRule -DisplayName 'Paritr mDNS' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        New-NetFirewallRule -DisplayName 'Paritr local management' -Direction Inbound -Protocol TCP -LocalPort 5051 -RemoteAddress LocalSubnet -Profile Private -Action Allow | Out-Null
        New-NetFirewallRule -DisplayName 'Paritr mDNS' -Direction Inbound -Protocol UDP -LocalPort 5353 -RemoteAddress LocalSubnet -Profile Private -Action Allow | Out-Null
    } else {
        Write-Warning 'Run once as Administrator to open the local management and mDNS firewall rules for the Private network profile.'
    }

    Write-Host "Installation complete: $Dir"
    Write-Host "Local API: http://127.0.0.1:$Port"
    Write-Host "Manage: cd `"$Dir`"; .\manage.ps1 status"
    Invoke-Checked $Binary @('--config',$Config,'admin-access')
} finally {
    if (Test-Path -LiteralPath $Temporary) { Remove-Item -LiteralPath $Temporary -Recurse -Force }
}
