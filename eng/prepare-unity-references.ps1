param(
    [Parameter(Mandatory)][string]$UnityEditorDirectory,
    [string]$UrpReferenceDirectory,
    [string]$UrpReferenceVersion,
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskEditor = (Resolve-Path -LiteralPath $UnityEditorDirectory).Path
if (Test-Path -LiteralPath (Join-Path $taskEditor 'Editor/Unity.exe')) { $taskEditor = Join-Path $taskEditor 'Editor' }
$taskExecutable = Join-Path $taskEditor 'Unity.exe'
if (!(Test-Path -LiteralPath $taskExecutable)) { throw 'UnityEditorDirectory must contain the official Unity Editor.' }
$taskVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($taskExecutable).ProductVersion
if ($taskVersion -notlike '6000.3.14*') { throw "Expected Unity 6000.3.14f1 SDK, found $taskVersion" }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $taskRoot '.local/unity-references/6000.3.14f1' }
$taskModules = Join-Path $taskEditor 'Data/Managed/UnityEngine'
$taskCaches = Join-Path $taskEditor 'Data/Resources/PackageManager/ProjectTemplates/libcache'
if ($UrpReferenceDirectory) {
    if (!$UrpReferenceVersion) { throw 'Record the supplied URP reference version with UrpReferenceVersion.' }
    $taskScripts = (Resolve-Path -LiteralPath $UrpReferenceDirectory).Path
} else {
    $taskUrp = @(Get-ChildItem -LiteralPath $taskCaches -Recurse -File -Filter 'Unity.RenderPipelines.Universal.Runtime.dll' |
        Sort-Object FullName | Select-Object -First 1)
    if (!$taskUrp.Count) { throw 'Supply UrpReferenceDirectory with SDK URP compile references, or prepare them in an Editor project.' }
    $taskScripts = $taskUrp[0].DirectoryName
    $UrpReferenceVersion = '17.3.0'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
# Compile references only. None of these Unity SDK DLLs belong in the mod package.
Get-ChildItem -LiteralPath $taskScripts -Filter '*.dll' -File | Copy-Item -Destination $OutputDirectory
# Keep engine modules from the pinned SDK even when the URP cache contains engine DLLs.
Get-ChildItem -LiteralPath $taskModules -Filter '*.dll' -File | Copy-Item -Destination $OutputDirectory
$taskRequired = @('UnityEngine.CoreModule.dll', 'UnityEngine.SharedInternalsModule.dll',
    'Unity.RenderPipelines.Core.Runtime.dll', 'Unity.RenderPipelines.Core.Runtime.Shared.dll',
    'Unity.RenderPipelines.Universal.Runtime.dll')
$taskHashes = [ordered]@{}
foreach ($taskName in $taskRequired) {
    $taskPath = Join-Path $OutputDirectory $taskName
    if (!(Test-Path -LiteralPath $taskPath)) { throw "Missing SDK reference: $taskName" }
    $taskHashes[$taskName] = (Get-FileHash -LiteralPath $taskPath -Algorithm SHA256).Hash.ToLowerInvariant()
}
$taskManifest = [ordered]@{ source='official Unity Editor SDK and supplied URP compile references'; version='6000.3.14f1';
    revision='d68c3f99a318'; urp_reference_version=$UrpReferenceVersion;
    urp_reference_source=$taskScripts; game_files_required=$false; files=$taskHashes }
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'references.json'),
    ($taskManifest | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
Write-Output "Unity compile references: $([IO.Path]::GetFullPath($OutputDirectory))"
