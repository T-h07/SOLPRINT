<#
.SYNOPSIS
  DATA ENGINE diagnostics. Offline by default; one provider request only with -Live.

.DESCRIPTION
  Wraps `solprint data-engine`. The backend loads the repository .env itself;
  this script never reads, prints or copies credential values.

  Without -Live nothing is contacted:
    1. the decoder registry is listed (program ids and decoder version);
    2. every synthetic fixture is decoded and checked against its expected
       result (no database needed);
    3. if PostgreSQL is reachable (scripts/dev.ps1 -Mode db), local status,
       recent jobs and open quality issues are printed. If it is not, the
       report says so and the offline checks still count.

  With -Live -Signature <SIG>, exactly one transaction is queued and fetched
  now through GATEWAY: one getTransaction request, at most 1 Helius credit
  (or one public-RPC request if GATEWAY falls back), under budget admission
  and rate limits. Nothing is signed or submitted; no wallet is involved.
  The database must be running, because the result is persisted.

.PARAMETER Live
  Explicit opt-in to contact a provider (requires -Signature).

.PARAMETER Signature
  The transaction signature to ingest with -Live.

.PARAMETER Finalized
  With -Live, require finalized commitment instead of confirmed.

.EXAMPLE
  ./scripts/data-engine-diagnostics.ps1
  ./scripts/data-engine-diagnostics.ps1 -Live -Signature <SIGNATURE>
#>
[CmdletBinding()]
param(
    [switch]$Live,
    [string]$Signature,
    [switch]$Finalized
)

. "$PSScriptRoot/_common.ps1"

if ($Live -and -not $Signature) { throw '-Live requires -Signature <SIGNATURE>.' }
if ($Signature -and -not $Live) { throw '-Signature is only used with -Live (offline diagnostics never fetch).' }
if ($Finalized -and -not $Live) { throw '-Finalized requires -Live.' }
if ($Signature -and $Signature -notmatch '^[1-9A-HJ-NP-Za-km-z]{64,88}$') { throw 'Signature is not a base58 transaction signature.' }
if (-not (Test-Path (Join-Path $RepoRoot '.env'))) { throw '.env missing. Run scripts/setup.ps1 first.' }
Assert-Command 'cargo' 'Install the Rust toolchain (see docs/development/SETUP.md).'

function Invoke-DataEngine([string[]]$Arguments) {
    cargo run --quiet --manifest-path backend/Cargo.toml --bin solprint -- data-engine @Arguments
}

Push-Location $RepoRoot
try {
    Write-Step 'Decoder registry (local)'
    Invoke-Checked 'data-engine decoders' { Invoke-DataEngine @('decoders') }

    Write-Step 'Synthetic fixtures (offline decode, no database)'
    Invoke-Checked 'data-engine decode-fixture' { Invoke-DataEngine @('decode-fixture') }

    Write-Step 'Local state (PostgreSQL only; no provider)'
    Invoke-DataEngine @('status')
    if ($LASTEXITCODE -ne 0) {
        Write-Note 'PostgreSQL not reachable or not migrated: local state skipped (start it with scripts/dev.ps1 -Mode db).'
        if ($Live) { throw 'Live ingestion needs the database to persist its result.' }
        $global:LASTEXITCODE = 0
    } else {
        Write-Step 'Recent jobs'
        Invoke-Checked 'data-engine jobs' { Invoke-DataEngine @('jobs', '--limit', '10') }
        Write-Step 'Open quality issues'
        Invoke-Checked 'data-engine issues' { Invoke-DataEngine @('issues', '--limit', '10') }
    }

    if ($Live) {
        Write-Step 'LIVE: one getTransaction through GATEWAY (at most 1 Helius credit)'
        $ingest = @('ingest', $Signature, '--live')
        if ($Finalized) { $ingest += '--finalized' }
        Invoke-Checked 'data-engine ingest --live' { Invoke-DataEngine $ingest }
    }
} finally {
    Pop-Location
}
Write-Step 'Diagnostics finished'
