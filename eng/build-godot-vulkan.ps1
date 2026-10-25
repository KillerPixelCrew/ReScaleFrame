<#
.SYNOPSIS
Builds and stages Godot's pinned FidelityFX and Streamline Vulkan runtimes using local tools.
#>
[CmdletBinding()]
param(
    [ValidateRange(1,4)][int]$Parallel = 4,
    [ValidateRange(1,4)][int]$ShaderThreads = 1
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $taskRoot 'integrations/godot/dependencies.json') | ConvertFrom-Json
$taskEngine = Join-Path $taskRoot 'references/godot/engine'
$taskSdk = Join-Path $taskRoot 'references/godot/fidelityfx-vulkan'
foreach ($taskInput in @(@{path=$taskEngine; revision=$taskLock.engine.revision}, @{path=$taskSdk; revision=$taskLock.vendor.fidelityfx_vulkan.revision})) {
    $taskRevision = & git -C $taskInput.path rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $taskRevision -ne $taskInput.revision) { throw 'Run eng/prepare-godot.ps1 to obtain the pinned sources.' }
}
$taskHeaders = Join-Path $taskEngine 'thirdparty/vulkan/include'
$taskHeaderChanges = & git -C $taskEngine status --porcelain -- thirdparty/vulkan/include
if ($LASTEXITCODE -ne 0 -or $taskHeaderChanges) { throw 'Godot Vulkan reference headers must be unchanged.' }
$taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$taskVs = @((& $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json) | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0 -or $taskVs.Count -ne 1) { throw 'Installed Visual Studio x64 C++ tools are required.' }
$taskToolset = Get-ChildItem -LiteralPath (Join-Path $taskVs[0].installationPath 'VC/Tools/MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1
$taskTools = Join-Path $taskToolset.FullName 'bin/Hostx64/x64'
$taskLoader = Join-Path $env:SystemRoot 'System32/vulkan-1.dll'
if (!(Test-Path -LiteralPath $taskLoader)) { throw 'A Vulkan driver and loader must already be installed.' }
$taskRecipe = Join-Path $taskRoot 'runtime/vulkan/fsr3/build-sdk.ps1'
$taskRecipeHash = (Get-FileHash -LiteralPath $taskRecipe).Hash
$taskLocal = Join-Path $taskRoot '.local/godot/vulkan'
$taskDev = Join-Path $taskLocal 'development'
New-Item -ItemType Directory -Force -Path (Join-Path $taskDev 'Include'),(Join-Path $taskDev 'Lib') | Out-Null
# Only headers and a loader import library are needed. No global SDK install or
# global environment change is necessary. Preserve all source notices in Include.
Copy-RsfGodotAddon $taskHeaders (Join-Path $taskDev 'Include')
$taskExports = @(& (Join-Path $taskTools 'dumpbin.exe') /nologo /exports $taskLoader)
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the installed Vulkan loader exports.' }
$taskNames = @($taskExports | ForEach-Object {
    if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(vk[A-Za-z0-9_]+)\s*$') { $Matches[1] }
} | Sort-Object -Unique)
foreach ($taskRequired in @('vkGetInstanceProcAddr','vkGetDeviceProcAddr','vkCreateInstance','vkCreateDevice')) {
    if ($taskNames -notcontains $taskRequired) { throw "Vulkan loader export is missing: $taskRequired" }
}
$taskDefinition = Join-Path $taskDev 'Lib/vulkan-1.def'
[IO.File]::WriteAllText($taskDefinition, "LIBRARY vulkan-1.dll`nEXPORTS`n" + ($taskNames -join "`n") + "`n")
& (Join-Path $taskTools 'lib.exe') /nologo /machine:x64 "/def:$taskDefinition" "/out:$(Join-Path $taskDev 'Lib/vulkan-1.lib')"
if ($LASTEXITCODE -ne 0) { throw 'Could not create the Vulkan loader import library.' }
@{engine_revision=$taskLock.engine.revision; loader=$taskLoader; loader_sha256=(Get-FileHash -LiteralPath $taskLoader).Hash;
    import_library_sha256=(Get-FileHash -LiteralPath (Join-Path $taskDev 'Lib/vulkan-1.lib')).Hash; exports=$taskNames.Count} |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskDev 'inputs.json') -Encoding utf8
$taskHost = Join-Path $taskLocal 'host'
New-Item -ItemType Directory -Force -Path $taskHost | Out-Null
$taskInputHashes = @($taskRecipeHash)
foreach ($taskInput in @('wsi-dispatch.h','sdk-fixes/luma-history-format.json','sdk-fixes/swapchain-lifetime.json','sdk-fixes/fsr2-storage-formats.json','sdk-fixes/descriptor-pool-size.json','sdk-fixes/presenter-cpu-drain.json')) {
    $taskInputHashes += (Get-FileHash -LiteralPath (Join-Path (Split-Path $taskRecipe) $taskInput)).Hash
}
$taskBuildKey = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes(($taskInputHashes -join ':'))))
$taskOutput = Join-Path $taskHost ('artifacts/' + $taskBuildKey.Substring(0,16).ToLowerInvariant())
& $taskRecipe -HostRoot $taskHost -SdkRoot $taskSdk -OutputDirectory $taskOutput -VulkanSdkRoot $taskDev -Parallel $Parallel -ShaderThreads $ShaderThreads
foreach ($taskFile in @('amd_fidelityfx_vk.dll','LICENSE.txt','sdk-LICENSE.txt','fsr3-sdk-build.json')) {
    Copy-RsfGodotArtifact (Join-Path $taskOutput "runtime/$taskFile") (Join-Path $taskLocal "runtime/$taskFile")
}
& (Join-Path $taskRoot 'runtime/vulkan/streamline/build-sdk.ps1') -HostRoot $taskHost `
    -SdkRoot (Join-Path $taskRoot 'references/godot/streamline') `
    -OutputDirectory (Join-Path $taskHost 'artifacts/streamline-production-pcl-wsi') `
    -Python (Join-Path $taskRoot '.local/godot/build-env/Scripts/python.exe')
Write-Output 'Vulkan SR/FG/PCL payloads built. Run eng/build-godot.ps1 -StageRuntime to include them in the add-on.'
