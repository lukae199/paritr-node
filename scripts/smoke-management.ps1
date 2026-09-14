# Bounded release check of the bundled DLL, shared UI/API listener and authentication.
[CmdletBinding()]
param([Parameter(Mandatory)][string] $Binary)
$ErrorActionPreference = 'Stop'
$taskDir = Join-Path ([IO.Path]::GetTempPath()) ('paritr-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskDir | Out-Null
$configPath = Join-Path $taskDir 'config.json'
& $Binary --config $configPath init --public-bind 127.0.0.1:15050 --admin-bind 127.0.0.1:15051 --management-bind 127.0.0.1:15051 | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Smoke configuration failed' }
$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
$config.seed_nodes = @()
$config | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding utf8
$process = Start-Process -FilePath $Binary -ArgumentList @('--config',('"' + $configPath + '"'),'run') -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $taskDir 'stderr.log') -RedirectStandardOutput (Join-Path $taskDir 'stdout.log')
try {
    $ready = $false
    for ($attempt = 0; $attempt -lt 45; $attempt++) {
        if ($process.HasExited) { throw 'Bundled node exited during startup' }
        try {
            $health = Invoke-RestMethod http://127.0.0.1:15050/health -TimeoutSec 1
            $ready = $health.chain_id -eq 'paritr-mainnet'
            if ($ready) { break }
        } catch { }
        Start-Sleep -Seconds 1
    }
    if (-not $ready) { throw 'Bundled node startup timed out' }
    $page = Invoke-WebRequest http://127.0.0.1:15051/ -TimeoutSec 5
    if ($page.Content -notmatch '/logo.svg') { throw 'Management page missing logo' }
    $logo = Invoke-WebRequest http://127.0.0.1:15051/logo.svg -TimeoutSec 5
    if ($logo.StatusCode -ne 200) { throw 'Logo unavailable' }
    $unauthorized = Invoke-WebRequest http://127.0.0.1:15051/admin/config -SkipHttpErrorCheck -TimeoutSec 5
    if ($unauthorized.StatusCode -ne 401) { throw 'Admin authentication not enforced' }
    $headers = @{ Authorization = 'Bearer ' + $config.admin_secret }
    $null = Invoke-RestMethod http://127.0.0.1:15051/admin/auth -Headers $headers -TimeoutSec 5
    $null = Invoke-RestMethod http://127.0.0.1:15051/admin/stop -Method Post -Headers $headers -ContentType application/json -Body '{}' -TimeoutSec 5
    $status = Invoke-RestMethod http://127.0.0.1:15051/admin/status -Headers $headers -TimeoutSec 5
    if ($status.node_enabled -ne $false) { throw 'Stop did not pause the node' }
    Write-Host 'Bundled node: public API, shared admin/UI, logo, authentication and stop OK.'
} finally {
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
}
