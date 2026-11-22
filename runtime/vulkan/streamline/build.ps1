<#
.SYNOPSIS
Compiles the retained Streamline Vulkan bridge with its compiler runtime linked statically.
.DESCRIPTION
Checks Streamline/Vulkan headers, builds the shared Rust interposer lifecycle owner for Windows GNU, then invokes g++ in C++20 mode with wintrust/advapi32 linkage and builds a sibling temporary DLL. Successful compilation replaces Output; failure removes temporary output and throws. SDK plugin binaries are supplied separately by native bundle preparation.
.PARAMETER SdkRoot
Streamline SDK root containing include/sl.h.
.PARAMETER Output
Target bridge DLL path; parent directories are created and successful output replaces the file.
.PARAMETER VulkanSdkRoot
Vulkan SDK root containing Include/vulkan/vulkan.h; defaults to VULKAN_SDK.
#>
param(
    [Parameter(Mandatory = $true)][string]$SdkRoot,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$VulkanSdkRoot = $env:VULKAN_SDK,
    [string]$CorrectedRuntimeDirectory
)

$ErrorActionPreference = 'Stop'
$compiler = (Get-Command g++.exe -ErrorAction Stop).Source
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }

$source = Join-Path $PSScriptRoot 'bridge.cpp'
$wsiExports = Join-Path $PSScriptRoot 'wsi_exports.def'
$hostExports = Join-Path $PSScriptRoot '../../../integrations/vulkanstory/native/sl.def'
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$vulkanSdk = (Resolve-Path -LiteralPath $VulkanSdkRoot).Path
$include = Join-Path $sdk 'include'
$vulkanInclude = Join-Path $vulkanSdk 'Include'
if (-not (Test-Path -LiteralPath (Join-Path $include 'sl.h'))) {
    throw 'Streamline headers are required.'
}
if (-not (Test-Path -LiteralPath (Join-Path $vulkanInclude 'vulkan\vulkan.h'))) {
    throw 'Vulkan SDK headers are required.'
}

$target = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
$temporary = [IO.Path]::ChangeExtension($target, '.tmp.dll')
$correctionArguments = @()
if ($CorrectedRuntimeDirectory) {
    $receipt = Get-Content -LiteralPath (Join-Path $CorrectedRuntimeDirectory 'streamline-sdk-build.json') -Raw | ConvertFrom-Json
    if ($receipt.sdkCommit -ne '2122257e0fce486f91b385aa63b9a09b0a34b363' -or
        $receipt.patchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk-fixes/prepare.py')).Hash -or
        $receipt.selectedDevicePatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk-fixes/selected_device.py')).Hash -or
        $receipt.selectedDeviceConfigSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk-fixes/selected-device-config.h')).Hash -or
        $receipt.dispatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk-fixes/ngx-vulkan-dispatch.h')).Hash) {
        throw 'Corrected Streamline receipt does not match the pinned source corrections.'
    }
    foreach ($name in @('sl.common.dll','sl.interposer.dll')) {
        if ($receipt.binaries.$name -ne (Get-FileHash -LiteralPath (Join-Path $CorrectedRuntimeDirectory $name)).Hash) {
            throw "Corrected Streamline hash mismatch: $name"
        }
    }
    $hash = $receipt.binaries.'sl.interposer.dll'
    $bytes = for ($i = 0; $i -lt 64; $i += 2) { '0x' + $hash.Substring($i, 2) }
    $correctionArguments += '-DVULKANSTORY_CORRECTED_STREAMLINE_SHA256=' + ($bytes -join ',')
}
$objects = Join-Path ([IO.Path]::GetTempPath()) ('VulkanStoryStreamline-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $objects | Out-Null
try {
    # The interposer lifecycle is the same Rust crate the Runtime links.
    $rustTarget = 'x86_64-pc-windows-gnu'
    $rustOutput = Join-Path $objects 'cargo'
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $cargo build --locked --release -p rsf-streamline-vk --target $rustTarget --target-dir $rustOutput
        if ($LASTEXITCODE -ne 0) { throw "Streamline Rust lifecycle compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    $rustArchive = Join-Path $rustOutput "$rustTarget/release/librsf_streamline_vk.a"
    $exports = Join-Path $objects 'exports.def'
    @(Get-Content -LiteralPath $wsiExports
      Get-Content -LiteralPath $hostExports | Select-Object -Skip 1) |
        Set-Content -LiteralPath $exports -Encoding ascii
    & $compiler -std=c++20 -O2 -shared -static -I $include -I $vulkanInclude @correctionArguments $source $exports $rustArchive `
        -o $temporary -lwintrust -ladvapi32 -lntdll -luserenv -lws2_32 -ldbghelp
    if ($LASTEXITCODE -ne 0) { throw 'Streamline bridge compilation failed.' }
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
catch {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw
}
finally {
    if (Test-Path -LiteralPath $objects) { Remove-Item -LiteralPath $objects -Recurse -Force }
}
Write-Host "vulkanstory-streamline: built $target"
