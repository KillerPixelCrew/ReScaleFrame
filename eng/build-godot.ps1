param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [switch]$VS2026,
    [switch]$Prepare,
    [switch]$StageRuntime,
    [switch]$StageOnly,
    [string]$Fsr3RuntimeDirectory = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskSource = Join-Path $taskRoot 'integrations/godot'
$taskBindings = Join-Path $taskRoot 'references/godot/godot-cpp'
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $taskSource 'dependencies.json') | ConvertFrom-Json
if ($Prepare) { & (Join-Path $PSScriptRoot 'prepare-godot.ps1') }
if (!(Test-Path -LiteralPath $taskBindings)) { throw 'Run eng/prepare-godot.ps1 first, or pass -Prepare.' }
$taskRevision = & git -C $taskBindings rev-parse HEAD
if ($LASTEXITCODE -or $taskRevision -ne $taskLock.bindings.revision) { throw 'godot-cpp differs from dependencies.json.' }
$taskDirty = & git -C $taskBindings status --porcelain
if ($LASTEXITCODE -or $taskDirty) { throw 'The godot-cpp reference must be clean.' }
$taskSuffix = if ($VS2026) { '-vs18' } else { '' }
if (!$StageOnly) {
    Push-Location -LiteralPath $taskRoot
    try {
        & cmake --preset "windows-x64$taskSuffix" "-DRSF_GODOT_CPP_DIR=$taskBindings" "-DPython3_EXECUTABLE=$taskRoot/.local/godot/build-env/Scripts/python.exe" "-DRSF_STREAMLINE_VK_RUNTIME=$taskRoot/.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime"
        if ($LASTEXITCODE) { throw 'Godot configuration failed.' }
        & cmake --build --preset "windows-$($Configuration.ToLowerInvariant())$taskSuffix" --target rsf_godot
        if ($LASTEXITCODE) { throw 'Godot native build failed.' }
    } finally { Pop-Location }
}
$taskPackage = Join-Path $taskRoot '.local/godot/package/addons/rescaleframe'
New-Item -ItemType Directory -Force -Path $taskPackage | Out-Null
Copy-RsfGodotAddon (Join-Path $taskSource 'addons/rescaleframe') $taskPackage
New-Item -ItemType Directory -Force -Path (Join-Path $taskPackage 'bin') | Out-Null
Copy-RsfGodotArtifact (Join-Path $taskRoot "build/windows-x64/bin/$Configuration/ReScaleFrame.Godot.Native.dll") (Join-Path $taskPackage 'bin/ReScaleFrame.Godot.Native.dll')
Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination (Join-Path $taskPackage 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $taskBindings 'LICENSE.md') -Destination (Join-Path $taskPackage 'godot-cpp-LICENSE.md') -Force
Copy-RsfGodotArtifact (Join-Path $taskRoot 'references/godot/engine/LICENSE.txt') (Join-Path $taskPackage 'Godot-LICENSE.txt')
Copy-RsfGodotArtifact (Join-Path $taskRoot 'references/godot/engine/COPYRIGHT.txt') (Join-Path $taskPackage 'Godot-COPYRIGHT.txt')
Copy-Item -LiteralPath (Join-Path $taskSource 'dependencies.json') -Destination $taskPackage -Force
if ($StageRuntime) {
    $taskRuntime = Join-Path $taskPackage 'runtime'
    $taskPayload = @{}
    foreach ($taskFile in @('sl.interposer.dll','sl.common.dll','sl.dlss.dll','sl.dlss_g.dll','sl.reflex.dll','sl.pcl.dll','nvngx_dlss.dll','nvngx_dlssg.dll','nvngx_dlss.license.txt','reflex.license.txt')) {
        $taskPayload["streamline/$taskFile"] = Join-Path $taskRoot ".local/streamline/sdk/bin/x64/$taskFile"
    }
    foreach ($taskFile in @('license.txt','3rd-party-licenses.md')) {
        $taskPayload["streamline/$taskFile"] = Join-Path $taskRoot ".local/streamline/sdk/$taskFile"
    }
    $taskSlVulkan = Join-Path $taskRoot '.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime'
    if (Test-Path -LiteralPath (Join-Path $taskSlVulkan 'streamline-sdk-build.json')) {
        $taskSlReceipt = Get-Content -Raw -LiteralPath (Join-Path $taskSlVulkan 'streamline-sdk-build.json') | ConvertFrom-Json
        if ($taskSlReceipt.sdkCommit -ne $taskLock.vendor.streamline_source.revision -or
            $taskSlReceipt.recipeSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/build-sdk.ps1')).Hash -or
            $taskSlReceipt.presentCaptureSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/present-capture.h')).Hash -or
            $taskSlReceipt.patchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/sdk-fixes/prepare.py')).Hash -or
            $taskSlReceipt.selectedDevicePatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/sdk-fixes/selected_device.py')).Hash -or
            $taskSlReceipt.selectedDeviceConfigSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/sdk-fixes/selected-device-config.h')).Hash -or
            $taskSlReceipt.dispatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h')).Hash) { throw 'Corrected Streamline receipt differs from pinned source corrections.' }
        foreach ($taskFile in @('sl.interposer.dll','sl.common.dll')) {
            if ($taskSlReceipt.binaries.$taskFile -ne (Get-FileHash -LiteralPath (Join-Path $taskSlVulkan $taskFile)).Hash) { throw "Corrected Streamline hash mismatch: $taskFile" }
            $taskPayload["streamline-vulkan/$taskFile"] = Join-Path $taskSlVulkan $taskFile
        }
        foreach ($taskFile in @('sl.reflex.dll','sl.pcl.dll','sl.dlss.dll','sl.dlss_g.dll','nvngx_dlss.dll','nvngx_dlssg.dll','nvngx_dlss.license.txt')) { $taskPayload["streamline-vulkan/$taskFile"] = Join-Path $taskRoot ".local/streamline/sdk/bin/x64/$taskFile" }
        foreach ($taskFile in @('license.txt','3rd-party-licenses.md')) { $taskPayload["streamline-vulkan/$taskFile"] = Join-Path $taskRoot ".local/streamline/sdk/$taskFile" }
        $taskPayload['streamline-vulkan/reflex.license.txt'] = Join-Path $taskRoot '.local/streamline/sdk/bin/x64/reflex.license.txt'
        $taskPayload['streamline-vulkan/streamline-sdk-build.json'] = Join-Path $taskSlVulkan 'streamline-sdk-build.json'
    }
    foreach ($taskFile in @('amd_fidelityfx_loader_dx12.dll','amd_fidelityfx_upscaler_dx12.dll','amd_fidelityfx_framegeneration_dx12.dll')) {
        $taskPayload["fidelityfx/$taskFile"] = Join-Path $taskRoot "vendor/fidelityfx/Kits/FidelityFX/signedbin/$taskFile"
    }
    $taskPayload['fidelityfx/license.md'] = Join-Path $taskRoot 'vendor/fidelityfx/Kits/FidelityFX/docs/license.md'
    $taskPayload['fidelityfx/3rdpartynotice.md'] = Join-Path $taskRoot 'vendor/fidelityfx/3rdpartynotice.md'
    foreach ($taskFile in @('libxess.dll','libxess_fg.dll','libxell.dll')) {
        $taskPayload["xess/$taskFile"] = Join-Path $taskRoot "vendor/xess/bin/$taskFile"
    }
    foreach ($taskFile in @('LICENSE.txt','third-party-programs.txt')) {
        $taskPayload["xess/$taskFile"] = Join-Path $taskRoot "vendor/xess/$taskFile"
    }
    $taskVulkan = if ($Fsr3RuntimeDirectory) { (Resolve-Path -LiteralPath $Fsr3RuntimeDirectory).Path } else { Join-Path $taskRoot '.local/godot/vulkan/runtime' }
    if (Test-Path -LiteralPath (Join-Path $taskVulkan 'amd_fidelityfx_vk.dll')) {
        $taskReceipt = Get-Content -Raw -LiteralPath (Join-Path $taskVulkan 'fsr3-sdk-build.json') | ConvertFrom-Json
        if ($taskReceipt.sdkCommit -ne $taskLock.vendor.fidelityfx_vulkan.revision -or
            $taskReceipt.buildRecipeSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/build-sdk.ps1')).Hash -or
            $taskReceipt.swapchainLifetimePatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/swapchain-lifetime.json')).Hash -or
            $taskReceipt.wsiDispatchHeaderSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/wsi-dispatch.h')).Hash -or
            $taskReceipt.fsr2StoragePatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/fsr2-storage-formats.json')).Hash -or
            $taskReceipt.descriptorPoolPatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/descriptor-pool-size.json')).Hash -or
            $taskReceipt.presenterCpuDrainPatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/presenter-cpu-drain.json')).Hash -or
            $taskReceipt.swapchainTracePatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.json')).Hash -or
            $taskReceipt.swapchainTraceHeaderSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.h')).Hash -or
            $taskReceipt.runtimeSha256 -ne (Get-FileHash -LiteralPath (Join-Path $taskVulkan 'amd_fidelityfx_vk.dll')).Hash) {
            throw 'The Vulkan FidelityFX runtime does not match its pinned build receipt.'
        }
        foreach ($taskFile in @('amd_fidelityfx_vk.dll','LICENSE.txt','sdk-LICENSE.txt','fsr3-sdk-build.json')) {
            $taskPayload["fidelityfx-vulkan/$taskFile"] = Join-Path $taskVulkan $taskFile
        }
    }
    $taskPayload['MFG-Unlocker-LICENSE.txt'] = Join-Path $taskRoot 'runtime/backends/dlss/third_party/mfg_unlocker/LICENSE'
    $taskManifest = @()
    foreach ($taskEntry in $taskPayload.GetEnumerator()) {
        if (!(Test-Path -LiteralPath $taskEntry.Value)) { throw "Required runtime or notice missing: $($taskEntry.Value)" }
        $taskTarget = Join-Path $taskRuntime $taskEntry.Key
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskTarget) | Out-Null
        Copy-RsfGodotArtifact $taskEntry.Value $taskTarget
        $taskManifest += @{path=('runtime/' + $taskEntry.Key); sha256=(Get-FileHash -LiteralPath $taskTarget -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    @{schema_version=1; files=@($taskManifest | Sort-Object { $_.path })} | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath (Join-Path $taskPackage 'runtime-manifest.json') -Encoding utf8
}
Write-Output "Staged Godot add-on: $taskPackage"
