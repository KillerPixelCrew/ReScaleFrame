param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [switch]$VS2026,
    [switch]$Prepare,
    [switch]$StageRuntime,
    [switch]$StageOnly,
    [string]$Fsr3RuntimeDirectory = '',
    [string]$NativeBuildDirectory = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
. (Join-Path $PSScriptRoot 'common.ps1')
$taskSource = Join-Path $taskRoot 'integrations/godot'
$taskBindings = Join-Path $taskRoot 'references/godot/godot-cpp'
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'dependencies.json') | ConvertFrom-Json
if ($Prepare) { & (Join-Path $PSScriptRoot 'prepare-godot.ps1') }
if (!(Test-Path -LiteralPath $taskBindings)) { throw 'Run eng/prepare-godot.ps1 first, or pass -Prepare.' }
$taskRevision = & git -C $taskBindings rev-parse HEAD
if ($LASTEXITCODE -or $taskRevision -ne $taskLock.bindings.revision) { throw 'godot-cpp differs from dependencies.json.' }
$taskDirty = & git -C $taskBindings status --porcelain
if ($LASTEXITCODE -or $taskDirty) { throw 'The godot-cpp reference must be clean.' }
$taskSuffix = if ($VS2026) { '-vs18' } else { '' }
$taskBuild = if ($NativeBuildDirectory) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($NativeBuildDirectory)
} else {
    Join-Path $taskRoot 'build/windows-x64'
}
if (!$StageOnly) {
    Push-Location -LiteralPath $taskRoot
    try {
        & cmake --preset "windows-x64$taskSuffix" -B $taskBuild "-DRSF_GODOT_NATIVE=ON""-DPython3_EXECUTABLE=$taskRoot/.local/godot/build-env/Scripts/python.exe" "-DRSF_STREAMLINE_VK_RUNTIME=$taskRoot/.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime"
        if ($LASTEXITCODE) { throw 'Godot configuration failed.' }
        & cmake --build $taskBuild --config $Configuration --target rsf_godot
        if ($LASTEXITCODE) { throw 'Godot native build failed.' }
    } finally { Pop-Location }
}
$taskNative = Join-Path $taskBuild "bin/$Configuration/ReScaleFrame.Godot.Native.dll"
if (!(Test-Path -LiteralPath $taskNative -PathType Leaf)) { throw "Godot native binary missing: $taskNative" }
$taskPackage = Join-Path $taskRoot '.local/godot/package/addons/rescaleframe'
New-Item -ItemType Directory -Force -Path $taskPackage | Out-Null
Copy-RsfGodotAddon (Join-Path $taskSource 'addons/rescaleframe') $taskPackage
New-Item -ItemType Directory -Force -Path (Join-Path $taskPackage 'bin') | Out-Null
Copy-RsfGodotArtifact $taskNative (Join-Path $taskPackage 'bin/ReScaleFrame.Godot.Native.dll')
Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination (Join-Path $taskPackage 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $taskBindings 'LICENSE.md') -Destination (Join-Path $taskPackage 'godot-cpp-LICENSE.md') -Force
Copy-RsfGodotArtifact (Join-Path $taskRoot 'references/godot/engine/LICENSE.txt') (Join-Path $taskPackage 'Godot-LICENSE.txt')
Copy-RsfGodotArtifact (Join-Path $taskRoot 'references/godot/engine/COPYRIGHT.txt') (Join-Path $taskPackage 'Godot-COPYRIGHT.txt')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'dependencies.json') -Destination $taskPackage -Force
if ($StageRuntime) {
    $taskRuntime = Join-Path $taskPackage 'runtime'
    $taskPayload = Get-RsfVendorPayload $taskRoot
    $taskSlSdk = Join-Path $taskRoot 'vendor/streamline-sdk'
    $taskPayload['streamline/reflex.license.txt'] = Join-Path $taskSlSdk 'bin/x64/reflex.license.txt'
    $taskSlVulkan = Join-Path $taskRoot '.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime'
    if (Test-Path -LiteralPath (Join-Path $taskSlVulkan 'streamline-sdk-build.json')) {
        # The one receipt verifier (also used by the Runtime build and the standalone bridge script).
        & (Get-Command cmake.exe -ErrorAction Stop).Source "-DRUNTIME=$taskSlVulkan" -P (Join-Path $taskRoot 'runtime/vulkan/streamline/verify-receipt.cmake') | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Corrected Streamline receipt differs from pinned source corrections.' }
        # The Vulkan Streamline set is the same SDK payload, except that the corrected build supplies the interposer and common DLL.
        foreach ($taskEntry in @($taskPayload.GetEnumerator() | Where-Object { $_.Key -like 'streamline/*' })) {
            $taskPayload['streamline-vulkan/' + $taskEntry.Key.Substring('streamline/'.Length)] = $taskEntry.Value
        }
        foreach ($taskFile in @('sl.interposer.dll','sl.common.dll','streamline-sdk-build.json')) {
            $taskPayload["streamline-vulkan/$taskFile"] = Join-Path $taskSlVulkan $taskFile
        }
    }
    $taskVulkan = if ($Fsr3RuntimeDirectory) { (Resolve-Path -LiteralPath $Fsr3RuntimeDirectory).Path } else { Join-Path $taskRoot '.local/godot/vulkan/runtime' }
    if (Test-Path -LiteralPath (Join-Path $taskVulkan 'amd_fidelityfx_vk.dll')) {
        $taskReceipt = Get-Content -Raw -LiteralPath (Join-Path $taskVulkan 'fsr3-sdk-build.json') | ConvertFrom-Json
        . (Join-Path $taskRoot 'runtime/vulkan/fsr3/sdk-fixes/line-width.ps1')
        Assert-RsfFfxLineWidthReceipt $taskReceipt
        $taskFixes = 'runtime/vulkan/fsr3/sdk-fixes'
        $taskPinned = [ordered]@{
            buildRecipeSha256 = 'runtime/vulkan/fsr3/build-sdk.ps1'
            swapchainLifetimePatchSha256 = "$taskFixes/swapchain-lifetime.json"
            wsiDispatchHeaderSha256 = 'runtime/vulkan/fsr3/wsi-dispatch.h'
            fsr2StoragePatchSha256 = "$taskFixes/fsr2-storage-formats.json"
            descriptorPoolPatchSha256 = "$taskFixes/descriptor-pool-size.json"
            presenterCpuDrainPatchSha256 = "$taskFixes/presenter-cpu-drain.json"
            swapchainTracePatchSha256 = "$taskFixes/swapchain-trace.json"
            swapchainTraceHeaderSha256 = "$taskFixes/swapchain-trace.h"
        }
        $taskMatches = $taskReceipt.sdkCommit -eq $taskLock.vendor.fidelityfx_vulkan.revision -and
            $taskReceipt.runtimeSha256 -eq (Get-FileHash -LiteralPath (Join-Path $taskVulkan 'amd_fidelityfx_vk.dll')).Hash
        foreach ($taskField in $taskPinned.Keys) {
            $taskMatches = $taskMatches -and $taskReceipt.$taskField -eq (Get-FileHash -LiteralPath (Join-Path $taskRoot $taskPinned[$taskField])).Hash
        }
        if (!$taskMatches) { throw 'The Vulkan FidelityFX runtime does not match its pinned build receipt.' }
        foreach ($taskFile in @('amd_fidelityfx_vk.dll','LICENSE.txt','sdk-LICENSE.txt','fsr3-sdk-build.json')) {
            $taskPayload["fidelityfx-vulkan/$taskFile"] = Join-Path $taskVulkan $taskFile
        }
    }
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
