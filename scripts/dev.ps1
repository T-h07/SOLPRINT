<#
.SYNOPSIS
  Start SOLPRINT for development.

.PARAMETER Mode
  native (default)  PostgreSQL in Docker; backend (cargo run) and frontend
                    (Vite, hot reload) as local processes in their own windows.
                    Frontend: http://127.0.0.1:5173
  docker            Full stack in Docker Compose (build + start, detached).
                    Frontend: http://127.0.0.1:3000 (or SOLPRINT_WEB_PORT)
  db                PostgreSQL only.

.NOTES
  Never deletes data. Stop with scripts/stop.ps1.
#>
[CmdletBinding()]
param([ValidateSet('native', 'docker', 'db')][string]$Mode = 'native')

. "$PSScriptRoot/_common.ps1"
Set-Location $RepoRoot

if (-not (Test-Path (Join-Path $RepoRoot '.env'))) { throw '.env missing. Run scripts/setup.ps1 first.' }
if (-not (Test-DockerEngine)) { throw 'Docker engine not reachable. Start Docker Desktop and retry.' }

function Wait-Healthy([string]$Service, [int]$TimeoutSeconds = 120) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $id = (docker compose ps -q $Service) | Select-Object -First 1
        if ($id) {
            $health = docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' $id
            if ($health -eq 'healthy') { Write-Ok "$Service healthy"; return }
            if ($health -eq 'unhealthy') { throw "$Service is unhealthy. Inspect: docker compose logs $Service" }
        }
        Start-Sleep -Seconds 2
    }
    throw "$Service did not become healthy within $TimeoutSeconds s. Inspect: docker compose logs $Service"
}

Write-Step 'PostgreSQL'
Invoke-Checked 'docker compose up postgres' { docker compose up -d postgres }
Wait-Healthy 'postgres'

switch ($Mode) {
    'db' {
        $port = (Read-DotEnv (Join-Path $RepoRoot '.env'))['POSTGRES_HOST_PORT']
        Write-Ok "PostgreSQL on 127.0.0.1:$(if ($port) { $port } else { 55432 })"
    }
    'docker' {
        Write-Step 'Building and starting backend + frontend containers'
        $env:SOLPRINT_GIT_COMMIT = (git rev-parse HEAD 2>$null)
        Invoke-Checked 'docker compose up' { docker compose up --build -d }
        Wait-Healthy 'backend'
        Wait-Healthy 'frontend'
        $port = (Read-DotEnv (Join-Path $RepoRoot '.env'))['SOLPRINT_WEB_PORT']
        Write-Ok "SOLPRINT: http://127.0.0.1:$(if ($port) { $port } else { 3000 })"
    }
    'native' {
        $stateDir = Join-Path $RepoRoot '.solprint'
        New-Item -ItemType Directory -Force $stateDir | Out-Null
        $shell = if (Test-Command 'pwsh') { 'pwsh' } else { 'powershell' }
        Write-Step 'Starting backend (cargo run) and frontend (vite) in new windows'
        $backend = Start-Process $shell -PassThru -WorkingDirectory (Join-Path $RepoRoot 'backend') `
            -ArgumentList '-NoExit', '-Command', '$host.UI.RawUI.WindowTitle = "SOLPRINT backend"; cargo run --bin solprint'
        $frontend = Start-Process $shell -PassThru -WorkingDirectory (Join-Path $RepoRoot 'frontend') `
            -ArgumentList '-NoExit', '-Command', '$host.UI.RawUI.WindowTitle = "SOLPRINT frontend"; npm run dev'
        @{ backend = $backend.Id; frontend = $frontend.Id } | ConvertTo-Json | Set-Content (Join-Path $stateDir 'dev-pids.json')
        Write-Ok "backend window pid $($backend.Id) -> http://127.0.0.1:8080/health/ready"
        Write-Ok "frontend window pid $($frontend.Id) -> http://127.0.0.1:5173"
        Write-Note 'The backend window shows a configuration error and exits if .env is invalid.'
    }
}
