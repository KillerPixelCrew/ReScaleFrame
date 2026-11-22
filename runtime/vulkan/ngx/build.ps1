<#
.SYNOPSIS
Builds the NGX caller shim and its Rust loader into VulkanStoryNgx.dll.
.DESCRIPTION
Uses Cargo for x86_64-pc-windows-gnu and links the archive with gcc.exe or cc.exe.
Reuses a target newer than the build inputs. A failed build throws and preserves
the previous DLL. The caller-frame-preserving compiler flag remains required.
.PARAMETER OutputDirectory
Destination directory for VulkanStoryNgx.dll; created if needed.
#>
param([Parameter(Mandatory = $true)][string]$OutputDirectory)

$ErrorActionPreference = 'Stop'
$output = [IO.Path]::GetFullPath($OutputDirectory)
$target = Join-Path $output 'VulkanStoryNgx.dll'
$workspace = Join-Path $PSScriptRoot '../../..'
$source = Join-Path $PSScriptRoot 'vulkanstory_ngx.c'
$inputs = @(
    $source
    (Join-Path $PSScriptRoot 'vulkanstory_ngx.h')
    (Join-Path $PSScriptRoot 'entries.h')
    (Join-Path $PSScriptRoot 'build.rs')
    (Join-Path $PSScriptRoot 'src/lib.rs')
    (Join-Path $PSScriptRoot 'Cargo.toml')
    (Join-Path $workspace 'Cargo.toml')
    (Join-Path $workspace 'Cargo.lock')
    (Join-Path $workspace 'rust-toolchain.toml')
    $PSCommandPath
)
if (Test-Path -LiteralPath $target -PathType Leaf) {
    $targetTime = (Get-Item -LiteralPath $target).LastWriteTimeUtc
    $stale = $inputs | Where-Object {
        -not (Test-Path -LiteralPath $_ -PathType Leaf) -or
        $targetTime -le (Get-Item -LiteralPath $_).LastWriteTimeUtc
    }
    if (-not $stale) { return }
}

$compiler = Get-Command gcc.exe -ErrorAction SilentlyContinue
if (-not $compiler) { $compiler = Get-Command cc.exe -ErrorAction SilentlyContinue }
if (-not $compiler) {
    throw 'No C compiler found; VulkanStoryNgx.dll was not built.'
}
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source

$scratch = Join-Path $output ('.vulkanstory-ngx-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
try {
    $rustTarget = 'x86_64-pc-windows-gnu'
    $rustOutput = Join-Path $scratch 'cargo'
    Push-Location -LiteralPath $workspace
    try {
        & $cargo build --locked --release -p rsf-vulkanstory-ngx --target $rustTarget --target-dir $rustOutput
        if ($LASTEXITCODE -ne 0) { throw 'The VulkanStory NGX Rust loader failed to compile.' }
    }
    finally {
        Pop-Location
    }

    $rustArchive = Join-Path $rustOutput "$rustTarget/release/librsf_vulkanstory_ngx.a"
    $temporary = Join-Path $scratch 'VulkanStoryNgx.dll'
    $libraries = @('-ladvapi32', '-lntdll', '-luserenv', '-lws2_32', '-ldbghelp')
    & $compiler.Source -std=c99 -O2 -fno-optimize-sibling-calls -shared $source $rustArchive -o $temporary @libraries
    if ($LASTEXITCODE -ne 0) { throw 'The VulkanStory NGX bridge failed to link.' }
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
Write-Host "vulkanstory-ngx: built $target"
