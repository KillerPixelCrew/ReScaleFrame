<#
.SYNOPSIS
Compiles the retained XeSS frame-generation Vulkan/DX12 bridge DLL.
.DESCRIPTION
Builds the shared Rust session for x86_64-pc-windows-gnu and links it with the C++20 bridge using g++. Replaces Output only after a successful link.
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
$compiler = (Get-Command g++.exe -ErrorAction Stop).Source
$rustCompiler = (Get-Command rustc.exe -ErrorAction Stop).Source
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
$rustArchive = "$temporaryBase.a"
try {
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $rustCompiler --edition=2024 --crate-name rsf_xess_fg_session --crate-type staticlib `
            --target x86_64-pc-windows-gnu -C opt-level=2 -C panic=abort -D unsafe_op_in_unsafe_fn `
            (Join-Path $PSScriptRoot 'session.rs') -o $rustArchive
        if ($LASTEXITCODE -ne 0) { throw "XeSS-FG Rust session compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    & $compiler -std=c++20 -O2 -shared -static -Wall -Wextra -I $include -I $vulkan `
        (Join-Path $PSScriptRoot 'bridge.cpp') (Join-Path $PSScriptRoot '../../../integrations/vulkanstory/native/xess_fg.def') $rustArchive -o $temporary `
        -ld3d12 -ldxgi -lbcrypt -lntdll -luserenv -lws2_32 -ldbghelp
    if ($LASTEXITCODE -ne 0) { throw "XeSS-FG bridge compilation failed: $LASTEXITCODE" }
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
finally {
    foreach ($artifact in @($temporary, $rustArchive)) {
        if (Test-Path -LiteralPath $artifact) { Remove-Item -LiteralPath $artifact -Force }
    }
}
