param(
    [switch]$Mono,
    [ValidateSet('editor','template_debug','template_release')][string]$Target = 'editor',
    [int]$Jobs = 12
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskPython = Join-Path $taskRoot '.local/godot/build-env/Scripts/python.exe'
if (!(Test-Path -LiteralPath $taskPython)) { throw 'Prepare the local Godot build environment first.' }
& $taskPython (Join-Path $PSScriptRoot 'patch-godot.py')
if ($LASTEXITCODE) { throw 'Godot bridge patch failed.' }
$taskEdition = if ($Mono) { 'mono' } else { 'standard' }
$taskSource = Join-Path $taskRoot 'references/godot/engine'
$taskOutput = if ($Target -eq 'editor') { Join-Path $taskRoot ".local/godot/engines/$taskEdition" } else { Join-Path $taskRoot ".local/godot/templates/$taskEdition" }
Push-Location -LiteralPath $taskSource
try {
    $taskArguments = @('-m','SCons','platform=windows',"target=$Target",'arch=x86_64','d3d12=yes',
        "mesa_libs=$taskRoot/.local/godot/build-deps/mesa", "custom_modules=$taskRoot/integrations/godot/modules",
        "module_mono_enabled=$($Mono.IsPresent.ToString().ToLowerInvariant())",'debug_symbols=no','optimize=speed',"-j$Jobs")
    & $taskPython @taskArguments
    if ($LASTEXITCODE) { throw 'Godot engine build failed.' }
    New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
    $taskBase = "godot.windows.$Target.x86_64" + $(if ($Mono) { '.mono' } else { '' })
    $taskHash = (Get-FileHash -LiteralPath (Join-Path $taskSource "bin/$taskBase.exe") -Algorithm SHA256).Hash
    $taskBuildDirectory = Join-Path $taskOutput ('builds/' + $taskHash.Substring(0,16).ToLowerInvariant())
    foreach ($taskSuffix in @('.exe','.console.exe')) {
        Copy-RsfGodotArtifact (Join-Path $taskSource "bin/$taskBase$taskSuffix") (Join-Path $taskBuildDirectory "$taskBase$taskSuffix")
    }
    if ($Mono -and $Target -eq 'editor') {
        # No public Godot API bindings change. Use the exact official 4.7.2 managed
        # assemblies for the matching native .NET engine until new types need glue.
        Copy-RsfGodotAddon (Join-Path $taskRoot '.local/godot/mono/Godot_v4.7.2-stable_mono_win64/GodotSharp') (Join-Path $taskBuildDirectory 'GodotSharp')
    }
    @{executable=(Join-Path $taskBuildDirectory "$taskBase.exe"); sha256=$taskHash; edition=$taskEdition; target=$Target;
        engine_revision=(& git rev-parse HEAD)} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput "$Target.json") -Encoding utf8
} finally { Pop-Location }
Write-Output "Staged Godot $taskEdition / $Target engine: $taskOutput"
