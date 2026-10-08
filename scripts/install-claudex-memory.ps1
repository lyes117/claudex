param(
    [Parameter(Mandatory=$true)][string]$Package,
    [string]$Binary = (Join-Path $env:LOCALAPPDATA 'Programs/Claudex/bin/claudex.exe'),
    [string]$Bun = (Join-Path $env:USERPROFILE '.bun/bin/bun.exe'),
    [int]$Port = 37778
)
$ErrorActionPreference = 'Stop'
$packagePath = (Resolve-Path -LiteralPath $Package).Path
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
$bunPath = (Resolve-Path -LiteralPath $Bun).Path
& node (Join-Path $PSScriptRoot 'claude-memory.mjs') install --package $packagePath --binary $binaryPath --bun $bunPath --port $Port
if ($LASTEXITCODE -ne 0) { throw 'Native Claudex memory installation failed' }
