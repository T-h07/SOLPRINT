<#
.SYNOPSIS
  The single local verification gate. Verifies an exact commit in a clean,
  isolated checkout. SOLPRINT uses no hosted CI/CD.

.DESCRIPTION
  1. Resolves -Ref (default HEAD) to a commit SHA and creates a detached Git
     worktree for it in a temporary directory. Only committed files exist
     there: the real .env, untracked sources and developer-local
     configuration are never copied. Uncommitted changes are NOT verified.
  2. Runs every check in that checkout and records PASS / FAIL / BLOCKED /
     NOT RUN with duration and test counts. A failing check does not stop
     the remaining checks, so the report is complete.
       secret scan          gitleaks (pinned image, no network, redacted):
                            the candidate revision's files, the staged
                            changes and the full Git history
       Dockerfile lint      hadolint (pinned image)
       npm ci               locked frontend install
       scripts/check.ps1    fmt, clippy -D warnings, Rust unit + mock
                            provider/WebSocket tests, contract drift,
                            frontend format, lint, strict types, unit tests,
                            production build
       bundle guard         no fixture or showcase content in dist/
       scripts/test.ps1     PostgreSQL integration tests on disposable
                            databases with fresh migrations
       cargo deny           advisories, licences, bans, sources
                            (BLOCKED when cargo-deny is not installed)
       npm audit            high and critical fail
       Playwright           ui (states, keyboard, axe, responsive) and
                            visual (against committed, owner-approved
                            baselines; a missing baseline is a FAIL until
                            the owner approves one)
       Compose stack        own project name, image tags and loopback ports;
                            synthetic .env from setup.ps1 (random passwords,
                            no provider credentials); clean image builds,
                            startup on fresh volumes, container hardening,
                            smoke test, e2e, logs free of configured secrets
  3. Removes ONLY what this run created (its worktree, its Compose project
     with its own volumes and images, its override file), including on
     failure. The owner's development database, containers, volumes, images
     and .env are never touched. Logs stay in .solprint/verify/<sha>/.

  The verification stack holds no provider credentials and GATEWAY makes no
  background requests, so no paid provider is contacted.

.PARAMETER Ref
  Commit, branch or tag to verify. Default HEAD.

.PARAMETER Quick
  Development mode: reuse the build cache in .solprint/verify-target and
  skip Playwright and the Compose stack (reported NOT RUN). A quick run is
  never a full pass.

.PARAMETER KeepWorktree
  Keep the temporary checkout for inspection.

.EXAMPLE
  ./scripts/verify.ps1                 # full gate on HEAD
  ./scripts/verify.ps1 -Ref feature/01-gateway
  ./scripts/verify.ps1 -Quick
#>
[CmdletBinding()]
param(
    [string]$Ref = 'HEAD',
    [switch]$Quick,
    [switch]$KeepWorktree,
    [int]$WebPort = 3901,
    [int]$DbPort = 55491
)

. "$PSScriptRoot/_common.ps1"
$ErrorActionPreference = 'Stop'

$PlaywrightImage = 'mcr.microsoft.com/playwright:v1.65.1-noble'
$GitleaksImage = 'ghcr.io/gitleaks/gitleaks:v8.30.1'
$HadolintImage = 'hadolint/hadolint:v2.15.1'

Assert-Command 'git' 'Install Git.'
$sha = (git -C $RepoRoot rev-parse --verify "$Ref^{commit}" 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $sha) { throw "Cannot resolve '$Ref' to a commit." }
$sha = $sha.Trim()
$short = $sha.Substring(0, 12)
if (git -C $RepoRoot status --porcelain --untracked-files=no) {
    Write-Warn2 "Uncommitted changes exist; they are NOT verified. Verifying commit $short only."
}

$project = "solprint-verify-$short"
$work = Join-Path ([System.IO.Path]::GetTempPath()) $project
$override = Join-Path ([System.IO.Path]::GetTempPath()) "$project.compose.yaml"
$logDir = Join-Path $RepoRoot ".solprint/verify/$short"
New-Item -ItemType Directory -Force $logDir | Out-Null
$results = [System.Collections.Generic.List[object]]::new()
$started = Get-Date
$docker = Test-DockerEngine
$script:stackCreated = $false

