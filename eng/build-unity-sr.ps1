<#
.SYNOPSIS
Build Unity Mono research artifacts against a specific game's managed assemblies.
.DESCRIPTION
Restores locked Harmony dependencies, builds the managed adapter and selected native targets,
then builds the Release overlay and copies DLLs/notices into build/windows-x64/bin/Configuration.
Requires Unity native headers under vendor/unity-native. Does not launch or deploy to a game.
.PARAMETER UnityManagedDirectory
Directory containing the researched game's Unity managed reference assemblies.
.PARAMETER Configuration
Native and managed configuration. The overlay is built in Release for either selection.
.PARAMETER VS2026
Use the Visual Studio 2026 generator presets instead of the default VS 2022 presets.
.OUTPUTS
Artifact directory on stdout; a failed external command throws.
#>
param(
    [Parameter(Mandatory)][string]$UnityManagedDirectory,
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [switch]$VS2026
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskManaged = (Resolve-Path -LiteralPath $UnityManagedDirectory).Path
$taskSuffix = if ($VS2026) { '-vs18' } else { '' }
# PowerShell's error preference does not convert native exit codes into exceptions.
function Invoke-UnityChecked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}
if (!(Test-Path -LiteralPath "$taskRoot/vendor/unity-native/include/IUnityGraphicsD3D12.h")) {
    throw 'Unity native headers are required for this adapter. See docs/dependencies.md.'
}
Push-Location -LiteralPath $taskRoot
try {
    Invoke-UnityChecked dotnet @('restore', 'games/unity-mono/managed/ReScaleFrame.Unity.Managed.csproj', '--locked-mode', "-p:UnityManagedDirectory=$taskManaged")
    Invoke-UnityChecked dotnet @('build', 'games/unity-mono/managed/ReScaleFrame.Unity.Managed.csproj', '--no-restore', '-c', $Configuration, "-p:UnityManagedDirectory=$taskManaged")
    Invoke-UnityChecked cmake @('--preset', "windows-x64$taskSuffix")
    Invoke-UnityChecked cmake @('--build', '--preset', "windows-$($Configuration.ToLowerInvariant())$taskSuffix", '--target', 'rsf_game_unity_mono', 'rsf_orchestrator', 'rsf_proxy_dinput8')
    Invoke-UnityChecked cargo @('build', '-p', 'rescaleframe-overlay', '--release', '--locked')
    $taskSource = "$taskRoot/build/managed/unity/bin/$Configuration/netstandard2.1"
    $taskDestination = "$taskRoot/build/windows-x64/bin/$Configuration"
    Copy-Item -LiteralPath "$taskRoot/target/release/rescaleframe_overlay.dll" -Destination "$taskDestination/rescaleframe_overlay.dll"
    foreach ($taskLeaf in @('ReScaleFrame.Unity.Managed.dll', '0Harmony.dll')) {
        Copy-Item -LiteralPath "$taskSource/$taskLeaf" -Destination "$taskDestination/$taskLeaf"
    }
    $taskNuGet = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.nuget/packages/lib.harmony/2.4.2/LICENSE'
    if (Test-Path -LiteralPath $taskNuGet) { Copy-Item -LiteralPath $taskNuGet -Destination "$taskDestination/Harmony-LICENSE.txt" }
    Write-Output "Unity SR research artifacts: $taskDestination"
}
finally { Pop-Location }
