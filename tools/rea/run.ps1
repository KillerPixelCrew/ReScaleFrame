# Launch the repository's pinned REA package without changing user-wide settings.
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$entryPoint = Join-Path $PSScriptRoot 'node_modules/rea-agents/scripts/rea.mjs'
if (-not (Test-Path -LiteralPath $entryPoint)) {
    throw 'REA is not installed. Run npm ci --prefix tools/rea from the repository root.'
}

$environmentFile = Join-Path $repoRoot '.local/rea/environment.json'
if (Test-Path -LiteralPath $environmentFile) {
    $providerEnvironment = Get-Content -LiteralPath $environmentFile -Raw | ConvertFrom-Json
    foreach ($name in @('REA_NODE_EXE', 'REA_ANALYSIS_PROVIDER', 'REA_IDA_MCP_CONFIG', 'REA_GHIDRA_STARTUP_TIMEOUT_MS', 'GHIDRA_INSTALL_DIR', 'JAVA_HOME')) {
        $value = $providerEnvironment.$name
        if ($value) {
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
    }
}

$nodeExecutable = if ($env:REA_NODE_EXE) { $env:REA_NODE_EXE } else { 'node' }
& $nodeExecutable $entryPoint @args
exit $LASTEXITCODE
