<#
.SYNOPSIS
Compiles the retained NGX caller shim into VulkanStoryNgx.dll.
.DESCRIPTION
Uses gcc.exe or cc.exe and builds to VulkanStoryNgx.tmp.dll before replacing the target. Skips compilation when the target is newer than both source and header. Missing compiler or failed compilation throws; failed temporary output is removed. The caller-frame-preserving compiler flag remains part of the bridge contract.
.PARAMETER OutputDirectory
Destination directory for VulkanStoryNgx.dll; created if needed.
#>
param([Parameter(Mandatory = $true)][string]$OutputDirectory)

$ErrorActionPreference = 'Stop'
$compiler = Get-Command gcc.exe -ErrorAction SilentlyContinue
if (-not $compiler) { $compiler = Get-Command cc.exe -ErrorAction SilentlyContinue }

$output = [IO.Path]::GetFullPath($OutputDirectory)
$target = Join-Path $output 'VulkanStoryNgx.dll'
$temporary = Join-Path $output 'VulkanStoryNgx.tmp.dll'
if (-not $compiler) {
    throw 'No C compiler found; VulkanStoryNgx.dll was not built.'
}

New-Item -ItemType Directory -Path $output -Force | Out-Null
$source = Join-Path $PSScriptRoot 'vulkanstory_ngx.c'
$header = Join-Path $PSScriptRoot 'vulkanstory_ngx.h'
if ((Test-Path -LiteralPath $target) -and
    (Get-Item -LiteralPath $target).LastWriteTimeUtc -gt (Get-Item -LiteralPath $source).LastWriteTimeUtc -and
    (Get-Item -LiteralPath $target).LastWriteTimeUtc -gt (Get-Item -LiteralPath $header).LastWriteTimeUtc) {
    return
}

& $compiler.Source -std=c99 -O2 -fno-optimize-sibling-calls -shared $source -o $temporary
if ($LASTEXITCODE -ne 0) {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    throw 'The VulkanStory NGX bridge failed to compile.'
}
Move-Item -LiteralPath $temporary -Destination $target -Force
Write-Host "vulkanstory-ngx: built $target"
