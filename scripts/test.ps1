<#
.SYNOPSIS
  Run all automated tests, including PostgreSQL integration tests, against a
  DISPOSABLE PostgreSQL container. The development database is never used.

.DESCRIPTION
  Starts `solprint-test-db` (tmpfs storage, random password, loopback port
  55433), runs the backend suite with ignored database tests included, runs
  the frontend unit tests, and removes the container afterwards, even on
  failure.

.PARAMETER BackendOnly
  Skip frontend unit tests.
#>
[CmdletBinding()]
param([switch]$BackendOnly, [int]$Port = 55433)

. "$PSScriptRoot/_common.ps1"

if (-not (Test-DockerEngine)) { throw 'Docker engine not reachable. Start Docker Desktop and retry.' }

$name = 'solprint-test-db'
$image = 'postgres:18.7-alpine3.24'
$bytes = [byte[]]::new(16); [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
$password = ([System.BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()

docker rm -f $name *> $null
Write-Step "Disposable PostgreSQL ($image) on 127.0.0.1:$Port"
Invoke-Checked 'docker run' {
    docker run -d --rm --name $name `
        -e POSTGRES_USER=solprint_test -e "POSTGRES_PASSWORD=$password" -e POSTGRES_DB=postgres `
        -p "127.0.0.1:${Port}:5432" --tmpfs /var/lib/postgresql $image | Out-Null
}
try {
    $deadline = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Milliseconds 500
        docker exec $name pg_isready -h 127.0.0.1 -U solprint_test -q *> $null
    } until ($LASTEXITCODE -eq 0 -or (Get-Date) -gt $deadline)
    if ($LASTEXITCODE -ne 0) { throw 'disposable PostgreSQL did not become ready' }
    Write-Ok 'ready'

    $env:SOLPRINT_TEST_DATABASE_URL = "postgres://solprint_test:$password@127.0.0.1:$Port/postgres"
    Push-Location (Join-Path $RepoRoot 'backend')
    try {
        Write-Step 'Backend: all tests including PostgreSQL integration'
        Invoke-Checked 'cargo test' { cargo test --workspace --all-features --locked -- --include-ignored }
    } finally { Pop-Location }

    if (-not $BackendOnly) {
        Push-Location (Join-Path $RepoRoot 'frontend')
        try {
            Write-Step 'Frontend: unit tests'
            Invoke-Checked 'vitest' { npm run --silent test:ci }
        } finally { Pop-Location }
    }
} finally {
    Remove-Item Env:SOLPRINT_TEST_DATABASE_URL -ErrorAction SilentlyContinue
    docker rm -f $name *> $null
    Write-Note 'disposable PostgreSQL removed'
}
Write-Step 'All tests passed'
