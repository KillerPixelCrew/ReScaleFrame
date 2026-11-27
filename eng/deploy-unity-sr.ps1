param(
    [Parameter(Mandatory)][string]$GameDirectory,
    [ValidateSet('Auto','DLSS','FSR1','FSR2','FSR3','FSR4','XeSS','Off')][string]$Backend = 'Auto',
    [ValidateRange(0,5)][int]$Quality = 1,
    [ValidateSet('Auto','Off','DLSS','FSR3','FSR4','XeSS')][string]$FrameGeneration = 'Auto',
    [ValidateRange(1,15)][int]$GeneratedFrames = 1,
    [switch]$GraphicsDebug,
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'common.ps1')
$taskGame = (Resolve-Path -LiteralPath $GameDirectory).Path
$taskBin = "$taskRoot/build/windows-x64/bin/$Configuration"
$taskMod = Join-Path $taskGame 'ReScaleFrame'
$taskManifest = Join-Path $taskMod 'deployment.json'
# Discover the Unity Mono URP player the adapter needs (AGENTS.md Rule 4). No executable build is
# pinned: its hash is recorded as evidence. The plugin and managed helper check the rest at start-up.
$taskPlayers = @(Get-ChildItem -LiteralPath $taskGame -Filter '*.exe' -File | Where-Object {
    Test-Path -LiteralPath (Join-Path $taskGame "$($_.BaseName)_Data/Managed") -PathType Container })
