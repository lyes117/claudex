param(
    [ValidateSet('dev-small', 'release')]
    [string]$Profile = 'dev-small',
    [switch]$IncludeTestFixtures
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$originalRepoRoot = $env:CODEX_REPO_ROOT
$originalV8Archive = $env:RUSTY_V8_ARCHIVE
$originalV8Binding = $env:RUSTY_V8_SRC_BINDING_PATH
$originalIncremental = $env:CARGO_INCREMENTAL
$locationPushed = $false
try {
    Push-Location -LiteralPath (Join-Path $repo 'codex-rs')
    $locationPushed = $true
    $env:CODEX_REPO_ROOT = $repo
    # Keep routine fork builds from accumulating large incremental caches.
    # An explicit caller choice still wins and is restored after the build.
    if ([string]::IsNullOrWhiteSpace($env:CARGO_INCREMENTAL)) {
        $env:CARGO_INCREMENTAL = '0'
    }
    $rustVersion = & rustc.exe -vV
    if ($LASTEXITCODE -ne 0 -or $rustVersion -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'This installer layout requires the x86_64-pc-windows-msvc Rust host.'
    }
    # Reuse upstream's pinned manifest and artifact verification, including its
    # support for explicit caller overrides. Keep V8's sandbox feature enabled.
    # PowerShell 5.1 may surface native stderr as ErrorRecord when redirected.
    # Check native exit codes explicitly instead of aborting on Cargo progress.
    $ErrorActionPreference = 'Continue'
    $v8Environment = @'
import json
import os
import sys
import tomllib
from pathlib import Path
repo = Path(os.environ['CODEX_REPO_ROOT'])
sys.path.insert(0, str(repo))
from scripts.codex_package.targets import TARGET_SPECS
from scripts.codex_package.v8 import resolve_codex_v8_cargo_env
# An explicit Cargo target changes both the V8 architecture and output layout.
# Refuse it rather than silently hand the installer an old host binary.
if os.environ.get('CARGO_BUILD_TARGET'):
    raise RuntimeError('Unset CARGO_BUILD_TARGET for this native Windows builder.')
cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
config_dirs = [directory / '.cargo' for directory in [repo / 'codex-rs', repo, *repo.parents]]
for config_dir in [*config_dirs, cargo_home]:
    legacy = config_dir / 'config'
    config = legacy if legacy.is_file() else config_dir / 'config.toml'
    if config.is_file():
        with config.open('rb') as source:
            target = tomllib.load(source).get('build', {}).get('target')
        if target:
            raise RuntimeError(f'Remove build.target from {config} for this native Windows builder.')
environment = resolve_codex_v8_cargo_env(
    TARGET_SPECS['x86_64-pc-windows-msvc'],
    cache_root=Path(os.environ['CODEX_REPO_ROOT']) / '.build-tools' / 'v8',
)
print(json.dumps(environment))
'@ | & python.exe -
    $fetchExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($fetchExit -ne 0) { throw 'Cannot resolve the verified V8 artifact pair.' }
    $v8Environment = $v8Environment | ConvertFrom-Json
    if ($v8Environment.PSObject.Properties['RUSTY_V8_ARCHIVE']) {
        $env:RUSTY_V8_ARCHIVE = $v8Environment.RUSTY_V8_ARCHIVE
        $env:RUSTY_V8_SRC_BINDING_PATH = $v8Environment.RUSTY_V8_SRC_BINDING_PATH
    }
    $buildArguments = @(
        'build', '--locked', '--profile', $Profile,
        '--target-dir', (Join-Path $repo 'codex-rs/target'),
        '-p', 'codex-cli', '-p', 'codex-code-mode-host', '-p', 'codex-windows-sandbox',
        '--bin', 'codex', '--bin', 'codex-code-mode-host',
        '--bin', 'codex-command-runner', '--bin', 'codex-windows-sandbox-setup'
    )
    if ($IncludeTestFixtures) {
        $buildArguments += @('-p', 'codex-rmcp-client', '--bin', 'test_stdio_server')
    }
    $ErrorActionPreference = 'Continue'
    & cargo.exe @buildArguments
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($buildExit -ne 0) { throw 'Claudex source build failed.' }
    Write-Output "Claudex and its native helpers built with profile $Profile."
}
finally {
    $env:CODEX_REPO_ROOT = $originalRepoRoot
    $env:RUSTY_V8_ARCHIVE = $originalV8Archive
    $env:RUSTY_V8_SRC_BINDING_PATH = $originalV8Binding
    $env:CARGO_INCREMENTAL = $originalIncremental
    if ($locationPushed) { Pop-Location }
}
