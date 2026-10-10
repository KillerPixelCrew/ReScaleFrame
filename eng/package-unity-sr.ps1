param(
    [Parameter(Mandatory)][string]$GameDirectory,
    [ValidateSet('Auto','DLSS','FSR1','FSR2','FSR3','FSR4','XeSS','Off')][string]$Backend = 'Auto',
    [ValidateSet('Auto','Off','DLSS','FSR3','FSR4','XeSS')][string]$FrameGeneration = 'Auto',
    [ValidateRange(0,4)][int]$Quality = 1
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskGame = (Resolve-Path -LiteralPath $GameDirectory).Path
$taskDeployment = Get-Content -Raw -LiteralPath (Join-Path $taskGame 'ReScaleFrame/deployment.json') | ConvertFrom-Json
$taskVersion = (Get-Content -LiteralPath "$taskRoot/VERSION" -Raw).Trim()
$taskBin = Join-Path $taskRoot 'build/windows-x64/bin/Release'
$taskFirstParty = @('ReScaleFrame.Runtime.dll', 'ReScaleFrame.Game.UnityMono.dll',
    'ReScaleFrame.Unity.Managed.dll', 'rescaleframe_overlay.dll')
$taskName = "ReScaleFrame-DragNWash-$taskVersion-Claw-Test-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
$taskOutput = Join-Path $taskRoot '.local/packages'
$taskStage = Join-Path $taskOutput $taskName
$taskZip = Join-Path $taskOutput "$taskName.zip"
if ((Test-Path -LiteralPath $taskStage) -or (Test-Path -LiteralPath $taskZip)) { throw 'Package destination already exists.' }
if ($taskDeployment.game -ne 'drag-n-wash') { throw 'Expected the DragNWash deployment manifest.' }
$taskExpected = @(
    'version.dll', 'ReScaleFrame.ini',
    'ReScaleFrame/ReScaleFrame.Runtime.dll', 'ReScaleFrame/ReScaleFrame.Game.UnityMono.dll',
    'ReScaleFrame/ReScaleFrame.Unity.Managed.dll', 'ReScaleFrame/rescaleframe_overlay.dll',
    'ReScaleFrame/0Harmony.dll', 'ReScaleFrame/Harmony-LICENSE.txt',
    'ReScaleFrame/streamline/sl.interposer.dll', 'ReScaleFrame/streamline/sl.common.dll',
    'ReScaleFrame/streamline/sl.dlss.dll', 'ReScaleFrame/streamline/sl.pcl.dll',
    'ReScaleFrame/streamline/sl.dlss_g.dll', 'ReScaleFrame/streamline/sl.reflex.dll',
    'ReScaleFrame/streamline/nvngx_dlssg.dll',
    'ReScaleFrame/streamline/nvngx_dlss.dll', 'ReScaleFrame/streamline/nvngx_dlss.license.txt',
    'ReScaleFrame/streamline/license.txt', 'ReScaleFrame/streamline/3rd-party-licenses.md',
    'ReScaleFrame/fidelityfx/amd_fidelityfx_loader_dx12.dll',
    'ReScaleFrame/fidelityfx/amd_fidelityfx_upscaler_dx12.dll',
    'ReScaleFrame/fidelityfx/amd_fidelityfx_framegeneration_dx12.dll',
    'ReScaleFrame/fidelityfx/license.md', 'ReScaleFrame/fidelityfx/3rdpartynotice.md',
    'ReScaleFrame/xess/libxess.dll', 'ReScaleFrame/xess/libxess_fg.dll', 'ReScaleFrame/xess/libxell.dll',
    'ReScaleFrame/xess/LICENSE.txt', 'ReScaleFrame/xess/third-party-programs.txt'
)
$taskFiles = @($taskDeployment.files)
if ($taskFiles.Count -ne $taskExpected.Count -or @($taskFiles.path | Select-Object -Unique).Count -ne $taskExpected.Count) {
    throw 'Deployment payload is incomplete or contains duplicate files.'
}
foreach ($taskFile in $taskFiles) {
    if ($taskFile.path -notin $taskExpected) { throw "Unexpected package entry: $($taskFile.path)" }
    $taskSource = Join-Path $taskGame $taskFile.path
    if ((Get-FileHash -LiteralPath $taskSource).Hash -ne $taskFile.sha256) {
        throw "Deployed file differs from the tested manifest: $taskSource"
    }
}
New-Item -ItemType Directory -Path $taskStage -Force | Out-Null
foreach ($taskFile in $taskFiles) {
    $taskDestination = Join-Path $taskStage $taskFile.path
    New-Item -ItemType Directory -Path (Split-Path -Parent $taskDestination) -Force | Out-Null
    $taskSource = Join-Path $taskGame $taskFile.path
    $taskLeaf = [IO.Path]::GetFileName($taskFile.path)
    if ($taskFile.path -eq 'version.dll') { $taskSource = Join-Path $taskBin 'ReScaleFrame.Loader.dll' }
    elseif ($taskLeaf -in $taskFirstParty) { $taskSource = Join-Path $taskBin $taskLeaf }
    $taskSourceHash = (Get-FileHash -LiteralPath $taskSource).Hash
    Copy-Item -LiteralPath $taskSource -Destination $taskDestination
    if ((Get-FileHash -LiteralPath $taskDestination).Hash -ne $taskSourceHash) { throw "Copy verification failed: $taskDestination" }
}
$taskIni = Join-Path $taskStage 'ReScaleFrame.ini'
$taskBackendId = @{ Off=0; DLSS=1; FSR2=2; FSR3=3; FSR4=4; XeSS=5; FSR1=6; Auto=7 }[$Backend]
$taskSettings = [IO.File]::ReadAllText($taskIni)
$taskSettings = [regex]::Replace($taskSettings, '(?m)^Backend=\d+', "Backend=$taskBackendId")
$taskSettings = [regex]::Replace($taskSettings, '(?m)^Quality=\d+', "Quality=$Quality")
$taskFgId = @{ Off=0; DLSS=1; FSR3=3; FSR4=4; XeSS=5; Auto=7 }[$FrameGeneration]
$taskSettings = [regex]::Replace($taskSettings, '(?m)^FrameGeneration=\d+', "FrameGeneration=$taskFgId")
[IO.File]::WriteAllText($taskIni, $taskSettings, [Text.Encoding]::Unicode)
Copy-Item -LiteralPath "$taskRoot/LICENSE" -Destination "$taskStage/LICENSE.txt"
Copy-Item -LiteralPath "$taskRoot/build/windows-x64/_deps/minhook-src/LICENSE.txt" -Destination "$taskStage/MinHook-LICENSE.txt"
[IO.File]::WriteAllText("$taskStage/Unity-NOTICE.txt", @'
Unity Native Plugin API copyright (c) 2015 Unity Technologies ApS.
The native rendering interface declarations are licensed under the Unity Companion
License for Unity-dependent projects:
https://unity.com/legal/licenses/unity-companion-license
Reference revision: 522254181faf188efa8b50c3e3bf6fce720b26e4.
No Unity engine binaries or game assemblies are included in this package.
'@, [Text.UTF8Encoding]::new($false))
$taskReadme = @'
ReScaleFrame: Drag'n Wash DX12 test package

Install
1. Close Drag'n Wash.
2. Open Steam > Drag'n Wash > Manage > Browse local files.
3. Extract version.dll, ReScaleFrame.ini and the ReScaleFrame folder beside DragNWash.exe.
4. Start the game normally through Steam. Keep its default DX12 renderer.
5. Press Insert to open the overlay. On a handheld without Insert, use a keyboard
   or Windows' full On-Screen Keyboard (osk.exe).

Default: Auto, Quality. All vendor runtimes are included.
Auto identifies the GPU owning the game's DX12 device: NVIDIA selects DLSS,
Intel selects XeSS, AMD selects FSR3. Software adapters disable SR.
FSR4 is a manual experimental choice; RDNA4 auto-detection is not implemented.
The panel offers DLSS, FSR1, FSR2, FSR3, FSR4 and XeSS, with Native, Quality,
Balanced, Performance and Ultra Performance modes. Unsupported providers report
refusal and keep a spatial fallback. DLSS requires supported NVIDIA hardware.
FSR4 uses the experimental INT8 compatibility path on non-native hardware;
support and image quality on your Claw have not been validated yet.

This is a Unity Mono/URP DX12 test build for Steam build 25286774,
Unity 6000.3.14f1. It refuses an unrecognized executable or native player.
Temporal SR runs before DoF/blur, bloom and tone mapping. UI is drawn afterward.
This build includes independent DLSS-G, FSR3/4 and XeSS FG selection in Insert,
with live provider changes and SDK-limited multiplier controls. FG defaults to
Auto: NVIDIA selects DLSS-G, Intel selects XeSS FG and AMD selects FSR3 FG.
Software or unknown adapters select Off. Saved manual FG preferences take precedence.
The overlay accepts touch/pen contacts and Windows-promoted touch taps.
One generated frame means 2x. DLSS-G uses Reflex (effective mode at least On
while active), XeSS uses XeLL, and the pinned FSR path stays 2x. Unsupported
requests retain the working provider. FSR4 SR INT8 does not establish FSR4 FG.
Unity saves the FG provider in ReScaleFrame/preferences.ini; that file is not
included here. Other controls can remain session settings. SR Off/FSR1 can still
supply FG depth/motion inputs.

Recorded 4 October user acceptance covers XeSS/DLSS-G and the final FSR image/
switching correction. This does not validate every package, scene, resize or
hardware family. Higher DLSS/Intel MFG counts and MSI Claw acceptance are pending.
The HUD separates rendered FPS from SDK/DXGI presented counts; it does not
measure input latency or physical scanout.

No original game file is included or needs replacing. If a different mod already
owns version.dll, do not overwrite it. Remove or resolve that mod first.
Only replace files from an older RSF installation after backing them up.

Logs
ReScaleFrame/loader.log and ReScaleFrame/unity-sr.log.
When reporting a problem, include the provider, quality, output resolution,
whether the scene/camera was static, the logs and a short recording.
Compare Native, Quality and Performance at the same scene and let history settle.
Check the skyline, independent object motion, DoF, UI, settings changes and resize.
Accepted counters prove evaluation, not image quality or a performance gain.

Uninstall
Close the game, then remove the added version.dll, ReScaleFrame.ini and
ReScaleFrame folder. Leave every original game file in place.

This package contains the verified Release build and hash-verified deployed vendor
payload, including the accepted post-processing correction and new Auto policy.
MSI Claw hardware acceptance is pending. The existing game installation is not
changed by packaging.
First-party code is GPL-3.0-only; see LICENSE.txt. Vendor and Harmony notices
are included alongside their runtimes. No game assets, game assemblies,
decompiles, logs or machine-specific deployment baseline are included.
'@
[IO.File]::WriteAllText("$taskStage/README.txt", $taskReadme, [Text.UTF8Encoding]::new($false))
$taskManifest = [ordered]@{
    format = 1; version = $taskVersion; game = 'drag-n-wash'; steam_build = '25286774'
    executable_sha256 = '5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc'
    artifact_source = 'verified Release build with hash-verified deployed vendor payload'; backend = $Backend; quality = $Quality
    source_baseline = (& git -C $taskRoot rev-parse HEAD).Trim()
    source_has_uncommitted_changes = [bool](& git -C $taskRoot status --porcelain)
    frame_generation = $FrameGeneration
    claw_device_tested = $false; created_utc = [DateTime]::UtcNow.ToString('o')
}
[IO.File]::WriteAllText("$taskStage/package.json", ($taskManifest | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
$taskHashes = @{}
foreach ($taskFile in Get-ChildItem -LiteralPath $taskStage -Recurse -File) {
    $taskRelative = [IO.Path]::GetRelativePath($taskStage, $taskFile.FullName).Replace('\','/')
    $taskHashes[$taskRelative] = (Get-FileHash -LiteralPath $taskFile.FullName).Hash.ToLowerInvariant()
}
$taskChecksumLines = @($taskHashes.Keys | Sort-Object | ForEach-Object { "$($taskHashes[$_])  $_" })
[IO.File]::WriteAllLines("$taskStage/SHA256SUMS.txt", $taskChecksumLines, [Text.UTF8Encoding]::new($false))
$taskHashes['SHA256SUMS.txt'] = (Get-FileHash -LiteralPath "$taskStage/SHA256SUMS.txt").Hash.ToLowerInvariant()
[IO.Compression.ZipFile]::CreateFromDirectory($taskStage, $taskZip)
$taskArchive = [IO.Compression.ZipFile]::OpenRead($taskZip)
try {
    if ($taskArchive.Entries.Count -ne $taskHashes.Count) { throw 'Archive file count differs from staged payload.' }
    foreach ($taskEntry in $taskArchive.Entries) {
        if (!$taskHashes.ContainsKey($taskEntry.FullName)) { throw "Unexpected archive entry: $($taskEntry.FullName)" }
        $taskStream = $taskEntry.Open()
        try { $taskHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($taskStream)).ToLowerInvariant() }
        finally { $taskStream.Dispose() }
        if ($taskHash -ne $taskHashes[$taskEntry.FullName]) { throw "Archive checksum mismatch: $($taskEntry.FullName)" }
    }
}
finally { $taskArchive.Dispose() }
$taskZipHash = (Get-FileHash -LiteralPath $taskZip).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$taskZip.sha256", "$taskZipHash  $taskName.zip`n", [Text.UTF8Encoding]::new($false))
Write-Output "ZIP: $taskZip"
Write-Output "SHA256: $taskZipHash"
Write-Output "Verified $($taskHashes.Count) archive entries; default $Backend quality $Quality."
