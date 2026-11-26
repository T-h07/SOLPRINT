<#
.SYNOPSIS
  Run Playwright suites inside the pinned Playwright container, so browser
  binaries are never installed on the host and screenshots render with the
  same fonts locally and in CI.

.PARAMETER Project
  ui      mocked API: states, keyboard, accessibility, responsive overflow
  visual  screenshot comparison against approved baselines
  e2e     the real Compose stack (start it first: scripts/dev.ps1 -Mode docker)
  all     ui + visual

.PARAMETER UpdateSnapshots
  Re-record visual baselines. New baselines are CANDIDATES: review the PNGs
  and commit them only after owner approval.

.EXAMPLE
  ./scripts/playwright.ps1 -Project ui
  ./scripts/playwright.ps1 -Project visual -UpdateSnapshots
  ./scripts/playwright.ps1 -Project e2e
#>
[CmdletBinding()]
param(
    [ValidateSet('ui', 'visual', 'e2e', 'all')][string]$Project = 'all',
    [switch]$UpdateSnapshots
)

. "$PSScriptRoot/_common.ps1"

if (-not (Test-DockerEngine)) { throw 'Docker engine not reachable. Start Docker Desktop and retry.' }

# Keep in lockstep with @playwright/test in frontend/package.json.
$image = 'mcr.microsoft.com/playwright:v1.65.1-noble'
$frontend = Join-Path $RepoRoot 'frontend'

$projects = if ($Project -eq 'all') { @('--project=ui', '--project=visual') } else { @("--project=$Project") }
$extra = @()
if ($UpdateSnapshots) { $extra += '--update-snapshots' }

$network = @()
$envArgs = @('-e', 'CI=1')
if ($Project -eq 'e2e') {
    # Reach the Compose frontend by service name on the Compose network.
    $network = @('--network', 'solprint_default')
    $envArgs += @('-e', 'E2E_BASE_URL=http://frontend:8080')
}

# node_modules lives in a named volume so Linux-native packages never mix
# with the host's Windows install.
$cmd = "npm ci --no-audit --no-fund --loglevel=error && npx playwright test $($projects -join ' ') $($extra -join ' ')"

Write-Step "Playwright ($Project) in $image"
Invoke-Checked 'playwright' {
    docker run --rm --init --ipc=host @network @envArgs `
        -v "${frontend}:/work" `
        -v 'solprint-playwright-node-modules:/work/node_modules' `
        -w /work $image bash -lc $cmd
}
Write-Step 'Playwright passed'
