<#
.SYNOPSIS
Compiles the retained FSR3 Vulkan bridge DLL with a static compiler runtime.
.DESCRIPTION
Checks SDK/Vulkan headers, invokes g++ in C++17 mode to a sibling temporary DLL, and replaces Output only after compilation succeeds. Compilation failure removes the temporary output and throws; the existing target remains.
.PARAMETER SdkRoot
FidelityFX SDK v1.1.4 root containing ffx-api/include/ffx_api/ffx_api.h.
.PARAMETER Output
Target bridge DLL path; its parent is created and a successful build replaces the target.
.PARAMETER VulkanSdkRoot
Vulkan SDK root supplying Include headers; defaults to VULKAN_SDK.
#>
param(
    [Parameter(Mandatory)][string]$SdkRoot,
    [Parameter(Mandatory)][string]$Output,
    [string]$VulkanSdkRoot = $env:VULKAN_SDK
)
$ErrorActionPreference = 'Stop'
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$header = Join-Path $sdk 'ffx-api\include'
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }
$vulkanRoot = (Resolve-Path -LiteralPath $VulkanSdkRoot).Path
$vulkan = Join-Path $vulkanRoot 'Include'
if (-not (Test-Path -LiteralPath (Join-Path $header 'ffx_api\ffx_api.h'))) { throw 'FidelityFX SDK v1.1.4 headers are required' }
if (-not (Test-Path -LiteralPath (Join-Path $vulkan 'vulkan\vulkan.h'))) { throw 'Vulkan SDK headers are required' }
$compiler = (Get-Command g++.exe -ErrorAction Stop).Source
$target = [IO.Path]::GetFullPath($Output)
$temporary = "$target.tmp.dll"
$targetDir = Split-Path -Parent $target
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
& $compiler -std=c++17 -O2 -shared -static -Wall -Wextra -I $header -I $vulkan (Join-Path $PSScriptRoot 'bridge.cpp') -o $temporary
if ($LASTEXITCODE -ne 0) {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw "FSR3 bridge build failed: $LASTEXITCODE"
}
Move-Item -LiteralPath $temporary -Destination $target -Force
Write-Host "vulkanstory-fsr3: built $target"
