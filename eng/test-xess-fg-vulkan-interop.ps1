<#
.SYNOPSIS
Runs the explicit XeSS FG Vulkan/DX12 import fixture under the shared GPU lease.
.DESCRIPTION
Does not build, stage or modify SDK payloads. The fixture imports three formats
and a timeline fence, then drains and releases them. This does not test XeSS
dispatch, UI composition, generated images or WSI ownership.
#>
param(
    [Parameter(Mandatory = $true)][string]$Fixture,
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$ValidationLayerDirectory,
    [switch]$Present
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
. (Join-Path $PSScriptRoot 'godot-process-isolation.ps1')
$taskFixture = (Resolve-Path -LiteralPath $Fixture).ProviderPath
$taskRuntime = (Resolve-Path -LiteralPath $RuntimeDirectory).ProviderPath
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
$taskArguments = '--device "' + $taskRuntime + '"'
if ($Present) { $taskArguments += ' --present' }
$taskRun = Invoke-RsfInteropFixture $taskFixture $taskOutput $taskArguments $ValidationLayerDirectory
$taskReceipt = [ordered]@{
    timestamp_utc = [DateTime]::UtcNow.ToString('o')
    fixture = $taskFixture
    fixture_sha256 = (Get-FileHash -LiteralPath $taskFixture -Algorithm SHA256).Hash
    effective_extension_point_policy = $taskRun.Policy
    exit_code = $taskRun.ExitCode
    validation_requested = [bool]$ValidationLayerDirectory
    presentation_requested = [bool]$Present
    synchronization_enabled = $taskRun.SynchronizationEnabled
    validation_errors = $taskRun.ValidationErrors
    device_result = if ($taskRun.Metrics.Count) { $taskRun.Metrics[-1] | ConvertFrom-Json } else { $null }
    runtime = @{}
}
foreach ($taskDll in @('libxess_fg.dll', 'libxell.dll')) {
    $taskReceipt.runtime[$taskDll] = (Get-FileHash -LiteralPath (Join-Path $taskRuntime $taskDll) -Algorithm SHA256).Hash
}
$taskReceipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskOutput 'receipt.json') -Encoding utf8
Get-Content -LiteralPath $taskRun.Stdout
if ($taskRun.ExitCode) { throw "Interop fixture failed: $($taskRun.ExitCode). See $taskOutput" }
if ($taskReceipt.validation_errors -or ($ValidationLayerDirectory -and !$taskReceipt.synchronization_enabled)) {
    throw "Interop validation did not pass. See $taskOutput"
}
