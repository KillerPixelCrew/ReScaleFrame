<#
.SYNOPSIS
Builds the pinned FidelityFX Vulkan runtime with its luma-history storage format corrected.
.DESCRIPTION
Copies tracked sdk/ and ffx-api/ sources into a fresh artifacts directory, applies the
guarded upstream issue 161 correction, and builds the SDK's own API target and shaders.
The vendor submodule remains unchanged. Output contains amd_fidelityfx_vk.dll and a
hash receipt required by prepare-native-bundle.ps1's Fsr3RuntimeDirectory override.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$HostRoot,
    [string]$OutputDirectory = (Join-Path $HostRoot 'artifacts/rescaleframe/fsr3-sdk/Release/win-x64'),
    [string]$VulkanSdkRoot = $env:VULKAN_SDK,
    [ValidateSet('Release','RelWithDebInfo')][string]$Configuration = 'Release',
    [ValidateRange(1,4)][int]$Parallel = 4,
    [ValidateRange(1,4)][int]$ShaderThreads = 1
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath $HostRoot).Path
$sdkRoot = Join-Path $projectRoot 'sdk/fidelityfx-vk'
$artifactsRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'artifacts'))
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (-not $output.StartsWith($artifactsRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The SDK build output must be a fresh directory inside this repository artifacts/.'
}
$fixPath = Join-Path $PSScriptRoot 'sdk-fixes/luma-history-format.json'
$fix = Get-Content -LiteralPath $fixPath -Raw | ConvertFrom-Json
$commit = (& git -C $sdkRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $commit -ne $fix.sdkCommit) { throw 'The FSR3 correction requires the pinned FidelityFX SDK commit.' }
$dirty = & git -C $sdkRoot status --porcelain --untracked-files=no
if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'FidelityFX SDK tracked sources must be unchanged before staging.' }
$recipeHash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
$fixHash = (Get-FileHash -LiteralPath $fixPath -Algorithm SHA256).Hash
$lifetimeFixPath = Join-Path $PSScriptRoot 'sdk-fixes/swapchain-lifetime.json'
$lifetimeFix = Get-Content -LiteralPath $lifetimeFixPath -Raw | ConvertFrom-Json
$lifetimeFixHash = (Get-FileHash -LiteralPath $lifetimeFixPath -Algorithm SHA256).Hash
if ($lifetimeFix.sdkCommit -ne $commit) { throw 'Swapchain corrections require the pinned SDK commit.' }
if (Test-Path -LiteralPath $output) {
    $cachedReceiptPath = Join-Path $output 'runtime/fsr3-sdk-build.json'
    $cachedBinaryPath = Join-Path $output 'runtime/amd_fidelityfx_vk.dll'
    if ((Test-Path -LiteralPath $cachedReceiptPath -PathType Leaf) -and
        (Test-Path -LiteralPath $cachedBinaryPath -PathType Leaf)) {
        $cached = Get-Content -LiteralPath $cachedReceiptPath -Raw | ConvertFrom-Json
        if ($cached.schema -eq 1 -and $cached.sdkCommit -eq $commit -and
            $cached.configuration -eq $Configuration -and $cached.patchSha256 -eq $fixHash -and
            $cached.buildRecipeSha256 -eq $recipeHash -and $cached.swapchainLifetimePatchSha256 -eq $lifetimeFixHash -and
            $cached.runtimeSha256 -eq (Get-FileHash -LiteralPath $cachedBinaryPath -Algorithm SHA256).Hash) {
            Write-Host "Using verified corrected FidelityFX runtime: $cachedBinaryPath"
            return
        }
    }
    throw 'Existing SDK output has no matching corrected build receipt. Choose a fresh SDK build output directory.'
}
$cmake = (Get-Command cmake.exe -ErrorAction Stop).Source
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw 'Visual Studio Installer vswhere.exe is required.' }
$instances = @((& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json) | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0 -or $instances.Count -ne 1) { throw 'Visual Studio with x64 C++ build tools is required.' }
$instance = $instances[0]
$generator = switch ([int]($instance.installationVersion.Split('.')[0])) {
    18 { 'Visual Studio 18 2026' }
    17 { 'Visual Studio 17 2022' }
    default { throw 'This SDK build requires Visual Studio 2022 or 2026.' }
}
$toolsets = @(Get-ChildItem -LiteralPath (Join-Path $instance.installationPath 'VC/Tools/MSVC') -Directory | Sort-Object Name -Descending)
if ($toolsets.Count -eq 0) { throw 'Visual Studio MSVC tools are missing.' }
$compiler = Join-Path $toolsets[0].FullName 'bin/Hostx64/x64/cl.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'The selected Visual Studio x64 compiler is missing.' }
if (-not $VulkanSdkRoot) { throw 'Specify -VulkanSdkRoot or set VULKAN_SDK.' }
$vulkanRoot = (Resolve-Path -LiteralPath $VulkanSdkRoot).Path
foreach ($relative in @('Include/vulkan/vulkan.h','Lib/vulkan-1.lib')) {
    if (-not (Test-Path -LiteralPath (Join-Path $vulkanRoot $relative) -PathType Leaf)) { throw "Missing Vulkan SDK input: $relative" }
}
foreach ($relative in @('sdk/tools/binary_store/FidelityFX_SC.exe','sdk/tools/binary_store/glslangValidator.exe','ffx-api/CMakeLists.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $sdkRoot $relative) -PathType Leaf)) { throw "Missing pinned FidelityFX build input: $relative" }
}
$tracked = @(& git -C $sdkRoot ls-files -- sdk ffx-api LICENSE.txt)
if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) { throw 'Could not enumerate the pinned SDK sources.' }
$source = Join-Path $output 'source'
$build = Join-Path $output 'build'
$runtime = Join-Path $output 'runtime'
New-Item -ItemType Directory -Path $source -Force | Out-Null
foreach ($relative in $tracked) {
    $destination = Join-Path $source $relative
    New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $sdkRoot $relative) -Destination $destination
}
$callback = Join-Path $source $fix.file
$beforeHash = (Get-FileHash -LiteralPath $callback -Algorithm SHA256).Hash
$text = [IO.File]::ReadAllText($callback)
if ([regex]::Matches($text, [regex]::Escape($fix.before)).Count -ne 1 -or $text.Contains($fix.after)) {
    throw 'The pinned luma-history declaration differs from the reviewed correction.'
}
$resourceSource = [IO.File]::ReadAllText((Join-Path $source 'sdk/src/components/fsr3upscaler/ffx_fsr3upscaler.cpp'))
foreach ($number in @(1,2)) {
    $pattern = 'FFX_FSR3UPSCALER_RESOURCE_IDENTIFIER_LUMA_HISTORY_' + $number + ',[^\r\n]+\r?\n\s*FFX_SURFACE_FORMAT_R16G16B16A16_FLOAT,'
    if ([regex]::Matches($resourceSource, $pattern).Count -ne 1) { throw 'The SDK luma-history resource is no longer the reviewed RGBA16F image.' }
}
[IO.File]::WriteAllText($callback, $text.Replace($fix.before, $fix.after), [Text.UTF8Encoding]::new($false))
foreach ($replacement in $lifetimeFix.replacements) {
    $path = Join-Path $source $replacement.file
    $code = [IO.File]::ReadAllText($path).Replace("`r`n", "`n")
    if ([regex]::Matches($code, [regex]::Escape($replacement.before)).Count -ne 1) {
        throw "Pinned SDK swapchain correction anchor changed: $($replacement.file)"
    }
    [IO.File]::WriteAllText($path, $code.Replace($replacement.before, $replacement.after), [Text.UTF8Encoding]::new($false))
}
$afterHash = (Get-FileHash -LiteralPath $callback -Algorithm SHA256).Hash
# The SDK otherwise gives every shader compiler the entire host CPU thread count.
# Limit only staged build scheduling; shader flags and permutations remain identical.
$shaderCommands = Join-Path $source 'sdk/include/FidelityFX/gpu/CMakeCompileShaders.txt'
$commandsText = [IO.File]::ReadAllText($shaderCommands)
$threadAnchor = 'set(SC_ARGS ${BASE_ARGS} ${API_BASE_ARGS} ${PERMUTATION_ARGS})'
$threadAnchors = [regex]::Matches($commandsText, [regex]::Escape($threadAnchor)).Count
if ($threadAnchors -ne 1) { throw 'The pinned SDK shader scheduling function changed.' }
$threadReplacement = 'set(SC_ARGS -num-threads=' + $ShaderThreads + ' ${BASE_ARGS} ${API_BASE_ARGS} ${PERMUTATION_ARGS})'
[IO.File]::WriteAllText($shaderCommands, $commandsText.Replace($threadAnchor, $threadReplacement), [Text.UTF8Encoding]::new($false))
$previousVulkanSdk = $env:VULKAN_SDK
try {
    $env:VULKAN_SDK = $vulkanRoot
    & $cmake -S (Join-Path $source 'ffx-api') -B $build -G $generator -A x64 `
        "-DCMAKE_GENERATOR_INSTANCE=$($instance.installationPath)" -DFFX_API_BACKEND=VK_X64 `
        "-DVulkan_INCLUDE_DIR=$(Join-Path $vulkanRoot 'Include')" "-DVulkan_LIBRARY=$(Join-Path $vulkanRoot 'Lib/vulkan-1.lib')"
    if ($LASTEXITCODE -ne 0) { throw "FidelityFX SDK configure failed: $LASTEXITCODE" }
    & $cmake --build $build --config $Configuration --target amd_fidelityfx_vk --parallel $Parallel
    if ($LASTEXITCODE -ne 0) { throw "FidelityFX SDK build failed: $LASTEXITCODE" }
} finally { $env:VULKAN_SDK = $previousVulkanSdk }
$binaryName = if ($Configuration -eq 'Release') { 'amd_fidelityfx_vk.dll' } else { 'amd_fidelityfx_vkdrel.dll' }
$built = Join-Path $source "ffx-api/bin/$binaryName"
if (-not (Test-Path -LiteralPath $built -PathType Leaf)) { throw "The SDK build did not produce $built" }
New-Item -ItemType Directory -Path $runtime -Force | Out-Null
$destination = Join-Path $runtime 'amd_fidelityfx_vk.dll'
Copy-Item -LiteralPath $built -Destination $destination
$receipt = [ordered]@{
    schema=1; fix=$fix.id; upstream=$fix.upstream; sdkCommit=$commit; configuration=$Configuration
    callback=$fix.file; sourceBeforeSha256=$beforeHash; sourceAfterSha256=$afterHash
    patchSha256=$fixHash; buildRecipeSha256=$recipeHash; swapchainLifetimePatchSha256=$lifetimeFixHash
    runtimeSha256=(Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
    cmake=$cmake; compiler=$compiler; generator=$generator; vulkanSdk=$vulkanRoot
    parallel=$Parallel; shaderThreads=$ShaderThreads
}
$receipt | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $runtime 'fsr3-sdk-build.json') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $sdkRoot 'LICENSE.txt') -Destination (Join-Path $runtime 'LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $sdkRoot 'sdk/LICENSE.txt') -Destination (Join-Path $runtime 'sdk-LICENSE.txt')
Write-Host "Corrected FidelityFX runtime and hash receipt: $runtime"
