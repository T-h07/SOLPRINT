<#
.SYNOPSIS
  Run the standard quality gate locally (the same checks as CI, minus the
  steps that need containers). Fails on the first failing check.

.PARAMETER WithDatabase
  Also run PostgreSQL integration tests via scripts/test.ps1.
#>
[CmdletBinding()]
param([switch]$WithDatabase)

. "$PSScriptRoot/_common.ps1"

$started = Get-Date
Push-Location (Join-Path $RepoRoot 'backend')
try {
    Write-Step 'Rust: format'
    Invoke-Checked 'cargo fmt' { cargo fmt --all -- --check }
    Write-Step 'Rust: clippy'
    Invoke-Checked 'cargo clippy' { cargo clippy --workspace --all-targets --all-features --locked -- -D warnings }
    Write-Step 'Rust: tests (database tests are ignored here; use -WithDatabase)'
    Invoke-Checked 'cargo test' { cargo test --workspace --all-features --locked }
} finally { Pop-Location }

Write-Step 'API contracts: generated TypeScript is up to date'
$diff = git -C $RepoRoot status --porcelain -- frontend/src/types/generated
if ($diff) { throw "frontend/src/types/generated changed after cargo test; commit the regenerated contracts.`n$diff" }
Write-Ok 'contracts in sync'

Push-Location (Join-Path $RepoRoot 'frontend')
try {
    Write-Step 'Frontend: format'
    Invoke-Checked 'prettier' { npm run --silent format:check }
    Write-Step 'Frontend: lint'
    Invoke-Checked 'eslint' { npm run --silent lint }
    Write-Step 'Frontend: typecheck'
    Invoke-Checked 'tsc' { npm run --silent typecheck }
    Write-Step 'Frontend: unit tests'
    Invoke-Checked 'vitest' { npm run --silent test:ci }
    Write-Step 'Frontend: production build'
    Invoke-Checked 'vite build' { npm run --silent build }
} finally { Pop-Location }

if (Test-DockerEngine) {
    Write-Step 'Compose configuration'
    Push-Location $RepoRoot
    try { Invoke-Checked 'docker compose config' { docker compose config --quiet } } finally { Pop-Location }
} else { Write-Note 'Docker engine not reachable: compose validation skipped' }

if ($WithDatabase) { & (Join-Path $PSScriptRoot 'test.ps1') -BackendOnly }

Write-Step ("All checks passed in {0:n0}s" -f ((Get-Date) - $started).TotalSeconds)
