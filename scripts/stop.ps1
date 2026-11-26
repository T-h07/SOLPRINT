<#
.SYNOPSIS
  Stop SOLPRINT development processes and containers. Never deletes data:
  containers are stopped (not removed) and the PostgreSQL volume is kept.
#>
[CmdletBinding()]
param()

. "$PSScriptRoot/_common.ps1"
Set-Location $RepoRoot

$pidFile = Join-Path $RepoRoot '.solprint/dev-pids.json'
if (Test-Path $pidFile) {
    Write-Step 'Native processes'
    $pids = Get-Content $pidFile | ConvertFrom-Json
    foreach ($name in 'backend', 'frontend') {
        $id = $pids.$name
        $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($proc) {
            # Closing the window process tree sends the console close event,
            # which the backend handles as a graceful shutdown.
            Stop-Process -Id $id -Confirm:$false
            Write-Ok "$name window (pid $id) stopped"
        } else { Write-Note "$name window already closed" }
    }
    Remove-Item $pidFile -Confirm:$false
}

if (Test-DockerEngine) {
    Write-Step 'Containers'
    Invoke-Checked 'docker compose stop' { docker compose stop }
    Write-Ok 'containers stopped; PostgreSQL data volume preserved'
} else {
    Write-Note 'Docker engine not reachable; no containers to stop'
}
