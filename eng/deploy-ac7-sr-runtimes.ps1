# Explicit local SDK deployment. This is never called by verify.ps1.
param(
    [Parameter(Mandatory = $true)][string]$GameDirectory,
    [string]$FidelityFxRoot = '',
    [string]$XeSSRoot = '',
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$gameRoot = (Resolve-Path -LiteralPath $GameDirectory).Path
if (-not (Test-Path -LiteralPath (Join-Path $gameRoot 'Ace7Game.exe') -PathType Leaf)) {
    throw 'GameDirectory must contain Ace7Game.exe.'
}
if (-not $FidelityFxRoot) { $FidelityFxRoot = Join-Path $repoRoot 'vendor/fidelityfx' }
if (-not $XeSSRoot) { $XeSSRoot = Join-Path $repoRoot 'vendor/xess' }
$inventory = @(
    @{ Source = (Join-Path $FidelityFxRoot 'Kits/FidelityFX/signedbin/amd_fidelityfx_upscaler_dx12.dll'); Relative = 'ReScaleFrame/fidelityfx/amd_fidelityfx_upscaler_dx12.dll' },
    @{ Source = (Join-Path $FidelityFxRoot 'Kits/FidelityFX/docs/license.md'); Relative = 'ReScaleFrame/fidelityfx/LICENSE.md' },
    @{ Source = (Join-Path $FidelityFxRoot '3rdpartynotice.md'); Relative = 'ReScaleFrame/fidelityfx/3rdpartynotice.md' },
    @{ Source = (Join-Path $XeSSRoot 'bin/libxess.dll'); Relative = 'ReScaleFrame/xess/libxess.dll' },
    @{ Source = (Join-Path $XeSSRoot 'LICENSE.txt'); Relative = 'ReScaleFrame/xess/LICENSE.txt' }
)
# Validate every source before touching the installation. No SDK discovery or downloads.
foreach ($file in $inventory) {
    $file.Source = (Resolve-Path -LiteralPath $file.Source).Path
    if (-not (Test-Path -LiteralPath $file.Source -PathType Leaf)) { throw "Missing SDK file: $($file.Source)" }
    $file.Hash = (Get-FileHash -LiteralPath $file.Source -Algorithm SHA256).Hash
    if ([IO.Path]::GetExtension($file.Source) -eq '.dll') {
        $signature = Get-AuthenticodeSignature -LiteralPath $file.Source
        if ($signature.Status -ne 'Valid') { throw "SDK signature verification failed: $($file.Source)" }
    }
    $file.Destination = Join-Path $gameRoot $file.Relative
}
if ($ValidateOnly) {
    $inventory | ForEach-Object { Write-Output "Validated $($_.Relative) SHA256=$($_.Hash)" }
    return
}
function Assert-GameStopped {
    if (Get-Process -Name Ace7Game -ErrorAction SilentlyContinue) {
        throw 'Close AC7 before deploying its SR runtimes.'
    }
}
Assert-GameStopped
$backup = Join-Path $repoRoot ('.local/deploy-backups/ac7-sr-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $backup | Out-Null
foreach ($file in $inventory) {
    $file.Existed = Test-Path -LiteralPath $file.Destination -PathType Leaf
    if ($file.Existed) {
        $saved = Join-Path $backup $file.Relative
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $saved) | Out-Null
        Copy-Item -LiteralPath $file.Destination -Destination $saved
    }
}
Assert-GameStopped
$written = @()
try {
    foreach ($file in $inventory) {
        $written += $file
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $file.Destination) | Out-Null
        Copy-Item -LiteralPath $file.Source -Destination $file.Destination
        if ((Get-FileHash -LiteralPath $file.Destination -Algorithm SHA256).Hash -ne $file.Hash) {
            throw "Hash mismatch for $($file.Relative)"
        }
    }
}
catch {
    foreach ($file in $written) {
        if ($file.Existed) {
            Copy-Item -LiteralPath (Join-Path $backup $file.Relative) -Destination $file.Destination
        } elseif (Test-Path -LiteralPath $file.Destination -PathType Leaf) {
            Remove-Item -LiteralPath $file.Destination
        }
    }
    throw
}
[ordered]@{
    Date = (Get-Date -Format o); Game = $gameRoot; Backup = $backup
    Files = $inventory
    Validation = 'Unmodified signed SR DLLs and notices copied; SHA256 readback matched. Game switching pending.'
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $backup 'deployment.json') -Encoding utf8
Write-Output "Deployed FidelityFX SR and XeSS SR runtimes. Backup: $backup"
