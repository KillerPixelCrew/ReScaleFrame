param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [switch]$VS2026,
    [switch]$StageOnly,
    [switch]$Pack,
    [string]$UnityPluginApiDirectory = 'C:/Program Files/Unity/Hub/Editor/6000.6.6f1/Editor/Data/PluginAPI'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskPackage = Join-Path $taskRoot 'integrations/unity/com.kpc.rescaleframe'
$taskBin = Join-Path $taskRoot "build/windows-x64/bin/$Configuration"
if (!$StageOnly) {
    Push-Location -LiteralPath $taskRoot
    try {
        $taskSuffix = if ($VS2026) { '-vs18' } else { '' }
        & cmake --preset "windows-x64$taskSuffix"
        if ($LASTEXITCODE) { throw 'Unity package configuration failed.' }
        & cmake --build --preset "windows-$($Configuration.ToLowerInvariant())$taskSuffix" --target rsf_unity_package
        if ($LASTEXITCODE) { throw 'Unity native package build failed.' }
    } finally { Pop-Location }
}
$taskNative = Join-Path $taskPackage 'Plugins/x86_64/ReScaleFrame.Unity.Native.dll'
foreach ($taskProcess in Get-Process Unity -ErrorAction SilentlyContinue) {
    foreach ($taskModule in $taskProcess.Modules) {
        if ($taskModule.FileName -eq $taskNative) { throw 'The package DLL is loaded in Unity. Close that editor before staging a replacement.' }
    }
}
$taskPayload = Join-Path $taskPackage 'Runtime~'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskNative),$taskPayload | Out-Null
Copy-Item -LiteralPath (Join-Path $taskBin 'ReScaleFrame.Unity.Native.dll') -Destination $taskNative
$taskCopies = @{}
$taskStreamline = Join-Path $taskRoot '.local/streamline/sdk'
foreach ($taskName in @('sl.interposer.dll','sl.common.dll','sl.dlss.dll','sl.dlss_g.dll','sl.reflex.dll','sl.pcl.dll','nvngx_dlss.dll','nvngx_dlssg.dll','nvngx_dlss.license.txt')) {
    $taskCopies["streamline/$taskName"] = Join-Path $taskStreamline "bin/x64/$taskName"
}
foreach ($taskName in @('license.txt','3rd-party-licenses.md')) { $taskCopies["streamline/$taskName"] = Join-Path $taskStreamline $taskName }
$taskFidelity = Join-Path $taskRoot 'vendor/fidelityfx/Kits/FidelityFX'
foreach ($taskName in @('amd_fidelityfx_loader_dx12.dll','amd_fidelityfx_upscaler_dx12.dll','amd_fidelityfx_framegeneration_dx12.dll')) {
    $taskCopies["fidelityfx/$taskName"] = Join-Path $taskFidelity "signedbin/$taskName"
}
$taskCopies['fidelityfx/license.md'] = Join-Path $taskFidelity 'docs/license.md'
$taskCopies['fidelityfx/3rdpartynotice.md'] = Join-Path $taskRoot 'vendor/fidelityfx/3rdpartynotice.md'
foreach ($taskName in @('libxess.dll','libxess_fg.dll','libxell.dll')) { $taskCopies["xess/$taskName"] = Join-Path $taskRoot "vendor/xess/bin/$taskName" }
foreach ($taskName in @('LICENSE.txt','third-party-programs.txt')) { $taskCopies["xess/$taskName"] = Join-Path $taskRoot "vendor/xess/$taskName" }
$taskCopies['Unity-Companion-LICENSE.md'] = Join-Path $UnityPluginApiDirectory 'LICENSE.md'
$taskCopies['MFG-Unlocker-LICENSE.txt'] = Join-Path $taskRoot 'runtime/backends/dlss/third_party/mfg_unlocker/LICENSE'
foreach ($taskCopy in $taskCopies.GetEnumerator()) {
    if (!(Test-Path -LiteralPath $taskCopy.Value -PathType Leaf)) { throw "Required runtime/license missing: $($taskCopy.Value)" }
    $taskTarget = Join-Path $taskPayload $taskCopy.Key
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskTarget) | Out-Null
    Copy-Item -LiteralPath $taskCopy.Value -Destination $taskTarget
}
if (!(Test-Path -LiteralPath ($taskNative + '.meta'))) { throw 'The checked-in Unity native plugin metadata is missing.' }
foreach ($taskEntry in Get-ChildItem -LiteralPath $taskPackage -Recurse | Where-Object { $_.Extension -ne '.meta' -and $_.FullName -notlike "$taskPayload*" }) {
    $taskMetadata = $taskEntry.FullName + '.meta'
    if (Test-Path -LiteralPath $taskMetadata) { continue }
    $taskRelative = [IO.Path]::GetRelativePath($taskPackage, $taskEntry.FullName).Replace('\','/')
    $taskDigest = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes('com.kpc.rescaleframe/' + $taskRelative))
    $taskGuid = [Convert]::ToHexString($taskDigest).ToLowerInvariant().Substring(0,32)
    $taskLines = @('fileFormatVersion: 2', "guid: $taskGuid")
    if ($taskEntry.PSIsContainer) { $taskLines += @('folderAsset: yes','DefaultImporter:','  externalObjects: {}') }
    elseif ($taskEntry.Extension -eq '.cs') { $taskLines += @('MonoImporter:','  externalObjects: {}','  serializedVersion: 2','  defaultReferences: []','  executionOrder: 0','  icon: {instanceID: 0}') }
    elseif ($taskEntry.Extension -eq '.asmdef') { $taskLines += @('AssemblyDefinitionImporter:','  externalObjects: {}') }
    elseif ($taskEntry.Extension -eq '.shader') { $taskLines += @('ShaderImporter:','  externalObjects: {}','  defaultTextures: []','  nonModifiableTextures: []') }
    else { $taskLines += @('DefaultImporter:','  externalObjects: {}') }
    [IO.File]::WriteAllText($taskMetadata, ($taskLines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
}
Write-Output "Staged Unity package: $taskPackage"
if ($Pack) {
    $taskVersion = (Get-Content -Raw -LiteralPath (Join-Path $taskPackage 'package.json') | ConvertFrom-Json).version
    $taskOutput = Join-Path $taskRoot '.local/packages'
    $taskStaging = Join-Path $taskOutput ('unity-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    New-Item -ItemType Directory -Force -Path $taskStaging | Out-Null
    Copy-Item -LiteralPath $taskPackage -Destination (Join-Path $taskStaging 'package') -Recurse
    $taskArchive = Join-Path $taskOutput "com.kpc.rescaleframe-$taskVersion.tgz"
    & tar -czf $taskArchive -C $taskStaging package
    if ($LASTEXITCODE) { throw 'Unity package archive failed.' }
    $taskHash = (Get-FileHash -LiteralPath $taskArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($taskArchive + '.sha256', "$taskHash  $([IO.Path]::GetFileName($taskArchive))`n", [Text.UTF8Encoding]::new($false))
    Write-Output "UPM archive: $taskArchive"
}
