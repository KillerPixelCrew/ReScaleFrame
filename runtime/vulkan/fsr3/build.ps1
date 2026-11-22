<#
.SYNOPSIS
Builds the standalone Rust FSR3 Vulkan bridge DLL.
.DESCRIPTION
Builds the rsf-vk-fsr3 archive for x86_64-pc-windows-gnu with Cargo and links it with the
VulkanStory export aliases using gcc. The committed bindings come from the pinned SDK, so the
SDK and Vulkan headers are only checked here. Replaces the target only after successful linking.
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
$vulkan = Join-Path (Resolve-Path -LiteralPath $VulkanSdkRoot).Path 'Include'
if (-not (Test-Path -LiteralPath (Join-Path $header 'ffx_api\ffx_api.h'))) { throw 'FidelityFX SDK v1.1.4 headers are required' }
if (-not (Test-Path -LiteralPath (Join-Path $vulkan 'vulkan\vulkan.h'))) { throw 'Vulkan SDK headers are required' }
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
$compiler = (Get-Command gcc.exe -ErrorAction Stop).Source
$target = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
$temporary = [IO.Path]::ChangeExtension($target, '.tmp.dll')
$objects = Join-Path ([IO.Path]::GetTempPath()) ('VulkanStoryFsr3-' + [Guid]::NewGuid().ToString('N'))
try {
    $rustTarget = 'x86_64-pc-windows-gnu'
    Push-Location -LiteralPath (Join-Path $PSScriptRoot '../../..')
    try {
        & $cargo build --locked --release -p rsf-vk-fsr3 --target $rustTarget --target-dir $objects
        if ($LASTEXITCODE -ne 0) { throw "FSR3 bridge compilation failed: $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
    $archive = Join-Path $objects "$rustTarget/release/librsf_vk_fsr3.a"
    # Every export is defined in the Rust archive. --undefined pulls each one in for the .def file.
    $definition = Join-Path $PSScriptRoot '../../../integrations/vulkanstory/native/fsr3.def'
    $exports = @(Get-Content -LiteralPath $definition | ForEach-Object {
        if ($_ -match '^\s+(?:\w+=)?(\w+)\s*$') { "-Wl,--undefined=$($Matches[1])" }
    })
    & $compiler -O2 -shared -static $exports $definition $archive -o $temporary `
        -lntdll -luserenv -lws2_32 -ldbghelp
    if ($LASTEXITCODE -ne 0) { throw "FSR3 bridge link failed: $LASTEXITCODE" }
    Move-Item -LiteralPath $temporary -Destination $target -Force
    Write-Host "vulkanstory-fsr3: built $target"
}
catch {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw
}
finally {
    if (Test-Path -LiteralPath $objects) { Remove-Item -LiteralPath $objects -Recurse -Force }
}
