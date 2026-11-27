<#
.SYNOPSIS
Compiles the retained XeSS frame-generation Vulkan/DX12 bridge DLL.
.DESCRIPTION
Builds the rsf-vk-xess-fg archive for x86_64-pc-windows-gnu with Cargo and links it with the C11 bridge leaves using gcc. Replaces Output only after a successful link.
.PARAMETER SdkRoot
XeSS SDK root containing inc/xess_fg/xefg_swapchain_d3d12.h.
.PARAMETER Output
Target bridge DLL path; parent directories are created and successful output replaces this file.
.PARAMETER VulkanSdkRoot
Vulkan SDK root supplying Include headers for adapter matching; defaults to VULKAN_SDK.
#>
param(
    [Parameter(Mandatory = $true)][string]$SdkRoot,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$VulkanSdkRoot = $env:VULKAN_SDK
)
$ErrorActionPreference = 'Stop'
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$include = Join-Path $sdk 'inc'
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }
$vulkan = Join-Path (Resolve-Path -LiteralPath $VulkanSdkRoot).Path 'Include'
$compiler = (Get-Command gcc.exe -ErrorAction Stop).Source
if (-not (Test-Path -LiteralPath (Join-Path $include 'xess_fg/xefg_swapchain_d3d12.h'))) {
    throw 'XeSS 3 SDK frame generation headers are required'
}
if (-not (Test-Path -LiteralPath (Join-Path $vulkan 'vulkan/vulkan.h'))) {
    throw 'Vulkan SDK headers are required for DXGI adapter matching'
}
$target = [IO.Path]::GetFullPath($Output)
$targetDirectory = Split-Path $target
New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
$temporaryBase = Join-Path $targetDirectory ('VulkanStoryXessFg-' + [Guid]::NewGuid().ToString('N'))
$temporary = "$temporaryBase.tmp.dll"
$cargoTarget = "$temporaryBase-cargo"
try {
    $rustTarget = 'x86_64-pc-windows-gnu'
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        # rsf-game-sys generates the public SDK bindings at build time (libclang, see docs/tooling.md).
        & (Get-Command cargo.exe -ErrorAction Stop).Source build --locked --release -p rsf-vk-xess-fg `
            --target $rustTarget --target-dir $cargoTarget
        if ($LASTEXITCODE -ne 0) { throw "XeSS-FG Rust session compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    $rustArchive = Join-Path $cargoTarget "$rustTarget/release/librsf_vk_xess_fg.a"
    & $compiler -std=c11 -O2 -shared -static -Wall -Wextra -I $include -I $vulkan `
        (Join-Path $PSScriptRoot 'bridge.c') (Join-Path $PSScriptRoot '../../../integrations/vulkanstory/native/xess_fg.def') $rustArchive -o $temporary `
        -ld3d12 -ldxgi -ldxguid -lbcrypt -lntdll -luserenv -lws2_32 -ldbghelp
    if ($LASTEXITCODE -ne 0) { throw "XeSS-FG bridge compilation failed: $LASTEXITCODE" }
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    if (Test-Path -LiteralPath $cargoTarget) { Remove-Item -LiteralPath $cargoTarget -Recurse -Force }
}