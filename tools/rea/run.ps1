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
    foreach ($name in @('REA_ANALYSIS_PROVIDER', 'GHIDRA_INSTALL_DIR', 'JAVA_HOME')) {
        $value = $providerEnvironment.$name
        if ($value) {
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
    }
}

& node $entryPoint @args
exit $LASTEXITCODE
