<#
.SYNOPSIS
Run the Windows native build/CTest gate and Rust formatting/lint checks.
.DESCRIPTION
Checks VERSION against the Cargo workspace version, configures and builds the selected CMake
preset, runs its registered CTest fixtures, then checks Cargo formatting and Clippy with locked
dependencies. This script does not run cargo test, deploy files or establish game acceptance.
.PARAMETER Configuration
Debug or Release build/test preset; CI selects Release.
.PARAMETER VS2026
Select explicit Visual Studio 2026 presets. Default presets target VS 2022/windows-2022 CI.
.OUTPUTS
Build/check output; throws on the first failed external command or version mismatch.
#>
param(
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Debug',
    [switch]$VS2026
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot

# Convert native command failures to exceptions so the gate stops immediately.
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
    $suffix = if ($VS2026) { '-vs18' } else { '' }
    $preset = 'windows-' + $Configuration.ToLowerInvariant() + $suffix
    Invoke-Checked cmake @('--preset', ('windows-x64' + $suffix))
    Invoke-Checked cmake @('--build', '--preset', $preset)
    Invoke-Checked ctest @('--preset', $preset)
    Invoke-Checked cargo @('fmt', '--all', '--', '--check')
    Invoke-Checked cargo @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
}
finally {
    Pop-Location
}
