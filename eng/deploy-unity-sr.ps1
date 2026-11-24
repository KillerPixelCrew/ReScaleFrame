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
$taskExe = Join-Path $taskGame 'DragNWash.exe'
$taskExpected = '5FDFFFE386A2F43B77626CD3D70554D84C6588C94D309544924D6FAB088DDAFC'
if (!(Test-Path -LiteralPath $taskExe) -or (Get-FileHash -LiteralPath $taskExe).Hash -ne $taskExpected) {
    throw 'Deployment requires the researched DragNWash.exe fingerprint.'
}
if (Get-Process DragNWash -ErrorAction SilentlyContinue) { throw 'Close DragNWash before updating the added RSF files.' }
$taskOld = if (Test-Path -LiteralPath $taskManifest) { Get-Content -Raw -LiteralPath $taskManifest | ConvertFrom-Json } else { $null }
$taskOwned = @{}
if ($taskOld) { foreach ($taskFile in $taskOld.files) { $taskOwned[$taskFile.path] = $taskFile.sha256 } }
$taskOriginal = if ($taskOld) { @($taskOld.original_files) } else {
    @(Get-ChildItem -LiteralPath $taskGame -Recurse -File | ForEach-Object {
        @{ path = [IO.Path]::GetRelativePath($taskGame, $_.FullName); sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash }
    })
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
            'Plugin=ReScaleFrame/ReScaleFrame.Game.UnityMono.dll','Log=ReScaleFrame/unity-sr.log',
            'Streamline=ReScaleFrame/streamline','FSR2=ReScaleFrame/fidelityfx','FSR3=ReScaleFrame/fidelityfx',
            'FSR4=ReScaleFrame/fidelityfx','XeSS=ReScaleFrame/xess'), [Text.Encoding]::Unicode)
    }
    $taskFiles += @{ path=$taskCopy.path; sha256=(Get-FileHash -LiteralPath $taskDestination).Hash }
}
foreach ($taskFile in $taskOriginal) {
    $taskPath = Join-Path $taskGame $taskFile.path
    $taskExpectedOriginal = $taskFile.sha256
    if ($taskOld -and $taskOld.PSObject.Properties.Name -contains 'game_patches') {
        foreach ($taskPatch in $taskOld.game_patches) {
            if ($taskPatch.path.Replace('\','/') -eq $taskFile.path.Replace('\','/') -and $taskPatch.original_sha256 -eq $taskFile.sha256) {
                $taskExpectedOriginal = $taskPatch.patched_sha256
            }
        }
    }
    if (!(Test-Path -LiteralPath $taskPath) -or (Get-FileHash -LiteralPath $taskPath).Hash -ne $taskExpectedOriginal) {
        throw "An original game file changed: $taskPath"
    }
}
$taskDeployment = @{ loader='shared-rsf-shim'; carrier='version.dll'; game='drag-n-wash'; backend=$Backend;
    files=$taskFiles; original_files=$taskOriginal; original_files_unchanged=$true }
if ($taskOld -and $taskOld.PSObject.Properties.Name -contains 'game_patches') {
    $taskDeployment.game_patches = @($taskOld.game_patches)
    $taskDeployment.original_files_unchanged = @($taskOld.game_patches).Count -eq 0
}
[IO.File]::WriteAllText($taskManifest, ($taskDeployment | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
if ($taskDeployment.original_files_unchanged) {
    Write-Output "Deployed $($taskFiles.Count) RSF files. Verified $($taskOriginal.Count) original files unchanged. Launch normally."
}
else {
    Write-Output "Deployed $($taskFiles.Count) RSF files. Verified $($taskOriginal.Count) original-file hashes with $(@($taskDeployment.game_patches).Count) recorded game patches. Launch normally."
}
