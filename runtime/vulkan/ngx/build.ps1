<#
.SYNOPSIS
Builds the Rust NGX caller module into VulkanStoryNgx.dll.
.DESCRIPTION
Uses Cargo for x86_64-pc-windows-gnu.
Reuses a target newer than the build inputs. A failed build throws and preserves
the previous DLL.
.PARAMETER OutputDirectory
Destination directory for VulkanStoryNgx.dll; created if needed.
#>
param([Parameter(Mandatory = $true)][string]$OutputDirectory)

$ErrorActionPreference = 'Stop'
$output = [IO.Path]::GetFullPath($OutputDirectory)
$target = Join-Path $output 'VulkanStoryNgx.dll'
$workspace = Join-Path $PSScriptRoot '../../..'
$inputs = @(
    (Join-Path $PSScriptRoot 'vulkanstory_ngx.h')
    (Join-Path $PSScriptRoot 'src/lib.rs')
    (Join-Path $PSScriptRoot 'src/loader.rs')
    (Join-Path $PSScriptRoot 'src/parameters.rs')
    (Join-Path $workspace 'integrations/vulkanstory/native/ngx.rs')
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

    $temporary = Join-Path $rustOutput "$rustTarget/release/rsf_vulkanstory_ngx.dll"
    Move-Item -LiteralPath $temporary -Destination $target -Force
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
Write-Host "vulkanstory-ngx: built $target"
