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
New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
$taskSaved = @{}
foreach ($taskName in @('VK_INSTANCE_LAYERS', 'VK_LAYER_PATH', 'VK_LOADER_LAYERS_DISABLE',
    'VK_VALIDATION_VALIDATE_SYNC', 'VK_LAYER_REPORT_FLAGS', 'VK_LAYER_DEBUG_ACTION')) {
    $taskSaved[$taskName] = [Environment]::GetEnvironmentVariable($taskName)
}
$taskLease = Enter-RsfGodotValidation
try {
    $env:VK_LOADER_LAYERS_DISABLE = '~implicit~'
    $env:VK_LAYER_PATH = (Resolve-Path -LiteralPath $ValidationLayerDirectory).ProviderPath
    $env:VK_INSTANCE_LAYERS = 'VK_LAYER_KHRONOS_validation'
    $env:VK_VALIDATION_VALIDATE_SYNC = '1'
    $env:VK_LAYER_REPORT_FLAGS = 'error,warn,info'
    $env:VK_LAYER_DEBUG_ACTION = 'VK_DBG_LAYER_ACTION_LOG_MSG'
    $taskStdout = Join-Path $taskOutput 'stdout.log'
    $taskStderr = Join-Path $taskOutput 'stderr.log'
    [uint32]$taskPolicy = 0
    $taskProcess = [RsfSyncIsolationProcess]::Start($taskFixture,
        ('--device "' + $taskRuntime + '"'), (Get-Location).ProviderPath,
        $taskStdout, $taskStderr, $true, [ref]$taskPolicy)
    if (!$taskProcess.WaitForExit(60000)) {
        $taskProcess.Kill($true); $taskProcess.WaitForExit()
        throw 'Owned FSR4 fixture exceeded 60 seconds.'
    }
    $taskLog = (Get-Content -LiteralPath $taskStdout -Raw) + (Get-Content -LiteralPath $taskStderr -Raw)
    $taskMetrics = @(Get-Content -LiteralPath $taskStdout | Where-Object { $_.StartsWith('{') })
    $taskReceipt = [ordered]@{
        timestamp_utc = [DateTime]::UtcNow.ToString('o')
        fixture = $taskFixture
        fixture_sha256 = (Get-FileHash -LiteralPath $taskFixture -Algorithm SHA256).Hash
        runtime_sha256 = (Get-FileHash -LiteralPath (Join-Path $taskRuntime 'amd_fidelityfx_upscaler_dx12.dll') -Algorithm SHA256).Hash
        effective_extension_point_policy = $taskPolicy
        exit_code = $taskProcess.ExitCode
        synchronization_enabled = $taskLog.Contains('CURRENT-VALIDATION-ENABLED') -and $taskLog.Contains('Synchronization')
        validation_errors = @([regex]::Matches($taskLog, 'Validation Error|VUID-|SYNC-HAZARD')).Count
        result = if ($taskMetrics.Count) { $taskMetrics[-1] | ConvertFrom-Json } else { $null }
    }
    $taskReceipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskOutput 'receipt.json') -Encoding utf8
    Get-Content -LiteralPath $taskStdout
    if ($taskProcess.ExitCode -or $taskReceipt.validation_errors -or !$taskReceipt.synchronization_enabled) {
        throw "FSR4 fixture did not pass. See $taskOutput"
    }
} finally {
    Restore-RsfGodotEnvironment $taskSaved
    $taskLease.ReleaseMutex(); $taskLease.Dispose()
}
