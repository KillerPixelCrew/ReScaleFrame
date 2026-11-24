<#
.SYNOPSIS
Runs the explicit FSR4 Vulkan/DX12 deferred-submit fixture under the GPU lease.
.DESCRIPTION
Does not build or stage SDK payloads. Captures compact validation/results only;
the service uses GPU sharing throughout, with no production CPU image readback.
#>
param(
    [Parameter(Mandatory = $true)][string]$Fixture,
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][string]$ValidationLayerDirectory
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
. (Join-Path $PSScriptRoot 'godot-process-isolation.ps1')
$taskFixture = (Resolve-Path -LiteralPath $Fixture).ProviderPath
$taskRuntime = (Resolve-Path -LiteralPath $RuntimeDirectory).ProviderPath
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
$taskRun = Invoke-RsfInteropFixture $taskFixture $taskOutput ('--device "' + $taskRuntime + '"') $ValidationLayerDirectory
$taskReceipt = [ordered]@{
    timestamp_utc = [DateTime]::UtcNow.ToString('o')
    fixture = $taskFixture
    fixture_sha256 = (Get-FileHash -LiteralPath $taskFixture -Algorithm SHA256).Hash
    runtime_sha256 = (Get-FileHash -LiteralPath (Join-Path $taskRuntime 'amd_fidelityfx_upscaler_dx12.dll') -Algorithm SHA256).Hash
    effective_extension_point_policy = $taskRun.Policy
    exit_code = $taskRun.ExitCode
    synchronization_enabled = $taskRun.SynchronizationEnabled
    validation_errors = $taskRun.ValidationErrors
    result = if ($taskRun.Metrics.Count) { $taskRun.Metrics[-1] | ConvertFrom-Json } else { $null }
}
$taskReceipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskOutput 'receipt.json') -Encoding utf8
Get-Content -LiteralPath $taskRun.Stdout
if ($taskRun.ExitCode -or $taskReceipt.validation_errors -or !$taskReceipt.synchronization_enabled) {
    throw "FSR4 fixture did not pass. See $taskOutput"
}
