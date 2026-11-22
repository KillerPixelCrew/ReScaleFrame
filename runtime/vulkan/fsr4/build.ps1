<#
.SYNOPSIS
Compiles the retained FSR4 Vulkan/DX12 bridge DLL.
.DESCRIPTION
Builds the INT8, bridge owner and file hashing Rust archives for Windows GNU, compiles MinHook with gcc, and links them with the Rust bridge exports through the g++ driver. Builds a sibling temporary DLL, replacing Output only after success.
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
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
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
    $rustTarget = 'x86_64-pc-windows-gnu'
    $rustOutput = Join-Path $objects 'cargo'
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $cargo build --locked --release -p rsf-fsr4-compat -p rsf-runtime-common --target $rustTarget --target-dir $rustOutput
        if ($LASTEXITCODE -ne 0) { throw "FSR 4 Rust compatibility compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    $rustArchive = Join-Path $rustOutput "$rustTarget/release/librsf_fsr4_compat.a"
    $commonArchive = Join-Path $rustOutput "$rustTarget/release/librsf_runtime_common.a"

    # hde32.c compiles to an empty object on x64; MinHook stays C, not C++.
    $minHookObjects = @($minHookSources | ForEach-Object {
        Join-Path $objects ([IO.Path]::GetFileNameWithoutExtension($_) + '.o')
    })
    for ($index = 0; $index -lt $minHookSources.Count; $index++) {
        & $cCompiler -O2 -c -I (Join-Path $minHook 'include') $minHookSources[$index] -o $minHookObjects[$index]
        if ($LASTEXITCODE -ne 0) { throw "MinHook compilation failed ($($minHookSources[$index])): $LASTEXITCODE" }
    }
    # Every export is defined in the Rust archive. --undefined pulls each one in for the .def file.
    $definition = Join-Path $PSScriptRoot '../../../integrations/vulkanstory/native/fsr4.def'
    $exports = @(Get-Content -LiteralPath $definition | ForEach-Object {
        if ($_ -match '^\s+(?:\w+=)?(\w+)\s*$') { "-Wl,--undefined=$($Matches[1])" }
    })
    & $compiler -O2 -shared -static $minHookObjects $exports $definition `
        $rustArchive $commonArchive -o $temporary -ld3d12 -ldxgi -lbcrypt -lntdll -luserenv -lws2_32 -ldbghelp
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
