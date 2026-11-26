<#
.SYNOPSIS
  Prepare a SOLPRINT development environment. Safe to run repeatedly.

.DESCRIPTION
  * Checks the toolchain (Rust per rust-toolchain.toml, Node, Docker).
  * Creates .env from .env.example, or MERGES missing keys into an existing
    .env. Existing values are never changed, reordered or printed; the
    original file is backed up first (.env.backup-<timestamp>).
  * Generates random database passwords when missing or still placeholders,
    and keeps DATABASE_URL consistent with SOLPRINT_DB_PASSWORD.
  * Installs frontend dependencies from the lockfile (npm ci).

.PARAMETER SkipInstall
  Do not run npm ci.
#>
[CmdletBinding()]
param([switch]$SkipInstall)

. "$PSScriptRoot/_common.ps1"
Set-Location $RepoRoot

Write-Step 'Toolchain'
Assert-Command 'cargo' 'Install Rust via https://rustup.rs (toolchain is pinned by rust-toolchain.toml).'
Assert-Command 'node' 'Install Node.js 24 LTS.'
Assert-Command 'npm' 'Install Node.js 24 LTS.'
Write-Ok "cargo  $((cargo --version) -join '')"
Write-Ok "node   $(node --version)"
if (Test-DockerEngine) { Write-Ok "docker $(docker version --format '{{.Server.Version}}')" }
else { Write-Warn2 'Docker engine not reachable: start Docker Desktop before scripts/dev.ps1.' }

Write-Step 'Environment file (.env)'
$envPath = Join-Path $RepoRoot '.env'
$examplePath = Join-Path $RepoRoot '.env.example'
$example = Read-DotEnv $examplePath

function New-Secret {
    $bytes = [byte[]]::new(24)
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    return ([System.BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()
}

$existed = Test-Path $envPath
$current = Read-DotEnv $envPath
$additions = [ordered]@{}

foreach ($key in $example.Keys) {
    if (-not $current.Contains($key)) { $additions[$key] = $example[$key] }
}

# Generated secrets replace only *missing* keys or untouched placeholders.
$generated = @()
foreach ($key in 'POSTGRES_ADMIN_PASSWORD', 'SOLPRINT_DB_PASSWORD') {
    $value = if ($current.Contains($key)) { $current[$key] } else { $additions[$key] }
    if (-not $value -or $value -like 'change-me*') {
        $secret = New-Secret
        if ($current.Contains($key)) { $current[$key] = $secret; $generated += "$key (placeholder replaced)" }
        else { $additions[$key] = $secret; $generated += $key }
    }
}

$appPassword = if ($current.Contains('SOLPRINT_DB_PASSWORD')) { $current['SOLPRINT_DB_PASSWORD'] } else { $additions['SOLPRINT_DB_PASSWORD'] }
$hostPort = if ($current.Contains('POSTGRES_HOST_PORT')) { $current['POSTGRES_HOST_PORT'] } else { $additions['POSTGRES_HOST_PORT'] }
$expectedUrl = "postgres://solprint_app:$appPassword@127.0.0.1:$hostPort/solprint"
$urlNeedsUpdate = $false
if (-not $current.Contains('DATABASE_URL')) { $additions['DATABASE_URL'] = $expectedUrl }
elseif ($current['DATABASE_URL'] -like '*change-me*' -or ($generated -contains 'SOLPRINT_DB_PASSWORD (placeholder replaced)')) {
    $current['DATABASE_URL'] = $expectedUrl; $urlNeedsUpdate = $true
}

$placeholderEdits = @($generated | Where-Object { $_ -like '*placeholder replaced*' }).Count -gt 0 -or $urlNeedsUpdate

if (-not $existed) {
    Copy-Item -LiteralPath $examplePath -Destination $envPath
    $lines = Get-Content -LiteralPath $envPath
    foreach ($key in $additions.Keys) {
        $lines = $lines -replace "^$key=.*$", "$key=$($additions[$key])"
    }
    Set-Content -LiteralPath $envPath -Value $lines -Encoding utf8NoBOM
    Write-Ok '.env created from .env.example'
} elseif ($additions.Count -gt 0 -or $placeholderEdits) {
    $backup = "$envPath.backup-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Copy-Item -LiteralPath $envPath -Destination $backup
    Write-Ok "backup written: $(Split-Path $backup -Leaf) (gitignored)"
    if ($placeholderEdits) {
        # Rewrite only the placeholder lines, preserving every other line verbatim.
        $lines = Get-Content -LiteralPath $envPath
        foreach ($key in 'POSTGRES_ADMIN_PASSWORD', 'SOLPRINT_DB_PASSWORD', 'DATABASE_URL') {
            if ($current.Contains($key)) { $lines = $lines -replace "^$key=.*$", "$key=$($current[$key])" }
        }
        Set-Content -LiteralPath $envPath -Value $lines -Encoding utf8NoBOM
    }
    if ($additions.Count -gt 0) {
        $block = @('', "# --- Added by scripts/setup.ps1 on $(Get-Date -Format 'yyyy-MM-dd') (see .env.example) ---")
        foreach ($key in $additions.Keys) { $block += "$key=$($additions[$key])" }
        Add-Content -LiteralPath $envPath -Value $block -Encoding utf8NoBOM
    }
    Write-Ok ".env updated: $($additions.Count) key(s) added; existing values preserved"
    foreach ($key in $additions.Keys) { Write-Note "added $key" }
} else {
    Write-Ok '.env already complete; not modified'
}
foreach ($g in $generated) { Write-Note "generated random value for $g" }

$final = Read-DotEnv $envPath
if ($final['TRADING_MODE'] -and $final['TRADING_MODE'] -ne 'paper') { Write-Warn2 'TRADING_MODE is not "paper": the backend will refuse to start (Phase 00 is non-executing).' }
if ($final['LIVE_TRADING_ENABLED'] -and $final['LIVE_TRADING_ENABLED'] -ne 'false') { Write-Warn2 'LIVE_TRADING_ENABLED is not "false": the backend will refuse to start.' }
if ($final['SOLANA_FALLBACK_RPC_URL'] -and $final['SOLANA_FALLBACK_RPC_URL'] -notlike 'https://*') { Write-Warn2 'SOLANA_FALLBACK_RPC_URL must be an https:// URL (recommended: https://api.mainnet-beta.solana.com).' }

if (-not $SkipInstall) {
    Write-Step 'Frontend dependencies (npm ci)'
    Push-Location (Join-Path $RepoRoot 'frontend')
    try { Invoke-Checked 'npm ci' { npm ci --no-fund --no-audit } } finally { Pop-Location }
    Write-Ok 'installed from package-lock.json'
}

Write-Step 'Done. Next: scripts/dev.ps1'
