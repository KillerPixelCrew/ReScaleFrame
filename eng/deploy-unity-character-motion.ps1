param(
    [Parameter(Mandatory)][string]$GameDirectory,
    [Parameter(Mandatory)][string]$PatchDirectory,
    [string]$ManagedHelper
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
if (!$ManagedHelper) { $ManagedHelper = Join-Path $taskRoot 'build/managed/unity/bin/Release/netstandard2.1/ReScaleFrame.Unity.Managed.dll' }
$taskGame = (Resolve-Path -LiteralPath $GameDirectory).Path
$taskPatch = (Resolve-Path -LiteralPath $PatchDirectory).Path
$taskHelper = (Resolve-Path -LiteralPath $ManagedHelper).Path
$taskManifestPath = Join-Path $taskGame 'ReScaleFrame/deployment.json'
$taskManifest = Get-Content -Raw -LiteralPath $taskManifestPath | ConvertFrom-Json
$taskReport = Get-Content -Raw -LiteralPath (Join-Path $taskPatch 'patch.json') | ConvertFrom-Json
$taskAssetRelative = 'DragNWash_Data/globalgamemanagers.assets'
$taskAsset = Join-Path $taskGame $taskAssetRelative
$taskSource = Join-Path $taskPatch 'globalgamemanagers.assets'
$taskDestination = Join-Path $taskGame 'ReScaleFrame/ReScaleFrame.Unity.Managed.dll'
if ($taskManifest.game -ne 'drag-n-wash' -or $taskReport.source_sha256 -ne '0948b788c15132fccb447a120daa4ad991ebf7878db7365757381dbb7c0ab4f0' -or
    $taskReport.shader -ne 'WalkNWashLit' -or $taskReport.pass_name -ne 'ReScaleFrameMotion' -or $taskReport.programs -ne 64 -or
    @($taskReport.changed_objects).Count -ne 1 -or $taskReport.changed_objects[0] -ne 81) { throw 'Unrecognized character shader patch.' }
if ((Get-FileHash -LiteralPath (Join-Path $taskGame 'DragNWash.exe')).Hash -ne '5FDFFFE386A2F43B77626CD3D70554D84C6588C94D309544924D6FAB088DDAFC') { throw 'Game executable fingerprint differs.' }
if ((Get-FileHash -LiteralPath $taskSource).Hash -ne $taskReport.patched_sha256) { throw 'Staged shader patch hash differs.' }
$taskCurrentAsset = (Get-FileHash -LiteralPath $taskAsset).Hash
$taskExistingPatch = @()
if ($taskManifest.PSObject.Properties.Name -contains 'game_patches') {
    $taskExistingPatch = @($taskManifest.game_patches | Where-Object { $_.path -eq $taskAssetRelative -and $_.original_sha256 -eq $taskReport.source_sha256 -and $_.patched_sha256 -eq $taskReport.patched_sha256 })
}
$taskAlreadyPatched = $taskCurrentAsset -eq $taskReport.patched_sha256 -and $taskExistingPatch.Count -eq 1
if ($taskCurrentAsset -ne $taskReport.source_sha256 -and !$taskAlreadyPatched) { throw 'Game asset differs from the expected original or recorded patch.' }
$taskOwned = @($taskManifest.files | Where-Object { $_.path.Replace('\','/') -eq 'ReScaleFrame/ReScaleFrame.Unity.Managed.dll' })
if ($taskOwned.Count -ne 1 -or (Get-FileHash -LiteralPath $taskDestination).Hash -ne $taskOwned[0].sha256) { throw 'Managed helper differs from its deployment manifest.' }
if (Get-Process DragNWash -ErrorAction SilentlyContinue) { throw 'DragNWash is running. No game files were replaced.' }
$taskBackup = Join-Path $taskGame ('ReScaleFrame/backups/character-motion-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $taskBackup -Force | Out-Null
Copy-Item -LiteralPath $taskAsset -Destination (Join-Path $taskBackup 'globalgamemanagers.assets')
Copy-Item -LiteralPath $taskDestination -Destination (Join-Path $taskBackup 'ReScaleFrame.Unity.Managed.dll')
Copy-Item -LiteralPath $taskManifestPath -Destination (Join-Path $taskBackup 'deployment.json')
try {
    if (!$taskAlreadyPatched) { Copy-Item -LiteralPath $taskSource -Destination $taskAsset }
    Copy-Item -LiteralPath $taskHelper -Destination $taskDestination
    if ((Get-FileHash -LiteralPath $taskAsset).Hash -ne $taskReport.patched_sha256 -or
        (Get-FileHash -LiteralPath $taskDestination).Hash -ne (Get-FileHash -LiteralPath $taskHelper).Hash) { throw 'Post-copy hash verification failed.' }
    $taskOwned[0].sha256 = (Get-FileHash -LiteralPath $taskDestination).Hash
    $taskOriginalBackup = if ($taskAlreadyPatched) { $taskExistingPatch[0].backup } else { Join-Path $taskBackup 'globalgamemanagers.assets' }
    $taskRecord = [PSCustomObject]@{ path=$taskAssetRelative; original_sha256=$taskReport.source_sha256;
        patched_sha256=$taskReport.patched_sha256; backup=$taskOriginalBackup; shader='WalkNWashLit'; pass='ReScaleFrameMotion' }
    $taskManifest | Add-Member -NotePropertyName game_patches -NotePropertyValue @($taskRecord) -Force
    $taskManifest.original_files_unchanged = $false
    [IO.File]::WriteAllText($taskManifestPath, ($taskManifest | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
}
catch {
    Copy-Item -LiteralPath (Join-Path $taskBackup 'globalgamemanagers.assets') -Destination $taskAsset
    Copy-Item -LiteralPath (Join-Path $taskBackup 'ReScaleFrame.Unity.Managed.dll') -Destination $taskDestination
    Copy-Item -LiteralPath (Join-Path $taskBackup 'deployment.json') -Destination $taskManifestPath
    throw
}
Write-Output "Deployed character-motion shader and managed adapter. Backup: $taskBackup"
