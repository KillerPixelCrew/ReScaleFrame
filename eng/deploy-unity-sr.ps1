param(
    [Parameter(Mandatory)][string]$GameDirectory,
    [ValidateSet('Auto','DLSS','FSR1','FSR2','FSR3','FSR4','XeSS','Off')][string]$Backend = 'Auto',
    [ValidateRange(0,5)][int]$Quality = 1,
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
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
$taskSl = "$taskRoot/.local/streamline/sdk"
foreach ($taskLeaf in @('sl.interposer.dll','sl.common.dll','sl.dlss.dll','sl.pcl.dll','nvngx_dlss.dll','nvngx_dlss.license.txt')) {
    Add-UnityCopy "$taskSl/bin/x64/$taskLeaf" "ReScaleFrame/streamline/$taskLeaf"
}
foreach ($taskLeaf in @('license.txt','3rd-party-licenses.md')) { Add-UnityCopy "$taskSl/$taskLeaf" "ReScaleFrame/streamline/$taskLeaf" }
$taskFfx = "$taskRoot/vendor/fidelityfx/Kits/FidelityFX"
foreach ($taskLeaf in @('amd_fidelityfx_loader_dx12.dll','amd_fidelityfx_upscaler_dx12.dll')) {
    Add-UnityCopy "$taskFfx/signedbin/$taskLeaf" "ReScaleFrame/fidelityfx/$taskLeaf"
}
Add-UnityCopy "$taskFfx/docs/license.md" 'ReScaleFrame/fidelityfx/license.md'
Add-UnityCopy "$taskRoot/vendor/fidelityfx/3rdpartynotice.md" 'ReScaleFrame/fidelityfx/3rdpartynotice.md'
Add-UnityCopy "$taskRoot/vendor/xess/bin/libxess.dll" 'ReScaleFrame/xess/libxess.dll'
foreach ($taskLeaf in @('LICENSE.txt','third-party-programs.txt')) { Add-UnityCopy "$taskRoot/vendor/xess/$taskLeaf" "ReScaleFrame/xess/$taskLeaf" }
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
        [IO.File]::WriteAllLines($taskDestination, @('[UnitySR]',"Backend=$taskId","Quality=$Quality",
            'Plugin=ReScaleFrame/ReScaleFrame.Game.UnityMono.dll','Log=ReScaleFrame/unity-sr.log',
            'Streamline=ReScaleFrame/streamline','FSR2=ReScaleFrame/fidelityfx','FSR3=ReScaleFrame/fidelityfx',
            'FSR4=ReScaleFrame/fidelityfx','XeSS=ReScaleFrame/xess'), [Text.Encoding]::Unicode)
    }
    $taskFiles += @{ path=$taskCopy.path; sha256=(Get-FileHash -LiteralPath $taskDestination).Hash }
}
foreach ($taskFile in $taskOriginal) {
    $taskPath = Join-Path $taskGame $taskFile.path
    if (!(Test-Path -LiteralPath $taskPath) -or (Get-FileHash -LiteralPath $taskPath).Hash -ne $taskFile.sha256) {
        throw "An original game file changed: $taskPath"
    }
}
$taskDeployment = @{ loader='shared-rsf-shim'; carrier='version.dll'; game='drag-n-wash'; backend=$Backend;
    files=$taskFiles; original_files=$taskOriginal; original_files_unchanged=$true }
[IO.File]::WriteAllText($taskManifest, ($taskDeployment | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
Write-Output "Deployed $($taskFiles.Count) RSF files alongside the game. Verified $($taskOriginal.Count) original files unchanged. Launch normally."
