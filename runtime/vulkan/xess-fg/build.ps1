<#
.SYNOPSIS
Compiles the retained XeSS frame-generation Vulkan/DX12 bridge DLL.
.DESCRIPTION
Checks XeSS FG/Vulkan headers and invokes g++ in C++20 mode with static compiler runtime and d3d12/dxgi linkage. Builds a sibling temporary DLL and replaces Output only after success; failure removes the temporary and throws.
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
if (-not (Test-Path -LiteralPath (Join-Path $include 'xess_fg/xefg_swapchain_d3d12.h'))) {
    throw 'XeSS 3 SDK frame generation headers are required'
}
if (-not (Test-Path -LiteralPath (Join-Path $vulkan 'vulkan/vulkan.h'))) {
    throw 'Vulkan SDK headers are required for DXGI adapter matching'
}
$target = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
$temporary = [IO.Path]::ChangeExtension($target, '.tmp.dll')
& $compiler -std=c++20 -O2 -shared -static -Wall -Wextra -I $include -I $vulkan `
    (Join-Path $PSScriptRoot 'bridge.cpp') -o $temporary -ld3d12 -ldxgi
if ($LASTEXITCODE -ne 0) {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw "XeSS-FG bridge compilation failed: $LASTEXITCODE"
}
Move-Item -LiteralPath $temporary -Destination $target -Force
