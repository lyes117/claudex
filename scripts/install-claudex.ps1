param(
    [string]$Binary = (Join-Path $PSScriptRoot '../codex-rs/target/dev-small/codex.exe'),
    [string]$HelperPackage = (Join-Path $env:USERPROFILE '.codex/packages/app-server-daemon/releases/0.160.0-x86_64-pc-windows-msvc')
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $Binary).Path
$install = Join-Path $env:LOCALAPPDATA 'Programs/Claudex/bin'
New-Item -ItemType Directory -Path $install -Force | Out-Null
$helpers = @{}
foreach ($helper in @('codex-code-mode-host.exe', 'codex-command-runner.exe', 'codex-windows-sandbox-setup.exe')) {
    $helperSource = Join-Path (Split-Path $source) $helper
    if (-not (Test-Path -LiteralPath $helperSource)) {
        $folder = if ($helper -eq 'codex-code-mode-host.exe') { 'bin' } else { 'codex-resources' }
        $helperSource = Join-Path (Join-Path $HelperPackage $folder) $helper
    }
    if (-not (Test-Path -LiteralPath $helperSource)) { throw "Required native helper missing: $helperSource" }
    $helpers[$helper] = $helperSource
}
Copy-Item -LiteralPath $source -Destination (Join-Path $install 'claudex.exe') -Force
Copy-Item -LiteralPath $helpers['codex-code-mode-host.exe'] -Destination $install -Force
$resources = Join-Path (Split-Path $install) 'codex-resources'
$pathDir = Join-Path (Split-Path $install) 'codex-path'
New-Item -ItemType Directory -Path $resources, $pathDir -Force | Out-Null
foreach ($helper in @('codex-command-runner.exe', 'codex-windows-sandbox-setup.exe')) {
    Copy-Item -LiteralPath $helpers[$helper] -Destination $resources -Force
}
if (Test-Path -LiteralPath (Join-Path $HelperPackage 'codex-resources/voice')) {
    Copy-Item -LiteralPath (Join-Path $HelperPackage 'codex-resources/voice') -Destination $resources -Recurse -Force
}
Copy-Item -LiteralPath (Get-Command rg.exe -ErrorAction Stop).Source -Destination $pathDir -Force
$manifest = @{ layoutVersion=1; version='0.160.0'; target='x86_64-pc-windows-msvc'; variant='claudex'; entrypoint='bin/claudex.exe'; resourcesDir='codex-resources'; pathDir='codex-path' }
[IO.File]::WriteAllText((Join-Path (Split-Path $install) 'codex-package.json'), ($manifest | ConvertTo-Json))
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'workflows.mjs') -Destination (Join-Path $install 'workflows.mjs') -Force
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$entries = @($userPath -split ';' | Where-Object { $_ })
if (-not ($entries | Where-Object { $_.TrimEnd('\') -ieq $install.TrimEnd('\') })) {
    [Environment]::SetEnvironmentVariable('Path', (($entries + $install) -join ';'), 'User')
}
$env:Path = "$install;$env:Path"
& (Join-Path $install 'claudex.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Installed binary did not launch' }
Write-Output "Installed native fork: $install"