if ($taskPlayers.Count -ne 1) { throw "Expected one Unity player executable beside a <name>_Data/Managed directory; found $($taskPlayers.Count)." }
$taskPlayer = $taskPlayers[0]
foreach ($taskRequired in @('UnityPlayer.dll', 'MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll',
    "$($taskPlayer.BaseName)_Data/Managed/UnityEngine.CoreModule.dll",
    "$($taskPlayer.BaseName)_Data/Managed/Unity.RenderPipelines.Core.Runtime.dll",
    "$($taskPlayer.BaseName)_Data/Managed/Unity.RenderPipelines.Universal.Runtime.dll")) {
    if (!(Test-Path -LiteralPath (Join-Path $taskGame $taskRequired) -PathType Leaf)) { throw "Not a Unity Mono URP player: $taskRequired is missing." }
}
$taskUnity = (Get-Item -LiteralPath (Join-Path $taskGame 'UnityPlayer.dll')).VersionInfo.ProductVersion
$taskExeHash = (Get-FileHash -LiteralPath $taskPlayer.FullName).Hash
Write-Output "Unity player $($taskPlayer.Name): Unity $taskUnity, executable SHA256 $taskExeHash (evidence only)."
if (Get-Process -Name $taskPlayer.BaseName -ErrorAction SilentlyContinue) { throw "Close $($taskPlayer.BaseName) before updating the added RSF files." }
$taskOld = if (Test-Path -LiteralPath $taskManifest) { Get-Content -Raw -LiteralPath $taskManifest | ConvertFrom-Json } else { $null }
$taskOwned = @{}
if ($taskOld) { foreach ($taskFile in $taskOld.files) { $taskOwned[$taskFile.path] = $taskFile.sha256 } }
# The game's own files as they are now: everything outside RSF's folder and its two top-level files.
# A game update changes them legitimately, so the baseline is retaken on every deployment; the copy
# below must leave it unchanged.
$taskPatches = @()
if ($taskOld -and $taskOld.PSObject.Properties.Name -contains 'game_patches') { $taskPatches = @($taskOld.game_patches) }
$taskOriginal = @(Get-ChildItem -LiteralPath $taskGame -Recurse -File | ForEach-Object {
    $taskPath = [IO.Path]::GetRelativePath($taskGame, $_.FullName)
    if ($taskPath -notmatch '^ReScaleFrame[\\/]' -and $taskPath -notin @('version.dll', 'ReScaleFrame.ini')) {
        @{ path = $taskPath; sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash }
    }
})
$taskCurrent = @{}
foreach ($taskFile in $taskOriginal) { $taskCurrent[$taskFile.path.Replace('\','/')] = $taskFile }
# A recorded game patch still in place keeps its original hash in the baseline; one the game
# replaced is dropped and must be patched again.
$taskPatches = @($taskPatches | Where-Object {
    $taskFile = $taskCurrent[$_.path.Replace('\','/')]
    if ($taskFile -and $taskFile.sha256 -eq $_.patched_sha256) { $taskFile.sha256 = $_.original_sha256; $true }
    else { Write-Warning "Recorded game patch no longer present (replaced by the game): $($_.path)"; $false }
})
if ($taskOld) {
    $taskPrevious = @{}
    foreach ($taskFile in $taskOld.original_files) { $taskPrevious[$taskFile.path.Replace('\','/')] = $taskFile.sha256 }
    $taskChanged = @($taskCurrent.Keys | Where-Object { $taskPrevious[$_] -ne $taskCurrent[$_].sha256 }) +
        @($taskPrevious.Keys | Where-Object { !$taskCurrent.ContainsKey($_) })
    if ($taskChanged.Count) { Write-Output "$($taskChanged.Count) game files changed since the last deployment (game update)." }
}
$taskCopies = [Collections.Generic.List[object]]::new()
function Add-UnityCopy([string]$Source, [string]$Relative) {
    if (!(Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Required artifact missing: $Source" }
    $taskCopies.Add(@{ source=$Source; path=$Relative })
}
Add-UnityCopy "$taskBin/ReScaleFrame.Loader.dll" 'version.dll'
foreach ($taskLeaf in @('ReScaleFrame.Runtime.dll','ReScaleFrame.Game.UnityMono.dll','ReScaleFrame.Unity.Managed.dll','rescaleframe_overlay.dll','0Harmony.dll','Harmony-LICENSE.txt')) {
    Add-UnityCopy "$taskBin/$taskLeaf" "ReScaleFrame/$taskLeaf"
}
foreach ($taskVendor in (Get-RsfVendorPayload $taskRoot).GetEnumerator()) { Add-UnityCopy $taskVendor.Value "ReScaleFrame/$($taskVendor.Key)" }
$taskCopies.Add(@{ source=$null; path='ReScaleFrame.ini' })
foreach ($taskCopy in $taskCopies) {
    $taskDestination = Join-Path $taskGame $taskCopy.path
    if (Test-Path -LiteralPath $taskDestination) {
        if (!$taskOwned.ContainsKey($taskCopy.path) -or (Get-FileHash -LiteralPath $taskDestination).Hash -ne $taskOwned[$taskCopy.path]) {
            throw "Refused to replace a game file or unowned/modified mod file: $taskDestination"
        }
    }
}
New-Item -ItemType Directory -Force -Path $taskMod | Out-Null
$taskBackup = Join-Path $taskMod ('backups/' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$taskFiles = @()
foreach ($taskCopy in $taskCopies) {
    $taskDestination = Join-Path $taskGame $taskCopy.path
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskDestination) | Out-Null
    if (Test-Path -LiteralPath $taskDestination) {
        $taskBackupFile = Join-Path $taskBackup $taskCopy.path
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $taskBackupFile) | Out-Null
        Copy-Item -LiteralPath $taskDestination -Destination $taskBackupFile
    }
    if ($taskCopy.source) { Copy-Item -LiteralPath $taskCopy.source -Destination $taskDestination }
    else {
        $taskId = @{ Off=0; DLSS=1; FSR2=2; FSR3=3; FSR4=4; XeSS=5; FSR1=6; Auto=7 }[$Backend]
        $taskFg = @{ Off=0; DLSS=1; FSR3=3; FSR4=4; XeSS=5; Auto=7 }[$FrameGeneration]
        [IO.File]::WriteAllLines($taskDestination, @('[Loader]',
            'EntryModule=ReScaleFrame/ReScaleFrame.Game.UnityMono.dll','EntryPoint=rsf_unity_start',
            '[UnitySR]',"Backend=$taskId","Quality=$Quality",
            "FrameGeneration=$taskFg","GeneratedFrames=$GeneratedFrames","GraphicsDebug=$([int]$GraphicsDebug.IsPresent)",
            'Plugin=ReScaleFrame/ReScaleFrame.Game.UnityMono.dll','Log=ReScaleFrame/unity-sr.log'), [Text.Encoding]::Unicode)
    }
    $taskFiles += @{ path=$taskCopy.path; sha256=(Get-FileHash -LiteralPath $taskDestination).Hash }
}
foreach ($taskFile in $taskOriginal) {
    $taskPath = Join-Path $taskGame $taskFile.path
    $taskExpectedOriginal = $taskFile.sha256
    foreach ($taskPatch in $taskPatches) {
        if ($taskPatch.path.Replace('\','/') -eq $taskFile.path.Replace('\','/') -and $taskPatch.original_sha256 -eq $taskFile.sha256) {
            $taskExpectedOriginal = $taskPatch.patched_sha256
        }
    }
    if (!(Test-Path -LiteralPath $taskPath) -or (Get-FileHash -LiteralPath $taskPath).Hash -ne $taskExpectedOriginal) {
        throw "An original game file changed: $taskPath"
    }
}
$taskDeployment = @{ loader='shared-rsf-shim'; carrier='version.dll'; game='drag-n-wash'; backend=$Backend;
    executable=$taskPlayer.Name; executable_sha256=$taskExeHash; unity_version=$taskUnity
    files=$taskFiles; original_files=$taskOriginal; original_files_unchanged=$taskPatches.Count -eq 0 }
if ($taskPatches.Count) { $taskDeployment.game_patches = $taskPatches }
[IO.File]::WriteAllText($taskManifest, ($taskDeployment | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
if ($taskDeployment.original_files_unchanged) {
    Write-Output "Deployed $($taskFiles.Count) RSF files. Verified $($taskOriginal.Count) original files unchanged. Launch normally."
}
else {
    Write-Output "Deployed $($taskFiles.Count) RSF files. Verified $($taskOriginal.Count) original-file hashes with $($taskPatches.Count) recorded game patches. Launch normally."
}
