param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$directory = Join-Path $repo '.build-tools\ownedfixture\live-memory'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'Windows x64 .NET Framework C# compiler is required' }
$null = New-Item -ItemType Directory -Path $directory -Force
$output = Join-Path $directory 'auth-read-guard.exe'
& $compiler /nologo /target:exe /platform:x64 "/out:$output" (Join-Path $PSScriptRoot 'claude-memory-live-auth-guard.cs')
if ($LASTEXITCODE -ne 0) { throw 'Auth guard compilation failed' }
Write-Output $output
