<#
.SYNOPSIS
Compiles the retained FSR4 Vulkan/DX12 bridge DLL.
.DESCRIPTION
Checks FidelityFX DX12/Vulkan/MinHook headers, compiles the MinHook C sources with gcc into a temporary object directory, and links them with the C++20 bridge sources through g++ with static compiler runtime and d3d12/dxgi/bcrypt linkage. Builds a sibling temporary DLL, replacing Output only after success; compilation failure removes the temporary outputs and throws.
.PARAMETER SdkRoot
FidelityFX SDK root containing Kits/FidelityFX/api/include/dx12/ffx_api_dx12.h.
.PARAMETER Output
Target bridge DLL path; a successful build replaces this file.
.PARAMETER VulkanSdkRoot
Vulkan SDK root supplying Include headers for adapter matching; defaults to VULKAN_SDK.
.PARAMETER MinHookRoot
MinHook 1.3.4 source root containing include/MinHook.h and src/; defaults to the sdk/minhook submodule.
#>
param(
    [Parameter(Mandatory = $true)][string]$SdkRoot,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$VulkanSdkRoot = $env:VULKAN_SDK,
    [Parameter(Mandatory)][string]$MinHookRoot
)
$ErrorActionPreference = 'Stop'
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }
$vulkan = Join-Path (Resolve-Path -LiteralPath $VulkanSdkRoot).Path 'Include'
$compiler = (Get-Command g++.exe -ErrorAction Stop).Source
$cCompiler = (Get-Command gcc.exe -ErrorAction Stop).Source
if (-not (Test-Path -LiteralPath (Join-Path $sdk 'Kits/FidelityFX/api/include/dx12/ffx_api_dx12.h'))) {
    throw 'FidelityFX SDK DX12 API headers are required'
}
if (-not (Test-Path -LiteralPath (Join-Path $vulkan 'vulkan/vulkan.h'))) {
    throw 'Vulkan SDK headers are required for DXGI adapter matching'
}
if (-not (Test-Path -LiteralPath (Join-Path $MinHookRoot 'include/MinHook.h') -PathType Leaf)) {
    throw "MinHook sources are required at $MinHookRoot (run 'git submodule update --init sdk/minhook')"
}
$minHook = (Resolve-Path -LiteralPath $MinHookRoot).Path
$minHookSources = @(@('src/buffer.c', 'src/hook.c', 'src/trampoline.c', 'src/hde/hde32.c', 'src/hde/hde64.c') |
    ForEach-Object { Join-Path $minHook $_ })
foreach ($source in $minHookSources) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing MinHook source: $source" }
}
$target = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
$temporary = [IO.Path]::ChangeExtension($target, '.tmp.dll')
$objects = Join-Path ([IO.Path]::GetTempPath()) ('VulkanStoryFsr4-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $objects | Out-Null
try {
    # hde32.c compiles to an empty object on x64; MinHook stays C, not C++.
    $minHookObjects = @($minHookSources | ForEach-Object {
        Join-Path $objects ([IO.Path]::GetFileNameWithoutExtension($_) + '.o')
    })
    for ($index = 0; $index -lt $minHookSources.Count; $index++) {
        & $cCompiler -O2 -c -I (Join-Path $minHook 'include') $minHookSources[$index] -o $minHookObjects[$index]
        if ($LASTEXITCODE -ne 0) { throw "MinHook compilation failed ($($minHookSources[$index])): $LASTEXITCODE" }
    }
    & $compiler -std=c++20 -O2 -shared -static -Wall -Wextra -I (Join-Path $sdk 'Kits/FidelityFX') -I $vulkan `
        -I (Join-Path $minHook 'include') -I (Join-Path $PSScriptRoot '../../backends/fsr/src') -I (Join-Path $PSScriptRoot '../../common/include') `
        (Join-Path $PSScriptRoot 'bridge.cpp') (Join-Path $PSScriptRoot '../../backends/fsr/src/fsr4_compat_core.cpp') $minHookObjects `
        -o $temporary -ld3d12 -ldxgi -lbcrypt
    if ($LASTEXITCODE -ne 0) { throw "FSR 4 bridge compilation failed: $LASTEXITCODE" }
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
catch {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw
}
finally {
    if (Test-Path -LiteralPath $objects) { Remove-Item -LiteralPath $objects -Recurse -Force }
}
