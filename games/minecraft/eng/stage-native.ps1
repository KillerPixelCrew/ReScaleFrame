param(
    [string]$SdkWorkspace = (Join-Path $PSScriptRoot '../../..'),
    [ValidateSet('Release','Debug')][string]$Configuration='Release'
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$project=Split-Path -Parent $PSScriptRoot
$repo=(Resolve-Path -LiteralPath (Join-Path $project '../..')).Path
$sdk=(Resolve-Path -LiteralPath $SdkWorkspace).Path
$stage=Join-Path $project 'build/native-resources/native/windows-x64'
$copies=[Collections.Generic.List[object]]::new()
function Add-Payload([string]$Source,[string]$Relative) {
    if(!(Test-Path -LiteralPath $Source -PathType Leaf)) {throw "Missing native dependency: $Source"}
    $copies.Add(@{source=(Resolve-Path -LiteralPath $Source).Path;relative=$Relative})
}
Add-Payload "$repo/build/windows-x64/bin/$Configuration/ReScaleFrame.Minecraft.dll" 'ReScaleFrame.Minecraft.dll'
$sl="$sdk/.local/streamline/sdk"
foreach($leaf in @('sl.interposer.dll','sl.common.dll','sl.dlss.dll','sl.dlss_g.dll','sl.reflex.dll','sl.pcl.dll','nvngx_dlss.dll','nvngx_dlssg.dll','nvngx_dlss.license.txt','reflex.license.txt')) {
    Add-Payload "$sl/bin/x64/$leaf" "streamline/$leaf"
}
foreach($leaf in @('license.txt','3rd-party-licenses.md')) {Add-Payload "$sl/$leaf" "streamline/$leaf"}
$ffx="$sdk/vendor/fidelityfx/Kits/FidelityFX"
foreach($leaf in @('amd_fidelityfx_loader_dx12.dll','amd_fidelityfx_upscaler_dx12.dll','amd_fidelityfx_framegeneration_dx12.dll')) {
    Add-Payload "$ffx/signedbin/$leaf" "fidelityfx/$leaf"
}
Add-Payload "$ffx/docs/license.md" 'fidelityfx/LICENSE.md'
Add-Payload "$sdk/vendor/fidelityfx/3rdpartynotice.md" 'fidelityfx/3rdpartynotice.md'
foreach($leaf in @('libxess.dll','libxess_fg.dll','libxell.dll')) {Add-Payload "$sdk/vendor/xess/bin/$leaf" "xess/$leaf"}
foreach($leaf in @('LICENSE.txt','third-party-programs.txt')) {Add-Payload "$sdk/vendor/xess/$leaf" "xess/$leaf"}
Add-Payload "$repo/LICENSE" 'ReScaleFrame-LICENSE.txt'
Add-Payload "$repo/runtime/backends/dlss/third_party/mfg_unlocker/LICENSE" 'MFG-Unlocker-LICENSE.txt'
Add-Payload "$repo/build/windows-x64/_deps/minhook-src/LICENSE.txt" 'MinHook-LICENSE.txt'
$manifest=[Collections.Generic.List[string]]::new()
foreach($copy in $copies) {
    $destination=Join-Path $stage $copy.relative
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $copy.source -Destination $destination -Force
    $hash=(Get-FileHash -LiteralPath $copy.source -Algorithm SHA256).Hash.ToLowerInvariant()
    if((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash) {throw "Native copy mismatch: $destination"}
    $manifest.Add("$hash  $($copy.relative)")
}
[IO.File]::WriteAllLines((Join-Path $stage 'files.sha256'),($manifest | Sort-Object),[Text.UTF8Encoding]::new($false))
Write-Output "Native payload staged: $stage ($($copies.Count) files)"
