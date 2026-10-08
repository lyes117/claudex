param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$source = Join-Path $PSScriptRoot 'claude-memory-observer-proxy-launcher.cs'
$directory = Join-Path $repo '.build-tools\ownedfixture\observer-proxy'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'Windows x64 .NET Framework C# compiler is required' }
$null = New-Item -ItemType Directory -Path $directory -Force
$output = Join-Path $directory 'claude-memory-observer-proxy.exe'
# This builds only an owned instrumentation launcher. No binary is installed globally.
& $compiler /nologo /target:exe /platform:x64 /reference:System.Web.Extensions.dll "/out:$output" $source
if ($LASTEXITCODE -ne 0) { throw 'Observer proxy launcher compilation failed' }
Write-Output $output