function Get-TestCount([string]$Log) {
    if (-not (Test-Path $Log)) { return '' }
    $text = Get-Content -Raw $Log
    $cargo = 0; foreach ($m in [regex]::Matches($text, 'test result: ok\. (\d+) passed')) { $cargo += [int]$m.Groups[1].Value }
    $parts = @()
    if ($cargo -gt 0) { $parts += "rust $cargo" }
    $v = [regex]::Match($text, 'Tests\s+(\d+) passed'); if ($v.Success) { $parts += "vitest $($v.Groups[1].Value)" }
    foreach ($m in [regex]::Matches($text, '(?m)^\s+(\d+) passed \(')) { $parts += "playwright $($m.Groups[1].Value)" }
    foreach ($m in [regex]::Matches($text, '(?m)^\s+(\d+) failed')) { $parts += "failed $($m.Groups[1].Value)" }
    return ($parts -join ', ')
}

function Invoke-Gate([string]$Name, [scriptblock]$Body, [switch]$NeedsDocker) {
    if ($NeedsDocker -and -not $docker) {
        Write-Warn2 "BLOCKED $Name (Docker engine not reachable)"
        $results.Add([pscustomobject]@{ Check = $Name; Result = 'BLOCKED'; Seconds = 0; Tests = 'Docker engine not reachable' })
        return $false
    }
    Write-Step $Name
    $slug = ($Name -replace '[^A-Za-z0-9]+', '-').Trim('-').ToLowerInvariant()
    $log = Join-Path $logDir "$slug.log"
    $t = Get-Date
    $result = 'PASS'
    $global:LASTEXITCODE = 0
    try {
        & $Body *>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw "exit code $LASTEXITCODE" }
    } catch {
        $result = 'FAIL'
        Write-Host "    FAIL $Name :: $($_.Exception.Message)" -ForegroundColor Red
        Add-Content -LiteralPath $log -Value "FAIL: $($_.Exception.Message)"
    }
    $results.Add([pscustomobject]@{ Check = $Name; Result = $result; Seconds = [int]((Get-Date) - $t).TotalSeconds; Tests = (Get-TestCount $log) })
    $global:LASTEXITCODE = 0
    return ($result -eq 'PASS')
}

function Set-Status([string]$Name, [string]$Status, [string]$Why) {
    Write-Note "$Status $Name ($Why)"
    $results.Add([pscustomobject]@{ Check = $Name; Result = $Status; Seconds = 0; Tests = $Why })
}

function Invoke-Compose {
    docker compose -p $project --project-directory $work -f (Join-Path $work 'compose.yaml') -f $override @args
}

