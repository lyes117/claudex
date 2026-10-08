param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$directory = Join-Path $repo '.build-tools\ownedfixture\live-memory'
$null = New-Item -ItemType Directory -Path $directory -Force
$output = Join-Path $directory 'live-memory-supervisor.exe'
& $compiler /nologo /target:exe /platform:x64 /reference:System.Web.Extensions.dll "/out:$output" (Join-Path $PSScriptRoot 'claude-memory-live-supervisor.cs')
if ($LASTEXITCODE -ne 0) { throw 'Live fixture supervisor compilation failed' }
Write-Output $output
