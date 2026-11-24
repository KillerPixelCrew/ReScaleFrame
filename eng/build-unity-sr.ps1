param(
    [Alias('UnityReferenceDirectory')][string]$UnityManagedDirectory,
    [string]$UnityEditorDirectory,
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [switch]$VS2026
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'common.ps1')
if ($UnityEditorDirectory) {
    & "$PSScriptRoot/prepare-unity-references.ps1" -UnityEditorDirectory $UnityEditorDirectory
    if (!$UnityManagedDirectory) { $UnityManagedDirectory = Join-Path $taskRoot '.local/unity-references/6000.3.14f1' }
}
if (!$UnityManagedDirectory) { $UnityManagedDirectory = Join-Path $taskRoot '.local/unity-references/6000.3.14f1' }
if (!(Test-Path -LiteralPath $UnityManagedDirectory)) {
    throw 'Prepare public Unity SDK references with eng/prepare-unity-references.ps1, or supply UnityReferenceDirectory.'
}
$taskManaged = (Resolve-Path -LiteralPath $UnityManagedDirectory).Path
$taskSuffix = if ($VS2026) { '-vs18' } else { '' }
if (!(Test-Path -LiteralPath "$taskRoot/vendor/unity-native/include/IUnityGraphicsD3D12.h")) {
    throw 'Unity native headers are required for this adapter. See docs/dependencies.md.'
}
Push-Location -LiteralPath $taskRoot
try {
    Invoke-Checked dotnet @('restore', 'games/unity-mono/managed/ReScaleFrame.Unity.Managed.csproj', '--locked-mode', "-p:UnityManagedDirectory=$taskManaged")
    Invoke-Checked dotnet @('build', 'games/unity-mono/managed/ReScaleFrame.Unity.Managed.csproj', '--no-restore', '-c', $Configuration, "-p:UnityManagedDirectory=$taskManaged")
    Invoke-Checked cmake @('--preset', "windows-x64$taskSuffix")
    Invoke-Checked cmake @('--build', '--preset', "windows-$($Configuration.ToLowerInvariant())$taskSuffix", '--target', 'rsf_game_unity_mono', 'rsf_orchestrator', 'rsf_proxy_dinput8')
    Invoke-Checked cargo @('build', '-p', 'rescaleframe-overlay', '--release', '--locked')
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
