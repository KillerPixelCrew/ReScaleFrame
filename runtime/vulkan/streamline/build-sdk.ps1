<#
.SYNOPSIS
Builds the pinned Streamline common/interposer Vulkan corrections in an isolated source copy.
.DESCRIPTION
Builds Production plugins only. The interposer pins this build's common DLL by SHA-256;
closed NVIDIA plugins still require their original signatures. The bridge pins the
resulting interposer through the generated receipt. No game installation is changed.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$HostRoot,
    [string]$SdkRoot = (Join-Path $HostRoot 'sdk/streamline'),
    [string]$OutputDirectory = (Join-Path $HostRoot 'artifacts/rescaleframe/streamline/Release/win-x64'),
    [string]$Python = 'python.exe',
    [string]$PackageCache = (Join-Path $HostRoot 'artifacts/packman-cache')
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $HostRoot).Path
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
$artifactRoot = [IO.Path]::GetFullPath((Join-Path $root 'artifacts'))
if (-not $output.StartsWith($artifactRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Streamline output must be inside the repository artifacts directory.'
}
$commit = '2122257e0fce486f91b385aa63b9a09b0a34b363'
if ((& git -C $sdk rev-parse HEAD).Trim() -ne $commit) { throw 'Unexpected Streamline source commit.' }
$patch = Join-Path $PSScriptRoot 'sdk-fixes/prepare.py'
$patchHash = (Get-FileHash -LiteralPath $patch).Hash
$dispatchHash = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk-fixes/ngx-vulkan-dispatch.h')).Hash
$recipeHash = (Get-FileHash -LiteralPath $PSCommandPath).Hash
$runtime = Join-Path $output 'runtime'
$receiptPath = Join-Path $runtime 'streamline-sdk-build.json'
if (Test-Path -LiteralPath $output) {
    if (Test-Path -LiteralPath $receiptPath) {
        $cached = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        $valid = $cached.schema -eq 1 -and $cached.sdkCommit -eq $commit -and
            $cached.patchSha256 -eq $patchHash -and $cached.dispatchSha256 -eq $dispatchHash -and $cached.recipeSha256 -eq $recipeHash
        foreach ($name in @('sl.common.dll','sl.interposer.dll')) {
            $binary = Join-Path $runtime $name
            $valid = $valid -and (Test-Path -LiteralPath $binary)
            if ($valid) { $valid = $cached.binaries.$name -eq (Get-FileHash -LiteralPath $binary).Hash }
        }
        if ($valid) { Write-Host "Using verified corrected Streamline runtime: $runtime"; return }
    }
    throw 'Existing output has no matching corrected build receipt. Select a fresh output directory.'
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = @((& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json) | ConvertFrom-Json)[0]
if (-not $vs) { throw 'Visual Studio x64 C++ tools are required.' }
$toolset = switch ([int]$vs.installationVersion.Split('.')[0]) { 18 { 'msc-v145' } 17 { 'msc-v143' } default { throw 'Unsupported MSVC toolset.' } }
$msbuild = Join-Path $vs.installationPath 'MSBuild/Current/Bin/MSBuild.exe'
$source = Join-Path $output 'source'
New-Item -ItemType Directory -Path $source -Force | Out-Null
$files = @(& git -C $sdk ls-files)
if ($LASTEXITCODE -ne 0 -or -not $files.Count) { throw 'Could not enumerate pinned SDK sources.' }
foreach ($file in $files) {
    $from = Join-Path $sdk $file
    if (-not (Test-Path -LiteralPath $from -PathType Leaf)) { continue }
    $to = Join-Path $source $file
    New-Item -ItemType Directory -Path (Split-Path $to) -Force | Out-Null
    Copy-Item -LiteralPath $from -Destination $to
}
$saved = @{}
foreach ($name in @('PM_PACKAGES_ROOT','GIT_DIR','GIT_WORK_TREE','GIT_INDEX_FILE')) { $saved[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
try {
    $env:PM_PACKAGES_ROOT = [IO.Path]::GetFullPath($PackageCache)
    $env:GIT_DIR = (& git -C $sdk rev-parse --absolute-git-dir).Trim()
    $env:GIT_WORK_TREE = $source
    $env:GIT_INDEX_FILE = Join-Path $output 'source-index'
    Push-Location $source
    try {
        & git read-tree $commit
        if ($LASTEXITCODE -ne 0) { throw 'Isolated SDK index initialization failed.' }
        & git lfs pull '--include=external/nvapi/amd64/nvapi64.lib,external/ngx-sdk/lib/Windows_x86_64/nvsdk_ngx_d.lib,external/reflex-sdk-vk/lib/NvLowLatencyVk.lib' '--exclude='
        if ($LASTEXITCODE -ne 0) { throw 'Pinned SDK build-library fetch failed.' }
        & './tools/packman/packman.cmd' pull -p windows-x86_64 project.xml
        if ($LASTEXITCODE -ne 0) { throw 'Pinned SDK dependency fetch failed.' }
        & './tools/premake5/premake5.exe' vs2022 --file=premake.lua "--with-toolset=$toolset" --with-nvllvk=yes
        if ($LASTEXITCODE -ne 0) { throw 'SDK project generation failed.' }
    } finally { Pop-Location }
    & $Python $patch $source
    if ($LASTEXITCODE -ne 0) { throw 'SDK source corrections failed.' }
    New-Item -ItemType Directory -Path (Join-Path $source '_artifacts') -Force | Out-Null
    @('#pragma once','#define GIT_LAST_COMMIT "SHA:2122257"','#define GIT_LAST_COMMIT_TAG "v2.14.1"','#define GIT_LAST_COMMIT_SHORT "2122257"') |
        Set-Content -LiteralPath (Join-Path $source '_artifacts/gitVersion.h')
    foreach ($project in @('sl.compute','sl.common','sl.interposer')) {
        if ($project -eq 'sl.interposer') {
            $hash = (Get-FileHash -LiteralPath (Join-Path $source '_artifacts/sl.common/Production_x64/sl.common.dll')).Hash
            $bytes = for ($i = 0; $i -lt 64; $i += 2) { '0x' + $hash.Substring($i,2) }
            ('static constexpr unsigned char vulkanstoryCommonSha256[32] = {' + ($bytes -join ',') + '};') |
                Set-Content -LiteralPath (Join-Path $source 'source/core/sl.security/vulkanstory-common-hash.h')
        }
        & $msbuild (Join-Path $source "_project/vs2022/$project.vcxproj") /t:Build /p:Configuration=Production /p:Platform=x64 /m:4 /v:minimal /nologo
        if ($LASTEXITCODE -ne 0) { throw "SDK build failed: $project" }
    }
    New-Item -ItemType Directory -Path $runtime -Force | Out-Null
    $binaries = [ordered]@{}
    foreach ($project in @('sl.common','sl.interposer')) {
        $name = "$project.dll"
        Copy-Item -LiteralPath (Join-Path $source "_artifacts/$project/Production_x64/$name") -Destination $runtime
        $binaries[$name] = (Get-FileHash -LiteralPath (Join-Path $runtime $name)).Hash
    }
    Copy-Item -LiteralPath (Join-Path $source 'vulkanstory-fixes.json') -Destination $runtime
    [ordered]@{schema=1;sdkCommit=$commit;configuration='Production';patchSha256=$patchHash;dispatchSha256=$dispatchHash;recipeSha256=$recipeHash;binaries=$binaries} |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $receiptPath
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
}
