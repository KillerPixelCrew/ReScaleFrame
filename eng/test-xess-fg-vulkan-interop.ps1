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
New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
$taskVariables = @('VK_INSTANCE_LAYERS', 'VK_LAYER_PATH', 'VK_LOADER_LAYERS_DISABLE',
    'VK_VALIDATION_VALIDATE_SYNC', 'VK_LAYER_REPORT_FLAGS', 'VK_LAYER_DEBUG_ACTION')
$taskSaved = @{}
foreach ($taskName in $taskVariables) { $taskSaved[$taskName] = [Environment]::GetEnvironmentVariable($taskName) }
$taskLease = Enter-RsfGodotValidation
try {
    $env:VK_LOADER_LAYERS_DISABLE = '~implicit~'
    if ($ValidationLayerDirectory) {
        $env:VK_LAYER_PATH = (Resolve-Path -LiteralPath $ValidationLayerDirectory).ProviderPath
        $env:VK_INSTANCE_LAYERS = 'VK_LAYER_KHRONOS_validation'
        $env:VK_VALIDATION_VALIDATE_SYNC = '1'
        $env:VK_LAYER_REPORT_FLAGS = 'error,warn,info'
        $env:VK_LAYER_DEBUG_ACTION = 'VK_DBG_LAYER_ACTION_LOG_MSG'
    }
    $taskStdout = Join-Path $taskOutput 'stdout.log'
    $taskStderr = Join-Path $taskOutput 'stderr.log'
    [uint32]$taskPolicy = 0
    $taskArguments = '--device "' + $taskRuntime + '"'
    if ($Present) { $taskArguments += ' --present' }
    $taskProcess = [RsfSyncIsolationProcess]::Start($taskFixture,
        $taskArguments, (Get-Location).ProviderPath,
        $taskStdout, $taskStderr, $true, [ref]$taskPolicy)
    if (!$taskProcess.WaitForExit(60000)) {
        $taskProcess.Kill($true)
        $taskProcess.WaitForExit()
        throw 'Owned interop fixture exceeded 60 seconds.'
    }
    $taskLog = (Get-Content -LiteralPath $taskStdout -Raw) + (Get-Content -LiteralPath $taskStderr -Raw)
    $taskMetrics = @(Get-Content -LiteralPath $taskStdout | Where-Object { $_.StartsWith('{') })
    $taskReceipt = [ordered]@{
        timestamp_utc = [DateTime]::UtcNow.ToString('o')
        fixture = $taskFixture
        fixture_sha256 = (Get-FileHash -LiteralPath $taskFixture -Algorithm SHA256).Hash
        effective_extension_point_policy = $taskPolicy
        exit_code = $taskProcess.ExitCode
        validation_requested = [bool]$ValidationLayerDirectory
        presentation_requested = [bool]$Present
        synchronization_enabled = $taskLog.Contains('CURRENT-VALIDATION-ENABLED') -and $taskLog.Contains('Synchronization')
        validation_errors = @([regex]::Matches($taskLog, 'Validation Error|VUID-|SYNC-HAZARD')).Count
        device_result = if ($taskMetrics.Count) { $taskMetrics[-1] | ConvertFrom-Json } else { $null }
        runtime = @{}
    }
    foreach ($taskDll in @('libxess_fg.dll', 'libxell.dll')) {
        $taskReceipt.runtime[$taskDll] = (Get-FileHash -LiteralPath (Join-Path $taskRuntime $taskDll) -Algorithm SHA256).Hash
    }
    $taskReceipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskOutput 'receipt.json') -Encoding utf8
    Get-Content -LiteralPath $taskStdout
    if ($taskProcess.ExitCode) { throw "Interop fixture failed: $($taskProcess.ExitCode). See $taskOutput" }
    if ($taskReceipt.validation_errors -or ($ValidationLayerDirectory -and !$taskReceipt.synchronization_enabled)) {
        throw "Interop validation did not pass. See $taskOutput"
    }
} finally {
    Restore-RsfGodotEnvironment $taskSaved
    $taskLease.ReleaseMutex()
    $taskLease.Dispose()
}
