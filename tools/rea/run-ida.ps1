# Launch the repo-local upstream IDA supervisor. IDA itself remains operator-installed.
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$python = Join-Path $repoRoot '.local/rea/ida-venv/Scripts/python.exe'
if (-not (Test-Path -LiteralPath $python)) {
    throw 'The IDA MCP environment is missing. Follow docs/tooling.md to install it.'
}

$environmentFile = Join-Path $repoRoot '.local/rea/environment.json'
if (Test-Path -LiteralPath $environmentFile) {
    $providerEnvironment = Get-Content -LiteralPath $environmentFile -Raw | ConvertFrom-Json
    if ($providerEnvironment.IDADIR) {
        $env:IDADIR = $providerEnvironment.IDADIR
    }
}

# Windows venv python.exe is a redirector with a separate child process. Launch
# the base interpreter so upstream's Popen handle owns the actual IDA worker.
$basePython = & $python -c 'import sys; print(sys._base_executable)'
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $basePython)) {
    throw 'Cannot resolve the IDA environment base interpreter.'
}
$env:PYTHONPATH = Join-Path $repoRoot '.local/rea/ida-venv/Lib/site-packages'
$env:PYTHONNOUSERSITE = '1'
$env:IDA_MCP_OPEN_TIMEOUT = '1800'
$env:IDA_MCP_WORKER_CALL_TIMEOUT = '1800'
$env:IDA_MCP_TOOL_TIMEOUT_SEC = '1800'
& $basePython -m ida_pro_mcp.idalib_supervisor --stdio --max-workers 1 @args
exit $LASTEXITCODE
