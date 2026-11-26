# Shared helpers for SOLPRINT scripts. Dot-source: . "$PSScriptRoot/_common.ps1"
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Write-Step([string]$Message) { Write-Host "==> $Message" -ForegroundColor White }
function Write-Ok([string]$Message) { Write-Host "    ok  $Message" -ForegroundColor Green }
function Write-Note([string]$Message) { Write-Host "    --  $Message" -ForegroundColor DarkGray }
function Write-Warn2([string]$Message) { Write-Host "    !!  $Message" -ForegroundColor Yellow }

function Invoke-Checked {
    <# Run a native command; throw with a clear message if it fails. #>
    param([Parameter(Mandatory)][string]$Label, [Parameter(Mandatory)][scriptblock]$Command)
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Label failed (exit code $LASTEXITCODE)" }
}

function Test-Command([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Assert-Command([string]$Name, [string]$Hint) {
    if (-not (Test-Command $Name)) { throw "$Name not found. $Hint" }
}

function Read-DotEnv([string]$Path) {
    <# Parse KEY=VALUE lines into an ordered hashtable. Never prints values. #>
    $map = [ordered]@{}
    if (-not (Test-Path $Path)) { return $map }
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ($line -match '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=(.*)$') {
            $map[$Matches[1]] = $Matches[2].Trim()
        }
    }
    return $map
}

function Test-DockerEngine {
    if (-not (Test-Command 'docker')) { return $false }
    docker info --format '{{.ServerVersion}}' *> $null
    return ($LASTEXITCODE -eq 0)
}
