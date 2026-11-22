<#
.SYNOPSIS
Builds the standalone Rust FSR3 Vulkan bridge DLL.
.DESCRIPTION
Builds the x86_64-pc-windows-gnu DLL with rustc and the checked-in bindings
generated from the pinned SDK. Replaces the target only after successful linking.
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
$rustCompiler = (Get-Command rustc.exe -ErrorAction Stop).Source
$target = [IO.Path]::GetFullPath($Output)
$targetDir = Split-Path -Parent $target
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
$staging = Join-Path $targetDir ('VulkanStoryFsr3-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
$temporary = Join-Path $staging 'VulkanStoryFsr3.dll'
try {
    # The repository root selects the pinned rust-toolchain.toml.
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $rustCompiler --edition=2024 --crate-name rescaleframe_fsr3 --crate-type cdylib `
            --target x86_64-pc-windows-gnu -C opt-level=2 -C panic=abort -D unsafe_op_in_unsafe_fn `
            (Join-Path $PSScriptRoot 'owner.rs') -o $temporary
        if ($LASTEXITCODE -ne 0) { throw "FSR3 bridge build failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    Move-Item -LiteralPath $temporary -Destination $target -Force
    Write-Host "vulkanstory-fsr3: built $target"
}
finally {
    foreach ($artifact in (Get-ChildItem -LiteralPath $staging -File)) {
        Remove-Item -LiteralPath $artifact.FullName -Force
    }
    Remove-Item -LiteralPath $staging -Force
}