try {
    $ok = Invoke-Gate "Isolated checkout of $short" {
        if (Test-Path $work) {
            git -C $RepoRoot worktree remove --force $work *> $null
            Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
        }
        Invoke-Checked 'git worktree add' { git -C $RepoRoot worktree add --detach $work $sha 2>&1 }
        if (Test-Path (Join-Path $work '.env')) { throw 'a .env is tracked in the commit' }
        Write-Ok $work
    }
    if (-not $ok) { throw 'cannot create the isolated checkout' }

    $env:CARGO_TARGET_DIR = if ($Quick) { Join-Path $RepoRoot '.solprint/verify-target' } else { Join-Path $work 'backend/target' }
    Write-Note "Cargo target: $($env:CARGO_TARGET_DIR) ($(if ($Quick) { 'reused cache' } else { 'clean build' }))"

    Invoke-Gate 'Secret scan: candidate revision' -NeedsDocker {
        Invoke-Checked 'gitleaks dir' { docker run --rm --network none -v "${work}:/repo:ro" $GitleaksImage dir /repo --redact --no-banner --exit-code 1 }
    } | Out-Null
    Invoke-Gate 'Secret scan: staged changes and full history' -NeedsDocker {
        # Git produces the text on the host and gitleaks reads it from stdin in
        # an offline container: no directory (and no .env) is mounted.
        Invoke-Checked 'gitleaks staged' { git -C $RepoRoot diff --cached | docker run --rm -i --network none $GitleaksImage stdin --redact --no-banner --exit-code 1 }
        Invoke-Checked 'gitleaks history' { git -C $RepoRoot log -p --all --full-history | docker run --rm -i --network none $GitleaksImage stdin --redact --no-banner --exit-code 1 }
    } | Out-Null

    Invoke-Gate 'Dockerfile lint (hadolint)' -NeedsDocker {
        foreach ($f in Get-ChildItem (Join-Path $work 'infrastructure/docker') -Filter '*.Dockerfile') {
            Invoke-Checked "hadolint $($f.Name)" { Get-Content -Raw $f.FullName | docker run --rm -i --network none $HadolintImage hadolint --failure-threshold warning - }
        }
    } | Out-Null

    $npmOk = Invoke-Gate 'Locked frontend install (npm ci)' {
        Push-Location (Join-Path $work 'frontend')
        try { Invoke-Checked 'npm ci' { npm ci --no-audit --no-fund --loglevel=error } } finally { Pop-Location }
    }

    if ($npmOk) {
        Invoke-Gate 'Quality gate (check.ps1)' { & (Join-Path $work 'scripts/check.ps1') } | Out-Null
        Invoke-Gate 'Production bundle excludes fixtures and showcase' {
            $dist = Join-Path $work 'frontend/dist'
            if (-not (Test-Path $dist)) { throw 'frontend/dist missing (build failed)' }
            $hits = Get-ChildItem -Recurse -File $dist | Select-String -Pattern 'FIXTURE|DEMO DATA|Colour tokens' -List
            if ($hits) { throw "fixture or showcase content in dist: $($hits.Path -join ', ')" }
        } | Out-Null
    } else {
        Set-Status 'Quality gate (check.ps1)' 'BLOCKED' 'npm ci failed'
        Set-Status 'Production bundle excludes fixtures and showcase' 'BLOCKED' 'npm ci failed'
    }

    Invoke-Gate 'PostgreSQL integration tests (test.ps1)' -NeedsDocker { & (Join-Path $work 'scripts/test.ps1') -BackendOnly } | Out-Null

    if (Test-Command 'cargo-deny') {
        Invoke-Gate 'Rust advisories and licences (cargo deny)' {
            Push-Location (Join-Path $work 'backend')
            try { Invoke-Checked 'cargo deny' { cargo deny --all-features check } } finally { Pop-Location }
        } | Out-Null
    } else { Set-Status 'Rust advisories and licences (cargo deny)' 'BLOCKED' 'cargo-deny not installed (cargo install cargo-deny --locked)' }

    Invoke-Gate 'npm advisories (npm audit --audit-level=high)' {
        Push-Location (Join-Path $work 'frontend')
        try { Invoke-Checked 'npm audit' { npm audit --audit-level=high } } finally { Pop-Location }
    } | Out-Null

    if ($Quick) {
        Set-Status 'Playwright ui' 'NOT RUN' '-Quick'
        Set-Status 'Playwright visual' 'NOT RUN' '-Quick'
        Set-Status 'Compose stack, smoke, e2e' 'NOT RUN' '-Quick'
    } else {
        Invoke-Gate 'Playwright ui (states, keyboard, axe, responsive)' -NeedsDocker { & (Join-Path $work 'scripts/playwright.ps1') -Project ui } | Out-Null
        Invoke-Gate 'Playwright visual (approved baselines)' -NeedsDocker { & (Join-Path $work 'scripts/playwright.ps1') -Project visual } | Out-Null

        $envOk = Invoke-Gate 'Synthetic .env and isolated Compose config' -NeedsDocker {
            Push-Location $RepoRoot   # setup.ps1 changes directory
            try { & (Join-Path $work 'scripts/setup.ps1') -SkipInstall } finally { Pop-Location }
            $envFile = Join-Path $work '.env'
            $lines = Get-Content -LiteralPath $envFile
            $lines = $lines -replace '^SOLPRINT_WEB_PORT=.*$', "SOLPRINT_WEB_PORT=$WebPort"
            $lines = $lines -replace '^POSTGRES_HOST_PORT=.*$', "POSTGRES_HOST_PORT=$DbPort"
            Set-Content -LiteralPath $envFile -Value $lines -Encoding utf8NoBOM
            # Own image tags, so the owner's solprint-*:dev images are never replaced.
            @"
services:
  backend:
    image: solprint-verify-backend:$short
  frontend:
    image: solprint-verify-frontend:$short
"@ | Set-Content -LiteralPath $override -Encoding utf8NoBOM
            $script:stackCreated = $true
            Invoke-Checked 'compose config' { Invoke-Compose config --quiet }
        }

        $upOk = $false
        if ($envOk) {
            $upOk = Invoke-Gate 'Clean image builds and stack startup' {
                $env:SOLPRINT_GIT_COMMIT = $sha
                Invoke-Checked 'compose build' { Invoke-Compose build --pull }
                Invoke-Checked 'compose up' { Invoke-Compose up -d --wait --wait-timeout 180 }
            }
        } else { Set-Status 'Clean image builds and stack startup' 'BLOCKED' 'no isolated configuration' }

        if ($upOk) {
            Invoke-Gate 'Container hardening' {
                foreach ($svc in 'backend', 'frontend') {
                    $id = (Invoke-Compose ps -q $svc) | Select-Object -First 1
                    $ro = docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' $id
                    $caps = docker inspect -f '{{json .HostConfig.CapDrop}}' $id
                    $user = docker inspect -f '{{.Config.User}}' $id
                    Write-Note "$svc readonly=$ro capdrop=$caps user=$user"
                    if ($ro -ne 'true' -or $caps -notmatch 'ALL' -or -not $user -or $user -in @('root', '0')) { throw "$svc is not hardened" }
                }
                $backendId = (Invoke-Compose ps -q backend) | Select-Object -First 1
                $bindings = docker inspect -f '{{json .HostConfig.PortBindings}}' $backendId
                if ($bindings -notin @('{}', 'null')) { throw "backend publishes host ports: $bindings" }
            } | Out-Null
            Invoke-Gate 'Smoke test' { & (Join-Path $work 'scripts/smoke-test.ps1') -BaseUrl "http://127.0.0.1:$WebPort" } | Out-Null
            Invoke-Gate 'End-to-end (real stack)' {
                Invoke-Checked 'playwright e2e' {
                    docker run --rm --init --ipc=host --network "${project}_default" `
                        -e CI=1 -e E2E_BASE_URL=http://frontend:8080 `
                        -v "$(Join-Path $work 'frontend'):/work" `
                        -v 'solprint-playwright-node-modules:/work/node_modules' -w /work $PlaywrightImage `
                        bash -lc 'npm ci --no-audit --no-fund --loglevel=error && npx playwright test --project=e2e'
                }
            } | Out-Null
            Invoke-Gate 'Container logs contain no configured secret' {
                $logText = (Invoke-Compose logs --no-color) -join "`n"
                $envMap = Read-DotEnv (Join-Path $work '.env')
                foreach ($key in 'POSTGRES_ADMIN_PASSWORD', 'SOLPRINT_DB_PASSWORD') {
                    $value = $envMap[$key]
                    if ($value -and $logText.Contains($value)) { throw "$key value appears in container logs" }
                }
                Write-Ok 'no configured secret in logs'
            } | Out-Null
        } else {
            foreach ($n in 'Container hardening', 'Smoke test', 'End-to-end (real stack)', 'Container logs contain no configured secret') {
                Set-Status $n 'BLOCKED' 'stack did not start'
            }
        }
    }
} catch {
    Write-Host "    ABORTED: $($_.Exception.Message)" -ForegroundColor Red
    $results.Add([pscustomobject]@{ Check = 'verification run'; Result = 'FAIL'; Seconds = 0; Tests = $_.Exception.Message })
} finally {
    Write-Step 'Cleanup (resources created by this run only)'
    if ($script:stackCreated -and (Test-Path $override)) {
        # Scoped to this run's own Compose project: its containers, network,
        # volumes and locally built images. Never the development project.
        Invoke-Compose down --volumes --rmi local --remove-orphans *> $null
        Write-Note "Compose project $project removed"
    }
    Remove-Item -LiteralPath $override -ErrorAction SilentlyContinue
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:SOLPRINT_GIT_COMMIT -ErrorAction SilentlyContinue
    if ($KeepWorktree) { Write-Note "checkout kept at $work" }
    elseif (Test-Path $work) {
        git -C $RepoRoot worktree remove --force $work *> $null
        if (Test-Path $work) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
        git -C $RepoRoot worktree prune *> $null
        Write-Note 'temporary checkout removed'
    }
}

