$ErrorActionPreference = 'Stop'
$node = (Get-Command node -ErrorAction Stop).Source
$target = Join-Path $env:USERPROFILE '.local\bin\claudex.cmd'
$launcher = Join-Path $PSScriptRoot 'launcher.mjs'
$content = "@echo off`r`n`"$node`" `"$launcher`" %*`r`nexit /b %errorlevel%`r`n"
if ((Test-Path -LiteralPath $target) -and ((Get-Content -LiteralPath $target -Raw) -ne $content)) {
    throw "Existing launcher differs; inspect it before replacement: $target"
}
[IO.File]::WriteAllText($target, $content, [Text.Encoding]::ASCII)
Write-Output "Installed: $target"
