param([ValidateSet('Debug', 'Release')][string]$Configuration = 'Debug')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Program failed with exit code $LASTEXITCODE"
    }
}

Push-Location -LiteralPath $repoRoot
try {
    $version = (Get-Content -Raw -LiteralPath 'VERSION').Trim()
    $cargoManifest = Get-Content -Raw -LiteralPath 'Cargo.toml'
    $cargoVersion = [regex]::Match($cargoManifest, '(?m)^version = "([^"]+)"$').Groups[1].Value
    if ($version -ne $cargoVersion) {
        throw 'VERSION and the Cargo workspace version must match.'
    }
    $preset = 'windows-' + $Configuration.ToLowerInvariant()
    Invoke-Checked cmake @('--preset', 'windows-x64')
    Invoke-Checked cmake @('--build', '--preset', $preset)
    Invoke-Checked ctest @('--preset', $preset)
    Invoke-Checked cargo @('fmt', '--all', '--', '--check')
    Invoke-Checked cargo @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
}
finally {
    Pop-Location
}