$elapsed = [int]((Get-Date) - $started).TotalSeconds
$summary = @(
    "SOLPRINT verification of $sha"
    "mode: $(if ($Quick) { 'QUICK (not a full gate)' } else { 'FULL' })   finished: $((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))   duration: ${elapsed}s"
    "toolchain: $((rustc --version) -join '') | node $(node --version) | docker $(if ($docker) { docker version --format '{{.Server.Version}}' } else { 'unavailable' })"
    ($results | Format-Table -AutoSize -Wrap | Out-String).TrimEnd()
)
$summary | Set-Content -LiteralPath (Join-Path $logDir 'summary.txt') -Encoding utf8NoBOM
Write-Step 'Summary'
$summary | ForEach-Object { Write-Host $_ }
Write-Note "logs: $logDir"

$bad = @($results | Where-Object { $_.Result -in @('FAIL', 'BLOCKED') }).Count
$notRun = @($results | Where-Object { $_.Result -eq 'NOT RUN' }).Count
if ($bad -gt 0) { Write-Host "VERIFY FAILED: $bad check(s) FAIL or BLOCKED" -ForegroundColor Red; exit 1 }
if ($notRun -gt 0) { Write-Host "QUICK VERIFY PASSED; $notRun check(s) NOT RUN (not a full gate)" -ForegroundColor Yellow; exit 0 }
Write-Host 'VERIFY PASSED (full gate)' -ForegroundColor Green
exit 0
