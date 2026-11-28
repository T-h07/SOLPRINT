<#
.SYNOPSIS
  RADAR diagnostics. Offline by default; provider work only with -Live -RequestCycle.

.DESCRIPTION
  Wraps `solprint radar` and the RADAR test suites. The backend loads the
  repository .env itself; this script never reads, prints or copies
  credential values, never resets or deletes a database, and never builds,
  signs or submits a transaction.

  Default (nothing is contacted):
    1. Policy fixtures, no database: market classification and screening,
       wallet eligibility, FIFO screening and activity windows, priority
       and qualification, lifecycle, monitoring rotation, budget admission
       (`cargo test -p solprint-radar --lib`).
    2. Local registry state, if PostgreSQL is reachable (scripts/dev.ps1
       -Mode db): status and control state, top wallets, markets and jobs.
    3. Discovery sources and budget admission as the running service sees
       them, if the API answers at -BaseUrl (local state only; the API never
       triggers provider traffic).

  -EndToEnd runs the fixture discovery suite on a DISPOSABLE PostgreSQL
  container (not the development database): an autonomous cycle from an
  empty registry, idempotent event redelivery and retraction, budget pause,
  provider outage, restart recovery without duplicate jobs, operator
  controls and coverage-aware dormancy. In-memory chain and fixture feed: no
  provider is contacted.

  -Live -RequestCycle asks the RUNNING service for one discovery cycle. It is
  refused unless the service reports RADAR running with autonomous
  discovery enabled (the owner's opt-in, RADAR_AUTO_DISCOVERY_ENABLED=true).
  The cycle is bounded by configuration: at most
  RADAR_MAX_MARKET_SAMPLES_PER_CYCLE pool samples (default 5, about 130
  Helius credits at most), under RADAR's budget share and GATEWAY admission.
  Wallet history jobs that follow are admitted the same way. The script waits
  up to -WaitSeconds for the cycle and prints its outcome.

.PARAMETER EndToEnd
  Run the fixture discovery suite on a disposable PostgreSQL container.

.PARAMETER Live
  Explicit opt-in to provider work (requires -RequestCycle).

.PARAMETER RequestCycle
  With -Live, request one bounded discovery cycle from the running service.

.PARAMETER BaseUrl
  Origin serving the API (default http://127.0.0.1:3000, Compose).

.PARAMETER WaitSeconds
  With -RequestCycle, how long to wait for the cycle (default 120).

.EXAMPLE
  ./scripts/radar-diagnostics.ps1
  ./scripts/radar-diagnostics.ps1 -EndToEnd
  ./scripts/radar-diagnostics.ps1 -Live -RequestCycle
#>
[CmdletBinding()]
param(
    [switch]$EndToEnd,
    [switch]$Live,
    [switch]$RequestCycle,
    [string]$BaseUrl = 'http://127.0.0.1:3000',
    [ValidateRange(15, 900)][int]$WaitSeconds = 120,
    [int]$Port = 55436
)

. "$PSScriptRoot/_common.ps1"

if ($Live -and -not $RequestCycle) { throw '-Live requires -RequestCycle.' }
if ($RequestCycle -and -not $Live) { throw '-RequestCycle contacts providers and requires the explicit -Live opt-in.' }
if ($BaseUrl -notmatch '^http://(127\.0\.0\.1|localhost|\[::1\]):\d+$') { throw 'BaseUrl must be a loopback origin such as http://127.0.0.1:3000.' }
if (-not (Test-Path (Join-Path $RepoRoot '.env'))) { throw '.env missing. Run scripts/setup.ps1 first.' }
Assert-Command 'cargo' 'Install the Rust toolchain (see docs/development/SETUP.md).'

function Invoke-Radar([string[]]$Arguments) {
    cargo run --quiet --manifest-path backend/Cargo.toml --bin solprint -- radar @Arguments
}

function Get-Api([string]$Path) {
    try {
        return Invoke-RestMethod -Uri "$BaseUrl$Path" -TimeoutSec 5 -ErrorAction Stop
    } catch {
        return $null
    }
}

Push-Location $RepoRoot
try {
    Write-Step 'Policy fixtures (offline: no database, no provider)'
    Invoke-Checked 'radar policy tests' {
        cargo test --quiet --manifest-path backend/Cargo.toml -p solprint-radar --lib --locked
    }

    Write-Step 'Local registry state (PostgreSQL only; no provider)'
    Invoke-Radar @('status')
    if ($LASTEXITCODE -ne 0) {
        Write-Note 'PostgreSQL not reachable or not migrated: local state skipped (start it with scripts/dev.ps1 -Mode db).'
        $global:LASTEXITCODE = 0
    } else {
        Write-Step 'Top wallets by investigation priority'
        Invoke-Checked 'radar wallets' { Invoke-Radar @('wallets', '--limit', '10') }
        Write-Step 'Market universe'
        Invoke-Checked 'radar markets' { Invoke-Radar @('markets', '--limit', '10') }
        Write-Step 'Work queue'
        Invoke-Checked 'radar jobs' { Invoke-Radar @('jobs', '--limit', '10') }
    }

    Write-Step "Running service at $BaseUrl (local state only)"
    $status = Get-Api '/api/v1/radar/status'
    if (-not $status) {
        Write-Note 'API not reachable: discovery-source and budget views skipped.'
    } else {
        $b = $status.budget
        Write-Ok ("RADAR enabled={0} running={1} autoDiscovery={2} paused={3}" -f $status.enabled, $status.running, $status.auto_discovery_enabled, $status.paused)
        Write-Ok ("budget mode={0} reason={1} gateway={2} allowance={3} available={4} spent24h={5}" -f $b.mode, $b.reason, $b.gateway_state, $b.allowance, $b.available, $b.spent_24h)
        $sources = Get-Api '/api/v1/radar/sources?window_hours=24'
        if ($sources) {
            foreach ($s in $sources.items) {
                Write-Ok ("{0,-22} last={1,-18} runs={2} failed={3} wallets={4} qualified={5}" -f $s.source, $s.last_status, $s.runs, $s.runs_failed, $s.unique_wallets, $s.qualified_wallets)
            }
            if (-not $sources.items) { Write-Note 'no source has run in the last 24 h' }
        }
    }

    if ($EndToEnd) {
        if (-not (Test-DockerEngine)) { throw 'Docker engine not reachable. Start Docker Desktop and retry.' }
        $name = 'solprint-radar-diag-db'
        $image = 'postgres:18.7-alpine3.24'
        $bytes = [byte[]]::new(16); [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
        $password = ([System.BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()
        docker rm -f $name *> $null
        Write-Step "Fixture discovery suite on a disposable PostgreSQL ($image) on 127.0.0.1:$Port"
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
            $env:SOLPRINT_TEST_DATABASE_URL = "postgres://solprint_test:$password@127.0.0.1:$Port/postgres"
            Invoke-Checked 'radar end-to-end' {
                cargo test --manifest-path backend/Cargo.toml -p solprint-radar --test radar_db --locked -- --include-ignored
            }
        } finally {
            Remove-Item Env:SOLPRINT_TEST_DATABASE_URL -ErrorAction SilentlyContinue
            docker rm -f $name *> $null
            Write-Note 'disposable PostgreSQL removed'
        }
    }

    if ($Live) {
        Write-Step 'LIVE: one bounded discovery cycle by the running service'
        if (-not $status) { throw "The service is not reachable at $BaseUrl; start it with scripts/dev.ps1." }
        if (-not $status.running) { throw 'RADAR is not running in the service (RADAR_ENABLED=false?).' }
        if (-not $status.auto_discovery_enabled) {
            throw 'Autonomous discovery is off (RADAR_AUTO_DISCOVERY_ENABLED=false). Enabling it is the owner''s decision; this script does not change configuration.'
        }
        if ($status.paused) { throw 'RADAR is paused by the operator (solprint radar resume).' }
        if ($status.budget.mode -eq 'PAUSED') { throw "Budget admission is paused ($($status.budget.reason)); no provider work would run." }
        $before = $status.last_cycle.cycle_id
        Invoke-Checked 'radar cycle' { Invoke-Radar @('cycle') }
        $deadline = (Get-Date).AddSeconds($WaitSeconds)
        $cycle = $null
        do {
            Start-Sleep -Seconds 5
            $c = Get-Api '/api/v1/radar/cycles?limit=1'
            if ($c -and $c.items -and $c.items[0].cycle_id -ne $before -and $c.items[0].status -ne 'RUNNING') { $cycle = $c.items[0] }
        } until ($cycle -or (Get-Date) -gt $deadline)
        if (-not $cycle) {
            Write-Warn2 "No finished cycle within $WaitSeconds s; check /radar or solprint radar status."
        } else {
            Write-Ok ("cycle {0}: {1} (trigger {2}) observations={3} new markets={4} selected={5} samples={6} est. credits={7}" -f $cycle.cycle_id, $cycle.status, $cycle.trigger, $cycle.observations, $cycle.markets_new, $cycle.markets_selected, $cycle.samples_scheduled, $cycle.credits_estimated)
            if ($cycle.deferred_reason) { Write-Note "deferred: $($cycle.deferred_reason)" }
            Write-Note 'Sample jobs and the history that follows run under budget admission; follow them in /radar.'
        }
    }
} finally {
    Pop-Location
}
Write-Step 'Diagnostics finished'
