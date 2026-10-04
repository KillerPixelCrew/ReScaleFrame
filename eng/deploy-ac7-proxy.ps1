# Explicit local deployment, separate from verify.ps1. The caller names the game installation.
param(
    [Parameter(Mandatory = $true)][string]$GameDirectory,
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$gameRoot = (Resolve-Path -LiteralPath $GameDirectory).Path
if (-not (Test-Path -LiteralPath (Join-Path $gameRoot 'Ace7Game.exe') -PathType Leaf)) {
    throw 'GameDirectory must contain Ace7Game.exe.'
}
function Assert-GameStopped {
    if (Get-Process -Name Ace7Game -ErrorAction SilentlyContinue) {
        throw 'Close AC7 before deploying the proxy and overlay.'
    }
}
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}
Assert-GameStopped
Push-Location -LiteralPath $repoRoot
try {
    # Configure and verify first with eng/verify.ps1; this uses that existing native build tree.
    Invoke-Checked cmake @('--build', 'build/windows-x64', '--config', $Configuration,
                           '--target', 'rsf_proxy_ac7', 'rsf_overlay_host')
    $cargoArguments = @('build', '-p', 'rescaleframe-overlay', '--locked')
    $rustConfiguration = 'debug'
    if ($Configuration -eq 'Release') {
        $cargoArguments += '--release'
        $rustConfiguration = 'release'
    }
    Invoke-Checked cargo $cargoArguments
    $nativeBin = Join-Path $repoRoot "build/windows-x64/bin/$Configuration"
    $ac7Proxy = Join-Path $repoRoot "build/windows-x64/ac7/bin/$Configuration/dinput8.dll"
    $panel = Join-Path $repoRoot "target/$rustConfiguration/rescaleframe_overlay.dll"
    # This loads the real panel, exercises Insert, draws pixels and resizes a D3D11 swap chain.
    # Game-facing proxy names in the shared artifact directory can shadow Windows DLLs in
    # the fixture. Run its unchanged executable separately, with the real panel's full path.
    $overlayFixtureRoot = Join-Path $repoRoot "build/fixtures/ac7-overlay/$Configuration"
    New-Item -ItemType Directory -Path $overlayFixtureRoot -Force | Out-Null
    $overlayFixture = Join-Path $overlayFixtureRoot 'rsf_overlay_host.exe'
    Copy-Item -LiteralPath (Join-Path $nativeBin 'rsf_overlay_host.exe') -Destination $overlayFixture
    Invoke-Checked $overlayFixture @($panel)
    Assert-GameStopped
    # The overlay exposes FSR and XeSS; deploy their SR DLLs and notices too.
    & (Join-Path $PSScriptRoot 'deploy-ac7-sr-runtimes.ps1') -GameDirectory $gameRoot
    $backup = Join-Path $repoRoot ('.local/deploy-backups/ac7-pair-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
    New-Item -ItemType Directory -Path $backup | Out-Null
    $files = @(
        @{ Name = 'dinput8.dll'; Source = $ac7Proxy },
        @{ Name = 'ReScaleFrame.Game.AC7.dll'; Source = (Join-Path $nativeBin 'ReScaleFrame.Game.AC7.dll') },
        @{ Name = 'rescaleframe_overlay.dll'; Source = $panel }
    )
    foreach ($file in $files) {
        $destination = Join-Path $gameRoot $file.Name
        $file.Existed = Test-Path -LiteralPath $destination
        $file.Hash = (Get-FileHash -LiteralPath $file.Source -Algorithm SHA256).Hash
        if ($file.Existed) { Copy-Item -LiteralPath $destination -Destination (Join-Path $backup $file.Name) }
    }
    try {
        foreach ($file in $files) {
            $destination = Join-Path $gameRoot $file.Name
            Copy-Item -LiteralPath $file.Source -Destination $destination
            if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $file.Hash) {
                throw "Hash mismatch for $($file.Name)"
            }
        }
    }
    catch {
        foreach ($file in $files) {
            $destination = Join-Path $gameRoot $file.Name
            if ($file.Existed) {
                Copy-Item -LiteralPath (Join-Path $backup $file.Name) -Destination $destination
            } elseif (Test-Path -LiteralPath $destination) {
                Remove-Item -LiteralPath $destination
            }
        }
        throw
    }
    [ordered]@{
        Date = (Get-Date -Format o); Game = $gameRoot; Backup = $backup
        Configuration = $Configuration; Files = $files
        Validation = 'Real overlay ABI, Insert, rendering and resize passed; AC7 run pending'
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $backup 'deployment.json') -Encoding utf8
    Write-Output "Deployed matching proxy and overlay. Backup: $backup"
}
finally { Pop-Location }
