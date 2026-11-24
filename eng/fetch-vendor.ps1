<#
.SYNOPSIS
Checks the vendor SDK submodules and fetches the signed Streamline runtime, all against eng/dependencies.json.
.DESCRIPTION
Run after `git submodule update --init --depth 1`. Each submodule must sit at its pinned revision, and the
FidelityFX and XeSS DLLs the packages carry must match their pinned SHA-256. The Streamline release zip is
downloaded, required to match its SHA-512, and only the pinned retail DLLs of bin/x64 and the licence files are unpacked into
vendor/streamline-sdk (the packagers' and deploy scripts' default Streamline root); the zip is then deleted.
Finally vendor/runtimes receives the package layout (streamline/, fidelityfx/, xess/) that the Anomaly SR
device fixtures use as RSF_ANOMALY_SR_VENDOR_ROOT, as hard links where the volume allows.
Nothing outside vendor/ is written, and a second run that finds everything in place downloads nothing.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'common.ps1')
$taskPins = (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'dependencies.json') | ConvertFrom-Json).vendor

function Test-PinnedHashes([string]$Directory, $Hashes) {
    foreach ($taskName in $Hashes.PSObject.Properties.Name) {
        $taskFile = Join-Path $Directory $taskName
        if (!(Test-Path -LiteralPath $taskFile -PathType Leaf) -or
            (Get-FileHash -LiteralPath $taskFile -Algorithm SHA256).Hash -ine $Hashes.$taskName) { return $false }
    }
    $true
}

foreach ($taskPin in @($taskPins.PSObject.Properties.Value | Where-Object { $_.PSObject.Properties['path'] })) {
    $taskPath = Join-Path $taskRoot $taskPin.path
    $taskHead = if (Test-Path -LiteralPath (Join-Path $taskPath '.git')) { (& git -C $taskPath rev-parse HEAD) } else { '' }
    if ($taskHead -ne $taskPin.revision) {
        throw "$($taskPin.path) is not at $($taskPin.revision). Run: git submodule update --init --depth 1"
    }
}
foreach ($taskCheck in @(@{dir='vendor/fidelityfx/Kits/FidelityFX/signedbin'; pin=$taskPins.fidelityfx}, @{dir='vendor/xess/bin'; pin=$taskPins.xess})) {
    if (!(Test-PinnedHashes (Join-Path $taskRoot $taskCheck.dir) $taskCheck.pin.sha256)) { throw "A vendor DLL in $($taskCheck.dir) is missing or differs from its pinned SHA-256." }
}

$taskSdk = Join-Path $taskRoot 'vendor/streamline-sdk'
$taskBin = Join-Path $taskSdk 'bin/x64'
$taskLicences = @('license.txt', '3rd-party-licenses.md', 'bin/x64/nvngx_dlss.license.txt', 'bin/x64/reflex.license.txt')
$taskPresent = (Test-PinnedHashes $taskBin $taskPins.streamline.sha256) -and
    !($taskLicences | Where-Object { !(Test-Path -LiteralPath (Join-Path $taskSdk $_)) })
if (!$taskPresent) {
    if (Test-Path -LiteralPath $taskSdk) { Remove-Item -LiteralPath $taskSdk -Recurse -Force }
    $taskZip = Join-Path $taskRoot 'vendor/streamline-sdk.zip'
    try {
        Get-PinnedArchive $taskPins.streamline.url $taskPins.streamline.sha512 $taskZip $taskSdk 'bin/x64/sl.interposer.dll' (@($taskPins.streamline.sha256.PSObject.Properties.Name | ForEach-Object { "bin/x64/$_" }) + $taskLicences)
    } finally { if (Test-Path -LiteralPath $taskZip) { Remove-Item -LiteralPath $taskZip -Force } }
    if (!(Test-PinnedHashes $taskBin $taskPins.streamline.sha256)) { throw "The pinned Streamline zip lacks a pinned runtime DLL." }
}

$taskLayout = Join-Path $taskRoot 'vendor/runtimes'
foreach ($taskEntry in (Get-RsfVendorPayload $taskRoot $taskSdk).GetEnumerator()) {
    $taskTarget = Join-Path $taskLayout $taskEntry.Key
    if (Test-Path -LiteralPath $taskTarget) { Remove-Item -LiteralPath $taskTarget -Force }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskTarget) | Out-Null
    try { New-Item -ItemType HardLink -Path $taskTarget -Target $taskEntry.Value | Out-Null }
    catch { Copy-Item -LiteralPath $taskEntry.Value -Destination $taskTarget }
}
Write-Output "Streamline runtime: $taskSdk"
Write-Output "Package layout (RSF_ANOMALY_SR_VENDOR_ROOT): $taskLayout"
