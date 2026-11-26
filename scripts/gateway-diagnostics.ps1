<#
.SYNOPSIS
  GATEWAY diagnostics. Offline by default; provider calls only with -Live.

.DESCRIPTION
  Wraps `solprint gateway-diagnostics`. The backend loads the repository .env
  itself; this script never reads, prints or copies credential values.

  Without -Live nothing is contacted: configuration is validated and the
  effective limits are printed, with credentials reported by name and
  presence only.

  With -Live a small, fixed, read-only check runs (at most 3 Helius credits,
  one Jupiter price request, one DEX Screener request; the public Solana RPC
  only with -IncludeFallback). Never transactions, never /swap/v2/execute.
  Credits are charged to the persisted ledger when PostgreSQL is reachable
  (start it with scripts/dev.ps1 -Mode db); otherwise the report says so.

.PARAMETER Live
  Explicit opt-in to contact the configured providers.

.PARAMETER IncludeFallback
  With -Live, also query the public Solana RPC fallback once.

.EXAMPLE
  ./scripts/gateway-diagnostics.ps1
  ./scripts/gateway-diagnostics.ps1 -Live
#>
[CmdletBinding()]
param(
    [switch]$Live,
    [switch]$IncludeFallback
)

. "$PSScriptRoot/_common.ps1"

if ($IncludeFallback -and -not $Live) { throw '-IncludeFallback requires -Live.' }
if (-not (Test-Path (Join-Path $RepoRoot '.env'))) { throw '.env missing. Run scripts/setup.ps1 first.' }
Assert-Command 'cargo' 'Install the Rust toolchain (see docs/development/SETUP.md).'

$argsList = @('gateway-diagnostics')
if ($Live) { $argsList += '--live' }
if ($IncludeFallback) { $argsList += '--include-fallback' }

if ($Live) {
    Write-Step 'GATEWAY diagnostics (LIVE: bounded read-only provider calls)'
} else {
    Write-Step 'GATEWAY diagnostics (offline: no provider is contacted)'
}

Push-Location $RepoRoot
try {
    Invoke-Checked 'gateway-diagnostics' {
        cargo run --quiet --manifest-path backend/Cargo.toml --bin solprint -- @argsList
    }
} finally {
    Pop-Location
}
Write-Step 'Diagnostics finished'
