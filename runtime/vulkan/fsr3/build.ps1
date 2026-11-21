<#
.SYNOPSIS
Compiles the retained FSR3 Vulkan bridge DLL with a static compiler runtime.
.DESCRIPTION
Builds the Rust SR owner (owner.rs) for x86_64-pc-windows-gnu with rustc, then links it with the C++17 bridge through g++ into a sibling temporary DLL. Output is replaced only after both steps succeed. Any failure removes the temporary archive and DLL and throws; the existing target remains.
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
$rustCompiler = (Get-Command rustc.exe -ErrorAction Stop).Source
$target = [IO.Path]::GetFullPath($Output)
$targetDir = Split-Path -Parent $target
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
$temporaryBase = Join-Path $targetDir ('VulkanStoryFsr3-' + [Guid]::NewGuid().ToString('N'))
$temporary = "$temporaryBase.tmp.dll"
$rustArchive = "$temporaryBase.a"
try {
    # The repository root selects the pinned rust-toolchain.toml.
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $rustCompiler --edition=2024 --crate-name rsf_fsr3_bridge_owner --crate-type staticlib `
            --target x86_64-pc-windows-gnu -C opt-level=2 -C panic=abort -D unsafe_op_in_unsafe_fn `
            (Join-Path $PSScriptRoot 'owner.rs') -o $rustArchive
        if ($LASTEXITCODE -ne 0) { throw "FSR3 Rust owner compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    & $compiler -std=c++17 -O2 -shared -static -Wall -Wextra -I $header -I $vulkan `
        (Join-Path $PSScriptRoot 'bridge.cpp') $rustArchive -o $temporary `
        -lbcrypt -lntdll -luserenv -lws2_32 -ldbghelp
    if ($LASTEXITCODE -ne 0) { throw "FSR3 bridge build failed: $LASTEXITCODE" }
    Move-Item -LiteralPath $temporary -Destination $target -Force
    Write-Host "vulkanstory-fsr3: built $target"
}
finally {
    foreach ($artifact in @($temporary, $rustArchive)) {
        if (Test-Path -LiteralPath $artifact) { Remove-Item -LiteralPath $artifact -Force }
    }
}
