<#
.SYNOPSIS
Compiles the Streamline Vulkan bridge with its compiler runtime linked statically.
.DESCRIPTION
Checks Streamline/Vulkan headers and, with -CorrectedRuntimeDirectory, the corrected runtime's receipt (verify-receipt.cmake), builds the Rust bridge and interposer lifecycle owner for Windows GNU with that digest, then links it with gcc and wintrust/advapi32 into a sibling temporary DLL. Successful compilation replaces Output; failure removes temporary output and throws. SDK plugin binaries are supplied separately by native bundle preparation.
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
$compiler = (Get-Command gcc.exe -ErrorAction Stop).Source
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }

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
$corrected = $null
if ($CorrectedRuntimeDirectory) {
    # The one receipt verifier; it prints the corrected interposer digest that rsf-streamline-vk admits.
    $corrected = & (Get-Command cmake.exe -ErrorAction Stop).Source "-DRUNTIME=$CorrectedRuntimeDirectory" `
        -P (Join-Path $PSScriptRoot 'verify-receipt.cmake')
    if ($LASTEXITCODE -ne 0) { throw 'Corrected Streamline receipt does not match the pinned source corrections.' }
    $corrected = "$corrected".Trim()
}$objects = Join-Path ([IO.Path]::GetTempPath()) ('VulkanStoryStreamline-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $objects | Out-Null
try {
    # The bridge exports and interposer lifecycle are the same Rust crate the Runtime links.
    $rustTarget = 'x86_64-pc-windows-gnu'
    $rustOutput = Join-Path $objects 'cargo'
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    $previousDigest = $env:RSF_CORRECTED_STREAMLINE_SHA256
    $env:RSF_CORRECTED_STREAMLINE_SHA256 = $corrected
    try {
        & $cargo build --locked --release -p rsf-streamline-vk --target $rustTarget --target-dir $rustOutput
        if ($LASTEXITCODE -ne 0) { throw "Streamline Rust lifecycle compilation failed: $LASTEXITCODE" }
    }
    finally {
        $env:RSF_CORRECTED_STREAMLINE_SHA256 = $previousDigest
        Pop-Location
    }
    $rustArchive = Join-Path $rustOutput "$rustTarget/release/librsf_streamline_vk.a"
    $exports = Join-Path $objects 'exports.def'
    @(Get-Content -LiteralPath $wsiExports
      Get-Content -LiteralPath $hostExports | Select-Object -Skip 1) |
        Set-Content -LiteralPath $exports -Encoding ascii
    & $compiler -O2 -shared -static $exports $rustArchive `
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
