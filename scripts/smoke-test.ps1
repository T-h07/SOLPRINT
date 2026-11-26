<#
.SYNOPSIS
  Smoke-test a running SOLPRINT stack over HTTP.

.PARAMETER BaseUrl
  Frontend origin (same-origin API). Default http://127.0.0.1:3000 (Compose).
  Use http://127.0.0.1:5173 for the native Vite workflow.
#>
[CmdletBinding()]
param([string]$BaseUrl = 'http://127.0.0.1:3000')

. "$PSScriptRoot/_common.ps1"

$failures = 0
function Check([string]$Name, [scriptblock]$Test) {
    try {
        $result = & $Test
        if ($result -eq $false) { throw 'assertion failed' }
        Write-Ok $Name
    } catch {
        $script:failures++
        Write-Host "    FAIL $Name :: $($_.Exception.Message)" -ForegroundColor Red
    }
}

function Get-Http([string]$Path) {
    return Invoke-WebRequest -Uri "$BaseUrl$Path" -SkipHttpErrorCheck -TimeoutSec 10
}

Write-Step "Smoke test $BaseUrl"
Check 'liveness 200 ALIVE' { $r = Get-Http '/health/live'; $r.StatusCode -eq 200 -and ($r.Content | ConvertFrom-Json).status -eq 'ALIVE' }
Check 'readiness 200 READY (database available)' { $r = Get-Http '/health/ready'; $r.StatusCode -eq 200 -and ($r.Content | ConvertFrom-Json).status -eq 'READY' }
Check 'status: PAPER, live trading disabled, no execution capability' {
    $s = (Get-Http '/api/v1/system/status').Content | ConvertFrom-Json
    $s.trading.mode -eq 'PAPER' -and $s.trading.live_trading_enabled -eq $false -and $s.trading.execution_capability -eq 'NONE'
}
Check 'status: providers truthfully NOT_INITIALIZED' {
    $s = (Get-Http '/api/v1/system/status').Content | ConvertFrom-Json
    @($s.providers | Where-Object { $_.state -ne 'NOT_INITIALIZED' -or $null -ne $_.last_checked_at }).Count -eq 0
}
Check 'version endpoint' { ((Get-Http '/api/v1/system/version').Content | ConvertFrom-Json).name -eq 'solprint' }
Check 'unknown API route -> structured 404' {
    $r = Get-Http '/api/v1/does-not-exist'
    $r.StatusCode -eq 404 -and ($r.Content | ConvertFrom-Json).error.code -eq 'NOT_FOUND'
}
Check 'API security headers' { $r = Get-Http '/api/v1/system/version'; $r.Headers['X-Content-Type-Options'] -eq 'nosniff' -and $r.Headers['X-Request-Id'] }
Check 'frontend shell served' { $r = Get-Http '/'; $r.StatusCode -eq 200 -and $r.Content -match '<div id="root">' }
Check 'client route falls back to SPA' { (Get-Http '/system').StatusCode -eq 200 }

if ($failures -gt 0) { Write-Host "$failures smoke check(s) failed" -ForegroundColor Red; exit 1 }
Write-Step 'Smoke test passed'
